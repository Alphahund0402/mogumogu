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

Ausgewählte Navigationseinträge, Filter und geöffnete Listenzeilen verwenden dieselbe blaue Fläche und dunkle Schrift; Häkchen bzw. eine Seitenmarkierung machen die Auswahl zusätzlich sichtbar. Die zuletzt geöffnete Zeile bleibt markiert. Der Tastaturfokus erscheint als Rahmen innerhalb des Elements und ist von der Auswahl getrennt.

„Erfasste Pakete“ zählt unterschiedliche aufgelöste oder installierte Pakete aus vollständigen Erfassungen. Die Paketmanager-Ansicht unterscheidet deklarierte, aufgelöste und installierte Einträge; ein Lockfile allein belegt keine Installation.

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
& $cli system approve --kind scoop          # Standard: %SCOOP% oder %USERPROFILE%\scoop
& $cli system approve --kind chocolatey     # Standard: %ChocolateyInstall%
& $cli system approve --kind winget         # Windows-Softwareliste (Registry, nur lesen)
```

## 4. Scratchpads und verwaltete Läufe

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

Weitere Aktionen: `scratch adopt` (bestehenden Ordner übernehmen), `scratch protect`, `scratch extend --days 7`, `scratch promote` (zum Projekt machen; Ablaufregel entfällt, Historie bleibt). Integrationen ohne verwalteten Lauf können `session register` und `session complete` nutzen; das ist ein schwächerer Beleg.

## 5. Prüfliste und Bereinigung

`& $cli review` zeigt Hinweise mit Auslöser, Beleg, Besitzer und Unsicherheit. Ein Hinweis ist kein Urteil. „Inaktiv“ wird nur behauptet, wenn mogumogu lange genug beobachtet hat — und beobachtet werden ausschließlich registrierte Sessions.

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

Updatehinweise fragen nur öffentliche Registries ab, deren Herkunft im Lockfile oder in der Umgebung belegt ist, und nur nach Freigabe des jeweiligen Hosts. Private, Git-, Pfad- und unbekannte Quellen werden nie gesendet. Nichts wird installiert.

## 7. Daten, Sicherung, Export

```powershell
& $cli backup --to D:\Sicherung\mogumogu-2026-10-04.sqlite3   # SQLite-Backup-API
& $cli export                     # ohne Pfade, Projektnamen und private Paketnamen
& $cli export --include-paths     # nur bewusst teilen
```

Vor jeder Schemamigration legt mogumogu automatisch eine konsistente Sicherung unter `%LOCALAPPDATA%\mogumogu\backups` an. Eine ältere mogumogu-Version verändert eine neuere Datenbank nicht.

Deinstallation: `dist\mogumogu` löschen und optional `%LOCALAPPDATA%\mogumogu`. Registrierte Projekte, Scratchpads und Ergebnisse werden dabei nicht berührt.

## 8. Tastatur im Dashboard

`Strg+K` Suche · `Alt+1` … `Alt+8` Bereiche · `Tab`/`Umschalt+Tab` Fokus · `Leertaste`/`Enter` auslösen · `Esc` Dialog schließen.

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
