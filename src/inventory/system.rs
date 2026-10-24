//! Metadata-only system check (docs/SYSTEMERKENNUNG.md). Which tools lie on
//! the search path, which known global stores and user configuration folders
//! exist. Nothing is opened for reading, started or followed: entries are
//! checked with `symlink_metadata`, so links and app aliases are reported as
//! such, never resolved. Content needs its own identity-bound read approval.
use crate::domain::{FindingEvidence, FindingGroup, ScopeKind, SystemFinding, SystemReport};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Search path entries checked per pass; later entries count as unchecked.
const MAX_PATH_ENTRIES: usize = 256;
/// The whole pass stops after this; what is left stays unchecked.
const TIME_BUDGET: Duration = Duration::from_secs(2);
const REPARSE_POINT: u32 = 0x400;

/// Environment lookups, injectable for tests.
pub trait Env {
    fn var(&self, name: &str) -> Option<OsString>;
}

/// The owner's own process environment.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name).filter(|value| !value.is_empty())
    }
}

struct Tool {
    id: &'static str,
    name: &'static str,
    files: &'static [&'static str],
}

/// Command names, not installations: a hit says a shell would find a file.
const TOOLS: &[Tool] = &[
    Tool { id: "node", name: "Node.js", files: &["node.exe"] },
    Tool { id: "npm", name: "npm", files: &["npm.cmd", "npm.exe"] },
    Tool { id: "pnpm", name: "pnpm", files: &["pnpm.exe", "pnpm.cmd"] },
    Tool { id: "python", name: "python", files: &["python.exe", "python.bat", "python.cmd"] },
    Tool { id: "py", name: "Python-Launcher (py)", files: &["py.exe"] },
    Tool { id: "pip", name: "pip", files: &["pip.exe", "pip.bat", "pip.cmd"] },
    Tool { id: "uv", name: "uv · uvx", files: &["uv.exe"] },
    Tool { id: "cargo", name: "Cargo", files: &["cargo.exe"] },
    Tool { id: "rustup", name: "rustup", files: &["rustup.exe"] },
    Tool { id: "dotnet", name: ".NET (dotnet)", files: &["dotnet.exe"] },
    Tool { id: "go", name: "Go", files: &["go.exe"] },
    Tool { id: "bun", name: "Bun", files: &["bun.exe"] },
    Tool { id: "yarn", name: "Yarn", files: &["yarn.cmd", "yarn.exe"] },
    Tool { id: "gradle", name: "Gradle", files: &["gradle.bat", "gradle.exe"] },
    Tool { id: "mvn", name: "Maven (mvn)", files: &["mvn.cmd", "mvn.exe"] },
    Tool { id: "git", name: "Git", files: &["git.exe"] },
    Tool { id: "gh", name: "GitHub CLI", files: &["gh.exe"] },
    Tool { id: "docker", name: "Docker", files: &["docker.exe"] },
    Tool { id: "code", name: "VS Code", files: &["code.cmd"] },
    Tool { id: "codex", name: "Codex", files: &["codex.cmd", "codex.exe"] },
    Tool { id: "claude", name: "Claude Code", files: &["claude.exe", "claude.cmd"] },
    Tool { id: "opencode", name: "OpenCode", files: &["opencode.cmd", "opencode.exe"] },
    Tool { id: "gemini", name: "Gemini CLI", files: &["gemini.cmd"] },
];

/// Launcher folders: the file found is a forwarder, not the tool itself.
const SHIM_DIRS: &[(&[&str], &str)] = &[
    (&[".pyenv", "pyenv-win", "shims"], "pyenv-Shim"),
    (&["scoop", "shims"], "Scoop-Shim"),
    (&["chocolatey", "bin"], "Chocolatey-Shim"),
    (&[".volta", "bin"], "Volta-Shim"),
    (&["mise", "shims"], "mise-Shim"),
    (&[".cargo", "bin"], "rustup-Proxy oder Cargo-Tool"),
    (&["Microsoft", "WindowsApps"], "App-Ausführungsalias"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    UserProfile,
    AppData,
    LocalAppData,
    Temp,
}

impl Base {
    fn var(self) -> &'static str {
        match self {
            Self::UserProfile => "USERPROFILE",
            Self::AppData => "APPDATA",
            Self::LocalAppData => "LOCALAPPDATA",
            Self::Temp => "TEMP",
        }
    }
}

