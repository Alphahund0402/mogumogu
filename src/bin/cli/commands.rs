//! Maps command lines to owner requests. Every command is explicit; no
//! command combines registration, program start, protection and approval.
use crate::args::Options;
use mogumogu::clock::{MINUTE, Timestamp};
use mogumogu::domain::{PackageFilter, PackageQuery, QuietHours, ScopeKind};
use mogumogu::limits::PACKAGE_PAGE;
use mogumogu::service::Request;
use mogumogu::{Error, Result};

pub const HELP: &str = r#"mogumogu-cli 0.1.0 — lokales Entwicklungsinventar (Windows 10/11)

Aufruf:  mogumogu-cli [--demo] [--data-dir ABSOLUTER_PFAD] [--json] [--no-start] BEFEHL

Überblick
  status | list | review | sessions | adapters | catalog
  why --resource ID                      Besitzer, Sessions, Schutz, Hinweise

Projekte und Lesefreigaben (Registrierung ≠ Lesefreigabe)
  project register --name NAME --path C:\dev\projekt
  project approve --project ID           Ordner identitätsgebunden zum Lesen freigeben
  system approve --kind scoop|chocolatey|winget|npm-global|uv-tools|python|cargo-tools|nuget-cache [--path PFAD]
  system approve --kind projects --path PFAD   Projektsuche, nur Dateinamen
  system approve --kind user-ai --path PFAD    bekannter KI-Benutzerordner, z. B. %USERPROFILE%\.claude
  system findings [--refresh]            Suchpfad und bekannte Orte, nur Metadaten, nichts gestartet
  search TEXT                            Projekte, Pakete, Ressourcen, KI und System
  project candidates                    gefundene Kandidaten, keine Inhaltsfreigabe
  project accept --scope ID [--relative RELATIVER_PFAD]   Kandidat registrieren
  scope revoke --scope ID
  scan [--scope ID]                      statische Erfassung, startet keine Programme
  measure --resource ID | --scope ID     Größe messen (unbekannt bleibt unbekannt)
  storage                               Laufwerke, Projektgrößen und Speicheranteile
  storage tree --scope ID [--relative PFAD]   freigegebene Ordnerstruktur
  discoveries                           Projekte, Experimente, Umgebungen und Testdaten
  resource adopt --scope ID --relative PFAD --project ID [--purpose TEXT]
  ai content --scope ID --relative PFAD  Skill-/Instruktionstext schreibgeschützt ansehen

Scratchpads und Sessions
  scratch create --project ID --name NAME [--purpose TEXT] [--review-days N]
  scratch register|adopt --project ID --name NAME --path PFAD [--purpose TEXT]
  scratch protect|expendable|promote --resource ID
  scratch extend --resource ID --days N
  run --scratch ID -- PROGRAMM [ARGUMENTE …]   verwalteter Lauf (TEMP → temporary\)
  session register --scratch ID --purpose TEXT [--pid PID]
  session complete --session ID
  session release --session ID --reason TEXT

Bereinigung (nur Testexecutor in registrierten Wegwerf-Testwurzeln, G5 offen)
  cleanup test-root --path PFAD           Ordner mit Markierungsdatei registrieren
  cleanup plan --resource ID [--resource ID …]
  cleanup approve --plan ID --confirm FINGERABDRUCK
  cleanup apply|cancel|reconcile --plan ID

Einstellungen, Updates, Daten
  settings
  settings set monitoring|notifications|updates|guide|daily-summary on|off
  settings set update-source registry.npmjs.org|pypi.org|crates.io|api.nuget.org on|off
  settings set scratch-root PFAD
  settings set pause MINUTEN|off           Überwachung vorübergehend anhalten
  settings set summary-hour 0-23 | quiet-hours 19-8|off
  autostart on|off                        Tray-Besitzer bei Anmeldung starten (nur dieser Benutzer)
  updates check | updates list            nur freigegebene öffentliche Quellen
  packages [--offset N] [--limit N] [--search TEXT] [--filter review|deprecated|unreferenced|kept] [--manager ID]
  packages keep|unkeep --key PAKETSCHLUESSEL   lokale Behalten-Entscheidung
  export [--include-paths] [--to NEUE_DATEI]   standardmäßig ohne Pfade und Namen
  backup --to ABSOLUTER_PFAD
  owner status | owner stop | dashboard [hide]

