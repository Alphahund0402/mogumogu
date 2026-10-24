//! Where a plan may act, and the verified way to one folder. Every open
//! below the approved root is handle-relative and never follows a link.
use crate::domain::Resource;
use crate::fsread::{self, Budget, ScopeRoot};
use crate::inventory::tidy;
use crate::limits::ScanLimits;
use crate::platform::{self, DirHandle, FileIdentity};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// File that marks a folder as an explicitly disposable test root.
pub const DISPOSABLE_MARKER: &str = ".mogumogu-disposable-test-root";
pub const DISPOSABLE_MARKER_TEXT: &str = "mogumogu disposable test root";
/// File that protects its folder and every folder below it from cleanup,
/// counted from the project root downwards.
pub const KEEP_MARKER: &str = ".mogumogu-keep";
/// The shell's classic path limit; longer paths fail there or behave oddly.
const SHELL_PATH_MAX: usize = 259;

fn components(path: &str) -> Vec<String> {
    path.split(['\\', '/']).filter(|c| !c.is_empty()).map(str::to_lowercase).collect()
}

/// A name the shell resolves exactly as written. Windows drops trailing dots
/// and spaces and maps device names; such a name could point the shell at
/// another folder than the one checked on handles.
fn plain_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.chars().count() == 4
            && stem.chars().last().is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
    !name.is_empty()
        && !device
        && !name.starts_with(' ')
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
}

fn plain_path(path: &str) -> Result<(), String> {
    match path.split('\\').skip(1).find(|part| !plain_name(part)) {
        Some(part) => Err(format!("Der Name „{part}“ ist für Windows nicht eindeutig.")),
        None => Ok(()),
    }
}

/// Until G5 a folder must lie inside a registered disposable test root
/// whose marker file is still intact – physically, not just by its path: the
/// way from the test root is walked without links and must end at `identity`.
pub(super) fn allowed(roots: &[(String, FileIdentity)], path: &str, identity: FileIdentity) -> Result<(), String> {
    if super::PRODUCTION_ENABLED {
        return Ok(());
    }
    let target = components(path);
    let Some((root_path, root_identity)) = roots.iter().find(|(root, _)| {
        let root = components(root);
        root.len() < target.len() && target.starts_with(&root)
    }) else {
        return Err("Gesperrt bis zum unabhängigen Review (G5): vorher wird nur in einer registrierten \
                    wegwerfbaren Testwurzel in den Papierkorb verschoben."
            .into());
    };
    let root = ScopeRoot::open(Path::new(root_path), Some(*root_identity))
        .map_err(|e| format!("Testwurzel nicht verwendbar: {e}"))?;
    let mut budget = Budget::new(ScanLimits::default());
    let marker = fsread::read_text(root.handle(), OsStr::new(DISPOSABLE_MARKER), 4096, &mut budget)
        .map_err(|_| "Markierungsdatei der Testwurzel fehlt oder ist unlesbar.".to_string())?;
    if marker.trim() != DISPOSABLE_MARKER_TEXT {
        return Err("Markierungsdatei der Testwurzel hat einen unerwarteten Inhalt.".into());
    }
    let parts: Vec<&str> = path.split(['\\', '/']).filter(|c| !c.is_empty()).collect();
    let mut current: Option<DirHandle> = None;
    for part in &parts[components(root_path).len()..] {
        if !plain_name(part) {
            return Err(format!("Der Name „{part}“ ist für Windows nicht eindeutig."));
        }
        let next = current
            .as_ref()
            .unwrap_or(root.handle())
            .open_dir(OsStr::new(part))
            .map_err(|e| format!("Weg von der Testwurzel nicht sicher ({part}): {e}"))?;
        current = Some(next);
    }
    let reached = current.ok_or("Ziel ist die Testwurzel selbst.")?.identity().map_err(|e| e.to_string())?;
    if reached != identity {
        return Err("Der Ordner liegt nicht wirklich in der Testwurzel.".into());
    }
    Ok(())
}

