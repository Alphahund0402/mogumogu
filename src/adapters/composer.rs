//! Composer (PHP): composer.json (declared), composer.lock (resolved) and
//! vendor/composer/installed.json (installed). Platform requirements such
//! as php, ext-* and lib-* are no packages and are only counted. Neither
//! Composer nor PHP is started: scripts and plugins never run, auth.json and
//! the global configuration are not read, and vendor/ is not walked.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::json;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub struct Composer;

const PUBLIC: &[&str] = &[
    "packagist.org",
    "repo.packagist.org",
    "api.github.com",
    "github.com",
    "codeload.github.com",
    "gitlab.com",
    "bitbucket.org",
];
/// Repository and source types that check out a version-control repository.
const VCS: &[&str] =
    &["vcs", "git", "github", "gitlab", "bitbucket", "git-bitbucket", "hg", "fossil", "perforce", "svn"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "composer",
    name: "Composer",
    markers: &["composer.json", "composer.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "composer.json (require, require-dev, repositories)",
        "composer.lock",
        "vendor/composer/installed.json (Composer 1 und 2)",
    ],
    limits: "Kein Composer- oder PHP-Aufruf; Skripte und Plugins werden nicht ausgeführt, auth.json und globale Konfiguration nicht gelesen; Plattformanforderungen (php, ext-*, lib-*) sind keine Pakete.",
};

