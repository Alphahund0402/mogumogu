//! Maven: pom.xml (declared dependencies, managed versions, plugins, build
//! extensions, profiles and the parent; modules as edges; repository hosts)
//! and `.mvn/extensions.xml` (core extensions). `${…}` placeholders resolve
//! only from the same file's properties; parent POMs, profile activation
//! and settings.xml (mirrors, credentials) are never read, so sources stay
//! unknown. `target` beside a pom.xml is an output and is not walked.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::xml::{self, Element};
use std::collections::{BTreeMap, BTreeSet};

pub struct Maven;

pub(super) const PUBLIC: &[&str] = &["repo.maven.apache.org", "repo1.maven.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "maven",
    name: "Maven",
    markers: &["pom.xml", "extensions.xml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "pom.xml (dependencies, dependencyManagement, plugins, extensions, profiles, parent, modules)",
        ".mvn/extensions.xml",
    ],
    limits: "Kein mvn-Aufruf; Elternprojekte, Profilaktivierung und settings.xml werden nicht ausgewertet – Platzhalter nur aus den Properties derselben Datei; Repositories nur als Hosts.",
};

impl Adapter for Maven {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("extensions.xml") {
            return bounded(core_extensions(file)?);
        }
        bounded(pom(file)?)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == "target" && siblings.iter().any(|s| s == "pom.xml")).then_some(DirClaim {
            adapter: "maven",
            category: ItemCategory::Output,
            label: "Maven-Buildausgabe",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![
            InventoryItem::new(ItemCategory::Output, "maven", "target", rel_path)
                .detail("Generierte Buildausgabe; Größe erst nach Messung bekannt"),
        ])
    }
}

/// Text of the direct child `name` of the element at `index`.
fn child<'a>(elements: &'a [Element], index: usize, name: &str) -> Option<&'a str> {
    let depth = elements[index].depth;
    elements[index + 1..]
        .iter()
        .take_while(|e| e.depth > depth)
        .find(|e| e.depth == depth + 1 && e.name == name)
        .map(|e| e.text.as_str())
}

