# Aufbau und Änderungswege

Ein Cargo-Paket hält den Kern unabhängig von der Oberfläche. Einstieg für
Prüfungen ist `python scripts/check.py`; Container-Nutzung steht in
[DOCKER.md](DOCKER.md). Der native Release-Build bleibt `build.ps1 -NoRun`.
Die [Architekturkarte](ARCHITEKTUR.md) zeigt Einstiegspunkte, Prozesse,
Modulverträge und den Datenfluss einer Erfassung. Hier stehen die konkreten
Änderungswege und Prüfungen.

## Ein Anwendungsfall, mehrere Zugänge

UI und CLI formulieren `service::Request`. Der Besitzerprozess serialisiert
die Arbeit auf einem Worker mit begrenzter Queue; nur dieser Worker hält den
Kern. `service::Core` orchestriert Domänenregeln und Repository-Aufrufe.
SQL bleibt in `storage`, Windows-FFI in `platform/windows.rs`, freigegebenes
Lesen in `fsread` und Entfernen in `cleanup`.

`service/mod.rs` hält Zustand und Lebenszyklus des Kerns. Die Anwendungsfälle
liegen nach Verantwortung in `inventory.rs` (Registrierung, Lesefreigaben,
Scans, Messung, Inventarfundstellen), `discovery.rs` (Projektkandidaten),
`resources.rs` (Scratchpads, Schutz, Erklärungen, Prüfliste),
`sessions.rs` (Läufe und Abgleich) und `maintenance.rs` (Pläne, Einstellungen,
Updates, Export, Sicherung). `packages.rs` lädt Paketdaten und verwaltet den
Seitencache; `packages/analysis.rs` berechnet den Bericht und
`packages/references.rs` bündelt den statischen Referenzabgleich. Beide
Auswertungsmodule erhalten ausschließlich Daten und führen keine I/O aus.
`status.rs` liest Gesamtzähler statt eines vollständigen Snapshots. Snapshot und CLI teilen
dieselben Anwendungsfälle; der Snapshot verwendet seine vorhandenen Projekt-/
Scope-Daten für die zusätzliche Auswertung. Ein begrenzter Paket-Seitencache
gilt nur für dieselbe Repository-Revision und Seitenauswahl und nur, bis die
nächste noch frische Quellenprüfung als veraltet gilt (`analysis::valid_until`).
Jeder erfolgreiche Schreib-Commit invalidiert ihn; kein volles Inventar wird
dauerhaft im Desktop gehalten. Ohne sichtbares Dashboard berechnet der Besitzer
nach Änderungen nur `Core::tray_summary` statt eines vollständigen Snapshots. Die Module ergänzen denselben `Core`; öffentliche Typen
werden weiterhin direkt über `service` exportiert.

| Änderung | Einstieg | Prüfen |
|---|---|---|
| Neues Verhalten | `src/service/request.rs`, zuständiges Service- und Domänenmodul | CLI und UI müssen denselben Request verwenden |
| Paketbelege und Referenzen | `src/service/packages/analysis.rs`, `references.rs` | Versions-/Quellenidentität, unbekannte Nutzung, Abdeckung und Behalten erhalten; `tests/package_evidence.rs` |
| Projektkandidaten | `src/service/discovery.rs`, `src/inventory/discovery.rs` | Dateinamensuche, Registrierung und Lesefreigabe getrennt halten; `tests/package_scopes.rs` |
| Statuszähler | `src/service/status.rs`, `src/storage/summary.rs` | Gesamtbestand zählen; ältere offene Sessions dürfen nicht durch UI-Paging verschwinden |
| Dashboard-Daten | `src/service/snapshot.rs` | Review-Evidenz, unbekannte Größen und vollständige Abdeckung erhalten |
| UI-Lebenszyklus | `src/desktop/mod.rs` | Fenster schließen gibt Ansichten frei; Besitzer und Tray bleiben aktiv |
| UI-Aktionen | `src/desktop/actions.rs` | Callback übersetzt Absicht in Request; keine I/O |
| Formulare und Dialoge | `src/desktop/forms.rs`, `src/desktop/dialogs.rs`, `ui/dialogs.slint` | Fehler erhalten Eingaben; kein versehentlicher zweiter Auftrag |
| UI-Datenmodelle | `src/desktop/view.rs` | Nur betroffene Modelle aktualisieren |
| Seitenlayout | `ui/pages/<seite>.slint` | Gemeinsame Komponenten und Theme verwenden |
| Speicher oder Migration | `src/storage/`, `migrations/` | Bestehende Migrationen unverändert; Backups und neueres Schema testen |

