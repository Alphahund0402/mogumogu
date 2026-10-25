//! Helm: Chart.yaml (declared chart dependencies), Chart.lock (resolved),
//! the Helm 2 files requirements.yaml / requirements.lock beside a
//! Chart.yaml, and `charts/` (installed subcharts by file or folder name).
//! YAML is read line by line; templates and values are never rendered,
//! repository indexes are not fetched and `helm` is never started.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::lines;

pub struct Helm;

/// Well-known public chart repositories; any other host counts as private.
const PUBLIC: &[&str] = &[
    "charts.bitnami.com",
    "registry-1.docker.io",
    "charts.helm.sh",
    "kubernetes.github.io",
    "prometheus-community.github.io",
    "grafana.github.io",
    "charts.jetstack.io",
    "helm.releases.hashicorp.com",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "helm",
    name: "Helm",
    markers: &["Chart.yaml", "Chart.lock", "requirements.yaml", "requirements.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "Chart.yaml (apiVersion v1/v2)",
        "Chart.lock",
        "requirements.yaml/.lock (Helm 2)",
        "charts/ (Chart-Pakete, Ordnernamen)",
    ],
    limits: "Kein helm-Aufruf; Templates und values.yaml werden nicht gerendert, Repository-Aliase und -Indizes nicht aufgelöst; YAML-Flow-Listen und Versionen entpackter Subcharts bleiben offen.",
};

impl Adapter for Helm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let chart_beside = file.siblings.iter().any(|s| s.eq_ignore_ascii_case("Chart.yaml"));
        match file.name.to_ascii_lowercase().as_str() {
            "chart.yaml" => bounded(chart(file)),
            "chart.lock" => bounded(lock(file, true)?),
            "requirements.lock" if chart_beside => bounded(lock(file, false)?),
            "requirements.yaml" if chart_beside => bounded(requirements(file)),
            _ => Ok(Vec::new()),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let chart = siblings.iter().any(|s| s.eq_ignore_ascii_case("Chart.yaml"));
        (name == "charts" && chart).then_some(DirClaim {
            adapter: "helm",
            category: ItemCategory::Environment,
            label: "Helm-Subcharts",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "helm", "charts", rel_path)
                .detail("Mitgelieferte Subcharts; nicht gerendert"),
        ];
        for (entry, is_dir) in access.list(&[]).into_iter().take(10_000) {
            let (name, version, detail, qualifier) = if is_dir {
                if entry.starts_with('.') {
                    continue;
                }
                (entry.as_str(), "", "entpackter Subchart; Version nicht gelesen", "Ordner")
            } else {
                let Some((name, version)) = entry.strip_suffix(".tgz").and_then(split_package) else { continue };
                (name, version, "Chart-Paket in charts/", version)
            };
            let mut item = InventoryItem::new(ItemCategory::Package, "helm", name, rel_path)
                .version(version)
                .state(InstallState::Installed)
                .detail(detail);
            item.evidence.qualifier = qualifier.into();
            items.push(item);
        }
        bounded(items)
    }
}

/// `name-1.2.3.tgz` → (`name`, `1.2.3`): the first dash before a digit
/// whose remainder is a SemVer version, else the last dash before a digit.
fn split_package(stem: &str) -> Option<(&str, &str)> {
    let dashes: Vec<usize> = stem
        .match_indices('-')
        .map(|(index, _)| index)
        .filter(|&index| index > 0 && stem[index + 1..].starts_with(|c: char| c.is_ascii_digit()))
        .collect();
    let index = dashes.iter().copied().find(|&index| semver::Version::parse(&stem[index + 1..]).is_ok());
    let index = index.or(dashes.last().copied())?;
    Some((&stem[..index], &stem[index + 1..]))
}

/// Top-level scalars and the entries of the top-level `dependencies` list.
struct ChartFile<'a> {
    top: Vec<(&'a str, &'a str)>,
    dependencies: Vec<Vec<(&'a str, &'a str)>>,
    /// A top-level line that is no `key: value` mapping.
    malformed: bool,
}

impl ChartFile<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.top.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).filter(|v| !v.is_empty())
    }

    fn has(&self, key: &str) -> bool {
        self.top.iter().any(|(k, _)| *k == key)
    }
}

