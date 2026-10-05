# mogumogu — Implementierungsplan v0.1

**Ziel:** Lokale, performante Windows-10/11-Tray-Anwendung für Entwicklungsressourcen.
**Stack:** Rust, Slint und SQLite. **Lizenz:** GPL-3.0-only.
**Stand:** 4. Oktober 2026. Alle Arbeitspakete F-01 bis F-28 sind im Code umgesetzt und automatisiert getestet. Offen sind ausschließlich Abnahmen, die eine zweite Person oder eine weitere Plattform erfordern (Windows 11, unabhängiger Löschpfad-Review G5, manuelle Desktopprüfungen). Die Spalte „Stand“ trennt beides ausdrücklich.

## 1. Produkt und Grenze

mogumogu macht sichtbar, welche Entwicklungsressourcen existieren, welchem Projekt oder Experiment sie gehören, welche Nutzung tatsächlich beobachtet wurde und welche Ressourcen eine Prüfung verdienen. Unterstützte Ressourcen können später durch einen bestätigten Plan entfernt werden.

**Keine beobachtete Nutzung ist kein Entbehrlichkeitsnachweis.** Quellcode, Einstellungen, Skills, einzigartige Ergebnisse und unbekannte Inhalte bleiben geschützt. Erkennung, Updateabfrage und Entfernung sind getrennte Fähigkeiten.

Die Zielmatrix bleibt npm, pnpm, pip, uv, Cargo, NuGet, WinGet, Scoop und Chocolatey. Dazu kommen generische Scratchpads und Temp-/Buildausgaben sowie versionierte KI-Profile für Agent Skills, Codex, Claude Code, Copilot/VS Code, Cursor, Gemini, Windsurf/Cascade, Cline, Roo Code, OpenCode, Continue und Aider. „Unterstützt“ gilt nur innerhalb einer veröffentlichten Versions-/Fähigkeitsmatrix: [UNTERSTUETZUNGSMATRIX.md](UNTERSTUETZUNGSMATRIX.md).

## 2. Bewusst kleine Startarchitektur

Ein Cargo-Paket mit klar getrennten Modulen, ein Besitzerprozess mit genau einem Datenbank-Worker, eine Tray-Ereignisschleife. Module werden erst zu eigenen Crates, wenn eine echte unabhängige Schnittstelle oder Wiederverwendung existiert. Keine Plugin-DLLs, Microservices, WebView, Graphdatenbank, Async-Runtime oder eigene Regelsprache.

| Entscheidung | Baseline und Grund |
|---|---|
| Rust + Slint | Native UI ohne Browserfrontend; Software-Renderer, Bilder eingebettet, Systemschriften |
| SQLite lokal | Kein Server; WAL/FULL, Fremdschlüssel, parametrisierte SQL-Abfragen, kurze Transaktionen, Migration mit Sicherung |
| Ein Besitzerfenster bei Bedarf | Dashboard schließen gibt die Komponente frei; Tray und Kern laufen weiter |
| Ein Besitzerprozess, eine Schreibstelle | Desktop oder `--headless`; CLI spricht über eine benutzergebundene Named Pipe |
| Statische Erkennung als Standard | Handle-relatives Lesen freigegebener Ordner; keine Programmstarts beim Erfassen |
| Eigene Demo-Datenbank | Darstellung testen, ohne Benutzerdaten mit erfundenen Messwerten zu vermischen |
| Eingebaute Paketadapter | Pro Fähigkeit getrennt (Inventar/Updates/Bereinigung), keine Fremdcodeausführung |
| Deklarativer KI-Katalog | `profiles/ai-catalog.toml`, beim Build eingebettet und validiert; keine Skript- oder Netzwerkfelder |
| Bereinigung nur als Testexecutor | Verwaltete Temp-Ausgabe in registrierten Wegwerf-Testwurzeln; produktiv gesperrt bis G5 |
| Manueller Build und Start | Kein Autostart, Dienst oder Self-Updater |

