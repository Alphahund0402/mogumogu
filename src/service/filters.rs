//! Context filters for AI tools in approved projects: a preview first, then
//! an append-only write that the user confirms with the preview's
//! fingerprint. Only the personal `.claude/settings.local.json` is ever
//! written, so the shared, usually committed `.claude/settings.json` stays as
//! the team keeps it. Existing entries stay, and a changed file needs a new
//! preview. The write goes through the held `.claude` folder handle: a new
//! temporary file there, renamed over the target, never through a link.
use super::Core;
use crate::domain::*;
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::{AI_FILE_MAX_BYTES, AI_MAX_NESTING, ScanLimits};
use crate::parsers::json;
use crate::platform::DirHandle;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

const LOCAL: &str = "settings.local.json";
const LOCAL_PATH: &str = ".claude/settings.local.json";
const SHARED: &str = "settings.json";
/// Secrets an agent never needs. No `!` exception: in Claude Code it would
/// lift earlier deny rules of the same file, including the user's own.
const SECRET_RULES: &[&str] = &["Read(.env)", "Read(.env.*)", "Read(*.pem)", "Read(*.key)"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterFile {
    pub tool: String,
    pub rel_path: String,
    pub exists: bool,
    /// Rules this write adds, in order.
    pub add: Vec<String>,
    /// Rules already in this file and kept.
    pub kept: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterPlan {
    pub scope_id: i64,
    pub project: String,
    pub files: Vec<FilterFile>,
    pub hints: Vec<String>,
    /// Binds a confirmation to exactly these files and these additions.
    pub fingerprint: String,
    pub written: bool,
}

impl FilterPlan {
    pub fn additions(&self) -> usize {
        self.files.iter().map(|f| f.add.len()).sum()
    }
}

/// One project that gets no filter in a batch, and why.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterSkip {
    pub project: String,
    pub reason: String,
}

/// The context filter for every approved project at once.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterBatch {
    /// Projects that get new rules.
    pub plans: Vec<FilterPlan>,
    /// Projects whose files already hold every rule.
    pub complete: usize,
    pub skipped: Vec<FilterSkip>,
    pub fingerprint: String,
    /// Projects written by the last apply.
    pub written: usize,
}

/// Claude settings of one project as far as a filter needs them.
struct ClaudeSettings {
    /// Text and value of the personal file, the only one that is written.
    local: Option<(String, Value)>,
    /// Deny rules of the shared file; they apply as well and are not repeated.
    shared_denies: Vec<String>,
    shared_note: Option<String>,
    /// A Git repository whose own ignore files do not name the personal file.
    unignored: bool,
}

impl Core {
    /// What a context filter would add for Claude Code, plus the Codex note.
    pub fn context_filter(&mut self, scope_id: i64) -> Result<FilterPlan> {
        let (plan, ..) = self.filter_plan(scope_id)?;
        Ok(plan)
    }

    /// Writes exactly the previewed additions. A different fingerprint means
    /// a file or the findings changed since the preview: nothing is written.
    pub fn apply_context_filter(&mut self, scope_id: i64, fingerprint: &str) -> Result<FilterPlan> {
        self.repo.require_local()?;
        let (mut plan, document, base) = self.filter_plan(scope_id)?;
        if plan.fingerprint != fingerprint {
            return Err(Error::conflict("Datei oder Funde haben sich geändert. Bitte die Vorschau neu öffnen."));
        }
        if plan.additions() == 0 {
            return Ok(plan);
        }
        let scope = self.repo.scope(scope_id)?;
        let identity = self.repo.scope_identity(scope_id)?;
        // Root and `.claude` stay open without delete sharing until the write
        // is done, so neither can be swapped for a link meanwhile.
        let root = ScopeRoot::open(Path::new(&scope.path), identity).map_err(|e| Error::blocked(e.to_string()))?;
        let folder = root
            .handle()
            .create_dir_pinned(OsStr::new(".claude"))
            .map_err(|e| Error::blocked(format!(".claude nicht sicher zu öffnen: {}", explain(&e))))?;
        // Claude Code writes this file too: it must still be what the preview read.
        let now =
            read_text(&folder, LOCAL).map_err(|issue| Error::blocked(format!("{LOCAL_PATH} nicht lesbar: {issue}")))?;
        if now != base {
            return Err(Error::conflict(format!(
                "{LOCAL_PATH} wurde inzwischen geändert, etwa von Claude Code. Bitte die Vorschau neu öffnen."
            )));
        }
        let mut text = serde_json::to_string_pretty(&document)?;
        text.push('\n');
        folder
            .replace_file(OsStr::new(LOCAL), text.as_bytes())
            .map_err(|e| Error::blocked(format!("{LOCAL_PATH} nicht geschrieben: {}", explain(&e))))?;
        self.repo.log(
            "settings",
            "Kontextfilter geschrieben",
            &format!(
                "{} Leseregeln für Claude Code in {LOCAL_PATH} ergänzt; bestehende Einträge unverändert.",
                plan.additions()
            ),
        )?;
        plan.written = true;
        Ok(plan)
    }

