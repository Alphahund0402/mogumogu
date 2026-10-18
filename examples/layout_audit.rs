//! Render the real Slint dashboard and export its computed item geometry.
//!
//! PowerShell: `$env:SLINT_EMIT_DEBUG_INFO='1'; cargo run --locked --example layout_audit`.
//! Add `-- target/layout-audit-quick --quick` for focused iteration.
//! Only synthetic fixture strings are used. Outputs live under target/layout-audit.
//! The private inspection API is deliberately confined to this version-pinned QA tool.

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!("layout_audit requires the desktop feature");
}

#[cfg(feature = "desktop")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    audit::run()
}

#[cfg(feature = "desktop")]
mod audit {
    use serde_json::{Value, json};
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Key, Platform, PointerEventButton, WindowAdapter, WindowEvent};
    use slint::private_unstable_api::re_exports::{
        Clip, ComplexText, Flickable, Item, ItemRc, LogicalLength, Orientation, SimpleText, TextHorizontalAlignment,
        TextInput, TextVerticalAlignment, WindowInner,
    };
    use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
    use std::{ops::ControlFlow, path::Path, rc::Rc};

    #[allow(missing_debug_implementations)]
    mod ui {
        slint::include_modules!();
    }

    struct RenderPlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for RenderPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }

    fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
        Rc::new(VecModel::from(rows)).into()
    }

    const LONG: &str = "Ein sehr langer synthetischer Projektname zur Prüfung von Umbruch und Abständen";
    const PATH: &str = r"C:\fixture\workspace\ein-sehr-langer-projektordner\packages\beispiel-mit-langem-namen";

    fn populate(app: &ui::Dashboard, stress: bool) {
        let s = app.global::<ui::Store>();
        let title = if stress { LONG } else { "Beispielprojekt" };
        let detail = if stress {
            "Zusätzliche Beschreibung mit mehreren Sätzen für verlässliche Prüfung langer Texte. Die Eingaben sind ausschließlich synthetische Testdaten."
        } else {
            "Synthetische lokale Testdaten"
        };
        s.set_status(detail.into());
        s.set_monitoring_text("Beobachtung aktiv".into());
        s.set_mascot_line("Alles bleibt geschützt, bis du es geprüft hast.".into());
        s.set_mascot_caption("Dein Inventarhelfer".into());
        s.set_project_count("24".into());
        s.set_package_count("12.450".into());
        s.set_ai_count("128".into());
        s.set_review_count(42);
        s.set_count_all("14.271".into());
        s.set_count_observed("12.450".into());
        s.set_count_review("42".into());
        s.set_count_unknown("1.779".into());
        s.set_cleanup_status("Produktive Bereinigung ist gesperrt. Testpläne bleiben überprüfbar.".into());
        s.set_package_coverage(detail.into());
        s.set_package_page_label("1–40 von 12.450 Paketen".into());
        s.set_package_counts(model(vec!["12.450".into(); 5]));
        s.set_package_next(true);
        s.set_package_previous(true);
        s.set_coverage_caption(detail.into());
        s.set_scratch_root(PATH.into());
        s.set_data_path(r"C:\fixture\mogumogu".into());
        s.set_sqlite_version("3.51.0".into());
        s.set_autostart_note(detail.into());
        s.set_projects(model(
            (0..12)
                .map(|id| ui::ProjectRow {
                    id,
                    name: title.into(),
                    path: PATH.into(),
                    ecosystem: "npm · Cargo · Python".into(),
                    state: "Geschützt".into(),
                    state_kind: "protected".into(),
                    size: "Unbekannt".into(),
                    detail: detail.into(),
                    approved: true,
                    coverage: "Vollständig".into(),
                    coverage_kind: "ok".into(),
                })
                .collect(),
        ));
        s.set_resources(model(
            (0..12)
                .map(|id| ui::ResourceRow {
                    id,
                    name: title.into(),
                    kind: "Scratchpad".into(),
                    owner: title.into(),
                    size: "1.234,56 MiB".into(),
                    state: "Geschützt".into(),
                    state_kind: "protected".into(),
                    detail: detail.into(),
                })
                .collect(),
        ));
        s.set_artifacts(s.get_resources());
        s.set_ai(model(
            (0..12)
                .map(|index| ui::AiRow {
                    index,
                    client: "Claude Code".into(),
                    kind: "Projekteinstellungen".into(),
                    detail: detail.into(),
                    path: PATH.into(),
                    letter: "C".into(),
                    references: 128,
                    scope: "Projekt".into(),
                })
                .collect(),
        ));
        s.set_profiles(model(
            [
                "Agent Skills",
                "Codex",
                "Claude Code",
                "GitHub Copilot / VS Code",
                "Cursor",
                "Gemini CLI",
                "Windsurf / Cascade",
                "Cline",
                "Roo Code",
                "OpenCode",
                "Continue",
                "Aider",
            ]
            .into_iter()
            .map(|name| ui::ProfileChip { name: name.into(), status: "Experimentell".into() })
            .collect(),
        ));
        s.set_events(model(
            (0..12)
                .map(|_| ui::EventRow {
                    title: title.into(),
                    detail: detail.into(),
                    time: "vor 12 Minuten".into(),
                    kind: "ok".into(),
                })
                .collect(),
        ));
        s.set_packages(model(
            (0..12)
                .map(|id| ui::PackageRow {
                    key: id.to_string().into(),
                    name: if stress {
                        "@beispiel/ein-extrem-langer-paketname-mit-zahlreichen-segmenten".into()
                    } else {
                        "@beispiel/paket".into()
                    },
                    version: "123.456.789-beta.12".into(),
                    source: "npm · registry.npmjs.org".into(),
                    upstream: if stress {
                        "Lockfile: abgekündigt · älter".into()
                    } else {
                        "Update verfügbar".into()
                    },
                    upstream_kind: "review".into(),
                    references: "128 Projekte".into(),
                    update: "124.0.0".into(),
                    review: true,
                    kept: false,
                })
                .collect(),
        ));
        s.set_adapters(model(
            (0..9)
                .map(|id| ui::AdapterRow {
                    id: id.to_string().into(),
                    name: "Paketmanager".into(),
                    inventory: "Statisch".into(),
                    updates: "Opt-in".into(),
                    cleanup: "Gesperrt".into(),
                    counts: "12.450 Pakete".into(),
                    formats: "package.json · package-lock.json".into(),
                    limits: detail.into(),
                })
                .collect(),
        ));
        s.set_hints(model(
            (0..12)
                .map(|id| ui::HintRow {
                    id,
                    name: title.into(),
                    kind: "Prüfen".into(),
                    trigger: detail.into(),
                    evidence: detail.into(),
                    size: "Unbekannt".into(),
                    protected: true,
                    plannable: false,
                    stalled: false,
                })
                .collect(),
        ));
        s.set_scopes(model(
            (0..8)
                .map(|id| ui::ScopeRow {
                    id,
                    kind: "Projekt".into(),
                    path: PATH.into(),
                    status: "Freigegeben".into(),
                    coverage: detail.into(),
                    last: "vor 12 Minuten".into(),
                    watched: true,
                    healthy: true,
                })
                .collect(),
        ));
        s.set_experiments(model(
            (0..8)
                .map(|id| ui::ExperimentRow {
                    id,
                    name: title.into(),
                    project: title.into(),
                    purpose: detail.into(),
                    state: "Aktiv".into(),
                    state_kind: "ok".into(),
                })
                .collect(),
        ));
        s.set_experiment_id(0);
        s.set_experiment_name(title.into());
        s.set_experiment_path(PATH.into());
        s.set_experiment_review("Prüfung erforderlich".into());
        s.set_experiment_review_kind("review".into());
        s.set_experiment_stall(detail.into());
        s.set_experiment_plannable(true);
        s.set_experiment_sessions(model(
            (0..5)
                .map(|id| ui::SessionRow {
                    id,
                    scratch: title.into(),
                    scratch_id: 0,
                    purpose: detail.into(),
                    state: "Angehalten".into(),
                    state_kind: "review".into(),
                    started: "vor 12 Minuten".into(),
                    stall: detail.into(),
                    releasable: true,
                    note: detail.into(),
                })
                .collect(),
        ));
        s.set_experiment_folders(model(vec![
            ui::FolderRow {
                name: PATH.into(),
                size: "Unbekannt".into(),
                fate: "Geschützt".into(),
                fate_kind: "protected".into()
            };
            5
        ]));
        let system = model(
            (0..6)
                .map(|id| ui::SystemRow {
                    id: id.to_string().into(),
                    name: title.into(),
                    location: PATH.into(),
                    evidence: detail.into(),
                    evidence_kind: "neutral".into(),
                    note: detail.into(),
                    approvable: true,
                    approved: false,
                    coverage: detail.into(),
                    scope_id: id,
                })
                .collect(),
        );
        s.set_system_tools(system.clone());
        s.set_system_stores(system.clone());
        s.set_system_ai(system);
        s.set_system_checked("vor 12 Minuten".into());
        s.set_attention(model(vec![
            ui::AttentionCard {
                kind: "review".into(),
                label: "Prüfliste".into(),
                count: "42".into(),
                title: title.into(),
                detail: detail.into(),
                action: "Prüfliste öffnen".into(),
                page: 7
            };
            3
        ]));
        s.set_coverage_rows(model(vec![
            ui::LegendRow {
                label: "Beobachtet".into(),
                value: "12.450".into(),
                color: slint::Color::from_rgb_u8(127, 176, 221),
                share: 0.8
            };
            3
        ]));
        s.set_disk_checked("Laufwerksstand: synthetische Beispieldaten".into());
        s.set_disk_note("Projektgrößen sind logische Dateigrößen. Gemeinsame Dateien und Hardlinks können mehrfach vorkommen; keine zugesagte Ersparnis.".into());
        s.set_drives(model(
            [
                ("C:\\", "System", "512 GB", "350 GB", "162 GB", 0.68),
                ("D:\\", "Entwicklung", "2.000 GB", "820 GB", "1.180 GB", 0.41),
            ]
            .into_iter()
            .map(|(root, label, total, used, free, usage)| ui::DriveRow {
                root: root.into(),
                label: label.into(),
                kind: "Lokales Laufwerk".into(),
                total: if stress && root == "D:\\" { "Unbekannt" } else { total }.into(),
                used: if stress && root == "D:\\" { "Unbekannt" } else { used }.into(),
                free: if stress && root == "D:\\" { "Unbekannt" } else { free }.into(),
                projects: if stress && root == "D:\\" { "Unbekannt" } else { "42,0 GB" }.into(),
                usage: if stress && root == "D:\\" { 0. } else { usage },
                project_share: 0.08,
                note: "Metadaten vom Betriebssystem".into(),
            })
            .collect(),
        ));
        s.set_disk_projects(model(
            (1..4)
                .map(|id| ui::DiskProjectRow {
                    id,
                    name: title.into(),
                    path: PATH.into(),
                    drive: "C:\\".into(),
                    size: "14,0 GB".into(),
                    share: "2,73 % des Laufwerks".into(),
                    fraction: 0.0273,
                    scope_id: id,
                    note: detail.into(),
                })
                .collect(),
        ));
        s.set_tree_scopes(model(vec![ui::TreeScopeRow { id: 1, label: title.into(), path: PATH.into() }]));
        s.set_tree_scope_id(1);
        s.set_tree_path(PATH.into());
        s.set_tree_note("Momentaufnahme · 4 Einträge · keine Links verfolgt".into());
        s.set_tree_rows(model(
            [
                ("source", "120 MB", true),
                ("environment", "1,4 GB", true),
                ("results", "Unbekannt", true),
                ("README.md", "0 MB", false),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (name, size, is_dir))| ui::TreeRow {
                index: index as i32,
                name: name.into(),
                path: name.into(),
                size: size.into(),
                is_dir,
                note: if is_dir { "Gemessener Ordner".into() } else { "Datei · Inhalt nicht gelesen".into() },
            })
            .collect(),
        ));
        s.set_content_text("---\nname: beispiel-skill\ndescription: Ein lokaler Beispiel-Skill\n---\n\n# Hinweise\n\n1. Prüfe die Anforderungen.\n2. Lies die zugehörigen Dateien.\n3. Halte deine Ergebnisse fest.\n\n```rust\nfn beispiel() {\n    println!(\"Nur Text, wird nicht ausgeführt\");\n}\n```\n".repeat(if stress {12} else {1}).into());
        s.set_content_note("Schreibgeschützt · Text markieren und kopieren möglich".into());
        s.set_sources(model(vec![ui::SourceToggle { host: "registry.npmjs.org".into(), enabled: true }; 5]));
        s.set_legend(s.get_coverage_rows());
        s.set_donut_value("1,23 GiB".into());
        s.set_storage_subtitle(detail.into());
        s.set_candidates(model(
            (0..8)
                .map(|index| ui::CandidateRow {
                    index,
                    path: PATH.into(),
                    evidence: detail.into(),
                    status: "Unbekannt".into(),
                })
                .collect(),
        ));
        s.set_plans(model(
            (0..6)
                .map(|id| ui::PlanRow {
                    id,
                    state: "Entwurf".into(),
                    created: "vor 12 Minuten".into(),
                    summary: title.into(),
                    fingerprint: "0123456789abcdef0123456789abcdef".into(),
                })
                .collect(),
        ));
        s.set_panel_title(title.into());
        s.set_panel_source("npm · registry.npmjs.org".into());
        s.set_panel_status("Update verfügbar".into());
        s.set_panel_status_kind("review".into());
        s.set_panel_advice(detail.into());
        s.set_panel_note(detail.into());
        s.set_panel_chain(model(vec![ui::ChainStep { label: title.into(), detail: detail.into(), warn: true }; 4]));
        s.set_form_title("Experiment registrieren".into());
        s.set_form_text(detail.into());
        s.set_form_label_1("Experimentname".into());
        s.set_form_label_2("Lokaler Windows-Pfad".into());
        s.set_form_value_1(title.into());
        s.set_form_value_2(PATH.into());
        s.set_form_submit("Geschützt registrieren".into());
        s.set_form_error(detail.into());
        s.set_detail_title(title.into());
        s.set_detail_path(PATH.into());
        s.set_detail_body(detail.repeat(8).into());
        s.set_detail_rows(model(vec![ui::DetailRow { label: "Beobachtungsstatus".into(), value: detail.into() }; 8]));
        s.set_detail_actions(model(vec![
            ui::DetailAction { id: "inspect".into(), label: "Erneut prüfen".into(), primary: true },
            ui::DetailAction { id: "keep".into(), label: "Behalten".into(), primary: false },
        ]));
        s.set_guide_path(PATH.into());
        s.set_guide_name(title.into());
        s.set_guide_error(detail.into());
        s.set_guide_result(detail.into());
        s.set_scan_text(detail.into());
        s.set_scan_progress(0.6);
        s.set_search_results(model(vec![
            ui::SearchRow {
                area: if stress { "ai".into() } else { "project".into() },
                area_label: if stress { "KI-Konfiguration".into() } else { "Projekt".into() },
                key: "0".into(),
                title: title.into(),
                detail: detail.into()
            };
            8
        ]));
    }

    fn inspect(item: &ItemRc) -> Value {
        let geometry = item.geometry();
        let position = item.map_to_window(geometry.origin);
        let infos: Vec<_> = (0..item.element_count().unwrap_or(0))
            .flat_map(|index| {
                item.element_type_names_and_ids(index)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(kind, id)| json!({"type": kind.as_str(), "id": id.as_str()}))
            })
            .collect();
        let mut result = json!({"elements": infos, "x": position.x, "y": position.y,
            "width": geometry.width(), "height": geometry.height(), "visible": item.is_visible()});
        result["clips_children"] = json!(item.borrow().as_ref().clips_children());
        result["visibility_clip"] =
            json!(item.downcast::<Clip>().is_some_and(|clip| clip.as_pin_ref().is_visibility_clip()));
        result["scroll_container"] = json!(item.downcast::<Flickable>().is_some());
        let adapter = item.window_adapter().unwrap();
        macro_rules! text_metrics {
            ($text:expr) => {{
                let text = $text.as_pin_ref();
                let horizontal = text.layout_info(Orientation::Horizontal, geometry.height(), &adapter, item);
                let vertical = text.layout_info(Orientation::Vertical, geometry.width(), &adapter, item);
                let w = horizontal.preferred.min(geometry.width());
                let h = vertical.preferred;
                let dx = match text.horizontal_alignment() { TextHorizontalAlignment::Center => (geometry.width() - w) / 2., TextHorizontalAlignment::Right => geometry.width() - w, _ => 0. };
                let dy = match text.vertical_alignment() { TextVerticalAlignment::Center => (geometry.height() - h) / 2., TextVerticalAlignment::Bottom => geometry.height() - h, _ => 0. };
                result["text"] = json!({"value": text.text().as_str(), "preferred_width": horizontal.preferred, "preferred_height": h,
                    "min_width": horizontal.min, "x": position.x + dx, "y": position.y + dy, "width": w, "height": h});
            }};
        }
        if let Some(text) = item.downcast::<ComplexText>() {
            text_metrics!(text);
            result["text"]["wrap"] = json!(format!("{:?}", text.as_pin_ref().wrap()));
            result["text"]["overflow"] = json!(format!("{:?}", text.as_pin_ref().overflow()));
        } else if let Some(text) = item.downcast::<SimpleText>() {
            text_metrics!(text);
            result["text"]["wrap"] = json!("NoWrap");
            result["text"]["overflow"] = json!("Clip");
        } else if let Some(input) = item.downcast::<TextInput>() {
            let input = input.as_pin_ref();
            let vertical = input.layout_info(Orientation::Vertical, geometry.width(), &adapter, item);
            // Editable text scrolls internally. Inspect its control/line height,
            // not an invented full-string rectangle beyond the input viewport.
            result["input"] = json!({"value": input.text().as_str(), "preferred_height": vertical.preferred});
        }
        let mut children = Vec::new();
        let mut child = item.first_child();
        while let Some(current) = child {
            children.push(inspect(&current));
            child = current.next_sibling();
        }
        result["children"] = json!(children);
        result
    }

    fn frame(
        app: &ui::Dashboard,
        window: &MinimalSoftwareWindow,
        output: &Path,
        name: &str,
        size: PhysicalSize,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        let mut pixels = vec![Rgb8Pixel::default(); (size.width * size.height) as usize];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        }));
        let mut bytes = format!("P6\n{} {}\n255\n", size.width, size.height).into_bytes();
        bytes.extend(pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]));
        std::fs::write(output.join(format!("{name}.ppm")), bytes)?;
        let root = ItemRc::new_root(WindowInner::from_pub(app.window()).component());
        if root.element_count().is_none() {
            return Err("Build with SLINT_EMIT_DEBUG_INFO=1 to retain component names for the layout audit".into());
        }
        root.visit_descendants(|_| ControlFlow::<()>::Continue(()));
        let tree = inspect(&root);
        let record = json!({"name": name, "physical_width": size.width, "physical_height": size.height,
            "scale": window.scale_factor(), "logical_width": app.global::<ui::Store>().get_window_width(), "tree": tree});
        std::fs::write(output.join(format!("{name}.json")), serde_json::to_vec_pretty(&record)?)?;
        Ok(
            json!({"name": name, "physical_width": size.width, "physical_height": size.height, "scale": window.scale_factor()}),
        )
    }

    fn scroll_to_bottom(app: &ui::Dashboard, window: &MinimalSoftwareWindow) {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        let size = window.size();
        let mut pixels = vec![Rgb8Pixel::default(); (size.width * size.height) as usize];
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        });
        let root = ItemRc::new_root(WindowInner::from_pub(app.window()).component());
        root.visit_descendants(|item| {
            if let Some(flickable) = item.downcast::<Flickable>() {
                let flickable = flickable.as_pin_ref();
                let offset = (item.geometry().height() - flickable.content_height().0).min(0.0);
                Flickable::FIELD_OFFSETS.content_y().apply_pin(flickable).set(LogicalLength::new(offset));
            }
            ControlFlow::<()>::Continue(())
        });
    }

    // Exercise disclosure through real pointer events, without test-only UI state.
    fn click_control(app: &ui::Dashboard, window: &MinimalSoftwareWindow, id: &str) {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        let size = window.size();
        let mut pixels = vec![Rgb8Pixel::default(); (size.width * size.height) as usize];
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        });
        let root = ItemRc::new_root(WindowInner::from_pub(app.window()).component());
        let mut position = None;
        root.visit_descendants(|item| {
            let matches = (0..item.element_count().unwrap_or(0)).any(|index| {
                item.element_type_names_and_ids(index)
                    .unwrap_or_default()
                    .iter()
                    .any(|(_, name)| name.as_str().rsplit("::").next() == Some(id))
            });
            let geometry = item.geometry();
            let origin = item.map_to_window(geometry.origin);
            let center = LogicalPosition::new(origin.x + geometry.width() / 2., origin.y + geometry.height() / 2.);
            if matches
                && item.is_visible()
                && center.x > 0.
                && center.y > 0.
                && center.x < size.width as f32 / window.scale_factor()
                && center.y < size.height as f32 / window.scale_factor()
            {
                position = Some(center);
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        });
        let position = position.unwrap_or_else(|| panic!("Visible control {id} not found"));
        window.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
        window.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        window.dispatch_event(WindowEvent::PointerExited);
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let quick = std::env::args().any(|argument| argument == "--quick");
        let output = std::env::args().nth(1).unwrap_or_else(|| "target/layout-audit".into());
        let output = Path::new(&output);
        std::fs::create_dir_all(output)?;
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(RenderPlatform(window.clone())))?;
        let app = ui::Dashboard::new()?;
        app.show()?;
        let mut records = Vec::new();
        for (width, height, scale) in
            [(900, 560, 1.0), (1080, 680, 1.0), (1440, 900, 1.0), (900, 560, 1.5), (900, 560, 2.0)]
        {
            if quick && scale != 1.0 {
                continue;
            }
            window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
            let size = PhysicalSize::new((width as f32 * scale) as u32, (height as f32 * scale) as u32);
            window.set_size(size);
            for stress in [false, true] {
                populate(&app, stress);
                let s = app.global::<ui::Store>();
                let prefix = format!(
                    "{width}x{height}-{}pct-{}",
                    (scale * 100.0) as u32,
                    if stress { "long" } else { "normal" }
                );
                for page in 0..11 {
                    if quick && page != 0 && page != 2 && page != 3 && page != 6 && page != 9 && page != 10 {
                        continue;
                    }
                    s.set_page(page);
                    records.push(frame(&app, &window, output, &format!("{prefix}-page-{page}"), size)?);
                }
                if !quick {
                    s.set_page(1);
                    s.set_show_candidates(true);
                    records.push(frame(&app, &window, output, &format!("{prefix}-candidates"), size)?);
                    s.set_show_candidates(false);
                    s.set_page(5);
                    s.set_resource_tab(3);
                    records.push(frame(&app, &window, output, &format!("{prefix}-artifacts"), size)?);
                    s.set_resource_tab(0);
                    s.set_page(3);
                    click_control(&app, &window, "profile-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-ai-profiles"), size)?);
                    click_control(&app, &window, "profile-toggle");
                    s.set_page(6);
                    click_control(&app, &window, "tool-details-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-tool-details"), size)?);
                    scroll_to_bottom(&app, &window);
                    click_control(&app, &window, "place-details-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-place-details"), size)?);
                    s.set_page(8);
                    click_control(&app, &window, "events-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-activity-events"), size)?);
                    s.set_page(10);
                    click_control(&app, &window, "drive-details-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-drive-details"), size)?);
                    click_control(&app, &window, "drive-details-toggle");
                    click_control(&app, &window, "measurement-toggle");
                    records.push(frame(&app, &window, output, &format!("{prefix}-measurement-details"), size)?);
                    click_control(&app, &window, "measurement-toggle");
                }
                s.set_page(2);
                s.set_package_panel(true);
                records.push(frame(&app, &window, output, &format!("{prefix}-package-panel"), size)?);
                s.set_package_panel(false);
                {
                    s.set_page(10);
                    s.set_storage_tab(1);
                    records.push(frame(&app, &window, output, &format!("{prefix}-folder-tree"), size)?);
                    s.set_storage_tab(0);
                    s.set_modal(3);
                    records.push(frame(&app, &window, output, &format!("{prefix}-skill-content"), size)?);
                    s.set_modal(0);
                    s.set_form_project_names(model(vec!["Beispielprojekt".into(), "Weiteres Projekt".into()]));
                    s.set_form_title("Fund einem Projekt zuordnen".into());
                    s.set_form_label_1("Projekt".into());
                    s.set_form_label_2("Zweck (optional)".into());
                    s.set_form_value_2("Testdaten für den Parser".into());
                    s.set_form_submit("Zuordnen".into());
                    s.set_form_project_picker(true);
                    s.set_form_project_index(0);
                    s.set_modal(2);
                    records.push(frame(&app, &window, output, &format!("{prefix}-adopt-finding"), size)?);
                    s.set_form_project_picker(false);
                    s.set_modal(0);
                }
                if quick {
                    continue;
                }
                s.set_page(2);
                s.set_package_tab(1);
                records.push(frame(&app, &window, output, &format!("{prefix}-adapters"), size)?);
                s.set_package_tab(0);
                for modal in 1..4 {
                    s.set_modal(modal);
                    records.push(frame(&app, &window, output, &format!("{prefix}-modal-{modal}"), size)?);
                }
                s.set_modal(0);
                s.set_page(0);
                s.set_guide_open(true);
                for step in 0..5 {
                    s.set_guide_step(step);
                    records.push(frame(&app, &window, output, &format!("{prefix}-guide-{step}"), size)?);
                }
                s.set_guide_open(false);
                s.set_guide_scan_now(false);
                s.set_guide_open(true);
                for step in [2, 4] {
                    s.set_guide_step(step);
                    records.push(frame(&app, &window, output, &format!("{prefix}-guide-deferred-{step}"), size)?);
                }
                s.set_guide_open(false);
                s.set_guide_scan_now(true);
                s.set_scan_running(true);
                records.push(frame(&app, &window, output, &format!("{prefix}-scan"), size)?);
                s.set_scan_running(false);
                s.set_query("Beispiel".into());
                s.set_search_open(true);
                records.push(frame(&app, &window, output, &format!("{prefix}-search"), size)?);
                s.set_search_open(false);
                s.set_query("".into());
            }
        }
        if !quick {
            populate(&app, true);
            for width in
                [1011, 1012, 1035, 1036, 1049, 1050, 1099, 1100, 1199, 1200, 1215, 1216, 1275, 1276, 1335, 1336]
            {
                window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
                let size = PhysicalSize::new(width, 680);
                window.set_size(size);
                for page in [0, 1, 2, 4, 6, 9] {
                    app.global::<ui::Store>().set_page(page);
                    records.push(frame(
                        &app,
                        &window,
                        output,
                        &format!("{width}x680-100pct-breakpoint-page-{page}"),
                        size,
                    )?);
                }
            }
            for (width, height) in [(900, 560), (1440, 900)] {
                let size = PhysicalSize::new(width, height);
                window.set_size(size);
                populate(&app, true);
                let s = app.global::<ui::Store>();
                let prefix = format!("{width}x{height}-100pct-long-bottom");
                for page in 0..11 {
                    s.set_page(page);
                    scroll_to_bottom(&app, &window);
                    records.push(frame(&app, &window, output, &format!("{prefix}-page-{page}"), size)?);
                }
                s.set_page(2);
                s.set_package_panel(true);
                scroll_to_bottom(&app, &window);
                records.push(frame(&app, &window, output, &format!("{prefix}-package-panel"), size)?);
                s.set_package_panel(false);
                for modal in 1..4 {
                    s.set_modal(modal);
                    scroll_to_bottom(&app, &window);
                    records.push(frame(&app, &window, output, &format!("{prefix}-modal-{modal}"), size)?);
                }
                s.set_modal(0);
                s.set_guide_open(true);
                for step in 0..5 {
                    s.set_guide_step(step);
                    scroll_to_bottom(&app, &window);
                    records.push(frame(&app, &window, output, &format!("{prefix}-guide-{step}"), size)?);
                }
                s.set_guide_open(false);
            }
            app.hide()?;
            drop(app);
            let empty = ui::Dashboard::new()?;
            empty.show()?;
            let size = PhysicalSize::new(900, 560);
            window.set_size(size);
            for page in 0..11 {
                empty.global::<ui::Store>().set_page(page);
                records.push(frame(&empty, &window, output, &format!("900x560-100pct-empty-page-{page}"), size)?);
            }
            let s = empty.global::<ui::Store>();
            s.set_page(1);
            s.set_show_candidates(true);
            records.push(frame(&empty, &window, output, "900x560-100pct-empty-candidates", size)?);
            s.set_page(5);
            s.set_resource_tab(3);
            records.push(frame(&empty, &window, output, "900x560-100pct-empty-artifacts", size)?);
            for tab in [1, 2] {
                s.set_resource_tab(tab);
                records.push(frame(
                    &empty,
                    &window,
                    output,
                    &format!("900x560-100pct-empty-resource-tab-{tab}"),
                    size,
                )?);
            }
            s.set_resource_tab(0);
            s.set_page(10);
            s.set_storage_tab(1);
            records.push(frame(&empty, &window, output, "900x560-100pct-empty-tree", size)?);
            s.set_storage_tab(0);
            s.set_page(2);
            s.set_package_search("kein Treffer".into());
            s.set_package_filter(1);
            records.push(frame(&empty, &window, output, "900x560-100pct-filtered-packages", size)?);
            s.set_page(5);
            s.set_resource_tab(2);
            s.set_candidate_search("kein Treffer".into());
            s.set_candidate_filter(2);
            records.push(frame(&empty, &window, output, "900x560-100pct-filtered-findings", size)?);
            s.set_page(8);
            click_control(&empty, &window, "events-toggle");
            records.push(frame(&empty, &window, output, "900x560-100pct-empty-events", size)?);
            s.set_page(0);
            s.set_search_open(true);
            s.set_query("nicht gefunden".into());
            records.push(frame(&empty, &window, output, "900x560-100pct-empty-search", size)?);
            s.set_search_open(false);
            s.set_detail_title("Erfassung beendet".into());
            s.set_detail_body("Dein Inventar ist aktualisiert.".into());
            s.set_modal(1);
            records.push(frame(&empty, &window, output, "900x560-100pct-short-message", size)?);
            s.set_modal(0);
            s.set_form_title("Scratchpad anlegen".into());
            s.set_form_label_1("Projekt".into());
            s.set_form_label_2("Name".into());
            s.set_form_value_2("parser-spike".into());
            s.set_form_submit("Anlegen".into());
            s.set_form_project_names(model(vec!["Projekt A".into(), "Projekt B".into()]));
            s.set_form_project_picker(true);
            s.set_modal(2);
            records.push(frame(&empty, &window, output, "900x560-100pct-scratch-form", size)?);
            window.dispatch_event(WindowEvent::KeyPressed { text: Key::DownArrow.into() });
            window.dispatch_event(WindowEvent::KeyReleased { text: Key::DownArrow.into() });
            assert_eq!(s.get_form_project_index(), 1, "Project picker must receive initial keyboard focus");
            window.dispatch_event(WindowEvent::KeyPressed { text: Key::Tab.into() });
            window.dispatch_event(WindowEvent::KeyReleased { text: Key::Tab.into() });
            let inner = WindowInner::from_pub(empty.window());
            let focused = inner.focus_item.borrow().upgrade().expect("Name field must receive focus");
            let input = focused.downcast::<TextInput>().expect("Tab must reach the name input");
            assert_eq!(input.as_pin_ref().text().as_str(), "parser-spike");
        }
        std::fs::write(output.join("manifest.json"), serde_json::to_vec_pretty(&records)?)?;
        println!("Rendered and measured {} native Slint states in {}", records.len(), output.display());
        Ok(())
    }
}
