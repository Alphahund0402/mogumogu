//! Cached volume metadata, recorded project sizes and an explicitly requested
//! folder view. Refreshing a dashboard never walks a project tree.
use super::Core;
use crate::domain::*;
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::{ScanLimits, UI_PAGE};
use crate::platform::{self, DirHandle};
use crate::{Error, Result};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

const DRIVE_CACHE_SECONDS: i64 = 30;
const TREE_MAX_ENTRIES: u64 = 10_000;
const TREE_MAX_DEPTH: u32 = 64;
const DEMO_SCOPE_BASE: i64 = 1_000_000;
const SIZE_NOTE: &str = "Projekte sind nach dem Laufwerksbuchstaben ihres registrierten Pfads gruppiert; Mountpoints und SUBST-Aliase werden nicht aufgelöst. Projektgrößen sind gespeicherte logische Dateigrößen. Hardlinks, Kompression und gemeinsame Speicher können vom belegten Plattenplatz abweichen; daraus folgt kein freigebbarer Speicher. Kapazitäten gelten für den aktuellen Benutzer; Quoten sind möglich.";

impl Core {
    pub fn storage_overview(&mut self) -> Result<StorageOverview> {
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        Ok(self.storage_with_basis(&scopes, &projects))
    }

    pub(super) fn storage_with_basis(&mut self, scopes: &[Scope], projects: &[Project]) -> StorageOverview {
        let now = self.repo.now();
        let expired = self
            .storage_cache
            .as_ref()
            .and_then(|c| c.checked_at)
            .is_none_or(|at| now < at || now.saturating_sub(at) >= DRIVE_CACHE_SECONDS);
        if expired {
            let (drives, note) = if self.is_demo() {
                (demo_drives(), format!("Beispieldaten · {SIZE_NOTE}"))
            } else {
                match platform::logical_drives() {
                    Ok(drives) => (drives, SIZE_NOTE.into()),
                    Err(_) => (Vec::new(), format!("Laufwerkskapazitäten derzeit nicht verfügbar. {SIZE_NOTE}")),
                }
            };
            self.storage_cache =
                Some(StorageOverview { checked_at: Some(now), drives, note, ..StorageOverview::default() });
        }
        let mut overview = self.storage_cache.clone().unwrap_or_default();
        overview.projects = project_storage(projects, scopes, &overview.drives);
        for project in &overview.projects {
            if !project.drive.is_empty() && !overview.drives.iter().any(|d| d.root == project.drive) {
                overview.drives.push(DriveUsage {
                    root: project.drive.clone(),
                    label: project.drive.clone(),
                    kind: "Registrierter Projektort".into(),
                    total_bytes: None,
                    free_bytes: None,
                    used_bytes: None,
                    project_bytes: None,
                    status: "Laufwerk nicht verfügbar oder nicht unterstützt".into(),
                });
            }
        }
        for drive in &mut overview.drives {
            let members: Vec<_> = overview.projects.iter().filter(|p| p.drive == drive.root && p.included).collect();
            drive.project_bytes = if members.is_empty() {
                Some(0)
            } else {
                members.iter().try_fold(0_u64, |sum, p| sum.checked_add(p.bytes?))
            };
        }
        overview.scopes = if self.is_demo() {
            projects
                .iter()
                .map(|p| StorageScope {
                    scope_id: DEMO_SCOPE_BASE + p.id,
                    path: p.path.clone(),
                    label: format!("{} · Beispieldaten", p.name),
                    project_id: Some(p.id),
                    kind: ScopeKind::Project,
                })
                .collect()
        } else {
            scopes
                .iter()
                .filter(|s| s.status == ScopeStatus::Approved && tree_kind(s.kind))
                .map(|s| StorageScope {
                    scope_id: s.id,
                    path: s.path.clone(),
                    label: projects
                        .iter()
                        .find(|p| Some(p.id) == s.project_id)
                        .map_or_else(|| s.kind.label().into(), |p| p.name.clone()),
                    project_id: s.project_id,
                    kind: s.kind,
                })
                .collect()
        };
        overview
    }