## 3. Umgesetzte Basis

| Bereich | Quelle | Stand |
|---|---|---|
| Tray, Fensterlebenszyklus, Dashboard | `src/main.rs`, `src/desktop/`, `ui/` | Acht Ansichten nach Design, Dialoge, Tastaturnavigation; nativ gebaut und auf Windows 10 geprüft |
| Besitzerprozess und IPC | `src/owner.rs`, `src/ipc/`, `src/platform/windows.rs` | Ein Worker, Named Pipe (nur aktueller Benutzer, nur lokal, erste Instanz), Protokollversion, 1-MiB-Limit |
| Kern und Anwendungsfälle | `src/service/`, `src/domain/` | Typisierte Zustände und Zustandsautomaten; gemeinsame Anwendungsfälle für UI und CLI |
| SQLite | `src/storage/`, `migrations/` | Schema v2, Migration mit konsistenter Sicherung, Generationen, Aktionsjournal, Wiederanlauf |
| Sichere Lesegrenze | `src/fsread.rs`, `src/platform/` | Identitätsgebundene Freigaben, `NtCreateFile` relativ zum Elternhandle, keine Reparse Points/Platzhalter |
| Inventar | `src/inventory/`, `src/adapters/`, `src/parsers/`, `src/profiles/` | Neun statische Adapter, zwölf KI-Profile, begrenzte Parser, Datenminimierung |
| Sessions und Scratchpads | `src/sessions/` | Verwaltete Struktur, Läufe in benannten Job-Objekten, Zustandsabgleich |
| Prüfregeln und Bereinigung | `src/review.rs`, `src/cleanup/` | Hinweise mit Evidenz, Planbindung per Fingerabdruck, Journal, Abgleich nach Absturz |
| Updatehinweise | `src/updates/` | Opt-in pro öffentlicher Quelle, private Quellen nie, Backoff und Cache |
| CLI | `src/bin/cli/` | Client des Besitzerprozesses, startet ihn bei Bedarf, versioniertes JSON, Exitcodes |
| Qualität | `tests/`, `scripts/validate.py`, `.github/workflows/windows.yml` | 109 automatisierte Rust-Tests + Lastreferenz, statische Verträge, Clippy ohne Warnungen |
| Build/Start/Release/Messung | `build.ps1`, `start.ps1`, `scripts/` | Release-Build, ZIP mit Prüfsummen und Abhängigkeitsinventar, Messskript |
| Designvorschau | `design/dashboard.html` | Unabhängiger Browserprototyp; kein Ersatz für native Tests |

## 4. Dashboardgestaltung

Das ausgewählte Dashboard ist die visuelle Basis: statischer Berg-/Waldkopf, helles Blau/Grau, schmale linke Navigation und großzügige weiße Karten. Kein animierter Hintergrund, keine permanenten Diagrammanimationen und keine externe Bildabfrage.

| Bereich | Verhalten |
|---|---|
| Übersicht | Projektanzahl, Paketanzahl, erfasster Speicher, KI-Konfiguration; immer Demo-/Lokalstatus zeigen |
| Projekte | Tabelle, Suche, Zustandsfilter und Detailansicht mit Herkunft, Schutz, Lesefreigabe und Abdeckung |
| Paketmanager | Unterstützungsgrad und konkrete Fähigkeiten statt irreführendem „aktiv“-Schalter |
| KI-Konfiguration | Quellen, Scope, Client und Referenzen; niemals Skill/MCP durch Betrachtung starten |
| Ressourcen | Besitzer, Typ, bekannte oder unbekannte Größe, Scratchpads und Sessions |
| Prüfliste | Kandidaten begründen, Plan-Vorschau und Bestätigung per Fingerabdruck; kein „Alles löschen“ |
| Aktivität | Erfassungsabdeckung je Lesefreigabe, Ereignisse und gruppierte Änderungen |
| Einstellungen | Datenorte, Überwachung, Tray-Hinweise, Netzwerkquellen, Scratchpad-Wurzel |