/// Where tools keep quick tries. A names-only search of each needs its own
/// approval; none of them is part of a drive search, which leaves AppData out.
const SCRATCH_PLACES: &[Place] = &[
    Place {
        id: "claude-temp",
        group: FindingGroup::Scratch,
        name: "Arbeitsordner von Claude Code",
        moved_by: None,
        candidates: &[(Base::Temp, &["claude"])],
        markers: &[],
        scope: Some(ScopeKind::ProjectSearch),
        note: "Scratchpads und Ausgaben von KI-Sitzungen; nur Namen werden gelesen",
    },
    Place {
        id: "jetbrains-scratches",
        group: FindingGroup::Scratch,
        name: "JetBrains-Scratches",
        moved_by: None,
        candidates: &[(Base::AppData, &["JetBrains"])],
        markers: &[],
        scope: Some(ScopeKind::ProjectSearch),
        note: "Je IDE ein Ordner scratches; nur Namen werden gelesen",
    },
    Place {
        id: "temp",
        group: FindingGroup::Scratch,
        name: "Temporärer Ordner",
        moved_by: None,
        candidates: &[(Base::Temp, &[])],
        markers: &[],
        scope: Some(ScopeKind::ProjectSearch),
        note: "Werkzeuge legen hier Versuche und Builds ab; vieles ist kurzlebig",
    },
];

/// A known place below a Windows base folder, optionally moved by a variable.
struct Place {
    id: &'static str,
    group: FindingGroup,
    name: &'static str,
    /// Variable that relocates the place; its value is used as the folder.
    moved_by: Option<&'static str>,
    candidates: &'static [(Base, &'static [&'static str])],
    /// Any of these below the place upgrades the evidence to "installed".
    markers: &'static [&'static str],
    scope: Option<ScopeKind>,
    note: &'static str,
}