    /// Lists one directory level and measures displayed subfolders under one
    /// shared small budget. Names-only search grants never expose file sizes.
    /// Narrow manager/configuration approvals cannot be used as a file browser.
    pub fn folder_tree(&mut self, scope_id: i64, rel_path: &str) -> Result<FolderTree> {
        let parts = relative_parts(rel_path)?;
        if self.is_demo() {
            if !self.repo.projects()?.iter().any(|p| DEMO_SCOPE_BASE + p.id == scope_id) {
                return Err(Error::not_found("Beispielordner nicht gefunden."));
            }
            return demo_tree(scope_id, rel_path);
        }
        let scope = self.repo.scope(scope_id)?;
        if scope.status != ScopeStatus::Approved || !tree_kind(scope.kind) {
            return Err(Error::blocked("Ordneransicht nur für freigegebene Projekte oder Dateinamensuchen."));
        }
        let identity = self
            .repo
            .scope_identity(scope_id)?
            .ok_or_else(|| Error::blocked("Dem Suchbereich fehlt eine bestätigte Ordneridentität."))?;
        let root = ScopeRoot::open(Path::new(&scope.path), Some(identity)).map_err(|_| {
            Error::blocked("Ordner nicht erreichbar, ersetzt oder nicht sicher lesbar. Bitte Freigabe prüfen.")
        })?;
        // Keep every ancestor handle live while descending. Each child is a
        // single handle-relative open; user input never becomes an OS path.
        let mut ancestors = vec![root.into_handle()];
        for part in parts {
            let child = ancestors
                .last()
                .expect("root present")
                .open_dir(OsStr::new(part))
                .map_err(|_| Error::blocked("Unterordner nicht erreichbar oder Verknüpfung/Cloud-Platzhalter."))?;
            ancestors.push(child);
        }
        let dir = ancestors.last().expect("root present");
        let mut budget = Budget::new(ScanLimits {
            max_depth: TREE_MAX_DEPTH,
            max_entries: TREE_MAX_ENTRIES,
            max_total_bytes: 0,
            time_budget: Duration::from_secs(2),
        });
        let (mut entries, truncated) = fsread::list(dir, &mut budget)
            .map_err(|_| Error::blocked("Ordner kann derzeit nicht aufgelistet werden."))?;
        entries.sort_by(|a, b| b.is_dir().cmp(&a.is_dir()).then_with(|| a.name.cmp(&b.name)));
        let mut tree = FolderTree {
            scope_id,
            rel_path: rel_path.into(),
            complete: !truncated && entries.len() <= UI_PAGE as usize,
            ..FolderTree::default()
        };
        let names_only = scope.kind == ScopeKind::ProjectSearch;
        for entry in entries.into_iter().take(UI_PAGE as usize) {
            // Listing may already have reached its budget. Its bounded names
            // still remain displayable; further size walks then stay unknown.
            let _ = budget.entry();
            let Some(name) = entry.name_str() else {
                tree.complete = false;
                continue;
            };
            let (bytes, status) = if entry.is_reparse() {
                tree.complete = false;
                (None, "Verknüpfung/Reparse Point · nicht verfolgt".into())
            } else if entry.is_placeholder() {
                tree.complete = false;
                (None, "Cloud-Platzhalter · nicht geladen".into())
            } else if names_only {
                (None, "Nur Dateinamen freigegeben".into())
            } else if entry.is_dir() {
                match dir.open_dir(&entry.name) {
                    Ok(child) => subtree_size(child, &mut budget),
                    Err(_) => (None, "Nicht erreichbar oder nicht sicher lesbar".into()),
                }
            } else {
                (Some(entry.size), "Logische Dateigröße · Metadaten".into())
            };
            if bytes.is_none() && !names_only {
                tree.complete = false;
            }
            tree.entries.push(FolderEntry {
                name: crate::privacy::label(name),
                rel_path: fsread::join(rel_path, name),
                depth: 0,
                is_dir: entry.is_dir(),
                bytes,
                status,
            });
        }
        if budget.exhausted() {
            tree.complete = false;
        }
        tree.note = if names_only {
            "Eine Ordnerebene · nur Namen; maximal 200 Einträge. Ordner lassen sich einzeln öffnen.".into()
        } else {
            "Eine Ordnerebene · maximal 200 Einträge. Größen auf Anfrage mit gemeinsamem Limit von 10.000 Einträgen / 2 Sekunden; unbekannte Größen bleiben offen. Logische Größen zählen Hardlinks je Pfad und sind kein freigebbarer Plattenplatz.".into()
        };
        if !tree.complete {
            tree.note = format!("Teilweise Ansicht · {}", tree.note);
        }
        Ok(tree)
    }
}

