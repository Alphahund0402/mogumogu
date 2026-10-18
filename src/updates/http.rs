//! Bounded, credential-free HTTPS against approved public origins.
//! Registry text remains untrusted data. No redirects, downloads or execution.
use super::{FetchError, Registry, RegistryMetadata};
use crate::domain::Lifecycle;
use crate::limits::{HTTP_MAX_BODY, HTTP_TIMEOUT};
use serde_json::Value;

#[derive(Debug)]
pub struct HttpRegistry {
    agent: ureq::Agent,
}

impl Default for HttpRegistry {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("mogumogu/", env!("CARGO_PKG_VERSION"), " (local package hints)"))
            .build();
        Self { agent: config.into() }
    }
}

fn safe_name(name: &str) -> Option<String> {
    (!name.is_empty()
        && name.len() <= 214
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'))
        && !name.contains(".."))
    .then(|| name.replace('/', "%2F"))
}
fn safe_version(version: &str) -> Option<String> {
    (!version.is_empty()
        && version.len() < 128
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '!')))
    .then(|| version.replace('+', "%2B").replace('!', "%21"))
}
fn rejected(message: &str) -> FetchError {
    FetchError::Rejected(message.into())
}

impl HttpRegistry {
    fn json(&self, url: &str, accept: &str) -> Result<Value, FetchError> {
        let mut response =
            self.agent.get(url).header("Accept", accept).call().map_err(|e| FetchError::Offline(e.to_string()))?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(FetchError::NotFound),
            status => return Err(FetchError::Offline(format!("HTTP {status}"))),
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(HTTP_MAX_BODY)
            .read_to_string()
            .map_err(|e| FetchError::Offline(e.to_string()))?;
        crate::parsers::json::parse(&text, 64).map_err(|_| rejected("Registryantwort ungültig oder zu tief"))
    }
}

impl Registry for HttpRegistry {
    fn metadata(&self, host: &str, name: &str, version: &str) -> Result<RegistryMetadata, FetchError> {
        let encoded = safe_name(name).ok_or_else(|| rejected("Ungültiger Paketname"))?;
        let encoded_version = safe_version(version).ok_or_else(|| rejected("Ungültige Paketversion"))?;
        match host {
            "registry.npmjs.org" => {
                let exact =
                    self.json(&format!("https://registry.npmjs.org/{encoded}/{encoded_version}"), "application/json")?;
                if exact["version"].as_str() != Some(version) {
                    return Err(rejected("Paketversion stimmt nicht überein"));
                }
                let latest =
                    self.json(&format!("https://registry.npmjs.org/{encoded}/latest"), "application/json").ok();
                Ok(npm_metadata(&exact, latest.as_ref()))
            }
            "pypi.org" => {
                let exact =
                    self.json(&format!("https://pypi.org/pypi/{encoded}/{encoded_version}/json"), "application/json")?;
                if exact["info"]["version"].as_str() != Some(version) {
                    return Err(rejected("Paketversion stimmt nicht überein"));
                }
                let latest = self.json(&format!("https://pypi.org/pypi/{encoded}/json"), "application/json").ok();
                let index = self
                    .json(&format!("https://pypi.org/simple/{encoded}/"), "application/vnd.pypi.simple.v1+json")
                    .ok();
                Ok(python_metadata(&exact, latest.as_ref(), index.as_ref()))
            }
            "crates.io" => {
                let value = self.json(&format!("https://crates.io/api/v1/crates/{encoded}"), "application/json")?;
                cargo_metadata(&value, version)
            }
            "api.nuget.org" => {
                let normalized =
                    nuget_version(version).ok_or_else(|| rejected("NuGet-Version nicht normalisierbar"))?;
                let leaf = self.json(
                    &format!("https://api.nuget.org/v3/registration5-gz-semver2/{encoded}/{normalized}.json"),
                    "application/json",
                )?;
                let catalog = match &leaf["catalogEntry"] {
                    Value::Object(_) => leaf["catalogEntry"].clone(),
                    Value::String(url)
                        if url.starts_with("https://api.nuget.org/v3/catalog0/data/")
                            && crate::privacy::url_host(url).as_deref() == Some("api.nuget.org") =>
                    {
                        self.json(url, "application/json")?
                    }
                    _ => return Err(rejected("NuGet-Katalogquelle nicht unterstützt")),
                };
                let versions = self
                    .json(&format!("https://api.nuget.org/v3-flatcontainer/{encoded}/index.json"), "application/json")
                    .ok();
                Ok(nuget_metadata(&catalog, versions.as_ref()))
            }
            _ => Err(rejected("Quelle nicht unterstützt")),
        }
    }
}

