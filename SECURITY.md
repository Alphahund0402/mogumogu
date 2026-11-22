# Sicherheit

## Schutzgrenzen
- Gelesen wird nur unter ausdrücklich freigegebenen Ordnern, gebunden an deren Identität.
  Darunter wird jede Pfadkomponente relativ zum Elternhandle geöffnet; Junctions, Symlinks,
  sonstige Reparse Points und Cloud-Platzhalter werden nicht verfolgt.
- Beim Erfassen startet mogumogu keine Paketmanager, Interpreter, Skripte, Hooks oder
  MCP-Server. Der optionale Schwachstellen-Watcher gleicht veröffentlichte
  Paketversionen aus Projekt- und Installationsbelegen lokal ab. Sein ausdrücklicher Abruf lädt öffentliche
  OSV-Datenbanken per HTTPS. [Schwachstellen-Watcher](docs/SCHWACHSTELLEN.md)
- Die lokale IPC-Pipe ist auf den aktuellen Benutzer und lokale Clients beschränkt und nur
  von der ersten Instanz belegbar; beide Seiten prüfen die Gegenseite (Benutzer, Sitzung).
- Ein bestätigter Papierkorb-Plan darf wiederherstellbar belegte Ordner aus
  registrierten Wegwerf-Testwurzeln verschieben. Vorher schreibt mogumogu das
  Journal und prüft die Verzeichnisidentität am Handle. Windows erhält den
  Pfad unter dem festgehaltenen Elternordner. Der unabhängige Review G5
  umfasst diese Übergabe. [Bereinigungsgrenzen](docs/PROJEKTBEWUSSTE-BEREINIGUNG.md)
- Freitext wird vor dem Speichern redigiert (Token-Muster, URL-Zugangsdaten, Query-Strings,
  Schlüssel=Wert-Geheimnisse). Das ist eine erste Schranke, keine Garantie.

## Außerhalb der Schutzgrenze
Ein kompromittierter Prozess desselben Benutzers kann die Datenbank lesen oder die Pipe
ansprechen. Die lokale Datenbank ist nicht verschlüsselt. Registrierte Namen und Pfade sind
vertrauliche Metadaten. Datenbank und Inventarexporte nur bewusst teilen.

## Meldungen
Vor einer Veröffentlichung müssen Maintainer einen privaten Meldekanal hinterlegen (z. B.
GitHub Private Vulnerability Reporting). Bis dahin Befunde direkt an die
Projektverantwortlichen geben; keine echten Secrets oder produktiven Inventare in
öffentliche Issues hochladen. Befunde zu Lese- oder Löschgrenzen bitte mit minimaler
Reproduktion in einer Wegwerf-Testwurzel.