const STORES: &[Place] = &[
    Place {
        id: "npm-global",
        group: FindingGroup::Store,
        name: "Globale npm-Pakete",
        moved_by: None,
        candidates: &[(Base::AppData, &["npm", "node_modules"])],
        markers: &[],
        scope: Some(ScopeKind::NpmGlobal),
        note: "Paketordner; Prefix-Overrides können ihn verlegen",
    },
    Place {
        id: "uv-tools",
        group: FindingGroup::Store,
        name: "uv-Toolumgebungen",
        moved_by: Some("UV_TOOL_DIR"),
        candidates: &[(Base::AppData, &["uv", "data", "tools"]), (Base::AppData, &["uv", "tools"])],
        markers: &[],
        scope: Some(ScopeKind::UvTools),
        note: "Toolumgebungen mit uv-receipt.toml",
    },
    Place {
        id: "uv-python",
        group: FindingGroup::Store,
        name: "uv-verwaltete Python-Installationen",
        moved_by: Some("UV_PYTHON_INSTALL_DIR"),
        candidates: &[(Base::AppData, &["uv", "data", "python"]), (Base::AppData, &["uv", "python"])],
        markers: &[],
        scope: None,
        note: "Ordner belegen keine aktive Python-Auswahl",
    },
    Place {
        id: "cargo-home",
        group: FindingGroup::Store,
        name: "Cargo-Installationen",
        moved_by: Some("CARGO_HOME"),
        candidates: &[(Base::UserProfile, &[".cargo"])],
        markers: &[".crates2.json", ".crates.toml"],
        scope: Some(ScopeKind::CargoTools),
        note: "Installationsdatensätze; bin enthält auch rustup-Proxys",
    },
    Place {
        id: "rustup-home",
        group: FindingGroup::Store,
        name: "rustup-Toolchains",
        moved_by: Some("RUSTUP_HOME"),
        candidates: &[(Base::UserProfile, &[".rustup"])],
        markers: &["toolchains"],
        scope: None,
        note: "Toolchains; settings.toml ist keine stabile Schnittstelle",
    },
    Place {
        id: "nuget-cache",
        group: FindingGroup::Store,
        name: "NuGet-Paketordner",
        moved_by: Some("NUGET_PACKAGES"),
        candidates: &[(Base::UserProfile, &[".nuget", "packages"])],
        markers: &[],
        scope: Some(ScopeKind::NugetCache),
        note: "Gemeinsamer Speicher, keine Projektzuordnung",
    },
    Place {
        id: "dotnet-tools",
        group: FindingGroup::Store,
        name: "Globale .NET-Tools",
        moved_by: None,
        candidates: &[(Base::UserProfile, &[".dotnet", "tools"])],
        markers: &[],
        scope: None,
        note: "Toolinstallationen; --tool-path kann weitere Orte nutzen",
    },
    Place {
        id: "pyenv",
        group: FindingGroup::Store,
        name: "pyenv-win",
        moved_by: None,
        candidates: &[(Base::UserProfile, &[".pyenv", "pyenv-win"])],
        markers: &["version"],
        scope: None,
        note: "Interpreterauswahl ungeprüft; nichts wird gestartet",
    },
    Place {
        id: "npm-cache",
        group: FindingGroup::Store,
        name: "npm-Cache",
        moved_by: Some("npm_config_cache"),
        candidates: &[(Base::LocalAppData, &["npm-cache"])],
        markers: &[],
        scope: Some(ScopeKind::NpmCache),
        note: "Heruntergeladene Pakete; wird nur gemessen",
    },
    Place {
        id: "pip-cache",
        group: FindingGroup::Store,
        name: "pip-Cache",
        moved_by: Some("PIP_CACHE_DIR"),
        candidates: &[(Base::LocalAppData, &["pip", "Cache"])],
        markers: &[],
        scope: Some(ScopeKind::PipCache),
        note: "Heruntergeladene Pakete und Builds; wird nur gemessen",
    },
    Place {
        id: "uv-cache",
        group: FindingGroup::Store,
        name: "uv-Cache",
        moved_by: Some("UV_CACHE_DIR"),
        candidates: &[(Base::LocalAppData, &["uv", "cache"])],
        markers: &[],
        scope: Some(ScopeKind::UvCache),
        note: "Pakete und Builds; wird nur gemessen",
    },
    Place {
        id: "cargo-registry",
        group: FindingGroup::Store,
        name: "Cargo-Registry",
        moved_by: None,
        candidates: &[(Base::UserProfile, &[".cargo", "registry"])],
        markers: &[],
        scope: Some(ScopeKind::CargoRegistry),
        note: "Heruntergeladene Crates und entpackte Quellen; wird nur gemessen",
    },
    Place {
        id: "gradle-caches",
        group: FindingGroup::Store,
        name: "Gradle-Caches",
        moved_by: None,
        candidates: &[(Base::UserProfile, &[".gradle", "caches"])],
        markers: &[],
        scope: Some(ScopeKind::GradleCaches),
        note: "Abhängigkeiten und Build-Caches; wird nur gemessen",
    },
    Place {
        id: "maven-repository",
        group: FindingGroup::Store,
        name: "Maven-Repository",
        moved_by: None,
        candidates: &[(Base::UserProfile, &[".m2", "repository"])],
        markers: &[],
        scope: Some(ScopeKind::MavenRepository),
        note: "Kann auch selbst installierte Artefakte enthalten; wird nur gemessen",
    },
    Place {
        id: "pnpm-store",
        group: FindingGroup::Store,
        name: "pnpm-Store",
        moved_by: None,
        candidates: &[(Base::LocalAppData, &["pnpm", "store"])],
        markers: &[],
        scope: Some(ScopeKind::PnpmStore),
        note: "Gemeinsamer Paketspeicher; Projekte verlinken hinein; wird nur gemessen",
    },
    Place {
        id: "yarn-cache",
        group: FindingGroup::Store,
        name: "Yarn-Cache",
        moved_by: None,
        candidates: &[(Base::LocalAppData, &["Yarn", "Cache"]), (Base::LocalAppData, &["Yarn", "Berry", "cache"])],
        markers: &[],
        scope: Some(ScopeKind::YarnCache),
        note: "Heruntergeladene Pakete; wird nur gemessen",
    },
];

