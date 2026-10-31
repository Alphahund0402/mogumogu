//! Maps command lines to owner requests. Every command is explicit; no
//! command combines registration, protection and approval.
use crate::args::Options;
use mogumogu::domain::{
    ArtifactKind, DatabaseEngine, DatabasePlace, DockerObject, PackageFilter, PackageQuery, RunState, ScopeKind,
};
use mogumogu::limits::PACKAGE_PAGE;
use mogumogu::service::{ArtifactRequest, ProjectRef, Request};
use mogumogu::{Error, Result};

pub const HELP: &str = r#"mogumogu-cli 0.1.0 — lokales Entwicklungsinventar (Windows 10/11)

Aufruf:  mogumogu-cli [--demo] [--data-dir ABSOLUTER_PFAD] [--json] [--no-start] BEFEHL

Überblick
  status | list | review | adapters | catalog
  storage approve --path PFAD             eigene Metadatenfreigabe für Ordner oder lokale Festplatte
  storage analyze --scope ID              Speicheranalyse ausdrücklich starten (max. 4 Mio. Einträge, 3 min)
  storage clear                          Analyseverzeichnis aus dem Arbeitsspeicher freigeben
  storage tree|files|types|duplicates --scope ID [--folder N] [--search TEXT] [--sort size|allocated|name|modified] [--offset N] [--limit N]
  storage export --scope ID [--folder N] [--search TEXT] [--offset N] [--limit N]
                                         eine Dateiseite als CSV auf stdout; maximal 200 Zeilen, Namen redigiert
                                         beide auch mit --analysis TOKEN (Feld `analysis` der Antwort):
                                         verweigert, falls inzwischen neu analysiert wurde
  why --resource ID                      Besitzer, Schutz, Hinweise

Projekte und Lesefreigaben (Registrierung ≠ Lesefreigabe)
  project register --name NAME --path C:\dev\projekt
  project approve --project ID           Ordner identitätsgebunden zum Lesen freigeben
  system approve --kind npm-global|uv-tools|python|cargo-tools|nuget-cache [--path PFAD]
  system approve --kind npm-cache|pip-cache|uv-cache|cargo-registry|gradle-caches|maven-repository|pnpm-store|yarn-cache [--path PFAD]
                                         Paket-Cache nur messen (Aufräumbericht nennt den Aufräumbefehl)
  system approve --kind projects --path PFAD   Projektsuche, nur Dateinamen; PFAD auch D:\ für eine lokale Festplatte
  system approve --kind user-ai --path PFAD    bekannter KI-Benutzerordner, z. B. %USERPROFILE%\.claude
  system findings [--refresh]            Suchpfad und bekannte Orte, nur Metadaten, nichts gestartet
  search TEXT                            Projekte, Pakete, Ressourcen, KI und System
  project candidates                    gefundene Kandidaten, keine Inhaltsfreigabe
  project accept --scope ID [--relative RELATIVER_PFAD]   Kandidat registrieren
  project accept-all [--scope ID] [--confirm FINGERABDRUCK]
                                         sichere äußere Projekte der Suche: Vorschau, mit --confirm
                                         alle registrieren und zum Lesen freigeben
  scope revoke --scope ID
  scan [--scope ID]                      statische Erfassung, startet keine Programme
  measure --resource ID | --scope ID     Größe messen (unbekannt bleibt unbekannt)
  storage                               Laufwerke, Projektgrößen und Speicheranteile
  tidy [--project ID]                    Aufräumbericht: wiederherstellbar, prüfen (nur freigegebene Projekte)
  discoveries                           Projekte, Experimente, Tests, Umgebungen und Testdaten
  resource adopt --scope ID --relative PFAD --project ID [--purpose TEXT]
  ai content --scope ID --relative PFAD  Skill-/Instruktionstext schreibgeschützt ansehen
  ai filter --scope ID|--all [--confirm FINGERABDRUCK]   Kontextfilter für Claude Code (.claude/settings.local.json): Vorschau, mit --confirm schreiben

Scratchpads
  scratch register|adopt --project ID --name NAME --path PFAD [--purpose TEXT]
  scratch protect|promote --resource ID
  scratch extend --resource ID --days N
  scratch adopt-found                    alle gefundenen Scratchpads geschützt übernehmen

