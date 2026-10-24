# mogumogu — Implementierungsplan v0.1

**Ziel:** Lokale, performante Windows-10/11-Tray-Anwendung für Entwicklungsressourcen.
**Stack:** Rust, Slint und SQLite. **Lizenz:** GPL-3.0-only.
**Stand:** 18. Oktober 2026. Die Basis F-01 bis F-28 ist innerhalb der veröffentlichten Unterstützungsmatrix implementiert. Hinzu kommen Systembefund, persönliche KI-Konfiguration und [Entwicklungsfunde mit Zuordnung, Laufwerksdashboard und Skill-Inhaltsansicht](ENTWICKLUNGSFUNDE.md). Am 20. Oktober 2026 wurden verwaltete Läufe, Sessions und das Anlegen von Scratchpads entfernt; Bereinigung verschiebt seither nur wiederherstellbar belegte Ordner in den Papierkorb (F-22 bis F-24). Automatische Zweckbestimmung, vollständige Manager-/Override-Erkennung, Windows-11-Abnahme, unabhängiger Review G5 des Papierkorb-Wegs und manuelle Desktopprüfungen bleiben offen.

## 1. Produkt und Grenze

mogumogu macht sichtbar, welche Entwicklungsressourcen existieren, welchem Projekt oder Experiment sie gehören, welche Nutzung tatsächlich beobachtet wurde und welche Ressourcen eine Prüfung verdienen. Unterstützte Ressourcen können später durch einen bestätigten Plan entfernt werden.

**Keine beobachtete Nutzung ist kein Entbehrlichkeitsnachweis.** Quellcode, Einstellungen, Skills, einzigartige Ergebnisse und unbekannte Inhalte bleiben geschützt. Erkennung und Entfernung sind getrennte Fähigkeiten; mogumogu stellt keine Netzwerkverbindungen her.

Die Zielmatrix ist npm, pnpm, Bun, Yarn, pip, uv, Cargo, NuGet, Go-Module, Gradle und Maven (die letzten fünf seit dem 24. Oktober 2026); WinGet-, Scoop- und Chocolatey-Listen sind entfallen. Dazu kommen generische Scratchpads und Temp-/Buildausgaben sowie versionierte KI-Profile für Agent Skills, Codex, Claude Code, Copilot/VS Code, Cursor, Gemini, Windsurf/Cascade, Cline, Roo Code, OpenCode, Continue und Aider. „Unterstützt“ gilt nur innerhalb einer veröffentlichten Versions-/Fähigkeitsmatrix: [UNTERSTUETZUNGSMATRIX.md](UNTERSTUETZUNGSMATRIX.md).

## 2. Bewusst kleine Startarchitektur

Ein Cargo-Paket mit klar getrennten Modulen, ein Besitzerprozess mit genau einem Datenbank-Worker, eine Tray-Ereignisschleife. Module werden erst zu eigenen Crates, wenn eine echte unabhängige Schnittstelle oder Wiederverwendung existiert. Keine Plugin-DLLs, Microservices, WebView, Graphdatenbank, Async-Runtime oder eigene Regelsprache.

Einstieg: [Architektur und Datenfluss](ARCHITEKTUR.md).
Entwicklung und Prüfungen: [gemeinsamer Runner und Modulaufbau](ENTWICKLUNG.md),
[Linux-Container für portable Kernprüfungen](DOCKER.md). Windows-Laufzeit und
Sicherheitsabnahme bleiben nativ. Der Desktop trennt Lebenszyklus, Callback-Verdrahtung,
Formulare und Modellabbildung; jede Slint-Seite liegt in einer eigenen Datei.