/// User configuration folders of AI clients. `prefix` maps the folder to the
/// catalog's project layout, so `.claude/skills/*/SKILL.md` matches inside
/// `%USERPROFILE%\.claude` without approving the whole profile.
#[derive(Debug)]
pub struct UserAiRoot {
    pub id: &'static str,
    pub name: &'static str,
    pub prefix: &'static str,
    moved_by: Option<&'static str>,
    base: Base,
    rel: &'static [&'static str],
}

pub const USER_AI_ROOTS: &[UserAiRoot] = &[
    UserAiRoot {
        id: "claude",
        name: "Claude Code",
        prefix: ".claude",
        moved_by: Some("CLAUDE_CONFIG_DIR"),
        base: Base::UserProfile,
        rel: &[".claude"],
    },
    UserAiRoot {
        id: "codex",
        name: "Codex",
        prefix: ".codex",
        moved_by: Some("CODEX_HOME"),
        base: Base::UserProfile,
        rel: &[".codex"],
    },
    UserAiRoot {
        id: "agents",
        name: "Agent Skills (persönlich)",
        prefix: ".agents",
        moved_by: None,
        base: Base::UserProfile,
        rel: &[".agents"],
    },
    UserAiRoot {
        id: "gemini",
        name: "Gemini CLI",
        prefix: ".gemini",
        moved_by: None,
        base: Base::UserProfile,
        rel: &[".gemini"],
    },
    UserAiRoot {
        id: "cursor",
        name: "Cursor",
        prefix: ".cursor",
        moved_by: None,
        base: Base::UserProfile,
        rel: &[".cursor"],
    },
    UserAiRoot {
        id: "continue",
        name: "Continue",
        prefix: ".continue",
        moved_by: None,
        base: Base::UserProfile,
        rel: &[".continue"],
    },
    UserAiRoot { id: "roo", name: "Roo Code", prefix: ".roo", moved_by: None, base: Base::UserProfile, rel: &[".roo"] },
    UserAiRoot {
        id: "opencode",
        name: "OpenCode",
        prefix: ".opencode",
        moved_by: Some("OPENCODE_CONFIG_DIR"),
        base: Base::UserProfile,
        rel: &[".config", "opencode"],
    },
    UserAiRoot {
        id: "vscode",
        name: "VS Code (Benutzer-MCP)",
        prefix: ".vscode",
        moved_by: None,
        base: Base::AppData,
        rel: &["Code", "User"],
    },
];

impl UserAiRoot {
    pub fn path(&self, env: &dyn Env) -> Option<PathBuf> {
        if let Some(moved) = self.moved_by.and_then(|name| env.var(name)) {
            return Some(PathBuf::from(moved));
        }
        env.var(self.base.var()).map(|base| self.rel.iter().fold(PathBuf::from(base), |p, c| p.join(c)))
    }
}

