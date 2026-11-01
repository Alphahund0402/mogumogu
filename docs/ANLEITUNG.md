# Anleitung · mogumogu 0.1.0

Für Entwicklerinnen und Entwickler, die mogumogu auf Windows 10/11 nutzen oder mitentwickeln. Alle Befehle in PowerShell im Projektordner; `$cli` steht für `.\dist\mogumogu\mogumogu-cli.exe`.

## 1. Grundprinzip in drei Sätzen

Registrieren speichert nur Namen und Pfade. Lesen erfordert eine eigene, an die Ordneridentität gebundene **Lesefreigabe**. Entfernen ist nur über einen bestätigten **Papierkorb-Plan** möglich, nur für wiederherstellbar belegte Ordner, und bis zum unabhängigen Review nur in einer **Wegwerf-Testwurzel**.

## 2. Starten

```powershell
.\build.ps1                       # bauen, testen, lokales Inventar im Tray starten
.\build.ps1 -ShowDashboard
.\start.ps1 -ShowDashboard        # bereits gebaut mit lokalen Daten starten
.\rebuild.ps1                     # nur neu bauen und den EXE-Pfad zeigen (oder rebuild.cmd doppelklicken)
```

Der Tray-Prozess ist der **Besitzer**: Er hält die Datenbank und die lokale Pipe. Fenster schließen beendet ihn nicht; „Beenden“ im Tray-Menü schon. Die CLI startet den Besitzer bei Bedarf selbst (Tray ohne Fenster); `--no-start` verhindert das. Für Automatisierung: `mogumogu.exe --headless`, beenden mit `$cli owner stop`.

Standard ist das lokale Inventar mit deinen registrierten und freigegebenen Projektordnern. Neue Inventare beginnen leer. Die schreibgeschützte Beispielvorschau startet nur ausdrücklich mit `-Mode Demo`. Demo- und lokale Datenbank sind getrennte Dateien unter `%LOCALAPPDATA%\mogumogu`. Zum Wechseln den Besitzer beenden und im anderen Modus starten.

### Aufbau des Dashboards

| Ansicht | Inhalt |
|---|---|
| **Übersicht** | Kennzahlen (Projekte, Paketversionen, KI-Dateien, Projektgröße), Entscheidungen auf anderen Seiten (Aufräumpotenzial, Abkündigungen, teilweise Erfassung, neu gefundene Orte) und darunter **Prüfhinweise**, **Letzte Änderungen** und – falls vorhanden – **Bereinigungspläne** |
| **Projekte** | Reiter **Projekte**, **Ressourcen** (Scratchpads, ihre Ordner, zugeordnete Funde) und **Funde** (Namenssuche in Suchordnern und Laufwerken sowie erfasste Umgebungen und Build-Ausgaben); ein Projekt öffnet seine **Projektseite** |
| **Pakete** | Paketversionen mit Belegen und Quellenstatus; Reiter **Werkzeuge & globale Bestände** für den Systembefund |
| **KI-Konfiguration** | Gefundene Skills, Instruktionen, MCP-Server und Hooks; Reiter **Persönliche Ordner** wie `.claude` |
| **Speicher** | Laufwerke mit **Projekte suchen** und **Speicher analysieren** je lokaler Festplatte; Reiter **Projekte** (Projektgrößen), **Aufräumen** und **Speicheranalyse** |
| **Einstellungen** | Reiter **Allgemein** und **Lesefreigaben** (alle freigegebenen Ordner; erfassen, messen, widerrufen) |

**Projektseite.** Ein Projekt aus der Liste, der Suche oder **Projekt öffnen** im Reiter Aufräumen öffnet seine Seite. Oben stehen Lesefreigabe, Erfassung, Stand des Aufräumberichts und, falls KI-Werkzeuge weit reichen, die KI-Stufe. Darunter folgen **Speicher** (ein Balken aus Wiederherstellbar, Prüfen, Versionsverwaltung und dem Rest, dazu die Projektangaben), **Aufräumen** (jeder Eintrag des letzten Berichts mit Beleg), **Papierkorb-Pläne** (offene und ausgeführte, keine verworfenen), **Schutz** (behaltene Ordner und registrierte KI-Läufe), **KI-Zugriff** und **Verlauf** mit den Daten der beiden verglichenen Berichte. Fehlt die Lesefreigabe oder der Bericht, sagt die Seite das und bietet den nächsten Schritt an. **Alle Projekte** oder `Esc` führt zur Liste zurück.

Was eine Aktion ergeben hat (Erfassung, Laufwerkssuche, Systemprüfung, Speicherstand, Sicherung), steht danach zehn Sekunden lang in der Leiste über der Statuszeile. Nach einer Suche führt **Funde ansehen** direkt zu **Projekte → Funde**.

