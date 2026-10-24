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
| Tastatur: Strg+F/Strg+K, Strg+N, F1/F5, Alt+1…7, Tab, Enter/Leertaste, Esc im Dialog | ☐ nach Zusammenführung auf sieben Bereiche erneut prüfen (früher: Alt+1…8, Esc, Fokusrahmen) | ☐ |
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
| Gepinnter Elternordner: weder er noch ein Ordner darüber lässt sich umbenennen, das Ziel darin schon | ☑ (automatisiert) | ☐ |
| Bereinigung nur in Wegwerf-Testwurzel; Plan-Invalidierung, `.mogumogu-keep`, Junction auf dem Weg | ☑ (automatisiert, Test-Papierkorb) | ☐ |
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

## E. Zusatzprüfungen nach dem Modulumbau

Die bisherigen Häkchen gelten für die Basisabnahme. Die folgenden UI-Änderungen
vom 5. Oktober 2026 brauchen zusätzlich eine visuelle und tastaturbasierte Abnahme.
Der native Release-Build und 100 Öffnen-/Schließen-Anfragen per IPC bestanden erneut.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Globale Suche führt zu den Projekten; Name, Pfad und Technologie werden gefunden | ☐ | ☐ |
| Leere Suche bietet Zurücksetzen; leeres Inventar bietet Registrierung | ☐ | ☐ |
| Ungültiges Formular erhält Eingaben und zeigt den Fehler; Korrektur ist möglich | ☐ | ☐ |
| Formular beginnt im ersten Feld; Enter reicht nur einen Auftrag ein | ☐ | ☐ |
| Paketmanager-, KI- und Speicherhinweisflächen per Tab/Enter/Leertaste bedienen | ☐ | ☐ |
| Strg+K bleibt bei offenem Dialog im Dialog; Esc und Schließen funktionieren | ☐ | ☐ |
| Große Lesefreigabenliste scrollt, Ereignisliste bleibt erreichbar | ☐ | ☐ |
| Beschäftigt-Zustand sperrt Schreibaktionen; Navigation und Dialogschließen bleiben möglich | ☐ | ☐ |
| Geänderte Farben, Formularhöhe und Fokusrahmen bei 100–200 % DPI prüfen | ☐ | ☐ |

## F. Kompaktes Fenster (17. Oktober 2026)

Slint-Renders und direkt eingespeiste Slint-Tastenereignisse ergänzen diese
Prüfpunkte. Sie ersetzen keine Bedienung des echten Windows-Fensters.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Alle sieben Bereiche und ihre Reiter bei 1080 × 680 und 900 × 560 ohne Überlagerung bedienen | ☐ | ☐ |
| Befehlsleiste und Statusleiste bleiben bei Verkleinerung erreichbar | ☐ | ☐ |
| Einstellungen, Lesefreigaben und lange Planlisten vertikal scrollen | ☐ | ☐ |
| Strg+F aus einem Eingabefeld, F5 und Alt+1…7; Dialoge sperren Hintergrundkürzel | ☐ | ☐ |
| Nach Esc, Schließen-Schaltfläche und erfolgreichem Formular funktionieren Kürzel wieder | ☐ | ☐ |
| Strg+N nur lokal; ungültige Formulare behalten Werte und sichtbaren Fehler | ☐ | ☐ |

## G. Paketbelege und globale Bereiche (17. Oktober 2026)

Automatisierte Fixtures prüfen Daten- und Leseverträge. Diese Bedienpunkte des
echten Windows-Fensters bleiben separat abzunehmen.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Paketbestand, Seitennavigation und lange Paketdetails bei beiden Fenstergrößen bedienen | ☐ | ☐ |
| Suchordner hinzufügen, Funde ansehen und registrieren; Inhaltsfreigabe bleibt eigener Schritt | ☐ | ☐ |
| Globale Bestände am Standardort („Lesen erlauben“) und an abweichendem Ort („Ort hinzufügen …“) freigeben; ungeeignete Ordner zeigen einen sichtbaren Fehler | ☐ | ☐ |
| Quellen offline/nicht freigegeben, veraltete Warnungen und gleiche Namen verschiedener Versionen prüfen | ☐ | ☐ |
| Behalten und Vormerkung lösen; Upstream-Warnung bleibt sichtbar | ☐ | ☐ |
| Projekte → Funde, Umgebung oder Build-Ausgabe: Fundstelle, Projektbezug; keine Entfernungsaktion | ☐ | ☐ |

## H. Mochi und japanische Schrift (18. Oktober 2026)