fn npm_metadata(exact: &Value, latest: Option<&Value>) -> RegistryMetadata {
    let reason = exact["deprecated"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    RegistryMetadata {
        latest: latest.and_then(|v| v["version"].as_str()).map(str::to_string),
        lifecycle: Lifecycle {
            deprecated: match exact.get("deprecated") {
                None | Some(Value::Null) => Some(false),
                Some(Value::String(s)) => Some(!s.is_empty()),
                _ => None,
            },
            reason,
            ..Default::default()
        },
    }
}
fn python_metadata(exact: &Value, latest: Option<&Value>, index: Option<&Value>) -> RegistryMetadata {
    let files = exact["urls"].as_array();
    let yanked = files.filter(|f| !f.is_empty()).map(|f| f.iter().all(|v| v["yanked"].as_bool() == Some(true)));
    let reason = files
        .into_iter()
        .flatten()
        .filter(|f| f["yanked"].as_bool() == Some(true))
        .find_map(|f| f["yanked_reason"].as_str().filter(|s| !s.is_empty()))
        .map(str::to_string);
    let status = index
        .and_then(|v| v["project-status"]["status"].as_str())
        .filter(|s| matches!(*s, "active" | "archived" | "deprecated" | "quarantined"))
        .map(str::to_string);
    let reason = index.and_then(|v| v["project-status"]["reason"].as_str()).map(str::to_string).or(reason);
    RegistryMetadata {
        latest: latest.and_then(|v| v["info"]["version"].as_str()).map(str::to_string),
        lifecycle: Lifecycle {
            deprecated: status.as_deref().map(|s| s == "deprecated"),
            yanked,
            project_status: status,
            reason,
            ..Default::default()
        },
    }
}
fn cargo_metadata(value: &Value, version: &str) -> Result<RegistryMetadata, FetchError> {
    let exact = value["versions"]
        .as_array()
        .and_then(|versions| versions.iter().find(|v| v["num"].as_str() == Some(version)))
        .ok_or(FetchError::NotFound)?;
    Ok(RegistryMetadata {
        latest: value["crate"]["max_stable_version"].as_str().map(str::to_string),
        lifecycle: Lifecycle { yanked: exact["yanked"].as_bool(), ..Default::default() },
    })
}
fn nuget_version(version: &str) -> Option<String> {
    let (core, pre) = version.split('+').next()?.split_once('-').unwrap_or((version.split('+').next()?, ""));
    let mut parts = core.split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok()?;
    if !(1..=4).contains(&parts.len()) {
        return None;
    }
    while parts.len() < 3 {
        parts.push(0);
    }
    if parts.len() == 4 && parts[3] == 0 {
        parts.pop();
    }
    let mut out = parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
    if !pre.is_empty() {
        out.push('-');
        out.push_str(&pre.to_ascii_lowercase());
    }
    Some(out)
}
fn nuget_metadata(catalog: &Value, versions: Option<&Value>) -> RegistryMetadata {
    let deprecation = catalog.get("deprecation").filter(|v| v.is_object());
    let reasons = deprecation
        .and_then(|v| v["reasons"].as_array())
        .map(|r| r.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "));
    let message = deprecation.and_then(|v| v["message"].as_str()).map(str::to_string);
    let replacement = deprecation.and_then(|v| v["alternatePackage"]["id"].as_str()).map(str::to_string);
    let latest = versions
        .and_then(|v| v["versions"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|v| !v.contains('-'))
        .max_by(|a, b| super::versions::ordering("nuget", a, b).unwrap_or(std::cmp::Ordering::Equal))
        .map(str::to_string);
    RegistryMetadata {
        latest,
        lifecycle: Lifecycle {
            deprecated: Some(deprecation.is_some()),
            reason: message.or(reasons),
            replacement,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_version_metadata_is_distinct_from_latest() {
        let m = npm_metadata(&json!({"deprecated":"use next","version":"1.0.0"}), Some(&json!({"version":"2.0.0"})));
        assert_eq!(m.lifecycle.deprecated, Some(true));
        assert_eq!(m.latest.as_deref(), Some("2.0.0"));
        let m=cargo_metadata(&json!({"crate":{"max_stable_version":"2.0.0"},"versions":[{"num":"1.0.0","yanked":true},{"num":"2.0.0","yanked":false}]}),"1.0.0").unwrap();
        assert_eq!(m.lifecycle.yanked, Some(true));
        assert_eq!(m.lifecycle.deprecated, None);
    }
    #[test]
    fn python_project_and_release_states_remain_separate() {
        let m = python_metadata(
            &json!({"urls":[{"yanked":true,"yanked_reason":"broken"}]}),
            None,
            Some(&json!({"project-status":{"status":"archived"}})),
        );
        assert_eq!(m.lifecycle.yanked, Some(true));
        assert_eq!(m.lifecycle.project_status.as_deref(), Some("archived"));
        let m = python_metadata(&json!({"urls":[]}), None, None);
        assert_eq!(m.lifecycle.yanked, None);
        assert_eq!(m.lifecycle.deprecated, None);
        let mut unknown = m.lifecycle;
        unknown.checked_at = Some(1);
        assert_eq!(unknown.label(), "Status unbekannt");
    }
    #[test]
    fn nuget_deprecation_and_version_normalization() {
        let m = nuget_metadata(
            &json!({"deprecation":{"reasons":["Legacy"],"alternatePackage":{"id":"Next"}}}),
            Some(&json!({"versions":["1.0.0","1.0.0.1","2.0.0-rc"]})),
        );
        assert_eq!(m.lifecycle.replacement.as_deref(), Some("Next"));
        assert_eq!(m.latest.as_deref(), Some("1.0.0.1"));
        assert_eq!(nuget_version("01.2.0.0+build").as_deref(), Some("1.2.0"));
        assert!(safe_name("../../x").is_none());
        assert!(safe_version("1/../../x").is_none());
        assert_eq!(safe_name("@scope/pkg").as_deref(), Some("@scope%2Fpkg"));
    }
}