Registrierung liest, erstellt oder verändert keine Ordner. Gefundene Skills,
Hooks und MCP-Einträge werden nie ausgeführt. JSON enthält lokale Metadaten:
nur bewusst teilen. Exit-Codes: 0 ok, 2 ungültig, 3 blockiert, 4 nicht
gefunden, 5 Konflikt, 6 kein Besitzerprozess/nicht unterstützt, 1 sonstiges.
"#;

/// How the result of a request is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Status,
    List,
    Review,
    Plan,
    Json,
}

#[derive(Debug)]
pub enum Action {
    Help,
    Static(serde_json::Value),
    Call(Request, View),
    SettingsSet { key: String, value: String },
    Run { scratch_id: i64, program: String, args: Vec<String> },
    OwnerStatus,
    OwnerStop,
}

fn opts(args: &[String], with_value: &[&str]) -> Result<Options> {
    Options::parse(args, with_value, &[], &[])
}

pub fn parse(args: &[String]) -> Result<Action> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let rest = |n: usize| &args[n.min(args.len())..];
    let call = |request, view| Ok(Action::Call(request, view));
    match words.as_slice() {
        [] | ["help"] | ["--help"] | ["-h"] => Ok(Action::Help),
        ["status"] => call(Request::Status, View::Status),
        ["list"] => call(Request::Snapshot, View::List),
        ["review"] => call(Request::Review, View::Review),
        ["sessions"] => call(Request::Sessions, View::Json),
        ["adapters"] => Ok(Action::Static(serde_json::to_value(mogumogu::adapters::descriptors())?)),
        ["catalog"] => call(Request::Catalog, View::Json),
        ["storage"] | ["storage", "overview"] => call(Request::StorageOverview, View::Json),
        ["storage", "tree", ..] => {
            let o = opts(rest(2), &["--scope", "--relative"])?;
            call(
                Request::FolderTree {
                    scope_id: o.id("--scope")?,
                    rel_path: o.optional("--relative").unwrap_or_default(),
                },
                View::Json,
            )
        }
        ["discoveries"] => call(Request::ProjectCandidates, View::Json),
        ["resource", "adopt", ..] => {
            let o = opts(rest(2), &["--scope", "--relative", "--project", "--purpose"])?;
            call(
                Request::AdoptCandidate {
                    scope_id: o.id("--scope")?,
                    rel_path: o.text("--relative")?,
                    project_id: o.id("--project")?,
                    purpose: o.optional("--purpose"),
                },
                View::Json,
            )
        }
        ["ai", "content", ..] => {
            let o = opts(rest(2), &["--scope", "--relative"])?;
            call(
                Request::AiContent {
                    scope_id: o.optional_id("--scope")?,
                    rel_path: o.optional("--relative").unwrap_or_default(),
                },
                View::Json,
            )
        }
        ["dashboard"] => call(Request::ShowDashboard, View::Json),
        ["dashboard", "hide"] => call(Request::HideDashboard, View::Json),
        ["owner", "status"] => Ok(Action::OwnerStatus),
        ["owner", "stop"] => Ok(Action::OwnerStop),
        ["why", ..] => {
            call(Request::Why { resource_id: opts(rest(1), &["--resource"])?.id("--resource")? }, View::Json)
        }
        ["project", "register", ..] => {
            let o = opts(rest(2), &["--name", "--path"])?;
            call(Request::RegisterProject { name: o.text("--name")?, path: o.text("--path")? }, View::Json)
        }
        ["project", "approve", ..] => {
            call(Request::ApproveProject { project_id: opts(rest(2), &["--project"])?.id("--project")? }, View::Json)
        }
        ["project", "candidates"] => call(Request::ProjectCandidates, View::Json),
        ["project", "accept", ..] => {
            let o = opts(rest(2), &["--scope", "--relative"])?;
            call(
                Request::RegisterCandidate {
                    scope_id: o.id("--scope")?,
                    rel_path: o.optional("--relative").unwrap_or_default(),
                },
                View::Json,
            )
        }
        ["system"] | ["system", "findings"] => call(Request::SystemFindings { refresh: false }, View::Json),
        ["system", "findings", "--refresh"] => call(Request::SystemFindings { refresh: true }, View::Json),
        ["search", text @ ..] if !text.is_empty() => call(Request::Search { query: text.join(" ") }, View::Json),
        ["autostart", value @ ("on" | "off")] => call(Request::SetAutostart { enabled: *value == "on" }, View::Json),
        ["system", "approve", ..] => {
            let o = opts(rest(2), &["--kind", "--path"])?;
            let kind = ScopeKind::parse(&o.text("--kind")?)
                .filter(|k| *k != ScopeKind::Project)
                .ok_or_else(|| Error::invalid("Unbekannte Art; `help` zeigt die freigebbaren Bereiche."))?;
            call(Request::ApproveSystem { kind, path: o.optional("--path") }, View::Json)
        }
        ["scope", "revoke", ..] => {
            call(Request::RevokeScope { scope_id: opts(rest(2), &["--scope"])?.id("--scope")? }, View::Json)
        }
        ["scan", ..] => {
            call(Request::Scan { scope_id: opts(rest(1), &["--scope"])?.optional_id("--scope")? }, View::Json)
        }
        ["measure", ..] => {
            let o = opts(rest(1), &["--resource", "--scope"])?;
            match (o.optional_id("--resource")?, o.optional_id("--scope")?) {
                (Some(resource_id), None) => call(Request::MeasureResource { resource_id }, View::Json),
                (None, Some(scope_id)) => call(Request::MeasureScope { scope_id }, View::Json),
                _ => Err(Error::invalid("Genau eine der Optionen --resource oder --scope angeben.")),
            }
        }
        ["scratch", "create", ..] => {
            let o = opts(rest(2), &["--project", "--name", "--purpose", "--review-days"])?;
            call(
                Request::ScratchCreate {
                    project_id: o.id("--project")?,
                    name: o.text("--name")?,
                    purpose: o.optional("--purpose").unwrap_or_default(),
                    review_days: o.optional_id("--review-days")?,
                },
                View::Json,
            )
        }
        ["scratch", "register", ..] => {
            let o = opts(rest(2), &["--project", "--name", "--path"])?;
            call(
                Request::ScratchRegister {
                    project_id: o.id("--project")?,
                    name: o.text("--name")?,
                    path: o.text("--path")?,
                },
                View::Json,
            )
        }
        ["scratch", "adopt", ..] => {
            let o = opts(rest(2), &["--project", "--name", "--path", "--purpose"])?;
            call(
                Request::ScratchAdopt {
                    project_id: o.id("--project")?,
                    name: o.text("--name")?,
                    path: o.text("--path")?,
                    purpose: o.optional("--purpose"),
                },
                View::Json,
            )
        }
        ["scratch", verb @ ("protect" | "expendable" | "promote"), ..] => {
            let resource_id = opts(rest(2), &["--resource"])?.id("--resource")?;
            let request = match *verb {
                "protect" => Request::Protect { resource_id },
                "expendable" => Request::MarkExpendable { resource_id },
                _ => Request::Promote { resource_id },
            };
            call(request, View::Json)
        }
        ["scratch", "extend", ..] => {
            let o = opts(rest(2), &["--resource", "--days"])?;
            call(Request::Extend { resource_id: o.id("--resource")?, days: o.id("--days")? }, View::Json)
        }
        ["session", "register", ..] => {
            let o = opts(rest(2), &["--scratch", "--purpose", "--pid"])?;
            let pid = o
                .optional_id("--pid")?
                .map(|p| u32::try_from(p).map_err(|_| Error::invalid("--pid ungültig.")))
                .transpose()?;
            call(
                Request::SessionRegister { scratch_id: o.id("--scratch")?, purpose: o.text("--purpose")?, pid },
                View::Json,
            )
        }
        ["session", "complete", ..] => {
            call(Request::SessionComplete { session_id: opts(rest(2), &["--session"])?.id("--session")? }, View::Json)
        }
        ["session", "release", ..] => {
            let o = opts(rest(2), &["--session", "--reason"])?;
            call(Request::SessionRelease { session_id: o.id("--session")?, reason: o.text("--reason")? }, View::Json)
        }
        ["run", ..] => {
            let separator = args
                .iter()
                .position(|a| a == "--")
                .ok_or_else(|| Error::invalid("Trenner -- vor dem Programm fehlt."))?;
            let o = opts(&args[1..separator], &["--scratch"])?;
            let mut command = args[separator + 1..].to_vec();
            if command.is_empty() {
                return Err(Error::invalid("Programm nach -- fehlt."));
            }
            let program = command.remove(0);
            Ok(Action::Run { scratch_id: o.id("--scratch")?, program, args: command })
        }
        ["cleanup", "test-root", ..] => {
            call(Request::RegisterDisposableRoot { path: opts(rest(2), &["--path"])?.text("--path")? }, View::Json)
        }
        ["cleanup", "plan", ..] => {
            let o = Options::parse(rest(2), &["--resource"], &[], &["--resource"])?;
            call(Request::CleanupPlan { resource_ids: o.ids("--resource")? }, View::Plan)
        }
        ["cleanup", "approve", ..] => {
            let o = opts(rest(2), &["--plan", "--confirm"])?;
            call(Request::CleanupApprove { plan_id: o.id("--plan")?, confirmation: o.text("--confirm")? }, View::Plan)
        }
        ["cleanup", verb @ ("apply" | "cancel" | "reconcile"), ..] => {
            let plan_id = opts(rest(2), &["--plan"])?.id("--plan")?;
            let request = match *verb {
                "apply" => Request::CleanupApply { plan_id },
                "cancel" => Request::CleanupCancel { plan_id },
                _ => Request::CleanupReconcile { plan_id },
            };
            call(request, View::Plan)
        }
        ["settings"] => call(Request::GetSettings, View::Json),
        ["settings", "set", key, value] => Ok(Action::SettingsSet { key: key.to_string(), value: value.to_string() }),
        ["settings", "set", "update-source", host, value] => {
            Ok(Action::SettingsSet { key: format!("update-source:{host}"), value: value.to_string() })
        }
        ["updates", "check"] => call(Request::Updates, View::Json),
        ["updates", "list"] | ["updates"] => call(Request::UpdateList, View::Json),
        ["packages", verb @ ("keep" | "unkeep"), ..] => call(
            Request::RetainPackage { key: opts(rest(2), &["--key"])?.text("--key")?, retained: *verb == "keep" },
            View::Json,
        ),
        ["packages", ..] => {
            let o = opts(rest(1), &["--offset", "--limit", "--search", "--filter", "--manager"])?;
            let filter = match o.optional("--filter") {
                Some(text) => PackageFilter::parse(&text)
                    .ok_or_else(|| Error::invalid("--filter: review, deprecated, unreferenced oder kept."))?,
                None => PackageFilter::All,
            };
            let query = PackageQuery {
                text: o.optional("--search").unwrap_or_default(),
                filter,
                ecosystem: o.optional("--manager").unwrap_or_default(),
            };
            let offset = o
                .optional("--offset")
                .map(|v| v.parse::<usize>().map_err(|_| Error::invalid("--offset muss nichtnegativ sein.")))
                .transpose()?
                .unwrap_or(0);
            let limit = o
                .optional("--limit")
                .map(|v| v.parse::<usize>().map_err(|_| Error::invalid("--limit muss positiv sein.")))
                .transpose()?
                .unwrap_or(PACKAGE_PAGE);
            call(Request::Packages { offset, limit, query }, View::Json)
        }
        ["export", ..] => {
            let o = Options::parse(rest(1), &["--to"], &["--include-paths"], &[])?;
            let include_paths = o.flag("--include-paths");
            match o.optional("--to") {
                Some(path) => call(Request::ExportTo { path, include_paths }, View::Json),
                None => call(Request::Export { include_paths }, View::Json),
            }
        }
        ["backup", ..] => call(Request::Backup { path: opts(rest(1), &["--to"])?.text("--to")? }, View::Json),
        _ => Err(Error::invalid("Unbekannter Befehl. `mogumogu-cli help` zeigt alle Befehle.")),
    }
}