/// The catalog prefix of an approved user folder. A relocated folder keeps
/// its meaning through the variable that relocated it.
pub fn user_ai_prefix(path: &Path, env: &dyn Env) -> Option<&'static str> {
    let text = path.to_string_lossy();
    if let Some(root) =
        USER_AI_ROOTS.iter().find(|r| r.path(env).is_some_and(|p| p.to_string_lossy().eq_ignore_ascii_case(&text)))
    {
        return Some(root.prefix);
    }
    // Fallback by folder name for dot folders only; `Code\User` needs the table.
    let last = path.file_name()?.to_string_lossy().to_string();
    USER_AI_ROOTS
        .iter()
        .filter(|r| r.rel.last().is_some_and(|c| c.starts_with('.') || *c == "opencode"))
        .find(|r| {
            r.rel.last().is_some_and(|c| c.trim_start_matches('.').eq_ignore_ascii_case(last.trim_start_matches('.')))
        })
        .map(|r| r.prefix)
}

/// Writes a local path with Windows placeholders, so lists and exports do
/// not carry the user name. Longest base first: APPDATA lies below the profile.
pub fn placeholder_path(path: &Path, env: &dyn Env) -> String {
    let text = path.to_string_lossy().to_string();
    let mut bases: Vec<(&str, String)> =
        ["TEMP", "LOCALAPPDATA", "APPDATA", "ProgramData", "USERPROFILE", "ProgramFiles"]
            .iter()
            .filter_map(|name| env.var(name).map(|v| (*name, v.to_string_lossy().trim_end_matches('\\').to_string())))
            .filter(|(_, v)| !v.is_empty())
            .collect();
    bases.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    for (name, base) in bases {
        let lower = text.to_lowercase();
        let base_lower = base.to_lowercase();
        if lower == base_lower || lower.starts_with(&format!("{base_lower}\\")) {
            return format!("%{name}%{}", &text[base.len()..]);
        }
    }
    text
}

/// What a check may touch: metadata of a named entry, never content.
pub trait Probe {
    /// `None`: missing or not checkable. `Some(attributes)` otherwise.
    fn attributes(&self, path: &Path) -> Option<u32>;
    /// Only local fixed drives are checked; network and removable media could block.
    fn checkable(&self, path: &Path) -> bool;
}

/// Real filesystem metadata without following links.
#[derive(Clone, Copy, Debug, Default)]
pub struct MetadataProbe;

impl Probe for MetadataProbe {
    fn attributes(&self, path: &Path) -> Option<u32> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Some(metadata.file_attributes())
        }
        #[cfg(not(windows))]
        {
            Some(if metadata.is_dir() { crate::platform::attributes::DIRECTORY } else { 0 })
        }
    }
    fn checkable(&self, path: &Path) -> bool {
        crate::platform::local_fixed_path(path)
    }
}

/// One pass over the search path and the known places.
pub fn discover(env: &dyn Env, probe: &dyn Probe) -> SystemReport {
    let started = Instant::now();
    let mut report = SystemReport::default();
    tools(env, probe, started, &mut report);
    for place in STORES.iter().chain(SCRATCH_PLACES) {
        place_finding(place, env, probe, &mut report);
    }
    for root in USER_AI_ROOTS {
        let Some(path) = root.path(env) else { continue };
        report.locations += 1;
        if !probe.checkable(&path) {
            continue;
        }
        let Some(attributes) = probe.attributes(&path) else { continue };
        let (evidence, note) = if attributes & REPARSE_POINT != 0 {
            (FindingEvidence::Alias, "Verknüpfung: wird nicht verfolgt".to_string())
        } else {
            (FindingEvidence::Location, "Inhalt ungelesen; Zugangsdaten und Verläufe werden nie gelesen".to_string())
        };
        report.findings.push(SystemFinding {
            id: format!("ai:{}", root.id),
            group: FindingGroup::AiConfig,
            name: root.name.into(),
            location: placeholder_path(&path, env),
            path: path.to_string_lossy().into_owned(),
            evidence,
            note,
            scope: (evidence == FindingEvidence::Location).then_some(ScopeKind::UserAi),
            scope_id: None,
            coverage: String::new(),
        });
    }
    report
}

