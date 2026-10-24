//! Translates UI intents into owner requests and wires callbacks in one place.
use super::{Desktop, defer_hide, dialogs, later, ui::*, with};
use mogumogu::domain::{PackageFilter, ScopeKind, SearchArea};
use mogumogu::limits::PACKAGE_PAGE;
use mogumogu::service::Request;
use slint::{ComponentHandle, SharedString, TimerMode};
use std::time::Duration;

/// Typing pauses this long before a query reaches the owner.
const DEBOUNCE: Duration = Duration::from_millis(180);

impl Desktop {
    fn detail_action(&mut self, id: &str) {
        if let Some(value) = id.strip_prefix("ai-content:") {
            let (scope_id, rel_path) = if value == "demo" {
                (None, String::new())
            } else {
                let Some((scope, rel)) = value.split_once(':') else { return };
                let Ok(scope) = scope.parse::<i64>() else { return };
                (Some(scope), rel.to_string())
            };
            self.submit(Request::AiContent { scope_id, rel_path });
            return;
        }
        if let Some(index) = id.strip_prefix("adopt-candidate:").and_then(|v| v.parse::<usize>().ok()) {
            self.adopt_candidate_form(index);
            return;
        }
        if let Some(value) = id.strip_prefix("keep-package:") {
            if let Some((retained, key)) = value.split_once(':') {
                self.submit(Request::RetainPackage { key: key.to_string(), retained: retained == "1" });
            }
            return;
        }
        if let Some(key) = id.strip_prefix("package-dialog:") {
            if let Some(pkg) = self.snapshot.packages.packages.iter().find(|p| p.key == key).cloned() {
                self.dialog(dialogs::package(&pkg));
            }
            return;
        }
        if let Some(kind) = id.strip_prefix("add-store:") {
            return self.add_store(kind);
        }
        let mut parts = id.split(':');
        let verb = parts.next().unwrap_or_default();
        let arg = parts.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or_default();
        let extra = parts.next().unwrap_or_default().to_string();
        let request = match verb {
            "approve-project" => Request::ApproveProject { project_id: arg },
            "scan-scope" => Request::Scan { scope_id: Some(arg) },
            "measure-scope" => Request::MeasureScope { scope_id: arg },
            "revoke-scope" => Request::RevokeScope { scope_id: arg },
            "protect" => Request::Protect { resource_id: arg },
            "extend" => Request::Extend { resource_id: arg, days: 7 },
            "promote" => Request::Promote { resource_id: arg },
            "measure" => Request::MeasureResource { resource_id: arg },
            "tidy-plan" => Request::CleanupPlan { project_id: arg, paths: Vec::new() },
            "approve-plan" => Request::CleanupApprove { plan_id: arg, confirmation: extra },
            "apply-plan" => Request::CleanupApply { plan_id: arg },
            "cancel-plan" => Request::CleanupCancel { plan_id: arg },
            "reconcile-plan" => Request::CleanupReconcile { plan_id: arg },
            "open-resource" => return self.open_resource(arg),
            "register-candidate" => Request::RegisterCandidate { scope_id: arg, rel_path: extra },
            "tidy" => Request::Tidy { project_id: Some(arg) },
            "filter-preview" => Request::ContextFilter { scope_id: arg },
            "filter-apply" => Request::ApplyContextFilter { scope_id: arg, fingerprint: extra },
            "filter-all-preview" => Request::ContextFilterAll,
            "filter-all-apply" => Request::ApplyContextFilterAll { fingerprint: extra },
            "found-preview" => Request::FoundProjects { scope_id: None },
            "found-apply" => Request::AdoptFoundProjects { scope_id: None, fingerprint: extra },
            _ => return,
        };
        self.submit(request);
    }

    fn package_query(&mut self) {
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        self.package_query.text = store.get_package_search().to_string();
        self.package_query.filter = match store.get_package_filter() {
            1 => PackageFilter::Review,
            2 => PackageFilter::Deprecated,
            3 => PackageFilter::Unreferenced,
            4 => PackageFilter::Kept,
            _ => PackageFilter::All,
        };
        let query = self.package_query.clone();
        self.submit(Request::Packages { offset: 0, limit: PACKAGE_PAGE, query });
    }

