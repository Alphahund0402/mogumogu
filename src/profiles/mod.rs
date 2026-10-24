//! Declarative AI profile catalog (F-10, F-19; PROJEKTPLAN §8.2–§8.4).
//!
//! The catalog is embedded at build time and validated on load: profiles
//! from scanned repositories are never installed. A profile can only name
//! relative patterns, a fixed parser and lower limits; there are no fields
//! for commands, URLs to fetch or selectors that could execute anything.
//! Found instructions, skills, hooks and MCP entries are *data*.
mod parse;

pub use parse::{RefCheck, parse_detection};

use crate::limits::AI_FILE_MAX_BYTES;
use serde::Deserialize;
use std::sync::LazyLock;

const EMBEDDED: &str = include_str!("../../profiles/ai-catalog.toml");
const MAX_SEGMENTS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Parser {
    Skill,
    Instructions,
    McpJson,
    McpJsonc,
    SettingsJson,
    CodexToml,
    Rules,
    Presence,
    /// An ignore file of an AI tool: lines are counted, never stored.
    Ignore,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detection {
    pub pattern: String,
    pub parser: Parser,
    pub kind: String,
    #[serde(default)]
    pub mcp_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub status: crate::adapters::Support,
    pub source: String,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    pub detect: Vec<Detection>,
}

impl Profile {
    /// Effective per-file limit: a profile may only tighten the host limit.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes.unwrap_or(AI_FILE_MAX_BYTES).min(AI_FILE_MAX_BYTES)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub version: String,
    #[serde(rename = "profile")]
    pub profiles: Vec<Profile>,
}

static BUILT_IN: LazyLock<Catalog> =
    LazyLock::new(|| Catalog::from_toml(EMBEDDED).expect("embedded AI catalog must be valid"));

/// The validated built-in catalog.
pub fn catalog() -> &'static Catalog {
    &BUILT_IN
}

impl Catalog {
    /// Parses and validates a catalog. Used for the embedded catalog and for
    /// contributor tests of new formats; never for files found in projects.
    pub fn from_toml(text: &str) -> crate::Result<Self> {
        let catalog: Catalog =
            toml::from_str(text).map_err(|e| crate::Error::invalid(format!("KI-Katalog ungültig: {}", e.message())))?;
        catalog.validate()?;
        Ok(catalog)
    }

    fn validate(&self) -> crate::Result<()> {
        let invalid = |m: String| Err(crate::Error::invalid(format!("KI-Katalog: {m}")));
        if self.schema != 1 {
            return invalid(format!("Schema {} wird nicht unterstützt", self.schema));
        }
        let mut ids = std::collections::HashSet::new();
        for profile in &self.profiles {
            let id_ok = !profile.id.is_empty()
                && profile.id.len() <= 40
                && profile.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !id_ok || !ids.insert(profile.id.as_str()) {
                return invalid(format!("ungültige oder doppelte Profil-ID {}", profile.id));
            }
            if profile.max_bytes.is_some_and(|m| m > AI_FILE_MAX_BYTES) {
                return invalid(format!("{}: Limit über Hostlimit", profile.id));
            }
            if profile.detect.is_empty() {
                return invalid(format!("{}: keine Erkennungsmuster", profile.id));
            }
            for detection in &profile.detect {
                validate_pattern(&detection.pattern).or_else(|m| invalid(format!("{}: {m}", profile.id)))?;
                let needs_key = matches!(detection.parser, Parser::McpJson | Parser::McpJsonc);
                if needs_key != detection.mcp_key.is_some() {
                    return invalid(format!("{}: mcp_key passt nicht zum Parser", profile.id));
                }
            }
        }
        Ok(())
    }

    /// Comparison basis for generations: a catalog change starts a new baseline.
    pub fn basis(&self) -> String {
        format!("{}+ai-{}", crate::adapters::ADAPTERS_VERSION, self.version)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// All detections whose pattern matches the end of `rel_path`.
    pub fn matches<'a>(&'a self, rel_path: &str) -> Vec<(&'a Profile, &'a Detection)> {
        let segments: Vec<&str> = rel_path.split('/').collect();
        let mut out = Vec::new();
        for profile in &self.profiles {
            for detection in &profile.detect {
                let pattern: Vec<&str> = detection.pattern.split('/').collect();
                if pattern.len() <= segments.len()
                    && pattern
                        .iter()
                        .zip(&segments[segments.len() - pattern.len()..])
                        .all(|(p, s)| segment_matches(p, s))
                {
                    out.push((profile, detection));
                }
            }
        }
        out
    }

    /// Whether any pattern could match a file with this name (cheap prefilter).
    pub fn may_match(&self, file_name: &str) -> bool {
        self.profiles
            .iter()
            .flat_map(|p| &p.detect)
            .any(|d| d.pattern.rsplit('/').next().is_some_and(|last| segment_matches(last, file_name)))
    }