fn read(text: &str) -> ChartFile<'_> {
    let mut chart = ChartFile { top: Vec::new(), dependencies: Vec::new(), malformed: false };
    let (mut inside, mut indent) = (false, None);
    for entry in lines::yaml_entries(text).take(200_000) {
        if entry.indent == 0 && !entry.item {
            match entry.key {
                Some(key) => {
                    inside = key == "dependencies";
                    indent = None;
                    chart.top.push((key, entry.value));
                }
                None => chart.malformed = true,
            }
            continue;
        }
        if !inside {
            continue;
        }
        if entry.item && indent.is_none_or(|level| entry.indent <= level) {
            // A new entry; deeper list items belong to `tags` or `import-values`.
            indent = Some(entry.indent);
            chart.dependencies.push(Vec::new());
        } else if entry.item || Some(entry.indent) != indent {
            continue;
        }
        if let (Some(key), Some(dependency)) = (entry.key, chart.dependencies.last_mut())
            && dependency.len() < 64
        {
            dependency.push((key, entry.value));
        }
    }
    chart
}

fn chart(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let chart = read(file.content);
    let (Some(api), Some(name)) = (chart.get("apiVersion"), chart.get("name")) else { return Vec::new() };
    if !matches!(api, "v1" | "v2") {
        return Vec::new();
    }
    let mut detail = format!("Chart {name}");
    if let Some(version) = chart.get("version") {
        detail.push_str(&format!(" {version}"));
    }
    if let Some(app) = chart.get("appVersion") {
        detail.push_str(&format!(" · App {app}"));
    }
    if chart.get("type") == Some("library") {
        detail.push_str(" · Bibliotheks-Chart");
    }
    detail.push_str(&format!(" · apiVersion {api} · nicht gerendert"));
    let mut items = vec![manifest("helm", file, detail)];
    items.extend(dependency_items(file, &chart, InstallState::Declared));
    items
}

fn requirements(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let chart = read(file.content);
    if !chart.has("dependencies") {
        return Vec::new();
    }
    let mut items = vec![manifest("helm", file, "Helm-2-Abhängigkeiten")];
    items.extend(dependency_items(file, &chart, InstallState::Declared));
    items
}

/// Chart.lock is always Helm's; requirements.lock only beside a Chart.yaml.
fn lock(file: &FoundFile<'_>, strict: bool) -> AdapterResult {
    let chart = read(file.content);
    if !chart.has("dependencies") && !chart.has("digest") {
        return if strict { Err(format!("{} ohne dependencies und digest", file.name)) } else { Ok(Vec::new()) };
    }
    if chart.malformed {
        return Err(format!("{}: Zeile ohne Schlüssel auf oberster Ebene", file.name));
    }
    let mut detail = format!("Lockfile · {} Abhängigkeiten", chart.dependencies.len());
    let generated = chart.get("generated").and_then(|g| g.get(..10));
    if let Some(date) = generated.filter(|d| d.chars().all(|c| c.is_ascii_digit() || c == '-')) {
        detail.push_str(&format!(" · erzeugt {date}"));
    }
    let mut items = vec![manifest("helm", file, detail)];
    items.extend(dependency_items(file, &chart, InstallState::Resolved));
    Ok(items)
}

fn dependency_items(file: &FoundFile<'_>, chart: &ChartFile<'_>, state: InstallState) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for fields in chart.dependencies.iter().take(10_000) {
        let get = |key: &str| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).filter(|v| !v.is_empty());
        let Some(name) = get("name") else { continue };
        let (source, host, origin) = repository(get("repository").unwrap_or(""));
        let spec = get("version");
        let mut detail = match (state, spec) {
            (InstallState::Resolved, _) => format!("aufgelöst laut {}", file.name),
            (_, Some(spec)) => format!("Spezifikation {spec}"),
            (_, None) => "ohne Versionsangabe".to_string(),
        };
        detail.push_str(origin);
        if let Some(alias) = get("alias") {
            detail.push_str(&format!(" · Alias {alias}"));
        }
        if fields.iter().any(|(k, _)| matches!(*k, "condition" | "tags")) {
            detail.push_str(" · bedingt");
        }
        let resolved = state == InstallState::Resolved;
        let version = if resolved { spec } else { spec.filter(|s| semver::Version::parse(s).is_ok()) };
        let mut item = InventoryItem::new(ItemCategory::Package, "helm", name, file.rel_path())
            .version(version.unwrap_or_default())
            .state(state)
            .source(source, host)
            .detail(detail);
        // The same chart may appear under several aliases or versions.
        item.evidence.qualifier = if resolved { spec } else { get("alias") }.unwrap_or_default().to_string();
        items.push(item);
    }
    items
}

