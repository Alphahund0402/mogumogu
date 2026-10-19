# Anleitung · mogumogu 0.1.0

Für Entwicklerinnen und Entwickler, die mogumogu auf Windows 10/11 nutzen oder mitentwickeln. Alle Befehle in PowerShell im Projektordner; `$cli` steht für `.\dist\mogumogu\mogumogu-cli.exe`.

## 1. Grundprinzip in drei Sätzen

Registrieren speichert nur Namen und Pfade. Lesen erfordert eine eigene, an die Ordneridentität gebundene **Lesefreigabe**. Entfernen ist nur über einen bestätigten **Plan** möglich und in dieser Version auf verwaltete Temp-Ausgabe in einer **Wegwerf-Testwurzel** beschränkt.

## 2. Starten

```powershell
.\build.ps1                       # bauen, testen, lokales Inventar im Tray starten
.\build.ps1 -ShowDashboard
.\start.ps1 -ShowDashboard        # bereits gebaut mit lokalen Daten starten
```

Der Tray-Prozess ist der **Besitzer**: Er hält die Datenbank, die lokale Pipe und optional die Überwachung. Fenster schließen beendet ihn nicht; „Beenden“ im Tray-Menü schon. Die CLI startet den Besitzer bei Bedarf selbst (Tray ohne Fenster); `--no-start` verhindert das. Für Automatisierung: `mogumogu.exe --headless`, beenden mit `$cli owner stop`.

Standard ist das lokale Inventar mit deinen registrierten und freigegebenen Projektordnern. Neue Inventare beginnen leer. Die schreibgeschützte Beispielvorschau startet nur ausdrücklich mit `-Mode Demo`. Demo- und lokale Datenbank sind getrennte Dateien unter `%LOCALAPPDATA%\mogumogu`. Zum Wechseln den Besitzer beenden und im anderen Modus starten.

### Aufbau des Dashboards

| Ansicht | Inhalt |
|---|---|
| **Übersicht** | Kennzahlen (Projekte, Paketversionen, KI-Dateien, Projektgröße), Entscheidungen auf anderen Seiten (hängende Scratchpad-Session, Abkündigungen, teilweise Erfassung, neu gefundene Orte) und darunter **Prüfhinweise**, **Letzte Änderungen** und – falls vorhanden – **Bereinigungspläne** |
| **Projekte** | Reiter **Projekte**, **Ressourcen** (Scratchpads mit Sessions, ihre Ordner, zugeordnete Funde) und **Funde** (Namenssuche in Suchordnern und Laufwerken sowie erfasste Umgebungen und Build-Ausgaben) |
| **Pakete** | Paketversionen mit Belegen und Quellenstatus; Reiter **Werkzeuge & globale Bestände** für den Systembefund |
| **KI-Konfiguration** | Gefundene Skills, Instruktionen, MCP-Server und Hooks; Reiter **Persönliche Ordner** wie `.claude` |
| **Speicher** | Laufwerke mit **Projekte suchen** je lokaler Festplatte, Projektgrößen und Ordnerstruktur |
| **Einstellungen** | Reiter **Allgemein** und **Lesefreigaben** (alle freigegebenen Ordner; erfassen, messen, widerrufen) |

Was eine Aktion ergeben hat (Erfassung, Laufwerkssuche, Quellenprüfung, Systemprüfung, Speicherstand, Sicherung, Export), steht danach zehn Sekunden lang in der Leiste über der Statuszeile. Nach einer Suche führt **Funde ansehen** direkt zu **Projekte → Funde**.

Ausgewählte Navigationseinträge, Filter und geöffnete Listenzeilen verwenden dieselbe blaue Fläche und dunkle Schrift; Häkchen bzw. eine Seitenmarkierung machen die Auswahl zusätzlich sichtbar. Die zuletzt geöffnete Zeile bleibt markiert. Der Tastaturfokus erscheint als Rahmen innerhalb des Elements und ist von der Auswahl getrennt.

Die Ansicht **Pakete** zeigt Paketversionen mit Projektbelegen, möglichen Verweisen, Abdeckung und Quellenstatus; ein Lockfile allein belegt keine Installation. Suche und Filterchips (Prüfen, Abgekündigt, Ohne Projektbeleg, Behalten) wirken vor dem Blättern auf den ganzen Bestand. Unterstützte Formate je Paketmanager nennt `& $cli adapters` und die [Unterstützungsmatrix](UNTERSTUETZUNGSMATRIX.md). Globale Paketbestände, persönliche KI-Konfiguration und eine reine Dateinamensuche nach Projekten haben eigene Lesefreigaben. Ablauf und Grenzen: [Paketprüfung](PAKETPRUEFUNG.md). Die Bestimmung eines Scratchpad-Zwecks bleibt offen.