fn tree_kind(kind: ScopeKind) -> bool {
    matches!(kind, ScopeKind::Project | ScopeKind::ProjectSearch)
}

fn relative_parts(path: &str) -> Result<Vec<&str>> {
    if path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(Error::invalid("Ungültiger relativer Ordnerpfad."));
    }
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > TREE_MAX_DEPTH as usize
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || part.ends_with([' ', '.'])
                || part.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
        })
    {
        return Err(Error::invalid("Nur relative Ordnerpfade ohne Verknüpfungen oder Elternsegmente sind erlaubt."));
    }
    Ok(parts)
}

fn subtree_size(root: DirHandle, budget: &mut Budget) -> (Option<u64>, String) {
    if budget.exhausted() {
        return (None, "Messbudget erreicht · Größe unbekannt".into());
    }
    let mut total = 0_u64;
    let mut complete = true;
    let mut pending: Vec<(Rc<DirHandle>, Option<OsString>, u32)> = vec![(Rc::new(root), None, 0_u32)];
    while let Some((parent, name, depth)) = pending.pop() {
        let dir = match name {
            None => parent,
            Some(name) => match parent.open_dir(&name) {
                Ok(child) => Rc::new(child),
                Err(_) => {
                    complete = false;
                    continue;
                }
            },
        };
        let entries = match fsread::list(&dir, budget) {
            Ok((entries, truncated)) => {
                complete &= !truncated;
                entries
            }
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for entry in entries {
            if !budget.entry() {
                return (None, "Messbudget erreicht · Größe unbekannt".into());
            }
            if entry.is_reparse() || entry.is_placeholder() {
                complete = false;
            } else if entry.is_dir() {
                if depth >= TREE_MAX_DEPTH {
                    complete = false;
                } else {
                    // Retain names and parents, rather than thousands of
                    // simultaneously opened sibling directory handles.
                    pending.push((Rc::clone(&dir), Some(entry.name), depth + 1));
                }
            } else if let Some(sum) = total.checked_add(entry.size) {
                total = sum;
            } else {
                complete = false;
            }
        }
    }
    if complete && !budget.exhausted() {
        (Some(total), "Logische Größe · begrenzte Messung auf Anfrage".into())
    } else {
        (None, "Teilweise gemessen · Links, Platzhalter oder unlesbare Unterordner".into())
    }
}

fn drive_root(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') {
        format!("{}:\\", char::from(bytes[0].to_ascii_uppercase()))
    } else {
        String::new()
    }
}