fn tools(env: &dyn Env, probe: &dyn Probe, started: Instant, report: &mut SystemReport) {
    let Some(path) = env.var("PATH") else { return };
    let mut seen: Vec<String> = Vec::new();
    let mut found: Vec<(&Tool, PathBuf, u32)> = Vec::new();
    for dir in std::env::split_paths(&path) {
        let key = dir.to_string_lossy().trim_end_matches('\\').to_lowercase();
        // Relative, unexpanded and network entries are never checked.
        if key.is_empty() || seen.contains(&key) || !dir.is_absolute() || key.contains('%') || key.starts_with("\\\\") {
            continue;
        }
        seen.push(key);
        if seen.len() > MAX_PATH_ENTRIES || started.elapsed() > TIME_BUDGET || !probe.checkable(&dir) {
            continue;
        }
        report.path_entries += 1;
        for tool in TOOLS {
            if found.iter().any(|(f, ..)| f.id == tool.id) {
                continue;
            }
            if let Some((file, attributes)) =
                tool.files.iter().find_map(|name| probe.attributes(&dir.join(name)).map(|a| (dir.join(name), a)))
                && attributes & crate::platform::attributes::DIRECTORY == 0
            {
                found.push((tool, file, attributes));
            }
        }
    }
    let pyenv_selected =
        env.var("USERPROFILE").map(|home| PathBuf::from(home).join(".pyenv").join("pyenv-win").join("version"));
    for (tool, file, attributes) in found {
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let shim = SHIM_DIRS.iter().find(|(tail, _)| ends_with(&dir, tail)).map(|(_, label)| *label);
        // Version switchers such as nvm-windows link the search path folder itself.
        let linked_dir = probe.attributes(&dir).is_some_and(|a| a & REPARSE_POINT != 0);
        let (evidence, mut note) = if attributes & REPARSE_POINT != 0 {
            (FindingEvidence::Alias, "Verknüpfung oder App-Alias, nicht verfolgt".to_string())
        } else if let Some(label) = shim {
            (FindingEvidence::Shim, label.to_string())
        } else if linked_dir {
            (FindingEvidence::Shim, "Suchpfad-Ordner ist eine Verknüpfung (Versionsumschalter)".to_string())
        } else {
            (FindingEvidence::OnPath, "Im Suchpfad gefunden".to_string())
        };
        if tool.id == "python" && shim == Some("pyenv-Shim") {
            let selected = pyenv_selected.as_deref().is_some_and(|p| probe.attributes(p).is_some());
            note = if selected {
                "pyenv-Shim · Auswahldatei vorhanden, ungelesen".into()
            } else {
                "pyenv-Shim · keine globale Version gewählt".into()
            };
        }
        if tool.id == "python" && shim == Some("App-Ausführungsalias") {
            note = "Store-Alias; kein Interpreter belegt".into();
        }
        note.push_str(" · nicht gestartet");
        report.findings.push(SystemFinding {
            id: format!("tool:{}", tool.id),
            group: FindingGroup::Tool,
            name: tool.name.into(),
            location: placeholder_path(&file, env),
            path: file.to_string_lossy().into_owned(),
            evidence,
            note,
            scope: None,
            scope_id: None,
            coverage: String::new(),
        });
    }
}

