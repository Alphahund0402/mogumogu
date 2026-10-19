# Layoutprüfung – 18. Oktober 2026

Dieser Bericht dokumentiert den früheren Layoutstand. Die anschließende
Verdichtung aller nativen Informationsansichten und aktuelle Messwerte stehen in
[UI-UX-PRUEFUNG.md](UI-UX-PRUEFUNG.md) und [VALIDIERUNG.md](VALIDIERUNG.md).

Geprüft werden die native Slint-Oberfläche und die davon unabhängige HTML-Designvorschau.
Die Messungen verwenden ausschließlich synthetische Daten. Vorhandene lokale Inventare
werden weder geöffnet noch verändert.

## Ergebnis

Die finale Messung findet in den geprüften Zuständen **keine Layout-Prüfkandidaten**.
Die abschließende Sichtprüfung bestätigt insbesondere vollständig erreichbare lange
Dialoginhalte, saubere Scroll-Enden, lesbare Leerzustände und getrennte Aktionsbereiche.

| Messung | Native Oberfläche | HTML-Designvorschau |
| --- | --- | --- |
| Geprüfte Zustände | 363 | 528 |
| Gemessene Komponenteninstanzen | 151.638 | 20.940 |
| Sichtbare Texte | 27.125 Textblöcke | 38.887 Textfragmente |
| Gemessene Geschwisterabstände | 99.095 | In den Komponentenmaßen enthalten |
| Automatische Layoutbefunde | 0 | 0 |

Der native Analyzer wurde mit `--strict` erfolgreich beendet. Zusätzlich bestanden
15 Validierungsprüfungen, `cargo fmt --all -- --check`,
`cargo clippy --locked --all-targets -- -D warnings` und alle 139 ausgeführten
Rust-Tests. Der separate, ausdrücklich ignorierte Performance-Test wurde dabei nicht
ausgeführt.

`build.ps1 -NoRun` hat Tests und Release-Kompilierung erfolgreich abgeschlossen;
das anschließende Kopieren in `dist/mogumogu` scheiterte an der laufenden EXE.
Das vollständige neue Paket liegt deshalb unter `dist/mogumogu-layout-20261018`.
Beide kopierten EXE-Dateien wurden per Dateihash mit den Release-Artefakten verglichen.
Die bereits laufende Instanz wurde nicht neu gestartet und zeigt weiterhin ihren
bisherigen Build.

## Behobene Layoutprobleme

| Bereich | Beobachtung | Korrektur |
| --- | --- | --- |
| Native Übersicht | Umgebrochene Hinweiskarten beanspruchten mehr Höhe als die folgende Projektkarte freiließ. | Explizite Rasterzeilen mit inhaltsabhängiger Höhe; zwei bzw. drei Spalten. |
| Native Übersicht | Zusammenfassungskarten schrumpften innerhalb ihrer 244-px-Spalte auf 59 px. | Explizite Kartenbreiten; Zusammenfassungen unter der Tabelle bei schmalen Fenstern. |
| Toolbar | Feste Suchfeldbreite konkurrierte mit Aktionen; ohne flexiblen Zwischenraum wuchsen Buttons auf breiten Fenstern unnötig. | Suchfeld mit 160 px Mindestbreite und 286 px Wunschbreite; flexibler Zwischenraum. |
| Tabellen | Zusätzliche Spalten, lange Namen und Statuschips überschritten den verfügbaren Platz. | Breitenabhängige Zusatzspalten, begrenzte Statuschips und Ellipsen; vollständige Inhalte bleiben im Detail bzw. zugänglichen Namen. |
| Einstellungen, System, Experimente | Mehrspaltige Inhalte und Aktionsleisten passten nicht in kompakte Fenster. | Früherer Wechsel auf eine Spalte; umbrechende Aktionsleisten; vertikales Scrollen. |
| Paketdetails | Liste und Detailspalte konkurrierten um Breite; mehrzeilige Belegketten und Hinweise reservierten zu wenig Höhe. | Kompakte Detailansicht über die volle Inhaltsbreite; Text bestimmt die Zeilen- und Hinweishöhe. |
| Dialoge und Einstieg | Längere Texte, Formularfehler und zusätzliche Aktionen konnten feste Bereiche verdrängen. | Scrollbare Inhalte, getrennte erreichbare Abschlussaktionen, umbrechende Aktionsgruppen. |
| HTML-Vorschau | Legende, Metriktexte und Schnellaktionen ragten bei mehreren Größen aus festen Kartenhöhen; Textüberschneidung bei 320 px. | Intrinsische Höhen, frühere Spaltenumbrüche und gemeinsame Abstandsregeln. |

## Abstände und Größen

