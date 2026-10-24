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

## Wiederholung nach dem Modulumbau

Am 5. Oktober 2026 auf derselben Windows-10-Hardware, Rust 1.98.0 und
Slint 1.18.1. Release-Build, neue Demodatenbank, Schutzsoftware unverändert,
Docker Desktop aktiv. Rohdaten: [100 Fensterzyklen](messungen/messung-20261005-184947.md).
Während des frühen Leerlaufs lief die Kompilierung des Referenztests;
die Messbedingungen sind daher kein kontrollierter Vorher-/Nachher-Vergleich.

| Phase | Private Bytes | Handles | Threads |
|---|---|---|---|
| Tray vor dem ersten Fenster | 3,2 MiB | 176 | 6 |
| Tray nach 60 s Leerlauf | 3,2 MiB | 177 | 5 |
| Dashboard | 27,5 MiB | 268 | 12 |
| Tray nach 10 / 50 / 100 Zyklen | 6,9 / 7,6 / 7,5 MiB | 266 / 266 / 265 | 12 / 12 / 12 |

Leerlauf-CPU: 0 % eines Kerns über 60 s. Anzeige-Anfrage p95: 10,7 ms;
das misst die IPC-Bestätigung, nicht das vollständige Zeichnen. Nach dem
Aufwärmen blieb der Speicher stabil; ein zehnminütiger CPU-Nachweis fehlt weiter.

Der Referenztest meldet jetzt zusätzlich 30 warme Snapshot-Samples:
vor dem Umbau Median 33,8 ms, p95 34,7 ms; nach dem Umbau in drei Wiederholungen
Median 38,2–44,7 ms, p95 48,2–59,4 ms. Diese Läufe überschnitten sich mit
Fensterzyklen und weiterer Hintergrundlast. Daraus folgt **kein belastbarer
Beschleunigungsnachweis**; die gespeicherte Übersicht bleibt in dieser Fixture
deutlich unter dem 500-ms-Ziel. Für einen kontrollierten Vergleich alle Build-
und Containerarbeit beenden und dieselbe Fixture wiederholen.

Nachweisbar reduzierte Arbeit: Statuswechsel ersetzen keine Listenmodelle;
die Projektsuche ersetzt nur Projektzeilen statt aller Modelle und des
Speicherdiagramms. Review und Snapshot teilen geladene Ressourcen, Sessions,
Besitzer und Einstellungen. Der statische Validator steigt nicht mehr in
`target`, `dist` und `.git` ab (13 Verträge: zuvor 1,058 s, danach 0,043 s in
diesen Läufen). Der headless Build-Script-Abgleich beobachtet nur `build.rs`;
Dokumentationsänderungen lösen dort keinen Paket-Neuaufbau mehr aus.

Die geänderten zentralen Farben liefern rechnerisch: Sekundärtext auf Canvas
4,88:1 (zuvor 3,57:1), Weiß auf Primärblau 4,96:1 (zuvor 4,14:1), Fokusrahmen
auf Weiß 4,43:1 (zuvor 2,16:1). Das ist kein Nachweis für die Barrierefreiheit
aller Texte und Zustände; DPI und Screenreader bleiben manuell zu prüfen.

## Weitere offene Messungen

Windows 11, eine zweite Hardware, GPU-Speicher, I/O-Last großer realer Projektbäume, die Watcher-Last unter Dauerschreiblast sowie Zeiten bis zum vollständigen Zeichnen des Fensters. Diese Punkte gehören zur Abnahme G6.

## Paketbelege und Referenzanalyse (17. Oktober 2026)

Reproduzierbar mit `cargo test --locked --offline --release --no-default-features
--test performance -- --ignored --nocapture`. Rust 1.98.0, optimierter Build,
Windows 10.0.19045.0 (lokal geprüft), AMD64 Family 26 Model 68 Stepping 0,
16 logische Prozessoren. Vollständiger CPU-Name und RAM wurden in dieser
Toolsitzung nicht erneut über WMI erhoben. Schutzsoftware-Einstellungen wurden
nicht verändert. Kein Registryverkehr, keine geöffneten Test-Dashboards;
Editor und Tools aktiv, keine kontrollierte Lastisolation.

Gleiche Referenzfixture: 50 Projekte, je 200 npm-Pakete, zusätzliche KI-Dateien.
Der Snapshot enthält jetzt auch Paketbelege, Quellenstatus und Referenzabdeckung.