fn place_finding(place: &Place, env: &dyn Env, probe: &dyn Probe, report: &mut SystemReport) {
    let moved = place.moved_by.and_then(|name| env.var(name)).map(PathBuf::from);
    let candidates: Vec<PathBuf> = match &moved {
        Some(path) => vec![path.clone()],
        None => place
            .candidates
            .iter()
            .filter_map(|(base, rel)| env.var(base.var()).map(|b| rel.iter().fold(PathBuf::from(b), |p, c| p.join(c))))
            .collect(),
    };
    // npm can be relocated through its prefix; packages then sit below it.
    let candidates = match (place.id, env.var("npm_config_prefix")) {
        ("npm-global", Some(prefix)) => vec![PathBuf::from(prefix).join("node_modules")],
        _ => candidates,
    };
    for (index, path) in candidates.iter().enumerate() {
        report.locations += 1;
        if !probe.checkable(path) {
            continue;
        }
        let Some(attributes) = probe.attributes(path) else { continue };
        if attributes & crate::platform::attributes::DIRECTORY == 0 {
            continue;
        }
        let linked = attributes & REPARSE_POINT != 0;
        let marked = place.markers.iter().any(|m| probe.attributes(&path.join(m)).is_some());
        let mut note = place.note.to_string();
        if moved.is_none() && index > 0 && place.candidates.len() > 1 {
            note = format!("Ort weicht vom aktuellen Standard ab · {note}");
        }
        let evidence = if linked {
            note = "Verknüpfung: wird nicht verfolgt".into();
            FindingEvidence::Alias
        } else if marked {
            FindingEvidence::Installed
        } else {
            FindingEvidence::Location
        };
        report.findings.push(SystemFinding {
            id: format!("store:{}:{index}", place.id),
            group: place.group,
            name: place.name.into(),
            location: placeholder_path(path, env),
            path: path.to_string_lossy().into_owned(),
            evidence,
            note,
            scope: place.scope.filter(|_| !linked),
            scope_id: None,
            coverage: String::new(),
        });
    }
}

