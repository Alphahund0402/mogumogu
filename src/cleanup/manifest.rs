//! Identity manifests and the verified path from a disposable root to a
//! target. All opens are handle-relative and never follow links.
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::{CLEANUP_MAX_DEPTH, CLEANUP_MAX_ENTRIES, ScanLimits};
use crate::platform::{DirHandle, FileIdentity};
use crate::storage::ManifestEntry;
use std::ffi::OsStr;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

/// File that marks a folder as an explicitly disposable test root.
pub const DISPOSABLE_MARKER: &str = ".mogumogu-disposable-test-root";
pub const DISPOSABLE_MARKER_TEXT: &str = "mogumogu disposable test root";
/// File that protects its folder (and every parent operation) from cleanup.
pub const KEEP_MARKER: &str = ".mogumogu-keep";

/// A target folder reached through a verified disposable root. The root is
/// held without delete sharing so it cannot be swapped during the action.
#[derive(Debug)]
pub(super) struct OpenedTarget {
    _root: Rc<DirHandle>,
    pub target: Rc<DirHandle>,
}

fn components(path: &str) -> Vec<String> {
    path.split(['\\', '/']).filter(|c| !c.is_empty()).map(str::to_lowercase).collect()
}

pub(super) fn open_target(
    roots: &[(String, FileIdentity)],
    target_path: &str,
    target_identity: Option<FileIdentity>,
) -> Result<OpenedTarget, String> {
    let target = components(target_path);
    let Some((root_path, root_identity)) = roots.iter().find(|(root, _)| {
        let root = components(root);
        root.len() < target.len() && target.starts_with(&root)
    }) else {
        return Err(if super::PRODUCTION_ENABLED {
            "Kein freigegebener Bereinigungsbereich.".into()
        } else {
            "Produktive Bereinigung ist bis zum unabhängigen Review (G5) gesperrt; der Testexecutor arbeitet \
             nur in einer registrierten wegwerfbaren Testwurzel."
                .into()
        });
    };
    let root = ScopeRoot::open(Path::new(root_path), Some(*root_identity))
        .map_err(|e| format!("Testwurzel nicht verwendbar: {e}"))?
        .into_handle();
    let mut budget = Budget::new(ScanLimits::default());
    let marker = fsread::read_text(&root, OsStr::new(DISPOSABLE_MARKER), 4096, &mut budget)
        .map_err(|_| "Markierungsdatei der Testwurzel fehlt oder ist unlesbar.".to_string())?;
    if marker.trim() != DISPOSABLE_MARKER_TEXT {
        return Err("Markierungsdatei der Testwurzel hat einen unerwarteten Inhalt.".into());
    }
    let root = Rc::new(root);
    let original: Vec<&str> = target_path.split(['\\', '/']).filter(|c| !c.is_empty()).collect();
    let mut current = Rc::clone(&root);
    for component in &original[components(root_path).len()..] {
        current = Rc::new(
            current
                .open_dir(OsStr::new(component))
                .map_err(|e| format!("Pfad zum Ziel nicht sicher erreichbar ({component}): {e}"))?,
        );
    }
    let identity = current.identity().map_err(|e| e.to_string())?;
    match target_identity {
        Some(expected) if expected == identity => Ok(OpenedTarget { _root: root, target: current }),
        Some(_) => Err("Der Temp-Ordner wurde ersetzt; frühere Entscheidungen gelten nicht für ihn.".into()),
        None => Err("Für diesen Ordner ist keine Identität gespeichert.".into()),
    }
}

/// Builds the manifest of everything below `target`. Any link, placeholder,
/// protected child or unreadable entry becomes a blocker for the whole
/// target: the parent operation is refused rather than partially applied.
pub(super) fn build(target: &Rc<DirHandle>) -> (Vec<ManifestEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut blockers = Vec::new();
    let mut budget = Budget::new(ScanLimits {
        max_depth: CLEANUP_MAX_DEPTH,
        max_entries: CLEANUP_MAX_ENTRIES as u64,
        max_total_bytes: 0,
        time_budget: Duration::from_secs(60),
    });
    let mut stack: Vec<(Rc<DirHandle>, String, u32)> = vec![(Rc::clone(target), String::new(), 0)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let listing = match fsread::list(&dir, &mut budget) {
            Ok((listing, false)) => listing,
            Ok((_, true)) => {
                blockers.push(format!("Mehr als {CLEANUP_MAX_ENTRIES} Einträge – nicht planbar."));
                break;
            }
            Err(e) => {
                blockers.push(format!("{}: nicht lesbar ({e})", display(&rel)));
                continue;
            }
        };
        for entry in listing {
            if !budget.entry() {
                blockers.push("Planungsbudget überschritten.".into());
                return (entries, blockers);
            }
            let Some(name) = entry.name_str() else {
                blockers.push(format!("{}: Eintrag mit nicht darstellbarem Namen.", display(&rel)));
                continue;
            };
            let child_rel = fsread::join(&rel, name);
            if entry.is_reparse() {
                blockers.push(format!("{child_rel}: Verknüpfung – gesamte Operation verweigert."));
                continue;
            }
            if entry.is_placeholder() {
                blockers.push(format!("{child_rel}: Cloud-Platzhalter – nicht planbar."));
                continue;
            }
            if name.eq_ignore_ascii_case(KEEP_MARKER) {
                blockers
                    .push(format!("{}: geschütztes Kind ({KEEP_MARKER}) – Elternoperation verweigert.", display(&rel)));
                continue;
            }
            let child_depth = depth + 1;
            if entry.is_dir() {
                if child_depth > CLEANUP_MAX_DEPTH {
                    blockers.push(format!("{child_rel}: zu tief verschachtelt."));
                    continue;
                }
                match dir.open_dir(OsStr::new(name)).and_then(|d| Ok((d.identity()?, d))) {
                    Ok((identity, handle)) => {
                        entries.push(ManifestEntry {
                            rel_path: child_rel.clone(),
                            identity,
                            is_dir: true,
                            size: 0,
                            modified: entry.modified,
                            depth: child_depth,
                        });
                        stack.push((Rc::new(handle), child_rel, child_depth));
                    }
                    Err(e) => blockers.push(format!("{child_rel}: nicht sicher zu öffnen ({e})")),
                }
            } else {
                match dir.child_identity(OsStr::new(name), false) {
                    Ok(Some(identity)) => entries.push(ManifestEntry {
                        rel_path: child_rel,
                        identity,
                        is_dir: false,
                        size: entry.size,
                        modified: entry.modified,
                        depth: child_depth,
                    }),
                    Ok(None) => blockers.push(format!("{child_rel}: während der Planung verschwunden.")),
                    Err(e) => blockers.push(format!("{child_rel}: Identität nicht lesbar ({e})")),
                }
            }
        }
    }
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (entries, blockers)
}

fn display(rel: &str) -> &str {
    if rel.is_empty() { "Zielordner" } else { rel }
}

/// Opens the parent folder of a manifest entry below the target.
pub(super) fn parent_of(target: &Rc<DirHandle>, rel_path: &str) -> std::io::Result<(Rc<DirHandle>, String)> {
    let mut parts: Vec<&str> = rel_path.split('/').collect();
    let name = parts.pop().unwrap_or_default().to_string();
    let mut current = Rc::clone(target);
    for part in parts {
        current = Rc::new(current.open_dir(OsStr::new(part))?);
    }
    Ok((current, name))
}