| Messung | Ergebnis |
|---|---|
| Erste Erfassung | 250,6 ms |
| Neuerfassung ohne Änderung | 265,2 ms |
| Erster Snapshot einschließlich Belegaufbau | 124,1 ms |
| Warme Snapshots, 30 Samples | Median 51,2 ms; p95 55,2 ms |
| Datenbank nach zwei Generationen | 8.672 KiB |

Die Fixture verwendet eine feste Uhr. Warme Samples messen dieselbe Revision,
Uhrzeit und Seite; neue Commits, Uhrzeit oder Seite erzwingen die Neubewertung.
Der Seitencache hält maximal 60 aufbereitete Paketidentitäten, kein volles
Inventar. Gemeinsame Projekt-/Scope-Daten werden beim Snapshot wiederverwendet.
Ein erster Lauf ohne diese Wiederverwendung meldete etwa 200 ms pro Snapshot;
das ist ein interner Diagnosewert, kein allgemeiner Beschleunigungsnachweis.

Die zusätzlichen Belege kosten gegenüber einem reinen Inventar mehr Arbeit und
Speicher auf Platte. Beide gemessenen Snapshot-Werte liegen auf diesem Host unter
dem 500-ms-Ziel; daraus folgt kein Nachweis für größere Bestände oder andere
Hardware. Tray-Private-Bytes, zehnminütiger Leerlauf, 100 Fensterzyklen und
Windows-11-Abnahme wurden für diese Erweiterung nicht neu gemessen. Die älteren
Messreihen oben sind historische Ergebnisse ihres jeweiligen Builds.

## Architekturumbau und schlanke Statusabfrage (17. Oktober 2026)

Derselbe Host: Windows 10.0.19045.0, AMD64 Family 26 Model 68 Stepping 0,
16 logische Prozessoren, Rust 1.98.0, optimierter Build. Die Hardwareangaben
wurden über .NET/Umgebung erneut bestätigt; vollständiger CPU-Name und RAM
wurden nicht erhoben. Editor und Tools waren aktiv, die übrige Hintergrundlast
war nicht kontrolliert. Während der Messungen lief kein weiterer Cargo-Build.
Kein Registryverkehr und kein geöffnetes Testfenster.

```powershell
cargo test --locked --offline --release --no-default-features --test performance -- --ignored --nocapture
```

Drei aufeinanderfolgende Läufe derselben Fixture (50 Projekte, 10.000 Pakete),
jeweils 30 warme Snapshots und 30 Statusabfragen. Die Testläufe dauerten nach dem
Build etwa 2,9–3,9 Sekunden. Die Tabelle zeigt die Spanne der drei Ergebnisse.

| Messung | Ergebnis |
|---|---|
| Erste Erfassung | 277,5–318,0 ms |
| Neuerfassung ohne Änderung | 284,9–334,4 ms |
| Erster Snapshot einschließlich Belegaufbau | 150,5–284,8 ms |
| Warme Snapshots: Median / p95 | 62,9–82,6 ms / 68,4–128,6 ms |
| Status im Kern: Median / p95 | 12,7–19,3 µs / 19,4–46,6 µs |
| Datenbank nach zwei Generationen | 8.672 KiB |

Status liest nach dem Sessionabgleich vier Gesamtzähler in einer kurzen
Lesetransaktion. Dafür werden weder Paketbelege geladen noch Dashboard-Modelle
berechnet. Die Statuswerte messen den Kernaufruf, nicht CLI-Start oder IPC.
Die Snapshot-Samples verwenden wie zuvor eine feste Uhr und damit denselben
gültigen Paket-Seitencache.

Die Snapshotzeiten liegen höher als in der vorigen Messreihe und schwanken
zwischen den aktuellen Läufen. Ein allgemeiner Beschleunigungsnachweis für
das Dashboard ergibt sich daraus nicht. Nachweisbar entfällt bei Statusaufrufen
die vollständige Snapshot-Arbeit. Alle aktuellen Snapshotwerte bleiben auf
diesem Host unter dem 500-ms-Ziel. Neue Tray-, Fensterzyklus-, DPI- oder
Windows-11-Messungen sind damit nicht verbunden.

## Abfragepläne, Seitencache und Tray-Zusammenfassung (18. Oktober 2026)