impl Adapter for Composer {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
        if file.name.eq_ignore_ascii_case("composer.lock") {
            return bounded(lock_items(file, &value));
        }
        bounded(manifest_items(file, &value))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let composer = siblings.iter().any(|s| s == "composer.json" || s == "composer.lock");
        (name == "vendor" && composer).then_some(DirClaim {
            adapter: "composer",
            category: ItemCategory::Environment,
            label: "vendor (Composer)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "composer", "vendor", rel_path)
                .detail("Installierte PHP-Pakete; nicht ausgeführt"),
        ];
        match access.read_text(&["composer", "installed.json"], PACKAGE_FILE_MAX_BYTES) {
            Some(text) => {
                let value = json::parse(&text, 64).map_err(|e| e.to_string())?;
                // Composer 2: {"packages": […], "dev-package-names": […]}; Composer 1: a plain list.
                let dev: BTreeSet<&str> =
                    value["dev-package-names"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let rel = format!("{rel_path}/composer/installed.json");
                let packages = value["packages"].as_array().or_else(|| value.as_array());
                for entry in packages.into_iter().flatten().take(50_000) {
                    let context =
                        if entry["name"].as_str().is_some_and(|name| dev.contains(name)) { "dev" } else { "" };
                    items.extend(package_item(entry, &rel, InstallState::Installed, context));
                }
            }
            None => {
                for (vendor, is_dir) in access.list(&[]) {
                    if !is_dir || vendor.starts_with('.') || vendor == "bin" || vendor == "composer" {
                        continue;
                    }
                    for (package, is_dir) in access.list(&[&vendor]) {
                        if is_dir && !package.starts_with('.') {
                            items.push(
                                InventoryItem::new(
                                    ItemCategory::Package,
                                    "composer",
                                    format!("{vendor}/{package}"),
                                    rel_path,
                                )
                                .state(InstallState::Installed)
                                .detail("Ordner vorhanden; Version ohne installed.json unbekannt"),
                            );
                        }
                    }
                }
            }
        }
        bounded(items)
    }
}

/// Platform packages (php, php-64bit, hhvm, ext-*, lib-*, composer,
/// composer-plugin-api, composer-runtime-api) have no vendor prefix; every
/// installable package has one.
fn is_platform(name: &str) -> bool {
    !name.contains('/')
}

fn manifest_items(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let repositories = Repositories::read(&value["repositories"]);
    let mut items = Vec::new();
    let mut edges = Vec::new();
    let mut platform = 0;
    for (section, context) in [("require", ""), ("require-dev", "dev")] {
        for (name, spec) in value[section].as_object().into_iter().flatten().take(10_000) {
            if is_platform(name) {
                platform += 1;
                continue;
            }
            let (source, host) = repositories.source(name);
            let dev = if context.is_empty() { "" } else { " · dev" };
            let mut item = InventoryItem::new(ItemCategory::Package, "composer", name.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .source(source, host)
                .detail(format!("Spezifikation {}{dev}", spec.as_str().unwrap_or("?")));
            item.evidence.context = context.into();
            items.push(item);
        }
        edges.extend(requirements(&value[section], section));
    }
    let name = value["name"].as_str().unwrap_or("ohne Namen");
    let mut detail = format!("Projekt {name} · {} Abhängigkeiten", items.len());
    if platform > 0 {
        detail.push_str(&format!(" · {platform} Plattformanforderungen"));
    }
    let scripts = value["scripts"].as_object();
    if scripts.is_some_and(|s| !s.is_empty()) {
        detail.push_str(" · Skripte vorhanden (nicht ausgeführt)");
    }
    detail.push_str(&repositories.summary());
    let mut head = manifest("composer", file, detail);
    head.evidence.dependencies = edges;
    // `@script` references and `Class::method` callbacks are no command names.
    let mut commands: Vec<String> = scripts
        .into_iter()
        .flatten()
        .flat_map(|(_, script)| match script {
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            other => other.as_str().into_iter().collect::<Vec<_>>(),
        })
        .flat_map(super::commands::names)
        .collect();
    commands.sort();
    commands.dedup();
    commands.truncate(50);
    head.evidence.commands = commands;
    items.insert(0, head);
    items
}

fn lock_items(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let rel = file.rel_path();
    let mut items = Vec::new();
    let mut counts = [0; 2];
    for (index, (key, context)) in [("packages", ""), ("packages-dev", "dev")].into_iter().enumerate() {
        for entry in value[key].as_array().into_iter().flatten().take(50_000) {
            if let Some(item) = package_item(entry, &rel, InstallState::Resolved, context) {
                counts[index] += 1;
                items.push(item);
            }
        }
    }
    let platform: usize =
        ["platform", "platform-dev"].iter().map(|key| value[*key].as_object().map_or(0, Map::len)).sum();
    let mut detail = format!("Lockfile · {} Pakete · {} dev", counts[0], counts[1]);
    if platform > 0 {
        detail.push_str(&format!(" · {platform} Plattformanforderungen"));
    }
    items.insert(0, manifest("composer", file, detail));
    items
}

/// One package of composer.lock or installed.json.
fn package_item(entry: &Value, rel_path: &str, state: InstallState, context: &str) -> Option<InventoryItem> {
    let name = entry["name"].as_str().filter(|name| !is_platform(name))?;
    let mut detail = match state {
        InstallState::Installed => "installiert laut installed.json",
        _ => "aufgelöst laut composer.lock",
    }
    .to_string();
    if !context.is_empty() {
        detail.push_str(" · dev");
    }
    if entry["type"].as_str() == Some("composer-plugin") {
        detail.push_str(" · Composer-Plugin (nicht ausgeführt)");
    }
    let (source, host) = package_source(entry);
    let mut item = InventoryItem::new(ItemCategory::Package, "composer", name, rel_path)
        .version(entry["version"].as_str().unwrap_or_default())
        .state(state)
        .source(source, host)
        .detail(detail);
    item.evidence.context = context.into();
    item.evidence.dependencies = requirements(&entry["require"], "require");
    item.evidence.deprecated = match &entry["abandoned"] {
        Value::Bool(true) => Some("aufgegeben".into()),
        Value::String(replacement) if !replacement.is_empty() => Some(format!("aufgegeben, Ersatz {replacement}")),
        _ => None,
    };
    item.evidence.executables = entry["bin"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(super::commands::basename)
        .take(50)
        .collect();
    Some(item)
}

/// Package requirements as edges; platform requirements are skipped.
fn requirements(map: &Value, kind: &str) -> Vec<PackageDependency> {
    map.as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| !is_platform(name))
        .take(10_000)
        .map(|(name, spec)| PackageDependency {
            name: name.clone(),
            requirement: spec.as_str().unwrap_or("").into(),
            kind: kind.into(),
            exact: false,
        })
        .collect()
}

/// Packagist by its download notification URL, path repositories as local,
/// otherwise the download host or a version-control checkout. Credentials
/// and paths of URLs are dropped by `classify_url`.
fn package_source(entry: &Value) -> (SourceClass, Option<String>) {
    let (dist, source) = (&entry["dist"], &entry["source"]);
    if dist["type"].as_str() == Some("path") || source["type"].as_str() == Some("path") {
        return (SourceClass::Path, None);
    }
    if entry["notification-url"].as_str().and_then(crate::privacy::url_host).as_deref() == Some("packagist.org") {
        return (SourceClass::PublicRegistry, Some("packagist.org".into()));
    }
    match dist["url"].as_str() {
        Some(url) if url.contains("://") => classify_url(url, PUBLIC),
        Some(_) => (SourceClass::Path, None), // an archive of an artifact repository
        None if source["type"].as_str().is_some_and(|kind| VCS.contains(&kind)) => (SourceClass::Git, None),
        None => (SourceClass::Unknown, None),
    }
}

fn disables_packagist(map: &Map<String, Value>) -> bool {
    map.iter().any(|(key, value)| matches!(key.as_str(), "packagist.org" | "packagist") && *value == Value::Bool(false))
}

/// What composer.json's `repositories` say about package sources.
#[derive(Default)]
struct Repositories {
    /// Composer repositories outside the public list, with their hosts.
    private: usize,
    hosts: Vec<String>,
    public: usize,
    vcs: usize,
    local: usize,
    definitions: usize,
    other: usize,
    /// Packages defined inline by `package` repositories, with their source.
    inline: Vec<(String, (SourceClass, Option<String>))>,
    packagist_off: bool,
}

impl Repositories {
    /// The array form or the object form keyed by repository name.
    fn read(value: &Value) -> Self {
        let mut repositories = Self::default();
        let entries: Vec<&Value> = match value {
            Value::Array(list) => list.iter().take(1_000).collect(),
            Value::Object(map) => {
                repositories.packagist_off = disables_packagist(map);
                map.values().take(1_000).collect()
            }
            _ => Vec::new(),
        };
        for entry in entries {
            let Some(map) = entry.as_object() else { continue };
            if disables_packagist(map) {
                repositories.packagist_off = true;
                continue;
            }
            match entry["type"].as_str().unwrap_or("") {
                "composer" => match classify_url(entry["url"].as_str().unwrap_or(""), PUBLIC) {
                    (SourceClass::PublicRegistry, _) => repositories.public += 1,
                    (_, host) => {
                        repositories.private += 1;
                        repositories.hosts.extend(host);
                    }
                },
                "path" | "artifact" => repositories.local += 1,
                "package" => {
                    repositories.definitions += 1;
                    let packages: Vec<&Value> = match &entry["package"] {
                        Value::Array(list) => list.iter().take(1_000).collect(),
                        single => vec![single],
                    };
                    for package in packages {
                        // Each declared package searches this list.
                        if let Some(name) = package["name"].as_str().filter(|_| repositories.inline.len() < 1_000) {
                            repositories.inline.push((name.to_ascii_lowercase(), package_source(package)));
                        }
                    }
                }
                kind if VCS.contains(&kind) => repositories.vcs += 1,
                _ => repositories.other += 1,
            }
        }
        repositories
    }

    /// A declared package's source: an inline definition, otherwise private
    /// when a private Composer repository may provide it, else unknown.
    fn source(&self, name: &str) -> (SourceClass, Option<String>) {
        if let Some((_, source)) = self.inline.iter().find(|(inline, _)| inline.eq_ignore_ascii_case(name)) {
            return source.clone();
        }
        let class = if self.private > 0 { SourceClass::PrivateRegistry } else { SourceClass::Unknown };
        (class, None)
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.private > 0 {
            parts.push(match self.hosts.first() {
                Some(host) => format!("{} privat ({host})", self.private),
                None => format!("{} privat", self.private),
            });
        }
        for (count, label) in [
            (self.public, "öffentlich"),
            (self.vcs, "VCS"),
            (self.local, "lokal"),
            (self.definitions, "inline"),
            (self.other, "sonstige"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {label}"));
            }
        }
        if self.packagist_off {
            parts.push("ohne Packagist".into());
        }
        if parts.is_empty() { String::new() } else { format!(" · Repositories: {}", parts.join(", ")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    fn find<'a>(items: &'a [InventoryItem], name: &str) -> &'a InventoryItem {
        items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} missing: {items:#?}"))
    }

    const MANIFEST: &str = r#"{
        "name": "acme/shop",
        "type": "project",
        "require": {
            "php": "^8.2",
            "php-64bit": "*",
            "ext-json": "*",
            "lib-icu": ">=70",
            "composer-runtime-api": "^2.2",
            "monolog/monolog": "^3.5",
            "acme/billing": "dev-main",
            "smarty/smarty": "3.1.7"
        },
        "require-dev": { "phpunit/phpunit": "^11.0", "monolog/monolog": "^3.6" },
        "repositories": [
            { "type": "vcs", "url": "git@github.com:acme/billing.git" },
            { "type": "composer", "url": "https://deploy:s3cr3tpw@repo.acme.example" },
            { "type": "path", "url": "../packages/*", "options": { "symlink": true } },
            { "type": "package", "package": {
                "name": "smarty/smarty", "version": "3.1.7",
                "dist": { "url": "https://www.smarty.net/files/Smarty-3.1.7.zip", "type": "zip" }
            } },
            { "packagist.org": false }
        ],
        "scripts": {
            "test": "phpunit --colors=always",
            "post-autoload-dump": ["Illuminate\\Foundation\\ComposerScripts::postAutoloadDump", "@php artisan package:discover"]
        }
    }"#;

    #[test]
    fn manifest_skips_platform_requirements_and_reads_repositories() {
        let items = Composer.parse_file(&file("composer.json", MANIFEST)).unwrap();
        let head = &items[0];
        assert_eq!(
            head.detail,
            "Projekt acme/shop · 5 Abhängigkeiten · 5 Plattformanforderungen · Skripte vorhanden (nicht ausgeführt) · Repositories: 1 privat (repo.acme.example), 1 VCS, 1 lokal, 1 inline, ohne Packagist"
        );
        assert_eq!(head.evidence.commands, ["phpunit"]);
        assert_eq!(head.evidence.dependencies.len(), 5, "platform requirements are no edges");
        assert!(!items.iter().any(|i| ["php", "php-64bit", "ext-json", "lib-icu"].contains(&i.name.as_str())));

        let monolog = items.iter().find(|i| i.name == "monolog/monolog" && i.evidence.context.is_empty()).unwrap();
        assert_eq!(monolog.install_state, Some(InstallState::Declared));
        assert_eq!(monolog.source, SourceClass::PrivateRegistry, "a private repository may provide it");
        assert_eq!(monolog.detail, "Spezifikation ^3.5");
        let dev = items.iter().find(|i| i.name == "monolog/monolog" && i.evidence.context == "dev").unwrap();
        assert_eq!(dev.detail, "Spezifikation ^3.6 · dev");
        assert_eq!(find(&items, "phpunit/phpunit").evidence.context, "dev");
        let smarty = find(&items, "smarty/smarty");
        assert_eq!(
            (smarty.source, smarty.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("www.smarty.net"))
        );
        assert!(!format!("{items:?}").contains("s3cr3tpw") && !format!("{items:?}").contains("deploy"));

        let plain = r#"{"require": {"php": ">=8.1", "symfony/console": "^7.0"}}"#;
        let items = Composer.parse_file(&file("composer.json", plain)).unwrap();
        assert_eq!(items[0].detail, "Projekt ohne Namen · 1 Abhängigkeiten · 1 Plattformanforderungen");
        assert_eq!(find(&items, "symfony/console").source, SourceClass::Unknown, "global config is not read");
        assert!(Composer.parse_file(&file("composer.json", "{\"require\": {")).is_err());
    }

    const LOCK: &str = r#"{
        "_readme": ["This file locks the dependencies of your project to a known state"],
        "content-hash": "e3b0c44298fc1c149afbf4c8996fb924",
        "packages": [
            {
                "name": "monolog/monolog",
                "version": "3.5.0",
                "source": { "type": "git", "url": "https://github.com/Seldaek/monolog.git", "reference": "c915e2634718dbc8a4a15c61b0e62e7a44e14448" },
                "dist": { "type": "zip", "url": "https://api.github.com/repos/Seldaek/monolog/zipball/c915e2634718dbc8a4a15c61b0e62e7a44e14448", "reference": "c915e2634718dbc8a4a15c61b0e62e7a44e14448", "shasum": "" },
                "require": { "php": ">=8.1", "psr/log": "^2.0 || ^3.0" },
                "type": "library",
                "notification-url": "https://packagist.org/downloads/"
            },
            {
                "name": "acme/billing",
                "version": "dev-main",
                "source": { "type": "git", "url": "git@github.com:acme/billing.git", "reference": "0123456789abcdef0123456789abcdef01234567" },
                "type": "library"
            },
            {
                "name": "acme/shared",
                "version": "1.0.0",
                "dist": { "type": "path", "url": "../packages/shared", "reference": "abc" },
                "transport-options": { "symlink": true, "relative": true }
            },
            {
                "name": "acme/internal",
                "version": "2.1.0",
                "dist": { "type": "zip", "url": "https://ci:s3cr3tpw@satis.acme.example/dist/acme-internal-2.1.0.zip" }
            },
            {
                "name": "swiftmailer/swiftmailer",
                "version": "v6.3.0",
                "abandoned": "symfony/mailer",
                "notification-url": "https://packagist.org/downloads/"
            },
            {
                "name": "acme/composer-plugin",
                "version": "1.0.0",
                "type": "composer-plugin",
                "dist": { "type": "zip", "url": "local-artifacts/plugin-1.0.0.zip" }
            }
        ],
        "packages-dev": [
            {
                "name": "phpunit/phpunit",
                "version": "11.0.3",
                "bin": ["phpunit"],
                "require": { "ext-dom": "*", "sebastian/diff": "^6.0" },
                "notification-url": "https://packagist.org/downloads/"
            }
        ],
        "aliases": [],
        "minimum-stability": "stable",
        "platform": { "php": "^8.2", "ext-json": "*" },
        "platform-dev": [],
        "plugin-api-version": "2.6.0"
    }"#;

    #[test]
    fn lockfile_sources_dev_packages_and_abandoned() {
        let items = Composer.parse_file(&file("composer.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · 6 Pakete · 1 dev · 2 Plattformanforderungen");
        let monolog = find(&items, "monolog/monolog");
        assert_eq!((monolog.version.as_deref(), monolog.install_state), (Some("3.5.0"), Some(InstallState::Resolved)));
        assert_eq!(
            (monolog.source, monolog.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("packagist.org"))
        );
        let edges: Vec<&str> = monolog.evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(edges, ["psr/log"], "php is a platform requirement");
        assert_eq!(find(&items, "acme/billing").source, SourceClass::Git);
        assert_eq!(find(&items, "acme/shared").source, SourceClass::Path);
        let internal = find(&items, "acme/internal");
        assert_eq!(
            (internal.source, internal.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("satis.acme.example"))
        );
        let swift = find(&items, "swiftmailer/swiftmailer");
        assert_eq!(swift.evidence.deprecated.as_deref(), Some("aufgegeben, Ersatz symfony/mailer"));
        assert_eq!(swift.version.as_deref(), Some("v6.3.0"));
        let plugin = find(&items, "acme/composer-plugin");
        assert!(plugin.source == SourceClass::Path && plugin.detail.ends_with("Composer-Plugin (nicht ausgeführt)"));
        let phpunit = find(&items, "phpunit/phpunit");
        assert_eq!(
            (phpunit.evidence.context.as_str(), phpunit.detail.as_str()),
            ("dev", "aufgelöst laut composer.lock · dev")
        );
        assert_eq!(phpunit.evidence.executables, ["phpunit"]);
        let all = format!("{items:?}");
        for hidden in ["s3cr3tpw", "c915e2634718", "0123456789abcdef", "../packages", "local-artifacts", "Seldaek"] {
            assert!(!all.contains(hidden), "{hidden} must not be stored");
        }
        assert!(Composer.parse_file(&file("composer.lock", "{\"packages\": [")).is_err());
    }

    #[test]
    fn vendor_installed_json_of_composer_2_and_1_and_folder_names() {
        assert!(Composer.claim_dir("vendor", &["composer.json".into()], &[]).is_some());
        assert!(Composer.claim_dir("vendor", &["composer.lock".into()], &[]).is_some());
        assert!(Composer.claim_dir("vendor", &["go.mod".into()], &[]).is_none());
        let claim = Composer.claim_dir("vendor", &["composer.json".into()], &[]).unwrap();

        let mut dir = FakeDir::default();
        dir.files.insert(
            "composer/installed.json".into(),
            r#"{"packages": [
                {"name": "psr/log", "version": "3.0.0", "version_normalized": "3.0.0.0", "install-path": "../psr/log",
                 "notification-url": "https://packagist.org/downloads/"},
                {"name": "phpunit/phpunit", "version": "11.0.3", "bin": ["phpunit"], "install-path": "../phpunit/phpunit"}
            ], "dev": true, "dev-package-names": ["phpunit/phpunit"]}"#
                .into(),
        );
        let items = Composer.inspect_dir(claim, "vendor", &mut dir).unwrap();
        assert_eq!(items.len(), 3);
        let log = find(&items, "psr/log");
        assert_eq!((log.install_state, log.version.as_deref()), (Some(InstallState::Installed), Some("3.0.0")));
        assert_eq!((log.rel_path.as_str(), log.evidence.context.as_str()), ("vendor/composer/installed.json", ""));
        assert_eq!(find(&items, "phpunit/phpunit").evidence.context, "dev");
        assert!(!format!("{items:?}").contains("../psr"), "install paths are not stored");

        let mut dir = FakeDir::default();
        dir.files.insert(
            "composer/installed.json".into(),
            r#"[{"name": "symfony/yaml", "version": "v2.8.52", "installation-source": "dist"}]"#.into(),
        );
        let items = Composer.inspect_dir(claim, "vendor", &mut dir).unwrap();
        assert_eq!(find(&items, "symfony/yaml").version.as_deref(), Some("v2.8.52"));

        let mut dir = FakeDir::default();
        dir.dirs.insert(
            String::new(),
            vec![("bin".into(), true), ("composer".into(), true), ("psr".into(), true), ("autoload.php".into(), false)],
        );
        dir.dirs
            .insert("psr".into(), vec![("log".into(), true), ("container".into(), true), ("README.md".into(), false)]);
        let items = Composer.inspect_dir(claim, "vendor", &mut dir).unwrap();
        let names: Vec<&str> = items.iter().skip(1).map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["psr/log", "psr/container"]);
        assert!(items[1].version.is_none() && items[1].detail.contains("unbekannt"));

        let mut dir = FakeDir::default();
        dir.files.insert("composer/installed.json".into(), "{\"packages\": [".into());
        assert!(Composer.inspect_dir(claim, "vendor", &mut dir).is_err());
    }
}
