//! CocoaPods: Podfile (declared; Ruby matched as text, never run),
//! Podfile.lock (resolved, with spec repos, external sources and edges) and
//! Pods/Manifest.lock beside them (installed, same format). Subspecs such
//! as `Firebase/Analytics` count for their root pod. Conditions, loaded
//! files and hooks stay unknown; a pod in a helper method is attributed to
//! the method, not to the targets calling it. Spec repos and podspecs are
//! not read.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::lines;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct CocoaPods;

/// Host of the trunk spec repo, also for the legacy master repo on GitHub.
const TRUNK: &str = "cdn.cocoapods.org";

static DESCRIPTOR: Descriptor = Descriptor {
    id: "cocoapods",
    name: "CocoaPods",
    markers: &["Podfile", "Podfile.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "Podfile: pod, target, source als Text",
        "Podfile.lock (PODS, DEPENDENCIES, SPEC REPOS, EXTERNAL SOURCES, CHECKOUT OPTIONS)",
        "Pods/Manifest.lock",
    ],
    limits: "Kein Ruby- oder pod-Aufruf; das Podfile wird nicht ausgeführt – Bedingungen, geladene Dateien und Hooks bleiben unbekannt, Pods in Methoden zählen zur Methode statt zu den Targets; Subspecs zählen zum Haupt-Pod; Spec-Repos und Podspecs werden nicht gelesen.",
};

