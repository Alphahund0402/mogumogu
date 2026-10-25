//! sbt: build definitions (`*.sbt` in a build, `project/*.sbt` for plugins)
//! matched as text, never compiled or evaluated. Module IDs
//! (`"org" % "name" % "1.0"`, `%%` for Scala cross-built names), their
//! configuration and versions from string vals of the same file are read;
//! Scala files in `project/` (e.g. Dependencies.scala), computed settings
//! and resolver credentials are not. The Scala suffix of `%%` names stays
//! unresolved. `target` beside a build definition and the meta-build's own
//! `project/project` are outputs and are not walked.
use super::maven::Hosts;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use regex::{Captures, Regex};
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct Sbt;

const PUBLIC: &[&str] = &[
    "repo1.maven.org",
    "repo.maven.apache.org",
    "oss.sonatype.org",
    "s01.oss.sonatype.org",
    "central.sonatype.com",
    "repo.scala-sbt.org",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "sbt",
    name: "sbt",
    markers: &["*.sbt"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "build.sbt und *.sbt: Modul-IDs als Text (%, %%, %%%)",
        "project/plugins.sbt (addSbtPlugin)",
        "Versionen aus val derselben Datei",
    ],
    limits: "Kein sbt-Aufruf; Builddefinitionen werden nicht kompiliert – Abhängigkeiten aus project/*.scala, Variablen anderer Dateien und berechnete Einstellungen bleiben unbekannt; der Scala-Suffix von %%-Namen wird nicht aufgelöst; Resolver nur als Hosts.",
};

/// `"org" %% "name" % "1.0"` or `% versionVal`, with an optional `% Test`.
static MODULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#""([A-Za-z0-9_.\-]+)"\s*(%%%|%%|%)\s*"([A-Za-z0-9_.\-]+)"\s*%\s*(?:"([^"\s]+)"|([A-Za-z_][\w.]*))"#,
        r#"(?:\s*%\s*(?:"([^"]+)"|([A-Za-z][\w]*)))?"#
    ))
    .expect("valid regex")
});
static SETTING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(libraryDependencies|dependencyOverrides|excludeDependencies|addSbtPlugin|addCompilerPlugin)\b")
        .expect("valid regex")
});
static VAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(?:lazy\s+)?val\s+([A-Za-z_]\w*)\s*(?::\s*String\s*)?=\s*"([0-9][A-Za-z0-9.+_\-]{0,63})"\s*$"#)
        .expect("valid regex")
});
static SCALA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bscalaVersion(?:\s+in\s+\w+)?\s*:=\s*(?:"([^"]+)"|([A-Za-z_][\w.]*))"#).expect("valid regex")
});
static CROSS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcrossScalaVersions(?:\s+in\s+\w+)?\s*:=\s*(?:Seq|List)\s*\(([^)]*)\)").expect("valid regex")
});
static RESOLVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"]*"\s+at\s+"([^"]+)""#).expect("valid regex"));
/// `libraryDependencies ++= Dependencies.core`: a list defined in Scala code.
static REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\blibraryDependencies\s*\+\+?=\s*([A-Za-z_][\w.]*)").expect("valid regex"));

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Library,
    Override,
    Exclude,
    Plugin,
    CompilerPlugin,
}

impl Adapter for Sbt {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        // Builds under `src/sbt-test` are fixtures of plugin tests.
        if file.rel_dir.split('/').any(|part| part == "sbt-test") {
            return Ok(Vec::new());
        }
        bounded(definition(file))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let has = |file: &str| siblings.iter().any(|s| s == file);
        let definition = siblings.iter().any(|s| s.ends_with(".sbt"));
        // The names inside `project/`: build.properties beside build code.
        let meta = has("build.properties") && siblings.iter().any(|s| s.ends_with(".sbt") || s.ends_with(".scala"));
        let label = match name {
            "target" if definition || meta => "sbt-Buildausgabe",
            "project" if meta && !has("build.sbt") && !children.iter().any(|c| c == "build.properties") => {
                "sbt-Metabuild"
            }
            _ => return None,
        };
        Some(DirClaim { adapter: "sbt", category: ItemCategory::Output, label })
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        let (name, detail) = match claim.label {
            "sbt-Metabuild" => ("project", "Metabuild des Metabuilds; meist generiert, nicht gelesen"),
            _ => ("target", "Generierte Buildausgabe; Größe erst nach Messung bekannt"),
        };
        Ok(vec![InventoryItem::new(ItemCategory::Output, "sbt", name, rel_path).detail(detail)])
    }
}