Native Renders und Unicode-Tabellen prüfen Darstellung und Zeichenvorrat;
die tatsächliche Windows-Eingabe und Darstellung bei anderem DPI bleiben hier.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Mochi in Explorer, Fenster, Taskleiste und Tray bei 100–200 % DPI sowie hellem/dunklem Systemhintergrund prüfen | ☐ | ☐ |
| Japanische Projekt-/Paketnamen in Listen und Dialogen lesen; beide Schriftgewichte ohne Ersatzkästchen | ☐ | ☐ |
| Japanische IME-Eingabe, Auswahl, Cursor und Kopieren in Suche/Formularen prüfen | ☐ | ☐ |
| Wortmarke mit Narrator als mogumogu erkannt; dekoratives Maskottchen erzeugt kein zusätzliches Fokusziel | ☐ | ☐ |
| Erfassung, Pause und hängendes Experiment wechseln zum passenden Mochi-Ausdruck; der Bildbereich über Einstellungen bleibt stabil | ☐ | ☐ |
| Vereinheitlichter v2-Zeichensatz: alle vier Posen bei 100–200 % DPI mit vergleichbarer Strichstärke und Größe; Ordner, Laptop und Ausdruckszeichen vollständig sichtbar | ☐ | ☐ |

## I. Systembefund, Einstieg, Experimente und Tray (18. Oktober 2026)

Automatisiert geprüft: Systembefund mit synthetischer Umgebung, `user-ai`-Erfassung
entlang der Katalogmuster, Migration v3→v4, hängende Sessions, Suche, Export und
neue Einstellungen. Native Renders (`design/screens/render.ps1`) zeigen alle zehn
Ansichten, den Einstieg, Suche, Fortschritt und Pause bei 1080 × 680, 900 × 560
und 150 %. Am Entwicklungsrechner (W10) lief der Systembefund in 9,7 ms. Die
Bedienung des echten Fensters steht aus.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Systembefund auf diesem Rechner: Funde passen zu `where.exe`, keine Programmstarts (Prozessmonitor), UNC-/Netzlaufwerk im `PATH` wird übersprungen | ☐ | ☐ |
| Persönlichen KI-Ordner freigeben und erfassen; `projects`, `todos`, Sitzungen und Zugangsdateien erscheinen nicht | ☐ | ☐ |
| Projekt hinzufügen: Ordnerdialog (`SHBrowseForFolderW`), Freigabe, Erfassung mit Fortschritt, Abbrechen; bei leerem Inventar öffnet sich nichts von selbst, die Übersicht bietet „Projekt hinzufügen“ und „Laufwerk durchsuchen“ | ☐ | ☐ |
| Hängende Session – entfällt, Sessions am 20.10.2026 entfernt | – | – |
| Tray: Statuszeile, Hinweisanzahl, Pause 1 Std./bis morgen, Fortsetzen nach Ablauf, Einstellungen-Eintrag; Symbolvarianten bei 100–200 % DPI | ☐ | ☐ |
| Ruhezeiten und Tageszusammenfassung: Markierung erscheint erst zur eingestellten Stunde und verschwindet nach dem Öffnen des Dashboards | ☐ | ☐ |
| Tray-Symbol und Menü bleiben nach Explorer-Neustart erhalten | ☐ | ☐ |
| Autostart an/aus: genau der Wert `mogumogu` unter `HKCU\…\Run`; nach Anmeldung startet nur der Tray-Besitzer | ☐ | ☐ |
| Suche (Strg+K): Treffer aus allen Bereichen, Enter öffnet den ersten, Esc schließt | ☐ | ☐ |
| Sicherung und Export aus den Einstellungen: Vorschau, Ordnerwahl, vorhandene Datei wird nicht überschrieben | ☐ | ☐ |
| Stimmungsposen (Mampft, Schläft, Besorgt) bei 100–200 % DPI neben den v2-Posen vergleichen; in Fortschrittsleiste, Pausenkarte und Hinweis zum hängenden Experiment ohne sichtbare Papierkante | ☐ | ☐ |

## L. Verschlankung und Erkennung (20. Oktober 2026)

