# Verbindliche Gestaltung: Mochi

Am 18. Oktober 2026 wurde **A / Mochi** aus der Manga-Runde ausgewählt.
Kopf-Icon und handgeschriebener mogumogu-Schriftzug stammen weiterhin direkt
aus dem Original. Die vier Maskottchenposen wurden anschließend auf Wunsch
des Auftraggebers in Strichstärke, Farbe und Maßstab angeglichen.
[Asset- und Schriftvorschau](index.html).

## Aktueller Zeichensatz v2

Die Anwendung verwendet den gemeinsamen [Zeichensatz](../../assets/brand/mascots-v2.png):
1254 × 1254 Pixel, vier gleich große Zellen mit je 627 × 627 Pixeln. Alle Posen
wurden in einem Bilddurchlauf mit denselben Konturen, Körperproportionen,
Wangenfarben und Papierflächen überarbeitet. Die Vorlage wurde unverändert
vom Generator übernommen; die Anwendung wählt daraus lediglich einen Bildausschnitt.

| Position | Pose | Ansicht |
|---|---|---|
| Oben links | Neutral | Aktivität, Einstellungen |
| Oben rechts | Fröhlich | Übersicht |
| Unten links | Mit Ordner | Projekte, Ressourcen |
| Unten rechts | Am Laptop | Pakete, KI-Konfiguration, Prüfliste |

Die vier bisherigen Einzelzeichnungen dienten als Edit-Vorlagen. Der vollständige
[Prompt und Generierungsweg](consistent-v2-prompt.md) sind dokumentiert. Verwendet
wurde das eingebaute `image_gen`-Werkzeug, das keine Modellkennung zurückliefert.
SHA-256 des unveränderten Ergebnisses:
`a58212b3515793ce166ae9a6a82add08b01dc99c940569b059966182244eee1b`.

## Stimmungsposen v3

Drei weitere Posen zeigen Zustände, die bisher nur als Text sichtbar waren. Sie
liegen im eigenen [Bogen](../../assets/brand/mascots-v3-moods.png) mit derselben
Geometrie (1254 × 1254 Pixel, 2 × 2 Zellen zu 627 Pixeln, vier Prozent Innenrand);
die vierte Zelle ist leeres Papier.

| Position | Pose | Zustand in der Anwendung | Abgeleitet aus |
|---|---|---|---|
| Oben links | Mampft (Onigiri) | Seitenleiste während einer Erfassung, Fortschrittsleiste, Erfassungsschritt des Einstiegs | Fröhlich |
| Oben rechts | Schläft | Seitenleiste bei Pause, Karte „Überwachung“ in den Einstellungen | Neutral |
| Unten links | Besorgt | Seitenleiste bei hängendem Experiment oder Teilerfassung, Hinweis im Detail des hängenden Experiments | Neutral |

Neben Entscheidungen erscheint Mochi in `MochiCallout` (`ui/components.slint`):
Figur auf einer runden Cremefläche, daneben Titel und Text, die den Zustand
vollständig nennen. Die Figur ist dekorativ und kein Fokusziel. Weil der
Software-Renderer Bilder nicht an runden Ecken beschneidet, liegt das Papier der
Zeichnung mit etwas Rand auf der Fläche (`MascotPlate`). Damit Gesichter und
Zeichen lesbar bleiben, sind die Figuren dort 60–68 px groß, in der
Fortschrittsleiste 40 px.

**Herkunft:** Keine Neugenerierung. Die Posen sind eine deterministische
Pixelbearbeitung des unveränderten v2-Bogens mit
[derive.py](moods/derive.py) (feste Zufallssaat, numpy/scipy/OpenCV/Pillow,
nicht Teil des Builds):

- Körper, Fell, Ohren, Wangen, Schnurrhaare, Schwanz und Bodenschatten bleiben
  Originalpixel derselben Zelle. Entfernte Gesichts- oder Pfotenstriche werden
  mit Fell aus derselben Zelle überstempelt.
- Wo möglich, werden vorhandene Striche versetzt: Der geschlossene Mund von
  „Neutral“ ersetzt den offenen Mund beim Mampfen. Die gespiegelten Augenbögen
  von „Fröhlich“ werden zu Schlafaugen.
