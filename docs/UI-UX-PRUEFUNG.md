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

## Verbleibende Prüfung

Die geometrischen Checks finden mögliche Kollisionen und unzugängliche Grenzen,
ersetzen aber keine Bedienungsstudie. Screenreader, vollständige interaktive
Desktopabnahme und Windows 11 bleiben nach `WINDOWS-ABNAHME.md` offen. Die
Inhaltsverdichtung liefert keine neue Aussage darüber, ob Dateien entbehrlich
sind oder tatsächlich verwendet werden.