static POD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^pod\s*\(?\s*['"]([^'"]+)['"]\s*(.*)$"#).expect("valid regex"));
static SOURCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^source\s*\(?\s*['"]([^'"]+)['"]"#).expect("valid regex"));
static PLATFORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^platform\s*\(?\s*:(\w+)(?:\s*,\s*['"]([^'"]+)['"])?"#).expect("valid regex"));
/// `target 'App' do`, `abstract_target 'Shared' do` or `target :App do`.
static TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(?:abstract_)?target\s*\(?\s*(?:['"]([^'"]+)['"]|:(\w+))"#).expect("valid regex"));
static METHOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^def\s+(?:self\.)?(\w+[?!]?)").expect("valid regex"));
/// A line that opens a block closed by `end`.
static OPENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:if|unless|while|until|case|begin|for|def|class|module)\b|\bdo\s*(?:\|[^|]*\|)?$")
        .expect("valid regex")
});
static END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^end\b").expect("valid regex"));
static HOOK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:pre_install|post_install|pre_integrate|post_integrate)\b").expect("valid regex"));
/// `:git => …` or `git: …` inside a `pod` call.
static OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(?::(\w+)\s*=>|['"](\w+)['"]\s*=>|(\w+):)\s*(.*)$"#).expect("valid regex"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"]([^'"]*)['"]"#).expect("valid regex"));

impl Adapter for CocoaPods {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if !file.name.eq_ignore_ascii_case("Podfile.lock") {
            return bounded(podfile(file));
        }
        let lock = parse_lock(file.content).ok_or("Podfile.lock ohne CocoaPods-Abschnitte")?;
        let mut detail = format!("Lockfile · {} Pods", lock.pods.len());
        if let Some(version) = &lock.cocoapods {
            detail.push_str(&format!(" · CocoaPods {version}"));
        }
        let mut items = vec![manifest("cocoapods", file, detail)];
        items[0].evidence.dependencies = lock.direct.clone();
        items.extend(lock_items(lock, &file.rel_path(), InstallState::Resolved, "aufgelöst laut Podfile.lock"));
        bounded(items)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let podfile = siblings.iter().any(|s| s == "Podfile" || s == "Podfile.lock");
        (name == "Pods" && podfile).then_some(DirClaim {
            adapter: "cocoapods",
            category: ItemCategory::Environment,
            label: "Pods",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let environment = |detail: String| {
            InventoryItem::new(ItemCategory::Environment, "cocoapods", "Pods", rel_path).detail(detail)
        };
        let Some(text) = access.read_text(&["Manifest.lock"], PACKAGE_FILE_MAX_BYTES) else {
            return Ok(vec![environment("Installierte Pods; ohne Manifest.lock Bestand unbekannt".into())]);
        };
        let lock = parse_lock(&text).ok_or("Pods/Manifest.lock ohne CocoaPods-Abschnitte")?;
        let mut items =
            vec![environment(format!("{} installierte Pods laut Manifest.lock; nicht ausgeführt", lock.pods.len()))];
        let rel = format!("{rel_path}/Manifest.lock");
        items.extend(lock_items(lock, &rel, InstallState::Installed, "installiert laut Pods/Manifest.lock"));
        bounded(items)
    }
}

type Source = (SourceClass, Option<String>);

/// A spec repo: `trunk`, the CDN and the legacy master repo are public;
/// other repos keep only their host.
fn repo_source(repo: &str) -> Source {
    let lower = repo.trim().trim_end_matches('/').to_ascii_lowercase();
    if lower == "trunk" {
        return (SourceClass::PublicRegistry, Some(TRUNK.into()));
    }
    let Some((_, rest)) = lower.split_once("://") else {
        return if super::swiftpm::is_local(&lower) {
            (SourceClass::Path, None)
        } else {
            (SourceClass::PrivateRegistry, None) // `git@host:org/Specs.git`
        };
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let location = format!("{host}/{path}");
    let location = location.trim_end_matches('/').trim_end_matches(".git");
    if location == TRUNK || location == "github.com/cocoapods/specs" {
        return (SourceClass::PublicRegistry, Some(TRUNK.into()));
    }
    classify_url(repo, &[])
}

/// Without a `source` line CocoaPods uses trunk; with several sources the
/// one a pod comes from is not known without resolving.
fn default_source(sources: &[Source]) -> Source {
    match sources {
        [] => (SourceClass::PublicRegistry, Some(TRUNK.into())),
        [only] => only.clone(),
        _ if sources.iter().all(|s| s.0 == SourceClass::PublicRegistry) => sources[0].clone(),
        _ => (SourceClass::Unknown, None),
    }
}

fn root(name: &str) -> &str {
    name.split('/').next().unwrap_or(name)
}

/// Removes a `#` comment outside string literals.
fn strip_comment(line: &str) -> &str {
    let (mut quote, mut escaped) = (None, false);
    for (index, ch) in line.char_indices() {
        match quote {
            Some(q) => match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, c) if c == q => quote = None,
                _ => {}
            },
            None if ch == '#' => return &line[..index],
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None => {}
        }
    }
    line
}

/// Open brackets outside string literals.
fn depth(code: &str) -> i32 {
    let (mut depth, mut quote, mut escaped) = (0, None, false);
    for ch in code.chars() {
        match quote {
            Some(q) => match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, c) if c == q => quote = None,
                _ => {}
            },
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            },
        }
    }
    depth
}