### Geführter Einstieg

Ist das lokale Inventar leer, öffnet das Dashboard einen geführten Einstieg in drei Schritten: Ordner wählen, Lesefreigabe ansehen und erteilen, erfassen. Jeder Schritt zeigt vorher, was gelesen wird und was nie. „Später“ schließt ihn für diese Sitzung; „Nicht mehr anzeigen“ schaltet ihn ab. Wieder einschalten: **Einstellungen → Geführter Einstieg** oder `& $cli settings set guide on`. `Strg+N` und „Projekt hinzufügen“ starten ihn jederzeit.

Während einer Erfassung zeigt das Dashboard Fortschritt (Bereich *n* von *m*, gelesene Einträge). **Abbrechen** hält an der nächsten sicheren Grenze an; die Erfassung bleibt teilweise und der letzte vollständige Stand sichtbar. Danach nennt dieselbe Leiste das Ergebnis (z. B. „mogumogu erfasst · keine Änderungen seit der letzten Erfassung“) und schließt sich nach zehn Sekunden oder mit **Schließen**.

## 3. Projekte erfassen

```powershell
& $cli project register --name api --path C:\dev\api   # nur Metadaten
& $cli project approve --project 1                      # Lesefreigabe (identitätsgebunden)
& $cli scan                                             # statisch erfassen
& $cli list
& $cli why --resource 3                                 # Besitzer, Sessions, Schutz
```

Die erste vollständige Erfassung bildet eine **Vergleichsbasis** ohne Einzelmeldungen. Spätere Läufe melden gruppierte Änderungen. Ein Limit, ein unlesbarer Ordner oder ein fehlendes Laufwerk ergibt eine **teilweise** Erfassung: Der letzte vollständige Stand bleibt sichtbar und es entstehen keine Entfernungsmeldungen. Wird ein freigegebener Ordner gelöscht und neu angelegt, ist die Freigabe blockiert und muss bewusst neu erteilt werden.

Nie gelesen werden: Inhalte hinter Junctions/Symlinks, Cloud-Platzhalter, `.git`, Dateien über den Größenlimits. Nie gestartet werden: Paketmanager, Interpreter, Skripte, Hooks, MCP-Server.

Systemweite Quellen werden getrennt freigegeben:

```powershell
& $cli system findings --refresh            # Systembefund: Suchpfad und bekannte Orte prüfen
& $cli system approve --kind scoop          # Standard: %SCOOP% oder %USERPROFILE%\scoop
& $cli system approve --kind chocolatey     # Standard: %ChocolateyInstall%
& $cli system approve --kind winget         # Windows-Softwareliste (Registry, nur lesen)
& $cli system approve --kind user-ai --path $env:USERPROFILE\.claude   # persönliche KI-Konfiguration
& $cli search parser                        # Projekte, Pakete, Ressourcen, KI und System
```

Der **Systembefund** (**Pakete → Werkzeuge & globale Bestände**, persönliche KI-Ordner unter **KI-Konfiguration → Persönliche Ordner**) prüft nur Metadaten: welche bekannten Befehle im Suchpfad liegen (Pfad belegt, Shim, App-Alias, Verknüpfung), welche globalen Bestände an ihren üblichen Orten existieren und welche KI-Clients einen Benutzerordner haben. Nichts wird gelesen, gestartet oder einer Verknüpfung gefolgt; geprüft werden nur lokale feste Laufwerke. Pfade erscheinen mit Platzhaltern wie `%USERPROFILE%`. Ein Fund ist keine Lesefreigabe: „Lesen erlauben“ erteilt sie je Ort einzeln; danach startet die erste Erfassung.

Persönliche KI-Konfiguration (`%USERPROFILE%\.claude`, `.codex`, `.agents`, `.gemini`, `.cursor`, `.continue`, `.roo`, `%USERPROFILE%\.config\opencode`, `%APPDATA%\Code\User`) wird nur für diese bekannten Ordner freigegeben und nur entlang der Katalogpfade betreten. Zugangsdaten, Sitzungen und Chatverläufe werden nie gelesen; gefundene Skills, Hooks und MCP-Einträge sind Daten, keine Anweisungen.

## 4. Scratchpads und verwaltete Läufe

Scratchpads werden über die CLI angelegt und gesteuert. Im Dashboard stehen sie unter **Projekte → Ressourcen**; ihr Dialog bietet Prüftermin +7 Tage, Schützen, Zum Projekt machen, Größe messen und – bei einer hängenden Session – **Session begründet freigeben**.