Offen; mit eigenem Datenverzeichnis und, für die letzten Punkte, mit echten
eigenen Projektordnern.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Start mit einer Datenbank einer früheren Version: Einstellungen bleiben, Scoop-/Chocolatey-/WinGet-Freigaben sind widerrufen, Aktivität nennt das | ☐ | ☐ |
| Früherer Starteintrag dieser Installation ist nach dem Start entfernt; ein fremder Eintrag gleichen Namens bleibt | ☐ | ☐ |
| Tray-Menü ohne Überwachung; Symbol nur „bereit“/„arbeitet“ | ☐ | ☐ |
| Kein Netzwerkzugriff während Start, Erfassung und Laufwerkssuche (Firewall-Protokoll) | ☐ | ☐ |
| Funde: eigene Projekte als „Projekt“ mit KI-Dateien und Lockfile; Monorepo-Pakete als Teilprojekt | ☐ | ☐ |
| Funde: ZIP-Downloads (`*-main`) und Projekte unter Downloads abgestuft | ☐ | ☐ |
| Scratchpads: `ConsoleApp…`, `pythonProject…`, Kopien und einzelne Skripte auf Desktop/Downloads | ☐ | ☐ |
| Kontextfilter: Vorschau stimmt, Schreiben ergänzt nur `permissions.deny` in `.claude/settings.local.json`, `.claude/settings.json` bleibt, `/status` zeigt „Project local settings“, Claude Code liest `node_modules`/`.env` danach nicht mehr mit seinen Datei-Werkzeugen | ☐ | ☐ |
| Kontextfilter für alle: Vorschau listet jedes Projekt mit Regelzahl und Ausgelassene mit Grund; nach dem Schreiben ist die Datei in Git-Projekten ohne Ignore-Eintrag als Hinweis genannt | ☐ | ☐ |
| KI-Zugriff für eigene Projekte: Einstufung passt zu den eigenen `.claude`-/`.codex`-/`.cursor`-Einstellungen; keine Pfade im Dialog | ☐ | ☐ |
| Aufräumbericht für eigene Projekte: Wiederherstellbares entspricht Build-/Abhängigkeitsordnern, Testdaten unter „Prüfen“, nichts wurde verändert | ☐ | ☐ |
| Alle übernehmen: Scratchpads geschützt unter Ressourcen, Prüftermin 30 Tage, Besitzer nach Pfad | ☐ | ☐ |
| Ablageorte: Claude-Code-Arbeitsordner und JetBrains-Scratches über „Lesen erlauben“ durchsucht | ☐ | ☐ |
| Start mit einer Datenbank mit Sessions oder offenem altem Plan: Sicherung vor Schema 5, alter Plan „Ungültig geworden“, Scratchpads bleiben | ☐ | ☐ |
| Papierkorb-Plan für ein eigenes Projekt: nur Wiederherstellbares, Beleg je Ordner, Blocker „Gesperrt bis zum unabhängigen Review“, Bestätigen gesperrt | ☐ | ☐ |
| Papierkorb-Plan in einer Wegwerf-Testwurzel: Ordner liegen danach im Windows-Papierkorb und lassen sich dort wiederherstellen; Quellen und Lockfile bleiben | ☐ | ☐ |
| Zu großer Ordner für den Papierkorb: Windows fragt vor dem endgültigen Löschen; „Nein“ lässt den Ordner am Ort und meldet „nicht verschoben“ | ☐ | ☐ |
| Kontextfilter mit Entwicklermodus: Symlink an `.claude/settings.local.json` und an `.claude` wird abgelehnt; schnelles Ersetzen von `.claude` durch eine Junction während des Schreibens trifft nichts außerhalb | ☐ | ☐ |
| Echtes Claude Code nach dem Kontextfilter: `.env` und `.env.example` gesperrt, `/node_modules/**` greift bei Start in der Projektwurzel; ein „Nicht mehr fragen“ während des Schreibens geht nicht verloren (Vorschau neu nötig) | ☐ | ☐ |
| Laufwerkssuche als Administrator auf einem Rechner mit mehreren Benutzern: fremde Profile erscheinen nicht; OneDrive-Ordner werden durchsucht (keine Reparse Points); externe USB-Platte als „fest“ läuft nicht unnötig an | ☐ | ☐ |
| Paket-Caches: Systembefund zeigt die vorhandenen Caches mit maskiertem Pfad; nach „Lesen erlauben“ Größe im Aufräumbericht mit Aufräumbefehl; nichts im Cache verändert | ☐ | ☐ |
| Ordner in Benutzung (z. B. laufender Dev-Server in `node_modules`): Verschieben scheitert ohne Dialog, Plan meldet den Ordner als nicht verschoben, und im Papierkorb liegen keine einzelnen Teile daraus | ☐ | ☐ |
| Kein stilles endgültiges Löschen (in einer VM mit Wegwerf-Papierkorb): SUBST-Laufwerk, Richtlinie `NoRecycleFiles`, `NukeOnDelete`, Papierkorbgröße 0 oder zu klein – jeweils Nachfrage oder Ablehnung | ☐ | ☐ |
| Gepinnter Elternordner auf ReFS und auf exFAT/FAT: Umbenennen von Weg und Elternordner wird verweigert, sonst Plan dort sperren | ☐ | ☐ |

