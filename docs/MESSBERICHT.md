# Messbericht · mogumogu 0.1.0

Gemessen am 5. Oktober 2026 auf **einem** Rechner. Die Werte belegen das Verhalten dieses Builds auf dieser Hardware; sie sind keine allgemeine Zusage. Rohdaten: [`docs/messungen/`](messungen/).

| Umgebung | Wert |
|---|---|
| Windows | Windows 10 Pro 22H2, Build 19045, x64 |
| Hardware | AMD Ryzen 7 9800X3D, 31 GiB RAM |
| Build | Release (`build.ps1 -NoRun`), Rust 1.98.0, LTO thin, Slint 1.18.1 (winit, Software-Renderer) |
| Daten | Demodatenbank in einem frischen Wegwerf-Datenverzeichnis |
| Virenschutz | unverändert aktiv |
| Verfahren | `scripts/measure.ps1 -Cycles 100 -IdleSeconds 120` |

## Tray, Dashboard und Fensterzyklen

| Phase | Private Bytes | Working Set | Handles | Threads |
|---|---|---|---|---|
| Tray nach Start, vor dem ersten Fenster | 3,2 MiB | 27,1 MiB | 176 | 6 |
| Tray nach 120 s Leerlauf | 3,2 MiB | 27,1 MiB | 177 | 5 |
| Dashboard geöffnet | 27,6 MiB | 55,6 MiB | 268 | 12 |
| Tray nach 10 Öffnen/Schließen-Zyklen | 7,2 MiB | 35,1 MiB | 266 | 12 |
| Tray nach 50 Zyklen | 7,1 MiB | 35,2 MiB | 264 | 11 |
| Tray nach 100 Zyklen | 7,2 MiB | 35,3 MiB | 263 | 12 |

| Ziel (Implementierungsplan §8) | Ergebnis auf diesem Rechner |
|---|---|
| Tray unter 50 MiB Private Bytes | erfüllt: 3,2 MiB vor dem ersten Fenster, 7,2 MiB nach Fensterzyklen |
| Leerlauf-CPU unter 0,5 % eines Kerns | erfüllt: 0 % über 120 s (Messauflösung der Prozess-CPU-Zeit ≈ 16 ms) |
| Fensterzyklen ohne fortlaufendes Wachstum | erfüllt nach dem Aufwärmen: 10 → 50 → 100 Zyklen stabil (7,2 / 7,1 / 7,2 MiB; Handles 266 → 263) |
| Gespeicherte Übersicht p95 < 500 ms | Kern-Snapshot der Referenzlast: 32 ms; Anzeige-Anfrage per IPC p95: 5,1 ms (bestätigt, nicht „fertig gezeichnet“) |

Nach dem ersten Fenster bleiben Renderer- und Fensterthreads sowie Glyph-Caches einmalig bestehen (+4 MiB Private Bytes, +6 Threads gegenüber dem reinen Tray). Danach wächst nichts weiter. Die zehnminütige Leerlaufmessung des Plans ist mit `-IdleSeconds 600` reproduzierbar, wurde hier aber auf 120 s verkürzt.

## Referenzlast

`cargo test --release --no-default-features --test performance -- --ignored --nocapture`: 50 Projekte mit je 200 aufgelösten npm-Paketen (10 000 Paketinstallationen), dazu AGENTS.md und ein Skill je Projekt.

| Schritt | Dauer |
|---|---|
| Erste Erfassung aller 50 Bereiche (inkl. Vergleichsbasis) | 0,36 s |
| Neuerfassung ohne Änderung (keine Ereignisse) | 0,36 s |
| Dashboard-Snapshot | 32 ms |
| Datenbankgröße | 4,9 MiB |

Eine erste Fassung brauchte 959 ms für den Snapshot, weil Paketzahlen je Projekt über korrelierte Unterabfragen ermittelt wurden. Die gruppierte Abfrage (`src/storage/projects.rs`) senkt das auf 32 ms; Sicherheitsprüfungen wurden dafür nicht verändert.

## Nicht gemessen

Windows 11, eine zweite Hardware, GPU-Speicher, I/O-Last großer realer Projektbäume, die Watcher-Last unter Dauerschreiblast sowie Zeiten bis zum vollständigen Zeichnen des Fensters. Diese Punkte gehören zur Abnahme G6.
