# Sicherheit

## Schutzgrenzen
- Gelesen wird nur unter ausdrücklich freigegebenen Ordnern, gebunden an deren Identität.
  Darunter wird jede Pfadkomponente relativ zum Elternhandle geöffnet; Junctions, Symlinks,
  sonstige Reparse Points und Cloud-Platzhalter werden nicht verfolgt.
- Beim Erfassen startet mogumogu keine Paketmanager, Interpreter, Skripte, Hooks oder
  MCP-Server und stellt keine Netzwerkverbindung her. Updatehinweise sind opt-in je
  öffentlicher Quelle; private Quellen werden nie abgefragt.
- Die lokale IPC-Pipe ist auf den aktuellen Benutzer und lokale Clients beschränkt und nur
  von der ersten Instanz belegbar; beide Seiten prüfen die Gegenseite (Benutzer, Sitzung).
- Entfernt wird nur verwaltete Temp-Ausgabe nach bestätigtem Plan, in dieser Version nur in
  registrierten Wegwerf-Testwurzeln. Jede Entfernung ist vorher journalisiert und wirkt auf
  das zuvor identitätsgeprüfte Handle.
- Freitext wird vor dem Speichern redigiert (Token-Muster, URL-Zugangsdaten, Query-Strings,
  Schlüssel=Wert-Geheimnisse). Das ist eine erste Schranke, keine Garantie.

## Außerhalb der Schutzgrenze
Ein kompromittierter Prozess desselben Benutzers kann die Datenbank lesen oder die Pipe
ansprechen; die lokale Datenbank ist nicht verschlüsselt. Registrierte Namen und Pfade sind
vertrauliche Metadaten: Datenbank und `--include-paths`-Exporte nur bewusst teilen.

## Meldungen
Vor einer Veröffentlichung müssen Maintainer einen privaten Meldekanal hinterlegen (z. B.
GitHub Private Vulnerability Reporting). Bis dahin Befunde direkt an die
Projektverantwortlichen geben; keine echten Secrets oder produktiven Inventare in
öffentliche Issues hochladen. Befunde zu Lese- oder Löschgrenzen bitte mit minimaler
Reproduktion in einer Wegwerf-Testwurzel.