fn pom(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
    let parents = parents(&elements);
    let project = elements.iter().position(|e| e.name == "project" && e.depth == 0);
    let parent =
        project.and_then(|p| (p + 1..elements.len()).find(|i| parents[*i] == Some(p) && elements[*i].name == "parent"));
    let field = |index: Option<usize>, name: &str| index.and_then(|index| child(&elements, index, name));
    // Looked up once; a module inherits group and version from its parent.
    let (artifact, parent_group, parent_version) =
        (field(project, "artifactId"), field(parent, "groupId"), field(parent, "version"));
    let (group, version) = (field(project, "groupId").or(parent_group), field(project, "version").or(parent_version));
    let mut properties: BTreeMap<&str, &str> = BTreeMap::new();
    for element in elements.iter().filter(|e| e.parent == "properties") {
        properties.entry(element.name.as_str()).or_insert(element.text.as_str());
    }
    // `${name}` from this file only; anything else stays as written.
    let resolve = |value: &str| -> Option<String> {
        let Some(name) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) else {
            return Some(value.to_string());
        };
        let found = match name {
            "project.version" | "version" | "pom.version" => version,
            "project.groupId" | "groupId" => group,
            "project.artifactId" | "artifactId" => artifact,
            "project.parent.version" | "parent.version" => parent_version,
            "project.parent.groupId" | "parent.groupId" => parent_group,
            _ => properties.get(name).copied(),
        };
        found.map(str::to_string)
    };
    let coordinate = format!("{}:{}", group.unwrap_or("?"), artifact.unwrap_or("?"));
    let mut items = Vec::new();
    let mut modules: Vec<PackageDependency> = Vec::new();
    let mut seen_modules = BTreeSet::new();
    let mut profiles: BTreeMap<usize, String> = BTreeMap::new();
    let mut hosts = Hosts::default();
    for (index, element) in elements.iter().enumerate() {
        let up = |levels: usize| ancestor(&elements, &parents, index, levels);
        match (element.name.as_str(), element.parent.as_str()) {
            ("module", "modules") | ("subproject", "subprojects") => {
                if let Some(path) = relative_path(&element.text)
                    && seen_modules.insert(path.clone())
                {
                    modules.push(PackageDependency {
                        name: path,
                        kind: "module".into(),
                        ..PackageDependency::default()
                    });
                }
                continue;
            }
            ("url", "repository" | "pluginRepository")
                if matches!(up(2), Some("repositories" | "pluginRepositories")) =>
            {
                hosts.add(&element.text, PUBLIC);
                continue;
            }
            _ => {}
        }
        let base = match (element.name.as_str(), element.parent.as_str()) {
            ("dependency", "dependencies") => match up(2) {
                Some("dependencyManagement") => "managed",
                Some("plugin") => "plugin-dependency",
                _ => child(&elements, index, "scope").unwrap_or("compile"),
            },
            ("plugin", "plugins") => match up(2) {
                Some("pluginManagement") => "plugin-management",
                Some("reporting") => "reporting",
                _ => "plugin",
            },
            ("extension", "extensions") if up(2) == Some("build") => "extension",
            ("parent", "project") => "parent",
            _ => continue,
        };
        let own = |name: &str| child(&elements, index, name).and_then(|v| resolve(v).or(Some(v.to_string())));
        let default_group =
            matches!(base, "plugin" | "plugin-management" | "reporting").then_some("org.apache.maven.plugins");
        let group = own("groupId").or(default_group.map(str::to_string));
        let (Some(group), Some(artifact)) = (group, own("artifactId")) else { continue };
        let written = child(&elements, index, "version");
        let version = written.and_then(resolve);
        let mut detail = match (written, &version) {
            (Some(_), Some(version)) => format!("Spezifikation {version}"),
            (Some(written), None) => format!("Version aus {written} (nicht in dieser Datei)"),
            (None, _) => "Version verwaltet oder geerbt".into(),
        };
        detail.push_str(&format!(" · {base}"));
        if child(&elements, index, "optional") == Some("true") {
            detail.push_str(" · optional");
        }
        let profile = profile_of(&elements, &parents, index).map(|at| {
            let id = || child(&elements, at, "id").filter(|id| !id.is_empty()).unwrap_or("?").to_string();
            profiles.entry(at).or_insert_with(id).clone()
        });
        if let Some(id) = &profile {
            detail.push_str(&format!(" · Profil {id}"));
        }
        let mut item =
            InventoryItem::new(ItemCategory::Package, "maven", format!("{group}:{artifact}"), file.rel_path())
                .version(version.unwrap_or_default())
                .state(InstallState::Declared)
                .detail(detail);
        match profile {
            Some(id) => {
                item.evidence.context = format!("profile:{id}");
                item.evidence.qualifier = base.into();
            }
            None => item.evidence.context = base.into(),
        }
        items.push(item);
    }
    let mut detail = format!("Projekt {coordinate} (nicht gebaut)");
    if !modules.is_empty() {
        detail.push_str(&format!(" · {} Module", modules.len()));
    }
    if let Some(label) = hosts.label() {
        detail.push_str(&format!(" · Repositories: {label}"));
    }
    let mut manifest = manifest("maven", file, detail);
    manifest.evidence.dependencies = modules;
    items.insert(0, manifest);
    Ok(items)
}

/// `.mvn/extensions.xml` of a Maven build; the same name elsewhere is not ours.
fn core_extensions(file: &FoundFile<'_>) -> AdapterResult {
    if file.rel_dir != ".mvn" && !file.rel_dir.ends_with("/.mvn") {
        return Ok(Vec::new());
    }
    let elements = xml::elements(file.content, 10_000, 16).map_err(|e| e.to_string())?;
    if !elements.first().is_some_and(|e| e.depth == 0 && e.name == "extensions") {
        return Ok(Vec::new());
    }
    let mut items = vec![manifest("maven", file, "Maven-Kernerweiterungen (nicht geladen)")];
    for (index, element) in elements.iter().enumerate() {
        if element.name != "extension" || element.depth != 1 {
            continue;
        }
        let (Some(group), Some(artifact)) = (child(&elements, index, "groupId"), child(&elements, index, "artifactId"))
        else {
            continue;
        };
        let version = child(&elements, index, "version").filter(|v| !v.contains("${"));
        let spec = version.map_or("Version unbekannt".to_string(), |v| format!("Spezifikation {v}"));
        let mut item =
            InventoryItem::new(ItemCategory::Package, "maven", format!("{group}:{artifact}"), file.rel_path())
                .version(version.unwrap_or_default())
                .state(InstallState::Declared)
                .detail(format!("{spec} · Kernerweiterung"));
        item.evidence.context = "extension".into();
        items.push(item);
    }
    Ok(items)
}