    /// The context filter for every project with a read approval.
    pub fn context_filter_all(&mut self) -> Result<FilterBatch> {
        let scopes = self.repo.scopes()?;
        let mut batch =
            FilterBatch { plans: Vec::new(), complete: 0, skipped: Vec::new(), fingerprint: String::new(), written: 0 };
        let mut hash = Sha256::new();
        for scope in scopes.iter().filter(|s| s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved) {
            match self.filter_plan(scope.id) {
                Ok((plan, ..)) if plan.additions() == 0 => batch.complete += 1,
                Ok((plan, ..)) => {
                    hash.update(plan.scope_id.to_le_bytes());
                    hash.update(plan.fingerprint.as_bytes());
                    batch.plans.push(plan);
                }
                Err(e) => batch.skipped.push(FilterSkip { project: self.project_label(scope)?, reason: e.to_string() }),
            }
        }
        batch.fingerprint = format!("{:x}", hash.finalize())[..16].to_string();
        Ok(batch)
    }

    /// Writes the previewed batch. Each project is written on its own; one
    /// that fails is reported and the others still get their rules.
    pub fn apply_context_filter_all(&mut self, fingerprint: &str) -> Result<FilterBatch> {
        self.repo.require_local()?;
        let mut batch = self.context_filter_all()?;
        if batch.fingerprint != fingerprint {
            return Err(Error::conflict("Dateien oder Funde haben sich geändert. Bitte die Vorschau neu öffnen."));
        }
        for plan in &mut batch.plans {
            match self.apply_context_filter(plan.scope_id, &plan.fingerprint) {
                Ok(done) => {
                    *plan = done;
                    batch.written += 1;
                }
                Err(e) => batch.skipped.push(FilterSkip { project: plan.project.clone(), reason: e.to_string() }),
            }
        }
        Ok(batch)
    }

    fn project_label(&mut self, scope: &Scope) -> Result<String> {
        let projects = self.repo.projects()?;
        Ok(scope
            .project_id
            .and_then(|id| projects.iter().find(|p| p.id == id))
            .map_or_else(|| scope.path.clone(), |p| p.name.clone()))
    }

