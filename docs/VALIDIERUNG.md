# Validierung · mogumogu 0.1.0

Stand: 5. Oktober 2026. Alle Prüfungen liefen auf Windows 10 Pro 22H2 (Build 19045, x64) mit Rust 1.98.0 (MSVC). Was nicht ausgeführt wurde, steht ausdrücklich unten.

## Ausgeführt und bestanden

| Bereich | Ergebnis | Befehl / Beleg |
|---|---|---|
| Nativer Build | Beide EXEs, Release, `--locked` | `.\build.ps1 -NoRun` |
| Formatierung | ohne Abweichung | `cargo fmt --all -- --check` |
| Lint | 0 Warnungen, inkl. `undocumented_unsafe_blocks` | `cargo clippy --locked --all-targets -- -D warnings` |
| Unit-Tests | 71 bestanden | Domäne, Parser, Adapter, Katalog, Redaktion, Protokoll, Donut-Geometrie, CLI-Parser |
| Kernverträge | 22 bestanden | `tests/core.rs`, `tests/core/windows_cleanup.rs`: Migration v1→v2 mit Sicherung, neueres Schema abgewiesen, Generationen, Geheimnisse weder in DB noch WAL, Snapshot/CLI-Review identisch, Absturzmatrix der Bereinigung |
| Windows-Lesegrenze | 8 bestanden | `tests/safe_reading.rs`: echte Junctions, ersetzte Wurzel, Übergröße, Limits, venv/node_modules ohne Ausführung |
| Plattform | 3 bestanden | `tests/platform.rs`: benannte Job-Objekte, PID-Wiederverwendung über Startzeit, Benutzer-SID |
| Ende-zu-Ende | 6 bestanden | `tests/end_to_end.rs`: Besitzer im Testprozess + echte CLI über die Pipe, verwaltete Läufe, Bereinigung in Testwurzel, Negativfälle |
| Referenzlast | bestanden | `tests/performance.rs` (10 000 Pakete), siehe [Messbericht](MESSBERICHT.md) |
| Statische Verträge | bestanden | `python scripts/validate.py` (Schema, Katalog, Skill-Kopien, Allowlists für Löschen/Prozesse/`unsafe`) |
| Headless-Smoke-Test | bestanden | Besitzer `--headless`, Registrierung per CLI, `owner stop`; registrierter Pfad wurde nicht angelegt |
| Native Oberfläche | Screenshots aller acht Bereiche in Demo- und Lokalmodus | [`docs/screenshots/`](screenshots/) – Aufnahmen der echten App, nicht der HTML-Vorschau |
| Fensterlebenszyklus | bestanden | Fensterkreuz (WM_CLOSE) schließt nur das Fenster; Zweitstart holt das bestehende Fenster nach vorn und endet mit Code 0; Detaildialog per Mausklick, Esc schließt; `owner stop` beendet ohne Kindprozesse |
| Tastatur | bestanden | Alt+1…8 wechselt Bereiche (synthetische Eingaben), Fokusrahmen sichtbar |
| Lokaler Durchlauf | bestanden | Dieses Repository registriert, freigegeben, erfasst (542 Cargo-Pakete, 14 KI-Artefakte), Scratchpad angelegt, verwalteter Lauf `completed_verified` |
| Ressourcenmessung | bestanden | `scripts/measure.ps1`, [Messbericht](MESSBERICHT.md) |

Während der Validierung gefundene und behobene Fehler (Auswahl): Marker `requirements*.txt` erkannte `requirements.txt` nicht; Inventarschlüssel enthielt redaktionspflichtigen Rohtext; frisch gestartete Sessions wurden vor dem Andocken als „unklar“ markiert; Snapshot-Abfrage skalierte mit Projekte × Inventar; Pixel-Schrifteinbettung ignorierte Schriftgewichte; fehlender Initialfokus verhinderte Tastaturkürzel; Ressourcenliste und Einstellungsseite hatten Layoutfehler.

## Ergänzungen: Container und UI-Modulaufbau

Am 5. Oktober 2026 zusätzlich geprüft: `scripts/check.py --offline` auf Windows,
`build.ps1 -NoRun`, Docker-Ziel `validated` und
`docker compose run --build --rm checks`. Compose läuft mit UID/GID 10001,
schreibgeschütztem Image und zwei Cache-Volumes. Ein weiterer Compose-Lauf
mit `--core --offline` bestand ohne Dependency-Downloads. Die Docker-Prüfung
enthält 16 portable Kernverträge; die sechs Windows-Session-/Cleanup-Verträge
bleiben im nativen Lauf aktiv. Beide Featurevarianten (mit/ohne Netzwerk) bestanden.

Die native Referenzlast und 100 Fensterzyklen wurden wiederholt, siehe
[Messbericht](MESSBERICHT.md). Die neuen Tastaturflächen, Suche, Formularfehler
und Kontrastwerte sind implementiert und nativ kompiliert; visuelle Abnahme
und Screenreader-Prüfung dieser Änderungen bleiben offen
([Zusatzprüfungen](WINDOWS-ABNAHME.md#e-zusatzprüfungen-nach-dem-modulumbau)).
Vorhandene Screenshots dokumentieren den früheren UI-Stand.

## Nicht ausgeführt

| Bereich | Grund |
|---|---|
| Windows 11 | Kein Windows-11-System verfügbar |
| Tray-Klick und Tray-Menü per Maus | Nicht automatisiert; Menüaktionen sind über dieselben Anwendungsfälle per CLI getestet |
| DPI 125–200 %, Mehrmonitor, Explorer-Neustart, Schlafmodus | Manuelle Abnahme ausstehend; Slint meldet das Tray-Symbol nach `TaskbarCreated` selbst neu an |
| Screenreader (Narrator) | Accessibility-Rollen und -Beschriftungen gesetzt, nicht mit Narrator abgenommen |
| Reale Toolversionen der Paketmanager | Fixtures statt installierter npm/pnpm/uv/NuGet/Scoop/Chocolatey-Bestände (außer Cargo dieses Repos) |
| Updatehinweise gegen echte Registries | Netzwerkpfad ist implementiert, aber standardmäßig aus; in Tests ohne Netz geprüft |
| GitHub-Windows-CI | Workflow eingerichtet, erster Lauf nach dem Push |
| Unabhängiger Review der Löschpfade (G5) | Erfordert eine zweite Person |