- Neue Striche (Pfoten auf dem Onigiri, Z-Zeichen, Brauen, Wellenmund,
  Schweißtropfen, zwei Striche) übernehmen die gemessene Tinte des Bogens:
  8–10 px Breite je 627-px-Zelle, Kern etwa rgb(60, 62, 70), dunklerer Rand,
  heller Saum und leichte Körnung. Flächen nutzen die Palette des Bogens
  (Himmelblau #C7E0F6 für den Tropfen, Schiefer #4B5563 für das Nori).

Erneut erzeugen: `py -3 design/brand/moods/derive.py assets/brand/mascots-v2.png assets/brand/mascots-v3-moods.png`.
SHA-256 des Ergebnisses:
`b7a7cb07ceab3207c2a297559b61ef9d28f5c715313dae5bee739617d7001667`.
Die Konturmessung unten betrifft nur die vier v2-Posen. Für die neuen Striche
wurden die Breiten an Mund, Augenbögen, Schnurrhaaren und Strichen gemessen und
übernommen; eine eigene Profilmessung der v3-Posen steht aus.

Native Ansichten mit den neuen Posen: [Erfassung](../../docs/screenshots/wolke-erfassung.png),
[Pause](../../docs/screenshots/wolke-pausiert.png),
[hängendes Experiment](../../docs/screenshots/wolke-experiment-haengt.png).

## Messung der v2-Konturen

Die [native Konturmessung](measurement/README.md) an je 43 vergleichbaren
Kopf-/Ohrpositionen ergibt Mediane von 1,69 / 1,73 / 1,62 / 1,71 logischen Pixeln
(Neutral / Fröhlich / Ordner / Laptop, Referenz bei 400 %). Der größte Abstand
beträgt 0,107 px bzw. 6,6 %; die Handzeichnungen sind somit ähnlich, aber nicht
mathematisch gleich breit. Dieselbe Pose wird in ihren Ansichten und beiden
Fenstergrößen bei gleicher DPI pixelgleich gerendert. [Messstellen und Bilder](measurement/profiles.html).

## Original und Exporte v1 (Archiv)

Quelle ist das unveränderte [ausgewählte Bild](../variants/manga/01-mochi-cloud.png)
mit SHA-256 `59a2f8cbd92029cf12e8faaf5ba218286a8b6313b8380283a8a275be4643f6fe`.
Die folgenden archivierten Ausschnitte übernehmen die Originalpixel einschließlich
Papier- und Stifttextur. Für diese v1-Dateien gab es keine Neugenerierung oder
Vektornachzeichnung. Der Generierungsweg und ursprüngliche Prompt bleiben
bei der [Konzeptstudie](../variants/manga/README.md) dokumentiert.

| Asset | Quellrechteck x, y, Breite, Höhe | Verwendung |
|---|---|---|
| [icon.png](../../assets/brand/icon.png) | 732, 783, 168, 168 | Fenstericon und Sidebar |
| [mascot.png](../../assets/brand/mascot.png) | 12, 234, 494, 468 | v1-Vorlage mit Ordner |
| [mascot-neutral.png](../../assets/brand/mascot-neutral.png) | 10, 772, 224, 184 | v1-Vorlage neutral |
| [mascot-happy.png](../../assets/brand/mascot-happy.png) | 225, 772, 224, 184 | v1-Vorlage fröhlich |
| [mascot-focused.png](../../assets/brand/mascot-focused.png) | 449, 772, 224, 184 | v1-Vorlage am Laptop |
| [wordmark.png](../../assets/brand/wordmark.png) | 61, 165, 224, 50 | Exakter handschriftlicher Schriftzug |
| [tray.png](../../assets/tray.png) | Skalierung des Kopf-Icons auf 64 × 64 | Windows-Tray, einschließlich höherer DPI |
| [mogumogu.ico](../../assets/brand/mogumogu.ico) | Kopf-Icon bei 16/20/24/32/40/48/64/128/256 px | In die Windows-EXE eingebettetes Icon |

Die Ausschnitte behalten ihren hellen Papierhintergrund; sie werden nicht als
transparente Freisteller ausgegeben. Skalierungen verändern nur die Auflösung.
Das alte Vektorlogo unter `assets/logo.svg` bleibt als historische Quelle erhalten
und wird von der Anwendung nicht mehr verwendet.

Die v1-Ausschnitte sowie Icon, Tray und Wortmarke erneut exportieren:
`powershell -File design/brand/export.ps1` nach einem nativen Debug-Build.
Der Vorbereitungsschritt nutzt den bereits gebauten Bilddecoder von
Slint, prüft den Quellhash und vergleicht die gespeicherten Ausschnitte mit den
ausgewählten Pixeln. Er läuft weder beim normalen Build noch in der Anwendung.
Der aktuelle v2-Zeichensatz wird von diesem Archivexport nicht ersetzt. Die
Anwendung bettet den v2-Bogen, Icon und Wortmarke ein; die ursprüngliche
Konzepttafel bleibt eine separate Designquelle.

## Ausdruck passend zur Ansicht

Über **Einstellungen** in der Seitenleiste wählt `render_mascot` in
`src/desktop/view.rs` die Pose: zuerst der Zustand (Erfassung → Mampft, Pause →
Schläft, hängendes Experiment oder Teilerfassung → Besorgt, leeres Inventar →
Fröhlich), sonst die geöffnete Ansicht. Dazu gehören ein kurzer Satz in der
Sprechblase und eine Bildunterschrift; der Zustand steht immer auch als Text in
der Oberfläche. `Mascot` in `ui/components.slint` nimmt Pose 0–3 aus dem v2-Bogen
und 4–6 aus dem Stimmungsbogen.

Alle Posen werden mit demselben Maßstab angezeigt. Ein gemeinsamer Innenrand von
vier Prozent je Zelle hält die Figuren im Bildbereich gleich groß; bei beiden
Bögen sind das 25 Pixel Innenrand und 577 × 577 Pixel je angezeigtem Ausschnitt.
Der Bereich in der Sidebar behält seine Layoutvorgaben (64 bis 104 logische Pixel
Höhe); `image-fit: contain` erhält die Proportionen. Auf weißen Karten liegt die
Figur mit Rand auf einer runden cremefarbenen Fläche (`MascotPlate`), damit der
Papierrand der Zeichnung nicht sichtbar wird. Die Figur bleibt dekorativ und
erzeugt kein Fokusziel.

## Schrift im Programm

Die Schrift im generierten Bild ist Rasterkunst und keine verfügbare Fontdatei.
Der Schriftzug bleibt deshalb ein Bild. Für die tatsächlichen Bedienelemente
wird **Zen Maru Gothic** in Regular (400) und Bold (700) verwendet: eine weiche,
runde serifenlose Familie mit lateinischen Zeichen sowie japanischen Kana und
Kanji. Das ist eine passende Begleitschrift, keine behauptete Identifizierung
der gezeichneten Buchstaben.

Die vollständigen, unveränderten TTF-Dateien sind in der nativen Anwendung
eingebettet. Es gibt keinen Laufzeitdownload und keine systemweite Installation.
Japanische Projekt-/Paketnamen können damit dargestellt werden; eine japanische
Übersetzung der Bedienoberfläche ist damit nicht eingeführt.

[Herstellerprojekt](https://github.com/googlefonts/zen-marugothic) ·
[Dateien, feste Revision und SHA-256](../../assets/fonts/zen-maru-gothic/SOURCE.json) ·
[SIL Open Font License 1.1](../../assets/fonts/zen-maru-gothic/OFL.txt).
Die Lizenz wird zusätzlich als `FONT-LICENSE.txt` ins Build-/Release-Verzeichnis
kopiert. Beide Originalfonts behalten ihren vollständigen Zeichenvorrat;
eine Beschränkung auf die deutschen UI-Texte würde japanische Nutzerdaten brechen.

Die App bleibt ein natives Slint-Fenster mit dichter Navigation und Tabellen.
Papierweiß, ein heller blauer Hintergrund und leicht gerundete Karten begleiten
die Figur, ohne die Fenstermaße zu vergrößern. Die HTML-Seite ist nur die lokale
Assetvorschau.

Aktuelle native Vorschauen im Mochi-Wolke-Design (`design/screens/render.ps1`):
[Übersicht](../../docs/screenshots/wolke-uebersicht.png),
[900 × 560](../../docs/screenshots/wolke-uebersicht-900x560.png),
[150 %](../../docs/screenshots/wolke-uebersicht-150.png),
[geführter Einstieg](../../docs/screenshots/wolke-einstieg.png).

Frühere Vorschauen (v2, vor der Mochi-Wolke): [Übersicht bei 1080 × 680](../../docs/screenshots/dashboard-mochi-render.png)
und [japanischer Projektname bei 900 × 560](../../docs/screenshots/japanese-project-render.png).
Weitere Ansichten: [Projekte mit Ordner](../../docs/screenshots/mascot-folder-render.png),
[Pakete mit konzentriertem Mochi](../../docs/screenshots/mascot-focused-render.png)
und [Einstellungen mit neutralem Mochi](../../docs/screenshots/mascot-neutral-render.png).
Beides sind Ausgaben des echten Slint-Software-Renderers mit synthetischen Daten,
keine HTML-Nachbauten und keine Aufnahmen des Windows-Fensterrahmens.