/// Applies one `settings set` change to the current settings.
pub fn apply_setting(settings: &mut mogumogu::domain::Settings, key: &str, value: &str, now: Timestamp) -> Result<()> {
    let switch = || match value {
        "on" | "ein" | "true" => Ok(true),
        "off" | "aus" | "false" => Ok(false),
        _ => Err(Error::invalid("Wert on oder off erwartet.")),
    };
    let hour = |text: &str| text.trim().parse::<u8>().ok().filter(|h| *h <= 23);
    match key {
        "monitoring" => settings.monitoring_enabled = switch()?,
        "notifications" => settings.notifications_enabled = switch()?,
        "updates" => settings.network_updates_enabled = switch()?,
        "guide" => settings.guide_enabled = switch()?,
        "daily-summary" => settings.daily_summary = switch()?,
        "scratch-root" => settings.scratch_root = Some(value.to_string()),
        "pause" => {
            settings.monitoring_paused_until = match value {
                "off" | "aus" => None,
                minutes => {
                    let minutes = minutes.parse::<i64>().ok().filter(|m| (1..=7 * 24 * 60).contains(m));
                    Some(now + minutes.ok_or_else(|| Error::invalid("pause: 1–10080 Minuten oder off."))? * MINUTE)
                }
            }
        }
        "summary-hour" => {
            settings.summary_hour = hour(value).ok_or_else(|| Error::invalid("summary-hour: Stunde 0–23."))?;
        }
        "quiet-hours" => {
            settings.quiet_hours = match value.split_once('-') {
                _ if matches!(value, "off" | "aus") => None,
                Some((from, to)) => Some(QuietHours {
                    from: hour(from).ok_or_else(|| Error::invalid("quiet-hours: zum Beispiel 19-8."))?,
                    to: hour(to).ok_or_else(|| Error::invalid("quiet-hours: zum Beispiel 19-8."))?,
                }),
                None => return Err(Error::invalid("quiet-hours: zum Beispiel 19-8 oder off.")),
            }
        }
        _ if key.starts_with("update-source:") => {
            let host = &key["update-source:".len()..];
            settings.update_sources.retain(|h| h != host);
            if switch()? {
                settings.update_sources.push(host.to_string());
            }
        }
        _ => return Err(Error::invalid(format!("Unbekannte Einstellung {key}."))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn commands_map_to_single_requests() {
        assert!(matches!(parse(&words("cleanup plan --resource 3 --resource 4")).unwrap(),
            Action::Call(Request::CleanupPlan { resource_ids }, View::Plan) if resource_ids == vec![3, 4]));
        assert!(matches!(parse(&words("run --scratch 2 -- cmd /C echo hi")).unwrap(),
            Action::Run { scratch_id: 2, program, args } if program == "cmd" && args.len() == 3));
        assert!(parse(&words("cleanup apply --plan 1 --yes")).is_err(), "no generic --yes");
        assert!(parse(&words("delete --all")).is_err());
        assert!(matches!(parse(&words("storage")).unwrap(), Action::Call(Request::StorageOverview, View::Json)));
        assert!(
            matches!(parse(&words("storage tree --scope 2 --relative source")).unwrap(), Action::Call(Request::FolderTree {scope_id: 2, rel_path}, View::Json) if rel_path == "source")
        );
        assert!(matches!(
            parse(&words("resource adopt --scope 2 --relative test.db --project 3")).unwrap(),
            Action::Call(Request::AdoptCandidate { scope_id: 2, project_id: 3, .. }, View::Json)
        ));
        assert!(matches!(
            parse(&words("ai content --scope 2 --relative AGENTS.md")).unwrap(),
            Action::Call(Request::AiContent { scope_id: Some(2), .. }, View::Json)
        ));
        assert!(parse(&words("storage tree --scope 0")).is_err());
    }
}