    fn open_search_hit(&mut self, index: usize) {
        let Some(hit) = self.search_hits.get(index).cloned() else { return };
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_search_open(false);
        match hit.area {
            SearchArea::Project => {
                store.invoke_open(1, 0);
                if let Ok(id) = hit.key.parse() {
                    later(move |d| d.open_project(id));
                }
            }
            SearchArea::Resource => {
                let id: i64 = hit.key.parse().unwrap_or(-1);
                store.invoke_open(1, 1);
                later(move |d| d.open_resource(id));
            }
            SearchArea::Package => {
                store.invoke_open(2, 0);
                store.set_package_filter(0);
                store.set_package_search(hit.key.clone().into());
                self.package_key = None;
                self.package_query();
            }
            SearchArea::Ai => {
                store.invoke_open(3, 0);
                if let Some(entry) = hit.key.parse::<usize>().ok().and_then(|i| self.snapshot.ai.get(i).cloned()) {
                    later(move |d| d.dialog(dialogs::ai(&entry)));
                }
            }
            // Personal AI folders live on the AI page, tools and stores on
            // the package page.
            SearchArea::System if hit.key.starts_with("ai:") => store.invoke_open(3, 1),
            SearchArea::System => store.invoke_open(2, 1),
            SearchArea::Discovery => {
                store.invoke_open(1, 2);
                store.set_candidate_filter(0);
                store.set_candidate_search("".into());
                super::view::render_candidates(ui, &self.snapshot);
                if let Some((scope, rel)) = hit.key.split_once(':')
                    && let Ok(scope_id) = scope.parse::<i64>()
                {
                    self.submit(Request::DiscoveryCandidate { scope_id, rel_path: rel.to_string() });
                }
            }
        }
    }

    fn system_approve(&mut self, id: &str) {
        let Some(finding) = self.snapshot.system.findings.iter().find(|f| f.id == id).cloned() else { return };
        let Some(kind) = finding.scope else { return };
        let path = (kind != ScopeKind::Winget).then_some(finding.path);
        self.submit(Request::ApproveSystem { kind, path });
    }
}