Native Maße sind **logische Slint-Pixel**. Bei 150 % bzw. 200 % werden daraus entsprechend
mehr Bildpunkte. HTML-Maße sind CSS-Pixel. Es gibt keinen universellen vorgeschriebenen
Paddingwert; konsistente Gruppen, erkennbare Beziehungen und ausreichender Platz für
Text und Bedienziele sind ausschlaggebend.

| Native Komponente | Maß |
| --- | --- |
| Seitenleiste | 188 px breit |
| Seitenrand | 14 px links/rechts |
| Abstand zwischen Karten | 10 px |
| Standard-Seitenkarte | 12 px innen, 8 px Inhaltsabstand |
| Einstellungskarte | 14 px innen, 7 px Inhaltsabstand |
| Scrollreserve bei Einstellungen/System/Einstieg | 16 px rechts, 12 px unten |
| Paket-/Experimentdetails | 14 px links, 20 px rechts |
| Dialog | 18 px innen, 10 px Inhaltsabstand, mindestens 16 px zum Fenster |
| Einstiegskarten | Meist 20 px innen, 10–12 px Inhaltsabstand |
| Aktionsgruppen | 6–8 px Abstand |
| Buttons | 30 px hoch; kompakte Buttons 26 px |
| Navigation / Suchfeld | 32 px hoch |
| Navigation | 2 px Zeilenabstand, 10 px Seitenrand |
| Toolbar | 52 px hoch, 8 px Abstand zwischen Aktionen |
| Statusleiste | 28 px hoch |
| Hilfe-Schaltfläche | 24 px hoch |
| Projektzeile / Detailzeile | 36 / 50 px hoch |
| Experiment-Sessionzeile | Mindestens 48 px hoch |
| Statuschip | 22 px hoch, kein Bedienziel |

Die Messung bei 900 × 560 bestätigt für die Einstellungen eine 684 px breite Scrollfläche
und 668 px breite Karten: 16 px bleiben für den Scrollbereich. Aufeinanderfolgende
Karten besitzen 10 px Abstand. Im breiten Paketdetail belegen die langen Test-Belegschritte
jeweils 286 × 89 px: 10 px Marker, 9 px Abstand und eine 267 px breite Textspalte.
Lange Detaildialogzeilen beanspruchen jeweils 34 px Höhe und 8 px Zwischenraum;
die Beschriftungsspalte ist 140 px breit, gefolgt von 12 px Abstand. Der zugehörige
Scrollinhalt misst jetzt korrekt 575 statt zuvor 439 px und ist bis zum Ende erreichbar.

Die HTML-Vorschau verwendet 16 px Kartenabstand und Kartenpadding, 12 px zwischen Icon
und Text sowie 8 px zwischen Filtern. Außenpadding: 16 px auf kleinen, 24 px auf mittleren
und 32 px auf großen Ansichten. Buttons sind mindestens 32 px hoch, Navigation und
Schließen 44 px; bei Touch-Eingabe erhalten alle Buttons 44 px Mindesthöhe.

## Responsive Verhalten

Das native Fenster unterstützt mindestens **900 × 560 logische Pixel**. Innerhalb dieses
Rahmens bleiben lange Seiten vertikal scrollbar. Das entspricht bei 200 % Skalierung
1800 × 1120 Bildpunkten; ein Desktop mit weniger logischer Fläche ist damit nicht abgedeckt.

| Bereich | Wechsel |
| --- | --- |
| Übersicht – Zusammenfassungen | Unter 1012 px Fensterbreite unterhalb der Projekte |
| Übersicht – Hinweiskarten | Unter 1200 px Fensterbreite zwei Spalten; dritte Karte in eigener Zeile |
| Einstellungen | Unter 1120 px Inhaltsbreite eine Spalte |
| System | Unter 1000 px Inhaltsbreite eine Spalte |
| Paketdetails | Unter 1060 px Inhaltsbreite volle Detailansicht; Schließen führt zur Liste |
| Einstiegsbegrüßung | Unter 1120 px eigener Breite gestapelt |
| HTML-Metriken | Eine, zwei oder vier Spalten entsprechend der verfügbaren Breite |

Strukturabhängige native Breakpoints werden aus der Fensterbreite bestimmt. Sie dürfen
nicht von der intrinsischen Breite des gerade umgebauten Layouts abhängen: Slint 1.18.1
meldete hierfür nachvollziehbare Bindungszyklen. Die festen Seitenränder werden in den
Seiten-Breakpoints berücksichtigt.

## Messverfahren und Artefakte