/// Protection wins: a protected resource in the folder, at it or above it
/// keeps the folder where it is.
pub(super) fn unprotected(resources: &[Resource], paths: &[&str]) -> Result<(), String> {
    let normal = |path: &str| path.trim_end_matches('\\').to_lowercase();
    for resource in resources.iter().filter(|r| r.protected) {
        let kept = normal(&resource.path);
        for path in paths.iter().map(|p| normal(p)) {
            if path == kept || path.starts_with(&format!("{kept}\\")) || kept.starts_with(&format!("{path}\\")) {
                return Err(format!("Geschützte Ressource „{}“ liegt darin, daran oder darüber.", resource.name));
            }
        }
    }
    Ok(())
}

/// A folder below an approved root. Root and parent stay open without delete
/// sharing while this is held, so neither can be renamed, and Windows
/// refuses to rename a folder with open handles below it: no folder on the
/// way can be swapped for a link.
#[derive(Debug)]
pub(super) struct Located {
    root: ScopeRoot,
    /// `None` when the folder lies directly in the root.
    parent: Option<DirHandle>,
    name: String,
    /// The registered path joined with the folder's names, for the gate.
    pub path: PathBuf,
    /// The real place of the held parent plus the name: what the shell gets.
    pub shell: PathBuf,
}

/// Walks to the parent of `rel_path`, one component at a time. A name the
/// shell could resolve differently, a link on the way or a [`KEEP_MARKER`]
/// in the root or any folder on the way stops it, and so does a place the
/// recycle bin cannot take.
pub(super) fn locate(root_path: &str, identity: Option<FileIdentity>, rel_path: &str) -> Result<Located, String> {
    let mut parts: Vec<&str> = rel_path.split('/').collect();
    let name = parts.pop().filter(|n| !n.is_empty()).ok_or("Leerer Zielpfad.")?.to_string();
    if let Some(part) = parts.iter().chain([&name.as_str()]).find(|part| !plain_name(part)) {
        return Err(format!("Der Name „{part}“ ist für Windows nicht eindeutig."));
    }
    let root = ScopeRoot::open(Path::new(root_path), identity).map_err(|e| format!("Projektordner: {e}"))?;
    let mut located = Located { root, parent: None, name, path: PathBuf::from(root_path), shell: PathBuf::new() };
    refuse_kept(located.parent())?;
    for part in parts {
        let next = located
            .parent()
            .open_dir_pinned(OsStr::new(part))
            .map_err(|e| format!("Weg zum Ordner nicht sicher ({part}): {e}"))?;
        refuse_kept(&next)?;
        located.parent = Some(next);
        located.path.push(part);
    }
    located.path.push(&located.name);
    let real = located.parent().final_path().map_err(|e| format!("Ort des Ordners nicht bestimmbar: {e}"))?;
    let shell = format!("{}\\{}", real.trim_end_matches('\\'), located.name);
    if !platform::fixed_disk(Path::new(&shell)) {
        return Err("Kein Papierkorb auf diesem Laufwerk (nur feste lokale Laufwerke).".into());
    }
    plain_path(&shell)?;
    if shell.chars().count() > SHELL_PATH_MAX {
        return Err(format!("Pfad länger als {SHELL_PATH_MAX} Zeichen; der Papierkorb nimmt ihn nicht sicher an."));
    }
    located.shell = PathBuf::from(shell);
    Ok(located)
}

fn refuse_kept(dir: &DirHandle) -> Result<(), String> {
    match dir.child_identity(OsStr::new(KEEP_MARKER), false) {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(format!("Geschützt durch {KEEP_MARKER}.")),
        Err(e) => Err(format!("{KEEP_MARKER} nicht prüfbar: {e}")),
    }
}

impl Located {
    fn parent(&self) -> &DirHandle {
        self.parent.as_ref().unwrap_or(self.root.handle())
    }

    /// Identity of whatever folder is at the place now.
    pub fn current(&self) -> std::io::Result<Option<FileIdentity>> {
        self.parent().child_identity(OsStr::new(&self.name), true)
    }