Im Demomodus ist die Speicherverteilung ein festes, korrekt summiertes Beispieldiagramm. Im echten Inventar bleibt sie ohne Messung unbekannt; gemessen werden nur von mogumogu verwaltete Ordner und freigegebene Bereiche. Eine Prüfung heißt nie „sicher bereinigbar“.

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
| F-12 | Ereignisse und UI anbinden | Ein begrenzter Watcherpfad mit Debounce, Fehlerabgleich und sichtbarer Abdeckung | Umgesetzt: `watcher.rs` (Dirty-Set, 2 s Ruhe, 30 s Mindestabstand), Abdeckung in „Aktivität“ |

**Freigabe G2:** Dieses Repository (Cargo, Skills) sowie Fixtures für JavaScript- und Python-Projekte werden nachvollziehbar dargestellt.

### M3 — Generische Sessions und Scratchpads

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-13 | Befehle in Besitzerprozess bündeln | Abgesicherte Named Pipe, normaler Benutzer, Protokoll-/Größenlimits; CLI nicht mehr unabhängiger Writer | Umgesetzt: `ipc/`, Besitzerprüfung beider Seiten; Test `second_owner_and_foreign_protocols_are_refused` |
| F-14 | Scratchpad-Lebenszyklus | Quelle, temporäre Ausgabe und Ergebnis trennen; Übernehmen/Schützen/Prüfdatum/Promote | Umgesetzt: `scratch create/adopt/protect/extend/promote` |
| F-15 | Verwaltete Läufe | Expliziter Start, eigenes Temp, Prozessidentität und geprüfte Kindprozessbehandlung | Umgesetzt: benanntes Job-Objekt ohne KILL_ON_JOB_CLOSE, PID + Startzeit |
| F-16 | Sessionzustände und Wiederanlauf | Starterende/fehlender Heartbeat ist kein Abschlussbeweis; unklar bleibt geschützt | Umgesetzt: Entscheidungstabelle; Test `starter_ends_but_worker_continues_keeps_the_lock` |

**Freigabe G3:** Beliebige Programme laufen als verwaltete Session; Diagnose in `results/` bleibt, Starterabbruch ergibt „unklar“.

### M4 — Vereinbarte Breite, keine Scheingenauigkeit

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-17 | uv, Cargo, NuGet ergänzen | Lock-/Workspace-/Framework-/Storesemantik mit Versionen und negativen Fixtures | Umgesetzt mit Fixtures; Abgleich gegen reale Toolversionen offen |
| F-18 | WinGet, Scoop, Chocolatey ergänzen | Nur kontrollierte schreibgeschützte Erfassung; Verwaltungsbesitz nicht aus ähnlichen Namen ableiten | Umgesetzt: Registry-Softwareliste (WinGet „eingeschränkt“), Scoop/Chocolatey statisch |
| F-19 | KI-Profilkatalog erweitern | Alle zugesagten Clientfamilien statisch klassifiziert; Wirksamkeit und fehlende Overrides transparent | Umgesetzt: zwölf Profile im deklarativen Katalog; Erweiterungstest ohne Kernumbau |
| F-20 | Quellenbezogene Updatehinweise | Explizite Netzwerkfreigabe, private Registries wahren, Timeout/Backoff; keine Installation | Umgesetzt: `updates/`; Netzwerk nur nach Freigabe je Host |

**Freigabe G4:** Alle neun Paketmanager innerhalb der dokumentierten Fixtures getestet. Reale Toolversionen auf Windows 10/11 sind noch abzunehmen; bis dahin bleibt der Status „experimentell“.