/// The line without comments; `comment` carries an open `/* … */`.
fn code(line: &str, comment: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let (mut i, mut start, mut quoted) = (0, 0, false);
    while i < bytes.len() {
        if *comment {
            match line[i..].find("*/") {
                Some(end) => {
                    *comment = false;
                    i += end + 2;
                    start = i;
                    continue;
                }
                None => return out,
            }
        }
        match bytes[i] {
            b'"' => quoted = !quoted,
            b'\\' if quoted => i += 1,
            b'/' if !quoted && bytes.get(i + 1) == Some(&b'/') => break,
            b'/' if !quoted && bytes.get(i + 1) == Some(&b'*') => {
                out.push_str(&line[start..i]);
                out.push(' ');
                *comment = true;
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&line[start.min(line.len())..i.min(line.len())]);
    out
}

/// Positions of parentheses outside string literals: `+1` or `-1`.
fn parens(code: &str) -> Vec<(usize, i64)> {
    let mut out = Vec::new();
    let mut quoted = false;
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => quoted = !quoted,
            b'\\' if quoted => i += 1,
            b'(' if !quoted => out.push((i, 1)),
            b')' if !quoted => out.push((i, -1)),
            _ => {}
        }
        i += 1;
    }
    out
}

enum Mark<'a> {
    Setting(Target),
    Paren(i64),
    Module(Captures<'a>),
}