| Entscheidung | Baseline und Grund |
|---|---|
| Rust + Slint | Native UI ohne Browserfrontend; Software-Renderer, Bilder und japanischfähige Zen-Maru-Gothic-Fonts eingebettet |
| SQLite lokal | Kein Server; WAL/FULL, Fremdschlüssel, parametrisierte SQL-Abfragen, kurze Transaktionen, Migration mit Sicherung |
| Ein Besitzerfenster bei Bedarf | Dashboard schließen gibt die Komponente frei; Tray und Kern laufen weiter |
| Ein Besitzerprozess, eine Schreibstelle | Desktop oder `--headless`; CLI spricht über eine benutzergebundene Named Pipe |
| Statische Erkennung als Standard | Handle-relatives Lesen freigegebener Ordner; keine Programmstarts beim Erfassen |
| Lokalmodus als Startstandard, eigene Demo-Datenbank | Echte freigegebene Daten im normalen Start; Beispielvorschau nur explizit mit `-Mode Demo`, ohne Vermischung |
| Eingebaute Paketadapter | Pro Fähigkeit getrennt (Inventar/Bereinigung), keine Fremdcodeausführung |
| Deklarativer KI-Katalog | `profiles/ai-catalog.toml`, beim Build eingebettet und validiert; keine Skript- oder Netzwerkfelder |
| Bereinigung nur in den Papierkorb | Nur wiederherstellbar belegte Ordner, als Ganzes; ausgeführt bis G5 nur in registrierten Wegwerf-Testwurzeln |
| Manueller Build und Start | Kein Dienst, kein Self-Updater, kein Autostart |

## 3. Umgesetzte Basis

| Bereich | Quelle | Stand |
|---|---|---|
| Tray, Fensterlebenszyklus, Dashboard | `src/main.rs`, `src/desktop/`, `ui/` | Sechs Ansichten im Mochi-Wolke-Design, Projekt hinzufügen in drei Schritten, Suche über alle Bereiche, Erfassungsfortschritt mit Abbruch, Tray mit Statuszeile; Dialoge, Tastaturnavigation, gemeinsame Auswahl-/Hover-/Druck-/Fokuszustände; Lokalmodus als Startstandard |
| Besitzerprozess und IPC | `src/owner.rs`, `src/ipc/`, `src/platform/windows.rs` | Ein Worker, Named Pipe (nur aktueller Benutzer, nur lokal, erste Instanz), Protokollversion, 1-MiB-Limit |
| Kern und Anwendungsfälle | `src/service/`, `src/domain/` | Typisierte Zustände und Zustandsautomaten; gemeinsame Anwendungsfälle für UI und CLI |
| SQLite | `src/storage/`, `migrations/` | Schema v4 (Benutzer-Freigaben), Sicherung vor Migration; ausgewählte Paketkanten und versions-/quellengebundener Cache; bestehendes Journal unverändert |
| Sichere Lesegrenze | `src/fsread.rs`, `src/platform/` | Identitätsgebundene Freigaben, `NtCreateFile` relativ zum Elternhandle, keine Reparse Points/Platzhalter |
| Inventar | `src/inventory/`, `src/adapters/`, `src/parsers/`, `src/profiles/` | Elf statische Adapter, zwölf KI-Profile, begrenzte Parser, Datenminimierung |
| Prüfregeln und Bereinigung | `src/review.rs`, `src/cleanup/` | Hinweise mit Evidenz; Papierkorb-Plan aus dem Aufräumbericht, Planbindung per Fingerabdruck, Journal je Ordner, Abgleich nach Absturz |
| CLI | `src/bin/cli/` | Client des Besitzerprozesses, startet ihn bei Bedarf, versioniertes JSON, Exitcodes |
| Qualität | `tests/`, `scripts/validate.py`, `.github/workflows/windows.yml` | Automatisierte Rust-Prüfungen, Lastreferenz, statische Verträge, Clippy ohne Warnungen |
| Build/Start/Release/Messung | `build.ps1`, `start.ps1`, `scripts/` | Release-Build, ZIP mit Prüfsummen und Abhängigkeitsinventar, Messskript |
| Native Renderings | `design/screens/` | Bilder mit synthetischen Daten über den echten Slint-Software-Renderer (`design/screens/render.ps1`); die frühere HTML-Vorschau ist entfernt |

## 4. Dashboardgestaltung

