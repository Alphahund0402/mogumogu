//! Conan: conanfile.txt (declared by section), conanfile.py (Python code:
//! requirement strings matched as text, never imported or run) and
//! conan.lock (resolved: Conan 2 reference lists, Conan 1 graph locks with
//! edges). References name no remote, so sources stay unknown; profiles,
//! options and conditional requirements are not evaluated.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency};
use crate::parsers::json;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

pub struct Conan;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "conan",
    name: "Conan",
    markers: &["conanfile.txt", "conanfile.py", "conan.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["conanfile.txt", "conanfile.py: Referenzen als Text", "conan.lock (Conan 2 v0.5, Conan 1 graph_lock)"],
    limits: "Kein conan- oder Python-Aufruf; conanfile.py wird nicht ausgeführt – Bedingungen, berechnete Referenzen und geerbte python_requires bleiben unbekannt; Referenzen nennen kein Remote, die Quelle bleibt unbekannt.",
};

/// `requires = "zlib/1.3"`, also as tuple or list, in a recipe.
static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(requires|tool_requires|build_requires|test_requires|python_requires)\s*=\s*(.*)$")
        .expect("valid regex")
});
/// `self.requires("zlib/1.3", …)` and its siblings, also across lines.
static CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"self\s*\.\s*(requires|tool_requires|build_requires|test_requires)\s*\(\s*[fFrR]{0,2}['"]([^'"\n]*)['"]"#,
    )
    .expect("valid regex")
});
static STRING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"]([^'"\n]*)['"]"#).expect("valid regex"));

impl Adapter for Conan {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let name = file.name.to_ascii_lowercase();
        match name.as_str() {
            "conan.lock" => bounded(lock(file)?),
            "conanfile.py" => bounded(recipe(file)),
            _ => bounded(conanfile_txt(file)),
        }
    }
}

/// Context and detail suffix of a requirement kind.
fn kind(section: &str) -> Option<(&'static str, &'static str)> {
    match section {
        "requires" => Some(("", "")),
        "tool_requires" => Some(("build", " · tool_requires")),
        "build_requires" => Some(("build", " · build_requires")),
        "test_requires" => Some(("test", " · test_requires")),
        "python_requires" => Some(("python", " · python_requires")),
        "config_requires" => Some(("config", " · config_requires")),
        _ => None,
    }
}

/// `name/version[@user[/channel]][#revision][%timestamp]`; the version
/// may be a range such as `[>=1.0 <2]`.
struct Reference<'a> {
    name: &'a str,
    version: &'a str,
    user_channel: Option<&'a str>,
    revision: Option<&'a str>,
}

fn valid(text: &str) -> bool {
    !text.is_empty() && text.len() <= 101 && text.chars().all(|c| c.is_ascii_alphanumeric() || "_.+-".contains(c))
}

fn reference(text: &str) -> Option<Reference<'_>> {
    let text = text.trim();
    let text = text.split_once('%').map_or(text, |(reference, _)| reference);
    let (text, revision) = match text.split_once('#') {
        Some((reference, revision)) => (reference, revision.split(':').next().filter(|r| !r.is_empty())),
        None => (text, None),
    };
    let (text, user_channel) = match text.split_once('@') {
        Some((reference, user_channel)) => (reference, Some(user_channel).filter(|u| !u.is_empty())),
        None => (text, None),
    };
    let (name, version) = text.split_once('/')?;
    let range = version.starts_with('[') && version.ends_with(']');
    (valid(name) && (range || valid(version))).then_some(Reference { name, version, user_channel, revision })
}

/// Items with one entry per reference; a second version of the same name
/// in the same context gets a qualifier instead of replacing the first.
struct Collector {
    rel_path: String,
    state: InstallState,
    names: BTreeSet<(String, String)>,
    seen: BTreeSet<String>,
    items: Vec<InventoryItem>,
}

impl Collector {
    fn new(file: &FoundFile<'_>, state: InstallState) -> Self {
        Self { rel_path: file.rel_path(), state, names: BTreeSet::new(), seen: BTreeSet::new(), items: Vec::new() }
    }

    fn push(&mut self, reference: &Reference<'_>, context: &str, label: &str) -> Option<&mut InventoryItem> {
        let revision = reference.revision.map(super::swiftpm::short);
        let identity = format!("{}|{context}|{}|{}", reference.name, reference.version, revision.unwrap_or_default());
        if !self.seen.insert(identity) {
            return None;
        }
        let range = reference.version.starts_with('[');
        let mut detail = match (self.state, range) {
            (InstallState::Resolved, _) => "aufgelöst laut conan.lock".to_string(),
            (_, true) => format!("Versionsbereich {}", reference.version),
            (_, false) => format!("Spezifikation {}", reference.version),
        };
        if let Some(user_channel) = reference.user_channel {
            detail.push_str(&format!(" · @{user_channel}"));
        }
        if let Some(revision) = revision {
            detail.push_str(&format!(" · Revision {revision}"));
        }
        detail.push_str(label);
        let version = if range { "" } else { reference.version };
        let qualifier = format!("{version}{}", revision.map(|r| format!("#{r}")).unwrap_or_default());
        self.add(reference.name, version, context, detail, qualifier)
    }