pub(super) fn wire(ui: &Dashboard) {
    let actions = ui.global::<Actions>();
    actions.on_refresh(|| with(|d| d.owner.refresh()));
    actions.on_scan_all(|| with(|d| d.submit(Request::Scan { scope_id: None })));
    actions.on_cancel_scan(|| {
        with(|d| {
            d.control.cancel();
            d.status = "Erfassung wird an der nächsten sicheren Stelle angehalten …".into();
            d.render_status();
        })
    });
    actions.on_search(|text: SharedString| {
        let text = text.to_string();
        with(move |d| {
            d.search_timer.start(TimerMode::SingleShot, DEBOUNCE, move || {
                let query = text.clone();
                with(move |d| {
                    if query.trim().chars().count() < 2 {
                        d.search_hits.clear();
                        if let Some(ui) = &d.dashboard {
                            super::view::render_search(ui, &d.search_hits);
                        }
                    } else {
                        d.submit(Request::Search { query });
                    }
                })
            });
        })
    });
    actions.on_open_search_hit(|index| with(move |d| d.open_search_hit(index.max(0) as usize)));
    actions.on_open_project(|id| later(move |d| d.open_project(i64::from(id))));
    actions.on_open_resource(|id| later(move |d| d.open_resource(i64::from(id))));
    actions.on_open_ai(|index| {
        later(move |d| {
            if let Some(entry) = d.snapshot.ai.get(index as usize).cloned() {
                d.dialog(dialogs::ai(&entry));
            }
        })
    });
    actions.on_open_hint(|id| {
        later(move |d| {
            if let Some(hint) = d.snapshot.hints.iter().find(|h| h.resource_id == i64::from(id)).cloned() {
                d.dialog(dialogs::hint(&hint));
            }
        })
    });
    actions.on_open_scope(|id| {
        later(move |d| {
            if let Some(scope) = d.snapshot.scopes.iter().find(|s| s.id == i64::from(id)).cloned() {
                d.dialog(dialogs::scope(&scope));
            }
        })
    });
    actions.on_open_plan(|id| {
        later(move |d| {
            if let Some(plan) = d.snapshot.plans.iter().find(|p| p.id == i64::from(id)).cloned() {
                d.dialog(dialogs::plan(&plan));
            }
        })
    });
    actions.on_open_package(|key| {
        let key = key.to_string();
        with(move |d| {
            d.package_key = Some(key);
            if let Some(ui) = &d.dashboard {
                super::view::render_packages(ui, &d.snapshot.packages, d.package_key.as_deref());
            }
        })
    });
    actions.on_close_package(|| {
        with(|d| {
            d.package_key = None;
            if let Some(ui) = &d.dashboard {
                ui.global::<Store>().set_package_panel(false);
            }
        })
    });
    actions.on_package_page(|offset| {
        with(move |d| {
            let query = d.package_query.clone();
            d.submit(Request::Packages { offset: offset.max(0) as usize, limit: PACKAGE_PAGE, query })
        })
    });
    actions.on_package_query(|| {
        with(|d| {
            d.search_timer.start(TimerMode::SingleShot, DEBOUNCE, || with(Desktop::package_query));
        })
    });
    actions.on_keep_package(|key, retained| {
        let key = key.to_string();
        with(move |d| d.submit(Request::RetainPackage { key, retained }))
    });
    actions.on_open_candidate(|index| later(move |d| d.dialog(dialogs::candidate(&d.snapshot, index as usize))));
    actions.on_open_artifact(|index| later(move |d| d.dialog(dialogs::artifact(&d.snapshot, index as usize))));
    actions.on_filter_candidates(|| {
        with(|d| {
            if let Some(ui) = &d.dashboard {
                super::view::render_candidates(ui, &d.snapshot);
            }
        })
    });
    actions.on_storage_refresh(|| with(|d| d.submit(Request::StorageOverview)));
    actions.on_storage_drive(|drive| {
        let drive = drive.to_string();
        with(move |d| {
            if let Some(ui) = &d.dashboard {
                ui.global::<Store>().set_selected_drive(drive.into());
                super::disk::render(ui, &d.snapshot.disks);
            }
        });
    });
    actions.on_open_access(|index| {
        later(move |d| {
            if let Some(found) = d.snapshot.ai_access.get(index as usize).cloned() {
                d.dialog(dialogs::access(&found));
            }
        })
    });
    actions.on_tidy_report(|project| {
        with(move |d| d.submit(Request::Tidy { project_id: (project >= 0).then_some(i64::from(project)) }))
    });
    actions.on_open_tidy(|project| {
        later(move |d| {
            if let Some(found) = d.snapshot.tidy.projects.iter().find(|p| p.project_id == i64::from(project)).cloned() {
                d.dialog(dialogs::tidy(&found));
            }
        })
    });
    actions.on_adopt_scratchpads(|| with(|d| d.submit(Request::AdoptScratchpads)));
    actions.on_search_drive(|root| {
        let root = root.to_string();
        with(move |d| d.search_drive(&root))
    });
    actions.on_approve_search_folder(|| later(Desktop::approve_search_folder));
    actions.on_add_store(|| later(Desktop::add_store_dialog));
    actions.on_show_help(|| later(|d| d.dialog(dialogs::message("Hilfe", dialogs::HELP))));
    actions.on_detail_action(|id| {
        let id = id.to_string();
        later(move |d| d.detail_action(&id))
    });
    actions.on_submit_form(|a, b| {
        let (a, b) = (a.to_string(), b.to_string());
        later(move |d| d.submit_form(a, b))
    });
    // System
    actions.on_system_refresh(|| with(|d| d.submit(Request::SystemFindings { refresh: true })));
    actions.on_system_approve(|id| {
        let id = id.to_string();
        with(move |d| d.system_approve(&id))
    });
    actions.on_open_scope_scan(|scope| with(move |d| d.submit(Request::Scan { scope_id: Some(i64::from(scope)) })));
    // Adding a project in steps
    actions.on_guide_start(|| later(Desktop::guide_open));
    actions.on_guide_skip(|| later(Desktop::guide_close));
    actions.on_guide_browse(|| later(Desktop::guide_browse));
    actions.on_guide_approve(|| later(Desktop::guide_approve));
    actions.on_guide_finish(|| later(Desktop::guide_finish));
    // Settings
    actions.on_backup_now(|| later(Desktop::backup_now));
    ui.window().on_close_requested(|| {
        // Defer dropping the component until its close callback returned.
        defer_hide();
        slint::CloseRequestResponse::KeepWindowShown
    });
}
