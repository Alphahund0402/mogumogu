//! Cached volume metadata and recorded project sizes. Refreshing a dashboard
//! never walks a project tree.
use super::Core;
use crate::Result;
use crate::domain::*;
use crate::platform;

const DRIVE_CACHE_SECONDS: i64 = 30;
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
                    searchable: false,
                    search: None,
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
            drive.search = scopes
                .iter()
                .find(|s| {
                    s.kind == ScopeKind::ProjectSearch
                        && s.status != ScopeStatus::Revoked
                        && s.path.eq_ignore_ascii_case(&drive.root)
                })
                .map(|s| DriveSearch {
                    scope_id: s.id,
                    status: s.status,
                    findings: s.published.as_ref().map(|g| g.items),
                    checked_at: s.latest.as_ref().and_then(|g| g.finished_at),
                    partial: s.latest.is_some() && !s.current(),
                });
        }
        overview
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
                scope_id: scope.map(|s| s.id),
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
            searchable: true,
            search: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_matching_respects_components_and_case() {
        assert!(under_path(&path_key(r"c:\DEV\app\tool"), &path_key(r"C:\dev\app")));
        assert!(!under_path(r"C:\dev\apple", r"C:\dev\app"));
        assert!(!under_path(r"C:\dev\app", r"C:\dev\app"));
        assert_eq!(drive_root(r"d:\dev\app"), r"D:\");
        assert!(drive_root(r"\network\share").is_empty());
    }
}