fn ends_with(dir: &Path, tail: &[&str]) -> bool {
    let parts: Vec<String> = dir.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    parts.len() >= tail.len()
        && parts[parts.len() - tail.len()..].iter().zip(tail).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

// Windows path semantics (drive letters, `\`, PATHEXT); the portable core
// check on Linux skips them like the other Windows boundary tests.
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapEnv(HashMap<&'static str, OsString>);
    impl Env for MapEnv {
        fn var(&self, name: &str) -> Option<OsString> {
            self.0.get(name).cloned()
        }
    }
    struct LocalProbe;
    impl Probe for LocalProbe {
        fn attributes(&self, path: &Path) -> Option<u32> {
            MetadataProbe.attributes(path)
        }
        fn checkable(&self, _: &Path) -> bool {
            true
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn finds_tools_shims_stores_and_user_folders_without_content() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("Users").join("someone");
        let appdata = home.join("AppData").join("Roaming");
        let node = temp.path().join("nodejs");
        let shims = home.join(".pyenv").join("pyenv-win").join("shims");
        touch(&node.join("node.exe"));
        touch(&node.join("npm.cmd"));
        touch(&shims.join("python.bat"));
        touch(&home.join(".cargo").join(".crates2.json"));
        touch(&appdata.join("uv").join("tools").join("ruff").join("uv-receipt.toml"));
        touch(&home.join(".claude").join("settings.json"));
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let path = std::env::join_paths([node.clone(), shims.clone(), PathBuf::from("relative\\bin")]).unwrap();
        let env = MapEnv(HashMap::from([
            ("PATH", path),
            ("USERPROFILE", home.clone().into_os_string()),
            ("APPDATA", appdata.clone().into_os_string()),
        ]));
        let report = discover(&env, &LocalProbe);
        let by = |id: &str| report.findings.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("{id}: {report:#?}"));
        assert_eq!(by("tool:node").evidence, FindingEvidence::OnPath);
        assert_eq!(by("tool:npm").evidence, FindingEvidence::OnPath);
        let python = by("tool:python");
        assert_eq!(python.evidence, FindingEvidence::Shim);
        assert!(python.note.contains("keine globale Version"), "{}", python.note);
        assert_eq!(python.location, r"%USERPROFILE%\.pyenv\pyenv-win\shims\python.bat");
        assert_eq!(by("store:cargo-home:0").evidence, FindingEvidence::Installed);
        assert_eq!(by("store:cargo-home:0").scope, Some(ScopeKind::CargoTools));
        let uv = by("store:uv-tools:1");
        assert!(uv.note.starts_with("Ort weicht"), "observed older uv layout is not a defect");
        assert_eq!(uv.location, r"%APPDATA%\uv\tools");
        assert_eq!(by("ai:claude").scope, Some(ScopeKind::UserAi));
        assert_eq!(by("ai:codex").location, r"%USERPROFILE%\.codex");
        assert!(report.findings.iter().all(|f| f.id != "ai:gemini"), "missing places are not listed");
        assert_eq!(report.path_entries, 2, "relative entries are never checked");
        assert!(report.findings.iter().all(|f| !f.location.contains("someone")), "no user name in locations");
    }

    #[test]
    fn tool_scratch_folders_are_offered_for_a_names_only_search() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("Users").join("someone");
        let appdata = home.join("AppData").join("Roaming");
        let tmp = home.join("AppData").join("Local").join("Temp");
        std::fs::create_dir_all(tmp.join("claude")).unwrap();
        std::fs::create_dir_all(appdata.join("JetBrains")).unwrap();
        let env = MapEnv(HashMap::from([
            ("USERPROFILE", home.clone().into_os_string()),
            ("APPDATA", appdata.into_os_string()),
            ("TEMP", tmp.into_os_string()),
        ]));
        let report = discover(&env, &LocalProbe);
        let scratch: Vec<&SystemFinding> = report.of(FindingGroup::Scratch).collect();
        assert_eq!(scratch.len(), 3, "{scratch:#?}");
        assert!(scratch.iter().all(|f| f.scope == Some(ScopeKind::ProjectSearch)));
        assert!(scratch.iter().any(|f| f.id == "store:claude-temp:0" && f.location == r"%TEMP%\claude"));
        assert!(report.findings.iter().all(|f| !f.location.contains("someone")), "no user name in locations");
    }

    #[test]
    fn package_caches_are_offered_with_their_own_approval() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("Users").join("someone");
        let local = home.join("AppData").join("Local");
        std::fs::create_dir_all(local.join("npm-cache")).unwrap();
        std::fs::create_dir_all(home.join(".m2").join("repository")).unwrap();
        let env = MapEnv(HashMap::from([
            ("USERPROFILE", home.clone().into_os_string()),
            ("LOCALAPPDATA", local.into_os_string()),
        ]));
        let report = discover(&env, &LocalProbe);
        let store =
            |id: &str| report.findings.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("{id}: {report:#?}"));
        assert_eq!(store("store:npm-cache:0").scope, Some(ScopeKind::NpmCache));
        assert_eq!(store("store:npm-cache:0").location, r"%LOCALAPPDATA%\npm-cache");
        assert_eq!(store("store:maven-repository:0").scope, Some(ScopeKind::MavenRepository));
        assert!(report.findings.iter().all(|f| f.id != "store:pip-cache:0"), "missing places are not listed");
    }

    #[test]
    fn user_folders_map_to_catalog_prefixes() {
        let env = MapEnv(HashMap::from([
            ("USERPROFILE", OsString::from(r"C:\Users\x")),
            ("CODEX_HOME", OsString::from(r"D:\cfg\codex-home")),
        ]));
        assert_eq!(user_ai_prefix(Path::new(r"C:\Users\x\.claude"), &env), Some(".claude"));
        assert_eq!(user_ai_prefix(Path::new(r"D:\cfg\codex-home"), &env), Some(".codex"));
        assert_eq!(user_ai_prefix(Path::new(r"C:\Users\x\.config\opencode"), &env), Some(".opencode"));
        assert_eq!(user_ai_prefix(Path::new(r"C:\Users\x\Documents"), &env), None);
    }
}
