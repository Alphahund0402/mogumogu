//! Maven: pom.xml (declared dependencies, managed versions, plugins and
//! the parent). `${…}` placeholders resolve only from the same file's
//! properties; parent POMs, profiles' activation and settings.xml mirrors
//! are not evaluated, so sources stay unknown. `target` beside a pom.xml
//! is an output and is not walked.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::parsers::xml::{self, Element};

pub struct Maven;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "maven",
    name: "Maven",
    markers: &["pom.xml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["pom.xml (dependencies, dependencyManagement, plugins, parent)"],
    limits: "Kein mvn-Aufruf; Elternprojekte, Profile und settings.xml werden nicht ausgewertet – Platzhalter nur aus den Properties derselben Datei.",
};

impl Adapter for Maven {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
        let child = |index: usize, name: &str| -> Option<&str> {
            let depth = elements[index].depth;
            elements[index + 1..]
                .iter()
                .take_while(|e| e.depth > depth)
                .find(|e| e.depth == depth + 1 && e.name == name)
                .map(|e| e.text.as_str())
        };
        let project = elements.iter().position(|e| e.name == "project" && e.depth == 0);
        let own = |name: &str| project.and_then(|index| child(index, name));
        let properties: Vec<(&str, &str)> =
            elements.iter().filter(|e| e.parent == "properties").map(|e| (e.name.as_str(), e.text.as_str())).collect();
        // `${name}` from this file only; anything else stays as written.
        let resolve = |value: &str| -> Option<String> {
            let Some(name) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) else {
                return Some(value.to_string());
            };
            match name {
                "project.version" | "version" => own("version").map(str::to_string),
                _ => properties.iter().find(|(key, _)| *key == name).map(|(_, v)| v.to_string()),
            }
        };
        let coordinate = format!("{}:{}", own("groupId").unwrap_or("?"), own("artifactId").unwrap_or("?"));
        let mut items = vec![manifest("maven", file, format!("Projekt {coordinate} (nicht gebaut)"))];
        for (index, element) in elements.iter().enumerate() {
            let context = match (element.name.as_str(), element.parent.as_str()) {
                ("dependency", "dependencies") => match grandparent(&elements, index) {
                    Some("dependencyManagement") => "managed",
                    Some("plugin") => "plugin-dependency",
                    _ => child(index, "scope").unwrap_or("compile"),
                },
                ("plugin", "plugins") => "plugin",
                ("parent", "project") => "parent",
                _ => continue,
            };
            let group = child(index, "groupId").or((context == "plugin").then_some("org.apache.maven.plugins"));
            let (Some(group), Some(artifact)) = (group, child(index, "artifactId")) else { continue };
            let written = child(index, "version");
            let version = written.and_then(resolve);
            let mut detail = match (written, &version) {
                (Some(_), Some(version)) => format!("Spezifikation {version}"),
                (Some(written), None) => format!("Version aus {written} (nicht in dieser Datei)"),
                (None, _) => "Version verwaltet oder geerbt".into(),
            };
            detail.push_str(&format!(" · {context}"));
            if child(index, "optional") == Some("true") {
                detail.push_str(" · optional");
            }
            let mut item =
                InventoryItem::new(ItemCategory::Package, "maven", format!("{group}:{artifact}"), file.rel_path())
                    .version(version.unwrap_or_default())
                    .state(InstallState::Declared)
                    .detail(detail);
            item.evidence.context = context.into();
            items.push(item);
        }
        bounded(items)
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

/// The element two levels up, e.g. `dependencyManagement` for a managed
/// dependency (`dependencyManagement > dependencies > dependency`).
fn grandparent(elements: &[Element], index: usize) -> Option<&str> {
    let depth = elements[index].depth.checked_sub(2)?;
    elements[..index].iter().rev().find(|e| e.depth == depth).map(|e| e.name.as_str())
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
}