### M5 — Erklären und kontrolliert bereinigen

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-21 | Prüfregeln mit Evidenz | Inaktiv, abgelaufen, möglicherweise verwaist und unbekannt getrennt; Schutz gewinnt | Umgesetzt: `review.rs`; Inaktivität nur mit ausreichendem Beobachtungszeitraum |
| F-22 | Planer und Entbehrlichkeitsvertrag | Konkrete Ziele, Besitzer, Inhalt, Unsicherheit, Erhalt und Rekonstruktionsstatus sichtbar | Umgesetzt: `cleanup/planner.rs`, Blocker statt stiller Auslassung |
| F-23 | Sicherer Ausführer und Aktionsjournal | Erneute Handle-/Identitätsprüfung, aktive Sperren, vorab dauerhaft gespeicherte Aktion, Teilfehlerabgleich | Umgesetzt: `cleanup/executor.rs`; Absturzmatrix in `tests/core.rs` |
| F-24 | Erstes kontrolliertes Ende-zu-Ende-Cleanup | Nur verwaltete ausdrücklich entbehrliche Testausgabe in wegwerfbarem Testbestand; Quellcode/Ergebnisse unverändert | Umgesetzt und getestet (`tests/end_to_end.rs`) |

**Freigabe G5:** Unabhängiges Review der kritischen Löschpfade — **offen**. Bis dahin ist `cleanup::PRODUCTION_ENABLED = false`; es gibt keinen Schalter zum Umgehen.

### M6 — Vollständige erste Beta

| ID | Aufgabe | Fertigkriterium | Stand |
|---|---|---|---|
| F-25 | Performance und Skalierung | Dokumentierte Referenzfixture, begrenzte Queues/Verläufe; kein fortlaufendes Speicherwachstum | Umgesetzt: `tests/performance.rs` (10 000 Pakete), `scripts/measure.ps1`, Ergebnisse im Messbericht |
| F-26 | Native Bedienung härten | Tastatur, Fokus, Screenreader, DPI, Explorer-Neustart und Schlafmodus | Umgesetzt: Tastaturnavigation, Fokusfalle in Dialogen, Accessibility-Rollen, Tray-Neuanmeldung; DPI/Screenreader/Schlaf manuell offen |
| F-27 | Migration/Backup/Veröffentlichung | Konsistente Sicherung, Upgradefehler, source-passender Build, GPL-Abhängigkeiten und sichere CI | Umgesetzt: Backup-API, Migrationstests, `scripts/release.ps1`, CI mit minimalen Rechten |
| F-28 | Unterstützungsmatrix und Anleitung | Inventar/Update/Plan/Ausführung je Version; bekannte Grenzen; Windows-10/11-Nachweise | Umgesetzt: [UNTERSTUETZUNGSMATRIX](UNTERSTUETZUNGSMATRIX.md), [ANLEITUNG](ANLEITUNG.md); Windows-11-Nachweis offen |

**Freigabe G6:** Offen, bis G5, die Windows-11-Abnahme und ein Messbericht auf zweiter Hardware vorliegen.

## 6. Datenmodell

Schema v2 speichert Projekte, Ressourcen (mit Herkunft, Elternbezug, Prüftermin, Entbehrlichkeit, Identität), Lesefreigaben, Erfassungsgenerationen mit Inventareinträgen, gruppierte Ereignisse, Besitzerkanten, Sessions, Wegwerf-Testwurzeln, Pläne mit Identitätsmanifest, das Aktionsjournal, Einstellungen und Updatehinweise. Paketgraphen und Kanten zwischen Paketen werden erst mit ihrem Anwendungsfall ergänzt.

Pfade sind Orte, nicht dauerhaft sichere Identitäten. Besitzer, Nutzung, Verfügbarkeit, Schutz und Bereinigungsfähigkeit bleiben unabhängige Felder. Ein neu angelegtes Verzeichnis an einem alten Ort erbt niemals eine frühere Freigabe (Test `replaced_root_does_not_inherit_the_approval`, `replaced_temp_folder_does_not_inherit_the_decision`).

Grenzen: 2.000 Projekte, 5.000 registrierte Ressourcen, 200 Aktivitätszeilen, drei Generationen und 2.000 Ereignisse je Bereich, UI-Seiten zu 200 Einträgen.