    /// The preview, the document it would write and the personal file's
    /// text it is based on.
    fn filter_plan(&mut self, scope_id: i64) -> Result<(FilterPlan, Value, Option<String>)> {
        let scope = self.repo.scope(scope_id)?;
        if scope.kind != ScopeKind::Project || scope.status != ScopeStatus::Approved {
            return Err(Error::blocked("Kontextfilter nur für Projekte mit Lesefreigabe."));
        }
        let project = self.project_label(&scope)?;
        let identity = self.repo.scope_identity(scope_id)?;
        let root = ScopeRoot::open(Path::new(&scope.path), identity).map_err(|e| Error::blocked(e.to_string()))?;
        // Every environment, output and cache of this project, without the
        // dashboard's page limit, plus test data from the tidy-up report.
        let mut paths: Vec<String> = self
            .repo
            .published_items(scope_id)?
            .into_iter()
            .filter(|item| {
                matches!(item.category, ItemCategory::Environment | ItemCategory::Output | ItemCategory::Cache)
            })
            .map(|item| item.rel_path)
            .collect();
        // The report keeps real names, where stored findings may be redacted.
        let report =
            self.tidy.as_ref().and_then(|t| t.projects.iter().find(|p| Some(p.project_id) == scope.project_id));
        paths.extend(
            report
                .into_iter()
                .flat_map(|p| &p.items)
                .filter(|i| i.class == TidyClass::Regenerable || i.label == TEST_DATA_LABEL)
                .map(|i| i.rel_path.clone()),
        );
        paths.sort();
        paths.dedup();
        let mut unclear = 0;
        let mut wanted = Vec::new();
        for rel in &paths {
            match path_rule(root.handle(), rel) {
                Some(rule) => wanted.push(rule),
                // A redacted name that the report names in full is covered.
                None if rel.contains("[redigiert]") && report.is_some() => {}
                None => unclear += 1,
            }
        }
        wanted.sort();
        wanted.dedup();
        wanted.extend(SECRET_RULES.iter().map(|r| r.to_string()));
        let settings = read_settings(&root)?;
        let mut existing = deny_rules(settings.local.as_ref().map(|(_, value)| value));
        let kept = existing.len();
        existing.extend(settings.shared_denies.iter().cloned());
        let add: Vec<String> = wanted.into_iter().filter(|r| !existing.contains(r)).collect();
        let current = settings.local.as_ref().map(|(text, _)| text.clone());
        let mut document = settings.local.map(|(_, value)| value).unwrap_or_else(|| Value::Object(Default::default()));
        if !document.is_object() {
            return Err(Error::blocked(format!("{LOCAL_PATH} ist kein JSON-Objekt; nicht verändert.")));
        }
        let permissions = document
            .as_object_mut()
            .expect("object checked")
            .entry("permissions")
            .or_insert_with(|| Value::Object(Default::default()));
        let Some(permissions) = permissions.as_object_mut() else {
            return Err(Error::blocked("„permissions“ ist kein Objekt; nicht verändert."));
        };
        let deny = permissions.entry("deny").or_insert_with(|| Value::Array(Vec::new()));
        let Some(deny) = deny.as_array_mut() else {
            return Err(Error::blocked("„permissions.deny“ ist keine Liste; nicht verändert."));
        };
        deny.extend(add.iter().cloned().map(Value::String));
        let mut hash = Sha256::new();
        // Bound to this project and folder: another project's preview never fits.
        hash.update(scope_id.to_le_bytes());
        hash.update(identity.map(|i| i.to_string()).unwrap_or_default().as_bytes());
        hash.update(current.as_deref().unwrap_or("<keine Datei>").as_bytes());
        hash.update(b"\n--\n");
        hash.update(settings.shared_denies.join("\n").as_bytes());
        for rule in &add {
            hash.update(b"\n");
            hash.update(rule.as_bytes());
        }
        let fingerprint = format!("{:x}", hash.finalize())[..16].to_string();
        let mut hints = Vec::new();
        if !settings.shared_denies.is_empty() {
            hints.push(format!(
                "{} Verbote aus .claude/settings.json gelten weiter und werden nicht wiederholt.",
                settings.shared_denies.len()
            ));
        }
        hints.extend(settings.shared_note);
        if unclear > 0 {
            hints.push(format!(
                "{unclear} Ordner ohne Regel: Name im Inventar gekürzt (lange Zeichenfolgen werden redigiert), mit \
                 Platzhalterzeichen oder nicht mehr vorhanden. Ein Aufräumbericht liefert die vollen Namen."
            ));
        }
        if report.is_none() {
            hints
                .push("Testdaten fehlen noch: Für sie zuerst unter Speicher › Aufräumen den Bericht erstellen.".into());
        }
        if settings.unignored {
            hints.push(format!(
                "Git schließt {LOCAL_PATH} in diesem Projekt nicht erkennbar aus. Damit die persönlichen Regeln nicht \
                 eingecheckt werden, den Pfad in die .gitignore oder die globalen Git-Excludes aufnehmen; mogumogu \
                 ändert keine Git-Dateien."
            ));
        }
        hints.push("Codex kennt keine Ausschlussdatei. sandbox_mode = \"workspace-write\" begrenzt das Schreiben auf das Projekt; lesen kann Codex weiter überall. Enge Lesegrenzen gibt es erst in WSL2 oder einem Container.".into());
        hints.push("Claude Code wendet Leseverbote auf seine Datei-Werkzeuge, auf erkannte Shell-Dateibefehle (cat, head, tail, sed, tee) und auf Umleitungen an, nicht aber auf Programme, die Dateien selbst öffnen (etwa Python- oder Node-Skripte), oder auf Befehle ohne Dateinamen wie grep -r. Die Sandbox, die das systemweit erzwingt, gibt es unter nativem Windows nicht.".into());
        let plan = FilterPlan {
            scope_id,
            project,
            files: vec![FilterFile {
                tool: "Claude Code".into(),
                rel_path: LOCAL_PATH.into(),
                exists: current.is_some(),
                add,
                kept,
            }],
            hints,
            fingerprint,
            written: false,
        };
        Ok((plan, document, current))
    }
}