/// Parent index of every element, computed once (document order, depths).
pub(super) fn parents(elements: &[Element]) -> Vec<Option<usize>> {
    let mut stack: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        while stack.last().is_some_and(|top| elements[*top].depth >= element.depth) {
            stack.pop();
        }
        out.push(stack.last().copied());
        stack.push(index);
    }
    out
}

/// Name of the element `levels` above, e.g. `dependencyManagement` two
/// levels above a managed dependency.
fn ancestor<'a>(elements: &'a [Element], parents: &[Option<usize>], index: usize, levels: usize) -> Option<&'a str> {
    let mut current = index;
    for _ in 0..levels {
        current = parents[current]?;
    }
    Some(elements[current].name.as_str())
}

/// The `<profile>` element an element belongs to.
fn profile_of(elements: &[Element], parents: &[Option<usize>], index: usize) -> Option<usize> {
    let mut current = parents[index];
    while let Some(at) = current {
        if elements[at].name == "profile" && elements[at].parent == "profiles" {
            return Some(at);
        }
        current = parents[at];
    }
    None
}

/// A relative module/build path with `/` separators; absolute paths and
/// placeholders are not kept.
pub(super) fn relative_path(raw: &str) -> Option<String> {
    let path = raw.trim().replace('\\', "/");
    let bytes = path.as_bytes();
    let absolute = path.starts_with('/') || (bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':');
    if path.is_empty() || absolute || path.contains("${") || path.contains("$(") {
        return None;
    }
    let mut path = path.as_str();
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    let path = path.trim_end_matches('/');
    Some(if path.is_empty() { ".".into() } else { path.to_string() })
}

/// Repository hosts for a manifest detail. URLs, paths and credentials are
/// never kept: only counts and the hosts of private repositories.
#[derive(Debug, Default)]
pub(super) struct Hosts {
    public: usize,
    local: usize,
    unknown: usize,
    /// Distinct private hosts in order of appearance, bounded.
    private: Vec<String>,
}

/// Private hosts named in a label; the rest are counted.
const NAMED_HOSTS: usize = 3;

impl Hosts {
    pub(super) fn add(&mut self, url: &str, public: &[&str]) {
        let url = url.trim();
        // A placeholder, or a whole `%VARIABLE%` (a host with one is unknown below).
        let variable = url.len() > 1 && url.starts_with('%') && url.ends_with('%') && !url.contains(['/', '\\']);
        if url.contains("${") || url.contains("$(") || variable {
            self.unknown += 1;
            return;
        }
        if !url.contains("://") {
            self.local += 1;
            return;
        }
        match classify_url(url, public) {
            (SourceClass::PublicRegistry, _) => self.public += 1,
            (SourceClass::PrivateRegistry, Some(host)) => {
                if self.private.len() < 64 && !self.private.contains(&host) {
                    self.private.push(host);
                }
            }
            (SourceClass::Path, _) => self.local += 1,
            _ => self.unknown += 1,
        }
    }

    pub(super) fn has_private(&self) -> bool {
        !self.private.is_empty()
    }