fn definition(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut comment = false;
    let lines: Vec<String> = file.content.lines().take(100_000).map(|line| code(line, &mut comment)).collect();
    // Version-like string vals; the first definition of a name counts.
    let mut vals: BTreeMap<&str, &str> = BTreeMap::new();
    for line in &lines {
        if let Some(val) = VAL.captures(line).filter(|_| vals.len() < 2_000)
            && let (Some(name), Some(value)) = (val.get(1), val.get(2))
        {
            vals.entry(name.as_str()).or_insert(value.as_str());
        }
    }
    // `circeVersion`, or `V.circe` for a val inside an object of this file.
    let lookup =
        |name: &str| -> Option<&str> { vals.get(name).or_else(|| vals.get(name.rsplit('.').next()?)).copied() };
    let mut items = Vec::new();
    let (mut scala, mut cross, mut references) = (None, Vec::new(), 0);
    let mut hosts = Hosts::default();
    let mut depth: i64 = 0;
    let mut active: Option<(Target, i64)> = None;
    for line in &lines {
        if let Some(found) = SCALA.captures(line) {
            scala = found.get(1).map(|m| m.as_str()).or_else(|| found.get(2).and_then(|m| lookup(m.as_str())));
        }
        if let Some(found) = CROSS.captures(line) {
            cross = found[1]
                .split(',')
                .map(str::trim)
                .filter_map(|v| v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).or_else(|| lookup(v)))
                .take(16)
                .map(str::to_string)
                .collect();
        }
        for resolver in RESOLVER.captures_iter(line) {
            hosts.add(&resolver[1], PUBLIC);
        }
        references +=
            REFERENCE.captures_iter(line).filter(|c| !matches!(&c[1], "Seq" | "List" | "Vector" | "Set")).count();
        let mut marks: Vec<(usize, Mark<'_>)> = Vec::new();
        for setting in SETTING.captures_iter(line) {
            let target = match &setting[1] {
                "libraryDependencies" => Target::Library,
                "dependencyOverrides" => Target::Override,
                "excludeDependencies" => Target::Exclude,
                "addSbtPlugin" => Target::Plugin,
                _ => Target::CompilerPlugin,
            };
            marks.push((setting.get(0).map_or(0, |m| m.start()), Mark::Setting(target)));
        }
        marks.extend(parens(line).into_iter().map(|(at, delta)| (at, Mark::Paren(delta))));
        for module in MODULE.captures_iter(line) {
            marks.push((module.get(0).map_or(0, |m| m.start()), Mark::Module(module)));
        }
        marks.sort_by_key(|(at, _)| *at);
        for (_, mark) in marks {
            match mark {
                Mark::Setting(target) => active = Some((target, depth)),
                Mark::Paren(delta) => {
                    depth += delta;
                    if active.is_some_and(|(_, base)| depth < base) {
                        active = None;
                    }
                }
                Mark::Module(module) if items.len() < 10_000 => {
                    items.extend(module_item(file, &module, active.map(|(t, _)| t), &lookup));
                }
                Mark::Module(_) => {}
            }
        }
        // A statement ends with its line unless its parentheses or a
        // trailing `+=` continue it.
        let open = line.trim_end().ends_with("+=");
        if active.is_some_and(|(_, base)| depth <= base) && !open {
            active = None;
        }
    }
    let in_project = file.rel_dir == "project" || file.rel_dir.ends_with("/project");
    let mut detail = String::from(if in_project {
        "sbt-Metabuild (nicht ausgeführt)"
    } else {
        "sbt-Builddefinition (nicht ausgeführt)"
    });
    if let Some(scala) = scala {
        detail.push_str(&format!(" · Scala {scala}"));
    }
    if !cross.is_empty() {
        detail.push_str(&format!(" · Cross-Build {}", cross.join(", ")));
    }
    if references > 0 {
        detail.push_str(&format!(" · {references} Abhängigkeitslisten aus Scala-Code (nicht gelesen)"));
    }
    if let Some(label) = hosts.label() {
        detail.push_str(&format!(" · Resolver: {label}"));
    }
    items.insert(0, manifest("sbt", file, detail));
    items
}