Visuelle Basis ist die **Mochi-Wolke**: cremeweiße Karten auf hellem Himmelblau, 1,5-px-Konturen, Radien 16/12 px, Pillenschaltflächen und eine schmale linke Navigation mit Mochi. Statuschips verbinden immer Farbe, Symbol und Wort. Mochis Pose folgt dem Zustand (fröhlich, mampft beim Erfassen, besorgt bei teilweiser Erfassung) und ist nie die einzige Information. Kein animierter Hintergrund, keine permanenten Diagrammanimationen und keine externe Bildabfrage.

Navigation, Filter, Schaltflächen und Listenzeilen teilen zentrale Zustandsfarben. Ausgewählte Zeilen behalten ihre Markierung über stabile Schlüssel; Fokusrahmen liegen innerhalb der jeweiligen Komponente, auch in virtualisierten Listen. Der normale Build-/Startweg verwendet die lokale Datenbank; fehlende Messungen bleiben unbekannt.

| Bereich | Verhalten |
|---|---|
| Übersicht | Kennzahlen mit Sprung zur Seite; Entscheidungen auf anderen Seiten (Aufräumpotenzial, Abkündigungen, teilweise Erfassung, neu gefundene Orte); Prüfhinweise mit Auslöser und Beleg, letzte Änderungen und Papierkorb-Pläne mit Bestätigung per Fingerabdruck; kein „Alles löschen“; immer Demo-/Lokalstatus zeigen |
| Projekte | Tabelle und Detailansicht mit Lesefreigabe und Abdeckung; Reiter Ressourcen (Besitzer, Typ, bekannte oder unbekannte Größe; Scratchpad-Dialog mit Prüftermin, Schutz und Zum Projekt machen) und Funde (Namenssuche in Ordnern und Laufwerken und erfasste Umgebungen/Ausgaben mit Filtern und expliziter Projektzuordnung) |
| Pakete | Versionen und Quellen mit Projektbelegen, Abdeckung, Quellenstatus und Behalten-Vormerkung; Suche und Filterchips vor dem Blättern; Belegkette im Erklärpanel; Reiter Werkzeuge im Suchpfad und globale Bestände mit Nachweis und Freigabe je Ort |
| KI-Konfiguration | Quellen, Scope, Client und Referenzen; Skill-/Instruktionstexte auf Anfrage schreibgeschützt, flüchtig und redigiert anzeigen; Reiter persönliche Ordner mit Freigabe je Ort; Reiter Zugriff mit Einstufung je Ordner (weitreichend, ohne Filter, begrenzt), Befunden und Kontextfilter-Kandidaten; Kontextfilter nach Vorschau und Bestätigung in `.claude/settings.local.json`, je Projekt oder für alle |
| Speicher | Laufwerkskapazitäten, gemessene logische Projektanteile, Laufwerkssuche nach Projekten und Scratchpads mit einem Klick je lokaler Festplatte, Reiter Aufräumen mit lesendem Bericht je freigegebenem Projekt (wiederherstellbar/prüfen nach Wiederherstellungsbeleg, ruhende Projekte, globale Bestände); die Ordnerstruktur wurde am 24. Oktober 2026 entfernt |
| Einstellungen | Datenort und Sicherung; Reiter Lesefreigaben mit Erfassungsabdeckung je Freigabe |

Die Projektgröße der Übersicht summiert nur gemessene Projekte und nennt die Zahl der ungemessenen; gemessen werden nur freigegebene Bereiche und Ordner, die eine frühere Version angelegt hat. Eine Prüfung heißt nie „sicher bereinigbar“.

Die HTML-Vorschau erlaubt schnelle Abstimmung. Slint ist die Produktionsoberfläche. Beide verwenden dieselbe JSON-Fixture, aber kein gemeinsames Web-Laufzeitsystem.

## 5. Meilensteine und Arbeitspakete

Jeder Meilenstein endet mit einer prüfbaren Freigabe. „Umgesetzt“ bedeutet: Code vorhanden und durch die genannten automatisierten Tests belegt. „Abnahme offen“ nennt, was zusätzlich durch Menschen oder weitere Plattformen nachzuweisen ist.

