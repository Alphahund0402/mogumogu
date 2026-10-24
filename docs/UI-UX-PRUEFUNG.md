# Informationsdarstellung und Verdichtung · 18. Oktober 2026

Geprüft wurde die native Slint-Oberfläche: alle elf Seiten, ihre Tabs, Listen,
Detailansichten, Formulare, der geführte Einstieg sowie Suche und Statusanzeigen.
Die unabhängige HTML-Designvorschau wurde nicht verändert. Ziel ist, Projekte,
Bestände und ungeklärte Funde schneller zu überblicken, ohne die Datenlage oder
eine Berechtigung durch eine verkürzte Anzeige falsch darzustellen.

## Methode und Befunde

Heuristischer Review mit Intent/Evaluate, Textüberarbeitung mit Articulate,
Quellprüfung und native Software-Renderings mit synthetischen Daten. Dies ist
eine Expertenprüfung, keine Nutzerstudie und keine Screenreader-Abnahme.

Die wesentlichen Befunde waren übergroße wiederholte Erklärungen, zu schmale
Tabellenspalten, Metainformationen vor den eigentlichen Daten und abgeschnittene
Angaben ohne sichtbaren Detailzugang. Es gab keine neue Löschfähigkeit und keine
Änderung der Lese- oder Netzwerkfreigaben.

| Oberfläche | Prüfung und Verbesserung |
| --- | --- |
| Übersicht | Kürzerer Kopf, Hinweiswert beim Titel, Status und Aktion in einer Zeile. Direkter Zugang zu Laufwerken. |
| Projektliste | Bei wenig Breite steht der Pfad unter dem Namen. Größen rechtsbündig, Statusspalten begrenzt. Alle Werte bleiben im Projekt-Detail erreichbar. |
| Projektfunde / Fundliste | Gemeinsame Filter, Anzahl als angezeigte Treffer bezeichnet, eindeutiger Leerzustand und Zurücksetzen von Suchtext und Kategorie. |
| Paketinventar | Inventar und Paketmanager von Listenfiltern getrennt. Version vor der Quelle in eigener Unterzeile. Update-Einstellungen als erreichbarer nächster Schritt bei ausgeschalteten Quellenabfragen. |
| Paketdetails | Kompakte Ansicht mit beschriftetem „Zur Liste“. Belegkette und lange Quellenhinweise bleiben scrollbar. |
| Paketmanager | Capability- und Statusangaben umbrechen statt unzugänglich abgeschnitten zu werden. |
| KI-Konfiguration | Gefundene Einträge zuerst; zwölf Erkennungsprofile aufklappbar. Eintragsbezeichnung als Titel, Client/Typ/Pfad darunter. Inhaltseinsicht und Unsicherheit bleiben erklärt. |
| Skill-Inhalt | Schreibgeschützter, markierbarer Text mit Dateipfad und Redaktionshinweis. Unveränderte flüchtige Datenhaltung. |
| Laufwerke | Belegter Speicher als Hauptwert, freie Kapazität und Projektbytes zusammen. Füllbalken links verankert. Art und Hinweise über Details erreichbar. |
| Projektspeicher | Größe und Laufwerksanteil rechts, weniger redundante Balken. Messhinweise aufklappbar; logische Größen und unbekannte Werte bleiben ausdrücklich benannt. |
| Ordnerstruktur | Volle Namen und Pfade können umbrechen. Dateien sind Information ohne scheinbare Öffnen-Aktion; Ordner bleiben navigierbar. |
| Experimente | Kompakter Kopf, CLI-Hilfe bei Bedarf, vollständige Sessionnotizen und Planordnernamen. Sperren, Freigabegrund und Planvorschau bleiben sichtbar. |
| Ressourcen | Registrierte Ressourcen, Scratchpads, Funde sowie Umgebungen/Ausgaben klar getrennt. Jeder leere Tab erklärt den nächsten Schritt. |
| System | Werkzeugfunde und globale Fundorte haben aufklappbare vollständige Pfade/Hinweise. Lesefreigabe und Erfassung bleiben separate Aktionen. |
| Prüfliste | Hinweise und Pläne klar gegliedert, Erstellungszeit vor Fingerabdruck, sinnvolle Leerzustände. Produktive Bereinigung bleibt gesperrt. |
| Aktivität | Lesefreigaben und Ereignisse als Tabs mit mehr Listenfläche. Ereignisse sind nicht anklickbare Textzeilen und umbrechen bei langen Inhalten. |
| Einstellungen | Zwei Spalten ab 1036 logischen Pixeln; klare Texte und weiterhin getrennte globale Netzwerkoption und Quellenfreigaben. |
| Dialoge und Formulare | Gemeinsame umbrechende Fußzeile mit erreichbarem Abschluss. Kurze Meldungen kleiner, lange Details scrollbar. Scratchpads wählen ein Projekt nach Name/Pfad statt numerischer ID. |
| Einstieg | Kleinere dekorative Flächen. Freigabe ohne Soforterfassung hat eine passende Schaltfläche und behauptet noch keine Vergleichsbasis. |
| Navigation / Suche / Status | Kürzerer Suchplatzhalter und Statusabschluss. Navigationsziele, Scanfortschritt, Abbruch, Ergebnis, Suche ohne Treffer und Tray-Menüs überprüft. |