Papierkorb-Plan (nur „Wiederherstellbar“; bis zum Review nur in Wegwerf-Testwurzeln, G5 offen)
  cleanup test-root --path PFAD           Ordner mit Markierungsdatei registrieren
  cleanup plan --project ID [--path RELATIVER_PFAD …]
                                         Vorschau mit Fingerabdruck; bestätigen und ausführen
                                         nur im Dashboard (Übersicht → Bereinigungspläne)
  cleanup cancel|reconcile --plan ID
  cleanup keep --project ID --path RELATIVER_PFAD
                                         Ordner aus jedem Plan heraushalten (wieder vorschlagen nur im Dashboard)
  tidy history --project ID              gespeicherte Aufräumberichte und Wachstum

KI-Läufe (nur Herkunft und Schutz; kein Befehl entfernt etwas)
  run allow off                          Registrierung beenden; einschalten nur im Dashboard (Standard: aus)
  run list [--project ID]
  run start --harness NAME (--project ID | --path ORDNER) [--purpose TEXT]
  run artifact --run ID --kind workspace|scratch|diagnostic|test|code|state|output --path PFAD [--purpose TEXT]
  run artifact --run ID --kind docker --context NAME --type container|image|volume|network --id OBJEKT [--purpose TEXT]
  run state --run ID --state running|paused|succeeded|failed|cancelled [--note TEXT]
                                         Docker-Objekte eines Laufs auch per Label io.mogumogu.run=ID

Datenbanken überwachen (nur Größe aus Metadaten; nie verbunden, nie gelesen; immer geschützt)
  db list | db suggest                   überwachte Datenbanken und Vorschläge
  db register --path ORDNER_ODER_DATEI [--name NAME] [--engine sqlite|duckdb|postgres|mysql|mongodb|redis|other]
  db register --docker-volume NAME --context KONTEXT [--name NAME] [--engine …]
  db measure [--id ID]                   messen (ohne --id alle)
  db history --id ID
                                         Überwachung beenden und Läufe freigeben nur im Dashboard

Docker (nur lesend, nur Kontexte mit lokaler Named Pipe)
  docker contexts                        Kontexte der Docker-Konfiguration (nur meta.json gelesen)
  docker approve --context NAME         Freigabe widerrufen nur im Dashboard (Einbindungen hielten dann nicht mehr)
  docker inventory [--context NAME] [--usage]
                                         Container, Images, Volumes, Netze; --usage mit Docker-Speicherbilanz

Einstellungen und Daten
  settings
  packages [--offset N] [--limit N] [--search TEXT] [--filter review|deprecated|unreferenced|kept] [--manager ID]
  packages keep|unkeep --key PAKETSCHLUESSEL   lokale Behalten-Entscheidung
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
    DiskCsv,
}

#[derive(Debug)]
pub enum Action {
    Help,
    Static(serde_json::Value),
    Call(Request, View),
    OwnerStatus,
    OwnerStop,
}