### M0 — Native Basis verifizieren

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-01 | Rust/Slint-Build aufsetzen und Integrationsfehler schließen | Beide EXEs bauen mit MSVC; alle Rust-Kerntests erfolgreich | Umgesetzt: `build.ps1 -NoRun` grün (Rust 1.98.0, MSVC) |
| F-02 | Windows 10/11 Tray testen | Klick, Rechtsklick, Fenster schließen/erneut öffnen, Beenden und Instanzschutz funktionieren | Umgesetzt auf Windows 10; Windows 11 Abnahme offen ([WINDOWS-ABNAHME](WINDOWS-ABNAHME.md)) |
| F-03 | Buildbindung herstellen | Geprüftes Cargo.lock eingecheckt; Toolchain, Backend und SQLite-Engine dokumentiert | Umgesetzt: Cargo.lock, `rust-toolchain.toml` 1.98.0, SQLite 3.53.2, winit + Software-Renderer |
| F-04 | Native Darstellung und Ressourcen messen | Screenshots der tatsächlichen App; Idle/Öffnen/Schließen-Messung; keine erfundenen Zielerreichungen | Umgesetzt: native Aufnahmen, [MESSBERICHT](MESSBERICHT.md) mit Messwerten dieses Rechners |

**Freigabe G0:** Native Basis lauffähig. Die Windows-CI ist eingerichtet; ihr erster Lauf erfolgt nach dem Push.

### M1 — Sichere passive Erkennung

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-05 | Lokal freigegebene Suchbereiche | Projektregistrierung und Lesefreigabe getrennt; keine automatische Ausweitung | Umgesetzt: `scopes`, `project approve`; Test `pilot_registration_scan_run_and_test_cleanup` |
| F-06 | Windows-Lesegrenzen und Ressourcenidentität | Junctions, Links, Reparse Points, ersetzte Pfade und nicht freigegebene Ziele sicher blockiert; keine reine Präfixprüfung | Umgesetzt: handle-relatives `NtCreateFile`, Identität (Volume + 128-Bit-ID); `tests/safe_reading.rs` |
| F-07 | Begrenzte Parser und Datenminimierung | Größen-/Tiefe-/Zeitlimits; kein Import/Start von Projektcode; Test-Secrets nicht in Logs/DB/Export | Umgesetzt: `fsread::Budget`, `parsers/`, `privacy`; Test `synthetic_secrets_do_not_reach_the_database_files` |
| F-08 | Baselines und Erfassungsgenerationen | Teilfehler führen nicht zu falschen Entfernungsereignissen; letzter vollständiger Stand bleibt verfügbar | Umgesetzt: `storage/generations.rs`; Tests zu Teil-/Fehlläufen und Katalogwechsel |

**Freigabe G1:** Negative Fixtures (Junction, ersetzte Wurzel, Übergröße, Limits, Geheimnisse) bestehen automatisiert.

### M2 — Erster sinnvoller Inventarpilot

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-09 | npm/pnpm-Basis und statische Python-Metadaten | Deklariert/aufgelöst/installiert getrennt; keine fremden Interpreter automatisch starten | Umgesetzt: `adapters/{npm,pnpm,pip}.rs`; venv über `pyvenv.cfg` + `*.dist-info` |
| F-10 | Generisches Skill-/Instruktionsprofil | SKILL.md/AGENTS.md und erlaubte Referenzen lesen; geschützte Ressourcen; kein Ausführen | Umgesetzt: `profiles/parse.rs`; Verweise nur innerhalb der Freigabe geprüft |
| F-11 | Besitzer und „Warum hier?“ | Explizite und vermutete Besitzer mit Evidenz unterscheiden; unbekannt darstellbar | Umgesetzt: `owner_links`, `why`-Befehl, Detaildialog |
| F-12 | Ereignisse und UI anbinden | Ein begrenzter Watcherpfad mit Debounce, Fehlerabgleich und sichtbarer Abdeckung | Entfernt am 20.10.2026: Erfasst wird nur auf Klick oder per `scan`; kein Watcher, keine Tray-Hinweise, keine Pause. Abdeckung bleibt je Lesefreigabe sichtbar |