Ausgewählte Navigationseinträge, Filter und geöffnete Listenzeilen verwenden dieselbe blaue Fläche und dunkle Schrift; Häkchen bzw. eine Seitenmarkierung machen die Auswahl zusätzlich sichtbar. Die zuletzt geöffnete Zeile bleibt markiert. Der Tastaturfokus erscheint als Rahmen innerhalb des Elements und ist von der Auswahl getrennt.

Die Ansicht **Pakete** zeigt Paketversionen mit Projektbelegen, möglichen Verweisen, Abdeckung und Quellenstatus; ein Lockfile allein belegt keine Installation. Suche und Filterchips (Prüfen, Abgekündigt, Ohne Projektbeleg, Behalten) wirken vor dem Blättern auf den ganzen Bestand. Unterstützte Formate je Paketmanager nennt `& $cli adapters` und die [Unterstützungsmatrix](UNTERSTUETZUNGSMATRIX.md). Globale Paketbestände, persönliche KI-Konfiguration und eine reine Dateinamensuche nach Projekten haben eigene Lesefreigaben. Ablauf und Grenzen: [Paketprüfung](PAKETPRUEFUNG.md). Die Bestimmung eines Scratchpad-Zwecks bleibt offen.

### Projekt hinzufügen

**Projekt hinzufügen** (Werkzeugleiste, `Strg+N`) führt in drei Schritten zum Projekt: Ordner wählen, Lesefreigabe ansehen und erteilen, erfassen. Jeder Schritt zeigt vorher, was gelesen wird und was nie. Bei leerem Inventar bietet die Übersicht dasselbe an und daneben **Laufwerk durchsuchen**, das unter **Speicher** ein ganzes Laufwerk nach Projekten durchsucht; gefundene Projekte übernimmst du dann gesammelt (siehe „Projekte übernehmen“). Von selbst öffnet sich nichts.

Während einer Erfassung zeigt das Dashboard Fortschritt (Bereich *n* von *m*, gelesene Einträge). **Abbrechen** hält an der nächsten sicheren Grenze an; die Erfassung bleibt teilweise und der letzte vollständige Stand sichtbar. Danach nennt dieselbe Leiste das Ergebnis (z. B. „mogumogu erfasst · keine Änderungen seit der letzten Erfassung“) und schließt sich nach zehn Sekunden oder mit **Schließen**.

## 3. Projekte erfassen

```powershell
& $cli project register --name api --path C:\dev\api   # nur Metadaten
& $cli project approve --project 1                      # Lesefreigabe (identitätsgebunden)
& $cli scan                                             # statisch erfassen
& $cli list
& $cli why --resource 3                                 # Besitzer, Schutz, Hinweise
```

Die erste vollständige Erfassung bildet eine **Vergleichsbasis** ohne Einzelmeldungen. Spätere Läufe melden gruppierte Änderungen. Ein Limit, ein unlesbarer Ordner oder ein fehlendes Laufwerk ergibt eine **teilweise** Erfassung: Der letzte vollständige Stand bleibt sichtbar und es entstehen keine Entfernungsmeldungen. Wird ein freigegebener Ordner gelöscht und neu angelegt, ist die Freigabe blockiert und muss bewusst neu erteilt werden.

Nie gelesen werden: Inhalte hinter Junctions/Symlinks, Cloud-Platzhalter, `.git`, Dateien über den Größenlimits. Nie gestartet werden: Paketmanager, Interpreter, Skripte, Hooks, MCP-Server.

Systemweite Quellen werden getrennt freigegeben:

```powershell
& $cli system findings --refresh            # Systembefund: Suchpfad und bekannte Orte prüfen
& $cli system approve --kind user-ai --path $env:USERPROFILE\.claude   # persönliche KI-Konfiguration
& $cli search parser                        # Projekte, Pakete, Ressourcen, KI und System
```

Der **Systembefund** (**Pakete → Werkzeuge & globale Bestände**, persönliche KI-Ordner unter **KI-Konfiguration → Persönliche Ordner**) prüft nur Metadaten: welche bekannten Befehle im Suchpfad liegen (Pfad belegt, Shim, App-Alias, Verknüpfung), welche globalen Bestände an ihren üblichen Orten existieren und welche KI-Clients einen Benutzerordner haben. Nichts wird gelesen, gestartet oder einer Verknüpfung gefolgt; geprüft werden nur lokale feste Laufwerke. Pfade erscheinen mit Platzhaltern wie `%USERPROFILE%`. Ein Fund ist keine Lesefreigabe: „Lesen erlauben“ erteilt sie je Ort einzeln; danach startet die erste Erfassung.

Persönliche KI-Konfiguration (`%USERPROFILE%\.claude`, `.codex`, `.agents`, `.gemini`, `.cursor`, `.continue`, `.roo`, `%USERPROFILE%\.config\opencode`, `%APPDATA%\Code\User`) wird nur für diese bekannten Ordner freigegeben und nur entlang der Katalogpfade betreten. Zugangsdaten, Sitzungen und Chatverläufe werden nie gelesen; gefundene Skills, Hooks und MCP-Einträge sind Daten, keine Anweisungen.