fn opts(args: &[String], with_value: &[&str]) -> Result<Options> {
    Options::parse(args, with_value, &[])
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
        ["adapters"] => Ok(Action::Static(serde_json::to_value(mogumogu::adapters::descriptors())?)),
        ["catalog"] => call(Request::Catalog, View::Json),
        ["storage"] | ["storage", "overview"] => call(Request::StorageOverview, View::Json),
        ["storage", "clear"] => call(Request::ClearStorageAnalysis, View::Json),
        ["storage", "approve", ..] => {
            let o = opts(rest(2), &["--path"])?;
            call(Request::ApproveSystem { kind: ScopeKind::Storage, path: Some(o.text("--path")?) }, View::Json)
        }
        ["storage", "analyze", ..] => {
            call(Request::AnalyzeStorage { scope_id: opts(rest(2), &["--scope"])?.id("--scope")? }, View::Json)
        }
        ["storage", view @ ("tree" | "files" | "types" | "duplicates" | "export"), ..] => {
            use mogumogu::domain::{DiskQuery, DiskSort, DiskView};
            let o = opts(rest(2), &["--scope", "--analysis", "--folder", "--search", "--sort", "--offset", "--limit"])?;
            let number = |flag: &str| -> Result<usize> {
                o.optional(flag)
                    .map(|v| {
                        v.parse::<usize>()
                            .map_err(|_| Error::invalid(format!("{flag} erwartet eine nichtnegative ganze Zahl.")))
                    })
                    .transpose()
                    .map(|v| v.unwrap_or(0))
            };
            let query = DiskQuery {
                analysis: number("--analysis")? as u64,
                folder: number("--folder")?,
                view: match *view {
                    "tree" => DiskView::Tree,
                    "types" => DiskView::Types,
                    "duplicates" => DiskView::Duplicates,
                    _ => DiskView::Files,
                },
                sort: match o.optional("--sort").as_deref() {
                    None | Some("size") => DiskSort::Size,
                    Some("allocated") => DiskSort::Allocated,
                    Some("name") => DiskSort::Name,
                    Some("modified") => DiskSort::Modified,
                    _ => return Err(Error::invalid("--sort: size, allocated, name oder modified.")),
                },
                text: o.optional("--search").unwrap_or_default(),
                offset: number("--offset")?,
                limit: number("--limit")?,
            };
            call(
                Request::StoragePage { scope_id: o.id("--scope")?, query },
                if *view == "export" { View::DiskCsv } else { View::Json },
            )
        }
        ["ai", "filter", "--all", ..] => match opts(rest(3), &["--confirm"])?.optional("--confirm") {
            Some(fingerprint) => call(Request::ApplyContextFilterAll { fingerprint }, View::Json),
            None => call(Request::ContextFilterAll, View::Json),
        },
        ["ai", "filter", ..] => {
            let o = opts(rest(2), &["--scope", "--confirm"])?;
            let scope_id = o.id("--scope")?;
            match o.optional("--confirm") {
                Some(fingerprint) => call(Request::ApplyContextFilter { scope_id, fingerprint }, View::Json),
                None => call(Request::ContextFilter { scope_id }, View::Json),
            }
        }
        ["tidy", "history", ..] => {
            call(Request::TidyHistory { project_id: opts(rest(2), &["--project"])?.id("--project")? }, View::Json)
        }
        ["tidy", ..] => {
            let o = opts(rest(1), &["--project"])?;
            call(Request::Tidy { project_id: o.optional_id("--project")? }, View::Json)
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
        ["project", "accept-all", ..] => {
            let o = opts(rest(2), &["--scope", "--confirm"])?;
            let scope_id = o.optional_id("--scope")?;
            match o.optional("--confirm") {
                Some(fingerprint) => call(Request::AdoptFoundProjects { scope_id, fingerprint }, View::Json),
                None => call(Request::FoundProjects { scope_id }, View::Json),
            }
        }
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
        ["scratch", verb @ ("protect" | "promote"), ..] => {
            let resource_id = opts(rest(2), &["--resource"])?.id("--resource")?;
            let request = match *verb {
                "protect" => Request::Protect { resource_id },
                _ => Request::Promote { resource_id },
            };
            call(request, View::Json)
        }
        ["scratch", "adopt-found"] => call(Request::AdoptScratchpads, View::Json),
        ["scratch", "extend", ..] => {
            let o = opts(rest(2), &["--resource", "--days"])?;
            call(Request::Extend { resource_id: o.id("--resource")?, days: o.id("--days")? }, View::Json)
        }
        ["cleanup", "test-root", ..] => {
            call(Request::RegisterDisposableRoot { path: opts(rest(2), &["--path"])?.text("--path")? }, View::Json)
        }
        ["cleanup", "plan", ..] => {
            let o = Options::parse(rest(2), &["--project", "--path"], &["--path"])?;
            call(Request::CleanupPlan { project_id: o.id("--project")?, paths: o.texts("--path") }, View::Plan)
        }
        ["cleanup", "approve" | "apply", ..] => Err(Error::blocked(
            "Einen Papierkorb-Plan bestätigen und ausführen geht nur im Dashboard (Übersicht → Bereinigungspläne).",
        )),
        ["run", "allow", "off"] => call(Request::RunsAllow { allowed: false }, View::Json),
        ["run", "allow", "on"] => Err(Error::blocked(
            "KI-Läufe lassen sich nur im Dashboard einschalten (Speicher → Aufräumen → Läufe & Docker …).",
        )),
        ["run", "list", ..] => {
            call(Request::Runs { project_id: opts(rest(2), &["--project"])?.optional_id("--project")? }, View::Json)
        }
        ["run", "start", ..] => {
            let o = opts(rest(2), &["--harness", "--project", "--path", "--purpose"])?;
            let project = match (o.optional_id("--project")?, o.optional("--path")) {
                (Some(id), None) => ProjectRef::Id(id),
                (None, Some(path)) => ProjectRef::Path(path),
                _ => return Err(Error::invalid("Genau eines von --project oder --path angeben.")),
            };
            call(
                Request::RunStart { project, harness: o.text("--harness")?, purpose: o.optional("--purpose") },
                View::Json,
            )
        }
        ["run", "artifact", ..] => {
            let o = opts(rest(2), &["--run", "--kind", "--path", "--context", "--type", "--id", "--purpose"])?;
            let kind = ArtifactKind::parse(&o.text("--kind")?).ok_or_else(|| {
                Error::invalid("--kind: workspace, scratch, diagnostic, test, code, state, output oder docker.")
            })?;
            let docker = match (o.optional("--context"), o.optional("--type"), o.optional("--id")) {
                (Some(context), Some(object), Some(id)) => Some((
                    context,
                    DockerObject::parse(&object)
                        .ok_or_else(|| Error::invalid("--type: container, image, volume oder network."))?,
                    id,
                )),
                (None, None, None) => None,
                _ => return Err(Error::invalid("Ein Docker-Objekt braucht --context, --type und --id.")),
            };
            let artifact =
                ArtifactRequest { kind, path: o.optional("--path"), docker, purpose: o.optional("--purpose") };
            call(Request::RunArtifact { run_id: o.id("--run")?, artifact }, View::Json)
        }
        ["run", "state", ..] => {
            let o = opts(rest(2), &["--run", "--state", "--note"])?;
            let state = RunState::parse(&o.text("--state")?)
                .ok_or_else(|| Error::invalid("--state: running, paused, succeeded, failed oder cancelled."))?;
            call(Request::RunState { run_id: o.id("--run")?, state, note: o.optional("--note") }, View::Json)
        }
        ["db", "list"] | ["db", "suggest"] => call(Request::Databases, View::Json),
        ["db", "register", ..] => {
            let o = opts(rest(2), &["--path", "--docker-volume", "--context", "--name", "--engine"])?;
            let place = match (o.optional("--path"), o.optional("--docker-volume"), o.optional("--context")) {
                (Some(path), None, None) => DatabasePlace::Path(path),
                (None, Some(volume), Some(context)) => DatabasePlace::Volume { context, volume },
                _ => return Err(Error::invalid("Entweder --path oder --docker-volume mit --context angeben.")),
            };
            let engine = match o.optional("--engine") {
                Some(text) => Some(DatabaseEngine::parse(&text).ok_or_else(|| {
                    Error::invalid("--engine: sqlite, duckdb, postgres, mysql, mongodb, redis oder other.")
                })?),
                None => None,
            };
            call(Request::DbRegister { place, name: o.optional("--name"), engine }, View::Json)
        }
        ["db", "measure", ..] => {
            call(Request::DbMeasure { id: opts(rest(2), &["--id"])?.optional_id("--id")? }, View::Json)
        }
        ["db", "history", ..] => call(Request::DbHistory { id: opts(rest(2), &["--id"])?.id("--id")? }, View::Json),
        ["db", "unregister", ..] | ["run", "release", ..] | ["cleanup", "unkeep", ..] | ["docker", "revoke", ..] => {
            Err(Error::blocked("Schutz aufheben geht nur im Dashboard (Speicher → Aufräumen)."))
        }
        ["docker", "contexts"] => call(Request::DockerContexts, View::Json),
        ["docker", "approve", ..] => {
            call(Request::DockerApprove { context: opts(rest(2), &["--context"])?.text("--context")? }, View::Json)
        }
        ["docker", "inventory", ..] => {
            let usage = words.contains(&"--usage");
            let options: Vec<String> = rest(2).iter().filter(|a| *a != "--usage").cloned().collect();
            let context = opts(&options, &["--context"])?.optional("--context");
            call(Request::DockerInventory { context, usage }, View::Json)
        }
        ["cleanup", "keep", ..] => {
            let o = opts(rest(2), &["--project", "--path"])?;
            let request =
                Request::KeepFolder { project_id: o.id("--project")?, rel_path: o.text("--path")?, keep: true };
            call(request, View::Json)
        }
        ["cleanup", verb @ ("cancel" | "reconcile"), ..] => {
            let plan_id = opts(rest(2), &["--plan"])?.id("--plan")?;
            let request = match *verb {
                "cancel" => Request::CleanupCancel { plan_id },
                _ => Request::CleanupReconcile { plan_id },
            };
            call(request, View::Plan)
        }
        ["settings"] => call(Request::GetSettings, View::Json),
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
        ["backup", ..] => call(Request::Backup { path: opts(rest(1), &["--to"])?.text("--to")? }, View::Json),
        _ => Err(Error::invalid("Unbekannter Befehl. `mogumogu-cli help` zeigt alle Befehle.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn commands_map_to_single_requests() {
        assert!(matches!(parse(&words("cleanup plan --project 3 --path web/node_modules --path target")).unwrap(),
            Action::Call(Request::CleanupPlan { project_id: 3, paths }, View::Plan) if paths.len() == 2));
        assert!(parse(&words("run --scratch 2 -- cmd /C echo hi")).is_err(), "no program start");
        assert!(parse(&words("cleanup cancel --plan 1 --yes")).is_err(), "no generic --yes");
        assert!(matches!(
            parse(&words("cleanup reconcile --plan 4")).unwrap(),
            Action::Call(Request::CleanupReconcile { plan_id: 4 }, View::Plan)
        ));
        assert!(parse(&words("delete --all")).is_err());
        assert!(matches!(
            parse(&words("cleanup keep --project 3 --path web\\dist")).unwrap(),
            Action::Call(Request::KeepFolder { project_id: 3, keep: true, .. }, View::Json)
        ));
        assert!(matches!(parse(&words("storage")).unwrap(), Action::Call(Request::StorageOverview, View::Json)));
        assert!(matches!(
            parse(&words(r"run start --harness claude-code --path C:\dev\app")).unwrap(),
            Action::Call(Request::RunStart { project: ProjectRef::Path(_), .. }, View::Json)
        ));
        assert!(parse(&words(r"run start --harness x --project 1 --path C:\dev")).is_err(), "one project only");
        assert!(matches!(
            parse(&words("run artifact --run 2 --kind docker --context default --type volume --id pgdata")).unwrap(),
            Action::Call(Request::RunArtifact { run_id: 2, .. }, View::Json)
        ));
        assert!(parse(&words("run artifact --run 2 --kind docker --context default")).is_err());
        assert!(matches!(
            parse(&words("docker inventory --usage --context desktop-linux")).unwrap(),
            Action::Call(Request::DockerInventory { usage: true, context: Some(_) }, View::Json)
        ));
        assert!(matches!(
            parse(&words(r"db register --path C:\data\app.sqlite")).unwrap(),
            Action::Call(Request::DbRegister { place: DatabasePlace::Path(_), .. }, View::Json)
        ));
        assert!(matches!(
            parse(&words("db register --docker-volume pgdata --context default --engine postgres")).unwrap(),
            Action::Call(Request::DbRegister { engine: Some(DatabaseEngine::Postgres), .. }, View::Json)
        ));
        assert!(parse(&words(r"db register --path C:\x --docker-volume v --context c")).is_err(), "one place");
        for refused in [
            "run delete --run 1",
            "run clean",
            "run release --run 1",
            "cleanup approve --plan 1 --confirm 1a2b3c4d5e6f",
            "cleanup apply --plan 1",
            "db unregister --id 1",
            "cleanup unkeep --project 3 --path web/dist",
            "docker revoke --context default",
            "db drop",
            "docker prune",
            "docker rm --id x",
            "docker stop",
        ] {
            assert!(parse(&words(refused)).is_err(), "{refused}");
        }
        assert!(matches!(
            parse(&words("resource adopt --scope 2 --relative test.db --project 3")).unwrap(),
            Action::Call(Request::AdoptCandidate { scope_id: 2, project_id: 3, .. }, View::Json)
        ));
        assert!(matches!(
            parse(&words("ai content --scope 2 --relative AGENTS.md")).unwrap(),
            Action::Call(Request::AiContent { scope_id: Some(2), .. }, View::Json)
        ));
        assert!(matches!(
            parse(&words("storage tree --scope 2")).unwrap(),
            Action::Call(Request::StoragePage { scope_id: 2, .. }, View::Json)
        ));
        assert!(matches!(
            parse(&words("project accept-all")).unwrap(),
            Action::Call(Request::FoundProjects { scope_id: None }, View::Json)
        ));
        assert!(matches!(
            parse(&words("project accept-all --scope 4 --confirm abc")).unwrap(),
            Action::Call(Request::AdoptFoundProjects { scope_id: Some(4), .. }, View::Json)
        ));
    }
}