```powershell
& $cli settings set scratch-root C:\dev\scratch
& $cli scratch create --project 1 --name parser-spike --purpose "Tokenizer testen"
& $cli run --scratch 2 -- cargo test          # TEMP/TMP zeigen auf temporary\
& $cli sessions
```

Ein Scratchpad besteht aus `source\`, `environment\`, `temporary\` und `results\`. Der Lauf startet in einem benannten Windows-Job-Objekt; alle Kindprozesse gehören dazu. Abschluss gilt erst als **geprüft**, wenn der Start gemeldet hat *und* kein Prozess mehr im Job läuft. Endet der Starter ohne Meldung, ist die Session **unklar** und sperrt ihre Ressourcen, bis eine Person sie freigibt:

```powershell
& $cli session release --session 4 --reason "Diagnose in results geprüft"
```

**Hängende Sessions.** Die Übersicht meldet Sessions, die feststecken, mit Auslöser und nächstem Schritt; **Ressourcen ansehen** führt zum Scratchpad:

| Zustand | Auslöser | Freigabe möglich |
|---|---|---|
| Unklar unterbrochen | Starter endete ohne Abschlussmeldung | ja, mit Begründung |
| Kein Lebenszeichen | Registrierte Session ohne Job und ohne PID, seit 1 Std. kein Signal | ja, mit Begründung |
| Abschluss ungeprüft | Abschluss gemeldet, seit 15 Min. nicht bestätigt | ja, wenn kein Prozess mehr lebt |
| Läuft sehr lange | Lauf seit über 24 Std. aktiv | nein, nur Hinweis |

„Begründet freigeben“ verlangt eine Begründung und ist gesperrt, solange im Job-Objekt oder unter der gemeldeten PID noch ein Prozess lebt. mogumogu beendet nie Prozesse; die Sperre bleibt, bis eine Person entscheidet.

Weitere Aktionen: `scratch adopt` (bestehenden Ordner übernehmen), `scratch protect`, `scratch extend --days 7`, `scratch promote` (zum Projekt machen; Ablaufregel entfällt, Historie bleibt). Integrationen ohne verwalteten Lauf können `session register` und `session complete` nutzen; das ist ein schwächerer Beleg.

`scratch-root` legt nur den Ort für neue Scratchpads fest. Bestehende Projekte und Scratchpads findet die Namenssuche: einen Suchordner über **Projekte → Funde → Suchordner hinzufügen …** (oder `& $cli system approve --kind projects --path C:\dev`) oder ein ganzes Laufwerk über **Speicher → Projekte suchen** (oder `& $cli system approve --kind projects --path D:\`), danach erfassen. Unter **Projekte → Funde** erscheinen Projekte, Skripte, Notebooks, Umgebungen, Ausgaben und Testdaten mit Belegen, dazu die bei der Projekterfassung erkannten Umgebungen und Build-Ausgaben; **Projekt zuordnen** bestätigt Besitzer und optionalen Zweck. Die Einstellung selbst löst keine Suche oder Lesefreigabe aus.

**Laufwerkssuche.** **Projekte suchen** auf einer Laufwerkskarte erteilt mit einem Klick eine Namenssuche für die ganze lokale Festplatte, gebunden an ihre Identität, und startet sie. Gelesen werden nur Ordner- und Dateinamen. Übersprungen werden Windows, Programme, ProgramData, Wiederherstellungs- und Updateordner, `Users\Default`, im Benutzerprofil `AppData`, `scoop`, `go\pkg` und Punktordner von Werkzeugen (`.cargo`, `.rustup`, `.claude` …) sowie Programmordner (`.exe` neben `.dll` ohne Projektmarker) und Spielebibliotheken. Ordner ohne Zugriffsrecht zählen als ausgelassen. Einzelne lose Skripte werden auf einem Laufwerk nicht gelistet; ab zwei Skripten oder einem Notebook in einem Ordner ohne Projektmarker erscheint der Ordner als mögliches Scratchpad. Ein Durchgang ist auf 2 Mio. Einträge, 32 Ebenen und drei Minuten begrenzt; reicht das nicht, bleibt er teilweise und zeigt den letzten vollständigen Stand. Die Freigabe steht unter **Einstellungen → Lesefreigaben** als „Laufwerk D:“ und lässt sich dort widerrufen. Netzlaufwerke, Wechselmedien und optische Laufwerke lassen sich nur ordnerweise durchsuchen; ein ganzes Laufwerk wird nie ausgemessen.

**Speicher** zeigt Laufwerke mit belegtem/freiem Speicher und gemessenen logischen Projektanteilen. **Größe messen** arbeitet nur in freigegebenen Projektbereichen. Unter **Ordnerstruktur** einen freigegebenen Ordner wählen und Unterordner öffnen. Suchfreigaben zeigen nur Namen; begrenzte oder fehlende Messungen bleiben unbekannt. **Aktualisieren** erneuert die Laufwerksmetadaten und die offene Ordneransicht.

In **KI-Konfiguration** einen Skill oder eine Instruktion öffnen und **Inhalt ansehen** wählen. Der Text ist schreibgeschützt und kann markiert/kopiert werden. Konfigurationsmischdateien werden nicht angezeigt; erkannte sensible Werte werden ausgeblendet. Umfang und Grenzen: [Entwicklungsfunde, Speicher und Skill-Inhalte](ENTWICKLUNGSFUNDE.md).

## 5. Prüfhinweise und Bereinigung

Im Dashboard stehen die Prüfhinweise auf der **Übersicht**; `& $cli review` zeigt Hinweise mit Auslöser, Beleg, Besitzer und Unsicherheit. Ein Hinweis ist kein Urteil. „Inaktiv“ wird nur behauptet, wenn mogumogu lange genug beobachtet hat — und beobachtet werden ausschließlich registrierte Sessions.

Ablauf des Testexecutors:

```powershell
# 1. Wegwerf-Testwurzel ausdrücklich markieren und registrieren
New-Item -ItemType Directory C:\mogumogu-test | Out-Null
Set-Content C:\mogumogu-test\.mogumogu-disposable-test-root 'mogumogu disposable test root'
& $cli cleanup test-root --path C:\mogumogu-test
& $cli settings set scratch-root C:\mogumogu-test\scratch