fn path_key(path: &str) -> String {
    path.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn under_path(child: &str, parent: &str) -> bool {
    child.strip_prefix(parent).is_some_and(|rest| rest.starts_with('\\'))
}

fn project_storage(projects: &[Project], scopes: &[Scope], drives: &[DriveUsage]) -> Vec<ProjectStorage> {
    // Normalize once per project, not once for each pair on every UI refresh.
    let keys: Vec<_> = projects.iter().map(|p| path_key(&p.path)).collect();
    projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            let bytes = project.bytes.and_then(|b| u64::try_from(b).ok());
            let duplicate = projects.iter().zip(&keys).any(|(other, key)| other.id < project.id && *key == keys[index]);
            let included = !duplicate
                && !projects.iter().zip(&keys).any(|(parent, key)| {
                    parent.id != project.id && parent.bytes.is_some_and(|b| b >= 0) && under_path(&keys[index], key)
                });
            let drive = drive_root(&project.path);
            let total = drives.iter().find(|d| d.root == drive).and_then(|d| d.total_bytes).filter(|&n| n > 0);
            let scope = scopes.iter().find(|s| {
                s.kind == ScopeKind::Project && s.project_id == Some(project.id) && s.status == ScopeStatus::Approved
            });
            ProjectStorage {
                project_id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
                drive,
                bytes,
                share: bytes.zip(total).map(|(bytes, total)| bytes as f64 / total as f64 * 100.0),
                scope_id: if project.origin == Origin::Demo {
                    Some(DEMO_SCOPE_BASE + project.id)
                } else {
                    scope.map(|s| s.id)
                },
                included,
                note: if duplicate {
                    "Identischer registrierter Pfad · im Laufwerkssaldo nur einmal enthalten".into()
                } else if !included {
                    "Im gemessenen Elternprojekt enthalten · im Laufwerkssaldo nicht doppelt addiert".into()
                } else if project.origin == Origin::Demo {
                    "Beispielgröße".into()
                } else if bytes.is_some() {
                    "Gespeicherte Messung · kann seitdem verändert sein".into()
                } else {
                    "Noch nicht gemessen · Anteil unbekannt".into()
                },
            }
        })
        .collect()
}

fn demo_drives() -> Vec<DriveUsage> {
    [("C:\\", 512_000_000_000_u64, 183_000_000_000_u64), ("D:\\", 1_000_000_000_000, 594_000_000_000)]
        .into_iter()
        .map(|(root, total, free)| DriveUsage {
            root: root.into(),
            label: format!("{root} · Beispiel"),
            kind: "Beispiellaufwerk".into(),
            total_bytes: Some(total),
            free_bytes: Some(free),
            used_bytes: Some(total - free),
            project_bytes: None,
            status: "Synthetische Beispieldaten".into(),
        })
        .collect()
}

fn demo_tree(scope_id: i64, rel_path: &str) -> Result<FolderTree> {
    let rows: &[(&str, bool, u64)] = match rel_path {
        "" => &[
            ("src", true, 480_000),
            ("tests", true, 96_000),
            ("node_modules", true, 850_000_000),
            ("README.md", false, 2_400),
        ],
        "src" => &[("main.rs", false, 8_400), ("domain", true, 24_000)],
        "src/domain" => &[("project.rs", false, 12_000), ("storage.rs", false, 12_000)],
        "tests" => &[("storage.rs", false, 12_000)],
        "node_modules" => &[("example-library", true, 720_000)],
        "node_modules/example-library" => &[("index.js", false, 720_000)],
        _ => return Err(Error::not_found("Beispielordner nicht gefunden.")),
    };
    Ok(FolderTree {
        scope_id,
        rel_path: rel_path.into(),
        complete: true,
        entries: rows
            .iter()
            .map(|(name, is_dir, bytes)| FolderEntry {
                name: (*name).into(),
                rel_path: fsread::join(rel_path, name),
                depth: 0,
                is_dir: *is_dir,
                bytes: Some(*bytes),
                status: "Synthetische Beispieldaten".into(),
            })
            .collect(),
        note: "Beispielansicht · zeigt synthetische Ordner, keine Dateien dieses Computers.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_paths_cannot_escape_the_approved_handle() {
        assert!(relative_parts("").unwrap().is_empty());
        assert_eq!(relative_parts("src/domain").unwrap(), vec!["src", "domain"]);
        for path in [
            "/outside",
            "../outside",
            "src/../outside",
            "src//domain",
            "C:/outside",
            r"src\outside",
            "name:stream",
            "src\0",
        ] {
            assert!(relative_parts(path).is_err(), "{path}");
        }
    }

    #[test]
    fn parent_matching_respects_components_and_case() {
        assert!(under_path(&path_key(r"c:\DEV\app\tool"), &path_key(r"C:\dev\app")));
        assert!(!under_path(r"C:\dev\apple", r"C:\dev\app"));
        assert!(!under_path(r"C:\dev\app", r"C:\dev\app"));
        assert_eq!(drive_root(r"d:\dev\app"), r"D:\");
        assert!(drive_root(r"\network\share").is_empty());
    }
}
