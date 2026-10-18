# Validierung · mogumogu 0.1.0

Stand: 5. Oktober 2026. Alle Prüfungen liefen auf Windows 10 Pro 22H2 (Build 19045, x64) mit Rust 1.98.0 (MSVC). Was nicht ausgeführt wurde, steht ausdrücklich unten.

## Ergänzung vom 18. Oktober: Zusammenführung auf sieben Ansichten

Die Oberfläche wurde von elf auf sieben Ansichten zusammengeführt; Inventar und
Begründung stehen im [Oberflächeninventar](UI-UX-PRUEFUNG.md#zusammenführung-auf-sieben-ansichten--18-oktober-2026).
Geändert wurde nur die Darstellung in `ui/` und `src/desktop/`.

Der native Audit in `target/layout-audit-consolidation` umfasst **445 Zustände**
(sieben Seiten, alle Reiter, Leer- und Filterzustände, Dialoge, Einstieg,
Erfassung, Suche, 100/150/200 %, Breitenwechsel und Scroll-Enden) mit 162.942
Komponenteninstanzen und 27.934 sichtbaren Textblöcken.
`scripts/analyze_layout.py --strict` bestand mit **null Kandidaten**. Ein erster
Lauf fand 72 Kandidaten: Bei 900 × 560 schoben „Werkzeuge & globale Bestände“
und die Übersicht mit Ergebnisleiste die Statusleiste aus dem Fenster; die
Mindesthöhen wurden reduziert. Der Tastaturcheck des Audits bestand.

`cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
alle 15 statischen Verträge und 162 Funktionstests mit
`--no-default-features --features network` bestanden.
`design/screens/render.ps1` rendert wieder alle Ansichten und Reiter.

Nachtrag: Die Bedienung im echten Fenster (W10, 200 % DPI, eigenes
Datenverzeichnis, synthetische Ordner) ist inzwischen über UI Automation und an
das Fenster gesendete Nachrichten durchgeführt; Ergebnisse in Abschnitt J von
[WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md). Dabei behoben: Der Ordnerdialog nahm
`GetForegroundWindow()` als Besitzer und hing sich damit an ein fremdes Fenster,
wenn mogumogu nicht vorne war (jetzt das eigene Dashboard); ein automatisch
gesetzter Anzeigename folgte keiner neuen Ordnerwahl; in den Funden stand die
Projektwurzel als „Projekt · .“ und ein per Name gefundener Ordner zusätzlich
als erfasste Umgebung; Lesefreigaben schnitten den Ordnernamen am Pfadende ab;
der Exportdialog sagte „1 Projekte“. Narrator und Windows 11 bleiben offen.

## Ergänzung vom 18. Oktober: UI/UX und Informationsdarstellung

Alle elf nativen Seiten sowie Tabs, Details, Formulare, Einstieg und Statusanzeigen
wurden auf Informationshierarchie, Verdichtung und vollständige Erreichbarkeit
geprüft. Das [Oberflächeninventar](UI-UX-PRUEFUNG.md) nennt die einzelnen Änderungen.
Die erhöhten Listenabstände und das gleichmäßige Sprechblasen-Padding bleiben bestehen.

Der vollständige native Audit in `target/layout-audit-ui-ux-final` umfasst
**518 Zustände**, 206.839 Komponenteninstanzen, 36.061 sichtbare Textblöcke und
133.335 Geschwisterabstände. `scripts/analyze_layout.py --png --strict` bestand
mit **null Prüfkandidaten**. Enthalten sind 100/150/200-%-Skalierung, lange Texte,
Breitenwechsel, Scroll-Enden, unbekannte Größen, Leer- und Filterzustände sowie
per Pointer-Ereignis geöffnete Details. Der integrierte Tastaturcheck bestätigt
Projektwahl per Pfeiltaste und Tab zum Namenfeld.

Die abschließende Korrektur der Paketfilter wurde zusätzlich in 60 nativen
Zuständen unter `target/layout-audit-ui-ux-packages-final` geprüft: ebenfalls
null Kandidaten (22.806 Komponenten, 4.014 sichtbare Texte). Bei 900×560 beginnt
die Paketkarte nun bei y191 statt y223; der Abstand zur Filterleiste beträgt
14 px und die Liste gewinnt 32 px Höhe. `cargo fmt --all -- --check` und
`cargo clippy --locked --all-targets -- -D warnings` bestanden.

Die 162 Funktionstests mit `--no-default-features --features network --target
x86_64-pc-windows-msvc` und alle 15 statischen Verträge bestanden. Der gesonderte
Referenzlasttest wurde bei dieser Darstellungsänderung nicht erneut ausgeführt.
Ein anfänglicher Testaufruf ohne explizites Ziel traf die noch laufende Audit-EXE;
der oben genannte vollständige Lauf mit MSVC-Ziel bestand anschließend.

Der Analyzer berücksichtigt nun Slints transparente, nicht clippende
Visibility-Wrapper: Deren 0×0-Geometrie bildet keine Inhaltsgrenze; geprüft wird
die tatsächliche übergeordnete Layoutfläche. Echte Höhenfehler in Sessions,
Planordnern und aufgeklappten Laufwerksdetails wurden in den Komponenten behoben.

Native Beispielbilder: [KI-Liste](screenshots/ui-ux-ki.png),
[Pakete](screenshots/ui-ux-pakete.png),
[Speicher](screenshots/ui-ux-speicher.png),
[Einstellungen](screenshots/ui-ux-einstellungen.png). Es sind synthetische Daten.
Screenreader- und Windows-11-Abnahme bleiben offen.

Der abschließende Windows-Release wurde gebaut und mit `build.ps1 -NoRun
-SkipTests` nach `dist/mogumogu` kopiert; beide EXE-Dateien stimmen per Hash mit
den Release-Artefakten überein. Die lokale Instanz wurde nach erneuter Prüfung
auf null offene Sessions geordnet beendet und mit sichtbarem Dashboard neu gestartet.

## Ergänzung vom 18. Oktober: Entwicklungsfunde, Speicher, Skill-Inhalte

Die Windows-Testsuite (`cargo test --locked --no-default-features --features network`,
im nativen Build mit explizitem MSVC-Ziel) bestand mit 162 Tests und einem
ausdrücklich ignorierten Referenzlasttest: 88 Bibliotheks-, zwei CLI- und 72
Integrationstests. Der Referenzlasttest wurde anschließend separat im Release-Modus ausgeführt und bestand ebenfalls. Neu bzw. erweitert sind die Funde/Zuordnung (acht Tests),
Speicher/Ordnerstruktur (sieben) und Skill-Inhalte (sechs). Die 15 statischen
Verträge und Clippy mit `-D warnings` bestanden ebenfalls.

Die neuen Lesewege wurden zusätzlich durch einen jeweils anderen Agenten
geprüft. Dabei korrigiert: doppelte Projektpfade in Laufwerkssummen,
unbegrenztes Laden von Paketmetadaten für die Fundliste, Rekonstruktion
redigierter Pfade, Freigabewiderruf im Viewer und veraltete Ordnerlisten beim
Laufwerkswechsel. Dies ist kein menschlicher Sicherheitsnachweis.

Die native Slint-Softwaredarstellung wurde über `examples/layout_audit.rs`
in 54 Zuständen (900×560, 1080×680 und 1440×900, kurze/lange Texte,
Speicher/Ordnerstruktur/Skill-Viewer/Projektwähler) gerendert und mit
`scripts/analyze_layout.py` geometrisch geprüft: keine Kollisionskandidaten. Die
Ergebnisdateien liegen unter `target/layout-audit-development`. Sichtprüfungen
der echten Slint-Komponenten: [Speicher](screenshots/wolke-speicher.png),
[Ordnerstruktur](screenshots/wolke-ordnerstruktur.png),
[Skill-Inhalt](screenshots/wolke-skillinhalt.png). Diese Bilder verwenden nur
synthetische Daten und stammen nicht aus der HTML-Vorschau.

Ein zusätzlicher nativer Fokus-Probelauf bestätigte den Projektwähler beim
Zuordnen (Pfeiltaste wechselt Auswahl, Tab erreicht den Zweck) und den sichtbaren
ersten Eingabefokus bei normalen Formularen. Alle Sprechblasen verwenden einen
gemeinsamen Innenabstand von 14 px, getrennt von der gezeichneten Spitze.

Windows 11, Screenreader und vollständige interaktive Desktopabnahme bleiben
offen. Umfang und Grenzen: [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md).

## Ausgeführt und bestanden (ursprünglicher Prüfstand)

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

## Ergänzung: Kompaktes Windows-Layout (17. Oktober 2026)

`scripts/check.py --offline` bestand erneut auf Windows: statische Verträge,
Formatierung, Clippy, Kern mit und ohne Netzwerkfeature und Desktop-Binärtests.
Anschließend baute `build.ps1 -NoRun -SkipTests` beide nativen Release-EXEs mit
`--locked`; die Tests waren bereits im gemeinsamen Prüflauf bestanden.
Die Oberfläche wurde mit dem aktuellen Slint-1.18.1-Software-Renderer und den
echten Snapshot- und Dialogaufbereitungen geprüft: alle acht Bereiche in Demo-
und Lokalmodus bei 1080 × 680 und 900 × 560 Pixeln (Maßstab 1). Zusätzlich wurden
ein Ressourcendialog mit vier Aktionen, ein Formular mit erhaltenen Fehlerwerten,
der Beschäftigt-Zustand und eine Liste mit 20 Plänen gerendert und visuell geprüft.

Direkt eingespeiste Slint-Tastenereignisse bestätigten Strg+F, Strg+N mit
Demo-Sperre, F1, F5, Alt+1…8 aus einem Suchfeld, die Sperre bei offenem Dialog
und Fokus nach Esc sowie programmatischem Dialogschluss. Alle Schließwege
verwenden nun dieselbe Funktion zur Fokuswiederherstellung.

Die [aktuelle Abbildung](screenshots/dashboard-compact-render.png) ist ein Render
der nativen Slint-Oberfläche, ohne Systemfensterrahmen. Der Windows-Automationshelfer
lieferte das gestartete Vorschaufenster nicht als bedienbares Ziel. Deshalb sind
diese Renders und Tastenereignisse **keine interaktive Windows-Abnahme**; neue
manuelle Punkte stehen in [WINDOWS-ABNAHME.md, Abschnitt F](WINDOWS-ABNAHME.md#f-kompaktes-fenster-17-oktober-2026).
Frühere Screenshots bleiben historische Belege. DPI, Narrator und Windows 11
wurden für dieses Layout nicht abgenommen.

## Paketbelege (17. Oktober 2026)

`scripts/check.py --offline` prüft den erweiterten Kern mit/ohne Netzwerk,
Clippy, Format, statische Verträge und den nativen Test-Build. Neue Fixtures:

- `tests/package_evidence.rs`: parallele Versionen/Quellen, private Quellen,
  Backoff, erhaltene Warnungen bei Offlinebetrieb, Cache-/Befehlsreferenzen,
  Behalten ohne Warnungsunterdrückung, zeitabhängige Neubewertung sowie
  Migration v2→v3 mit konsistenter Sicherung und Invalidierung des alten Caches.
- `tests/package_scopes.rs`: Dateinamensuche ohne Inhaltslesen, getrennte
  Registrierung/Freigabe, Junctions, ersetzte Wurzel, RECORD-Pfade und `.pth`
  nicht verfolgt/ausgeführt; globale npm-/Cargo-Belege.
- Parserfixtures: genaue Versionsordnung, npm-Abkündigung, Python-Projektstatus
  und Release-Yanks, Cargo-Yanks, NuGet-Alternative, minimale CI-Befehlsnamen,
  unbekannte Installationsformate ergeben keinen leeren Erfolg.

Die tatsächlich kompilierte Slint-Oberfläche wurde mit synthetischen Daten
gerendert: alle acht Bereiche in Demo/Lokal bei beiden Fenstergrößen, dazu
Adapter, Paketdetails und leere Projektsuche; die bisherigen Tastatur-/Modal-
Prüfungen bestanden. [Paketansicht mit Testdaten](screenshots/package-review-render.png)
ist ein Software-Renderer-Bild, kein Screenshot eines OS-Fensters. Neue manuelle
Bedienpunkte stehen in [WINDOWS-ABNAHME, Abschnitt G](WINDOWS-ABNAHME.md#g-paketbelege-und-globale-bereiche-17-oktober-2026).

Zusätzlicher isolierter CLI-/IPC-Durchlauf: Dateinamensuche, Kandidatenregistrierung,
separate Inhaltsfreigabe, globale npm-Erfassung, zwei Versionen desselben Pakets
und Behalten aus Windows PowerShell bestanden; Netzwerk blieb aus. Die
PowerShell-Schlüssel sind jetzt undurchsichtige IDs, keine zu quotierenden JSON-Objekte.

Ein Capture-Test mit einem absichtlich langlebigen Headless-Testbesitzer deckte
geerbte Windows-Pipehandles beim CLI-Autostart auf. Die eigene Besitzer-EXE wird
jetzt über `CreateProcessW` mit explizitem Programmpfad und ohne Handle-Vererbung
gestartet. Nach der Korrektur kehrte erfasstes JSON in 623 ms zurück, während der
Besitzer weiterlief; ein Datenpfad mit Leerzeichen blieb erhalten und Shutdown
beendete den Testbesitzer. Dies ist ein Prozess-/IPC-Test, keine interaktive
Tray-Abnahme. Der normale GUI-Tray ließ sich in dieser Toolsitzung nicht zuverlässig
abnehmen; die gestarteten isolierten Testprozesse wurden beendet.

## Ergänzung: Architektur und Statusabfragen (17. Oktober 2026)

`scripts/check.py --offline` bestand nach dem Architekturumbau: 15 statische
Verträge, Formatierung, Clippy, Kern mit und ohne Netzwerkfeature sowie native
UI-Binärtests. `cargo rustdoc --locked --offline --no-default-features --lib --
-D warnings` erzeugte die Bibliotheksdokumentation ohne Warnungen. Lokale
Quellverweise der [Architekturkarte](ARCHITEKTUR.md) wurden aufgelöst.
`build.ps1 -NoRun -SkipTests` baute anschließend beide nativen Release-EXEs;
die Tests waren zuvor über den gemeinsamen Runner vollständig ausgeführt.
Der abschließende CLI-/IPC-Durchlauf mit diesen EXEs bestand in einer neuen,
isolierten Testdatenbank: Statusgesamtzahlen, Projektsuche, Registrierung,
separate Lesefreigabe, Scans, zwei Paketversionen und Behalten. Netzwerk blieb
aus; der Headless-Testbesitzer wurde danach beendet.

Die Paketprüfung behält ihre öffentliche `Core`-Schnittstelle. Datenzugriff und
Cache liegen im Service-Modul, Referenzindex und Berichtberechnung in privaten
Auswertungsmodulen ohne Kern-, Repository- oder Netzwerkclient. Die bestehenden
Paket-/Scope-Fixtures bestanden. Die Lese- und Löschimplementierungen sowie
Migrationen wurden bei diesem Umbau nicht geändert.

Der neue Statusvertrag in `tests/core.rs` legt alle sechs Sessionzustände und
200 neuere abgeschlossene Sessions an. Der Dashboard-Ausschnitt enthält nur
abgeschlossene Sessions; die Statusabfrage zählt trotzdem die vier älteren
weiterhin sperrenden Sessions. Der Sessionabgleich bleibt vor der Zählung aktiv.
Die Referenzlast misst Status und Snapshot getrennt; Werte im
[Messbericht](MESSBERICHT.md).

Ein früher Gesamtlauf meldete im bestehenden Test
`crash_after_a_completed_batch_reports_partial_success` einmal 100 übrige Dateien
statt eines Teilerfolgs. Der isolierte Test, acht Wiederholungen des vollständigen
Kernvertragstests und die anschließenden Gesamtläufe bestanden. Die Ursache
dieses intermittierenden Fehlers ist nicht bestätigt; die Assertion wurde nicht
abgeschwächt und der Löschpfad nicht geändert. G5 bleibt offen.

## Mochi-Identität und japanische Schrift (18. Oktober 2026)

Die ausgewählte Mochi-Tafel wurde über feste Ausschnitte übernommen; der Export
prüft Quellhash und Pixelgleichheit. Fenster und Sidebar verwenden das Kopf-Icon,
der Tray dessen 64-px-Ableitung. Die gezeichneten Buchstaben bleiben als Wortmarke
erhalten. Für die Bedienelemente sind Zen Maru Gothic Regular/Bold mit vollständigem
Zeichenvorrat eingebettet, inklusive fester Herkunft, Hashes und OFL-Lizenz.

`scripts/check.py --offline` bestand: 15 statische Verträge, Format, Clippy mit
`-D warnings`, Kern mit/ohne Netzwerk sowie Desktop-Binärtest. Die Font-Allowlist
erlaubt nur die beiden dokumentierten Originalfonts und prüft ihre Hashes und
die unveränderte Lizenz. Eine zusätzliche Prüfung der Unicode-Tabellen fand
alle geprüften deutschen Zeichen sowie Hiragana, Katakana und Kanji in beiden
Gewichten.

42 Frames der tatsächlich kompilierten Slint-Oberfläche wurden mit synthetischen
Daten gerendert: alle acht Bereiche in Demo/Lokal bei 1080 × 680 und 900 × 560,
ergänzt um Dialog-, Formular- und Paketansichten. Die bestehenden Tastatur- und
Modalprüfungen bestanden. Visuell geprüft wurden die Übersicht, die schmale
Einstellungsseite und ein japanischer Projektname. Belege:
[Mochi-Übersicht](screenshots/dashboard-mochi-render.png) und
[japanischer Projektname bei Mindestgröße](screenshots/japanese-project-render.png).
Die Bilder stammen vom nativen Software-Renderer und haben keinen OS-Fensterrahmen.
Interaktive Windows-/IME- und DPI-Abnahme bleibt separat in
[WINDOWS-ABNAHME, Abschnitt H](WINDOWS-ABNAHME.md#h-mochi-und-japanische-schrift-18-oktober-2026).

`build.ps1 -NoRun -SkipTests` baute anschließend beide nativen Release-EXEs;
die Tests waren bereits über den gemeinsamen Runner ausgeführt. Eine Prüfung
der gepackten `dist/mogumogu/mogumogu.exe` bestätigte alle neun ICO-Frames
bytegleich im Windows-Ressourcenverzeichnis sowie die drei Original-PNG-Exporte
und beide vollständigen TTF-Dateien im Binary. Die beigelegte
`FONT-LICENSE.txt` stimmt per SHA-256 mit der Original-OFL-Datei überein.

## Mochi-Ausdrücke je Ansicht (18. Oktober 2026)

Die drei kleinen Ausdruckszeichnungen wurden als pixelgleiche Ausschnitte der
unveränderten Originaltafel exportiert. `Mascot` in `ui/components.slint` bindet
den Seitenindex an die Pose: Übersicht fröhlich, Projekte/Ressourcen mit Ordner,
Pakete/KI/Prüfliste konzentriert sowie Aktivität/Einstellungen neutral. Die
Layoutgrenzen und die dekorative Accessibility-Rolle bleiben gleich.

`scripts/check.py --offline` bestand vollständig: 15 statische Verträge, Format,
Clippy, Kern mit/ohne Netzwerk und Desktop-Binärtest. Mit dem neu kompilierten
Slint-Layout wurden erneut 42 native Frames für Demo/Lokal und beide Fenstergrößen
erzeugt; Tastatur- und Modalprüfungen bestanden. Die Übersicht, Paketansicht und
Einstellungsseite wurden visuell geprüft. Belege: [fröhlich](screenshots/dashboard-mochi-render.png),
[konzentriert](screenshots/mascot-focused-render.png), [neutral](screenshots/mascot-neutral-render.png).
Dies sind Ausgaben des nativen Software-Renderers, keine OS-Fensteraufnahmen.

Der native Release-Build mit `cargo build --locked --offline --release --bins
--target x86_64-pc-windows-msvc` bestand. Wegen der noch laufenden bisherigen
App wurde die neue Fassung mit CLI und Lizenzen unter `dist/mogumogu-expressions`
bereitgestellt. Die Prüfung der fertigen EXE bestätigte die sechs PNG-Assets,
beide vollständigen Fonts und alle neun Windows-Iconframes bytegleich; die
beigelegte Fontlizenz entspricht dem Original. Zum Verwenden wird die bisherige
Instanz regulär im Tray beendet und anschließend die neue EXE gestartet.

## Vereinheitlichte Mochi-Zeichnungen v2 (18. Oktober 2026)

Die vier bisherigen Posen wurden mit dem eingebauten `image_gen`-Werkzeug in
einem gemeinsamen 2×2-Zeichensatz angeglichen. Der vollständige
[Prompt](../design/brand/consistent-v2-prompt.md) und die unveränderte PNG-Ausgabe
bleiben nachvollziehbar; die ursprünglichen Einzelzeichnungen sind archiviert.
Icon, Wortmarke und Fontdateien stammen weiterhin aus denselben Quellen.

Die Anwendung verwendet `assets/brand/mascots-v2.png` (1254 × 1254 Pixel).
`Mascot` wählt native Bildausschnitte von 577 × 577 Pixeln aus den vier
627-Pixel-Zellen, jeweils mit demselben 25-Pixel-Innenrand. Damit sind Maßstab
und Konturbehandlung bei der Anzeige vergleichbar; die Seitenzuordnung bleibt
im Darstellungscode.

`scripts/check.py --offline` bestand vollständig: 15 statische Verträge,
Formatierung, Clippy, Kern mit/ohne Netzwerk und Desktop-Binärtest. 42 Frames
des tatsächlich kompilierten Slint-Layouts wurden erneut in Demo/Lokal und bei
1080 × 680 sowie 900 × 560 gerendert; Tastatur- und Modalprüfungen bestanden.
Alle vier Posen wurden visuell geprüft: [fröhlich](screenshots/dashboard-mochi-render.png),
[mit Ordner](screenshots/mascot-folder-render.png), [konzentriert](screenshots/mascot-focused-render.png),
[neutral](screenshots/mascot-neutral-render.png). Die Bilder stammen vom nativen
Software-Renderer; eine interaktive OS-/DPI-Abnahme ist separat dokumentiert.

`build.ps1 -NoRun -SkipTests` baute nach dem vollständigen Prüflauf beide nativen
Release-EXEs und stellte sie regulär unter `dist/mogumogu` bereit. Die fertige
EXE enthält den unveränderten v2-Bogen, die bisherigen Icon-/Wortmarkendateien,
Tray und beide vollständigen Fonts. Alle neun Windows-Iconframes sind bytegleich
zum bisherigen ICO; die vier archivierten v1-Posen werden nicht zusätzlich in
die EXE eingebettet. Die beigelegte OFL-Datei wurde per SHA-256 abgeglichen.

## Gemessene Mochi-Konturen (18. Oktober 2026)

Die [Messung im nativen Renderer](../design/brand/measurement/README.md) umfasst
alle acht Ansichten, zwei Fenstergrößen und fünf DPI-Stufen (100/125/150/200/400 %).
Alle 80 Posenvergleiche sind bei derselben DPI-Stufe pixelgleich über ihre
zugehörigen Ansichten und beide Fenstergrößen. Acht 100-%-Ausschnitte stimmen
zusätzlich bytegleich mit den bereits geprüften nativen Demo-Frames überein.

Je 43 korrespondierende Kopf-/Ohrkonturen wurden quer zum Strich als
Halbwertsbreite gemessen. Mediane bei 400 %, zurückgerechnet auf logische Pixel:
Neutral 1,69, Fröhlich 1,73, Ordner 1,62, Laptop 1,71. Der größte Abstand beträgt
0,107 px bzw. 6,6 % relativ zur dünnsten Pose; eine exakt konstante Strichstärke
wird nicht behauptet. Native Kalibrierlinien von 0,75/1/1,5/2 px werden bei
400 % auf 0,01 px genau zurückgemessen. Rohwerte, Quellhashes und Messstellen
liegen beim Bericht. Die interaktive Windows-/Monitor-Abnahme bleibt separat.

Die fertige Release-EXE unter `dist/mogumogu` wurde erneut geprüft: v2-Bogen,
Original-Icon/-Wortmarke, Tray und beide japanischen Fonts sind eingebettet;
alle neun Windows-Iconframes sind bytegleich. Die archivierten v1-Posen sind
nicht zusätzlich in der EXE enthalten.

## Systembefund, Einstieg, Experimente, Tray und Mochi-Wolke (18. Oktober 2026)

`python scripts/check.py --offline` bestand vollständig: `validate.py` (15
Verträge), Formatierung, Clippy mit `-D warnings`, Tests mit und ohne
Netzwerkfeature (83 bzw. 80 Kern-Unit-Tests plus Integrationstests) und der
Desktop-Binärtest. Neue Tests:

| Test | Prüft |
|---|---|
| `inventory::system` (2 Unit-Tests, nur Windows) | Befehle, Shims, Bestände und KI-Ordner aus einer synthetischen Umgebung; relative Suchpfade übersprungen; kein Benutzername in Orten; Zuordnung von Ordnern zu Katalogpräfixen |
| `user_ai_folder_is_read_only_along_catalog_routes` | Persönlicher KI-Ordner: nur Katalogrouten betreten, Verläufe und Zugangsdateien nie gelesen |
| `user_folders_are_entered_only_along_catalog_routes` | Routenberechnung aus dem Katalog, ein Platzhalter je Segment |
| `v3_scopes_keep_their_meaning_and_user_folders_get_their_own_realm` | Migration 004 ändert bestehende Freigaben nicht; Benutzerordner eigener Bereich |
| `stuck_sessions_are_named_and_released_only_without_live_processes` | Hängende Sessions benannt; Freigabe gesperrt bei Aktivität, lebendem Prozess oder „läuft sehr lange“ |
| `search_spans_areas_and_needs_two_characters` | Suche über Bereiche, `LIKE`-Sonderzeichen maskiert |
| `export_writes_a_new_redacted_file_and_never_overwrites` | Export in neue Datei, vorhandenes Ziel ergibt Konflikt |
| `new_settings_are_validated_and_a_pause_keeps_the_choice` | Ruhezeiten, Zusammenfassungsstunde, Pause; alte Einstellungen behalten Standardwerte |
| Domänentests in `session.rs`, `settings.rs`, `clock.rs` | Hängen-Klassen, Ruhezeiten über Mitternacht, lokale Stunden aus dem Versatz |

Auf dem Entwicklungsrechner (W10) lief der Systembefund in 9,7 ms. Seine Funde
entsprachen der Beobachtung in [SYSTEMERKENNUNG](SYSTEMERKENNUNG.md), darunter der
pyenv-Shim ohne globale Version und das ältere uv-Tool-Layout.

Native Renderings über `design/screens/render.ps1` mit synthetischen Daten wurden
visuell geprüft: alle zehn Ansichten, Paket-Erklärpanel, hängendes Experiment,
Suche, laufende Erfassung, Pause, alle Einstiegsschritte, 900 × 560 und 150 %.
Dabei behoben: abgeschnittene Knöpfe in Hinweiskarten, zu niedriges Suchfenster,
überlaufende Chips und Tabellenspalten bei 900 × 560 und sichtbare
Papierkanten der Figur auf weißen Karten. Belege:
[Übersicht](screenshots/wolke-uebersicht.png),
[900 × 560](screenshots/wolke-uebersicht-900x560.png),
[150 %](screenshots/wolke-uebersicht-150.png),
[Pakete](screenshots/wolke-pakete.png),
[hängendes Experiment](screenshots/wolke-experiment-haengt.png),
[System](screenshots/wolke-system.png), [Suche](screenshots/wolke-suche.png),
[Erfassung](screenshots/wolke-erfassung.png), [Pause](screenshots/wolke-pausiert.png),
[Einstieg](screenshots/wolke-einstieg.png),
[Lesefreigabe im Einstieg](screenshots/wolke-einstieg-freigabe.png).

Die Stimmungsposen erscheinen zusätzlich neben den zugehörigen Entscheidungen:
besorgt im Detail eines hängenden Experiments, schlafend auf der Karte
„Überwachung“ während der Pause, mampfend (40 px) in der Fortschrittsleiste.
Der Bogen ist vollständig und byte-identisch in `mogumogu.exe` eingebettet
(Slint `EmbedFiles`, per Bytevergleich geprüft).

Der Stimmungsbogen ist deterministisch: Zwei Läufe von
`design/brand/moods/derive.py` ergaben denselben SHA-256; der v2-Bogen blieb
unverändert (`a58212b3…`).

Einmal beobachtet: `windows_cleanup::crash_after_journal_commit_is_reconciled_not_repeated`
schlug in einem Gesamtlauf fehl (`plans_recovery` ≠ 1), danach in zehn weiteren
Läufen (einzeln und im Paket) nicht mehr. Bereinigungscode und Test sind in dieser
Phase unverändert. Ursache noch offen; vermutlich wurde ein Plan wegen
gleichzeitiger Dateizugriffe vor der Ausführung abgebrochen.

## Nicht ausgeführt

| Bereich | Grund |
|---|---|
| Bedienung von Systembefund, Einstieg, Tray-Pause, Autostart und Ordnerdialog im echten Fenster | Manuelle Abnahme ausstehend ([WINDOWS-ABNAHME](WINDOWS-ABNAHME.md), Abschnitt I) |
| Zweiter Review der erweiterten Lesegrenze (Systembefund, `user-ai`) | Nach AGENTS.md erforderlich; erfordert eine zweite Person |
| Linux-Kernprüfung im Container für diese Phase | Nicht ausgeführt; plattformabhängige neue Tests sind auf Windows beschränkt, die Stubs in `platform/fallback.rs` wurden nur per Abgleich der Signaturen geprüft |
| Windows 11 | Kein Windows-11-System verfügbar |
| Tray-Klick und Tray-Menü per Maus | Nicht automatisiert; Menüaktionen sind über dieselben Anwendungsfälle per CLI getestet |
| DPI 125–200 %, Mehrmonitor, Explorer-Neustart, Schlafmodus | Manuelle Abnahme ausstehend; Slint meldet das Tray-Symbol nach `TaskbarCreated` selbst neu an |
| Screenreader (Narrator) | Accessibility-Rollen und -Beschriftungen gesetzt, nicht mit Narrator abgenommen |
| Reale Toolversionen der Paketmanager | Fixtures statt installierter npm/pnpm/uv/NuGet/Scoop/Chocolatey-Bestände (außer Cargo dieses Repos) |
| Updatehinweise gegen echte Registries | Netzwerkpfad ist implementiert, aber standardmäßig aus; in Tests ohne Netz geprüft |
| GitHub-Windows-CI | Workflow eingerichtet, erster Lauf nach dem Push |
| Unabhängiger Review der Löschpfade (G5) | Erfordert eine zweite Person |