# 2. Nach einem abgeschlossenen Lauf: Temp-Ordner lokal als entbehrlich markieren
& $cli scratch expendable --resource 5

# 3. Plan entwerfen, prüfen, mit dem Fingerabdruck bestätigen, ausführen
& $cli cleanup plan --resource 5
& $cli cleanup approve --plan 1 --confirm 1a2b3c4d5e6f
& $cli cleanup apply --plan 1
```

Blocker statt stiller Auslassung: Jede Verknüpfung, jeder Cloud-Platzhalter und jede Datei `.mogumogu-keep` im Ziel verhindert den ganzen Plan. Vor der Ausführung wird alles erneut geprüft; neue, fehlende oder veränderte Dateien machen den Plan ungültig. Jede Entfernung wird vorher dauerhaft protokolliert und wirkt auf genau das Objekt, dessen Identität gerade geprüft wurde. Der Ordner `temporary\` selbst sowie `source\`, `results\` und `environment\` bleiben immer erhalten. Es gibt keine Quarantäne und keine Sicherung: Entfernen ist endgültig.

Nach einem Absturz während der Ausführung steht der Plan auf „Abgleich erforderlich“:

```powershell
& $cli cleanup reconcile --plan 1
```

Der Abgleich vermerkt, was tatsächlich fehlt, und wiederholt nie eine Entfernung. Für einen weiteren Versuch ist ein neuer Plan nötig.

Außerhalb einer registrierten Testwurzel ist Bereinigung bis zum unabhängigen Review (Gate G5) gesperrt. Das ist keine Einstellung und lässt sich nicht umgehen.

## 6. Überwachung, Hinweise, Updates

```powershell
& $cli settings set monitoring on        # entprellte Neuerfassung bei Dateiänderungen
& $cli settings set notifications on     # generischer Tray-Tooltip mit Anzahl der Hinweise
& $cli settings set updates on
& $cli settings set update-source registry.npmjs.org on
& $cli updates check
& $cli updates list
```

**Tray.** Das Tray-Menü zeigt eine Statuszeile (Überwachung, letzte Erfassung), die Zahl offener Prüfhinweise, ein Untermenü **Überwachung** (1 Stunde oder bis morgen 8 Uhr pausieren, fortsetzen, aus/an) und **Einstellungen …**. Neue Hinweise meldet das Tray-Symbol mit einem Punkt und einem generischen Tooltip – nie mit Pfaden oder Paketnamen. Windows-Benachrichtigungen (Toasts) gibt es nicht, weil Slints Tray-Schnittstelle keine anbietet. Hinweise werden gesammelt; das Öffnen des Dashboards quittiert sie.

```powershell
& $cli settings set pause 90             # Überwachung 90 Minuten anhalten; off setzt fort
& $cli settings set quiet-hours 19-8     # in diesen Stunden keine Markierung am Symbol
& $cli settings set daily-summary on     # höchstens einmal täglich melden …
& $cli settings set summary-hour 17      # … ab 17 Uhr
& $cli autostart on                      # Tray-Besitzer bei Anmeldung starten
```

Pausieren hält nur die automatische Neuerfassung an; „Jetzt erfassen“ funktioniert weiter. Nach Ablauf setzt die Überwachung von selbst fort. **Autostart** schreibt nur den Wert `mogumogu` unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (aktueller Benutzer, startet `mogumogu.exe --tray` ohne Fenster); kein Dienst, keine geplante Aufgabe. Ausschalten entfernt genau diesen Wert.

Updatehinweise fragen nur öffentliche Registries ab, deren Herkunft im Lockfile oder in der Umgebung belegt ist, und nur nach Freigabe des jeweiligen Hosts. Private, Git-, Pfad- und unbekannte Quellen werden nie gesendet. Nichts wird installiert.

Die Abfrage prüft auch versionsbezogene Abkündigungen/Yanks und Python-Projektstatus;
NuGet-Hinweise benötigen einen Cache-Beleg für `api.nuget.org` und dessen eigene
Freigabe. „Behalten“ in den Paketdetails ist eine umkehrbare Vormerkung; Warnungen
bleiben sichtbar. Ohne Scanabdeckung und geklärten Zweck entsteht kein Urteil „ungenutzt“.

## 7. Daten, Sicherung, Export

```powershell
& $cli backup --to D:\Sicherung\mogumogu-2026-10-04.sqlite3   # SQLite-Backup-API
& $cli export                     # ohne Pfade, Projektnamen und private Paketnamen
& $cli export --include-paths     # nur bewusst teilen
& $cli export --to D:\Teilen\mogumogu-export.json   # in eine neue Datei, nie überschreiben
```

Im Dashboard: **Einstellungen → Daten**. „Sicherung erstellen …“ fragt nach einem Zielordner. „Exportieren …“ zeigt vorher, was enthalten ist und was nicht (Systembefund, Sessions, Einstellungen, Freigaben und Dateiinhalte nie), bietet dann redigiert oder mit Pfaden an und fragt erst danach nach dem Ordner. Dateinamen tragen einen Zeitstempel; ein vorhandenes Ziel wird nie überschrieben.

Vor jeder Schemamigration legt mogumogu automatisch eine konsistente Sicherung unter `%LOCALAPPDATA%\mogumogu\backups` an. Eine ältere mogumogu-Version verändert eine neuere Datenbank nicht.

Deinstallation: `dist\mogumogu` löschen und optional `%LOCALAPPDATA%\mogumogu`. Registrierte Projekte, Scratchpads und Ergebnisse werden dabei nicht berührt.

## 8. Tastatur im Dashboard

`Strg+K` (auch `Strg+F`) Suche über alle Bereiche, `Enter` öffnet den ersten Treffer · `Strg+N` geführter Einstieg (Projekt hinzufügen) · `F5` Ansicht neu laden · `F1` Hilfe mit Tastenkürzeln und CLI-Befehlen · `Alt+1` … `Alt+6` Bereiche in der Reihenfolge der Seitenleiste (Übersicht bis Einstellungen) · `Tab`/`Umschalt+Tab` Fokus · `Leertaste`/`Enter` auslösen · `Esc` schließt Dialog, Suche oder Einstieg.

Die Suche durchsucht nur bereits erfasste Metadaten (Projektnamen und -pfade, Paketnamen und Versionen, Ressourcen, KI-Einträge, Systembefund) und braucht mindestens zwei Zeichen.

Das Fenster startet mit 1080 × 680 logischen Pixeln und lässt sich bis 900 × 560 verkleinern. Die Befehlsleiste bleibt in allen Bereichen erreichbar; „Jetzt erfassen“ liest nur bereits freigegebene Bereiche. `F5` lädt die Ansicht neu und erteilt keine Lesefreigabe. Bei geöffnetem Dialog bleiben die übrigen Befehle gesperrt.

## 9. Mitentwickeln

```powershell
python scripts/validate.py                                   # statische Verträge
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-default-features --features network # Kern, Windows-Grenzen, E2E
cargo test --release --no-default-features --test performance -- --ignored --nocapture
.\scripts\measure.ps1 -Cycles 100 -IdleSeconds 600            # Ressourcenmessung
.\scripts\release.ps1                                         # ZIP, Prüfsummen, Lizenzinventar
python scripts/sync_skills.py                                 # Skills für Claude Code kopieren
```

Architektur und Regeln: [README](../README.md), [AGENTS.md](../AGENTS.md). Grenzen je Manager und Profil: [Unterstützungsmatrix](UNTERSTUETZUNGSMATRIX.md).