Derselbe Host: Windows 10.0.19045.7807, AMD64 Family 26 Model 68 Stepping 0,
16 logische Prozessoren, Rust 1.98.0, optimierter Build. Editor und Tools aktiv,
Hintergrundlast nicht kontrolliert; während der Messläufe lief kein weiterer
Cargo-Build. Gleiche Referenzfixture (50 Projekte, 10.000 Pakete).

Befunde vor der Änderung:

- Die Referenzmessung verwendet eine feste Uhr. Der Paket-Seitencache war an die
  exakte Sekunde gebunden und traf deshalb mit der Systemuhr praktisch nie. Ein
  Snapshot mit laufender Uhr dauerte 116,5–117,0 ms statt der gemessenen 50 ms.
- Projektliste und Managerzählung liefen laut Abfrageplan über den Index
  `inventory_items_package` durch alle gehaltenen Generationen (20.800 statt
  10.400 Zeilen), jeweils mit Einzelabfragen auf `generations` und `scopes`.
- Ohne geöffnetes Dashboard berechnete der Besitzer nach jeder Änderung einen
  vollständigen Snapshot. Der Tray nutzte davon nur Einstellungen und die Anzahl
  der Prüfhinweise; der Headless-Betrieb verwarf ihn.

Änderungen: `CROSS JOIN` legt die Schleifenreihenfolge auf veröffentlichte
Generationen fest. Auf einer Kopie der Fixture lieferten alte und neue Abfragen
identische Zeilen. Der Seitencache gilt bis zur nächsten Veraltungsgrenze einer
Quellenprüfung (`updates::stale_at`). Ohne sichtbares Dashboard veröffentlicht
der Besitzer eine `TraySummary`. Lese-, Lösch- und Journalgrenzen sind unverändert.

| Messung | Vorher | Nachher (drei Läufe) |
|---|---|---|
| Snapshot, feste Uhr: Median / p95 | 50,0 / 53,1 ms | 24,9–26,5 / 28,6–35,2 ms |
| Snapshot, laufende Uhr: Median / p95 | 116,5–117,0 / 120,3–122,4 ms¹ | 24,4–25,0 / 27,3–31,0 ms |
| Projektliste (`Repository::projects`) | 23,4 ms¹ | 6,9 ms¹ |
| Managerzählung (`ecosystem_counts`) | 13,9 ms¹ | 5,5 ms¹ |
| Arbeit nach Änderungen ohne Dashboard | vollständiger Snapshot | Tray-Zusammenfassung: Median 29–48 µs |
| Erster Snapshot einschließlich Belegaufbau | 117,1 ms | 95,5–105,9 ms |
| Erste Erfassung / Neuerfassung | 250,0 / 268,6 ms | 256,1–272,6 / 272,6–298,5 ms |

¹ Aus einem temporären Messprogramm dieser Sitzung mit derselben Fixture. Es ist
nicht Teil des Repositorys. Die Referenzmessung enthält jetzt selbst die Reihen
„laufende Uhr“ und „Tray-Zusammenfassung“.

Die Erfassungszeiten sind nicht Ziel dieser Änderung und schwanken im Rahmen
der früheren Läufe. Fensterzyklen, Tray-Private-Bytes, zehnminütiger Leerlauf,
DPI und Windows 11 wurden nicht neu gemessen.

## Realer Bestand (24. Oktober 2026)

Bisher maß mogumogu nur synthetische Fixtures. Diese Reihe läuft auf echten
Daten: derselbe Windows-10-Rechner, Release-Build, eigenes Datenverzeichnis im
Temp-Ordner (die lokale Datenbank blieb unberührt), Schutzsoftware aktiv, Editor
und Tools offen. Zeiten sind CLI-Aufrufe einschließlich Prozessstart und IPC.
Bestand: Laufwerk I: mit 626 GB belegt (Projekte, Spiele, Werkzeuge, Modelle)
und elf echte Projekte (Rust, Python/uv, npm, Bun, .NET, Go, Gradle, CMake).