`examples/layout_audit.rs` lädt dieselben kompilierten Slint-Komponenten wie die Anwendung,
rendert sie mit dem Software-Renderer und exportiert den tatsächlichen Item-Baum samt
berechneten Textmaßen. Das sind echte **Slint-Renderings ohne Windows-Fensterrahmen**,
keine HTML-Ersatzbilder. `scripts/analyze_layout.py` erzeugt daraus PNGs, CSV-Dateien und
Kandidaten für Überläufe, zu kleine Textflächen und Überschneidungen.

- `target/layout-audit/manifest.json`: tatsächlich gerenderte Zustände.
- `target/layout-audit/summary.json`: Messumfang und Kandidatenzahl.
- `target/layout-audit/component-bounds.csv`: Position, Größe und Abstand zum direkten
  Elternelement für jedes exportierte Element.
- `target/layout-audit/sibling-gaps.csv`: gemessene horizontale/vertikale Abstände zwischen
  Geschwisterelementen im Item-Baum.
- `target/layout-audit/text-metrics.csv`: tatsächliche Textflächen und benötigte Textmaße.
- `target/layout-audit/review-candidates.json`: gezielt visuell zu prüfende Stellen.
- `target/layout-audit/*.png`: native Renderbelege, benannt nach Größe, Skalierung und Zustand.
- `target/preview-layout/measurements.json`: HTML-Komponenten inklusive Bounds, Padding,
  Margin, Gap, Schriftgröße und Zeilenhöhe.

Abstände zum direkten Item-Elternknoten sind geometrische Insets, nicht automatisch
semantisches Kartenpadding. Textrechtecke beschreiben die vom Renderer geformten
Layoutflächen, keine einzelnen Glyphenmasken. Beabsichtigte Ellipsen und abgeschnittene
Zeilen an vertikalen Scrollgrenzen werden gesondert behandelt. Die automatische
Textanalyse erfasst Anzeige-Text; bei editierbaren Textfeldern misst sie den Viewport und
die minimale Zeilenhöhe. Der intern horizontal verschiebbare Eingabetext wird zusätzlich
visuell geprüft.

Die native Matrix umfasst 900 × 560, 1080 × 680 und 1440 × 900, zusätzlich 150 % und 200 %
Skalierung, zehn Seiten, Paketdetails, Adapter, beide Dialogarten, die fünf Einstiegszustände,
Suche, Erfassung, Leerzustände und Zwischenbreiten um Breakpoints. Gezielte Scroll-Enden
ergänzen die Anfangsansichten. Normale und lange Testdaten werden getrennt geprüft.

Die HTML-Matrix umfasst zwölf Größen von 320 × 568 bis 1920 × 1080, acht Seiten und drei
Dialoge mit Demo-, Langtext-, Leer- und 200-%-Textzuständen. Die Textvergrößerung ist ein
eigener Test und wird nicht als Browserzoom ausgegeben. Tabellen- und Codetexte sind in
der Kollisionsprüfung enthalten; beabsichtigtes horizontales Scrollen bleibt möglich.

## Wiederholen

```powershell
$env:SLINT_EMIT_DEBUG_INFO = '1'
cargo run --locked --example layout_audit
python scripts/analyze_layout.py target/layout-audit --png --strict
node scripts/check_preview_layout.mjs --screenshots
```

Python 3 und ein lokaler Chromium-Browser sind erforderlich. `CHROMIUM_PATH` kann den
Browserpfad vorgeben. Alle erzeugten Messdateien bleiben im ignorierten `target/`-Ordner.
Die native Inspektion benutzt bewusst die auf Slint 1.18.1 festgelegte interne API und
muss bei einem Slint-Update geprüft werden; sie gehört nicht zur produktiven Laufzeit.
`--strict` liefert bei gefundenen Prüfkandidaten einen Fehlerstatus, damit ein erneuter
Durchlauf nicht still trotz neuer Layoutprobleme als erfolgreich behandelt wird.

## Einordnung

Als Orientierung dienen die [Slint-Layoutdokumentation](https://docs.slint.dev/latest/docs/slint/guide/language/coding/positioning-and-layouts/),
[W3C Reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html) und
[W3C Target Size](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html).
Der 24-px-Mindestwert ist eine Orientierung für Bedienziele; daraus folgt keine vollständige
WCAG-Konformitätsaussage für die Desktop-Anwendung.

Die Prüfung ersetzt nicht die manuelle Windows-10/11-Abnahme aus `WINDOWS-ABNAHME.md`,
insbesondere echte Monitor-/DPI-Wechsel, Windows-Textvergrößerung, Tastaturbedienung und
Screenreader. Sie belegt die konkret ausgeführten Größen und Zustände, nicht beliebige
zukünftige Daten oder jeden möglichen Bedienablauf.
