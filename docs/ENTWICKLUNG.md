# Aufbau und Änderungswege

Ein Cargo-Paket hält den Kern unabhängig von der Oberfläche. Einstieg für
Prüfungen ist `python scripts/check.py`; Container-Nutzung steht in
[DOCKER.md](DOCKER.md). Der native Release-Build bleibt `build.ps1 -NoRun`.

## Ein Anwendungsfall, mehrere Zugänge

UI und CLI formulieren `service::Request`. Der Besitzerprozess serialisiert
die Arbeit auf einem Worker mit begrenzter Queue; nur dieser Worker hält den
Kern. `service::Core` orchestriert Domänenregeln und Repository-Aufrufe.
SQL bleibt in `storage`, Windows-FFI in `platform/windows.rs`, freigegebenes
Lesen in `fsread` und Entfernen in `cleanup`.

| Änderung | Einstieg | Prüfen |
|---|---|---|
| Neues Verhalten | `src/service/request.rs`, `src/service/mod.rs`, passende Domänenmodule | CLI und UI müssen denselben Request verwenden |
| Dashboard-Daten | `src/service/snapshot.rs` | Review-Evidenz, unbekannte Größen und vollständige Abdeckung erhalten |
| UI-Lebenszyklus | `src/desktop/mod.rs` | Fenster schließen gibt Ansichten frei; Besitzer und Tray bleiben aktiv |
| UI-Aktionen | `src/desktop/actions.rs` | Callback übersetzt Absicht in Request; keine I/O |
| Formulare und Dialoge | `src/desktop/forms.rs`, `src/desktop/dialogs.rs`, `ui/dialogs.slint` | Fehler erhalten Eingaben; kein versehentlicher zweiter Auftrag |
| UI-Datenmodelle | `src/desktop/view.rs` | Nur betroffene Modelle aktualisieren |
| Seitenlayout | `ui/pages/<seite>.slint` | Gemeinsame Komponenten und Theme verwenden |
| Speicher oder Migration | `src/storage/`, `migrations/` | Bestehende Migrationen unverändert; Backups und neueres Schema testen |

## Kleine, gezielte UI-Updates

`render_snapshot` ersetzt die Datenmodelle, wenn der Kern neue Daten liefert
oder ein Fenster aufgebaut wird. `render_status` aktualisiert ausschließlich
Status und Beschäftigt-Zustand. `render_projects` reagiert auf die Suche und
Projektfilter; `render_resources` auf Ressourcentabs. Dadurch bleiben andere
Listen, Einstellungen und das Speicherdiagramm beim Tippen erhalten.

Die globale Suche führt zur Projektseite. Leere Ergebnisse bieten das
Zurücksetzen von Suche und Filter an; ein leeres Inventar bietet die
Registrierung an. Formularfehler erscheinen beim erhaltenen Formular.
Schreibaktionen zeigen sofort den Beschäftigt-Zustand; Navigation und das
Schließen von Dialogen bleiben möglich.

`ui/pages/pages.slint` ist nur der Seitenindex. Jede Seite besitzt ihre eigene
Datei; `page-card.slint` teilt den Rahmen. Klickbare Flächen verwenden
`Activatable` für Enter, Leertaste, Fokusrahmen und Accessibility-Aktionen.
Lange Listen verwenden `ListView`, damit nur sichtbare Zeilen aufgebaut werden.
Neue UI-Texte müssen die tatsächliche Evidenz und Freigabestufe beschreiben.

## Performance nachvollziehbar prüfen

```powershell
cargo test --release --locked --no-default-features --test performance -- --ignored --nocapture
.\scripts\measure.ps1 -Cycles 100 -IdleSeconds 600
```

Die Referenzfixture prüft 50 Projekte mit 10 000 Paketinstallationen sowie
30 Snapshots derselben warmen Datenbank und meldet Median und p95. Tray,
geöffnetes Fenster und Rückkehr zum Tray werden getrennt gemessen. Ein
Compiler- oder Docker-Lauf beeinflusst die Messung; Hintergrundlast und
Messdauer gehören deshalb in den Bericht. Ziele sind keine Messergebnisse.
Sicherheitsprüfungen und Journal-Haltbarkeit werden für Performance nie gelockert.