## 4. Scratchpads, Funde und Aufräumen

Scratchpads sind Ordner, die du selbst angelegt hast; mogumogu legt keine an und startet keine Programme darin. Im Dashboard stehen sie unter **Projekte → Ressourcen**; ihr Dialog bietet Prüftermin +7 Tage, Schützen, Zum Projekt machen und – bei Ordnern, die eine frühere Version angelegt hat – Größe messen.

```powershell
& $cli scratch register --project 1 --name parser-spike --path C:\dev\scratch\parser-spike
& $cli scratch adopt --project 1 --name api-lab --path C:\dev\scratch\api-lab --purpose "Tokenizer testen"
```

Registrieren und Übernehmen setzen einen Prüftermin in 7 Tagen, „Alle übernehmen“ in 30 Tagen; erreicht er sich, erscheint ein Prüfhinweis. Weitere Aktionen: `scratch protect`, `scratch extend --days 7`, `scratch promote` (zum Projekt machen; Ablaufregel entfällt, Historie bleibt).

Bestehende Projekte und Scratchpads findet die Namenssuche: einen Suchordner über **Projekte → Funde → Suchordner hinzufügen …** (oder `& $cli system approve --kind projects --path C:\dev`) oder ein ganzes Laufwerk über **Speicher → Projekte suchen** (oder `& $cli system approve --kind projects --path D:\`), danach erfassen. Unter **Projekte → Funde** erscheinen Projekte (mit Sicherheit „Projekt“, „Wahrscheinlich Projekt“, „Mögliches Projekt“ oder als Teilprojekt), Scratchpads, Skripte, Notebooks, Testordner (mit Zahl der Testdateien und erkannten Testwerkzeugen), Umgebungen, Ausgaben und Testdaten mit Belegen und letzter Änderung. Projektfunde nennen außerdem den Paketmanager laut Dateinamen und ein Workspace, soweit erkennbar. Der Filter **Scratchpads & Skripte** bietet außerdem bekannte Ablageorte von Werkzeugen (Claude-Code-Arbeitsordner, JetBrains-Scratches, `%TEMP%`) zum Durchsuchen an, dazu die bei der Projekterfassung erkannten Umgebungen und Build-Ausgaben; **Projekt zuordnen** bestätigt Besitzer und optionalen Zweck.

**Laufwerkssuche.** **Projekte suchen** auf einer Laufwerkskarte erteilt mit einem Klick eine Namenssuche für die ganze lokale Festplatte, gebunden an ihre Identität, und startet sie. Gelesen werden nur Ordner- und Dateinamen. Übersprungen werden Windows, Programme, ProgramData, Wiederherstellungs- und Updateordner, alle Profile unter `Users` außer deinem eigenen, in deinem Profil `AppData`, `scoop`, `go\pkg` und Punktordner von Werkzeugen (`.cargo`, `.rustup`, `.claude` …) sowie Programmordner (`.exe` neben `.dll` ohne Projektmarker) und Spielebibliotheken – auch wenn an der Laufwerks- oder Profilwurzel ein Projektmarker liegt. Suchordner und Bestände lassen sich nur auf lokalen Festplatten freigeben. Ordner ohne Zugriffsrecht zählen als ausgelassen. Einzelne lose Skripte werden auf einem Laufwerk nicht gelistet; ab zwei Skripten oder einem Notebook in einem Ordner ohne Projektmarker erscheint der Ordner als mögliches Scratchpad. Ein Durchgang ist auf 2 Mio. Einträge, 32 Ebenen und drei Minuten begrenzt; reicht das nicht, bleibt er teilweise und zeigt den letzten vollständigen Stand. Die Freigabe steht unter **Einstellungen → Lesefreigaben** als „Laufwerk D:“ und lässt sich dort widerrufen. Netzlaufwerke, Wechselmedien und optische Laufwerke lassen sich nur ordnerweise durchsuchen; ein ganzes Laufwerk wird nie ausgemessen.

**Aufräumen.** **Speicher → Aufräumen → Bericht erstellen** misst jedes Projekt mit Lesefreigabe einmal und teilt seinen Inhalt auf: **Wiederherstellbar** ist, was ein Werkzeug neu baut – `node_modules` oder `.venv` mit Lockfile daneben, `target`/`dist` mit Build-Datei daneben, Werkzeugcaches und Test-Ausgaben. Wo ein Name auch selbst angelegt sein kann (`coverage`, `build`, `target`, `.venv`, die meisten Caches), muss zusätzlich die Spur des Werkzeugs im Ordner liegen, etwa `lcov.info`, `CACHEDIR.TAG` oder `pyvenv.cfg`. **Prüfen** sind Testdaten, Datenbanken, Logs, Dateien über 100 MB, `.cache`, `build` neben `package.json` sowie Umgebungen und Ausgaben ohne Beleg; außerdem jeder Ordner, in dem Git Dateien versioniert (etwa ein eingechecktes `dist/`) oder dessen Git-Stand sich nicht lesen lässt, jeder Ordner, den du behältst, und Reste von KI-Agenten wie `.claude/worktrees` oder `.playwright-mcp`, die der Bericht gesondert summiert. Testdaten zählt der Bericht gesondert; machen sie über 1 GB oder ab 100 MB mindestens die Hälfte des eigenen Projektinhalts aus (alles außer Ausgaben, Caches, Umgebungen und Versionsverwaltung), trägt das Projekt „Viele Testdaten“ – ein Anlass zum Hinsehen, kein Urteil. Projekte, die seit über einem halben Jahr unverändert sind und Wiederherstellbares enthalten, tragen „Ruht“. Freigegebene globale Bestände (z. B. der NuGet-Paketspeicher) und Paket-Caches (npm, pip, uv, Cargo-Registry, Gradle, Maven, pnpm, Yarn; im Systembefund mit „Lesen erlauben“) erscheinen mit ihrer Größe, Caches mit dem Aufräumbefehl ihres Werkzeugs – ausführen musst du ihn selbst. **Projekt öffnen** zeigt alle Einträge des Berichts mit Beleg auf der Projektseite und plant von dort (siehe Abschnitt 5). Der Bericht selbst entfernt nichts; die gemessene Projektgröße wird übernommen. CLI: `& $cli tidy` oder `& $cli tidy --project 1`.

**Projekte übernehmen.** Nach einer Projekt- oder Laufwerkssuche zeigt **Projekte → Funde → Projekte → Alle übernehmen …** die sicheren äußeren Projekte, die noch nichts verfolgt. Nach der Bestätigung registriert mogumogu sie, erlaubt das Lesen und erfasst sie; Teilprojekte und unsichere Funde übernimmst du einzeln. CLI: `& $cli project accept-all`, dann mit dem angezeigten Fingerabdruck `& $cli project accept-all --confirm FINGERABDRUCK`.

**Scratchpads übernehmen.** Unter **Projekte → Funde → Scratchpads & Skripte** übernimmt **Alle übernehmen** jeden gefundenen Scratchpad-Ordner, den noch nichts verfolgt, als geschützte Ressource mit Prüftermin in 30 Tagen. Liegt er in einem registrierten Projekt, gilt dieses als vermuteter Besitzer. Lesefreigaben entstehen dabei nicht. CLI: `& $cli scratch adopt-found`.

**Speicher** zeigt Laufwerke mit belegtem/freiem Speicher und gemessenen logischen Projektanteilen. **Größe messen** arbeitet nur in freigegebenen Projektbereichen; begrenzte oder fehlende Messungen bleiben unbekannt. **Aktualisieren** erneuert die Laufwerksmetadaten.

**Speicheranalyse.** Was in einem Ordner oder auf einer Festplatte Platz belegt, zeigt **Speicher → Speicheranalyse**. **Speicher analysieren** auf einer Laufwerkskarte oder **Ordner freigeben** im Reiter fragt zuerst nach einer eigenen Freigabe: mogumogu liest dann Namen, Größen und Änderungsdaten aller erreichbaren Dateien, aber keine Inhalte. Ein Projekt mit Lesefreigabe lässt sich ohne weitere Freigabe über **Analysieren** auf seiner Karte untersuchen. Der Reiter zeigt die Ordner des angezeigten Ordners mit Größe, belegtem Platz und Anteil (ein Klick öffnet einen Ordner, **Nach oben** geht zurück), **Größte Dateien**, **Dateitypen** und **Duplikat-Kandidaten** (gleicher Name und gleiche Größe; Inhalte werden nicht verglichen), darüber eine Flächenkarte. Das Suchfeld filtert nach einem Teil des Namens oder mit `*` und `?` nach dem ganzen Namen, z. B. `*.iso`. **CSV kopieren …** zeigt die aktuelle Seite zum Kopieren. Eine Analyse umfasst höchstens 4 Mio. Einträge und drei Minuten und belegt dabei bis zu etwa 300 MB Arbeitsspeicher (eine Systemfestplatte mit 3 Mio. Einträgen: rund 220 MB); reicht das nicht, oder fehlt das Leserecht für einen Systemordner, heißt das Ergebnis „Unvollständig“ und die Summen „Mindestens“; ein nicht vollständig gelesener Ordner zeigt „mind.“ vor seiner Größe, ein gar nicht gelesener „nicht gelesen“. Verknüpfungen und Cloud-Platzhalter werden ausgelassen. Das Ergebnis bleibt nur, bis das Fenster schließt; gelöscht oder verschoben wird hier nichts. CLI: `& $cli storage approve --path D:\`, `& $cli storage analyze --scope ID`, dann `storage tree|files|types|duplicates --scope ID` und `storage export --scope ID` für CSV.

**KI-Zugriff.** **KI-Konfiguration → Zugriff** stuft jeden erfassten Ordner mit KI-Dateien ein: **Weitreichend** (Bestätigungen oder Sandbox aus, z. B. `bypassPermissions`, `danger-full-access`, `approval_policy = never`, oder freigegebene Ordner und MCP-Server außerhalb des Projekts), **Ohne Filter** (nichts hält Dateien vom Agenten fern) oder **Begrenzt** (Lese-Verbote, Ignore-Dateien oder Sandbox). Der Dialog nennt die Befunde, Kontextfilter-Kandidaten (Umgebungen, Ausgaben, Testdaten des Ordners) und drei Stufen, um den Zugriff zu begrenzen. mogumogu liest dafür nur Zahlen und Schalter aus den Einstellungen, keine Pfade oder Regeltexte.

**Kontextfilter.** Im Zugriff-Dialog eines Projekts zeigt **Kontextfilter vorbereiten …** genau die Leseverbote, die in `.claude/settings.local.json` unter `permissions.deny` ergänzt würden: `Read(/ordner/**)` für Umgebungen, Ausgaben, Caches und Testdaten des Projekts sowie `Read(.env)`, `Read(.env.*)`, `Read(*.pem)` und `Read(*.key)`; einzelne Dateien bekommen eine genaue Regel, Ordner eine für ihren ganzen Inhalt. Testdaten stammen aus dem letzten Aufräumbericht; ohne Bericht sagt die Vorschau das. Eine Ausnahme für `.env.example` gibt es nicht, weil ein `!`-Muster in Claude Code auch frühere Verbote derselben Datei aufheben würde. Ordner, deren Name sich nicht eindeutig als Regel schreiben lässt, nennt die Vorschau als ausgelassen. Das ist deine persönliche Datei für dieses Projekt; die meist eingecheckte `.claude/settings.json` des Teams bleibt unverändert, ihre Verbote gelten weiter und werden nicht wiederholt. Erst **In Datei schreiben** ergänzt die Regeln; andere Einstellungen und bestehende Einträge bleiben, eine Verknüpfung auf dem Weg wird abgelehnt, und eine inzwischen geänderte Datei braucht eine neue Vorschau. Schließt die `.gitignore` des Projekts die Datei nicht erkennbar aus, sagt das die Vorschau; mogumogu ändert keine Git-Dateien. **Kontextfilter für alle …** im Reiter **Zugriff** zeigt dasselbe für alle Projekte mit Lesefreigabe auf einmal, nennt ausgelassene Projekte mit Grund und schreibt nach einer Bestätigung jedes Projekt für sich. Danach erfasst mogumogu neu. Für Codex gibt es keine Ausschlussdatei; der Dialog nennt stattdessen `sandbox_mode`. CLI: `& $cli ai filter --scope ID` bzw. `--all` zeigt die Vorschau samt Fingerabdruck, `--confirm FINGERABDRUCK` schreibt. Unter nativem Windows sandboxt Claude Code Shell-Befehle nicht. Seine Leseverbote gelten für die eigenen Datei-Werkzeuge, für erkannte Shell-Dateibefehle (`cat`, `head`, `tail`, `sed`, `tee`) und Umleitungen, nicht aber für Programme, die Dateien selbst öffnen (etwa Python- oder Node-Skripte), oder für Befehle ohne Dateinamen wie `grep -r`.

In **KI-Konfiguration** einen Skill oder eine Instruktion öffnen und **Inhalt ansehen** wählen. Der Text ist schreibgeschützt und kann markiert/kopiert werden. Konfigurationsmischdateien werden nicht angezeigt; erkannte sensible Werte werden ausgeblendet. Umfang und Grenzen: [Entwicklungsfunde, Speicher und Skill-Inhalte](ENTWICKLUNGSFUNDE.md).

## 5. Prüfhinweise und Papierkorb-Plan

Im Dashboard stehen die Prüfhinweise auf der **Übersicht**; `& $cli review` zeigt Hinweise mit Auslöser, Beleg, Besitzer und Unsicherheit. Ein Hinweis ist kein Urteil. Hinweise entstehen nur aus einem erreichten Prüftermin oder einem fehlenden Besitzer; Alter und Dateidaten allein ergeben keinen Hinweis. Wie lange ein Projekt unverändert ist, zeigt der Aufräumbericht („Ruht“).

**Papierkorb-Plan.** Auf der Projektseite steht vor jedem wiederherstellbar belegten Ordner ein Häkchen, anfangs bei allen; ein Häkchen gilt nur für diesen Plan. **Papierkorb-Plan prüfen …** misst das Projekt neu und plant genau die angehakten Ordner – höchstens 50 – mit Größe und Beleg (z. B. „package-lock.json daneben“). Ist ein angehakter Ordner inzwischen nicht mehr wiederherstellbar belegt, entsteht kein Plan, und die Meldung nennt ihn. Alles unter „Prüfen“, Quellen, Lockfiles und `.git` stehen nie im Plan. Bestätigen und verschieben geht nur im Dashboard, damit kein Agent mit CLI-Zugang einen Plan selbst ausführt; einen per CLI erstellten Plan öffnest du unter **Übersicht → Bereinigungspläne**. Mit dem Fingerabdruck bestätigt, verschiebt **In den Papierkorb verschieben** jeden Ordner als Ganzes in den Windows-Papierkorb; dort lässt er sich wiederherstellen, neu erzeugen lässt er sich mit dem Werkzeug aus dem Beleg (`npm ci`, `cargo build`, `uv sync` …). Den Speicher gibt Windows erst frei, wenn du den Papierkorb leerst. Nach dem Ausführen zeigt der Plan unter **Laufwerk** den freien Speicher, den mogumogu vorher und nachher gemessen hat – ein Messwert, keine Zusage, denn andere Programme schreiben gleichzeitig. Wäre ein Ordner zu groß für den Papierkorb, fragt Windows vor dem endgültigen Löschen; Netz- und Wechsellaufwerke ohne Papierkorb werden abgelehnt.

**Behalten.** Was nie in einen Plan soll, behältst du auf der Projektseite mit **Immer behalten** an jedem wiederherstellbaren Ordner oder per CLI. Der Ordner, alles darin und jeder Ordner, der ihn enthält, bleibt dann „Prüfen“ – in jedem Projekt, das ihn erreicht; ein schon bestätigter Plan stoppt vor dem Verschieben. Behalten braucht die Lesefreigabe des Projekts, Kurznamen wie `LONGFO~1` werden aufgelöst oder, für noch fehlende Ordner, abgelehnt. **Wieder vorschlagen** im Abschnitt Schutz der Projektseite hebt das auf, nur im Dashboard: Die CLI kann einen Ordner behalten, aber keinen Schutz aufheben. Anders als `.mogumogu-keep` schreibt das nichts ins Projekt. Ein Projekt, das selbst in einem Git-Repository liegt (etwa ein Paket eines Monorepos), räumt nichts auf, weil mogumogu dessen Index nicht lesen darf; registriere dafür das äußere Repository.

```powershell
& $cli cleanup plan --project 1                         # alle wiederherstellbaren Ordner
& $cli cleanup plan --project 1 --path web/node_modules  # nur diesen
& $cli cleanup cancel --plan 1                           # Plan verwerfen
& $cli cleanup keep --project 1 --path web/dist          # nie vorschlagen
```

Beim Planen und vor dem Verschieben prüft mogumogu jeden Ordner erneut: derselbe Ordner (Identität), derselbe Beleg, keine Verknüpfung auf dem Weg, ein Name, den Windows nicht anders liest (kein Punkt oder Leerzeichen am Ende, kein Gerätename), in einem Git-Repository keine Datei, die der Index von Git führt (ein unlesbarer Index sperrt), kein behaltener Ordner daran, darin oder darüber, und im **ganzen Inhalt** kein `.mogumogu-keep`, keine Versionsverwaltung (`.git`), keine Verknüpfung und kein Cloud-Platzhalter. Ordner mit Verknüpfungen darin – etwa `node_modules` von pnpm oder mit Workspace-Links – bleiben deshalb vorerst stehen. Ein `.mogumogu-keep` im Ordner oder darüber und eine geschützte Ressource darin, daran oder darüber sperren ihn ebenfalls; Pfade über 259 Zeichen werden abgelehnt. Weicht etwas ab, wird der ganze Plan ungültig, bevor sich etwas bewegt. Während des Verschiebens sind Projekt- und Elternordner so geöffnet, dass niemand sie umbenennen oder austauschen kann; jeder Schritt steht vorher dauerhaft im Journal.

Nach einem Absturz während der Ausführung steht der Plan auf „Abgleich erforderlich“:

```powershell
& $cli cleanup reconcile --plan 1
```

Der Abgleich vermerkt, welche Ordner nicht mehr am Ort sind, und verschiebt nie erneut. Für einen weiteren Versuch ist ein neuer Plan nötig.

**Gesperrt bis zum Review.** Bis zum unabhängigen Review (Gate G5) trägt jeder Ordner außerhalb einer registrierten Wegwerf-Testwurzel den Blocker „Gesperrt bis zum unabhängigen Review“; der Plan lässt sich ansehen, aber nicht bestätigen. Das ist keine Einstellung und lässt sich nicht umgehen. Zum Ausprobieren:

```powershell
New-Item -ItemType Directory C:\mogumogu-test | Out-Null
Set-Content C:\mogumogu-test\.mogumogu-disposable-test-root 'mogumogu disposable test root'
& $cli cleanup test-root --path C:\mogumogu-test     # vor der Registrierung der Testprojekte darin
```

Eine Testwurzel darf kein bereits registriertes Projekt enthalten oder in einem liegen.

**Verlauf.** Jeder Aufräumbericht wird je Projekt gespeichert (die letzten 30). Die Projektseite zeigt unter **Verlauf**, was seit dem ältesten vollständigen Bericht der letzten 90 Tage hinzukam, nennt beide Berichtsdaten und die drei am stärksten wachsenden Einträge. CLI: `& $cli tidy history --project 1`.

**KI-Läufe.** Unter **Speicher → Aufräumen → Läufe & Docker …** (auch im Abschnitt Schutz einer Projektseite) schaltest du mit **Läufe annehmen** ein, dass kooperierende Agenten ihre Läufe melden dürfen; ohne das lehnt mogumogu jede Registrierung ab. Einschalten geht nur im Dashboard, damit kein Agent es für sich selbst tut; ausschalten auch mit `& $cli run allow off`. Ein Agent meldet dann, woran er arbeitet und was er anlegt:

```powershell
& $cli run start --harness claude-code --path C:\dev\app --purpose "Fehler 42 nachstellen"
& $cli run artifact --run 1 --kind workspace --path C:\dev\app\agent-work
& $cli run artifact --run 1 --kind docker --context default --type volume --id app-testdb
& $cli run state --run 1 --state failed --note "Test bleibt rot"
& $cli run list --project 1
```

Alles, was ein Lauf meldet, ist geschützt und erscheint in keinem Papierkorb-Plan, gleich ob der Lauf läuft, pausiert oder beendet ist; der Aufräumbericht nennt den Lauf als Herkunft. Es gibt keinen Befehl, der einen Lauf, ein Artefakt oder etwas darin entfernt. Brauchst du die Artefakte eines beendeten Laufs nicht mehr, hebt **Lauf #N freigeben** im selben Dialog den Schutz auf: Lauf und Artefakte bleiben gespeichert, für den Aufräumbericht zählen dann wieder die gewöhnlichen Regeln. Freigeben geht nur im Dashboard, nicht über die CLI. Ein Artefakt muss im Projekt des Laufs liegen. Docker-Objekte kann ein Harness auch mit dem Label `io.mogumogu.run=1` versehen. mogumogu startet und überwacht keinen Lauf; der Zustand ist das, was der Agent meldet.

**Docker.** Im selben Dialog stehen die Docker-Kontexte deiner Docker-Konfiguration. Freigeben lässt sich nur ein Kontext mit einer Named Pipe dieses Rechners (etwa `desktop-linux` von Docker Desktop), nie ein entfernter. **„…“ ansehen** fragt die Engine nur lesend nach Containern (mit eingebundenen Ordnern und Projekt), Images, Volumes, Netzen und Dockers eigener Speicherbilanz. Diese Bilanz zählt in der Engine; die virtuelle Festplatte von Docker Desktop schrumpft dadurch nicht von selbst. mogumogu stoppt, entfernt oder bereinigt nichts in Docker. Ordner, die ein Container einbindet, laufend oder gestoppt, bleiben aus jedem Papierkorb-Plan heraus; ist eine freigegebene Engine nicht erreichbar, sperrt das jeden Plan, bis Docker läuft oder du die Freigabe widerrufst.

```powershell
& $cli docker contexts
& $cli docker approve --context desktop-linux
& $cli docker inventory --usage
```

Widerrufen (**„…“ widerrufen**) geht nur im Dashboard, weil danach die Einbindungen dieses Kontexts keinen Ordner mehr halten.

**Datenbanken.** **Speicher → Aufräumen → Datenbanken …** zeigt überwachte Datenbanken mit letzter Größe, letzter Änderung und Wachstum und schlägt weitere vor: Datenordner in den üblichen Installationsordnern von PostgreSQL, MySQL, MariaDB, MongoDB und Redis, Datenbankdateien aus dem letzten Aufräumbericht und Docker-Volumes freigegebener Kontexte, deren Name nach einer Datenbank klingt. **„…“ überwachen** registriert einen Vorschlag; auf der Projektseite (**Überwachen** bei Datenbankdateien) und im Docker-Inventar steht dieselbe Aktion. mogumogu verbindet sich mit keinem Datenbankserver und öffnet keine Datenbankdatei: Es zählt Größe, Dateien und letzte Änderung aus den Verzeichniseinträgen, bei einer Datei zusammen mit `-wal`, `-shm` und `-journal` daneben, bei einem Volume aus Dockers Speicherbilanz. Jeder vollständige Aufräumbericht misst Dateien und Ordner mit, **Alle messen** auch die Volumes; je Datenbank bleiben die letzten 100 Messungen. Eine überwachte Datenbank erscheint in keinem Papierkorb-Plan. Registrieren und messen darf auch die CLI; die Überwachung beenden (**„…“ nicht mehr überwachen**) nur du im Dashboard.

```powershell
& $cli db suggest
& $cli db register --path "C:\Program Files\PostgreSQL\17\data"
& $cli db register --path C:\dev\app\data\app.sqlite --name "App-Daten"
& $cli db register --docker-volume web_pgdata --context desktop-linux
& $cli db measure
& $cli db history --id 1
```

## 6. Erfassen und Tray

mogumogu erfasst nur, wenn du es auslöst: **Jetzt erfassen**, eine Lesefreigabe, „Projekte suchen“ oder `& $cli scan`. Es überwacht keine Ordner, startet nicht mit Windows und stellt keine Netzwerkverbindungen her.

**Tray.** Das Tray-Menü zeigt die letzte Erfassung, die Zahl offener Prüfhinweise, **Dashboard öffnen**, **Jetzt erfassen**, **Einstellungen …** und **mogumogu beenden**. Das Tray-Symbol zeigt nur „bereit“ oder „arbeitet“.

Paketversionen zeigen Abkündigungen, die ein lokales Lockfile vermerkt; Registries fragt mogumogu nicht ab. „Behalten“ in den Paketdetails ist eine umkehrbare Vormerkung; Lockfile-Hinweise bleiben sichtbar. Ohne Scanabdeckung und geklärten Zweck entsteht kein Urteil „ungenutzt“.

Frühere Versionen konnten mit Windows starten. Ein solcher Starteintrag, der genau diese mogumogu-Installation startet, wird beim nächsten Start einmal entfernt; ein fremder Eintrag gleichen Namens bleibt unberührt. Freigaben für WinGet-, Scoop- und Chocolatey-Listen werden einmal widerrufen; ihr letzter Stand bleibt als Historie in der Datenbank.

## 7. Daten und Sicherung

```powershell
& $cli backup --to D:\Sicherung\mogumogu-2026-10-04.sqlite3   # SQLite-Backup-API
```

Im Dashboard: **Einstellungen → Daten → Sicherung erstellen …** fragt nach einem Zielordner. Dateinamen tragen einen Zeitstempel; ein vorhandenes Ziel wird nie überschrieben.

Vor jeder Schemamigration legt mogumogu automatisch eine konsistente Sicherung unter `%LOCALAPPDATA%\mogumogu\backups` an. Eine ältere mogumogu-Version verändert eine neuere Datenbank nicht.

Deinstallation: `dist\mogumogu` löschen und optional `%LOCALAPPDATA%\mogumogu`. Registrierte Projekte, Scratchpads und Ergebnisse werden dabei nicht berührt.

## 8. Tastatur im Dashboard

`Strg+K` (auch `Strg+F`) Suche über alle Bereiche, `Enter` öffnet den ersten Treffer · `Strg+N` Projekt hinzufügen · `F5` Ansicht neu laden · `F1` Hilfe mit Tastenkürzeln und CLI-Befehlen · `Alt+1` … `Alt+6` Bereiche in der Reihenfolge der Seitenleiste (Übersicht bis Einstellungen) · `Tab`/`Umschalt+Tab` Fokus · `Leertaste`/`Enter` auslösen · `Esc` schließt Dialog, Suche oder Einstieg und führt von einer Projektseite zur Liste zurück.

Die Suche durchsucht nur bereits erfasste Metadaten (Projektnamen und -pfade, Paketnamen und Versionen, Ressourcen, KI-Einträge, Systembefund) und braucht mindestens zwei Zeichen.

Das Fenster startet mit 1080 × 680 logischen Pixeln und lässt sich bis 900 × 560 verkleinern. Die Befehlsleiste bleibt in allen Bereichen erreichbar; „Jetzt erfassen“ liest nur bereits freigegebene Bereiche. `F5` lädt die Ansicht neu und erteilt keine Lesefreigabe. Bei geöffnetem Dialog bleiben die übrigen Befehle gesperrt.

## 9. Mitentwickeln

```powershell
python scripts/validate.py                                   # statische Verträge
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-default-features                   # Kern, Windows-Grenzen, E2E
cargo test --release --no-default-features --test performance -- --ignored --nocapture
.\scripts\measure.ps1 -Cycles 100 -IdleSeconds 600            # Ressourcenmessung
.\scripts\release.ps1                                         # ZIP, Prüfsummen, Lizenzinventar
python scripts/sync_skills.py                                 # Skills für Claude Code kopieren
```

Architektur und Regeln: [README](../README.md), [AGENTS.md](../AGENTS.md). Grenzen je Manager und Profil: [Unterstützungsmatrix](UNTERSTUETZUNGSMATRIX.md).