/// `https://…` repositories, `oci://` registries, `@alias` / `alias:name`
/// of the local Helm configuration and `file://` paths.
fn repository(repository: &str) -> (SourceClass, Option<String>, &'static str) {
    let lower = repository.to_ascii_lowercase();
    if repository.is_empty() {
        return (SourceClass::Path, None, " · ohne Repository (charts/)");
    }
    if repository.starts_with('@') || lower.starts_with("alias:") {
        return (SourceClass::Unknown, None, " · Repository-Alias (lokale Helm-Konfiguration)");
    }
    if lower.starts_with("file://") {
        return (SourceClass::Path, None, " · lokaler Pfad");
    }
    if lower.starts_with("oci://") {
        let (source, host) = classify_url(&format!("https://{}", &repository[6..]), PUBLIC);
        return (source, host, " · OCI-Registry");
    }
    let (source, host) = classify_url(repository, PUBLIC);
    (source, host, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const CHART: &str = r#"apiVersion: v2
name: shop
description: A Helm chart for Kubernetes
type: application
version: 0.3.1
appVersion: "1.16.0"
maintainers:
  - name: Platform Team
    email: platform@example.com
dependencies:
  - name: postgresql
    version: "12.x.x"
    repository: "https://charts.bitnami.com/bitnami"
    condition: postgresql.enabled
    tags:
      - database
  - name: redis
    version: 17.11.3 # pinned
    repository: oci://registry-1.docker.io/bitnamicharts
    alias: cache
    import-values:
      - child: default.data
        parent: myimports
  - name: redis
    version: ~17.0.0
    repository: oci://robot:hunter2@harbor.corp.example/charts
    alias: sessions
  - name: common
    version: 1.x.x
    repository: "@bitnami"
  - name: internal
    version: ">=1.0.0 <2.0.0"
    repository: https://deploy:s3cret@charts.corp.example/stable/
  - name: local
    version: 0.1.0
    repository: file://../local
"#;

    #[test]
    fn chart_dependencies_with_repositories_and_aliases() {
        let items = Helm.parse_file(&file("Chart.yaml", CHART)).unwrap();
        assert_eq!(items[0].detail, "Chart shop 0.3.1 · App 1.16.0 · apiVersion v2 · nicht gerendert");
        assert_eq!(items.len(), 7, "{items:#?}");
        let find = |name: &str, alias: &str| {
            items.iter().find(|i| i.name == name && i.evidence.qualifier == alias).unwrap_or_else(|| panic!("{name}"))
        };
        let postgres = find("postgresql", "");
        assert_eq!(postgres.detail, "Spezifikation 12.x.x · bedingt");
        assert_eq!(
            (postgres.source, postgres.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("charts.bitnami.com"))
        );
        assert_eq!((postgres.version.as_deref(), postgres.install_state), (None, Some(InstallState::Declared)));
        let cache = find("redis", "cache");
        assert_eq!(cache.version.as_deref(), Some("17.11.3"));
        assert_eq!(cache.detail, "Spezifikation 17.11.3 · OCI-Registry · Alias cache");
        assert_eq!(cache.source_host.as_deref(), Some("registry-1.docker.io"));
        let sessions = find("redis", "sessions");
        assert_eq!(
            (sessions.source, sessions.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("harbor.corp.example"))
        );
        let common = find("common", "");
        assert_eq!(common.source, SourceClass::Unknown);
        assert!(common.detail.contains("Repository-Alias"));
        assert_eq!(find("internal", "").source, SourceClass::PrivateRegistry);
        assert_eq!(find("local", "").source, SourceClass::Path);
        assert!(!items.iter().any(|i| i.name == "Platform Team" || i.name == "database" || i.name == "default.data"));
        let text = format!("{items:?}");
        for secret in ["hunter2", "s3cret", "robot", "bitnamicharts", "stable/"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn foreign_chart_files_are_ignored() {
        assert!(Helm.parse_file(&file("Chart.yaml", "name: x\nversion: 1\n")).unwrap().is_empty());
        assert!(Helm.parse_file(&file("Chart.yaml", "apiVersion: v3\nname: x\n")).unwrap().is_empty());
        assert!(Helm.parse_file(&file("chart.yaml", "title: Sales\nseries:\n  - q1\n")).unwrap().is_empty());
        let helm2 = "dependencies:\n- name: mysql\n  version: 0.3.5\n  repository: https://charts.helm.sh/stable\n";
        assert!(Helm.parse_file(&file("requirements.yaml", helm2)).unwrap().is_empty(), "only beside a Chart.yaml");
        let rye = "# generated by rye\nrequests==2.31.0\n";
        let siblings = ["Chart.yaml".to_string()];
        let lock = FoundFile { name: "requirements.lock", rel_dir: "", content: rye, siblings: &siblings };
        assert!(Helm.parse_file(&lock).unwrap().is_empty());
        let requirements = FoundFile { name: "requirements.yaml", rel_dir: "", content: helm2, siblings: &siblings };
        let items = Helm.parse_file(&requirements).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[1].name.as_str(), items[1].version.as_deref()), ("mysql", Some("0.3.5")));
        assert_eq!(items[1].source, SourceClass::PublicRegistry);
    }

    #[test]
    fn lockfiles_are_resolved_and_broken_ones_are_errors() {
        let lock = "dependencies:\n- name: postgresql\n  repository: https://charts.bitnami.com/bitnami\n  version: 12.12.10\n- name: redis\n  repository: oci://registry-1.docker.io/bitnamicharts\n  version: 17.11.3\n- name: redis\n  repository: oci://registry-1.docker.io/bitnamicharts\n  version: 17.0.1\ndigest: sha256:3f4fd8a0c5b2d7f0c0d6b9c1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1\ngenerated: \"2024-03-07T11:43:27.193852+01:00\"\n";
        let items = Helm.parse_file(&file("Chart.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · 3 Abhängigkeiten · erzeugt 2024-03-07");
        let postgres = items.iter().find(|i| i.name == "postgresql").unwrap();
        assert_eq!(
            (postgres.version.as_deref(), postgres.install_state),
            (Some("12.12.10"), Some(InstallState::Resolved))
        );
        assert_eq!(postgres.detail, "aufgelöst laut Chart.lock");
        let redis: Vec<_> = items.iter().filter(|i| i.name == "redis").collect();
        assert_eq!(redis.len(), 2);
        assert_ne!(redis[0].key(), redis[1].key());
        assert!(!format!("{items:?}").contains("sha256"));
        assert!(Helm.parse_file(&file("Chart.lock", "{not yaml")).is_err());
        assert!(Helm.parse_file(&file("Chart.lock", "dependencies:\n- name: x\n{broken\n")).is_err());
    }

    #[test]
    fn subcharts_beside_a_chart() {
        assert!(Helm.claim_dir("charts", &["Chart.yaml".into()], &[]).is_some());
        assert!(Helm.claim_dir("charts", &["README.md".into()], &[]).is_none());
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            String::new(),
            vec![
                ("postgresql-12.12.10.tgz".into(), false),
                ("my-chart-1.0.0-rc.1.tgz".into(), false),
                ("app-2fa-1.0.0-1.tgz".into(), false),
                ("vendored".into(), true),
                ("notes.txt".into(), false),
            ],
        );
        let claim = Helm.claim_dir("charts", &["Chart.yaml".into()], &[]).unwrap();
        let items = Helm.inspect_dir(claim, "deploy/charts", &mut dir).unwrap();
        let installed: Vec<_> = items.iter().skip(1).map(|i| (i.name.as_str(), i.version.as_deref())).collect();
        assert_eq!(
            installed,
            [
                ("postgresql", Some("12.12.10")),
                ("my-chart", Some("1.0.0-rc.1")),
                ("app-2fa", Some("1.0.0-1")),
                ("vendored", None)
            ]
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
        assert_eq!(split_package("chart-v1"), None);
    }
}
