//! Minimal HTTPS client for the three approved public registries. No
//! cookies, no credentials, no redirects, bounded body, fixed timeout.
use super::{FetchError, Registry};
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
            .user_agent(concat!("mogumogu/", env!("CARGO_PKG_VERSION"), " (local update hints)"))
            .build();
        Self { agent: config.into() }
    }
}

/// Package names are restricted so they can never alter the URL structure.
fn safe_name(name: &str) -> Option<String> {
    let ok = !name.is_empty()
        && name.len() <= 214
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'))
        && !name.contains("..");
    ok.then(|| name.replace('/', "%2F"))
}

impl Registry for HttpRegistry {
    fn latest(&self, host: &str, name: &str) -> Result<String, FetchError> {
        let encoded = safe_name(name).ok_or_else(|| FetchError::Rejected("ungültiger Paketname".into()))?;
        let (url, pointer) = match host {
            "registry.npmjs.org" => (format!("https://registry.npmjs.org/{encoded}/latest"), "/version"),
            "pypi.org" => (format!("https://pypi.org/pypi/{encoded}/json"), "/info/version"),
            "crates.io" => (format!("https://crates.io/api/v1/crates/{encoded}"), "/crate/max_stable_version"),
            _ => return Err(FetchError::Rejected("Quelle nicht unterstützt".into())),
        };
        let mut response = self.agent.get(&url).call().map_err(|e| FetchError::Offline(e.to_string()))?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(FetchError::NotFound),
            status => return Err(FetchError::Offline(format!("HTTP {status}"))),
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(HTTP_MAX_BODY)
            .read_to_string()
            .map_err(|e| FetchError::Offline(e.to_string()))?;
        let value: Value = serde_json::from_str(&body).map_err(|_| FetchError::Rejected("Antwort ungültig".into()))?;
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() < 64)
            .map(str::to_string)
            .ok_or(FetchError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn names_cannot_change_the_url() {
        assert_eq!(safe_name("@scope/pkg").as_deref(), Some("@scope%2Fpkg"));
        assert_eq!(safe_name("a?b=c"), None);
        assert_eq!(safe_name("../../x"), None);
    }
}