**Freigabe G2:** Dieses Repository (Cargo, Skills) sowie Fixtures für JavaScript- und Python-Projekte werden nachvollziehbar dargestellt.

### M3 — Besitzerprozess und Scratchpads

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-13 | Befehle in Besitzerprozess bündeln | Abgesicherte Named Pipe, normaler Benutzer, Protokoll-/Größenlimits; CLI nicht mehr unabhängiger Writer | Umgesetzt: `ipc/`, Besitzerprüfung beider Seiten; Test `second_owner_and_foreign_protocols_are_refused` |
| F-14 | Scratchpad-Lebenszyklus | Quelle, temporäre Ausgabe und Ergebnis trennen; Übernehmen/Schützen/Prüfdatum/Promote | Registrieren/Übernehmen (auch gesammelt) sowie Kandidatensuche in freigegebenen Wurzeln umgesetzt; Anlegen per CLI am 20.10.2026 entfernt; Skripte/Notebooks, Testdaten und Ausgaben mit Belegen, ausdrücklich bestätigtem Besitzer/Zweck; keine automatische Zweckbestimmung |
| F-15 | Verwaltete Läufe | Expliziter Start, eigenes Temp, Prozessidentität und geprüfte Kindprozessbehandlung | Entfernt am 20.10.2026: mogumogu startet keine Programme mehr; `mogumogu-cli run`, Job-Objekte und Prozessidentitäten sind ausgebaut |
| F-16 | Sessionzustände und Wiederanlauf | Starterende/fehlender Heartbeat ist kein Abschlussbeweis; unklar bleibt geschützt | Entfernt am 20.10.2026 mit F-15; Schema v5 löscht die Sessiontabelle nach automatischer Sicherung. Test `v4_plans_of_the_old_deletion_path_never_run_and_sessions_are_gone` |

**Freigabe G3:** Mit F-15/F-16 entfallen; der Besitzerprozess (F-13) bleibt die einzige schreibende Instanz.

### M4 — Vereinbarte Breite, keine Scheingenauigkeit

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-17 | uv, Cargo, NuGet ergänzen | Lock-/Workspace-/Framework-/Storesemantik mit Versionen und negativen Fixtures | Projektformate, separate globale Paketbereiche und statische Kanten umgesetzt; reale Toolversionen und vollständige Resolversemantik offen |
| F-18 | WinGet, Scoop, Chocolatey ergänzen | Nur kontrollierte schreibgeschützte Erfassung; Verwaltungsbesitz nicht aus ähnlichen Namen ableiten | Entfernt am 20.10.2026: Installierte Programme gehören nicht zur Entwicklungsumgebung; bestehende Freigaben werden beim Start widerrufen, ihr Stand bleibt als Historie. Test `approvals_for_installed_program_lists_are_retired_once` |
| F-19 | KI-Profilkatalog erweitern | Alle zugesagten Clientfamilien statisch klassifiziert; Wirksamkeit und fehlende Overrides transparent | Zwölf projektbezogene Profile umgesetzt, Erweiterungstest ohne Kernumbau; persönliche KI-Konfiguration bekannter Clients über den Freigabetyp `user-ai`, nur entlang der Katalogmuster (Test `user_ai_folder_is_read_only_along_catalog_routes`); Vorrangauflösung Benutzer/Projekt offen |
| F-20 | Quellenbezogene Updatehinweise | Explizite Netzwerkfreigabe, private Registries wahren, Timeout/Backoff; keine Installation | Entfernt am 20.10.2026: mogumogu stellt keine Netzwerkverbindungen her; Abkündigungen nur aus lokalen Lockfiles; `validate.py` verbietet HTTP-Clients |

**Freigabe G4:** Alle elf Paketmanager innerhalb der dokumentierten Fixtures getestet. Reale Toolversionen auf Windows 10/11 sind noch abzunehmen; bis dahin bleibt der Status „experimentell“.