## K. Laufwerkssuche und Wegfall der Experimente-Seite (19. Oktober 2026)

Offen; braucht ein echtes Laufwerk und damit echte Benutzerordner. Vorher den
zweiten Review der Lesegrenze abschließen.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Speicher › „Projekte suchen“ auf einer lokalen Festplatte: Freigabe „Laufwerk X:“ unter Lesefreigaben, Ergebnisleiste mit „Funde ansehen“ | ☐ | ☐ |
| Durchgang auf dem Systemlaufwerk vollständig innerhalb des Budgets; Notiz nennt ausgelassene Ordner | ☐ | ☐ |
| Keine Funde aus Windows, Programme, ProgramData, AppData, Punktordnern im Profil, Spielebibliotheken | ☐ | ☐ |
| Bekannte eigene Projekte und Scratchpads erscheinen unter Projekte › Funde vorn | ☐ | ☐ |
| Netzlaufwerk und Wechselmedium bieten kein „Projekte suchen“ | ☐ | ☐ |
| Abbrechen während der Laufwerkssuche: teilweise, kein Datenverlust, Karte meldet „Letzte Suche unvollständig“ | ☐ | ☐ |
| Freigabe widerrufen: Karte zeigt wieder „Projekte suchen“, Funde verschwinden | ☐ | ☐ |
| Hängende Session – entfällt, Sessions am 20.10.2026 entfernt | – | – |
| Seitenleiste mit sechs Einträgen; `Alt+1` … `Alt+6`; Tray „Einstellungen …“ öffnet Einstellungen | ☐ | ☐ |

## J. Zusammengeführte Oberfläche im echten Fenster (18. Oktober 2026)

Windows 10 Pro 22H2, 200 % DPI, Debug-Build mit eigenem Datenverzeichnis
(`--data-dir`) und synthetischen Testordnern; kein echter Benutzerordner wurde
freigegeben. Bedient über UI Automation (Invoke, Toggle, Value) und an das
Fenster gesendete Nachrichten, Bilder per `PrintWindow`. Das ersetzt keine
Bedienung mit Maus und Narrator durch eine Person.

| Prüfpunkt | W10 | W11 |
|---|---|---|
| Einstieg: Ordnerdialog, Lesefreigabe, Erfassung, Ergebnis „Vergleichsbasis erstellt“ | ☑ | ☐ |
| Ordnerdialog gehört dem Dashboard, auch wenn mogumogu nicht im Vordergrund ist; nur das Dashboard ist währenddessen gesperrt | ☑ | ☐ |
| Anzeigename folgt einer erneuten Ordnerwahl | ☑ | ☐ |
| Ein selbst eingegebener Anzeigename bleibt bei erneuter Ordnerwahl | ☐ | ☐ |
| Projekte › Funde › „Suchordner hinzufügen …“: Erfassung, Ergebnisleiste, Projektwurzel benannt, kein Ordner doppelt | ☑ | ☐ |
| Pakete › Werkzeuge & globale Bestände › „Ort hinzufügen …“ › Python-Umgebung: Erfassung, Paketzahl steigt | ☑ | ☐ |
| Einstellungen › Daten: redigierter Export (Inhalt ohne Pfade und Projektnamen) und Sicherung, jeweils mit Ergebnisleiste | ☑ | ☐ |
| Einstellungen › Lesefreigaben: Ordnername als Titel, Detaildialog, Freigabe widerrufen | ☑ | ☐ |
| Tray-Menü › „Einstellungen …“ öffnet die Seite Einstellungen | ☑ | ☐ |
| Fenster schließen: Prozess bleibt im Tray; Tray-Klick öffnet ein neues Fenster mit Barrierefreiheitsbaum | ☑ | ☐ |
| Navigation über UI Automation 10 von 10 wirksam; Knöpfe, Felder und Dialoge tragen lesbare Namen | ☑ | ☐ |
| Narrator liest Navigation, Kennzahlen, Reiter und Dialoge | ☐ | ☐ |
| Tray-Menüeinträge mit Bildschirmleser | ☐ | ☐ |