    /// Directory routes below a user folder that stands for `prefix`
    /// (`.claude`, …): only these subfolders are ever entered there.
    pub fn user_routes(&self, prefix: &str) -> Vec<Vec<String>> {
        let mut routes: Vec<Vec<String>> = Vec::new();
        for detection in self.profiles.iter().flat_map(|p| &p.detect) {
            let segments: Vec<&str> = detection.pattern.split('/').collect();
            if segments.len() > 2 && segments[0].eq_ignore_ascii_case(prefix) {
                let route: Vec<String> = segments[1..segments.len() - 1].iter().map(|s| s.to_string()).collect();
                if !routes.contains(&route) {
                    routes.push(route);
                }
            }
        }
        routes
    }
}

/// Whether a relative directory lies on one of the routes (or above its end).
pub fn route_allows(routes: &[Vec<String>], rel_dir: &str) -> bool {
    let parts: Vec<&str> = rel_dir.split('/').collect();
    routes.iter().any(|route| {
        parts.len() <= route.len() && parts.iter().zip(route).all(|(part, pattern)| segment_matches(pattern, part))
    })
}

fn validate_pattern(pattern: &str) -> Result<(), String> {
    let segments: Vec<&str> = pattern.split('/').collect();
    if pattern.is_empty() || segments.len() > MAX_SEGMENTS {
        return Err(format!("Muster {pattern} leer oder zu tief"));
    }
    for segment in &segments {
        let bad = segment.is_empty()
            || *segment == "."
            || *segment == ".."
            || segment.contains("**")
            || segment.matches('*').count() > 1
            || segment.chars().any(|c| matches!(c, '\\' | ':' | '?' | '"' | '<' | '>' | '|') || c.is_control());
        if bad {
            return Err(format!("Muster {pattern} enthält ein unzulässiges Segment"));
        }
    }
    Ok(())
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            segment.len() >= prefix.len() + suffix.len()
                && segment.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
                && segment.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase())
        }
        None => pattern.eq_ignore_ascii_case(segment),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_covers_all_agreed_families() {
        let ids: Vec<&str> = catalog().profiles.iter().map(|p| p.id.as_str()).collect();
        for id in [
            "agent-skills",
            "codex",
            "claude-code",
            "copilot",
            "cursor",
            "gemini",
            "windsurf",
            "cline",
            "roo",
            "opencode",
            "continue",
            "aider",
        ] {
            assert!(ids.contains(&id), "{id}");
        }
    }

    #[test]
    fn matching_is_suffix_based_and_shared_files_have_several_consumers() {
        let consumers: Vec<&str> =
            catalog().matches("packages/web/AGENTS.md").iter().map(|(p, _)| p.id.as_str()).collect();
        assert!(consumers.contains(&"codex") && consumers.contains(&"cursor"));
        let skill: Vec<&str> =
            catalog().matches("x/.claude/skills/review/SKILL.md").iter().map(|(p, _)| p.id.as_str()).collect();
        assert_eq!(skill, vec!["claude-code"], "a client folder is not claimed by another client");
        assert!(catalog().matches("src/main.rs").is_empty());
        assert!(catalog().may_match("SKILL.md"));
    }

    #[test]
    fn user_folders_are_entered_only_along_catalog_routes() {
        let routes = catalog().user_routes(".claude");
        for allowed in ["skills", "skills/review", "agents", "commands", "rules"] {
            assert!(route_allows(&routes, allowed), "{allowed}");
        }
        for denied in ["projects", "todos", "shell-snapshots", "skills/review/scripts", "statsig"] {
            assert!(!route_allows(&routes, denied), "{denied}");
        }
        assert!(catalog().user_routes(".codex").is_empty(), "only config.toml and AGENTS.md at the root");
        assert!(route_allows(&catalog().user_routes(".roo"), "rules-code"), "one wildcard per segment");
    }

    #[test]
    fn contributed_profile_without_core_change_and_rejections() {
        let ok = r#"schema = 1
version = "test"
[[profile]]
id = "synthetic"
name = "Synthetisches Format"
status = "experimental"
source = "fixture"
max_bytes = 4096
detect = [{ pattern = ".synthetic/*.rules.md", parser = "rules", kind = "Regel" }]
"#;
        let catalog = Catalog::from_toml(ok).unwrap();
        assert_eq!(catalog.matches("a/.synthetic/x.rules.md").len(), 1);
        for bad in [
            ok.replace(".synthetic/*.rules.md", "../outside/*.md"),
            ok.replace(".synthetic/*.rules.md", "C:/abs.md"),
            ok.replace(".synthetic/*.rules.md", "**/x.md"),
            ok.replace("max_bytes = 4096", "max_bytes = 999999999"),
            ok.replace("kind = \"Regel\"", "kind = \"Regel\", command = \"rm -rf\""),
            ok.replace("parser = \"rules\"", "parser = \"shell\""),
        ] {
            assert!(Catalog::from_toml(&bad).is_err(), "{bad}");
        }
    }
}
