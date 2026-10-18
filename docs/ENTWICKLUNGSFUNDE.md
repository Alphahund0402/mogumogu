# Entwicklungsfunde, Speicher und Skill-Inhalte

Stand: 18. Oktober 2026. Die neuen Fähigkeiten sind experimentell. Tests verwenden
synthetische Windows-Verzeichnisse; eine Windows-11-Abnahme steht aus.

## Fundliste

Die Dateinamensuche (`projects`) und freigegebene Projektbereiche erkennen
Projektmarker, kontextbezogene Experimentordner, einzelne Skripte ohne
Projektmarker, Notebooks, Python-Umgebungen, Abhängigkeitsordner, typische
Build-/Testausgaben, Testdaten, lokale Datenbanken und bekannte Cache-Namen.
Diese Erkennung liest nur Verzeichniseinträge. Große erkannte Speicher und
Ausgabeordner werden als Funde erfasst und in der Suche nicht weiter betreten.
Der bestehende Projektadapter kann nach Projektfreigabe eng begrenzte
Manifest-/Installationsmetadaten lesen.

Jeder Fund zeigt Typ, Beleg, Pfad, Scanabdeckung, mögliche Projektzuordnung und
bekannte Dateigröße/Änderungszeit. Ein Änderungsdatum ist kein Nutzungsnachweis.
Ordnernamen belegen weder Testzweck noch Entbehrlichkeit. Unbekannte Inhalte
bleiben unbekannt; Editor-Backups werden nicht als Scratchpads übernommen.

Unter **Projekte → Funde** lassen sich Funde nach Skripten/Experimenten,
Umgebungen/Builds, Testdaten/Datenbanken und fehlendem Projektbezug filtern.
Dieselbe Liste zeigt auch die bei der Projekterfassung erkannten Umgebungen,
Build-Ausgaben und Caches. Die Ansicht ist auf 200 Funde begrenzt. Die globale Suche
fragt gespeicherte Fundmetadaten unabhängig von dieser Grenze ab und öffnet auch
Funde außerhalb der ersten Seite. Für große Bestände die Suchwurzeln sinnvoll
aufteilen. Teilläufe ersetzen den letzten vollständigen Stand nicht.

Eine Zuordnung über **Projekt zuordnen** ist ausdrücklich, speichert einen
optionalen Zweck und lässt den Schutz aktiv. Sie gewährt keine zusätzlichen
Leserechte. Auch einzelne Dateien können so verfolgt werden. Projektregistrierung
ist nur für Verzeichnisse möglich. Veränderte oder redigierte Pfadmetadaten
dürfen nicht automatisch zu einem echten Dateipfad rekonstruiert werden.

Die Generationen vergleichen bei diesen Funden auch Dateigröße und
Änderungsdatum. Damit werden Veränderungen sichtbar, aber kein verursachender
Prozess und keine tatsächliche Verwendung bewiesen. Neue Experimente können
weiterhin über die verwalteten Scratchpads und Sessions von Beginn an Zweck,
Projekt, Umgebung, temporäre Ausgabe und Ergebnisse auseinanderhalten.

## Laufwerke und Ordnerstruktur

**Speicher** zeigt Windows-Laufwerke, OS-Kapazität, belegten/freien Speicher,
registrierte Projekte und gemessene Anteile an der Laufwerkskapazität. Größen
werden auf Anfrage gemessen. Normale Snapshot-Aktualisierungen führen keinen
Dateibaumscan aus; OS-Metadaten werden höchstens 30 Sekunden zwischengespeichert.
**Aktualisieren** fragt die Kapazität erneut ab und aktualisiert eine offene
Ordneransicht. Auf Wechselmedien, Netzlaufwerken und optischen Laufwerken bleibt
die Kapazität ohne Abfrage unbekannt.

Projektgrößen sind logische Dateigrößen, keine physisch freigebbare Belegung.
Hardlinks, Kompression und gemeinsame Daten können abweichen. Verschachtelte
Projekte und identische normalisierte Projektpfade werden in der Laufwerkssumme
nicht doppelt addiert. Unvermessene Projekte machen die Gesamtsumme unbekannt.
Gespeicherte Messungen werden als früherer Messstand gekennzeichnet
und können inzwischen verändert sein. Die Gruppierung verwendet den registrierten
Laufwerksbuchstaben; Mountpoints/SUBST werden nicht physisch aufgelöst.

Die Ordnerstruktur öffnet jeweils eine Ebene und lässt sich durch Ordner weiter
navigieren. Nur freigegebene Projekt- und Suchbereiche sind verfügbar; enge
Benutzer-KI- oder Paketfreigaben eröffnen keinen allgemeinen Dateibrowser.
Eine Ansicht enthält höchstens 200 Einträge. Die Größenmessung hat ein gemeinsames
Budget von 10.000 Einträgen und zwei Sekunden; unvollständige Größen bleiben
unbekannt. Bei reinen Suchfreigaben werden keine Größen gemessen. Links,
Junctions und Cloud-Platzhalter werden nicht verfolgt. Widerruf, Austausch der
Wurzel und ungültige relative Pfade blockieren den Zugriff.

## Skill-Inhalte

In **KI-Konfiguration** öffnet ein erkannter Skill, eine Instruktion oder Regel
über **Inhalt ansehen** einen schreibgeschützten, scrollbar auswählbaren Text.
Jedes Öffnen prüft veröffentlichte Fundmetadaten, Katalog, aktuelle Freigabe und
Ordneridentität erneut. Es werden keine Verweise verfolgt und keine gefundenen
Anweisungen ausgeführt. Gemischte Konfigurationsdateien und Credentials sind
keine Inhaltsquellen dieses Viewers.

Texte bleiben flüchtige Anzeigedaten und gelangen nicht in SQLite, Snapshot,
Aktivität oder Export. Bekannte sensible Muster werden redigiert; das ist keine
vollständige semantische Geheimniserkennung. Die Größe ist auf höchstens 128 KiB
und zusätzlich das jeweilige Profillimit begrenzt. Binäre, zu große oder
ungültige UTF-8-Dateien werden abgewiesen. Die Demo verwendet nur einen
eingebauten Beispieltext und synthetische Speicher-/Ordnerdaten.

## CLI

```powershell
mogumogu-cli discoveries
mogumogu-cli resource adopt --scope 2 --relative experiments/probe.py --project 1 --purpose "Parserprobe"
mogumogu-cli storage
mogumogu-cli storage tree --scope 1 --relative source
mogumogu-cli ai content --scope 1 --relative .agents/skills/beispiel/SKILL.md
```

## Prüfnachweise und Grenzen

`tests/package_scopes.rs` prüft Erkennung, Kontext, Zuordnung, Schutz,
Idempotenz, Teilläufe und den Zugriff außerhalb des Seitenlimits.
`tests/disk.rs` prüft Freigaben, Austausch, Widerruf, Junctions, Limits,
Summierung und Demotrennung. `tests/ai_content.rs` prüft flüchtige Inhalte,
Redaktion, Austausch, Widerruf, Pfadausbruch und Format-/Größenlimits.
Die Leseerweiterungen wurden zusätzlich von einem getrennten Agenten geprüft.
Dies ersetzt keine Windows-11- oder menschliche Sicherheitsabnahme.

Quellen: [Windows-Laufwerke](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getlogicaldrives),
[Laufwerkstyp](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew),
[Kapazität](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw),
[Slint TextEdit](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/textedit/).
