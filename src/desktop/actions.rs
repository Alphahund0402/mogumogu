//! Translates UI intents into owner requests and wires callbacks in one place.
use super::{Desktop, defer_hide, dialogs, later, ui::*, with};
use mogumogu::domain::{PackageFilter, QuietHours, ResourceKind, ScopeKind, SearchArea};
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
        if let Some(paths) = id.strip_prefix("export-confirm:") {
            return self.export_to_folder(paths == "1");
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
            "expendable" => Request::MarkExpendable { resource_id: arg },
            "extend" => Request::Extend { resource_id: arg, days: 7 },
            "promote" => Request::Promote { resource_id: arg },
            "measure" => Request::MeasureResource { resource_id: arg },
            "plan" => Request::CleanupPlan { resource_ids: vec![arg] },
            "approve-plan" => Request::CleanupApprove { plan_id: arg, confirmation: extra },
            "apply-plan" => Request::CleanupApply { plan_id: arg },
            "cancel-plan" => Request::CleanupCancel { plan_id: arg },
            "reconcile-plan" => Request::CleanupReconcile { plan_id: arg },
            "new-scratch" => return self.new_scratch(Some(arg)),
            "open-resource" => return self.open_resource(arg),
            "register-candidate" => Request::RegisterCandidate { scope_id: arg, rel_path: extra },
            _ => return,
        };
        self.submit(request);
    }

    /// Extend, protect, promote and plan act on the scratchpad or its
    /// managed temporary folder; the core re-checks everything.
    fn experiment_action(&mut self, verb: &str, scratch: i64) {
        let request = match verb {
            "extend" => Request::Extend { resource_id: scratch, days: 7 },
            "protect" => Request::Protect { resource_id: scratch },
            "promote" => Request::Promote { resource_id: scratch },
            "plan" => {
                let temporary = self
                    .snapshot
                    .resources
                    .iter()
                    .find(|r| r.parent_id == Some(scratch) && r.kind == ResourceKind::Temporary && r.expendable);
                match temporary {
                    Some(temp) => Request::CleanupPlan { resource_ids: vec![temp.id] },
                    None => return,
                }
            }
            _ => return,
        };
        self.submit(request);
    }

    fn render_experiment(&mut self) {
        if let Some(ui) = &self.dashboard {
            super::view::render_experiments(ui, &self.snapshot, self.experiment);
        }
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
                store.set_page(1);
                if let Ok(id) = hit.key.parse() {
                    later(move |d| d.open_project(id));
                }
            }
            SearchArea::Resource => {
                let id: i64 = hit.key.parse().unwrap_or(-1);
                let scratch = self.snapshot.resources.iter().any(|r| r.id == id && r.kind == ResourceKind::Scratchpad);
                if scratch {
                    store.set_page(4);
                    self.experiment = Some(id);
                    self.render_experiment();
                } else {
                    store.set_page(5);
                    later(move |d| d.open_resource(id));
                }
            }
            SearchArea::Package => {
                store.set_page(2);
                store.set_package_tab(0);
                store.set_package_filter(0);
                store.set_package_search(hit.key.clone().into());
                self.package_key = None;
                self.package_query();
            }
            SearchArea::Ai => {
                store.set_page(3);
                if let Some(entry) = hit.key.parse::<usize>().ok().and_then(|i| self.snapshot.ai.get(i).cloned()) {
                    later(move |d| d.dialog(dialogs::ai(&entry)));
                }
            }
            SearchArea::System => store.set_page(6),
            SearchArea::Discovery => {
                store.set_page(5);
                store.set_resource_tab(2);
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
    actions.on_filter_projects(|| {
        with(|d| {
            if let Some(ui) = &d.dashboard {
                super::view::render_projects(ui, &d.snapshot);
            }
        })
    });
    actions.on_filter_resources(|| {
        with(|d| {
            if let Some(ui) = &d.dashboard {
                super::view::render_resources(ui, &d.snapshot);
            }
        })
    });
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
    actions.on_open_adapter(|id| {
        let id = id.to_string();
        later(move |d| d.dialog(dialogs::adapter(&id)))
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
            d.folder_tree = None;
            if let Some(ui) = &d.dashboard {
                ui.global::<Store>().set_tree_loading(d.folder_pending.is_some());
                ui.global::<Store>().set_selected_drive(drive.into());
                super::disk::clear_tree(ui, "Wähle einen freigegebenen Ordner auf diesem Laufwerk.");
                super::disk::render(ui, &d.snapshot.disks);
            }
        });
    });
    actions.on_tree_open(|scope_id, rel_path| {
        let rel_path = rel_path.to_string();
        with(move |d| d.request_tree(i64::from(scope_id), rel_path));
    });
    actions.on_tree_enter(|index| {
        with(move |d| {
            if let Some(tree) = &d.folder_tree
                && let Some(entry) = tree.entries.get(index as usize)
                && entry.is_dir
            {
                d.request_tree(tree.scope_id, entry.rel_path.clone());
            }
        });
    });
    actions.on_tree_parent(|| {
        with(|d| {
            if let Some(tree) = &d.folder_tree {
                let rel_path = tree.rel_path.rsplit_once('/').map_or("", |(parent, _)| parent).to_string();
                d.request_tree(tree.scope_id, rel_path);
            }
        })
    });
    actions.on_approve_scope(|kind| {
        let kind = kind.to_string();
        later(move |d| d.approve_scope(&kind));
    });
    actions.on_new_project(|| later(|d| d.guide_open(1)));
    actions.on_new_scratch(|| later(|d| d.new_scratch(None)));
    actions.on_show_cli(|| later(|d| d.dialog(dialogs::message("Hilfe und CLI-Befehle", dialogs::CLI_HELP))));
    actions.on_detail_action(|id| {
        let id = id.to_string();
        later(move |d| d.detail_action(&id))
    });
    actions.on_submit_form(|a, b| {
        let (a, b) = (a.to_string(), b.to_string());
        later(move |d| d.submit_form(a, b))
    });
    // Experiments
    actions.on_select_experiment(|id| {
        with(move |d| {
            d.experiment = Some(i64::from(id));
            d.render_experiment();
        })
    });
    actions.on_release_session(|id| later(move |d| d.release_session(i64::from(id))));
    actions.on_recheck_sessions(|| with(|d| d.submit(Request::Sessions)));
    actions.on_experiment_action(|verb, id| {
        let verb = verb.to_string();
        with(move |d| d.experiment_action(&verb, i64::from(id)))
    });
    // System
    actions.on_system_refresh(|| with(|d| d.submit(Request::SystemFindings { refresh: true })));
    actions.on_system_approve(|id| {
        let id = id.to_string();
        with(move |d| d.system_approve(&id))
    });
    actions.on_open_scope_scan(|scope| with(move |d| d.submit(Request::Scan { scope_id: Some(i64::from(scope)) })));
    // Guided start
    actions.on_guide_start(|| later(|d| d.guide_open(if d.snapshot.projects.is_empty() { 0 } else { 1 })));
    actions.on_guide_skip(|| later(Desktop::guide_skip));
    actions.on_guide_disable(|| later(Desktop::guide_disable));
    actions.on_guide_browse(|| later(Desktop::guide_browse));
    actions.on_guide_approve(|| later(Desktop::guide_approve));
    actions.on_guide_sample(|| later(Desktop::guide_skip));
    actions.on_guide_finish(|| later(Desktop::guide_finish));
    // Settings
    actions.on_set_monitoring(|on| {
        with(move |d| {
            d.change_settings(|s| {
                s.monitoring_enabled = on;
                s.monitoring_paused_until = None;
            })
        })
    });
    actions.on_pause_monitoring(|minutes| with(move |d| d.pause_monitoring(minutes)));
    actions.on_set_notifications(|on| with(move |d| d.change_settings(|s| s.notifications_enabled = on)));
    actions.on_set_updates(|on| with(move |d| d.change_settings(|s| s.network_updates_enabled = on)));
    actions.on_set_source(|host: SharedString, on| {
        let host = host.to_string();
        with(move |d| {
            d.change_settings(|s| {
                s.update_sources.retain(|h| *h != host);
                if on {
                    s.update_sources.push(host);
                }
            })
        })
    });
    actions.on_set_guide(|on| with(move |d| d.change_settings(|s| s.guide_enabled = on)));
    actions.on_set_daily_summary(|on| with(move |d| d.change_settings(|s| s.daily_summary = on)));
    actions.on_set_summary_hour(|hour| with(move |d| d.change_settings(|s| s.summary_hour = hour.clamp(0, 23) as u8)));
    actions.on_set_quiet(|on, from, to| {
        with(move |d| {
            let from = from.clamp(0, 23) as u8;
            let mut to = to.clamp(0, 23) as u8;
            if to == from {
                to = (from + 1) % 24;
            }
            d.change_settings(|s| s.quiet_hours = on.then_some(QuietHours { from, to }))
        })
    });
    actions.on_set_autostart(|on| with(move |d| d.submit(Request::SetAutostart { enabled: on })));
    actions.on_save_scratch_root(|path| {
        let path = path.trim().to_string();
        with(move |d| d.change_settings(|s| s.scratch_root = (!path.is_empty()).then_some(path)))
    });
    actions.on_browse_scratch_root(|| later(Desktop::browse_scratch_root));
    actions.on_approve_system(|kind| {
        let kind = match kind.as_str() {
            "scoop" => ScopeKind::Scoop,
            "chocolatey" => ScopeKind::Chocolatey,
            _ => ScopeKind::Winget,
        };
        with(move |d| d.submit(Request::ApproveSystem { kind, path: None }))
    });
    actions.on_check_updates(|| with(|d| d.submit(Request::Updates)));
    actions.on_backup_now(|| later(Desktop::backup_now));
    actions.on_export_now(|paths| later(move |d| d.export_preview(paths)));
    actions.on_hide_dashboard(defer_hide);
    ui.window().on_close_requested(|| {
        // Defer dropping the component until its close callback returned.
        defer_hide();
        slint::CloseRequestResponse::KeepWindowShown
    });
}