### M5 — Erklären und kontrolliert bereinigen

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-21 | Prüfregeln mit Evidenz | Inaktiv, abgelaufen, möglicherweise verwaist und unbekannt getrennt; Schutz gewinnt | Umgesetzt: `review.rs` mit Prüftermin und fehlendem Besitzer. Die Regel „Inaktiv“ entfiel am 21.10.2026 mit den Sessions, die Aktivität belegt hatten; ohne Beobachtung wäre sie reines Alter |
| F-22 | Planer und Entbehrlichkeitsvertrag | Konkrete Ziele, Besitzer, Inhalt, Unsicherheit, Erhalt und Rekonstruktionsstatus sichtbar | Umgesetzt: `cleanup/planner.rs` aus einem frischen Aufräumbericht; nur „Wiederherstellbar“ mit Beleg (Datei daneben), Blocker statt stiller Auslassung; Test `folders_to_review_cannot_be_planned` |
| F-23 | Sicherer Ausführer und Aktionsjournal | Erneute Handle-/Identitätsprüfung, aktive Sperren, vorab dauerhaft gespeicherte Aktion, Teilfehlerabgleich | Umgesetzt: `cleanup/executor.rs`; Ordner als Ganzes in den Papierkorb, Wurzel und Elternordner ohne Löschfreigabe gehalten (Test `a_pinned_folder_holds_the_way_but_lets_its_children_move`), Absturzmatrix in `tests/core/windows_cleanup.rs` |
| F-24 | Erstes kontrolliertes Ende-zu-Ende-Cleanup | Nur wiederherstellbar belegte Ordner in wegwerfbarem Testbestand; Quellcode/Lockfiles/Testdaten unverändert | Umgesetzt und getestet mit Test-Papierkorb (`only_recoverable_folders_move_and_everything_else_stays`); über die CLI nur bis vor das Verschieben (`tests/end_to_end.rs`) |

**Freigabe G5:** Unabhängiges Review des Papierkorb-Wegs — **offen**. Bis dahin ist `cleanup::PRODUCTION_ENABLED = false`; es gibt keinen Schalter zum Umgehen.

### M6 — Vollständige erste Beta

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-25 | Performance und Skalierung | Dokumentierte Referenzfixture, begrenzte Queues/Verläufe; kein fortlaufendes Speicherwachstum | Umgesetzt: `tests/performance.rs` (10 000 Pakete), `scripts/measure.ps1`, Ergebnisse im Messbericht |
| F-26 | Native Bedienung härten | Tastatur, Fokus, Screenreader, DPI, Explorer-Neustart und Schlafmodus | Umgesetzt: Tastaturnavigation, Fokusfalle in Dialogen, Accessibility-Rollen, Tray-Neuanmeldung; DPI/Screenreader/Schlaf manuell offen |
| F-27 | Migration/Backup/Veröffentlichung | Konsistente Sicherung, Upgradefehler, source-passender Build, GPL-Abhängigkeiten und sichere CI | Umgesetzt: Backup-API, Migrationstests (v3→v4: `v3_scopes_keep_their_meaning_and_user_folders_get_their_own_realm`), Sicherung und Export mit Vorschau im Dashboard, `scripts/release.ps1`, CI mit minimalen Rechten |
| F-28 | Unterstützungsmatrix und Anleitung | Inventar/Update/Plan/Ausführung je Version; bekannte Grenzen; Windows-10/11-Nachweise | Umgesetzt: [UNTERSTUETZUNGSMATRIX](UNTERSTUETZUNGSMATRIX.md), [ANLEITUNG](ANLEITUNG.md); Windows-11-Nachweis offen |

**Freigabe G6:** Offen, bis G5, der zweite Review der erweiterten Lesegrenze (Systembefund, `user-ai`-Freigaben), die Windows-11-Abnahme und ein Messbericht auf zweiter Hardware vorliegen.

## 6. Datenmodell

