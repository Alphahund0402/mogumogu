# Validierung · mogumogu 0.1.0

Stand: 5. Oktober 2026. Alle Prüfungen liefen auf Windows 10 Pro 22H2 (Build 19045, x64) mit Rust 1.98.0 (MSVC). Was nicht ausgeführt wurde, steht ausdrücklich unten.

## Ausgeführt und bestanden

| Bereich | Ergebnis | Befehl / Beleg |
|---|---|---|
| Nativer Build | Beide EXEs, Release, `--locked` | `.\build.ps1 -NoRun` |
| Formatierung | ohne Abweichung | `cargo fmt --all -- --check` |
| Lint | 0 Warnungen, inkl. `undocumented_unsafe_blocks` | `cargo clippy --locked --all-targets -- -D warnings` |
| Unit-Tests | 71 bestanden | Domäne, Parser, Adapter, Katalog, Redaktion, Protokoll, Donut-Geometrie, CLI-Parser |
| Kernverträge | 21 bestanden | `tests/core.rs`: Migration v1→v2 mit Sicherung, neueres Schema abgewiesen, Generationen, Geheimnisse weder in DB noch WAL, Absturzmatrix der Bereinigung |
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