/// The project's Claude settings and Git ignore files, read handle-relative
/// below the approved root.
fn read_settings(root: &ScopeRoot) -> Result<ClaudeSettings> {
    let folder = root.handle().open_dir(OsStr::new(".claude")).ok();
    let local = match folder.as_ref().map(|dir| read_text(dir, LOCAL)) {
        None | Some(Ok(None)) => None,
        Some(Ok(Some(text))) => {
            let value = json::parse(&text, AI_MAX_NESTING)
                .map_err(|_| Error::blocked(format!("{LOCAL_PATH} ist kein gültiges JSON; nicht verändert.")))?;
            Some((text, value))
        }
        Some(Err(issue)) => return Err(Error::blocked(format!("{LOCAL_PATH} nicht lesbar: {issue}"))),
    };
    let (shared_denies, shared_note) = match folder.as_ref().map(|dir| read_text(dir, SHARED)) {
        None | Some(Ok(None)) => (Vec::new(), None),
        Some(Ok(Some(text))) => match json::parse(&text, AI_MAX_NESTING) {
            Ok(value) => (deny_rules(Some(&value)), None),
            Err(_) => (
                Vec::new(),
                Some(".claude/settings.json ist kein gültiges JSON; ihre Verbote sind nicht berücksichtigt.".into()),
            ),
        },
        Some(Err(_)) => {
            (Vec::new(), Some(".claude/settings.json nicht lesbar; ihre Verbote sind nicht berücksichtigt.".into()))
        }
    };
    let git =
        [true, false].iter().any(|dir| matches!(root.handle().child_identity(OsStr::new(".git"), *dir), Ok(Some(_))));
    let ignored = [Some(root.handle()), folder.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|dir| read_text(dir, ".gitignore").ok().flatten())
        .any(|text| ignores_local(&text));
    Ok(ClaudeSettings { local, shared_denies, shared_note, unignored: git && !ignored })
}

/// A file below `dir`, or `None` when it does not exist.
fn read_text(dir: &DirHandle, name: &str) -> std::result::Result<Option<String>, fsread::ReadIssue> {
    let mut budget = Budget::new(ScanLimits {
        max_depth: 1,
        max_entries: 16,
        max_total_bytes: AI_FILE_MAX_BYTES,
        time_budget: Duration::from_secs(2),
    });
    match fsread::read_text(dir, OsStr::new(name), AI_FILE_MAX_BYTES, &mut budget) {
        Ok(text) => Ok(Some(text)),
        Err(fsread::ReadIssue::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(issue) => Err(issue),
    }
}

fn deny_rules(document: Option<&Value>) -> Vec<String> {
    document
        .and_then(|value| value["permissions"]["deny"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Whether a `.gitignore` names the personal file or the whole `.claude`.
fn ignores_local(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with(['#', '!']))
        .any(|line| line.ends_with(LOCAL) || line == "*.local.json" || line.trim_matches('/') == ".claude")
}

/// A deny rule for one path below the project root: a folder with all its
/// content, a single file exactly. `None` when the name cannot be matched
/// exactly (redacted, glob characters) or the path is gone.
fn path_rule(root: &DirHandle, rel: &str) -> Option<String> {
    if rel.is_empty() || rel.contains("[redigiert]") || rel.chars().any(|c| "*?[]{}!\\".contains(c)) {
        return None;
    }
    let mut parts: Vec<&str> = rel.split('/').collect();
    let name = OsStr::new(parts.pop()?);
    let mut folder: Option<DirHandle> = None;
    for part in parts {
        folder = Some(folder.as_ref().unwrap_or(root).open_dir(OsStr::new(part)).ok()?);
    }
    let parent = folder.as_ref().unwrap_or(root);
    if matches!(parent.child_identity(name, true), Ok(Some(_))) {
        Some(format!("Read(/{rel}/**)"))
    } else if matches!(parent.child_identity(name, false), Ok(Some(_))) {
        Some(format!("Read(/{rel})"))
    } else {
        None
    }
}

/// Plain words for the errors a person can act on.
fn explain(error: &std::io::Error) -> String {
    // A refusal with its own reason (a link, further hard links) keeps it.
    if error.raw_os_error().is_none() {
        return error.to_string();
    }
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "Zugriff verweigert (schreibgeschützt oder in Benutzung)".into(),
        std::io::ErrorKind::AlreadyExists => "eine Hilfsdatei gleichen Namens liegt schon dort".into(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::ignores_local;

    #[cfg(windows)]
    #[test]
    fn rules_match_folders_whole_and_files_exactly() {
        use super::path_rule;
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("web/node_modules/dep")).unwrap();
        std::fs::write(temp.path().join("web/local.db"), "x").unwrap();
        let root = crate::platform::DirHandle::open_root(temp.path(), false).unwrap();
        assert_eq!(path_rule(&root, "web/node_modules").as_deref(), Some("Read(/web/node_modules/**)"));
        assert_eq!(path_rule(&root, "web/local.db").as_deref(), Some("Read(/web/local.db)"));
        for unclear in ["web/[redigiert]", "web/*", "gone", ""] {
            assert_eq!(path_rule(&root, unclear), None, "{unclear}");
        }
    }

    #[test]
    fn gitignore_lines_that_keep_the_personal_file_out() {
        for ignored in
            [".claude/settings.local.json", "**/.claude/settings.local.json", "/.claude/", ".claude", "*.local.json"]
        {
            assert!(ignores_local(&format!("node_modules\n{ignored}\n")), "{ignored}");
        }
        for open in ["", "# .claude/settings.local.json", "!.claude/settings.local.json", ".claude/settings.json"] {
            assert!(!ignores_local(open), "{open}");
        }
    }
}