/// Arguments split at commas outside strings and brackets.
fn split_args(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut escaped, mut start) = (0, None, false, 0);
    for (index, ch) in text.char_indices() {
        match quote {
            Some(q) => match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, c) if c == q => quote = None,
                _ => {}
            },
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&text[start..index]);
                    start = index + 1;
                }
                _ => {}
            },
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Ruby code without comments and `=begin` blocks, one logical line per
/// statement: a trailing `,` or `\` or an open bracket continues it.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let (mut joined, mut documentation) = (0, false);
    for raw in text.lines().take(100_000) {
        if documentation || raw.starts_with("=begin") {
            documentation = !raw.starts_with("=end");
            continue;
        }
        let code = strip_comment(raw).trim();
        if code.is_empty() && current.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(code.trim_end_matches('\\'));
        joined += 1;
        let open = code.ends_with(',') || code.ends_with('\\') || depth(&current) > 0;
        if open && joined < 50 && current.len() < 8_192 {
            continue;
        }
        out.push(std::mem::take(&mut current));
        joined = 0;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A `pod` line before the Podfile's sources are known.
struct Declared {
    name: String,
    qualifier: String,
    context: String,
    version: Option<String>,
    source: Option<Source>,
    detail: String,
}

fn declared(name: &str, rest: &str, context: String) -> Declared {
    let mut requirements = Vec::new();
    let mut options: Vec<(String, String)> = Vec::new();
    for part in split_args(rest) {
        let part = part.trim().trim_end_matches(')').trim();
        if let Some(found) = OPTION.captures(part) {
            let key = found.get(1).or(found.get(2)).or(found.get(3)).map_or("", |m| m.as_str());
            options.push((key.to_string(), found[4].trim().to_string()));
        } else if let Some(found) = QUOTED.captures(part).filter(|c| !c[1].contains("://")) {
            requirements.push(found[1].trim().to_string());
        }
    }
    let option = |key: &str| options.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let text = |key: &str| option(key).and_then(|v| QUOTED.captures(v)).map(|c| c[1].to_string());
    let list = |key: &str| {
        option(key).map(|v| QUOTED.captures_iter(v).map(|c| c[1].to_string()).collect::<Vec<_>>().join(", "))
    };
    let (mut detail, source) = if option("git").is_some() {
        let mut detail = String::from("Git");
        for (key, label) in [("branch", "Branch"), ("tag", "Tag"), ("commit", "Commit")] {
            if let Some(value) = text(key) {
                detail.push_str(&format!(" · {label} {}", super::swiftpm::short(&value)));
            }
        }
        (detail, Some((SourceClass::Git, None)))
    } else if option("path").is_some() {
        ("lokaler Pfad".to_string(), Some((SourceClass::Path, None)))
    } else if let Some(url) = text("podspec").or_else(|| text("http")) {
        let source = if url.contains("://") { classify_url(&url, &[]) } else { (SourceClass::Path, None) };
        ("Podspec- oder Archivverweis".to_string(), Some(source))
    } else {
        let spec = if requirements.is_empty() {
            "ohne Versionsangabe".to_string()
        } else {
            format!("Spezifikation {}", requirements.join(", "))
        };
        (spec, text("source").map(|repo| repo_source(&repo)))
    };
    let exact = match requirements.as_slice() {
        [only] if option("git").is_none() && option("path").is_none() => {
            let version = only.strip_prefix('=').map_or(only.as_str(), str::trim);
            version.starts_with(|c: char| c.is_ascii_digit()).then(|| version.to_string())
        }
        _ => None,
    };
    let subspec = name.split_once('/').map(|(_, sub)| sub);
    if let Some(sub) = subspec {
        detail.push_str(&format!(" · Subspec {sub}"));
    }
    for (key, label) in [("subspecs", "Subspecs"), ("testspecs", "Testspecs")] {
        if let Some(names) = list(key).filter(|n| !n.is_empty()) {
            detail.push_str(&format!(" · {label} {names}"));
        }
    }
    if let Some(configurations) = list("configurations").or_else(|| text("configuration")) {
        detail.push_str(&format!(" · nur {configurations}"));
    }
    Declared {
        name: root(name).to_string(),
        qualifier: if subspec.is_some() { name.to_string() } else { String::new() },
        context,
        version: exact,
        source,
        detail,
    }
}

fn podfile(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut sources = Vec::new();
    let (mut platform, mut hooks) = (None, false);
    // Named scopes (targets, methods) of the open blocks.
    let mut scopes: Vec<Option<String>> = Vec::new();
    let mut pods = Vec::new();
    for line in logical_lines(file.content) {
        if END.is_match(&line) {
            scopes.pop();
            continue;
        }
        if let Some(found) = POD.captures(&line) {
            let context = scopes.iter().rev().find_map(Clone::clone).unwrap_or_default();
            pods.push(declared(&found[1], &found[2], context));
        } else if let Some(found) = SOURCE.captures(&line) {
            sources.push(repo_source(&found[1]));
        } else if let Some(found) = PLATFORM.captures(&line) {
            platform = Some(found.get(2).map_or(found[1].to_string(), |v| format!("{} {}", &found[1], v.as_str())));
        }
        hooks |= HOOK.is_match(&line);
        if OPENS.is_match(&line) && !line.ends_with(" end") {
            let target = TARGET.captures(&line).and_then(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string());
            scopes.push(target.or_else(|| METHOD.captures(&line).map(|c| c[1].to_string())));
        }
    }
    let mut detail = String::from("Podfile (Ruby, nicht ausgeführt)");
    if let Some(platform) = platform {
        detail.push_str(&format!(" · Plattform {platform}"));
    }
    if !sources.is_empty() {
        detail.push_str(&format!(" · {} Spec-Repo-Angaben", sources.len()));
    }
    if hooks {
        detail.push_str(" · Hooks nicht ausgeführt");
    }
    detail.push_str(&format!(" · {} Pods", pods.len()));
    let fallback = default_source(&sources);
    let mut items = vec![manifest("cocoapods", file, detail)];
    for pod in pods {
        let (source, host) = pod.source.unwrap_or_else(|| fallback.clone());
        let mut item = InventoryItem::new(ItemCategory::Package, "cocoapods", pod.name, file.rel_path())
            .version(pod.version.unwrap_or_default())
            .state(InstallState::Declared)
            .source(source, host)
            .detail(pod.detail);
        item.evidence.context = pod.context;
        item.evidence.qualifier = pod.qualifier;
        items.push(item);
    }
    items
}

#[derive(Default)]
struct Pod {
    root: String,
    version: String,
    subspecs: Vec<String>,
    dependencies: Vec<PackageDependency>,
}

/// Options of an external source or checkout: `(git, …)`, `(commit, …)`.
type Options = Vec<(String, String)>;

#[derive(Default)]
struct Lock {
    pods: Vec<Pod>,
    direct: Vec<PackageDependency>,
    repos: BTreeMap<String, Source>,
    external: BTreeMap<String, Options>,
    checkout: BTreeMap<String, Options>,
    cocoapods: Option<String>,
}

/// `Name (1.2.3)` or `Name (~> 1.0)`: the name and the text in parentheses.
fn spec(text: &str) -> Option<(&str, Option<&str>)> {
    let text = text.trim();
    let (name, requirement) = match text.split_once(" (") {
        Some((name, rest)) => (name.trim(), Some(rest.strip_suffix(')').unwrap_or(rest).trim())),
        None => (text, None),
    };
    (!name.is_empty()).then_some((name, requirement))
}

fn edge(name: &str, requirement: Option<&str>, kind: &str) -> PackageDependency {
    let (requirement, exact) = match requirement {
        // `(from `…`)` names a URL or path, which is never kept.
        Some(text) if text.starts_with("from ") || text.contains('`') => ("externe Quelle".to_string(), false),
        Some(text) => match text.strip_prefix('=').filter(|v| !v.starts_with(['=', '>'])) {
            Some(version) => (version.trim().to_string(), true),
            None => (text.to_string(), false),
        },
        None => (String::new(), false),
    };
    PackageDependency { name: root(name).to_string(), requirement, kind: kind.into(), exact }
}

/// The sections of a Podfile.lock or Manifest.lock; `None` when none of the
/// CocoaPods sections is present.
fn parse_lock(text: &str) -> Option<Lock> {
    let mut lock = Lock::default();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    // Edges seen so far, so a long list is not searched once per line.
    let mut seen: std::collections::HashSet<(Option<usize>, String, String)> = std::collections::HashSet::new();
    let (mut found, mut section, mut level, mut key, mut current) = (false, "", None, String::new(), None);
    for entry in lines::yaml_entries(text) {
        if entry.indent == 0 && !entry.item {
            section = entry.key.unwrap_or_default();
            level = None;
            found |= matches!(section, "PODS" | "DEPENDENCIES" | "SPEC REPOS" | "COCOAPODS");
            if section == "COCOAPODS" && !entry.value.is_empty() {
                lock.cocoapods = Some(entry.value.to_string());
            }
            continue;
        }
        let top = entry.indent == *level.get_or_insert(entry.indent);
        match section {
            "PODS" if entry.item => {
                let Some((name, requirement)) = spec(entry.key.unwrap_or(entry.value)) else { continue };
                let pod_root = root(name);
                if top {
                    let position = *index.entry(pod_root.to_string()).or_insert_with(|| {
                        lock.pods.push(Pod { root: pod_root.to_string(), ..Pod::default() });
                        lock.pods.len() - 1
                    });
                    let pod = &mut lock.pods[position];
                    if pod.version.is_empty() {
                        pod.version = requirement.unwrap_or_default().to_string();
                    }
                    if let Some((_, sub)) = name.split_once('/')
                        && pod.subspecs.len() < 50
                        && !pod.subspecs.iter().any(|s| s == sub)
                    {
                        pod.subspecs.push(sub.to_string());
                    }
                    current = Some(position);
                } else if let Some(position) = current {
                    let pod = &mut lock.pods[position];
                    let dependency = edge(name, requirement, "lock");
                    let new = seen.insert((Some(position), dependency.name.clone(), dependency.requirement.clone()));
                    if dependency.name != pod.root && new {
                        pod.dependencies.push(dependency);
                    }
                }
            }
            "DEPENDENCIES" if entry.item => {
                if let Some((name, requirement)) = spec(entry.value) {
                    let dependency = edge(name, requirement, "Podfile");
                    if seen.insert((None, dependency.name.clone(), dependency.requirement.clone())) {
                        lock.direct.push(dependency);
                    }
                }
            }
            "SPEC REPOS" if top && !entry.item => key = entry.key.unwrap_or_default().to_string(),
            "SPEC REPOS" if entry.item => {
                lock.repos.insert(root(entry.value).to_string(), repo_source(&key));
            }
            "EXTERNAL SOURCES" | "CHECKOUT OPTIONS" => {
                let Some(name) = entry.key else { continue };
                if top {
                    key = root(name).to_string();
                    continue;
                }
                let map = if section == "CHECKOUT OPTIONS" { &mut lock.checkout } else { &mut lock.external };
                let options = map.entry(key.clone()).or_default();
                if options.len() < 16 {
                    options.push((name.trim_start_matches(':').to_string(), entry.value.to_string()));
                }
            }
            _ => {}
        }
    }
    found.then_some(lock)
}

fn lock_items(lock: Lock, rel_path: &str, state: InstallState, label: &str) -> Vec<InventoryItem> {
    let Lock { pods, repos, external, checkout, .. } = lock;
    let value = |options: Option<&Options>, key: &str| -> Option<String> {
        options?.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).filter(|v| !v.is_empty())
    };
    let mut items = Vec::new();
    for pod in pods {
        let options = external.get(&pod.root);
        let pinned = checkout.get(&pod.root);
        let (source, host) = if options.is_none() {
            repos.get(&pod.root).cloned().unwrap_or((SourceClass::Unknown, None))
        } else if value(options, "git").is_some() {
            (SourceClass::Git, None)
        } else if value(options, "path").is_some() {
            (SourceClass::Path, None)
        } else {
            match value(options, "podspec").or_else(|| value(options, "http")) {
                Some(url) if url.contains("://") => classify_url(&url, &[]),
                Some(_) => (SourceClass::Path, None),
                None => (SourceClass::Unknown, None),
            }
        };
        let mut detail = label.to_string();
        if !pod.subspecs.is_empty() {
            detail.push_str(&format!(" · Subspecs {}", pod.subspecs.join(", ")));
        }
        for (key, name) in [("tag", "Tag"), ("branch", "Branch"), ("commit", "Commit")] {
            if let Some(text) = value(pinned, key).or_else(|| value(options, key)) {
                detail.push_str(&format!(" · {name} {}", super::swiftpm::short(&text)));
            }
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "cocoapods", pod.root, rel_path)
            .version(pod.version)
            .state(state)
            .source(source, host)
            .detail(detail);
        item.evidence.dependencies = pod.dependencies;
        items.push(item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const PODFILE: &str = r#"source 'https://cdn.cocoapods.org/'
platform :ios, '15.0'
use_frameworks!

=begin
pod 'Documented', '1.0'
=end

def shared_pods
  pod 'SwiftLint', '0.54.0'
end

target 'MyApp' do
  shared_pods
  pod 'Alamofire', '~> 5.8'
  pod 'Firebase/Analytics'
  pod 'Firebase/Crashlytics'
  pod 'AFNetworking', :git => 'https://github.com/gowalla/AFNetworking.git', :branch => 'dev'
  pod 'Private', git: 'https://deploy:s3cret@git.corp.example/ios/private.git', tag: '1.2.0'
  pod 'LocalKit', :path => '../LocalKit'
  pod 'JSONKit', :podspec => 'https://example.com/JSONKit.podspec'
  pod 'PonyDebugger', :configurations => ['Debug', 'Beta']
  pod 'Objection', '> 0.1', '< 1.0'
  pod 'QueryKit',
      :subspecs => ['Attribute', 'QuerySet']
  # pod 'Commented', '1.0'

  target 'MyAppTests' do
    inherit! :search_paths
    pod 'Quick', '7.3.0' # exact pin
  end
end

post_install do |installer|
  installer.pods_project.targets.each do |target|
    target.build_configurations.each do |config|
      config.build_settings['IPHONEOS_DEPLOYMENT_TARGET'] = '15.0'
    end
  end
end
"#;

    #[test]
    fn podfile_pods_with_targets_options_and_sources() {
        let items = CocoaPods.parse_file(&file("Podfile", PODFILE)).unwrap();
        assert!(items[0].detail.contains("Plattform ios 15.0"), "{}", items[0].detail);
        assert!(items[0].detail.contains("Hooks nicht ausgeführt") && items[0].detail.ends_with("12 Pods"));
        assert!(items[0].detail.contains("1 Spec-Repo-Angaben"), "{}", items[0].detail);
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let lint = find("SwiftLint");
        assert_eq!((lint.evidence.context.as_str(), lint.version.as_deref()), ("shared_pods", Some("0.54.0")));
        let alamofire = find("Alamofire");
        assert_eq!(alamofire.evidence.context, "MyApp");
        assert_eq!(alamofire.detail, "Spezifikation ~> 5.8");
        assert_eq!((alamofire.source, alamofire.source_host.as_deref()), (SourceClass::PublicRegistry, Some(TRUNK)));
        let firebase: Vec<&str> =
            items.iter().filter(|i| i.name == "Firebase").map(|i| i.evidence.qualifier.as_str()).collect();
        assert_eq!(firebase, ["Firebase/Analytics", "Firebase/Crashlytics"]);
        let af = find("AFNetworking");
        assert_eq!((af.source, af.detail.as_str()), (SourceClass::Git, "Git · Branch dev"));
        assert_eq!(find("Private").detail, "Git · Tag 1.2.0");
        assert_eq!(find("LocalKit").source, SourceClass::Path);
        assert_eq!(find("JSONKit").source_host.as_deref(), Some("example.com"));
        assert!(find("PonyDebugger").detail.ends_with("nur Debug, Beta"));
        let objection = find("Objection");
        assert_eq!((objection.detail.as_str(), objection.version.as_deref()), ("Spezifikation > 0.1, < 1.0", None));
        assert!(find("QueryKit").detail.ends_with("Subspecs Attribute, QuerySet"));
        let quick = find("Quick");
        assert_eq!((quick.evidence.context.as_str(), quick.version.as_deref()), ("MyAppTests", Some("7.3.0")));
        assert!(!items.iter().any(|i| i.name == "Commented" || i.name == "Documented"));
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cret") && !debug.contains("git.corp.example") && !debug.contains("deploy"));
    }

    #[test]
    fn podfile_with_private_spec_repos_is_not_public() {
        let mixed = "source 'https://github.com/artsy/Specs.git'\nsource 'https://cdn.cocoapods.org/'\npod 'Artsy+UIFonts', '3.3.1'\n";
        let items = CocoaPods.parse_file(&file("Podfile", mixed)).unwrap();
        assert_eq!((items[1].source, items[1].version.as_deref()), (SourceClass::Unknown, Some("3.3.1")));
        let private = "source 'https://token@specs.corp.example/ios.git'\npod 'Kit'\n";
        let items = CocoaPods.parse_file(&file("Podfile", private)).unwrap();
        assert_eq!(items[1].source, SourceClass::PrivateRegistry);
        assert_eq!(items[1].source_host.as_deref(), Some("specs.corp.example"));
        let keys = "pod 'StringKeys', 'git' => 'https://github.com/a/string-keys.git', 'branch' => 'main'\n";
        let items = CocoaPods.parse_file(&file("Podfile", keys)).unwrap();
        assert_eq!((items[1].source, items[1].detail.as_str()), (SourceClass::Git, "Git · Branch main"));
        let legacy = "source 'https://github.com/CocoaPods/Specs.git'\npod 'Kit'\n";
        assert_eq!(CocoaPods.parse_file(&file("Podfile", legacy)).unwrap()[1].source, SourceClass::PublicRegistry);
    }

    const LOCK: &str = r#"PODS:
  - Alamofire (5.8.1)
  - AFNetworking (4.0.1):
    - AFNetworking/NSURLSession (= 4.0.1)
    - AFNetworking/Reachability (= 4.0.1)
  - AFNetworking/NSURLSession (4.0.1):
    - AFNetworking/Reachability
  - AFNetworking/Reachability (4.0.1)
  - Firebase/Analytics (10.18.0):
    - Firebase/Core
  - Firebase/Core (10.18.0):
    - Firebase/CoreOnly
    - FirebaseAnalytics (~> 10.18.0)
  - Firebase/CoreOnly (10.18.0):
    - FirebaseCore (= 10.18.0)
  - FirebaseAnalytics (10.18.0):
    - "GoogleUtilities/AppDelegateSwizzler (~> 7.11)"
  - FirebaseCore (10.18.0)
  - GoogleUtilities/AppDelegateSwizzler (7.12.0)
  - LocalKit (0.1.0)
  - LookinShared (1.2.8)
  - PrivateKit (2.0.0)

DEPENDENCIES:
  - Alamofire (~> 5.8)
  - AFNetworking (= 4.0.1)
  - Firebase/Analytics
  - LocalKit (from `../LocalKit`)
  - LookinShared (from `https://github.com/QMUI/LookinServer.git`, branch `develop`)
  - PrivateKit

SPEC REPOS:
  https://deploy:t0ken@git.corp.example/ios/specs.git:
    - PrivateKit
  trunk:
    - AFNetworking
    - Alamofire
    - Firebase
    - FirebaseAnalytics
    - FirebaseCore
    - GoogleUtilities

EXTERNAL SOURCES:
  LocalKit:
    :path: "../LocalKit"
  LookinShared:
    :branch: develop
    :git: https://github.com/QMUI/LookinServer.git

CHECKOUT OPTIONS:
  LookinShared:
    :commit: 985e8afe77aa2f3fab599da4f9882f6e43eed919
    :git: https://github.com/QMUI/LookinServer.git

SPEC CHECKSUMS:
  Alamofire: 3ec537f71edc9804815215393ae2b1a8ea33a844

PODFILE CHECKSUM: a8b13d75f888afecfc295c9bf857d0ca031665e9

COCOAPODS: 1.15.2
"#;

    #[test]
    fn lockfile_subspecs_sources_and_edges() {
        let items = CocoaPods.parse_file(&file("Podfile.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · 9 Pods · CocoaPods 1.15.2");
        assert_eq!(items.len(), 10, "subspecs count for their root pod");
        let direct = &items[0].evidence.dependencies;
        assert_eq!(direct.len(), 6);
        assert!(direct.iter().any(|d| d.name == "AFNetworking" && d.exact && d.requirement == "4.0.1"));
        assert!(direct.iter().any(|d| d.name == "Firebase" && d.requirement.is_empty()));
        assert!(direct.iter().any(|d| d.name == "LocalKit" && d.requirement == "externe Quelle"));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let af = find("AFNetworking");
        assert!(af.evidence.dependencies.is_empty(), "own subspecs are no edges");
        assert_eq!(af.detail, "aufgelöst laut Podfile.lock · Subspecs NSURLSession, Reachability");
        let firebase = find("Firebase");
        assert_eq!((firebase.version.as_deref(), firebase.source), (Some("10.18.0"), SourceClass::PublicRegistry));
        let edges: Vec<(&str, &str, bool)> =
            firebase.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.requirement.as_str(), d.exact)).collect();
        assert_eq!(edges, [("FirebaseAnalytics", "~> 10.18.0", false), ("FirebaseCore", "10.18.0", true)]);
        assert_eq!(find("FirebaseAnalytics").evidence.dependencies[0].name, "GoogleUtilities");
        assert_eq!(find("GoogleUtilities").version.as_deref(), Some("7.12.0"));
        assert_eq!(find("LocalKit").source, SourceClass::Path);
        let lookin = find("LookinShared");
        assert_eq!(lookin.source, SourceClass::Git);
        assert!(lookin.detail.ends_with("Branch develop · Commit 985e8afe77aa"), "{}", lookin.detail);
        let private = find("PrivateKit");
        assert_eq!(
            (private.source, private.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("git.corp.example"))
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        let debug = format!("{items:?}");
        assert!(!debug.contains("t0ken") && !debug.contains("deploy") && !debug.contains("LookinServer"));
    }

    #[test]
    fn broken_lockfile_is_an_error() {
        assert!(CocoaPods.parse_file(&file("Podfile.lock", "<html>not a lockfile</html>")).is_err());
        let legacy = "PODS:\n  - Kit (1.0)\n\nDEPENDENCIES:\n  - Kit\n\nCOCOAPODS: 0.39.0\n";
        let items = CocoaPods.parse_file(&file("Podfile.lock", legacy)).unwrap();
        assert_eq!(items[1].source, SourceClass::Unknown, "without SPEC REPOS the repo is unknown");
    }

    #[test]
    fn pods_folder_beside_a_podfile_reads_the_manifest_lock() {
        assert!(CocoaPods.claim_dir("Pods", &["README.md".into()], &[]).is_none());
        let claim = CocoaPods.claim_dir("Pods", &["Podfile".into()], &["Manifest.lock".into()]).unwrap();
        let mut dir = FakeDir::default();
        dir.files.insert("Manifest.lock".into(), LOCK.into());
        let items = CocoaPods.inspect_dir(claim, "ios/Pods", &mut dir).unwrap();
        assert_eq!(items[0].category, ItemCategory::Environment);
        assert_eq!(items.len(), 10);
        let alamofire = items.iter().find(|i| i.name == "Alamofire").unwrap();
        assert_eq!(alamofire.install_state, Some(InstallState::Installed));
        assert_eq!(alamofire.rel_path, "ios/Pods/Manifest.lock");
        let empty = CocoaPods.inspect_dir(claim, "ios/Pods", &mut FakeDir::default()).unwrap();
        assert_eq!(empty.len(), 1);
    }
}