    fn add(
        &mut self,
        name: &str,
        version: &str,
        context: &str,
        detail: String,
        qualifier: String,
    ) -> Option<&mut InventoryItem> {
        let duplicate = !self.names.insert((name.to_string(), context.to_string()));
        let mut item = InventoryItem::new(ItemCategory::Package, "conan", name, self.rel_path.as_str())
            .version(version)
            .state(self.state)
            .detail(detail);
        item.evidence.context = context.to_string();
        if duplicate {
            item.evidence.qualifier = qualifier;
        }
        self.items.push(item);
        self.items.last_mut()
    }
}

fn conanfile_txt(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut out = Collector::new(file, InstallState::Declared);
    let mut section = String::new();
    for line in file.content.lines().take(100_000) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')).filter(|_| !line.contains('/')) {
            section = name.trim().to_ascii_lowercase();
            continue;
        }
        let Some((context, label)) = kind(&section).filter(|_| section != "config_requires") else { continue };
        let text = line.split(" #").next().unwrap_or(line);
        if let Some(reference) = reference(text) {
            out.push(&reference, context, label);
        }
    }
    let mut items = vec![manifest("conan", file, format!("conanfile.txt · {} Referenzen", out.items.len()))];
    items.extend(out.items);
    items
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

fn open_brackets(code: &str) -> i32 {
    let mut depth = 0;
    let mut quote = None;
    for ch in code.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn recipe(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let lines: Vec<&str> = file.content.lines().take(100_000).map(strip_comment).collect();
    let mut out = Collector::new(file, InstallState::Declared);
    let mut index = 0;
    while index < lines.len() {
        let Some(found) = ATTRIBUTE.captures(lines[index]) else {
            index += 1;
            continue;
        };
        // A tuple or list may continue over the following lines.
        let mut value = found[2].to_string();
        let start = index;
        while (open_brackets(&value) > 0 || value.trim_end().ends_with('\\'))
            && index + 1 < lines.len()
            && index - start < 200
        {
            index += 1;
            value.push(' ');
            value.push_str(lines[index]);
        }
        let (context, label) = kind(&found[1]).unwrap_or(("", ""));
        for string in STRING.captures_iter(&value) {
            if let Some(reference) = reference(&string[1]) {
                out.push(&reference, context, label);
            }
        }
        index += 1;
    }
    let code = lines.join("\n");
    for found in CALL.captures_iter(&code).take(10_000) {
        let (context, label) = kind(&found[1]).unwrap_or(("", ""));
        let text = &found[2];
        if text.contains('{') || text.ends_with('/') {
            // `f"poco/{self.version}"`, `"poco/{}".format(v)` or `"poco/" + v`.
            if let Some(name) = text.split('/').next().filter(|n| valid(n)) {
                out.add(name, "", context, format!("Version aus Variable{label}"), String::new());
            }
            continue;
        }
        if let Some(reference) = reference(text) {
            out.push(&reference, context, label);
        }
    }
    let detail = format!("conanfile.py (Python, nicht ausgeführt) · {} Referenzen als Text", out.items.len());
    let mut items = vec![manifest("conan", file, detail)];
    items.extend(out.items);
    items
}

fn lock(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 32).map_err(|e| e.to_string())?;
    let version = value["version"].as_str().unwrap_or("?");
    let mut out = Collector::new(file, InstallState::Resolved);
    let mut direct = Vec::new();
    let detail = if let Some(nodes) = value["graph_lock"]["nodes"].as_object() {
        // Conan 1: nodes refer to each other by id; the node with a `path`
        // (or without a reference) is the consumer itself.
        let refs: BTreeMap<&str, Reference<'_>> = nodes
            .iter()
            .take(20_000)
            .filter_map(|(id, node)| Some((id.as_str(), reference(node["ref"].as_str()?)?)))
            .collect();
        let edges = |node: &Value| -> Vec<PackageDependency> {
            let mut edges = Vec::new();
            for key in ["requires", "build_requires"] {
                for id in node[key].as_array().into_iter().flatten().filter_map(Value::as_str).take(1_000) {
                    if let Some(target) = refs.get(id) {
                        edges.push(PackageDependency {
                            name: target.name.to_string(),
                            requirement: target.version.to_string(),
                            kind: key.into(),
                            exact: !target.version.starts_with('['),
                        });
                    }
                }
            }
            edges
        };
        // Edges copy their target's name and version: count them as they
        // are built instead of after the whole graph.
        let mut edge_count = 0_usize;
        let mut counted = |edges: Vec<PackageDependency>| {
            edge_count += edges.len();
            (edge_count <= crate::limits::ITEMS_PER_FILE).then_some(edges).ok_or_else(|| {
                format!("mehr als {} Abhängigkeitsverweise in einer Datei", crate::limits::ITEMS_PER_FILE)
            })
        };
        for (id, node) in nodes.iter().take(20_000) {
            for text in node["python_requires"].as_array().into_iter().flatten().filter_map(Value::as_str).take(100) {
                if let Some(reference) = reference(text) {
                    out.push(&reference, "python", " · python_requires");
                }
            }
            let Some(own) = refs.get(id.as_str()).filter(|_| node.get("path").is_none()) else {
                direct.extend(counted(edges(node))?);
                continue;
            };
            let context = if node["context"].as_str() == Some("build") { "build" } else { "" };
            if let Some(item) = out.push(own, context, if context.is_empty() { "" } else { " · build_requires" }) {
                item.evidence.dependencies = counted(edges(node))?;
            }
        }
        format!("Lockfile v{version} (Conan 1) · {} Pakete", out.items.len())
    } else if ["requires", "build_requires", "python_requires", "config_requires"].iter().any(|k| value[*k].is_array())
    {
        for key in ["requires", "build_requires", "python_requires", "config_requires"] {
            let (context, label) = kind(key).unwrap_or(("", ""));
            for text in value[key].as_array().into_iter().flatten().filter_map(Value::as_str).take(20_000) {
                if let Some(reference) = reference(text) {
                    out.push(&reference, context, label);
                }
            }
        }
        format!("Lockfile v{version} (Conan 2) · {} Referenzen", out.items.len())
    } else {
        return Err("conan.lock: Format nicht erkannt".into());
    };
    let mut items = vec![manifest("conan", file, detail)];
    items[0].evidence.dependencies = direct;
    items.extend(out.items);
    bounded(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn conanfile_txt_sections_and_references() {
        let text = "[requires]\nzlib/1.2.13\nopenssl/[>=1.1 <4]\npoco/1.12.4@pocoproject/stable\nfmt/10.1.1#d0a6b2ff8e9e8e4b2b45d2f9b5a1c2d3\n# boost/1.83.0\n\n[tool_requires]\ncmake/3.27.1\n\n[build_requires]\nninja/1.11.1\n\n[test_requires]\ngtest/1.14.0\n\n[generators]\nCMakeDeps\nCMakeToolchain\n\n[options]\npoco/*:shared=True\n\n[layout]\ncmake_layout\n";
        let items = Conan.parse_file(&file("conanfile.txt", text)).unwrap();
        assert_eq!(items[0].detail, "conanfile.txt · 7 Referenzen");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(find("zlib").version.as_deref(), Some("1.2.13"));
        let openssl = find("openssl");
        assert_eq!((openssl.version.as_deref(), openssl.detail.as_str()), (None, "Versionsbereich [>=1.1 <4]"));
        assert_eq!(find("poco").detail, "Spezifikation 1.12.4 · @pocoproject/stable");
        assert_eq!(find("fmt").detail, "Spezifikation 10.1.1 · Revision d0a6b2ff8e9e");
        let cmake = find("cmake");
        assert_eq!(
            (cmake.evidence.context.as_str(), cmake.detail.as_str()),
            ("build", "Spezifikation 3.27.1 · tool_requires")
        );
        assert_eq!(find("ninja").evidence.context, "build");
        assert_eq!(find("gtest").evidence.context, "test");
        assert!(!items.iter().any(|i| i.name == "boost"));
        assert_eq!(items.iter().filter(|i| i.name == "poco").count(), 1, "options name no requirement");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        assert!(items.iter().all(|i| i.source == crate::domain::SourceClass::Unknown));
    }

    const RECIPE: &str = r#"from conan import ConanFile
from conan.tools.cmake import cmake_layout


class CompressorRecipe(ConanFile):
    """Example recipe."""
    settings = "os", "compiler", "build_type", "arch"
    generators = "CMakeToolchain", "CMakeDeps"
    requires = "zlib/1.2.11", "fmt/10.1.1"
    tool_requires = (
        "cmake/3.22.6",
        "ninja/1.11.1",  # build tool
    )
    test_requires = ["gtest/1.14.0"]
    python_requires = "pyreq/0.1@user/testing"

    def requirements(self):
        self.requires("base64/0.4.0")
        if self.settings.os == "Windows":
            self.requires("openssl/[>=1.1 <4]", override=True)
        else:
            self.requires("openssl/3.2.0")
        self.requires(
            "boost/1.83.0",
            transitive_headers=True,
        )
        self.requires(f"poco/{self.version}")
        # self.requires("commented/1.0")

    def build_requirements(self):
        self.tool_requires("protobuf/3.21.12")
        self.test_requires("catch2/3.4.0")
"#;

    #[test]
    fn recipe_attributes_and_calls_are_matched_as_text() {
        let items = Conan.parse_file(&file("conanfile.py", RECIPE)).unwrap();
        assert!(items[0].detail.starts_with("conanfile.py (Python, nicht ausgeführt)"));
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}"))
        };
        assert_eq!(find("zlib", "").version.as_deref(), Some("1.2.11"));
        find("fmt", "");
        find("cmake", "build");
        find("ninja", "build");
        find("gtest", "test");
        assert_eq!(find("pyreq", "python").detail, "Spezifikation 0.1 · @user/testing · python_requires");
        find("base64", "");
        find("boost", "");
        find("protobuf", "build");
        find("catch2", "test");
        assert_eq!(find("poco", "").detail, "Version aus Variable");
        let openssl: Vec<(Option<&str>, &str)> = items
            .iter()
            .filter(|i| i.name == "openssl")
            .map(|i| (i.version.as_deref(), i.evidence.qualifier.as_str()))
            .collect();
        assert_eq!(openssl, [(None, ""), (Some("3.2.0"), "3.2.0")], "both branches stay visible");
        assert!(!items.iter().any(|i| i.name == "commented" || i.name == "os"));
    }

    #[test]
    fn conan2_lockfile_with_revisions_and_repeated_names() {
        let lock = r#"{
    "version": "0.5",
    "requires": [
        "zlib/1.3#f52e03ae3d251dec704634230cd806a2%1708593606.497",
        "matrix/1.1#905c3f0babc520684c84127378fefdd0%1675278901.7527816",
        "matrix/1.0#905c3f0babc520684c84127378fefdd0%1675278126.0552447",
        "poco/1.12.4@pocoproject/stable#1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d%1700000000.0"
    ],
    "build_requires": ["cmake/3.27.7#2b3e3ff6fd8f2a5e2b62f2bb3b6d3a5f%1697014985.112"],
    "python_requires": [],
    "config_requires": []
}"#;
        let items = Conan.parse_file(&file("conan.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v0.5 (Conan 2) · 5 Referenzen");
        let zlib = items.iter().find(|i| i.name == "zlib").unwrap();
        assert_eq!(zlib.version.as_deref(), Some("1.3"));
        assert_eq!(zlib.detail, "aufgelöst laut conan.lock · Revision f52e03ae3d25");
        let matrix: Vec<&str> =
            items.iter().filter(|i| i.name == "matrix").map(|i| i.evidence.qualifier.as_str()).collect();
        assert_eq!(matrix, ["", "1.0#905c3f0babc5"]);
        assert_eq!(items.iter().find(|i| i.name == "cmake").unwrap().evidence.context, "build");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        assert!(Conan.parse_file(&file("conan.lock", "{\"version\": \"0.5\", \"requires\": [")).is_err());
        assert!(Conan.parse_file(&file("conan.lock", "{\"something\": 1}")).is_err());
    }

    #[test]
    fn conan1_graph_lock_skips_the_consumer_and_keeps_edges() {
        let lock = r#"{
 "graph_lock": {
  "nodes": {
   "0": {"ref": "pkgb/0.1@user/testing", "options": "shared=False", "requires": ["1"], "build_requires": ["2"], "path": "conanfile.py", "context": "host"},
   "1": {"ref": "pkga/0.1@user/testing#fa1b2c3d4e5f60718293a4b5c6d7e8f9", "options": "", "package_id": "5ab84d6acfe1f23c4fae0ab88f26e3a396351ac9", "prev": "0", "requires": ["3"], "context": "host"},
   "2": {"ref": "cmake/3.25.3", "context": "build"},
   "3": {"ref": "zlib/1.2.13", "context": "host", "python_requires": ["pyreq/0.1@user/testing"]}
  },
  "revisions_enabled": true
 },
 "version": "0.4",
 "profile_host": "[settings]\narch=x86_64\nos=Windows\n"
}"#;
        let items = Conan.parse_file(&file("conan.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v0.4 (Conan 1) · 4 Pakete");
        assert!(!items.iter().any(|i| i.name == "pkgb"), "the consumer is not a dependency");
        let direct: Vec<&str> = items[0].evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(direct, ["pkga", "cmake"]);
        let pkga = items.iter().find(|i| i.name == "pkga").unwrap();
        assert_eq!(pkga.evidence.dependencies[0].name, "zlib");
        assert!(pkga.evidence.dependencies[0].exact);
        assert_eq!(items.iter().find(|i| i.name == "cmake").unwrap().evidence.context, "build");
        assert_eq!(items.iter().find(|i| i.name == "pyreq").unwrap().evidence.context, "python");
    }
}
