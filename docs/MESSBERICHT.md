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

## Speicheranalyse ganzer Laufwerke (30. Oktober 2026)

Derselbe Windows-10-Rechner, Release-Build,
`MOGU_DISK_BENCH=<Laufwerk> cargo test --release --no-default-features --test performance -- --ignored --nocapture --exact disk_analysis_of_a_real_folder`.
Der Test analysiert direkt über die Lesegrenze, ohne Datenbank und ohne
Namensausgabe; private Bytes alle 250 ms aus dem Testprozess abgetastet.
Grenzen: 4 Mio. Einträge, 64 Ebenen, 180 s.

| Laufwerk | Einträge | Analyse | Verzeichnis im Speicher | Spitze private Bytes | Ergebnis |
|---|---|---|---|---|---|
| C: (865 GB belegt, Systemfestplatte) | 3.067.201 (2,60 Mio. Dateien) | 90 s, beim zweiten Mal 54 s | 198,0 MiB (68 Bytes je Eintrag) | 218 MiB | bis auf 153 nicht lesbare Systemordner und 188 Verknüpfungen gelesen |
| I: (706 GB belegt, Projekte und Daten) | 1.382.813 (1,24 Mio. Dateien) | 30 s | 83,3 MiB (63 Bytes je Eintrag) | 95 MiB | bis auf 3 nicht lesbare Systemordner und 503 Verknüpfungen gelesen |

Seitenabfragen auf C: (2,6 Mio. Dateien): Ordner 5 ms, größte Dateien 0,12 s,
Sortierung nach Name 0,10 s, Duplikat-Kandidaten 0,05 s, Dateitypen 0,34 s,
Filter `*.dll` 0,66 s. Bevor eine Seite nur die Einträge bis zu sich selbst
ordnete, brauchten größte Dateien 0,90 s und die Sortierung nach Name 1,24 s.

Zwischenstand mit 2 Mio. Einträgen: C: brach nach 74 s (kalt; warm 15 s) am
Limit ab, das Verzeichnis belegte 134 MiB, die Spitze 146 MiB. Davor überschritt
jeder Namensblock seine Größe um bis zur Hälfte (eine Verdopplung ab ungerader
Startgröße); dasselbe Verzeichnis belegte damit 160 MiB. Das alte Limit von
200.000 Einträgen hätte von I: etwa 15 % der Einträge gelesen. Das Verzeichnis
existiert nur, solange eine Analyse offen ist; Schließen des Fensters gibt es
frei. Der Tray-Leerlauf (50-MiB-Ziel) ist davon nicht betroffen, gemessen wurde
das aber noch nicht im laufenden Tray (Punkt in der
[Windows-Abnahme](WINDOWS-ABNAHME.md)).

## Schwächere Rechner und Buildzeit (1. November 2026)

Derselbe Rechner: Windows 10 Pro 22H2 (Build 19045), AMD Ryzen 7 9800X3D,
16 Threads, 31 GiB, Bildschirm 3840 × 2160 bei 200 % Skalierung. Rust 1.98.0,
Slint 1.18.1 mit Software-Renderer. Editor und Werkzeuge offen, Hintergrundlast
nicht kontrolliert, Schutzsoftware unverändert; während der Laufzeitmessungen lief
kein Cargo-Build. Laufzeit mit der Demodatenbank in eigenen Wegwerf-Datenordnern,
die lokale Inventardatenbank blieb unberührt. Rohdaten der Fenstermessung:
[messung-20261101-184837](messungen/messung-20261101-184837.md).

**Was einen schwächeren Rechner belastet.** Kein `target-cpu`: Die Programme
verwenden den x86-64-Grundbefehlssatz und laufen auf jedem 64-Bit-Prozessor; eine
32-Bit- oder ARM64-Ausgabe gibt es nicht. Der Software-Renderer braucht keine
Grafikkarte und keinen Treiber, zeichnet aber auf einem einzigen CPU-Kern, und
seine Arbeit wächst mit der Pixelzahl. Der Besitzer arbeitet mit einem Worker- und
einem IPC-Thread; Erfassungen sind zeitbegrenzt (`ScanLimits`) und enden auf
langsamen Platten früher als „teilweise“, statt länger zu laufen.

**Laufzeit des Release-Builds mit dem neuen Profil** (`scripts/measure.ps1
-Cycles 100 -IdleSeconds 60 -Redraws 20`):

| Phase | Private Bytes | Threads |
|---|---|---|
| Tray nach dem Start / nach 60 s Leerlauf | 4,3 / 4,2 MiB | 12 / 8 |
| Dashboard geöffnet | 21,5 MiB | 10 |
| Tray nach 10 / 50 / 100 Fensterzyklen | 11,2 / 11,6 / 11,6 MiB | 10 / 10 / 6 |

Leerlauf-CPU 0 % eines Kerns über 60 s, Anzeige-Anfrage p95 5,8 ms. Neu misst das
Skript das Neuzeichnen des ganzen Fensters: F1 öffnet die Hilfe (der Hintergrund
dunkelt ab, die Karte hebt sich), Esc schließt sie, zusammen etwa 25 Bilder. Bei
2160 × 1360 Pixeln kostet ein solcher Zyklus 96 ms CPU, also rund 4 ms je
vollständig gezeichnetem Bild. Das Dashboard mit dem echten Inventar dieses
Rechners (drei Projekte, 919 Paketversionen) belegte im Build `fast` 25 MiB Private Bytes.

Vergleich der Builds, je eine eigene Demo-Instanz, zwei bis drei Läufe:

| Build | CPU je Dialogzyklus | Fenster erscheint nach |
|---|---|---|
| Release bisher (eine Codegen-Einheit, Thin-LTO) | 83–128 ms | 45–105 ms |
| Release neu (16 Einheiten, ohne LTO) | 80–106 ms | 37–389 ms¹ |
| `fast` (opt-level 1) | 231–271 ms | 91–116 ms |
| `fast` mit Abhängigkeiten auf opt-level 3 | 212–215 ms | 71–117 ms |

¹ Einzelne Ausreißer bis 433 ms kamen in jeder Variante vor und folgen dem
Dateisystem-Cache, nicht dem Profil.

Referenzlast (50 Projekte, 10.000 Pakete), je drei Läufe: Snapshot warm Median
28,1–29,6 ms bisher und 29,3–31,2 ms neu; mit laufender Uhr 28,1–29,6 und
29,3–29,7 ms; erste Erfassung 473–506 und 478–488 ms; Neuerfassung 515–576 und
513–528 ms; erster Snapshot 152–157 und 151–187 ms. Der Verzicht auf LTO kostet
damit höchstens etwa 1,5 ms beim warmen Snapshot, der Rest liegt in der Streuung.

**Abschätzung für schwächere Hardware, keine Messung.** Einfache x64-Prozessoren
(Celeron, Pentium Silver, ältere Doppelkerne) erreichen grob ein Viertel bis ein
Fünftel der Einzelkernleistung dieses Prozessors; das ist eine Annahme, hier nicht
gemessen. Bei 1080 × 680 logischen Pixeln und 100 % Skalierung (0,73 Mio. Pixel)
ergäbe das etwa 4–5 ms je vollständigem Bild, bei 125 % (1,15 Mio.) etwa 6–8 ms.
Animationen laufen nach Zeit: Ein langsamer Rechner zeichnet weniger Zwischenbilder,
der Dialog ist trotzdem nach 180 ms offen. 25 MiB im Betrieb fallen neben 4 GB
Arbeitsspeicher nicht ins Gewicht; die Speicheranalyse eines ganzen Laufwerks belegt
aber, solange sie offen ist, bis etwa 300 MB (siehe 30. Oktober 2026). Belegt wird
die Abschätzung erst durch eine Messung auf solcher Hardware.

**Buildzeit.** Slint erzeugt aus `ui/` 14,7 MB Rust (143.000 Zeilen) in einer
Kiste. Mit einer Codegen-Einheit und Thin-LTO übersetzte ein einziger Kern davon das
Desktop-Programm: 231 der 287 s eines Release-Builds nach einer Dateiänderung.

| Release-Profil | Nach einer Dateiänderung | Sauber | `mogumogu.exe` |
|---|---|---|---|
| bisher: eine Einheit, Thin-LTO | 287 s | – | 45,5 MB |
| 16 Einheiten, Thin-LTO | 158 s | 198 s | 49,5 MB |
| **neu: 16 Einheiten, ohne LTO** | **113 s** (`rebuild.ps1` samt Stopp und Neustart: 127 s) | **167 s** | **46,1 MB** |

Das Lesen und Prüfen des erzeugten Codes bleibt einkernig; deshalb helfen weitere
Kerne nur zum Teil, und er braucht Arbeitsspeicher: Der größte Compilerprozess
belegte für das Desktop-Programm 3,14 GB im Release- und 1,89 GB im `fast`-Profil,
für die Kerntests ohne Oberfläche 0,53 GB (alle zusammen 0,94 GB, 4,9 s nach
einer Dateiänderung). `rebuild.ps1 -Fast` braucht nach einer Rust-Änderung 12–19 s,
nach einer `.slint`-Änderung 12 s. Abhängigkeiten in `fast` auf opt-level 3
verkürzten das Neuzeichnen nur um 20 %, weil die meiste Arbeit im erzeugten Code
dieser Kiste liegt, und verlängerten den ersten Build auf 286 s; verworfen. Die
Tests (`cargo test --no-default-features --no-run`) bauen nach einer Dateiänderung
in 5,3 s; die 15 Testprogramme linken parallel, sie zusammenzulegen lohnt nicht.

**Nebenbei behoben.** Das Dashboard bekam seine Startgröße in physischen Pixeln:
Bei 200 % öffnete es mit 540 × 340 logischen Pixeln, also auf der Mindestgröße
900 × 560. Jetzt sind es wie dokumentiert 1080 × 680 logische Pixel (hier 2160 × 1360
statt 1800 × 1120). Die Layoutprüfung (`scripts/analyze_layout.py --png`) behielt
neben jedem PNG das 30- bis 60-mal größere PPM; jeder Lauf belegte so rund 1,5 GB.

**Speicherplatz des Builds.** `target/` war auf diesem Rechner rund 80 GB groß:
`target/debug` 45 GB, davon 34 GB inkrementelle Caches in 489 Sitzungsordnern, die
nach Feature-, Profil- und Compilerwechseln liegen bleiben; zwölf alte
Layoutprüfungen rund 19 GB, fast alles PPM; `release` 3,1 GB, `fast` 2,9 GB. Cargo
räumt Caches anderer Konfigurationen nicht selbst auf. `cargo clean` oder das
Löschen von `target\debug\incremental` gibt den Platz frei; der nächste Build ist
dann ein vollständiger.

Offen: eine Messung auf tatsächlich schwacher Hardware, mit Festplatte statt SSD,
unter Windows 11 und bei 100 % Skalierung.