    /// Opens the folder itself (never a link), reads its identity, checks
    /// everything below it and asks the tidy rules again. Every handle inside
    /// the folder is closed before this returns, so nothing of mogumogu stays
    /// open in the folder that moves.
    pub fn inspect(&self) -> Result<(FileIdentity, &'static str, String), String> {
        let identity = {
            let folder = self
                .parent()
                .open_dir(OsStr::new(&self.name))
                .map_err(|e| format!("Ordner nicht sicher zu öffnen: {e}"))?;
            let identity = folder.identity().map_err(|e| format!("Identität nicht lesbar: {e}"))?;
            contents_clear(folder)?;
            identity
        };
        let (label, evidence) = tidy::recoverable(self.parent(), &self.name)
            .ok_or("Nicht mehr wiederherstellbar belegt (Datei daneben oder Spur des Werkzeugs fehlt).")?;
        Ok((identity, label, evidence))
    }
}

struct Pending {
    parent: Rc<DirHandle>,
    name: Option<OsString>,
    rel: String,
    depth: u32,
}

/// Everything below a folder, bounded: no version control, no keep marker,
/// no link, no cloud placeholder and nothing unreadable. A link inside would
/// only move along, but the shell's fallbacks are not proven not to follow it.
fn contents_clear(folder: DirHandle) -> Result<(), String> {
    let limits =
        ScanLimits { max_depth: 64, max_entries: 2_000_000, max_total_bytes: 0, time_budget: Duration::from_secs(120) };
    let mut budget = Budget::new(limits);
    let too_big = || "Zu groß, um den Inhalt vollständig zu prüfen.".to_string();
    let mut stack = vec![Pending { parent: Rc::new(folder), name: None, rel: String::new(), depth: 0 }];
    while let Some(pending) = stack.pop() {
        let shown = if pending.rel.is_empty() { "Ordner".to_string() } else { pending.rel.clone() };
        let dir = match pending.name {
            None => pending.parent,
            Some(name) => {
                Rc::new(pending.parent.open_dir(&name).map_err(|e| format!("{shown}: nicht sicher zu öffnen ({e})"))?)
            }
        };
        let (entries, truncated) =
            fsread::list(&dir, &mut budget).map_err(|e| format!("{shown}: nicht lesbar ({e})"))?;
        if truncated || budget.exhausted() {
            return Err(too_big());
        }
        for entry in entries {
            if !budget.entry() {
                return Err(too_big());
            }
            let name = entry.name_str().ok_or_else(|| format!("{shown}: Eintrag mit nicht darstellbarem Namen."))?;
            let child = fsread::join(&pending.rel, name);
            let lower = name.to_ascii_lowercase();
            if entry.is_reparse() {
                return Err(format!("{child}: Verknüpfung darin – der Ordner bleibt."));
            }
            if entry.is_placeholder() {
                return Err(format!("{child}: Cloud-Platzhalter darin."));
            }
            if lower == KEEP_MARKER {
                return Err(format!("{child}: geschützt."));
            }
            if matches!(lower.as_str(), ".git" | ".hg" | ".svn") {
                return Err(format!("{child}: Versionsverwaltung darin."));
            }
            if entry.is_dir() {
                if pending.depth + 1 > limits.max_depth {
                    return Err(format!("{child}: zu tief verschachtelt."));
                }
                stack.push(Pending {
                    parent: Rc::clone(&dir),
                    name: Some(entry.name.clone()),
                    rel: child,
                    depth: pending.depth + 1,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::plain_name;

    #[test]
    fn names_the_shell_could_read_differently_are_refused() {
        for good in ["node_modules", ".venv", "target", "My Project", "dist.v2", "a.b"] {
            assert!(plain_name(good), "{good}");
        }
        for bad in ["foo.", "foo ", " foo", "con", "NUL.txt", "com1", "LPT9.log", "a:b", "a?b", "a\u{1}b", ""] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }
}
