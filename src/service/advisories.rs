//! Version evidence for the offline advisory watcher. This reads published
//! metadata through the owner; it never scans a root or fetches advisories.
use super::Core;
use crate::clock::Timestamp;
use crate::domain::{InstallState, Scope, ScopeKind, ScopeStatus, SourceClass, package_ecosystem, package_name};
use crate::limits::{ADVISORY_ITEMS, ADVISORY_PAGE, MAX_VISIBLE_SCOPES};
use crate::sha256::{Sha256, hex};
use crate::storage::AdvisoryPackageEvidence;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) enum AdvisorySelection {
    Installed,
    Projects { project_id: Option<i64> },
}

#[derive(Debug, Serialize)]
pub(super) struct AdvisoryInventory {
    basis: String,
    current_scopes: usize,
    incomplete_scopes: usize,
    oldest_scan_at: Option<Timestamp>,
    scope_limit_reached: bool,
    item_limit_reached: bool,
    packages: Vec<AdvisoryPackage>,
    skipped: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    skipped_by_scope: BTreeMap<i64, usize>,
    next_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projects: Option<Vec<AdvisoryProject>>,
}

#[derive(Debug, Serialize)]
struct AdvisoryPackage {
    ecosystem: &'static str,
    name: String,
    version: String,
    scope_id: i64,
    public_source: bool,
    state: InstallState,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum AdvisoryScanState {
    Complete,
    NotScanned,
    Incomplete,
    Unavailable,
    IdentityChanged,
    NotApproved,
}

#[derive(Debug, Serialize)]
struct AdvisoryProject {
    id: i64,
    name: String,
    scope_id: Option<i64>,
    scan_allowed: bool,
    scan_state: AdvisoryScanState,
    scan_finished_at: Option<Timestamp>,
}

impl Core {
    pub(super) fn advisory_inventory(
        &mut self,
        selection: AdvisorySelection,
        offset: usize,
        basis: Option<&str>,
    ) -> Result<AdvisoryInventory> {
        if self.is_demo() {
            return Err(Error::blocked("Schwachstellenabgleich ist nur mit dem lokalen Inventar verfügbar."));
        }
        if offset >= ADVISORY_ITEMS || (offset > 0 && basis.is_none()) {
            return Err(Error::invalid("Ungültige Inventarseite; bitte mit der ersten Seite beginnen."));
        }
        let (mut scopes, scope_limit_reached, project_labels) = match selection {
            AdvisorySelection::Installed => {
                let scopes = self.repo.scopes()?;
                let limit_reached = scopes.len() == MAX_VISIBLE_SCOPES;
                (scopes, limit_reached, None)
            }
            AdvisorySelection::Projects { project_id } => {
                let labels = self.repo.advisory_project_labels(project_id)?;
                if project_id.is_some() && labels.is_empty() {
                    return Err(Error::not_found("Projekt-ID nicht gefunden."));
                }
                let scopes = self.repo.advisory_project_scopes(project_id)?;
                let limit_reached = scopes.len() > MAX_VISIBLE_SCOPES;
                (scopes, limit_reached, Some(labels))
            }
        };
        scopes.truncate(MAX_VISIBLE_SCOPES);
        let projects = project_labels.map(|labels| {
            let by_project: BTreeMap<i64, &Scope> =
                scopes.iter().filter_map(|scope| Some((scope.project_id?, scope))).collect();
            labels
                .into_iter()
                .map(|(id, name)| advisory_project(id, &name, by_project.get(&id).copied()))
                .collect::<Vec<_>>()
        });
        let include_resolved = projects.is_some();
        let scopes = scopes.iter().filter(|scope| {
            matches!(
                scope.kind,
                ScopeKind::Project
                    | ScopeKind::NpmGlobal
                    | ScopeKind::Python
                    | ScopeKind::UvTools
                    | ScopeKind::CargoTools
            )
        });
        let mut inventory_basis = Vec::new();
        let mut generations = Vec::new();
        let mut oldest_scan_at = None;
        for scope in scopes {
            let latest = scope.latest.as_ref();
            inventory_basis.push((scope.id, scope.status, latest.map(|generation| (generation.id, generation.state))));
            if let Some((id, finished_at)) = current_generation(scope) {
                generations.push(id);
                oldest_scan_at = Some(oldest_scan_at.map_or(finished_at, |oldest: Timestamp| oldest.min(finished_at)));
            }
        }
        let token =
            hex(&Sha256::digest(serde_json::to_vec(&(selection, scope_limit_reached, &inventory_basis, &projects))?));
        if basis.is_some_and(|basis| basis != token) {
            return Err(Error::conflict("Inventar geändert; bitte mit der ersten Seite beginnen."));
        }
        let limit = ADVISORY_PAGE.min(ADVISORY_ITEMS - offset);
        let mut rows = self.repo.advisory_packages(&generations, include_resolved, offset, limit + 1)?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let count = rows.len();
        let mut packages = Vec::new();
        let mut skipped_by_scope = BTreeMap::new();
        for row in rows {
            let scope_id = row.scope_id;
            if let Some(package) = advisory_package(row, include_resolved) {
                packages.push(package);
            } else if include_resolved {
                *skipped_by_scope.entry(scope_id).or_default() += 1;
            }
        }
        Ok(AdvisoryInventory {
            basis: token,
            current_scopes: generations.len(),
            incomplete_scopes: inventory_basis.len() - generations.len(),
            oldest_scan_at,
            scope_limit_reached,
            item_limit_reached: more && offset + limit == ADVISORY_ITEMS,
            skipped: count - packages.len(),
            next_offset: (more && offset + limit < ADVISORY_ITEMS).then_some(offset + limit),
            skipped_by_scope,
            projects: if offset == 0 { projects } else { None },
            packages,
        })
    }
}

fn current_generation(scope: &Scope) -> Option<(i64, Timestamp)> {
    let generation = scope.latest.as_ref().filter(|_| scope.current())?;
    Some((generation.id, generation.finished_at?))
}

fn advisory_project(id: i64, name: &str, scope: Option<&Scope>) -> AdvisoryProject {
    let ordered_scan = scope.and_then(current_generation);
    let scan_state = match scope {
        Some(scope) => match scope.status {
            ScopeStatus::Approved if ordered_scan.is_some() => AdvisoryScanState::Complete,
            ScopeStatus::Approved if scope.latest.is_none() => AdvisoryScanState::NotScanned,
            ScopeStatus::Approved => AdvisoryScanState::Incomplete,
            ScopeStatus::Unavailable => AdvisoryScanState::Unavailable,
            ScopeStatus::IdentityChanged => AdvisoryScanState::IdentityChanged,
            ScopeStatus::Revoked => AdvisoryScanState::NotApproved,
        },
        None => AdvisoryScanState::NotApproved,
    };
    AdvisoryProject {
        id,
        // The first IPC page can list all 2,000 projects as well as one package page.
        name: crate::privacy::label(name).chars().take(48).collect(),
        scope_id: scope.map(|s| s.id),
        scan_allowed: scope.is_some_and(|s| matches!(s.status, ScopeStatus::Approved | ScopeStatus::Unavailable)),
        scan_state,
        scan_finished_at: ordered_scan.map(|(_, at)| at),
    }
}

fn advisory_package(row: AdvisoryPackageEvidence, project_mode: bool) -> Option<AdvisoryPackage> {
    let ecosystem = match package_ecosystem(&row.ecosystem) {
        "npm" => "npm",
        "python" => "PyPI",
        "cargo" => "crates.io",
        "go" if project_mode => "Go",
        "nuget" if project_mode => "NuGet",
        "maven" if project_mode => "Maven",
        "composer" if project_mode => "Packagist",
        "rubygems" if project_mode => "RubyGems",
        "pub" if project_mode => "Pub",
        "hex" if project_mode => "Hex",
        "cran" if project_mode => "CRAN",
        "julia" if project_mode => "Julia",
        _ => return None,
    };
    if !matches!(row.source, SourceClass::PublicRegistry | SourceClass::Unknown) {
        return None;
    }
    // Bioconductor packages share the renv adapter but do not live on CRAN.
    if ecosystem == "CRAN" && row.source_host.as_deref() == Some("bioconductor.org") {
        return None;
    }
    let name = match ecosystem {
        "Go" | "Maven" | "RubyGems" | "CRAN" | "Julia" => row.name,
        _ => package_name(&row.ecosystem, &row.name),
    };
    let version = row.version?;
    let name_punctuation = if ecosystem == "Maven" { b"-_.:".as_slice() } else { b"-_.@/".as_slice() };
    if !ascii_identity(&name, name_punctuation) || !ascii_identity(&version, b"-_.+!") {
        return None;
    }
    Some(AdvisoryPackage {
        ecosystem,
        name,
        version,
        scope_id: row.scope_id,
        public_source: row.source == SourceClass::PublicRegistry,
        state: row.state,
    })
}

fn ascii_identity(text: &str, punctuation: &[u8]) -> bool {
    !text.is_empty()
        && text.len() < crate::limits::LABEL_MAX_CHARS
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || punctuation.contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::config::Config;
    use crate::domain::{InstallState, InventoryItem, ItemCategory};
    use crate::service::Request;
    use crate::storage::GenerationResult;
    use std::sync::Arc;

    #[test]
    fn the_largest_project_page_fits_the_ipc_limit() {
        let projects = (0..MAX_VISIBLE_SCOPES)
            .map(|_| AdvisoryProject {
                id: i64::MAX,
                name: "🧪".repeat(48),
                scope_id: Some(i64::MAX),
                scan_allowed: false,
                scan_state: AdvisoryScanState::IdentityChanged,
                scan_finished_at: Some(i64::MAX),
            })
            .collect();
        let packages = (0..ADVISORY_PAGE)
            .map(|_| AdvisoryPackage {
                ecosystem: "crates.io",
                name: "a".repeat(crate::limits::LABEL_MAX_CHARS - 1),
                version: "1".repeat(crate::limits::LABEL_MAX_CHARS - 1),
                scope_id: i64::MAX,
                public_source: false,
                state: InstallState::Installed,
            })
            .collect();
        let page = AdvisoryInventory {
            basis: "a".repeat(64),
            current_scopes: MAX_VISIBLE_SCOPES,
            incomplete_scopes: MAX_VISIBLE_SCOPES,
            oldest_scan_at: Some(i64::MAX),
            scope_limit_reached: true,
            item_limit_reached: true,
            packages,
            skipped: ADVISORY_PAGE,
            skipped_by_scope: (0..ADVISORY_PAGE).map(|n| (i64::MAX - n as i64, ADVISORY_PAGE)).collect(),
            next_offset: Some(ADVISORY_PAGE),
            projects: Some(projects),
        };
        assert!(serde_json::to_vec(&page).unwrap().len() + 512 < crate::limits::IPC_MAX_MESSAGE);
    }

    #[test]
    fn export_is_installed_and_current_without_source_hosts_or_paths() {
        let temp = tempfile::tempdir().unwrap();
        let mut core = Core::open_with(
            Config::new(Some(temp.path().join("data")), false).unwrap(),
            Arc::new(FixedClock::new(1_800_000_000)),
        )
        .unwrap();
        let scope = core.repository().approve_scope(ScopeKind::NpmGlobal, None, "fixture-marker", None).unwrap();
        let request = Request::AdvisoryInventory { offset: 0, basis: None };
        assert!(!request.is_write());
        assert_eq!(core.dispatch(request.clone()).unwrap()["incomplete_scopes"], 1);
        let item = |name, state, source| {
            InventoryItem::new(ItemCategory::Package, "npm", name, "local-fixture/path")
                .state(state)
                .version("1.2.3")
                .source(source, Some("fixture-host.example".into()))
        };
        let generation = core.repository().begin_generation(scope, "fixture").unwrap();
        core.repository()
            .finish_generation(
                generation,
                &GenerationResult {
                    items: vec![
                        item("installed", InstallState::Installed, SourceClass::PublicRegistry),
                        item("other-installation", InstallState::Installed, SourceClass::Unknown),
                        item("resolved", InstallState::Resolved, SourceClass::PublicRegistry),
                        item("private", InstallState::Installed, SourceClass::PrivateRegistry),
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        let old = core.dispatch(request.clone()).unwrap();
        assert_eq!(old["current_scopes"], 1);
        assert_eq!(old["skipped"], 1);
        let packages = old["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages.iter().any(|p| p["name"] == "installed" && p["public_source"] == true));
        assert!(packages.iter().any(|p| p["name"] == "other-installation" && p["public_source"] == false));
        let exported = old.to_string();
        assert!(!exported.contains("fixture-host") && !exported.contains("local-fixture"));

        let partial = core.repository().begin_generation(scope, "fixture").unwrap();
        core.repository()
            .finish_generation(
                partial,
                &GenerationResult { errors: vec!["fixture partial".into()], ..Default::default() },
            )
            .unwrap();
        let next = core.dispatch(request).unwrap();
        assert_eq!(next["incomplete_scopes"], 1);
        assert!(next["packages"].as_array().unwrap().is_empty());
        assert!(
            core.dispatch(Request::AdvisoryInventory { offset: 1, basis: old["basis"].as_str().map(str::to_string) })
                .is_err()
        );
        core.revoke_scope(scope).unwrap();
        let revoked = core.dispatch(Request::AdvisoryInventory { offset: 0, basis: None }).unwrap();
        assert_eq!(revoked["incomplete_scopes"], 0);
        assert!(revoked["packages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn project_export_covers_extended_ecosystems_and_skips_private_and_bioconductor() {
        let temp = tempfile::tempdir().unwrap();
        let mut core = Core::open_with(
            Config::new(Some(temp.path().join("data")), false).unwrap(),
            Arc::new(FixedClock::new(1_800_000_000)),
        )
        .unwrap();
        let project = core.repository().register_project("polyglot", "C:\\fixture\\polyglot").unwrap();
        let scope = core.repository().approve_scope(ScopeKind::Project, Some(project), "fixture-proj", None).unwrap();
        let pkg = |eco, name, ver, state, source, host: Option<&str>| {
            InventoryItem::new(ItemCategory::Package, eco, name, "fixture/lockfile")
                .state(state)
                .version(ver)
                .source(source, host.map(str::to_string))
        };
        let generation = core.repository().begin_generation(scope, "fixture").unwrap();
        core.repository()
            .finish_generation(
                generation,
                &GenerationResult {
                    items: vec![
                        pkg(
                            "composer",
                            "Monolog/Monolog",
                            "3.5.0",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("packagist.org"),
                        ),
                        pkg(
                            "bundler",
                            "RedCloth",
                            "4.3.4",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("rubygems.org"),
                        ),
                        pkg(
                            "pub",
                            "http",
                            "1.2.1",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("pub.dev"),
                        ),
                        pkg(
                            "mix",
                            "phoenix",
                            "1.7.10",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("hex.pm"),
                        ),
                        pkg(
                            "renv",
                            "Rcpp",
                            "1.0.12",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("cloud.r-project.org"),
                        ),
                        pkg(
                            "renv",
                            "Biobase",
                            "2.66.0",
                            InstallState::Resolved,
                            SourceClass::PublicRegistry,
                            Some("bioconductor.org"),
                        ),
                        pkg("julia", "HTTP", "1.10.5", InstallState::Resolved, SourceClass::Unknown, None),
                        pkg(
                            "composer",
                            "acme/private",
                            "1.0.0",
                            InstallState::Resolved,
                            SourceClass::PrivateRegistry,
                            Some("satis.acme.example"),
                        ),
                        pkg("mix", "heroicons", "2.1.1", InstallState::Resolved, SourceClass::Git, None),
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        let exported =
            core.dispatch(Request::AdvisoryProjects { project_id: Some(project), offset: 0, basis: None }).unwrap();
        let packages = exported["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 6, "six supported public/unknown ecosystems exported: {packages:#?}");
        assert_eq!(exported["skipped"], 3, "Bioconductor, private Composer, and Git Mix stay skipped");
        let has = |eco, name, public| {
            packages.iter().any(|p| p["ecosystem"] == eco && p["name"] == name && p["public_source"] == public)
        };
        assert!(has("Packagist", "monolog/monolog", true));
        assert!(has("RubyGems", "RedCloth", true));
        assert!(has("Pub", "http", true));
        assert!(has("Hex", "phoenix", true));
        assert!(has("CRAN", "Rcpp", true));
        assert!(has("Julia", "HTTP", false));
        let text = exported.to_string();
        for forbidden in ["bioconductor", "satis.acme.example", "packagist.org", "fixture/lockfile", "C:\\fixture"] {
            assert!(!text.contains(forbidden), "{forbidden} must not leak into advisory export");
        }
        // Installed-only mode does not export project-only ecosystems.
        let installed = core.dispatch(Request::AdvisoryInventory { offset: 0, basis: None }).unwrap();
        assert!(installed["packages"].as_array().unwrap().is_empty());
    }
}
