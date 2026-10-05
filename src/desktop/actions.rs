//! Translates UI intents into owner requests and wires callbacks in one place.
use super::{Desktop, defer_hide, dialogs, later, ui::*, with};
use mogumogu::service::Request;
use slint::{ComponentHandle, SharedString};

impl Desktop {
    fn detail_action(&mut self, id: &str) {
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
            _ => return,
        };
        self.submit(request);
    }
}

pub(super) fn wire(ui: &Dashboard) {
    let actions = ui.global::<Actions>();
    actions.on_refresh(|| with(|d| d.owner.refresh()));
    actions.on_scan_all(|| with(|d| d.submit(Request::Scan { scope_id: None })));
    actions.on_search(|_| {
        with(|d| {
            if let Some(ui) = &d.dashboard {
                super::view::render_projects(ui, &d.snapshot);
            }
        })
    });
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
    actions.on_new_project(|| later(Desktop::new_project));
    actions.on_new_scratch(|| later(|d| d.new_scratch(None)));
    actions.on_show_cli(|| later(|d| d.dialog(dialogs::message("Generische CLI", dialogs::CLI_HELP))));
    actions.on_detail_action(|id| {
        let id = id.to_string();
        later(move |d| d.detail_action(&id))
    });
    actions.on_submit_form(|a, b| {
        let (a, b) = (a.to_string(), b.to_string());
        later(move |d| d.submit_form(a, b))
    });
    actions.on_set_monitoring(|on| with(|d| d.change_settings(|s| s.monitoring_enabled = on)));
    actions.on_set_notifications(|on| with(|d| d.change_settings(|s| s.notifications_enabled = on)));
    actions.on_set_updates(|on| with(|d| d.change_settings(|s| s.network_updates_enabled = on)));
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
    actions.on_save_scratch_root(|path| {
        let path = path.trim().to_string();
        with(move |d| d.change_settings(|s| s.scratch_root = (!path.is_empty()).then_some(path)))
    });
    actions.on_approve_system(|kind| {
        let kind = match kind.as_str() {
            "scoop" => mogumogu::domain::ScopeKind::Scoop,
            "chocolatey" => mogumogu::domain::ScopeKind::Chocolatey,
            _ => mogumogu::domain::ScopeKind::Winget,
        };
        with(move |d| d.submit(Request::ApproveSystem { kind, path: None }))
    });
    actions.on_check_updates(|| with(|d| d.submit(Request::Updates)));
    actions.on_hide_dashboard(defer_hide);
    ui.window().on_close_requested(|| {
        // Defer dropping the component until its close callback returned.
        defer_hide();
        slint::CloseRequestResponse::KeepWindowShown
    });
}