Die erhöhten Listenabstände bleiben erhalten: 14 px zwischen Gruppen, 40 px für
einzeilige und mindestens 56 px für zweizeilige Standardzeilen. Schrift wurde
nicht pauschal verkleinert. Sprechblasen behalten gleichmäßige 14 px Innenabstand.
Lange reine Informationszeilen dürfen wachsen; Zeilen mit einem Detaildialog
können weiterhin Ellipsen verwenden.

## Zustände und Verifikation

`examples/layout_audit.rs` rendert echte kompilierte Slint-Komponenten und
exportiert ihre Geometrie. Der vollständige Lauf umfasst alle Seiten und Tabs,
lange und kurze Texte, leere Listen, Scroll-Enden, Dialoge, den Einstieg mit und
ohne Soforterfassung, unbekannte Laufwerksgrößen und die relevanten Breitenwechsel.
Die Skalierungen sind 100 %, 150 % und 200 % bei mindestens 900 × 560 logischen
Pixeln. Neue Details werden mit nativen Pointer-Ereignissen geöffnet. Ein
Tastaturcheck prüft die initiale Projektauswahl, Pfeilwechsel und Tab zum Namen.

Die Ergebnisse des letzten vollständigen Laufs stehen in
[VALIDIERUNG.md](VALIDIERUNG.md). Lokale Renderings und Messdateien liegen unter
`target/layout-audit-ui-ux-final`, ohne echte Benutzerpfade oder Inventardaten.
Die letzte Paketfilter-Korrektur wurde gezielt zusätzlich unter
`target/layout-audit-ui-ux-packages-final` geprüft. Beispielbilder:
[KI](screenshots/ui-ux-ki.png), [Pakete](screenshots/ui-ux-pakete.png),
[Speicher](screenshots/ui-ux-speicher.png),
[Einstellungen](screenshots/ui-ux-einstellungen.png).