    /// `1 öffentlich, 1 lokal, privat: maven.corp.example`
    pub(super) fn label(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.public > 0 {
            parts.push(format!("{} öffentlich", self.public));
        }
        if self.local > 0 {
            parts.push(format!("{} lokal", self.local));
        }
        if self.unknown > 0 {
            parts.push(format!("{} unbekannt", self.unknown));
        }
        if !self.private.is_empty() {
            let named = self.private.len().min(NAMED_HOSTS);
            let more = self.private.len() - named;
            let more = if more > 0 { format!(" +{more}") } else { String::new() };
            parts.push(format!("privat: {}{more}", self.private[..named].join(", ")));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const POM: &str = r#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId><version>3.4.0</version></parent>
  <groupId>com.example</groupId><artifactId>app</artifactId><version>1.0.0</version>
  <properties><guava.version>33.3.1-jre</guava.version></properties>
  <dependencyManagement><dependencies>
    <dependency><groupId>com.fasterxml.jackson</groupId><artifactId>jackson-bom</artifactId><version>2.18.0</version><type>pom</type><scope>import</scope></dependency>
  </dependencies></dependencyManagement>
  <dependencies>
    <dependency><groupId>com.google.guava</groupId><artifactId>guava</artifactId><version>${guava.version}</version></dependency>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><scope>test</scope></dependency>
    <dependency><groupId>com.example</groupId><artifactId>core</artifactId><version>${project.version}</version><optional>true</optional></dependency>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>${slf4j.version}</version></dependency>
  </dependencies>
  <build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin></plugins></build>
</project>"#;

    #[test]
    fn dependencies_with_scopes_properties_and_plugins() {
        let items = Maven.parse_file(&file("pom.xml", POM)).unwrap();
        assert_eq!(items[0].detail, "Projekt com.example:app (nicht gebaut)");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(find("com.google.guava:guava").version.as_deref(), Some("33.3.1-jre"));
        let junit = find("org.junit.jupiter:junit-jupiter");
        assert!(junit.version.is_none() && junit.evidence.context == "test");
        assert_eq!(find("com.fasterxml.jackson:jackson-bom").evidence.context, "managed");
        let core = find("com.example:core");
        assert_eq!(core.version.as_deref(), Some("1.0.0"));
        assert!(core.detail.ends_with("optional"));
        let slf4j = find("org.slf4j:slf4j-api");
        assert!(slf4j.version.is_none() && slf4j.detail.contains("${slf4j.version}"));
        assert_eq!(find("org.apache.maven.plugins:maven-compiler-plugin").evidence.context, "plugin");
        assert_eq!(find("org.springframework.boot:spring-boot-starter-parent").evidence.context, "parent");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        assert!(
            Maven.parse_file(&file("pom.xml", "<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///x\">]><project/>")).is_err()
        );
        assert!(Maven.claim_dir("target", &["pom.xml".into()], &[]).is_some());
    }

    const AGGREGATOR: &str = r#"<project>
  <parent><groupId>com.example</groupId><artifactId>parent</artifactId><version>2.1.0</version></parent>
  <artifactId>platform</artifactId>
  <packaging>pom</packaging>
  <modules><module>core</module><module>./web/</module><module>C:\elsewhere\x</module></modules>
  <repositories>
    <repository><id>central</id><url>https://repo.maven.apache.org/maven2</url></repository>
    <repository><id>corp</id><url>https://deploy:s3cr3t-Token@maven.corp.example/releases</url></repository>
  </repositories>
  <pluginRepositories><pluginRepository><id>local</id><url>file://${project.basedir}/repo</url></pluginRepository></pluginRepositories>
  <distributionManagement><repository><id>out</id><url>https://upload.corp.example/x</url></repository></distributionManagement>
  <dependencies>
    <dependency><groupId>${project.groupId}</groupId><artifactId>core</artifactId><version>${project.version}</version></dependency>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.16</version></dependency>
  </dependencies>
  <build>
    <extensions><extension><groupId>kr.motd.maven</groupId><artifactId>os-maven-plugin</artifactId><version>1.7.1</version></extension></extensions>
    <pluginManagement><plugins><plugin><artifactId>maven-surefire-plugin</artifactId><version>3.5.2</version></plugin></plugins></pluginManagement>
    <plugins><plugin><artifactId>maven-surefire-plugin</artifactId></plugin></plugins>
  </build>
  <profiles>
    <profile><id>it</id>
      <modules><module>integration-tests</module></modules>
      <dependencies>
        <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.16</version><scope>test</scope></dependency>
        <dependency><groupId>org.testcontainers</groupId><artifactId>postgresql</artifactId><version>1.20.4</version><scope>test</scope></dependency>
      </dependencies>
      <build><plugins><plugin><artifactId>maven-failsafe-plugin</artifactId><version>3.5.2</version></plugin></plugins></build>
    </profile>
  </profiles>
</project>"#;

    #[test]
    fn modules_repositories_profiles_and_extensions() {
        let items = Maven.parse_file(&file("pom.xml", AGGREGATOR)).unwrap();
        let detail = &items[0].detail;
        assert!(detail.starts_with("Projekt com.example:platform (nicht gebaut) · 3 Module"), "{detail}");
        assert!(detail.contains("Repositories: 1 öffentlich, 1 unbekannt, privat: maven.corp.example"), "{detail}");
        assert!(!format!("{items:?}").contains("s3cr3t") && !detail.contains("upload.corp.example"));
        let modules: Vec<(&str, &str)> =
            items[0].evidence.dependencies.iter().map(|d| (d.name.as_str(), d.kind.as_str())).collect();
        assert_eq!(modules, [("core", "module"), ("web", "module"), ("integration-tests", "module")]);

        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}"))
        };
        assert_eq!(find("com.example:core", "compile").version.as_deref(), Some("2.1.0"), "inherited version");
        assert_eq!(find("kr.motd.maven:os-maven-plugin", "extension").version.as_deref(), Some("1.7.1"));
        let managed = find("org.apache.maven.plugins:maven-surefire-plugin", "plugin-management");
        assert_eq!(managed.version.as_deref(), Some("3.5.2"));
        assert!(find("org.apache.maven.plugins:maven-surefire-plugin", "plugin").version.is_none());
        let profiled = find("org.slf4j:slf4j-api", "profile:it");
        assert_eq!(profiled.evidence.qualifier, "test");
        assert!(profiled.detail.ends_with("test · Profil it"), "{}", profiled.detail);
        assert_eq!(find("org.slf4j:slf4j-api", "compile").evidence.qualifier, "");
        assert_eq!(find("org.apache.maven.plugins:maven-failsafe-plugin", "profile:it").evidence.qualifier, "plugin");
        assert!(find("org.testcontainers:postgresql", "profile:it").version.is_some());
        let keys: std::collections::BTreeSet<String> = items.iter().map(InventoryItem::key).collect();
        assert_eq!(keys.len(), items.len(), "no item is silently dropped");
    }