Schema v3 ergänzt ausgewählte statische Paketkanten, Kontext und Dateibeleg-Zähler in Inventareinträgen, separate Lesebereichsprofile, lokale Behalten-Vormerkungen sowie einen nach Ökosystem/Name/Version/Quelle/Host getrennten Quellen-Cache. Der alte Latest-only-Cache wird nach konsistenter Sicherung verworfen und neu abgefragt. Ressourcen, Aktionsjournal und Bereinigungsfreigaben bleiben unabhängig. Schema v5 entfernt Sessions sowie Manifest und Journal des alten Löschwegs; offene alte Pläne werden ungültig, neue Pläne speichern je Ordner Identität, Beleg und Journalzustand (`recycle_targets`). Der Referenzgraph ist eine begrenzte Auswertung der veröffentlichten Inventargenerationen, kein neuer Dienst und keine Graphdatenbank.

Pfade sind Orte, nicht dauerhaft sichere Identitäten. Besitzer, Nutzung, Verfügbarkeit, Schutz und Bereinigungsfähigkeit bleiben unabhängige Felder. Ein neu angelegtes Verzeichnis an einem alten Ort erbt niemals eine frühere Freigabe (Test `replaced_root_does_not_inherit_the_approval`, `a_replaced_folder_or_a_junction_on_the_way_invalidates_the_plan`).

Grenzen: 2.000 Projekte, 5.000 registrierte Ressourcen, 200 Aktivitätszeilen, drei Generationen und 2.000 Ereignisse je Bereich, UI-Seiten zu 200 Einträgen.

## 7. Sicherheits- und Datenschutzabnahme

| Szenario | Muss gelten | Nachweis |
|---|---|---|
| Tempname, Testdaten oder altes Änderungsdatum | Keine automatische Entbehrlichkeit | `folders_to_review_cannot_be_planned` |
| Beleg fehlt nach Planfreigabe (Lockfile gelöscht) | Erneute Prüfung/Blockierung | `a_missing_lockfile_after_approval_invalidates_the_plan` |
| `.mogumogu-keep` im Ordner oder darüber | Ordner verweigern | `a_keep_marker_on_the_way_or_inside_blocks_the_folder` |
| Ordner ersetzt oder Junction auf dem Weg | Plan ungültig, nichts verschoben | `a_replaced_folder_or_a_junction_on_the_way_invalidates_the_plan` |
| Kein Papierkorb oder Ordner bleibt am Ort | Als nicht verschoben melden | `a_failing_or_idle_recycle_bin_is_reported_and_leaves_the_folder` |
| Projekt über eine Junction in der Testwurzel | G5-Sperre bleibt (Weg von der Testwurzel wird ohne Links gegangen) | `a_project_reached_through_a_junction_in_the_test_root_stays_locked` |
| Kontextfilter: Hardlink am Ziel, schreibgeschütztes Ziel | Nichts außerhalb geschrieben, keine Hilfsdatei bleibt zurück | `a_context_filter_never_writes_through_a_hard_link_and_leaves_nothing_behind` |
| Kontextfilter: `!`-Ausnahme würde eigene Verbote aufheben | Keine Negationsregel | `a_context_filter_goes_into_the_personal_file_and_keeps_the_shared_one` |
| Laufwerkssuche: fremde Profile, Projektmarker an Laufwerks- oder Profilwurzel | Profile außer dem eigenen und AppData/Punktordner bleiben außen vor | `drive_exclusions_depend_on_the_place` |
| Ordner `foo.` neben einer Junction `foo` (Windows liest beide gleich) | Nie verschoben; Shell bekommt den echten Ort des gehaltenen Elternordners | `a_name_the_shell_reads_differently_is_never_moved` |
| `.git`, `.mogumogu-keep` oder Verknüpfung irgendwo im Ordner | Ordner bleibt | `version_control_keep_markers_or_links_anywhere_inside_keep_the_folder` |
| Geschützte Ressource im Ordner | Ordner bleibt | `a_protected_resource_inside_keeps_the_folder` |
| `coverage/` als Quellpaket, `build/` neben `package.json`, `.cache` | Nur „Prüfen“, nie planbar | `names_alone_do_not_make_hand_made_folders_recoverable` |
| Nicht erreichbares Laufwerk/Scanfehler | Unverfügbar/partiell, keine fiktive Entfernung | `partial_and_failed_passes_never_report_removals` |
| KI-Datei mit Löschanweisungen | Nur Daten, keine Verhaltensänderung | `skill_references_inside_and_outside` |
| MCP-Startreferenz auf npx/uvx | Referenz, kein Installations- oder Nutzungsnachweis; kein Probestart | `mcp_entries_keep_only_safe_metadata` |
| Private Registry/Token/Fehlerausgabe | Keine Offenlegung/öffentliche Namensabfrage | `private_and_unknown_sources_are_never_hosts`, Redaktionstests |
| Datenbank-/Dateiaktion unterbrochen | Journal und Zustand abgleichen; kein blindes Wiederholen | `crash_after_the_journal_commit_is_reconciled_not_repeated` |
| Entfernen der App | Keine registrierten Projektdateien löschen | Kein Deinstallationsschritt berührt Projektordner; App-Daten liegen getrennt unter `%LOCALAPPDATA%\mogumogu` |