Bei der Implementierung wurden die [offiziellen Slint-Layoutregeln](https://docs.slint.dev/latest/docs/slint/guide/language/coding/positioning-and-layouts/)
und die [Text-Eigenschaften](https://docs.slint.dev/latest/docs/slint/reference/elements/text/)
mit dem lokal gepinnten 1.18.1-Quellstand abgeglichen; Context7 war wegen seines
Monatslimits nicht verfügbar.

## Verschlankung · 20. Oktober 2026

Funktionen ohne Bezug zu „Projekte und Scratchpads finden, Aufräumbedarf
bewerten, KI-Zugriff eingrenzen“ sind entfernt:

| Vorher | Jetzt |
|---|---|
| Einstellungen: Überwachung mit Pause, Tray-Hinweise mit Tageszusammenfassung und Ruhezeiten, Mit Windows starten, Updatehinweise mit vier Quellen, Daten mit Export | Geführter Einstieg und Daten (Sicherung); ein Satz erklärt, dass mogumogu offline ist und nur auf Klick erfasst |
| Pakete: „Quellen prüfen“, Spalten Quellenstatus und Update | Spalte **Lockfile-Hinweis**; keine Netzabfrage |
| Pakete › Werkzeuge & globale Bestände: Scoop, Chocolatey, Windows-Softwareliste | entfallen; Werkzeuge im Suchpfad bleiben |
| Tray: Überwachung an/aus/pausieren, Hinweis-Symbol, Pause-Symbol | letzte Erfassung, Prüfhinweise, Dashboard, Jetzt erfassen, Einstellungen, Beenden |
| Statusleiste: „Überwachung an · N Ordner beobachtet“, „Netzwerk aus“/„Updatehinweise an“ | letzte Erfassung, „Lokal · ohne Netzwerk“ |
| KI-Konfiguration: nur Dateilisten | Reiter **Zugriff**: Einstufung je Ordner mit Befunden, Kontextfilter-Kandidaten und Stufen zur Begrenzung; **Kontextfilter vorbereiten …** zeigt die Ergänzungen, **In Datei schreiben** schreibt sie in `.claude/settings.local.json`; **Kontextfilter für alle …** zeigt und schreibt alle Projekte in einem Schritt |
| Speicher: nur Projektgrößen | Reiter **Aufräumen** mit Bericht je Projekt, „Ruht“, globalen Beständen und Details; Übersichtskarte „Aufräumpotenzial“ |
| Übersicht „Session hängt“, Mochi besorgt bei hängender Session, „Session begründet freigeben“ im Scratchpad-Dialog | entfallen mit den Sessions |
| „Als entbehrlich markieren“ und „Plan-Vorschau“ im Dialog eines verwalteten Temp-Ordners | **Papierkorb-Plan …** im Aufräumen-Dialog eines Projekts; Plan-Dialog mit Ordner, Beleg, Größe, Blocker und Ergebnis je Ordner; „In den Papierkorb verschieben“ statt „Jetzt ausführen“ |
| Funde: Scratchpads einzeln zuordnen | zusätzlich **Alle übernehmen (N)** unter Scratchpads & Skripte |
| Funde: „Projekt“ für jede Manifestdatei | „Projekt“, „Wahrscheinlich Projekt“, „Mögliches Projekt“, „Teilprojekt“ mit letzter Änderung; unter Scratchpads & Skripte bekannte Ablageorte von Werkzeugen |

## Experimente entfernt, Laufwerkssuche · 19. Oktober 2026

Die Seite **Experimente** ist entfernt. Sie zeigte nur selbst angelegte
Scratchpads, die ohnehin unter **Projekte → Ressourcen** stehen. Ihre
Bedienelemente sind verteilt oder entfallen:

| Vorher | Jetzt |
|---|---|
| Navigationspunkt Experimente mit Hängt-Zähler | entfällt; Übersicht meldet „Session hängt“ mit „Ressourcen ansehen“ |
| „Scratchpad anlegen“ (Seite und Projektdialog) | nur CLI `scratch create` |
| „+7 Tage“, „Schützen“, „Zum Projekt machen“, „Alle Angaben“ | Scratchpad-Dialog unter Ressourcen (war dort bereits vorhanden) |
| „Begründet freigeben“ je Session | **Session begründet freigeben** im Scratchpad-Dialog |
| „Aufräumplan anzeigen …“ | „Plan-Vorschau“ im Dialog des verwalteten Temp-Ordners (war vorhanden) |
| Einstellungen › Ordner für Scratchpads | nur CLI `settings set scratch-root`; das Panel **Daten** rückt an seinen Platz |
| `Alt+1` … `Alt+7` | `Alt+1` … `Alt+6` |

Neu auf **Speicher**: Jede lokale Festplatte zeigt den Stand ihrer
Namenssuche und **Projekte suchen** bzw. **Erneut suchen**; „Nur dieses
Laufwerk“ heißt jetzt **Auswählen**. Die Ergebnisleiste bietet nach einer Suche
**Funde ansehen**. Die Fundliste hat den Filter **Projekte**, „Skripte &
Experimente“ heißt **Scratchpads & Skripte**, und die Fundart „Experiment /
Scratchpad“ heißt **Scratchpad**.

## Zusammenführung auf sieben Ansichten · 18. Oktober 2026

Anlass war die Rückmeldung, dass nicht jede Schaltfläche erkennbar etwas tut und
die Informationsmenge schwer verständlich ist. Geprüft wurden alle Seiten,
Dialoge und ihre Rust-Callbacks; geändert wurde nur die Darstellung
(`ui/`, `src/desktop/`). Kern, CLI, Lese- und Löschgrenzen sind unverändert.

**Doppelte Inhalte.** Die Fundliste stand unter Projekte *und* Ressourcen,
Scratchpads unter Ressourcen *und* Experimente, die Projekttabelle mit denselben
Filtern auf Übersicht *und* Projekte, Prüfhinweise als Übersichtskarten *und* in
der Prüfliste. Persönliche KI-Ordner lagen auf „System“ und waren aus der
KI-Seite nur über einen Verweisknopf erreichbar; Scoop, Chocolatey und die
Windows-Softwareliste hatten Freigabeknöpfe auf Pakete *und* System.
„Umgebungen & Ausgaben“ war zugleich Ressourcen-Reiter und Fundlisten-Filter.

**Schaltflächen ohne sichtbare Wirkung.** „Aktualisieren“ in der Werkzeugleiste
und „Sessions prüfen“ lösten nur einen Lesevorgang aus. „Quellen prüfen“,
„Erneut prüfen“, Sicherung und Export meldeten ihr Ergebnis allein in der
Statuszeile. „In den Tray wechseln“ entsprach dem Schließen des Fensters,
„Speichern“ war auch ohne Änderung aktiv, „Plan entwerfen“ stand dauerhaft
deaktiviert da, und „Suchwurzel freigeben“ verlangte interne Bezeichner wie
`npm-global` in einem Freitextfeld. Der Tray-Callback `refresh-inventory` war
an keinen Menüpunkt gebunden.

**Text.** Fast jede Seitenbeschreibung, viele Dialogzeilen und Mochis
seitenabhängige Sprechblasen wiederholten dieselben Schutzsätze („keine
Entbehrlichkeit“, „kein Löschplan“, „G5“). Fachbegriffe wie Suchwurzel oder
Wegwerf-Testwurzel standen ohne Erklärung.

| Vorher (11 Ansichten) | Jetzt (7 Ansichten) |
|---|---|
| Übersicht, Prüfliste, Aktivität › Ereignisse | **Übersicht**: Kennzahlen mit Sprung zur Seite, Entscheidungen auf anderen Seiten, darunter Prüfhinweise, letzte Änderungen und Pläne |
| Projekte, Ressourcen (alle Reiter) | **Projekte** mit Reitern Projekte, Ressourcen, Funde (Namenssuche und erfasste Umgebungen/Builds in einer Liste) |
| Pakete, Paketmanager-Reiter, System › Werkzeuge und Bestände | **Pakete** mit Reiter Werkzeuge & globale Bestände; Formatangaben bleiben in `mogumogu-cli adapters` und der Unterstützungsmatrix |
| KI-Konfiguration, Erkennungsprofile, System › KI-Benutzerordner | **KI-Konfiguration** mit Reiter Persönliche Ordner |
| Experimente | **Experimente** ohne „Sessions prüfen“; Aufräumplan nur, wenn etwas planbar ist |
| Speicher | **Speicher** ohne Aufklappknöpfe; Laufwerksart direkt in der Karte |
| Einstellungen, Aktivität › Lesefreigaben, Bereinigung-Panel | **Einstellungen** mit Reitern Allgemein und Lesefreigaben; Datenort und Sicherung in einem Panel; ein Exportdialog statt zweier Knöpfe |

Jede verbliebene Schaltfläche ändert sichtbar etwas: Hintergrundergebnisse
erscheinen zehn Sekunden in der Ergebnisleiste (`Desktop::notify`).
Suchordner werden über die native Ordnerauswahl hinzugefügt; Python-Umgebungen
und Bestände an abweichenden Orten über **Pakete → Werkzeuge & globale Bestände
→ Ort hinzufügen …** mit Auswahl in Klartext. Mochi spricht nur noch bei einem
Zustand (Erfassung, Pause, hängendes Experiment, teilweise Erfassung, leeres
Inventar). Dialoge verloren konstante Zeilen wie „Schutz: Aktiv …“ und passen
ihre Höhe dem Inhalt an. Die Hilfe (F1) nennt Tastenkürzel und CLI-Befehle.

Nebenbei behoben: Die Speicherseite reservierte durch `FlexboxLayout` in der
ScrollView leere Zeilen; Laufwerkskarten liegen jetzt in einem eigenen Raster.
`design/screens/render.rs` kompilierte nach früheren Domainänderungen nicht mehr
und rendert wieder alle Ansichten und Reiter.

## Verbleibende Prüfung

Die geometrischen Checks finden mögliche Kollisionen und unzugängliche Grenzen,
ersetzen aber keine Bedienungsstudie. Screenreader, vollständige interaktive
Desktopabnahme und Windows 11 bleiben nach `WINDOWS-ABNAHME.md` offen. Die
Inhaltsverdichtung liefert keine neue Aussage darüber, ob Dateien entbehrlich
sind oder tatsächlich verwendet werden.