fn module_item<'a>(
    file: &FoundFile<'_>,
    module: &Captures<'_>,
    target: Option<Target>,
    lookup: &impl Fn(&str) -> Option<&'a str>,
) -> Option<InventoryItem> {
    if target == Some(Target::Exclude) {
        return None;
    }
    let text = |index: usize| module.get(index).map(|m| m.as_str());
    let (org, operator, name) = (text(1)?, text(2)?, text(3)?);
    let (version, spec) = match (text(4), text(5)) {
        (Some(literal), _) => (Some(literal), format!("Spezifikation {literal}")),
        (None, Some(variable)) => match lookup(variable) {
            Some(value) => (Some(value), format!("Spezifikation {value} aus val {variable}")),
            None => (None, "Version aus Variable".to_string()),
        },
        _ => return None,
    };
    let config = text(6).map(str::to_ascii_lowercase).or_else(|| {
        text(7).map(|id| match id {
            "IntegrationTest" => "it".to_string(),
            other => other.to_ascii_lowercase(),
        })
    });
    let cross = match operator {
        "%%" => Some("Scala-Suffix nicht aufgelöst"),
        "%%%" => Some("Scala.js-/Native-Suffix nicht aufgelöst"),
        _ => None,
    };
    let (context, label) = match target {
        Some(Target::Plugin) => ("plugin", "sbt-Plugin · "),
        Some(Target::CompilerPlugin) => ("compiler-plugin", "Compiler-Plugin · "),
        Some(Target::Override) => ("override", "Override · "),
        _ if cross.is_some() => ("scala-cross", ""),
        _ => (config.as_deref().unwrap_or("compile"), ""),
    };
    let mut detail = format!("{label}{spec}");
    let mut qualifier = Vec::new();
    if let Some(config) = config.as_deref().filter(|c| *c != context) {
        detail.push_str(&format!(" · {config}"));
        qualifier.push(config.to_string());
    }
    if let Some(cross) = cross {
        detail.push_str(&format!(" · {cross}"));
    }
    if target.is_none() {
        detail.push_str(" · außerhalb von libraryDependencies definiert");
        qualifier.push("val".into());
    }
    let mut item = InventoryItem::new(ItemCategory::Package, "sbt", format!("{org}:{name}"), file.rel_path())
        .version(version.unwrap_or_default())
        .state(InstallState::Declared)
        .detail(detail);
    item.evidence.context = context.to_string();
    item.evidence.qualifier = qualifier.join("|");
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const BUILD: &str = r#"ThisBuild / scalaVersion := scala213
ThisBuild / organization := "com.example"

val scala213 = "2.13.15"
val circeVersion = "0.14.10"
lazy val pekkoVersion: String = "1.1.2"
val password = "not-a-version"

crossScalaVersions := Seq(scala213, "3.3.4")
resolvers += "corp" at "https://ci:t0ken@nexus.corp.example/repository/maven-releases/"
resolvers += "central" at "https://repo1.maven.org/maven2/"
credentials += Credentials("Sonatype Nexus", "nexus.corp.example", "deploy", "hunter2")

lazy val root = (project in file("."))
  .settings(
    name := "hello",
    libraryDependencies += "org.scalatest" %% "scalatest" % "3.2.19" % Test,
    libraryDependencies ++= Seq(
      "io.circe" %% "circe-core" % circeVersion,
      "io.circe" %% "circe-generic" % circeVersion, // derivation
      "org.apache.pekko" %% "pekko-actor-typed" % pekkoVersion,
      "org.postgresql" % "postgresql" % "42.7.4",
      "org.typelevel" %%% "cats-core" % "2.12.0",
      "com.example" % "secret-lib" % password,
      "javax.servlet" % "javax.servlet-api" % "4.0.1" % "provided"
    ),
    libraryDependencies +=
      "ch.qos.logback" % "logback-classic" % "1.5.12" % Runtime,
    dependencyOverrides += "com.google.guava" % "guava" % "33.3.1-jre",
    excludeDependencies += "org.slf4j" % "slf4j-log4j12" % "1.7.36",
    libraryDependencies ++= Dependencies.common,
    addCompilerPlugin("org.typelevel" % "kind-projector" % "0.13.3" cross CrossVersion.full)
  )

// libraryDependencies += "commented" % "out" % "1.0"
/* "block" % "comment" % "1.0" */
val sharedTest = "org.scalameta" %% "munit" % "1.0.3" % Test
"#;

    #[test]
    fn build_definition_with_vals_cross_names_and_seq_blocks() {
        let items = Sbt.parse_file(&file("build.sbt", BUILD)).unwrap();
        assert_eq!(
            items[0].detail,
            "sbt-Builddefinition (nicht ausgeführt) · Scala 2.13.15 · Cross-Build 2.13.15, 3.3.4 · 1 Abhängigkeitslisten aus Scala-Code (nicht gelesen) · Resolver: 1 öffentlich, privat: nexus.corp.example"
        );
        let debug = format!("{items:?}");
        for secret in ["t0ken", "hunter2", "deploy", "not-a-version", "commented", "block"] {
            assert!(!debug.contains(secret), "{secret} leaked");
        }
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let scalatest = find("org.scalatest:scalatest");
        assert_eq!(
            (scalatest.evidence.context.as_str(), scalatest.evidence.qualifier.as_str()),
            ("scala-cross", "test")
        );
        assert_eq!(scalatest.detail, "Spezifikation 3.2.19 · test · Scala-Suffix nicht aufgelöst");
        let circe = find("io.circe:circe-core");
        assert_eq!((circe.version.as_deref(), circe.evidence.context.as_str()), (Some("0.14.10"), "scala-cross"));
        assert_eq!(circe.detail, "Spezifikation 0.14.10 aus val circeVersion · Scala-Suffix nicht aufgelöst");
        assert_eq!(find("io.circe:circe-generic").version.as_deref(), Some("0.14.10"));
        assert_eq!(find("org.apache.pekko:pekko-actor-typed").version.as_deref(), Some("1.1.2"));
        let postgres = find("org.postgresql:postgresql");
        assert_eq!((postgres.version.as_deref(), postgres.evidence.context.as_str()), (Some("42.7.4"), "compile"));
        assert!(find("org.typelevel:cats-core").detail.ends_with("Scala.js-/Native-Suffix nicht aufgelöst"));
        let secret = find("com.example:secret-lib");
        assert!(secret.version.is_none() && secret.detail == "Version aus Variable");
        assert_eq!(find("javax.servlet:javax.servlet-api").evidence.context, "provided");
        assert_eq!(find("ch.qos.logback:logback-classic").evidence.context, "runtime", "continued after +=");
        let guava = find("com.google.guava:guava");
        assert_eq!(
            (guava.evidence.context.as_str(), guava.detail.as_str()),
            ("override", "Override · Spezifikation 33.3.1-jre")
        );
        assert!(!items.iter().any(|i| i.name == "org.slf4j:slf4j-log4j12"), "exclusions are no dependencies");
        assert_eq!(find("org.typelevel:kind-projector").evidence.context, "compiler-plugin");
        let munit = find("org.scalameta:munit");
        assert_eq!(munit.evidence.qualifier, "test|val");
        assert!(munit.detail.ends_with("außerhalb von libraryDependencies definiert"));
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared) && i.ecosystem == "sbt"));
        let keys: std::collections::BTreeSet<String> = items.iter().map(InventoryItem::key).collect();
        assert_eq!(keys.len(), items.len());
    }

    #[test]
    fn plugins_in_the_meta_build_and_claims() {
        let plugins = "// formatting\naddSbtPlugin(\"org.scalameta\" % \"sbt-scalafmt\" % \"2.5.2\")\naddSbtPlugin(\"com.github.sbt\" % \"sbt-native-packager\" % \"1.10.4\")\nval assemblyVersion = \"2.3.0\"\naddSbtPlugin(\"com.eed3si9n\" % \"sbt-assembly\" % assemblyVersion)\n";
        let mut found = file("plugins.sbt", plugins);
        found.rel_dir = "project";
        let items = Sbt.parse_file(&found).unwrap();
        assert_eq!(items[0].detail, "sbt-Metabuild (nicht ausgeführt)");
        assert_eq!(items.len(), 4);
        assert!(items.iter().skip(1).all(|i| i.evidence.context == "plugin" && i.detail.starts_with("sbt-Plugin · ")));
        let assembly = items.iter().find(|i| i.name == "com.eed3si9n:sbt-assembly").unwrap();
        assert_eq!(assembly.version.as_deref(), Some("2.3.0"));

        found.rel_dir = "src/sbt-test/plugin/simple";
        assert!(Sbt.parse_file(&found).unwrap().is_empty(), "scripted test fixtures are skipped");

        let root: Vec<String> = ["build.sbt", "project", "src"].map(String::from).to_vec();
        let meta: Vec<String> = ["build.properties", "plugins.sbt", "target"].map(String::from).to_vec();
        assert!(Sbt.claim_dir("target", &root, &[]).is_some());
        assert!(Sbt.claim_dir("target", &meta, &[]).is_some(), "project/target");
        assert!(Sbt.claim_dir("project", &meta, &["target".into()]).is_some(), "project/project");
        assert!(Sbt.claim_dir("project", &root, &["build.properties".into()]).is_none(), "the meta-build itself");
        let ant: Vec<String> = ["build.properties", "build.xml"].map(String::from).to_vec();
        assert!(Sbt.claim_dir("project", &ant, &[]).is_none() && Sbt.claim_dir("target", &ant, &[]).is_none());
    }
}
