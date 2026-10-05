# Windows-Abnahme · mogumogu 0.1.0

Für jeden Lauf Datum, Windows-Edition/Build, DPI, Rustversion, Commit, Slint und SQLite-Version festhalten. Windows 10 22H2 und Windows 11 getrennt prüfen; ein gehosteter CI-Server zählt nicht als Desktopabnahme.

Legende: ☑ geprüft · ☐ offen. Spalte „W10“ = Windows 10 Pro 22H2 (Build 19045), 5. Oktober 2026, 100 % DPI, Rust 1.98.0, Slint 1.18.1, SQLite 3.53.2. Spalte „W11“ ist noch vollständig offen.

## A. Build

| Prüfpunkt | W10 | W11 |
|---|---|---|
| `build.ps1 -NoRun` baut beide EXEs, alle Tests grün | ☑ | ☐ |
| `cargo fmt --check` und Clippy ohne Warnungen | ☑ | ☐ |
| Cargo.lock eingecheckt, Toolchain fixiert (`rust-toolchain.toml`) | ☑ | – |
| SQLite-Engine über `mogumogu-cli status` protokolliert | ☑ | ☐ |
| Quellverzeichnis mit Leerzeichen/Umlauten | ☐ | ☐ |

## B. Tray und Oberfläche

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Start ohne Fenster im Tray (`--tray`), Symbol sichtbar | ☐ manuell | ☐ |
| Linksklick öffnet, Rechtsklick-Menü reagiert | ☐ manuell | ☐ |
| Fensterkreuz schließt nur das Fenster; erneutes Öffnen funktioniert (100 Zyklen) | ☑ | ☐ |
| Zweiter Start holt das bestehende Fenster, kein zweiter Writer | ☑ | ☐ |
| Beenden (`owner stop`) hinterlässt keine Kindprozesse | ☑ | ☐ |
| Moduswechsel Demo/Lokal nach Beenden | ☑ | ☐ |
| Explorer-Neustart, Schlafmodus, Mehrmonitor | ☐ | ☐ |
| DPI 125/150/200 % | ☐ | ☐ |
| Tastatur: Strg+K, Alt+1…8, Tab, Enter/Leertaste, Esc im Dialog | ☑ (Alt+1…8, Esc, Fokusrahmen) | ☐ |
| Screenreader (Narrator) liest Navigation, Kennzahlen, Dialoge | ☐ | ☐ |
| Native Screenshots statt HTML-Vorschau | ☑ | ☐ |

## C. Daten und Funktionsgrenzen

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Demo: 6 Projekte, 428 Pakete, 142 GB, 6 KI-Einträge, 3 Prüfhinweise | ☑ | ☐ |
| Lokaler Erststart leer; unbekannte Größen/Pakete nicht null | ☑ | ☐ |
| Registrierung erstellt/öffnet den Pfad nicht | ☑ | ☐ |
| Lesefreigabe identitätsgebunden; ersetzter Ordner blockiert | ☑ (automatisiert) | ☐ |
| Junction im Projekt wird nicht verfolgt | ☑ (automatisiert) | ☐ |
| Teil-/Fehlerfassung erzeugt keine Entfernungsmeldungen | ☑ (automatisiert) | ☐ |
| Verwalteter Lauf: Starterende bei laufendem Worker bleibt gesperrt | ☑ (automatisiert) | ☐ |
| Bereinigung nur in Wegwerf-Testwurzel; Plan-Invalidierung, geschütztes Kind, Link-Blocker | ☑ (automatisiert) | ☐ |
| Absturz nach Journal-Commit: Abgleich ohne Wiederholung | ☑ (automatisiert) | ☐ |
| Neueres unbekanntes Schema wird nicht verändert; Migration mit Sicherung | ☑ (automatisiert) | ☐ |
| Keine Netzwerkzugriffe bei ausgeschalteten Updatehinweisen | ☑ (Code-Allowlist, kein Aufruf ohne Freigabe) | ☐ |
| Updatehinweis gegen echte öffentliche Registry | ☐ | ☐ |
| Scoop/Chocolatey/WinGet-Freigabe auf einem System mit diesen Managern | ☐ | ☐ |

## D. Betrieb und Messung

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Release-Messung Tray/Dashboard/100 Zyklen ([Messbericht](MESSBERICHT.md)) | ☑ | ☐ |
| Leerlauf-CPU 10 Minuten | ☐ (120 s gemessen) | ☐ |
| Desktop und CLI gleichzeitig: kein zweiter Writer | ☑ | ☐ |
| Nicht beschreibbares Datenverzeichnis: verständlicher Fehler | ☐ | ☐ |
| Entfernen von `dist` verändert keine Projekt- oder Scratchpadinhalte | ☑ (Daten liegen getrennt) | ☐ |

**Freigabe:** Die native Basis ist auf Windows 10 lauffähig (G0). Für G6 fehlen die offenen Punkte, insbesondere Windows 11, DPI und Screenreader. Produktive Bereinigung bleibt bis G5 gesperrt.