## 7. Sicherheits- und Datenschutzabnahme

| Szenario | Muss gelten | Nachweis |
|---|---|---|
| Tempname oder altes Änderungsdatum | Keine automatische Entbehrlichkeit | `only_explicit_completed_managed_temp_output_qualifies` |
| Neues Kind nach Planfreigabe | Erneute Prüfung/Blockierung | `new_file_after_approval_invalidates_the_plan` |
| Geschütztes Kind in entbehrlichem Elternordner | Gesamte Elternoperation verweigern | `protected_child_and_links_block_the_whole_target` |
| Unklarer/noch laufender Kindprozess | Bereinigung gesperrt | `starter_ends_but_worker_continues_keeps_the_lock` |
| Nicht erreichbares Laufwerk/Scanfehler | Unverfügbar/partiell, keine fiktive Entfernung | `partial_and_failed_passes_never_report_removals` |
| KI-Datei mit Löschanweisungen | Nur Daten, keine Verhaltensänderung | `skill_references_inside_and_outside` |
| MCP-Startreferenz auf npx/uvx | Referenz, kein Installations- oder Nutzungsnachweis; kein Probestart | `mcp_entries_keep_only_safe_metadata` |
| Private Registry/Token/Fehlerausgabe | Keine Offenlegung/öffentliche Namensabfrage | `private_and_unknown_sources_are_never_hosts`, Redaktionstests |
| Datenbank-/Dateiaktion unterbrochen | Journal und Zustand abgleichen; kein blindes Wiederholen | `crash_after_journal_commit_is_reconciled_not_repeated` |
| Entfernen der App | Keine registrierten Projektdateien löschen | Kein Deinstallationsschritt berührt Projektordner; App-Daten liegen getrennt unter `%LOCALAPPDATA%\mogumogu` |

## 8. Entwicklungsbudgets

**Ziele:** Tray nach Beruhigung unter 50 MiB Private Bytes; Idle-CPU unter 0,5 % eines logischen Kerns; gespeicherte Übersicht am 95. Perzentil innerhalb von 500 ms; Fensterzyklen ohne fortlaufendes Wachstum. Die gemessenen Werte dieses Rechners stehen im [Messbericht](MESSBERICHT.md); sie gelten nicht als allgemeine Zusage.

Messen mit Release-Build, normalem Virenschutz und dokumentierter Hardware. Bei Zielverfehlung erst Messdaten analysieren; niemals Sicherheitsprüfungen entfernen oder die Haltbarkeit des Aktionsjournals abschwächen.

## 9. Zusammenarbeit und KI-Entwicklung

`AGENTS.md` enthält die gemeinsame Baseline; sechs Skills unter `.agents/skills/` (Codex) werden deterministisch nach `.claude/skills/` (Claude Code) kopiert (`scripts/sync_skills.py`); `scripts/validate.py` meldet Abweichungen. Alle ausführenden Integrationen bleiben opt-in. Globale Einstellungen werden nicht verändert.

Projektvorschläge dürfen in Git geteilt werden, lokale Datenbank, absolute Pfade, Geheimnisse und Freigaben nicht. CI prüft Fremdbeiträge ohne Release-Secrets und ohne produktive Projektverzeichnisse.

## 10. Nächste Schritte

1. Windows-11-Abnahme nach [WINDOWS-ABNAHME](WINDOWS-ABNAHME.md) und Messbericht auf zweiter Hardware.
2. Unabhängiger Review der Löschpfade (`src/cleanup/`, `src/platform/windows.rs`) als Voraussetzung für G5; erst danach eine produktive Ressourcenart freischalten.
3. Adapter gegen reale Toolversionen abnehmen und den Status in der Unterstützungsmatrix von „experimentell“ auf „getestet“ heben.

Der ausführliche [PROJEKTPLAN](PROJEKTPLAN.md) bleibt das Produktziel; dieses Dokument hält die überprüfbare Umsetzungsreihenfolge und ihren Stand fest.