| Schritt | Ergebnis |
|---|---|
| Kaltstart des Besitzers mit neuer Datenbank / warmer `status` | 0,6–0,9 s / 6–30 ms |
| Elf Projekte registrieren und freigeben (22 Aufrufe) | 0,23 s |
| Erste Erfassung der elf Projekte, 18.500–20.900 Einträge | 7,5 s (einzeln 15 ms bis 3,2 s) |
| Neuerfassung aller Bereiche samt Laufwerk | 5,8–6,5 s |
| Dashboard-Snapshot `list` kalt / warm | 0,52–0,55 s / 0,26–0,27 s |
| Paketseite, Suche, Fundliste | 0,25 s, 0,08–0,11 s, 0,08–0,12 s |
| Aufräumbericht über elf Projekte | 3,3–5,1 s |
| Kontextfilter-Vorschau für alle / persönlicher Ordner `.claude` erfassen | 0,40 s / 13 ms |
| Besitzer nach allen Schritten | 14–19 MiB Private Bytes, 6 Threads |

Der kalte Snapshot liegt mit dieser Datenmenge knapp über dem 500-ms-Ziel, der
warme deutlich darunter; die Referenzfixture (10.000 Pakete) bildet reale
Bestände mit 20.000 Einträgen und einer Laufwerkssuche nicht ab.

Der Aufräumbericht fand 159,5 GB wiederherstellbaren Inhalt in 239 GB
Projektbestand, jeweils mit Beleg (größter Posten: 90 GB Cargo-`target` mit
`Cargo.toml` daneben und `CACHEDIR.TAG` darin). Der Kontextfilter schlug für
alle elf Projekte Leseverbote vor. Nicht inventarisiert wurden die
Go-Abhängigkeiten (0 Pakete), die Bun-Versionen (nur 75 deklarierte npm-Namen)
und die Gradle-Abhängigkeiten.

**Laufwerkssuche vor und nach den Korrekturen** (Pfadredaktion je Name,
`CACHEDIR.TAG` als Cache-Blatt, Teilprojekte nach äußeren Projekten):

| Laufwerkssuche I: | Vorher (fc421be) | Nachher |
|---|---|---|
| Dauer, vollständig | 14,6 s | 10,0 s und 14,3 s (zwei Läufe) |
| Gezählte Einträge | 7.494 | 3.268 |
| Projektfunde mit redigiertem Pfad (nicht registrierbar) | 1.024 von 1.198 (85 %) | 167 von 1.732 (10 %) |
| „Lose Skripte“ / „Scratchpads“ | 1.827 / 1.306 | 14 / 299 |
| Registrierbare Projektzeilen auf der ersten Seite (200) | 157 | 195 |
| Redigierte Pfade in den elf Projekterfassungen | 2.389 von 18.482 (13 %) | 244 von 20.923 (1,2 %) |

Vorher verdeckten zusammengefallene Pfade gleichnamige Funde; nachher sind sie
einzeln sichtbar, daher steigt die Zahl der Projektfunde. Die verbleibenden
Projektfunde stammen überwiegend aus Testfixtures und Monorepos eines Projekts
(925 gleichnamige Temp-Ordner mit `CMakeLists.txt`). Die Dauer der Laufwerkssuche
schwankt mit dem Dateisystem-Cache stärker als der Effekt der Änderung; ein
Zeitgewinn ist damit nicht belegt, wohl aber weniger gelesene Einträge (allein
der uv-Cache enthält 17.000 Python-Dateien, die nicht mehr betreten werden).
Mit der CI-Regel stehen alle zehn echten Projekte der Laufwerkssuche in den
ersten 18 Zeilen der Fundliste (Astra_Assistant vorher auf Zeile 164).

**Nach dem Schließen der Lücken** (gleicher Rechner, neues Datenverzeichnis,
Laufwerkssuche, gesammelte Übernahme, dann eine Erfassung aller Bereiche):

| Schritt | Ergebnis |
|---|---|
| Laufwerkssuche I: | 12,2 s, vollständig |
| Vorschau der sicheren äußeren Projekte / Übernahme von 18 Projekten | 16 ms / 0,10 s |
| Erste Erfassung der 18 Projekte samt Laufwerk | 11,1 s, alle vollständig |
| Dashboard-Snapshot kalt / warm, Paketseite | 0,50 s / 0,28 s, 0,27 s |
| Projektfunde mit redigiertem Pfad / redigierte Einträge in Projekterfassungen | 0 von 1.738 / 0 von 23.942 |
| Go (Sparkle), Bun (koharu), Gradle (lumina) | 27 + 46, 1.882 aufgelöst, 45 deklariert |
| Besitzer danach | 11,8 MiB Private Bytes |