    #[test]
    fn hosts_keep_counts_and_private_hosts_only() {
        let mut hosts = Hosts::default();
        assert_eq!(hosts.label(), None);
        for url in [
            "https://repo1.maven.org/maven2",
            "https://pkgs.dev.azure.com/org/_packaging/My%20Feed/maven/v1",
            "https://user:pa55@a.corp.example/x",
            "https://b.corp.example",
            "https://c.corp.example",
            "https://d.corp.example",
            "https://a.corp.example/other",
            "%FEED_URL%",
            "https://%FEED_HOST%/x",
            "%USERPROFILE%/packages",
            "../local-repo",
            "file:///C:/repo",
        ] {
            hosts.add(url, PUBLIC);
        }
        let label = hosts.label().unwrap();
        assert_eq!(
            label,
            "1 öffentlich, 3 lokal, 2 unbekannt, privat: pkgs.dev.azure.com, a.corp.example, b.corp.example +2"
        );
        assert!(!label.contains("pa55") && !label.contains("Feed") && hosts.has_private());
    }

    #[test]
    fn core_extensions_only_in_the_mvn_folder() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<extensions xmlns="http://maven.apache.org/EXTENSIONS/1.0.0">
  <extension><groupId>com.gradle</groupId><artifactId>develocity-maven-extension</artifactId><version>1.23</version></extension>
  <extension><groupId>fr.jcgay.maven</groupId><artifactId>maven-profiler</artifactId><version>${profiler.version}</version></extension>
</extensions>"#;
        let mut found = file("extensions.xml", text);
        assert!(Maven.parse_file(&found).unwrap().is_empty(), "outside .mvn the name is not Maven's");
        found.rel_dir = "service/.mvn";
        let items = Maven.parse_file(&found).unwrap();
        assert_eq!(items.len(), 3);
        let develocity = items.iter().find(|i| i.name == "com.gradle:develocity-maven-extension").unwrap();
        assert_eq!((develocity.version.as_deref(), develocity.evidence.context.as_str()), (Some("1.23"), "extension"));
        let profiler = items.iter().find(|i| i.name == "fr.jcgay.maven:maven-profiler").unwrap();
        assert!(profiler.version.is_none() && profiler.detail.starts_with("Version unbekannt"));

        let foreign = r#"<extensions><extension id="vscode.git"/></extensions>"#;
        assert_eq!(Maven.parse_file(&FoundFile { content: foreign, ..found }).unwrap().len(), 1);
        let other = r#"<configuration><extensions/></configuration>"#;
        assert!(Maven.parse_file(&FoundFile { content: other, ..found }).unwrap().is_empty());
        let broken = "<extensions><extension></extensions>";
        assert!(Maven.parse_file(&FoundFile { content: broken, ..found }).is_err());
    }
}