## 8. Entwicklungsbudgets

**Ziele:** Tray nach Beruhigung unter 50 MiB Private Bytes; Idle-CPU unter 0,5 % eines logischen Kerns; gespeicherte Übersicht am 95. Perzentil innerhalb von 500 ms; Fensterzyklen ohne fortlaufendes Wachstum. Die gemessenen Werte dieses Rechners stehen im [Messbericht](MESSBERICHT.md); sie gelten nicht als allgemeine Zusage.

Messen mit Release-Build, normalem Virenschutz und dokumentierter Hardware. Bei Zielverfehlung erst Messdaten analysieren; niemals Sicherheitsprüfungen entfernen oder die Haltbarkeit des Aktionsjournals abschwächen.

## 9. Zusammenarbeit und KI-Entwicklung

`AGENTS.md` enthält die gemeinsame Baseline; sechs Skills unter `.agents/skills/` (Codex) werden deterministisch nach `.claude/skills/` (Claude Code) kopiert (`scripts/sync_skills.py`); `scripts/validate.py` meldet Abweichungen. Alle ausführenden Integrationen bleiben opt-in. Globale Einstellungen werden nicht verändert.

Projektvorschläge dürfen in Git geteilt werden, lokale Datenbank, absolute Pfade, Geheimnisse und Freigaben nicht. CI prüft Fremdbeiträge ohne Release-Secrets und ohne produktive Projektverzeichnisse.

## 10. Nächste Schritte

1. Zweiter Review der erweiterten Lesegrenze: Metadatenprüfung des Systembefunds (`src/inventory/system.rs`, `platform::local_fixed_path`) und routengeführte Erfassung persönlicher KI-Ordner (`scan_user_ai`, `Catalog::user_routes`, Migration 004).
2. Systemerkennung ergänzen, wo bisher nur ein Ortsnachweis besteht: uv-Python, rustup, .NET-Tools, pyenv; wirksame Datenorte und Konfigurations-Overrides; Git-Konfiguration. Die Scratchpad-/Entwicklungsfundsuche mit unbekanntem Zweck ist implementiert, vollständige Worktree-/Editor-Erkennung bleibt offen. Stand und Grenzen: [Systemerkennung](SYSTEMERKENNUNG.md), [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md). Vollständige Resolver-, Feature-, Peer- und Frameworkanalyse ist noch offen.
3. Windows-11-Abnahme nach [WINDOWS-ABNAHME](WINDOWS-ABNAHME.md) und Messbericht auf zweiter Hardware.
4. Unabhängiger Review des Papierkorb-Wegs (`src/cleanup/`, `inventory::tidy::recoverable`, `platform::recycle`, `DirHandle::open_dir_pinned`) als Voraussetzung für G5; erst danach `PRODUCTION_ENABLED` freischalten.
5. Adapter gegen reale Toolversionen abnehmen und den Status in der Unterstützungsmatrix von „experimentell“ auf „getestet“ heben.

Der ausführliche [PROJEKTPLAN](PROJEKTPLAN.md) bleibt das Produktziel; dieses Dokument hält die überprüfbare Umsetzungsreihenfolge und ihren Stand fest.