Einfache SQLite-Listenabfragen verwenden den privaten Helfer
`Repository::query`: Er öffnet eine Lesetransaktion, sammelt typisierte Zeilen
und reicht Abfrage- und Konvertierungsfehler weiter. Zusammengehörige Abfragen
bleiben gemeinsam in `Repository::read`; Schreibtransaktionen bleiben in
`Repository::write`. SQL, Sortierung und Limits stehen beim jeweiligen Fachmodul.
`scripts/validate.py` prüft zusätzlich die Abhängigkeiten von Domäne und
Darstellung sowie die Beschränkung von SQLite auf das Speichermodul; erlaubte
Typkonvertierungen und Fehlerwrapper bleiben ausgenommen. Der private
Paket-Auswertungsordner darf weder den Kern noch Repository- oder
Netzwerkclients verwenden; die geliehenen Speicherprojektionen sind Daten.

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

## Kompaktes Windows-Fenster

Die Oberfläche richtet sich an die Arbeit mit Maus und Tastatur neben einem
Editor. Das ist eine Annahme aus dem Produktumfang, keine Nutzerstudie. Die
native Slint-Laufzeit verwendet Zen Maru Gothic (Regular/Bold, japanische Zeichen
eingeschlossen), Fluent-Steuerelemente und den Systemfensterrahmen. Daten und
Entscheidungen bleiben lokal. Die Fonts werden über `ui/app.slint` eingebettet;
Familie und Oberflächenwerte stehen in `ui/theme.slint`. Mochi-Assets, unverändertes
Original und Exportweg beschreibt die [Gestaltung](../design/brand/README.md).
`render_mascot` in `src/desktop/view.rs` wählt Pose, Sprechblase und
Bildunterschrift aus Zustand und Ansicht; `Mascot` in `ui/components.slint`
zeigt Pose 0–3 aus `assets/brand/mascots-v2.png` und 4–6 aus
`assets/brand/mascots-v3-moods.png` mit demselben Innenrand und Maßstab. Die
ursprünglichen Einzelzeichnungen bleiben im Archiv.

Native Renderings für Prüfung und Dokumentation erzeugt
`powershell -File design/screens/render.ps1` nach einem Debug-Build: Er füttert
eine synthetische Momentaufnahme durch das echte `src/desktop/view.rs` in die
kompilierte Oberfläche und rendert mit Slints Software-Renderer nach
`target/screens` (alle Ansichten, Einstieg, Suche, Fortschritt, Ergebnis, Pause,
900 × 560 und 150 %). Es sind keine Fensteraufnahmen; Tray, Fensterrahmen und
Windows-Dialoge bleiben Teil der manuellen [Abnahme](WINDOWS-ABNAHME.md).
Fensterbreitenabhängige Layouts lesen `Store.window-width`, das nur per
`changed width` gesetzt wird; eine direkte Bindung an `width` erzeugt in Slint
eine Layoutschleife.

| Kontext | Umsetzung |
|---|---|
| Fenster und Platz | Start 1080 × 680, Minimum 900 × 560 logische Pixel; 188 px Navigation, 52 px Befehlsleiste, 28 px Statusleiste |
| Arbeitsfläche | Kennzahlen nur in der Übersicht; Projekte als Hauptliste, Speicher und letzte Aktivität daneben; andere Bereiche nutzen die gesamte Inhaltshöhe |
| Dichte und Lesbarkeit | Projektzeilen 32 px, zweizeilige Einträge 48 px, Schaltflächen 28 px; gemeinsame Maße in `ui/theme.slint`, lesbare Schrift statt kleiner skalierter Gesamtansicht |
| Eingabe und Fokus | Strg+K/Strg+F Suche über alle Bereiche, Strg+N geführter Einstieg, F5 Ansicht, F1 Hilfe, Alt+1…9 und Alt+0 Navigation; Esc schließt Dialog, Suche oder Einstieg; Dialogschluss stellt das Tastaturziel wieder her |
| Große Bestände | Listen bleiben virtualisiert; KI-Profile scrollen horizontal, Pläne in einer eigenen begrenzten Liste, Einstellungen vertikal |
| Verbindung und Schutz | Keine zusätzliche Netzaktivität; Modus, Unbekannt-Zustände, Lesefreigaben und Planbestätigungen bleiben sichtbar |

Die HTML-Designvorschau ist ein unabhängiger früherer Entwurf. Änderungen an
der Windows-Oberfläche werden am kompilierten Slint-Layout geprüft.

## Performance nachvollziehbar prüfen

```powershell
cargo test --release --locked --no-default-features --test performance -- --ignored --nocapture
.\scripts\measure.ps1 -Cycles 100 -IdleSeconds 600
```

Die Referenzfixture prüft 50 Projekte mit 10 000 Paketinstallationen sowie
je 30 Snapshots derselben warmen Datenbank mit fester und mit laufender Uhr,
dazu die Tray-Zusammenfassung, und meldet Median und p95. Tray,
geöffnetes Fenster und Rückkehr zum Tray werden getrennt gemessen. Ein
Compiler- oder Docker-Lauf beeinflusst die Messung; Hintergrundlast und
Messdauer gehören deshalb in den Bericht. Ziele sind keine Messergebnisse.
Sicherheitsprüfungen und Journal-Haltbarkeit werden für Performance nie gelockert.
