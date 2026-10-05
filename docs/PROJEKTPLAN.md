# mogumogu — Projektplan v0.1

**Dokumentversion:** 0.1  
**Stand:** 4. Oktober 2026  
**Plattformen:** Windows 10 und Windows 11  
**Technischer Ansatz:** Rust · Slint · SQLite  
**Veröffentlichung:** Open Source unter GPLv3  
**Status:** Planungsgrundlage; Umsetzung und technische Abnahme stehen aus.

> **Arbeit behalten. Entwicklungsreste verstehen und kontrolliert entfernen.**

## Kurzüberblick

mogumogu ist eine lokale Windows-Desktopanwendung, die im Systemtray Entwicklungsressourcen überwacht: installierte Abhängigkeiten, Projektumgebungen, Scratchpads, temporäre Dateien, Buildausgaben und KI-bezogene Projektkonfiguration. Sie erklärt, woher Ressourcen kommen, wem sie gehören, welche Nutzung beobachtet wurde und welche Teile sich kontrolliert entfernen lassen.

Das Produkt kombiniert **passive Erkennung bestehender Ressourcen** mit **generischer CLI-Registrierung neuer Experimente**. Es verlangt weder einen bestimmten Editor noch einen Wechsel des Paketmanagers. Neun Paketmanager und ein erweiterbarer Katalog gängiger KI-Konfigurationen gehören zum Umfang der ersten vollständigen Beta.

Die Umsetzung beginnt mit einem kleinen vollständigen Arbeitsablauf. Eine sichere, begrenzte Testbereinigung wird früh erprobt; produktive Löschfunktionen werden erst nach gesonderter Abnahme freigeschaltet. Ein Inventar ist kein Löschrecht, ein alter Ordner kein Entbehrlichkeitsnachweis und ein fehlender Nutzungsbeleg kein Beweis für Nichtbenutzung.

Dieser Plan enthält Produktumfang, technische Entscheidungen, Sicherheitsverträge, Datenmodell, Meilensteine, Arbeitspakete und Abnahmekriterien. Zusätzliche Planungsdokumente werden zum Verständnis nicht vorausgesetzt.

### Verbindlichkeit

**Festgelegt** sind Produktname, Windows 10/11, Rust, SQLite, geringer Overhead, die genannten Paketmanager, einfache Erweiterbarkeit, passive Erkennung, CLI-Registrierung, KI-Ressourcen und Open-Source-Zusammenarbeit unter GPLv3.

**Empfohlene Umsetzungsentscheidungen** sind unter anderem Slint, das Prozessmodell, die genaue Windows-Testbasis und die nachstehenden Betriebsgrenzen. Sie bilden die technische Ausgangslage. Änderungen werden mit Begründung, Auswirkung und Abnahme dokumentiert.

**Messziele** sind keine zugesagten Ist-Werte. Alle Implementierungsaufgaben beginnen im Status „geplant“. Version 0.1 bezeichnet dieses Dokument, nicht den Fertigstellungsgrad einer Anwendung.

## Inhaltsverzeichnis

1. [Produktziel und Leitlinien](#ziel)
2. [Lieferumfang und Grenzen](#umfang)
3. [Funktionale Anforderungen](#anforderungen)
4. [Empfohlene technische Entscheidungen](#entscheidungen)
5. [Architektur und Windows-Betrieb](#architektur)
6. [Inventar, Datenmodell und SQLite](#daten)
7. [Sicherheit und Datenschutz](#sicherheit)
8. [Erweiterbare Adapter und KI-Profile](#adapter)
9. [Scratchpads, Sessions und CLI](#sessions)
10. [Prüfkandidaten und Bereinigung](#cleanup)
11. [Oberfläche, Benachrichtigungen und Updates](#ui)
12. [Performance und Ressourcenbudgets](#performance)
13. [Open Source, Zusammenarbeit und Entwicklungsskills](#opensource)
14. [Meilensteine und Freigaben](#meilensteine)
15. [Umsetzungsbacklog](#backlog)
16. [Teststrategie und Abnahme](#abnahme)
17. [Risiken, Verantwortung und Projektstart](#start)
18. [Technische Referenzen](#referenzen)

<a id="ziel"></a>
## 1. Produktziel und Leitlinien

### 1.1 Welches Problem mogumogu löst

Entwicklungsarbeit hinterlässt Ressourcen innerhalb und außerhalb von Projektordnern. Einige werden weiterhin gebraucht, andere lassen sich regenerieren, wieder andere enthalten einzigartige Arbeit. mogumogu soll diese Unterschiede sichtbar machen und Entscheidungen auf Projekt- und Experimentebene ermöglichen.

Die wichtigsten Fragen lauten:

> Was ist das? Wo liegt es? Warum ist es vorhanden? Was benötigt es noch? Welche Aktivität wurde tatsächlich beobachtet? Was würde eine Entfernung betreffen?

Die Zielgruppe sind Entwicklerinnen und Entwickler sowie kleine Open-Source-Teams, die ihren eigenen Rechner verwalten. Jeder Rechner behält sein Inventar und seine Freigaben lokal.

### 1.2 Durchgängige Arbeitsabläufe

| Arbeitsablauf | Gewünschtes Ergebnis |
|---|---|
| Bestehendes Projekt erfassen | Abhängigkeiten, Umgebungen und zugehörige Artefakte werden mit Herkunft, Besitzer und Abdeckungslücken angezeigt. |
| Neues Experiment verfolgen | Eine CLI-Registrierung verbindet Testlauf, Scratchpad, Umgebung, temporäre Dateien und Ergebnisse. |
| Experiment beenden | Ein selektiver Plan erhält die Arbeit und bietet die Entfernung ausdrücklich entbehrlicher Ressourcen an. |
| Projekt stilllegen | Generierte Ressourcen können entfernt werden, während Quellcode, Spezifikationen und Ergebnisse erhalten bleiben. |
| KI-Konfiguration verstehen | Skills, Instruktionen, Einstellungen und Referenzen werden ohne Aktivierung inventarisiert. |
| Unterstützung ergänzen | Ein neuer Manager oder ein neues Dateiformat verwendet die bestehenden Verträge, ohne die Oberfläche neu zu entwerfen. |

### 1.3 Leitlinien

**Herkunft vor Vermutung.** Explizite Registrierung, Paketmetadaten und passiv abgeleitete Beziehungen werden unterschieden. Zeitliche Nähe zu einem Prozess ist kein eindeutiger Erstellernachweis.

**Unsicherheit sichtbar machen.** „Nicht erfasst“, „unbekannt“, „nicht erreichbar“ und „nicht installiert“ sind verschiedene Aussagen. Die eigene Überwachung zählt nicht als Nutzung.

**Aufbewahren ist der sichere Ausgangszustand.** Unbekannte Inhalte, Quellcode, lokale Konfiguration, Ergebnisse und KI-Quellen bleiben geschützt. Eine Suche nach Ressourcen autorisiert keine Veränderung dieser Ressourcen.

**Wenig Arbeit im Leerlauf.** Keine dauernden Vollscans, keine zweite KI-Scanschleife und keine ständig aktive Dashboardansicht im Hintergrund.

**Fähigkeiten getrennt freigeben.** Erkennung, Updateprüfung, Ausführung von Werkzeugen und Bereinigung erhalten eigene Tests und Freigaben.

<a id="umfang"></a>
## 2. Lieferumfang und Grenzen

### 2.1 Plattform und Arbeitsweise

Windows 10 und Windows 11 sind festgelegt. Empfohlene erste Testbasis sind **Windows 10 22H2 x64** sowie konkret dokumentierte **Windows-11-x64-Builds**. Editionen, Buildnummern und Abhängigkeiten werden vor der Implementierung in der Testmatrix festgehalten. Anwendungskompatibilität ist keine Aussage über den Sicherheits- oder Supportstatus des Betriebssystems.[^t01]

Die erste vollständige Dateisystemabdeckung gilt für freigegebene lokale NTFS-Verzeichnisse. ARM64, WSL, Netzlaufwerke und synchronisierte Cloudordner erhalten zunächst keine pauschale Vollunterstützung. Nicht unterstützte Orte bleiben als solche sichtbar; insbesondere darf ein Scan nicht unbemerkt Cloudinhalte herunterladen.

Normaler Betrieb erfolgt als angemeldeter Benutzer ohne Administratorrechte. Ein Konto, eine Cloudverbindung oder Telemetrie sind nicht erforderlich. Generische Registrierung und passive Erkennung funktionieren unabhängig vom eingesetzten Editor oder KI-Client.

### 2.2 Paketmanager der vollständigen Beta

| Manager | Vorgesehener Inventarumfang | Wichtige Abgrenzung |
|---|---|---|
| **npm** | Projekte, Workspaces, Manifeste, Lockdateien, lokale und globale Installationen | Deklariert, aufgelöst und installiert getrennt darstellen. |
| **pnpm** | Projekte, Workspaces, Lockdateien, Installationen, Store- und Linkbeziehungen | Gemeinsamen Store nicht als exklusiven Projektbesitz behandeln. |
| **pip** | Projekt-/Requirements-Metadaten, `dist-info`, virtuelle Umgebungen und bekannte Installationen | Keine automatische Ausführung eines gefundenen Interpreters; dynamische Angaben gegebenenfalls unbekannt. |
| **uv** | Projekte, Locks, virtuelle und Toolumgebungen, Skriptmetadaten, Cachebeziehungen | Quellen und Cachezuordnung getrennt von der Projektumgebung modellieren. |
| **Cargo** | Workspaces, Manifeste, Locks, bekannte Feature-/Zielkontexte, Buildausgaben und globale Toolnachweise | Aufgelöste Crates, Quellcache und gebaute Artefakte nicht gleichsetzen. |
| **NuGet** | Projekt- und zentrale Paketmetadaten, Lock-/Assets-Dateien, Frameworkkontexte und globale Paketspeicher | Kein Restore oder Build allein zum Inventarisieren; Bedingungen können nur partiell bekannt sein. |
| **WinGet** | Installationsnachweise, Versionen und Quellenzuordnung im getesteten Umfang | Ein weiterer Nachweis ist nicht automatisch eine weitere Installation oder exklusiver Verwaltungsbesitz. |
| **Scoop** | Installierte Pakete, Versionen, Herkunft sowie bekannte Cache-/Persistenzbezüge | Paket- und Installationsskripte beim Scan nicht ausführen; persistente Daten erhalten. |
| **Chocolatey** | Installierte Paketnachweise, Versionen und Herkunft im getesteten Umfang | Ausgabeformate versionieren; kein Paket-PowerShell während der Discovery. |

Jeder Manager wird pro Fähigkeit und getesteter Version ausgewiesen. Der Pilot unterstützt eine Teilmenge; die vollständige Beta umfasst alle neun Manager. Eine bewusst beschlossene Einschränkung muss als Umfangsabweichung sichtbar sein.

Updatehinweise gehören ebenfalls zum Produkt. Für jeden Manager wird mindestens der sichere Standardquellenweg der getesteten Variante umgesetzt oder eine ausdrücklich beschlossene Einschränkung dokumentiert. Unbekannte, lokale, private oder Git-basierte Quellen werden nicht durch erfundene öffentliche Registryziele ersetzt. Updatehinweise sind keine automatische Installation.

Die physische Installation muss getrennt vom Abhängigkeitsmodell betrachtet werden. Dies ist beispielsweise für npm und die verlinkten Strukturen von pnpm relevant.[^t02][^t03]

### 2.3 Weitere Ressourcen

Zum Kernumfang gehören Scratchpads, einzelne registrierte Dateien, temporäre Verzeichnisse, Build- und Testausgaben, Berichte, Diagnosen und unterstützte Caches. Die Funktionen „Datei/Ordner verfolgen“, „Besitzer zuordnen“, „Schützen“ und „Prüftermin setzen“ stehen auch ohne spezialisierten Adapter zur Verfügung.

Runtimes, SDKs, Toolchains, Container, Worktrees, Modelle und Retrieval-Indizes können zunächst generisch registriert werden. Eine solche Registrierung verspricht weder vollständige Erkennung ihrer internen Struktur noch sichere automatische Bereinigung.

### 2.4 KI-Ressourcen

Die Beta erfasst statisch und geschützt das Agent-Skills-Format sowie lokale Projektressourcen der folgenden Clientfamilien: **Codex, Claude Code, GitHub Copilot/VS Code, Cursor, Gemini CLI, Windsurf/Cascade, Cline, Roo Code, OpenCode, Continue und Aider**.

Dazu gehören Skills, Instruktionen, Regeln, Promptvorlagen, Agentdefinitionen, MCP-Definitionen, Hooks, Berechtigungen, Providerreferenzen, Workflows und unterstützte Pluginmanifeste. Die genaue Pfad- und Versionsmatrix steht in den ausgelieferten Profilen, nicht als universelle Annahme im Kern.

Vollständige Rekonstruktion wirksamer Laufzeiteinstellungen wird nicht für alle Clients zugesagt. Organisationsregeln, Cloudzustände, Kommandozeilenargumente und temporäre Overrides können fehlen.

### 2.5 Nichtziele der ersten Beta

Keine pauschale Windows-Temp- oder Registry-Bereinigung; keine automatische globale Deinstallation; keine automatische Änderung von Manifesten oder Lockdateien; keine unbeaufsichtigten Paketupdates; keine Löschung unbekannter persistenter Daten; keine Aktivierung gefundener Skills, Hooks oder MCP-Server; keine universelle Prozess-/Dateizugriffsüberwachung; keine beliebigen ausführbaren Fremdplugins; keine Cloud-Synchronisation.

Importanalyse, spezialisierte Container-/WSL-Verwaltung, native globale Cachebereinigung und ein eigener mogumogu-MCP-Server sind mögliche spätere Erweiterungen. Sie sind keine Voraussetzung des Starts.

<a id="anforderungen"></a>
## 3. Funktionale Anforderungen

Diese Anforderungen definieren den Funktionsumfang. Die folgenden Kapitel präzisieren ihre Umsetzung.

| ID | Anforderung | Erforderliches Verhalten |
|---|---|---|
| F-01 | Tray und Dashboard | Überwachung bei geschlossenem Dashboard; klar getrennte Aktionen für Öffnen, Pause, Fortsetzen und Beenden. |
| F-02 | Freigegebene Erkennung | Suchwurzeln, Ausschlüsse und Abdeckung verwalten; neue Bereiche mit eigener Baseline beginnen. |
| F-03 | Abhängigkeitsinventar | Paketidentität, Installation, deklarierte/aufgelöste Zustände sowie managerabhängige Kontexte unterscheiden. |
| F-04 | Besitzer und Erklärung | Mehrere Besitzer, Evidenz und Abhängigkeitsketten darstellen; Zuordnungen bestätigen oder korrigieren. |
| F-05 | Beobachtung und Prüfbedarf | Historie und Beweisstärke erhalten; Inaktivität, Ablauf, mögliche Verwaisung und Unsicherheit getrennt bewerten. |
| F-06 | Scratchpadverwaltung | Erstellen, entdecken, übernehmen, schützen, verlängern, zum Projekt machen und selektiv beenden. |
| F-07 | Sessions und CLI | Wiederholbare Registrierung, verwaltete Läufe, Ergebnisse und aktive Nutzungssperren mit strukturiertem Protokoll. |
| F-08 | Beliebige Artefakte | Dateien und Ordner mit Zweck, Besitzer, Aufbewahrung und Schutz registrieren; einzigartige und generierte Inhalte unterscheiden. |
| F-09 | Änderungen und Meldungen | Vergleichbare Inventarstände auswerten; Ereignisse gruppieren, filtern und ohne Baseline-Meldungsflut anzeigen. |
| F-10 | Updates | Quellenbezogene, separat aktivierte Prüfungen mit Datum, Einschränkungen und Offlinezustand; keine automatische Installation. |
| F-11 | KI-Inventar | Skills und Einstellungen statisch klassifizieren; Scope, Referenzen und Grenzen ohne Aktivierungsbehauptung zeigen. |
| F-12 | Erweiterbarkeit | Statische Profile und getestete Paketadapter über versionierte, begrenzte Verträge ergänzen. |
| F-13 | Lokale Regeln und Schutz | Schutzvorrang, Prüfzeiträume und lokale Zustimmung; „nicht beobachten“ und „nicht entfernen“ getrennt behandeln. |
| F-14 | Bereinigungsplanung | Ziele, Voraussetzungen, Besitzer, erhaltene Inhalte, Speicherwirkung und Wiederherstellung vor Freigabe erklären. |
| F-15 | Kontrollierte Ausführung | Aktuelle Identität prüfen, dauerhaft protokollieren, Teilfehler und Wiederanlauf ohne blindes Wiederholen behandeln. |
| F-16 | Projekt stilllegen | Ausgewählte generierte Ressourcen entfernen, Quellen und Ergebnisse erhalten; Rekonstruktion ehrlich kennzeichnen. |
| F-17 | Größe, Export und eigene Daten | Gemeinsamen Speicher berücksichtigen, redigierte Exporte ermöglichen und eigene Historie begrenzen. |
| F-18 | Zusammenarbeit und Release | Gemeinsame Regeln/Skills in Git, lokale Inventare und Freigaben, dokumentierte GPLv3-Veröffentlichung und sichere Beitragswege. |

<a id="entscheidungen"></a>
## 4. Empfohlene technische Entscheidungen

| ID | Entscheidung | Begründung und Prüfkriterium |
|---|---|---|
| E-01 | **Rust für Kern und CLI; Slint für die Oberfläche** | Kein Webfrontend als Grundvoraussetzung. Kern bleibt unabhängig vom UI. Tray, große Tabellen und Fensterfreigabe werden früh gemessen. |
| E-02 | **Ein normaler Benutzerprozess** | Tray, Scheduler und Inventar teilen einen Kern; kein Windows-Dienst und kein lokaler HTTP-Server. |
| E-03 | **Eine Instanz pro Benutzer und Datenverzeichnis** | Verhindert konkurrierende Inventarbesitzer. IPC ist auf die aktive Anmeldesitzung beschränkt. |
| E-04 | **SQLite über `rusqlite`** | Lokale eingebettete Datenhaltung, keine zusätzliche Graph- oder Serverdatenbank. Engine und Features werden im Release nachgewiesen. |
| E-05 | **Statische Erkennung als Standard** | Externe Inspektion, Netzwerk und Testläufe sind gesonderte Ausführungswege mit eigenen Freigaben. |
| E-06 | **Ereignisbasiert und begrenzt arbeiten** | Gezielte Abgleiche, begrenzte Queues und faire Worker statt periodischer Vollscans. |
| E-07 | **Adaptervertrag vor Adapterbreite** | Alle Manager verwenden dieselben Sicherheits- und Testkonventionen, aber ihre eigene Fachsemantik. |
| E-08 | **Deklarative Profile ohne Codeausführung** | Neue Formate innerhalb vorhandener Parser ergänzen; neue Fachsemantik benötigt einen geprüften Host-/Adapterwechsel. |
| E-09 | **Explizite Session- und Aktionszustände** | Fehlender Heartbeat, Prozessende oder Absturz dürfen nicht still Bereinigung erlauben. |
| E-10 | **Observe und Review statt automatischer Bereinigung** | Produktive Löschfähigkeit nur pro Ressourcentyp und nach unabhängiger kritischer Prüfung. |
| E-11 | **Typisierte, minimierte Metadaten** | Rohinhalt, lokales Inventar und Export sind getrennte Datenwege. Keine standardmäßige Freitexthistorie. |
| E-12 | **GPLv3 mit empfohlenem SPDX-Ausdruck `GPL-3.0-only`** | GPLv3 ist festgelegt; „oder später“ wird nicht ohne eigene Entscheidung ergänzt. Abhängigkeitslizenzen gehören zur Releaseabnahme. |
| E-13 | **Gemeinsame Quellen, lokale Autorisierung** | Git transportiert Vorschläge und Entwicklungsregeln, aber weder Inventardatenbanken noch Löschrechte. |
| E-14 | **Früher vollständiger Pilot** | Nutzbarkeit und Risiken vor neun Managerimplementierungen prüfen, ohne deren Betaumfang zu reduzieren. |
| E-15 | **Versionierte Releases, manuelle Anwendungsupdates** | Zunächst Release-ZIP, anschließend getesteter Benutzerinstaller. Kein eigener Self-Updater in der ersten Beta. |

Slint dokumentiert einen Traybetrieb ohne Dashboardfenster. Die konkrete veröffentlichte Version, der Renderer und die Featureauswahl werden dennoch im Machbarkeitsprototyp festgelegt.[^t04] Die GPLv3- und Slint-Lizenzbedingungen sind mit dem tatsächlich ausgelieferten Quell- und Abhängigkeitsstand abzugleichen.[^t05][^t06]

<a id="architektur"></a>
## 5. Architektur und Windows-Betrieb

### 5.1 Prozess- und Oberflächenlebenszyklus

Der Besitzerprozess hält Tray, Arbeitsplanung und Inventar. Er erzeugt das Dashboard erst bei Bedarf. Beim Schließen werden Ansichten, Abonnements und freigebbare Renderressourcen abgegeben. Ein unsichtbares Fenster zählt nicht automatisch als freigegebene Oberfläche.

Langsame Arbeit läuft außerhalb des UI-Threads. Ein fester, begrenzter Workerpool verarbeitet Scans und Parsing; der Datenbankschreibpfad ist koordiniert. Es gibt weder einen Thread pro Projekt noch eine unabhängige zweite Pipeline für KI-Dateien. Eine zusätzliche Async-Laufzeit wird nur für einen nachgewiesenen Bedarf aufgenommen.

### 5.2 Geplanter Rust-Workspace

```text
mogumogu/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  LICENSE
  README.md
  CONTRIBUTING.md
  SECURITY.md
  AGENTS.md
  .gitignore
  crates/
    mogumogu-core/       # Ressourcen, Beziehungen, Regeln, Zustandsautomaten
    mogumogu-storage/    # SQLite, Generationen, Migrationen, Aktionsjournal
    mogumogu-windows/    # Pfade, Identitäten, Prozesse, IPC, Shellintegration
    mogumogu-adapters/   # Paketmanager und ihre Fachsemantik
    mogumogu-profiles/   # Statische Erkennung und begrenzte Parser
    mogumogu-desktop/    # Slint, Tray, Darstellung und Interaktion
    mogumogu-cli/        # Kurzlebiger Client, Registrierung und JSON-Ausgabe
  profiles/
  schemas/
  migrations/
  tests/
    fixtures/
    contracts/
    windows/
    recovery/
    performance/
  .agents/skills/
  .github/workflows/
  docs/
    decisions/
    support-matrix.md
    release-checklist.md
```

Die Struktur ist ein Implementierungsziel, kein vorhandener Quellcode. `windows`, `rusqlite` und zunächst `notify` sind die vorgesehenen Anbindungen. Konkrete Versionen werden zusammen mit Rust-Toolchain, Slint-Backend und SQLite-Engine festgeschrieben. Analyse beginnt im Kern; zusätzliche Crates entstehen erst bei einer sinnvollen Schnittstellengrenze.

UI und CLI dürfen Dateien nicht direkt löschen. Sie rufen dieselben validierten Anwendungsfälle im Kern auf.

### 5.3 Lokale Kommunikation

Die CLI verbindet sich über eine Windows Named Pipe mit dem Besitzerprozess. Die Schnittstelle besitzt explizite Zugriffsrechte, Remotezurückweisung, eine Protokollversion, Request-IDs, Größenlimits und begrenzte gleichzeitige Anfragen. Die Pipe wird auf den vorgesehenen Benutzer und seine Anmeldesitzung eingeschränkt.[^t07]

Läuft noch keine Instanz, darf die CLI sie ohne Dashboard starten. Läuft der Besitzer in einer anderen Sitzung desselben Benutzers, wird dies verständlich gemeldet; es entstehen weder ein zweiter Writer noch ein Startloop oder erweiterte Pipe-Rechte.

Registrierung, Programmstart, Schutzänderung und Planbestätigung sind getrennte Befehle. Eine registrierende Integration darf keine globale Autorisierung anfordern. IPC-Rechte sind keine Sandbox gegen einen bereits kompromittierten Prozess desselben Benutzers.

### 5.4 Datenorte und Beenden

SQLite-Datenbank, Freigaben und redigierte Logs liegen im lokalen Benutzer-Datenverzeichnis von mogumogu, nicht im Projekt oder Cloudordner. Verwaltete Scratchpads erhalten eine separat freigegebene Wurzel. Das Programm ermittelt Systempfade über die Betriebssystemintegration statt fest codierte Benutzernamen vorauszusetzen.

Start bei Anmeldung ist optional. „Dashboard schließen“, „Überwachung pausieren“ und „mogumogu beenden“ sind getrennt. Beim Beenden werden laufende Nutzerprozesse nicht still beendet; offene Sessions werden bei erneutem Start überprüft.

<a id="daten"></a>
## 6. Inventar, Datenmodell und SQLite

### 6.1 Logisches Modell

| Entität | Wesentliche Felder |
|---|---|
| Projekt / Suchbereich | ID, freigegebene Wurzeln, Freigabeversion, Workspaces, Abdeckung |
| Ressource / Ressourcenort | ID, Typ, bekannte Orte, überprüfte Datei-/Volume-Identität, Verfügbarkeit |
| Paket / Installation | Ökosystem, Name, Version, Quelle, Umgebung, deklarierter/aufgelöster/installierter Zustand |
| Besitzer- und Abhängigkeitsbeziehung | Quell-/Ziel-ID, Art, Beleg, Kontext, letzte Validierung |
| Scan / Beobachtung | Generation, Scope, Adapter-/Profilversion, Vergleichbarkeit, Ergebnis, Fehler |
| Session / Nutzungssperre | Zweck, Ursprung, Prozessbezug, Ergebnis, Zustand, Ressourcen und Unsicherheit |
| KI-Artefakt / Referenz | Typ, Verbraucher, Scope, erlaubte Metadaten, Referenz und bekannte Wirksamkeit |
| Regel / Prüfhinweis | Schutz, Aufbewahrung, Auslöser, Belege, nächste Handlung und Blocker |
| Bereinigungsplan / Teilaktion | Zielidentität, Operation, Voraussetzungen, Freigabe, Ausführungszustand und Recovery |
| Update / Meldung | Quelle, Prüfergebnis, Zeit, Einschränkung, Gruppierung und bereits gemeldeter Zustand |

Besitz, Aktivität, Verfügbarkeit, Schutz und Bereinigungsfähigkeit bleiben unabhängige Eigenschaften. Ein alter Pfad vererbt weder Historie noch Löschrecht an ein neues Verzeichnis. Unsichere Identitätszuordnungen benötigen eine neue Entscheidung.

Gleiche Inhalte sind ein Duplikathinweis, keine gemeinsame Eigentümerschaft. Eine Ressource mit mehreren Managern oder KI-Verbrauchern soll nicht mehrfach als exklusiver Speicher gezählt werden. Ein globales Tool ist nicht automatisch verwaist, weil kein Projektmanifest es nennt.

### 6.2 Zeit und Beweisstärke

Erstbeobachtung, bekannte Erstellung, letzte erfolgreiche Erfassung und letzte relevante Nutzung werden getrennt gespeichert. Beobachtungslücken bleiben sichtbar. Eine registrierte Testausführung belegt Aktivität der erfassten Umgebung, nicht aller darin enthaltenen Pakete oder Skills.

„Keine relevante Aktivität während des beobachteten Zeitraums“ ist eine zulässige Aussage. „Seit Dateialter unbenutzt“ ist keine zulässige Ableitung.

### 6.3 Erfassungsgenerationen

Jeder Scope/Adapter erhält Generationen mit den Zuständen `running`, `partial`, `complete`, `failed` und `superseded`. Zum Vergleichskontext gehören Scopefreigabe, Adapter-/Profilversion und relevante Format-/Konfigurationsinformationen.

Scans schreiben begrenzte Ergebnisbatches in die neue Generation. Erst bei ausreichender Vollständigkeit wird der veröffentlichte Stand atomar umgestellt. Der letzte vollständige Stand bleibt während einer partiellen Erfassung verfügbar und als älter gekennzeichnet.

Entfernungsmeldungen entstehen nur zwischen geeigneten vollständigen Vergleichsständen. Positive Teilbeobachtungen können angezeigt werden, erlauben aber keine Aussage über die Abwesenheit anderer Ressourcen. Neue Suchbereiche und wesentliche Profiländerungen beginnen eine neue Baseline.

Ein Dateisystemscan ist kein atomarer Betriebssystemsnapshot. Gleichzeitige Änderungen lösen begrenzte Nachprüfungen oder einen Teilstatus aus. Die endgültige Bereinigungsvalidierung arbeitet immer mit aktuellen Prüfungen und nicht allein mit gespeicherten Inventardaten.

### 6.4 Betrieb, Sicherung und Wiederanlauf

Empfohlene SQLite-Ausgangslage: geprüfte gebündelte Engine, `foreign_keys=ON`, WAL, ein koordinierter Schreiber, kurze Transaktionen, kleine Schreibbatches und begrenzte Verbindungen/Caches. Lange UI- und Exportabfragen dürfen Checkpoints nicht unbegrenzt aufhalten.[^t08]

Für den Schreibpfad gilt zunächst `synchronous=FULL`. Ein dauerhaft bestätigter Aktionszustand muss jedem destruktiven Dateisystemeingriff vorausgehen. Inventarereignisse werden gebündelt; die Haltbarkeit des Löschjournals wird nicht zur Performanceoptimierung abgesenkt.[^t09]

Der Release dokumentiert die tatsächliche SQLite-Engine, nicht nur die Rust-Crate-Version. Bekannte Sicherheits- und Korrekturhinweise werden vor Versionsfreigabe geprüft. Bei vollem Datenträger, anhaltendem WAL-Wachstum oder Journalausfall wird gedrosselt beziehungsweise Bereinigung gestoppt.

Migrationen erhalten eine konsistente Sicherung, Schema-Versionsprüfung und Fehlerbehandlung. Eine ältere Anwendung verändert kein unbekanntes neueres Schema. Ein geeigneter Sicherungsweg ist die SQLite Backup API.[^t10]

Datenbank-Commit und Dateisystemaktion sind keine gemeinsame atomare Transaktion. Nach einer Unterbrechung werden Journal und tatsächliche Zielidentität abgeglichen. Schon ausgeführte Schritte werden nicht blind wiederholt. Datenschutz gilt auch für WAL, Sicherungen und Diagnosekopien; das Entfernen eines Datensatzes ist keine Garantie physischer Löschung aus allen Kopien.

<a id="sicherheit"></a>
## 7. Sicherheit und Datenschutz

Diese Verträge sind Voraussetzung für reale Scans und werden nicht auf die abschließende Releaseprüfung verschoben.

### 7.1 Sicheres Lesen

Jeder Zugriff muss auf einen lokal freigegebenen Suchbereich zurückführbar sein. Reine Stringpräfixe, pauschale Kleinschreibung oder ein einmaliges Auflösen eines Pfads genügen nicht als Sicherheitsprüfung.

Der Windows-Baustein behandelt insbesondere Objektidentität, Austausch während des Öffnens, Junctions, Symlinks, Reparse Points, Volume-Grenzen, lange Pfade und Unicode. Bekannte Verknüpfungen dürfen nur zu ebenfalls freigegebenen Zielen aufgelöst werden. Nicht unterstützte Reparse-Typen bleiben ungelöst.[^t11]

Relative Profilreferenzen müssen innerhalb des erlaubten Bereichs bleiben. Absolute, UNC-, Device- und ausbrechende `..`-Pfade werden abgewiesen. Benutzer- und verwaltete KI-Konfiguration außerhalb der Projektwurzel benötigt eigene Freigabe. Eine nicht auflösbare Referenz bleibt sichtbar, ohne ihr Ziel zu lesen.

Diese Grenzen gelten auch für Größenberechnungen, Vorschauen und Fehlerbehandlung. Ein Scan darf Cloud-Platzhalter nicht unbemerkt hydratisieren oder entfernte Dateien nachladen.

### 7.2 Vier getrennte Ausführungswege

| Weg | Erlaubtes Verhalten |
|---|---|
| **Statische Erkennung** | Begrenztes Lesen freigegebener Dateien; kein Programmstart, Download, Import oder Ausführen von Konfiguration. |
| **Kontrollierte Inspektion** | Ein pro Adapter geprüfter und lokal freigegebener Programmaufruf mit dokumentierten Nebenwirkungen. |
| **Updateprüfung** | Separat freigegebener Quellen-/Netzwerkzugriff ohne Installation oder Build. |
| **Verwalteter Lauf** | Ein ausdrücklich gestarteter Nutzerbefehl mit Sessionbezug; kein Bestandteil passiver Discovery. |

Ein Inspektionsvertrag legt Programmherkunft und Version, Argumente, Arbeitsverzeichnis, Umgebung, Konfigurationsquellen, mögliche Schreib-/Netzwerkeffekte, Zeitlimit, Ausgabelimit und Abbruchverhalten fest. Ein Projektverzeichnis darf nicht zur unkontrollierten Suche nach ausführbaren Programmen dienen.

Windows-Wrapper wie `.cmd` und PowerShell benötigen eigene Argument- und Profiltests. Keine generischen Shellstrings aus Paketnamen zusammensetzen. Lassen sich Hooks, Plugins oder Credential-Helfer nicht ausschließen oder gesondert freigeben, bleibt der Weg deaktiviert und ein statischer Teilstatus erhalten.

Das gilt auch für vermeintlich einfache Python-Inspektion: Die Initialisierung kann ausführbare `.pth`-Zeilen verarbeiten. Ein gefundener Interpreter wird deshalb nicht allein wegen seines Namens als passiver Metadatenleser gestartet.[^t12]

### 7.3 Drei Datenwege

| Ebene | Erlaubte Daten und Schutz |
|---|---|
| **Parser** | Rohinhalte kurzlebig und größenbegrenzt verarbeiten; keine Raw-Snapshots oder unbereinigten Standardlogs. |
| **Lokales Inventar** | Notwendige typisierte Metadaten, IDs, Beziehungen und freigegebene Orte speichern; Freitext nicht automatisch historisieren. |
| **Export und Diagnose** | Erneut reduzieren; absolute Pfade, private Namen und sensible Argumente standardmäßig entfernen; Vorschau vor Export. |

Auch Beschreibungen, Dateinamen, URLs und Fehlermeldungen können sensible Inhalte enthalten. Eine Feldnamen-Positivliste ist deshalb nur eine erste Schranke. Unbekannte Felder, Umgebungsvariablenwerte, Header, Tokens und rohe Kommandoausgaben gehören nicht in die automatische Historie. Credential-only-Dateien werden nicht geöffnet. Variablenreferenzen aus fremder Konfiguration werden nicht zu echten Geheimnissen expandiert.

Ein nützliches lokales Inventar benötigt teilweise private Paketnamen und Pfade. Diese werden als vertrauliche lokale Metadaten behandelt und bei der Suchbereichsfreigabe erklärt. Es gibt keine Zusage, beliebig in Namen versteckte Geheimnisse universell erkennen zu können.

Synthetische Testgeheimnisse in Secretfeldern, Beschreibungen, URLs, Argumenten und Fehlerausgaben prüfen Datenbank, WAL, Logs, Historie, Sicherungen und Exporte. Hashes sensibler Werte sind kein Ersatz für Datenminimierung.

### 7.4 Schutz und lokale Autorisierung

Schutz ist vorrangig. Ein geschütztes Unterverzeichnis verhindert eine pauschale Elternlöschung. Geteilte Repositorydateien dürfen weder Suchbereiche erweitern noch Schutz entfernen oder Ressourcen ohne lokale Entscheidung als entbehrlich erklären.

Fremde Instruktionen, Skills, Hooktexte und MCP-Konfiguration sind Daten für mogumogu. Sie verändern nicht seine eigenen Rechte oder Ausführungsregeln. Roh-Markdown wird nicht als ausführbarer Inhalt gerendert; Remoteinhalte werden nicht automatisch geladen.

Notwendige unbekannte Besitz-, Aktivitäts- oder Identitätsinformationen blockieren die entsprechende Bereinigung. Eine manuelle Prüfung kann Fakten ergänzen, ist aber kein allgemeiner „Trotzdem alles löschen“-Schalter.

<a id="adapter"></a>
## 8. Erweiterbare Adapter und KI-Profile

### 8.1 Paketadapter

Alle eingebauten Rust-Adapter verwenden denselben Fähigkeitsvertrag:

```text
discover        inventory        relationships        observe_activity
check_updates   estimate_size    plan_cleanup         execute_cleanup
```

Jede Fähigkeit meldet ihre Version, Quelle, Vollständigkeit, Unsicherheit und erforderliche Freigabe. Ein Adapter kann keine Kernschutzregel umgehen. Inventarisieren impliziert weder Updateberechtigung noch Löschfähigkeit.

Das gemeinsame Testsystem enthält positive und negative Fixtures, unbekannte Formate, Offlinezustände, defekte Eingaben, gemeinsame Besitzer, begrenzte Ausgaben und Windows-10/11-Fälle. Kontrollierte Programme werden nur über den zentralen Ausführungsvertrag aufgerufen. Für Cargo und NuGet bleibt statische Erfassung vom Auflösen, Wiederherstellen oder Bauen getrennt.[^t13][^t14]

Ein neuer eingebauter Rust-Adapter benötigt einen neuen Build. Ausführbare Drittanbieterplugins werden für die erste Beta nicht geladen. Ein separater Prozess wäre allein keine Sandbox gegen bösartigen Code desselben Benutzers.

### 8.2 Deklarative Dateiprofile

Neue Dateiformate können ohne Neubau ergänzt werden, soweit vorhandene Parser und Datentypen ausreichen. Der Vertrag ist klein, geschlossen und versioniert; er ist keine allgemeine Abfrage- oder Skriptsprache.

| Profilteil | Pflichtinhalt |
|---|---|
| Identität und Herkunft | Namespacierte ID, Schema-/Profilversion, Quelle, Lizenzhinweis, getestete Tool-/Formatversionen, Dokumentations- und Fixturebezug |
| Erkennung | Erlaubter Scopetyp, relative Muster, Parser, Ressourcentyp, schützende Klassifikation |
| Extraktion | Feste Schlüssel-/Indexpfade, begrenzte Typen und Transformationen, ausdrücklich erlaubte relative Referenzen |
| Ressourcenlimits | Dateigröße, Tiefe, Referenzen und Gesamtarbeit; darf Hostlimits nur verschärfen |

Vorgesehene Parser sind JSON/JSONC, TOML, Markdown-Frontmatter, YAML ohne benutzerdefinierte Konstruktoren und mit begrenzter Aliasverarbeitung sowie XML ohne DTD oder externe Entitäten. Selektoren dürfen keine Programme, dynamischen Ausdrücke oder Netzwerkressourcen aufrufen.

Extrahierte Werte durchlaufen zusätzlich den Datenschutzfilter. Neue Parser oder Fachsemantik erfordern eine Hoständerung und Tests. Ein Profilversprechen ist daher keine grenzenlose Erweiterung ohne Programmänderung.

Herkunftsmetadaten sind keine Vertrauensfreigabe. Profile aus einem gescannten Repository werden nicht automatisch installiert. Ein Profilupdate erweitert keine Rechte, löst eine neue Vergleichsbasis aus und invalidiert betroffene Bereinigungspläne.

**Erweiterungsabnahme:** Ein Contributor ergänzt ein synthetisches Settingsformat mit Herkunft, relativem Verweis und Negativtests ohne Änderung von UI oder Kerndomäne. Ausführungsfelder, Pfadausbrüche, unzulässige Selektoren und überhöhte Limits werden abgewiesen.

### 8.3 KI-Erkennungskatalog

Die folgende Tabelle beschreibt den geplanten Profilumfang. Pfade sind Erkennungskandidaten und müssen vor der Ausweisung als unterstützt gegen die konkrete Clientversion getestet werden. Datei vorhanden, Client installiert, Format unterstützt und tatsächlich verwendet bleiben getrennte Zustände.

| Profil | Erkennungskandidaten und Gegenstand | Besondere Modellierung |
|---|---|---|
| **Agent Skills** | `SKILL.md`, Frontmatter, Referenzen, Skripte und Assets | Format und unterstützter Suchpfad sind verschiedene Verträge. |
| **Codex** | `AGENTS.md`, `AGENTS.override.md`, `.agents/skills/`, `.codex/config.toml`, freigegebene Benutzereinstellungen | Verzeichnisbezogene Instruktionen, Verbraucher, Scope und Overrides. |
| **Claude Code** | `CLAUDE.md`, `.claude/skills/`, Projekt-/Lokaleinstellungen, Regeln, Agents, Commands, Hooks und `.mcp.json` | Gemeinsame und lokale Einstellungen sowie MCP-Bereiche getrennt. |
| **Copilot / VS Code** | `.github/copilot-instructions.md`, Instructions, Prompts, Agents, Skills und `.vscode/mcp.json` | Editor-, CLI- und andere Verbraucher nicht pauschal gleichsetzen. |
| **Cursor** | `.cursor/rules/`, Skill- und MCP-Konfiguration, `AGENTS.md`, relevante Altformate | Regelbedingungen und versionsabhängige Suchpfade. |
| **Gemini CLI** | `GEMINI.md`, `.gemini/settings.json`, Skillverzeichnisse und statische Imports | Begrenzte Importketten und getrennte Konfigurationsebenen. |
| **Windsurf/Cascade** | Lokale Regeln, Skills und Einstellungen der getesteten Version | Alte und neue Formate getrennt; Dokumentationsweiterleitungen ändern keine installierte Semantik. |
| **Cline** | Lokale Regeln, Skills und freigegebene Benutzereinstellungen | Editorprofile und Aktivierungsschalter nicht aus Dateiexistenz erfinden. |
| **Roo Code** | `.roo/`-Regeln, Skills, modusspezifische Ressourcen und MCP | Modus als Teil des Verbraucherkontexts. |
| **OpenCode** | `opencode.json`/`opencode.jsonc`, Skills, Agents und Commands | Konfigurationsschichten und statische Referenzen, keine Pluginaktivierung. |
| **Continue** | Lokale Regeldateien und registrierte Konfiguration | Nur getestete Schlüssel semantisch auswerten. |
| **Aider** | `.aider.conf.yml` und freigegebene referenzierte Instruktionen | Keine universelle `SKILL.md`-Aktivierung voraussetzen. |

Das Agent-Skills-Format kennt Instruktionen und ergänzende Ressourcen, aber keinen universellen Aktivierungs- oder Versionsregistryvertrag. Die Clientprofile müssen dies ergänzen, ohne Verwendung zu erfinden.[^t15] Die primären Referenzen für die Profilimplementierung sind im Quellenabschnitt aufgeführt.[^t16][^t17][^t18][^t19][^t20][^t21][^t22][^t23][^t24][^t25][^t26]

### 8.4 Referenzen und Wirksamkeit

Mögliche Beziehungen lauten beispielsweise:

```text
Projekt
  -> KI-Konfiguration
    -> MCP- oder Hookdefinition
      -> Paket-/Programmreferenz
        -> bekannte Runtime oder Umgebung
          -> registrierte Ausgabe oder zugehöriger Cache
```

Ein `npx`- oder `uvx`-Verweis ist zunächst nur eine Startreferenz. Der Scanner startet weder den Server noch einen Paketdownload oder eine Verbindung zu einem Remote-MCP-Endpunkt.

Die Ansicht berücksichtigt Clientversion, Arbeitsverzeichnis, Projekt-/Benutzer-/Managed-Scope und bekannte Overrides. Fehlen Laufzeit- oder Organisationsinformationen, lautet das Ergebnis „statisch ermittelt, unvollständig“. Es gibt keine universelle Regel „Projekt schlägt Global“.

Automatisch erzeugte Chatprotokolle, persönliche Agent-Memory-Bestände und Credentialdateien sind standardmäßig von der Inhaltsanalyse ausgeschlossen. Gemeinsame Instruktionsdateien bleiben als geschützte Projektkonfiguration erfassbar. Eine optionale Größeninventarisierung sensibler Bestände benötigt eigene Freigabe.

### 8.5 Veröffentlichte Unterstützungsmatrix

Pro Fähigkeit werden Toolversion, Format, Windows-Edition/Build/Architektur, statischer oder freigegebener Aufrufweg, Adapter-/Profilversion, Fixture-Commit, Quellenstand und Testergebnis dokumentiert.

Zulässige Statuswerte sind `planned`, `experimental`, `tested`, `limited` und `unsupported`. Ein Format kann erkannt sein, ohne dass die effektiven Einstellungen vollständig verstanden werden. Keine Anzeige behauptet „alle Skills gefunden“, ohne den Suchumfang zu nennen.

<a id="sessions"></a>
## 9. Scratchpads, Sessions und CLI

### 9.1 Ressourcen eines Experiments

Ein Scratchpad erhält Namen, Zweck, Parent-Projekt, Ursprung, Prüftermin, Schutz, Ergebnis und zugehörige Ressourcen. Passive Entdeckung, explizite Übernahme und Erstellung durch mogumogu bleiben unterscheidbar. Übernahme ist keine Löschfreigabe.

Empfohlene Struktur für verwaltete Sessions:

```text
<freigegebene-wurzel>/<session-id>/
  source/          # Quellcode und Notizen erhalten
  environment/     # Separat prüfen und klassifizieren
  temporary/       # Nur ausdrücklich entbehrliche Teile bereinigen
  results/         # Ergebnisse und Diagnosen erhalten
```

Verfügbare Aktionen: Prüftermin verlängern, schützen, zum Projekt machen, Experiment beenden und vorhandene Sicherungs-/Archivwege anzeigen. Eine Promotion entfernt Ablaufregeln, nicht die Historie.

Ein verwalteter Lauf kann passende Tempvariablen ausschließlich an den gestarteten Kindprozess weitergeben. Er verändert keine globalen Umgebungseinstellungen. Dies ist Routing, keine Sandbox und keine Garantie, dass jedes Programm ausschließlich dort schreibt.

### 9.2 Sessionzustände

| Zustand | Bedeutung | Konsequenz |
|---|---|---|
| `starting` | Registrierung oder Start wird eingerichtet | Ressourcen bleiben gesperrt. |
| `running` | Zugeordnete Arbeit läuft | Ressourcen bleiben gesperrt. |
| `completion_requested` | Abschluss wurde gemeldet, aber nicht abschließend geprüft | Keine Freigabe. |
| `completed_verified` | Ende des tatsächlich erfassten Umfangs geprüft | Weitere Ressourcen- und Planprüfung bleibt erforderlich. |
| `interrupted_unknown` | Absturz, verlorene Verbindung oder unvollständige Prozesszuordnung | Ressourcen bleiben geschützt. |
| `released_after_review` | Unklarer Sperrzustand lokal mit Begründung aufgelöst | Keine automatische Entbehrlichkeit; neuer oder erneut validierter Plan nötig. |

Verwaltete Starts, nachträgliche Prozessregistrierung und rein kooperative Meldungen haben unterschiedliche Beweisstärke. Prozessidentität darf nicht allein auf einer PID beruhen; Erstellungskontext und geprüfte Handles werden soweit verfügbar einbezogen.

Für verwaltete Läufe wird die frühe Zuordnung zu Windows Job Objects geprüft. Deren Umfang und Breakaway-Regeln müssen getestet werden. Sie belegen nicht, dass jede beliebige Fremdaktivität erfasst wurde.[^t27] Kein implizites Beenden von Nutzerarbeit durch `KILL_ON_JOB_CLOSE`.

Ein abgeschlossener Starter bei noch laufendem Worker ist keine abgeschlossene Session. Ein fehlender Heartbeat löst eine Prüfung aus, nicht das Ablaufen einer Löschsperre. Nach Neustart, Suspend/Resume und IPC-Verlust erfolgt ein Zustandsabgleich. Fehlgeschlagene oder unterbrochene Tests behalten Diagnosen standardmäßig.

### 9.3 CLI-Vertrag

Geplante Befehlsfamilien; noch keine implementierte CLI:

```text
mogumogu scan
mogumogu list
mogumogu why <resource-id>
mogumogu track <path>
mogumogu scratch create|adopt|extend|protect|promote
mogumogu session register|attach|heartbeat|complete|status
mogumogu run --session <id> -- <program> <arguments>
mogumogu updates
mogumogu cleanup plan
mogumogu cleanup approve <plan-id>
mogumogu cleanup apply <plan-id>
mogumogu project hibernate
```

Registrierung verwendet Idempotenzkennungen. JSON-Ausgabe besitzt eine Protokollversion und dokumentierte Fehlercodes. `run` transportiert strukturierte Argumente, nicht automatisch einen Shellstring. Rohargumente, Umgebungsvariablenwerte und stdout/stderr werden nicht standardmäßig historisiert.

Integrationen dürfen Ressourcen registrieren, Nutzungssperren erneuern, Ergebnisse melden und Pläne vorschlagen. Sie können sich nicht durch ein allgemeines `--yes` selbst globale Löschrechte erteilen. Eine lokale Bestätigung bindet sich an einen konkreten Plan; dessen Anwendung akzeptiert keine nachträglich beliebig ausgetauschten Zielpfade.

<a id="cleanup"></a>
## 10. Prüfkandidaten und Bereinigung

### 10.1 Hinweise statt unbelegter Urteile

Die Regelengine unterscheidet **inaktiv**, **Prüftermin erreicht**, **möglicherweise verwaist**, **möglicherweise unreferenziert** und **unklar**. Jeder Hinweis nennt Auslöser, Evidenz, beobachteten Zeitraum, Besitzer, Unsicherheiten und eine sinnvolle nächste Handlung.

Beispiel:

> Keine relevante Aktivität während 30 Tagen Beobachtung. Die Umgebung gehört weiterhin zum Projekt. Prüfen, schützen oder im Rahmen einer Projektstilllegung berücksichtigen.

Vorgeschlagene Prüfzeiträume: sieben Tage für Scratchpads, 30 Tage für generierte Projektumgebungen und 90 Tage für globale Tools. Diese Werte sind anpassbare Priorisierungsregeln, keine Löschfristen.

### 10.2 Entscheidung über Entbehrlichkeit

| Situation | Vorgesehenes Verhalten |
|---|---|
| Verwaltete Ausgabe, lokal ausdrücklich entbehrlich, Session abgeschlossen, keine Blocker | Selektiven Plan zulassen. |
| Passiv gefundener Tempordner oder Umgebung | Inventarisieren und prüfen; nicht automatisch entbehrlich. |
| Übernommene bestehende Umgebung | Inhalt, Besitzer, Schutz und lokale Klassifikation separat prüfen. |
| Neue oder geänderte Inhalte seit Bestätigung | Plan invalidieren oder betroffene Ziele blockieren. |
| Geschütztes Kindverzeichnis | Selektiv erhalten oder Elternoperation verweigern. |
| Verknüpfte Quelle, gemeinsamer Store oder weiterer Besitzer | Nicht als private generierte Kopie behandeln. |
| Aktive oder unklare Session, Installation, notwendige Sperrprüfung fehlt | Bereinigung blockieren. |
| KI-Skill, Regel, Konfiguration, Erinnerung oder einzigartige Daten | Im Beta-Cleanup geschützt. |
| Journal nicht dauerhaft schreibbar | Keine neue destruktive Teilaktion beginnen. |

Unbekannte Inhalte können nach lokaler Einsicht einzeln klassifiziert werden. Das hebt weder Schutz noch Identitätsprüfung auf. Eine vollständige Dateiliste oder Lockdatei beweist nicht, dass alle Inhalte ersetzbar oder wertlos sind.

### 10.3 Plan und Freigabe

Ein Plan nennt konkrete Ressourcen, Operationen, Besitzer, Gründe, erhaltene Inhalte, Unsicherheiten, Blocker und den tatsächlichen Wiederherstellungsweg. Ziele müssen abwählbar sein.

Die Bestätigung bindet Zielidentitäten und Orte, Scope-/Profil-/Regelversionen, Schutz, Besitz, relevante Inhaltsbedingungen und erforderliche Prozessprüfungen. Sie ist keine dauerhafte Genehmigung für alles, was später am selben Pfad liegt.

```text
draft -> approved -> revalidating -> executing
revalidating -> invalidated | cancelled | executing
executing -> completed | partial | failed | cancelled | recovery_required
```

Vor Beginn und vor relevanten Teilaktionen werden die Voraussetzungen erneut geprüft. Der Aktionszustand muss vor dem Eingriff dauerhaft gespeichert sein. Teilaktionen erhalten eigene IDs und Ergebnisse. Ressourcentypen mit nicht zuverlässig erfüllbaren Voraussetzungen bleiben schreibgeschützt.

Abbruch erfolgt an sicheren Grenzen. Nach bereits erfolgten Löschungen ist das Ergebnis ein Teilerfolg, kein Rollback. Nach einem Absturz wird zuerst abgeglichen; ein verändertes Ziel wird nicht aufgrund eines alten Journaleintrags erneut bearbeitet.

### 10.4 Reihenfolge der Löschfähigkeiten

Der erste destruktive Pilot entfernt ausschließlich eine synthetische, ausdrücklich entbehrliche Sessionausgabe in einer wegwerfbaren Testwurzel.

Erst nach zusätzlicher Abnahme folgen entsprechende produktive Ausgaben. Weitere lokale Umgebungen oder Buildressourcen werden separat freigeschaltet, mit testspezifischen Grenzen für Links, Locks und unbekannte Inhalte. Ganze Projekte, gemeinsame Paketspeicher und beliebige fremde Tempbäume sind keine generische Löschoperation.

„Experiment beenden“ und „Projekt stilllegen“ sind zusammengesetzte selektive Pläne. Sie erhalten Quellcode, Spezifikationen, Notizen, Konfiguration und geschützte Ergebnisse. Paketmanagersemantik darf nicht durch direkte Cachemanipulation umgangen werden; bei uv ist dies ausdrücklich relevant.[^t28]

### 10.5 Wiederherstellung und Speicherwirkung

Quarantäne, vorhandene Sicherung, Rekonstruktionsanleitung und irreversible Entfernung werden getrennt bezeichnet. Eine Rekonstruktionsanleitung ist keine Sicherung; ihre Existenz belegt keinen erfolgreichen Restore.

Quarantäne ist keine universelle Betafunktion. Unterstützte Verschiebungen benötigen Kollisions-, Volume-, Berechtigungs- und Wiederherstellungstests. Bei Fehler darf nicht still auf endgültige Löschung zurückgefallen werden. Restore und späteres endgültiges Leeren sind eigene Aktionen.

Ressourcengröße, zugeordneter Projektumfang, exklusiver Speicher und voraussichtlich freigebbarer Speicher sind unterschiedliche Werte. Hardlinks, überlappende Verzeichnisse und weitere Besitzer werden berücksichtigt. Unbekannt ist nicht null; Quarantäne zählt nicht als sofortige Speicherfreigabe.

<a id="ui"></a>
## 11. Oberfläche, Benachrichtigungen und Updates

### 11.1 Tray und Dashboard

Der Tray zeigt den Überwachungszustand, den letzten erfolgreichen Abgleich und gruppierten Prüfbedarf. Das Menü bietet Dashboard, Scan, Pause/Fortsetzen, Einstellungen und Beenden. Eine einzelne Mausgeste darf nicht der einzige Zugang sein.

Das Dashboard beginnt mit einer priorisierten Prüfansicht. Vorgesehene Bereiche sind Übersicht, Projekte, Ressourcen, Scratchpads/Sessions, KI-Konfiguration, Änderungen/Updates und Historie. „Warum ist das hier?“ öffnet eine Erklärung zu Besitzer, Referenzen, Aktivität und möglichen Auswirkungen.

Suchen und Filtern nach Typ, Besitzer, Manager, Ort, Aktivität, Schutz, Größe und Verfügbarkeit gehören zum Kern. Tabellen werden paginiert oder virtualisiert; keine vollständige Graphkopie im UI. Deutsch ist die erste Oberflächensprache, Texte werden zentral übersetzbar gehalten.

Tastaturbedienung, zugängliche Beschriftungen, lesbare Statusmeldungen und nicht nur farbliche Warnungen gehören zur Abnahme.

### 11.2 Benachrichtigungen

Erstinventare erzeugen keine Flut bestehender Paketmeldungen. Neue Ressourcen, verfügbare Updates, fällige Prüfungen und Überwachungsfehler bleiben getrennte Ereignistypen. Zusammengehörige Änderungen werden pro Projekt oder Session gruppiert und dedupliziert.

Ruhezeiten, Stummschaltung, spätere Erinnerung und tägliche Zusammenfassung werden unterstützt. Empfohlener Datenschutzstandard: Windows-Benachrichtigungen bleiben generisch und zeigen keine privaten Projektpfade oder Paketnamen. Details erscheinen im Dashboard.

### 11.3 Updateprüfungen

Onlineprüfung ist zunächst ausgeschaltet. Nach Quellenfreigabe erfolgt sie täglich mit Caching, begrenzten Wiederholungen und zusätzlicher manueller Auslösung. Ausgefallene Prüfungen führen zu `offline`, `stale`, `unknown` oder `unsupported`, nicht zu „alles aktuell“.

Angezeigt werden installierte Version, zulässiges Updateziel soweit bekannt, neuere Versionen außerhalb des Bereichs, Kanal/Quelle, letzter Erfolg und Einschränkungen. Eine Versionsnummer allein ist keine Kompatibilitätsgarantie. Managerabhängige Semantik darf nicht vereinheitlicht werden; npm unterscheidet beispielsweise verschiedene Updateziele.[^t29]

Private Quellen bleiben privat. Zugangsdaten werden nicht in eigene Logs oder Exporte übernommen. Netzwerkprüfungen installieren keine Pakete und ändern keine Lockdateien. Skillupdates benötigen bekannte Herkunft und Revision; ohne solche Daten wird kein Update erfunden.

<a id="performance"></a>
## 12. Performance und Ressourcenbudgets

### 12.1 Entwicklungsziele

| Messgröße | Ziel und Messkontext |
|---|---|
| Tray-Leerlauf | Unter **50 MiB Private Bytes** nach Inventarisierung und Beruhigung; vor erstem Dashboard und nach Schließen getrennt messen. |
| Leerlauf-CPU | Unter **0,5 % eines logischen Kerns**, über zehn Minuten ohne externe Änderungen oder geplante Scans gemittelt. |
| Dashboard öffnen | Gespeicherte erste Übersicht am **95. Perzentil innerhalb von 500 ms**, wenn der Kern bereits läuft. |
| Metadatenänderungen | Geeignete Änderungen innerhalb von **zehn Sekunden nach Beruhigung** bei funktionierender Überwachung sichtbar. |
| Fensterzyklen | Nach Aufwärmen kein anhaltendes Wachstum von Speicher, Threads oder Handles über **100 Öffnen-/Schließen-Zyklen**. |
| Lastbetrieb | Bedienbare Oberfläche, Fortschritt, begrenzter Rückstau und kontrollierbarer Abbruch. |

Diese Ziele werden im Machbarkeitsprototyp überprüft. Bei Überschreitung werden zunächst Renderer, Features, Caches, Listener und Lebenszyklus untersucht. Zieländerungen benötigen einen dokumentierten Entscheid; Sicherheitsprüfungen werden nicht für bessere Messwerte entfernt.

### 12.2 Ausgangsgrenzen für Implementierung und Lasttests

Die Werte sind Startparameter, keine bestätigten Optimalwerte. Profile dürfen sie nur verschärfen. Bei Überschreitung werden Fortschritt, Einschränkung und gegebenenfalls eine erweiterte Freigabe sichtbar, statt eine Ressource als abwesend zu melden.

| Bereich | Ausgangsgrenze / Reaktion |
|---|---|
| Worker | Zwei parallele Discovery-/Parseaufgaben, höchstens eine Größenberechnung. |
| Ereignisse | Maximal 4.096 Queueeinträge und 4 MiB Nutzdaten; Überlast zu abgleichbedürftigen Bereichen bündeln. |
| IPC | Maximal 1 MiB pro Nachricht; begrenzte Clients und offene Anfragen; Ergebnisse paginieren. |
| KI-Datei | Maximal 1 MiB und 32 Verschachtelungsebenen im Standardprofil; größere Dateien sichtbar begrenzen. |
| Paketmetadaten | Bis 32 MiB pro freigegebenem Parserweg; Streaming oder eigenes Allokationsbudget, andernfalls Teilstatus. |
| Faire Arbeitsportion | Nach spätestens 5.000 Kandidaten oder 64 MiB gelesenen Metadaten Fortschritt sichern und anderen Bereichen Zeit geben. |
| Gesamterfassung | Zunächst 1 GiB Metadaten und 1 Mio. Referenzkanten pro Scope/Generation; Erweiterung ausdrücklich freigeben. |
| Referenzen | Zyklen erkennen; maximal 16 Import-/Referenzebenen. |
| Externe Programme | Standardmäßig 30 Sekunden und 16 MiB Ausgabe; Ausnahmen nur pro geprüftem Adapter. |
| UI | Standardseite 200 Einträge; keine dauerhafte vollständige Inventarkopie. |
| SQLite | Ein Schreiber, begrenzte Batches; gesamte Page-Cache-Zielgröße zunächst höchstens 8 MiB über alle Verbindungen. |
| Logs | Redigiert und rotierend, maximal 20 MiB. |
| Historie | Detaillierte Beobachtungen zunächst 30 Tage, Aggregate 180 Tage; Baseline, Schutz- und nötige Recoverybelege erhalten. |
| Datenbank | Ab 250 MiB Warnung und Aufbewahrungsprüfung; kein ungeprüftes Verwerfen aktiver Journale. |
| WAL | Ab 32 MiB kontrollierten Checkpoint anfordern; bei anhaltendem Wachstum Ursachen prüfen und Inventararbeit drosseln. |

Bytegrenzen, gleichzeitige Aufgaben und tatsächliche Allokationen werden gemeinsam geprüft. Eine Eingabedateigröße begrenzt nicht automatisch den Speicherbedarf ihrer geparsten Struktur. Aktive Nutzungssperren und Schutzdaten dürfen unter Last nicht verloren gehen.

### 12.3 Ereignisse und Abgleich

Dateisystemereignisse lösen gezielte, gebündelte Prüfungen aus. Sie sind keine vollständige Historie: Windows-Verzeichnisbenachrichtigungen können bei Pufferüberlauf Detailereignisse verlieren. Der betroffene Bereich muss dann abgeglichen werden.[^t30]

Empfohlener Standard ist ein gestaffelter Metadatenabgleich bei Start/Fortsetzen und höchstens täglich im ruhigen Betrieb. Es gibt keine Nachholflut versäumter Termine. Teure Größenberechnungen laufen separat und zunächst nur bei Bedarf. Eine NTFS-Journaloptimierung ist eine spätere Option, keine Startvoraussetzung.

### 12.4 Referenzbestand und Messbericht

Der Referenzbestand umfasst 50 Projekte, 10.000 Paketinstallationen und 100.000 Beziehungen. Separate Stresstests verwenden bis zu eine Million Dateisystemeinträge sowie 10.000 synthetische Skills und 2.000 Settings-/Regeldateien. Diese Zahlen beschreiben Testdaten, nicht den Rechner des Nutzers.

Optimierte Builds werden mit dokumentierter Hardware, Windows-Version, Stromprofil, Renderer und Schutzsoftware gemessen. Private Bytes, Working Set, GPU-Speicher, CPU-Zeit, Handles und I/O werden getrennt berichtet. Kurzlebige CLI- und Managerprozesse zählen bei Lasttests mit.

VM-Kompatibilitätstests ersetzen keinen Hardware-Performancebericht. Schutzsoftware wird nicht zur Verbesserung der Messwerte abgeschaltet.

<a id="opensource"></a>
## 13. Open Source, Zusammenarbeit und Entwicklungsskills

### 13.1 Repositorygrundlagen

Lizenztext, empfohlene `GPL-3.0-only`-Metadaten, Copyright-/Drittanbieterhinweise, README, CONTRIBUTING und ein vertraulicher Sicherheitsmeldeweg gehören zum ersten Meilenstein. Die konkrete Slint- und Abhängigkeitslizenzierung wird für den ausgelieferten Build dokumentiert.

`.gitignore` schließt lokale Datenbanken, Exporte, Credentials und persönliche Clienteinstellungen gezielt aus. Gemeinsame Skills oder Projektregeln werden nicht pauschal mit ausgeschlossen.

Basis-CI beginnt mit Schema-, Profil- und Skillvalidierung; nach Aufbau des Rust-Workspace kommen Format-, Lint- und Unitprüfungen hinzu. Zugriffsrechte werden minimiert, Actions auf geprüfte Revisionen festgelegt. Fremde Pull Requests erhalten keine Release-Secrets und keinen Zugriff auf produktive oder ungeschützte persistente Runner.[^t31]

### 13.2 Gemeinsame Entwicklungsregeln

`AGENTS.md` ist die kanonische Quelle der mogumogu-Entwicklungsregeln. Sie beschreibt Architekturgrenzen, tatsächlich vorhandene Testbefehle, Datenschutz, erlaubte Testwurzeln und den erforderlichen Review kritischer Änderungen.

Clientbezogene Einstiegstexte verwenden getestete Import-/Referenzmechanismen oder erzeugte Kopien. Ein bloßer Hinweis, eine andere Datei zu lesen, wird nicht als nachgewiesene native Integration ausgegeben.

### 13.3 Sechs projektspezifische Skills

| Skill | Aufgabe und erwartetes Ergebnis |
|---|---|
| `mogumogu-adapter-development` | Neue Manager/Profile über den gemeinsamen Vertrag ergänzen; Versionsmatrix, positive/negative Fixtures und begrenzte Fähigkeiten liefern. |
| `mogumogu-windows-safety` | Pfade, Reparse Points, Prozessidentität, IPC und Windows-Lebenszyklen gegen konkrete Fehlerszenarien prüfen. |
| `mogumogu-sqlite-migrations` | Schemaänderungen, Generationen, Sicherungen und Journalwiederanlauf mit reproduzierbaren Tests entwerfen. |
| `mogumogu-performance` | Leerlauf, Warteschlangen, Allokationen, Scans und Fensterzyklen mit dokumentierter Fixture messen. |
| `mogumogu-ai-settings` | Clientprofile, Scope, Referenzen und datensparsame Metadaten ohne Aktivierung implementieren. |
| `mogumogu-cleanup-review` | Löschpläne, Voraussetzungen, erhaltene Inhalte und Wiederanlauf unabhängig auf Datenverlustpfade prüfen. |

Jeder Skill bekommt geeignete und ungeeignete Aufgabenbeispiele, erwartete Ergebnisse und verifizierte Testbefehle. Texte ersetzen weder Kernprüfungen noch einen menschlichen Review produktiver Löschfunktionen.

Empfohlene erste Entwicklungsclient-Tests: Codex, Claude Code und Copilot/VS Code. Weitere Clients bleiben im Produktinventar enthalten, ohne dass alle für die Entwicklung installiert sein müssen. Erforderliche Kopien entstehen deterministisch mit Herkunftsvermerk und CI-Prüfung auf Abweichung. Keine Symlinkpflicht unter Windows und kein stilles Überschreiben lokaler Anpassungen.

Keine Vorlage aktiviert globale Ausführungsrechte, Hooks oder MCP-Server. „Datei vorhanden“, „vom Client geladen“ und „bei einer Aufgabe verwendet“ werden separat getestet.

### 13.4 Geteilte Projektvorschläge

Eine optionale `.mogumogu/project.toml` darf relative Ressourcen, Besitzerhinweise und gewünschte Prüfzeiträume beschreiben. Sie enthält keine persönliche Inventardatenbank, absoluten Maschinenpfade oder Zugangsdaten.

Jeder Collaborator behält lokale Suchfreigaben, Schutz und Zustimmung. Das Öffnen eines Repositorys erteilt keine Löschrechte. Ein redigierter Export kann Projektvoraussetzungen vergleichen, aber keine fremden Rechner steuern.

### 13.5 Distribution

Entwicklungsartefakte werden zunächst als versioniertes ZIP bereitgestellt. Für die Beta folgt ein getesteter Benutzerinstaller. Upgrades koordinieren Instanzende und Migration; die Deinstallation entfernt keine Projekte, Scratchpadquellen oder Ergebnisse.

Releases enthalten Prüfsummen, Versionen, Abhängigkeits-/Lizenzinventar, Quellstand und die erforderlichen Quell-/Buildinformationen. Der genaue Signierungsweg wird vor Veröffentlichung entschieden. Eine Signatur ist kein Ersatz für Sicherheits- und Kompatibilitätstests.

Windows-10- und Windows-11-Prüfungen laufen in getrennt dokumentierten, rücksetzbaren Umgebungen. Ein gewöhnlicher Windows-CI-Runner belegt nicht automatisch Windows-10-Kompatibilität.

<a id="meilensteine"></a>
## 14. Meilensteine und Freigaben

Der Plan ist ergebnis- und abhängigkeitsbasiert. Verfügbare Teamkapazität und Erfahrungsstand sind nicht quantifiziert; deshalb werden keine pauschalen Kalendertermine vorgegeben. Nach M0 werden Arbeitspakete anhand des Prototyps und der tatsächlichen Kapazität geschätzt.

### 14.1 Lieferfolge

| Phase | Ergebnis | Abschlussbedingung |
|---|---|---|
| **M0 — Fundament und Machbarkeit** | Repository, Lizenz, Basis-CI, konkrete Windows-/Versionsmatrix und synthetischer Rust/Slint/SQLite-Prototyp | Betrieb ohne Administratorrechte und erste Ressourcenmessung auf beiden Systemen dokumentiert. |
| **M1 — Kern und Verträge** | Sichere Lesegrenzen, Datenschutz, Datenmodell, Generationen, IPC, Adapter-/Profilvertrag sowie Session-/Planmodelle | Negative Vertragsprüfungen bestanden; reale Scans nur nach G-READ. |
| **M2 — Vollständiger Pilot** | npm-Projekt, statische Python-Umgebung, ein Skill, Scratchpad, Testlauf, Erklärung und begrenzte Testbereinigung | Ein Collaborator reproduziert den Ablauf ohne Editorwechsel; kein produktives Löschziel. |
| **M3 — Nutzbare Alpha** | Überwachung, Abgleich, Dashboard, erste KI-Scopeansicht, Meldungen und freigegebene Updatewege | Teilabdeckung sichtbar; Fehler und partielle Scans erzeugen keine falschen Entfernungsmeldungen. |
| **M4 — Vereinbarte Breite** | Alle neun Manager, alle KI-Basisprofile, Unterstützungsmatrix, gemeinsame Projektvorschläge und Entwicklungsskills | Getestete Inventarbreite auf Windows 10/11; neue statische Profile ohne Kernumbau nachgewiesen. |
| **M5 — Produktive Bereinigung** | Ressourcenspezifische Operationen, Journal/Recovery, Experimentabschluss und Projektstilllegung | G-CLEANUP bestanden; unabhängige Prüfung der kritischen Implementierung. |
| **M6 — Härtung und Distribution** | Performance-/Lastbericht, Migration, Installation, Deinstallation, Exporte, Hilfe und gehärteter Releaseweg | Reproduzierbare technische und organisatorische Abnahme. |
| **M7 — Erste vollständige Beta** | Vereinbarter Gesamtumfang mit dokumentierten Grenzen | G-BETA bestanden; keine offene kritische Datenverlust- oder Datenschutzlücke. |

### 14.2 Fähigkeitsbezogene Freigaben

| Gate | Erforderlicher Nachweis | Danach zulässig |
|---|---|---|
| **G-READ** | Lese-/Scopegrenzen, Parserlimits, Datenschutz, statischer Modus und Vergleichsgenerationen mit Negativtests | Reale fremde Projektinhalte innerhalb freigegebener Bereiche lesen. |
| **G-INSPECT** | Konkreter Adapter-/Versionsweg mit Aufrufkontext, Nebenwirkungen, Limits und lokaler Zustimmung | Genau diesen externen Inspektions- beziehungsweise Updateweg nutzen. |
| **G-SESSION** | Prozessbaum, Abschlusszustände, fehlende Heartbeats, Abbruch und Wiederanlauf | Sessionabschluss als begrenzten Nutzungs-/Abschlussbeleg verwenden. |
| **G-TEST-CLEANUP** | Planbindung, Schutz, Identitätsprüfungen, dauerhaftes Journal und notwendige Sessiontests | Synthetische entbehrliche Ausgabe in einer wegwerfbaren Testwurzel entfernen. |
| **G-CLEANUP** | Zusätzliche ressourcentypspezifische Race-/Fehlertests und unabhängiger kritischer Review | Ausschließlich die geprüfte produktive Operation freischalten. |
| **G-BETA** | Paket-/KI-Matrix, Windows-, Performance-, Datenschutz-, Recovery-, OSS- und Benutzerabnahme | Vollständige Beta veröffentlichen. |

Ein Gate gilt für definierte Fähigkeiten und Versionen, nicht pauschal für die ganze Anwendung. Sicherheitsrelevante Änderungen können eine erneute Abnahme erfordern. Eine funktionsfähige Oberfläche hebt kein Backend-Gate auf.

### 14.3 Zusammenarbeit und kritischer Pfad

Die kritische Folge lautet **Fundament → Sicherheits-/Datenverträge → vollständiger Pilot → produktive Sicherheitsabnahme → Beta**. Paketadapter und KI-Profile können nach dem gemeinsamen Vertrag parallel entstehen. Die gesamte Adapterbreite darf den kleinen Pilot nicht blockieren.

Voraussetzungen im Backlog beschreiben notwendige Ergebnisse, keine starre Personalplanung. Mitwirkende können verschiedene Rollen übernehmen; für die kritische produktive Cleanupprüfung ist eine zweite prüfende Person vorgesehen. Fehlt dieser Nachweis, bleiben Inventar und Planung nutzbar, die produktive Löschfähigkeit aber deaktiviert.

<a id="backlog"></a>
## 15. Umsetzungsbacklog

Alle Arbeitspakete beginnen als **geplant**. **P0** blockiert die jeweilige Fähigkeit oder Betaabnahme. **P1** gehört ebenfalls zum Zielumfang, kann aber nur nach ausdrücklicher Umfangsentscheidung verschoben werden. Priorität ersetzt keine Abhängigkeit. Rollen sind Zuständigkeiten, keine bereits zugewiesenen Personen.

### M0 — Fundament

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-001 | P0 · Maintainer | **Repository und Lizenzgrundlagen.** LICENSE, Metadaten, README, CONTRIBUTING, SECURITY und gezielte Git-Ausschlüsse vorhanden. | — |
| AP-002 | P0 · CI/QA | **Frühe Prüfungen.** Unprivilegierte CI validiert erste Profile/Skills und weist absichtlich ungültige Fixtures zurück. | AP-001 |
| AP-003 | P0 · Windows/QA | **Test- und Versionsmatrix.** Konkrete Windows-10/11-x64-Stände, Rust, Slint, SQLite und synthetische Fixture dokumentiert. | AP-001 |
| AP-004 | P0 · UI/Windows | **Tray-/Datenbankprototyp.** Fensterloser Tray, SQLite-Testdaten und große Tabelle auf beiden Systemen; Öffnen und Schließen geprüft. | AP-002, AP-003 |
| AP-005 | P0 · QA/Architektur | **Machbarkeitsentscheidung.** Release-Messbericht für Leerlauf, UI, Speicher und Handles; Budgetabweichungen dokumentiert entschieden. | AP-004 |

### M1 — Kern und Verträge

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-006 | P0 · Core | **Domäne und Identität.** Besitz, Aktivität, Schutz, Abdeckung und Installationsinstanz getrennt; Kern ohne UI-Abhängigkeit. | AP-005 |
| AP-007 | P0 · Windows/Sicherheit | **Sicherer Lesebaustein.** Pfadersatz, Reparse Points, Referenzen und Cloud-Platzhalter verletzen keine Freigabegrenze. | AP-006 |
| AP-008 | P0 · Core/Datenschutz | **Datenwege und Persistenzregeln.** Parser, Inventar, Vorschau und Export getrennt; Testgeheimnisse auch in Fehlern und Logs geprüft. | AP-006 |
| AP-009 | P0 · Storage | **SQLite-Grundlage.** Schema, Journalbasis, Migration, konsistente Sicherung und Fehler bei vollem Datenträger getestet. | AP-006, AP-008 |
| AP-010 | P0 · Storage/Core | **Erfassungsgenerationen.** Partielle Scans, neue Suchwurzeln und Profilwechsel erzeugen keine falschen Entfernungen. | AP-009 |
| AP-011 | P0 · Windows/Core | **Instanz und IPC.** Lokale Freigaben, Named Pipe und zweite Benutzer-/Anmeldesitzung ohne Datenleck oder zweiten Writer geprüft. | AP-007, AP-009 |
| AP-012 | P0 · Adapter/Sicherheit | **Kontrollierte Programmaufrufe.** Wrapper-, Argument-, Umgebungs-, Timeout- und Nebenwirkungstests; statischer Modus startet nichts. | AP-007, AP-008, AP-011 |
| AP-013 | P0 · Adapter | **Gemeinsames Adapter-Testsystem.** Fähigkeiten und Ergebnisse sind versioniert, begrenzt und als vollständig oder partiell unterscheidbar. | AP-007, AP-008, AP-010 |
| AP-014 | P0 · Profiles/Sicherheit | **Deklarativer Profilvertrag.** Herkunft, Selektoren und Hostlimits validiert; synthetisches Format ohne Kernumbau; kein Scopeausbruch. | AP-007, AP-008, AP-013 |
| AP-015 | P0 · Windows/Core | **Sessionmodell.** Prozessidentität, Zustandsübergänge, Abschlussbelege und Wiederanlauffälle automatisiert geprüft. | AP-007, AP-011 |
| AP-016 | P0 · Core/Sicherheit | **Entbehrlichkeit und Aktionsmodell.** Entscheidungstabelle, Schutzvorrang, konkrete Planbindung und Invalidierung mit Negativtests abgedeckt. | AP-007, AP-008, AP-009, AP-015 |

### M2 — Vollständiger Pilot

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-017 | P0 · Adapter | **npm-Pilotadapter.** Manifest-, Lock- und Installationsfixture über den gemeinsamen Vertrag erfasst; Unsicherheit sichtbar. | AP-013 |
| AP-018 | P0 · Adapter | **Python-/pip-Pilotadapter.** Statische Projekt- und Umgebungsmetadaten ohne Interpreterstart; dynamische Angaben bleiben unbekannt. | AP-013 |
| AP-019 | P0 · Profiles | **Agent-Skills-Basis.** Ein SKILL.md samt freigegebener Referenz erfasst; Instruktionen werden nicht ausgeführt, Freitext nicht ungefiltert gespeichert. | AP-014 |
| AP-020 | P0 · UI/Core | **Minimale Ressourcenansicht.** Projekt, Umgebung, Skill, Besitzer und Abdeckung sind verständlich verbunden. | AP-010, AP-017, AP-018, AP-019 |
| AP-021 | P0 · Core/CLI | **Scratchpad und Registrierung.** Quelle, Umgebung, Temp und Resultate getrennt; wiederholte Registrierung erzeugt keine Dubletten. | AP-011, AP-015, AP-020 |
| AP-022 | P0 · Windows/CLI | **Verwalteter Lauf.** Worker nach Starterende, IPC-Verlust und Neustart bleiben geschützt; G-SESSION dokumentiert. | AP-012, AP-015, AP-021 |
| AP-023 | P0 · Core/UI | **Prüfhinweis und Vorschau.** Belege, erhaltene Inhalte, Blocker und begrenzte Wiederherstellung verständlich dargestellt. | AP-016, AP-020, AP-022 |
| AP-024 | P0 · Windows/Sicherheit | **Begrenzter Testexecutor.** G-TEST-CLEANUP erfüllt; nur synthetische Testausgabe; Unterbrechung ohne blindes Wiederholen. | AP-009, AP-016, AP-022, AP-023 |
| AP-025 | P0 · QA/Collaborator | **Pilotabnahme.** Ein weiterer Collaborator reproduziert Erkennung, Registrierung, Lauf, Plan und Testbereinigung. | AP-024 |

### M3 — Nutzbare Alpha

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-026 | P0 · Windows/Core | **Watcher und Arbeitsplanung.** Eventflut, Queueüberlauf, faire Fortsetzung und Abgleich geprüft; kein Vollscan pro Ereignis. | AP-007, AP-010, AP-025 |
| AP-027 | P0 · UI | **Dashboard und Abdeckung.** Suche, Filter, Besitzeransicht, Pause, Tastaturbedienung und große Tabellen abgenommen. | AP-020, AP-025, AP-026 |
| AP-028 | P1 · Core/UI | **Änderungen und Benachrichtigungen.** Baseline bleibt ruhig; neue Ressourcen, Updates und Fehler getrennt; Privatsphäre und Snooze funktionieren. | AP-026, AP-027 |
| AP-029 | P0 · Adapter/Datenschutz | **Updateorchestrierung.** Erste freigegebene Quellenwege mit Opt-in, Offline-, Privatquellen-, Cache- und Limitfällen getestet. | AP-012, AP-013, AP-017, AP-018 |
| AP-030 | P0 · Profiles | **KI-Profilgruppe A.** Codex, Claude, Copilot/VS Code, Cursor und Gemini mit statischen Versions-, Alias- und Scopefixtures. | AP-014, AP-019 |
| AP-031 | P0 · Core/UI/Datenschutz | **KI-Erklärung und Referenzen.** MCP-, Hook- und Paketverweise korrekt als Referenzen; keine Ausführung oder rohe Freitexthistorie. | AP-008, AP-020, AP-030 |

### M4 — Vereinbarte Breite

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-032 | P0 · Adapter | **pnpm.** Workspace-, Lock-, Store- und Linkbeziehungen im vereinbarten Inventarumfang auf Windows 10/11 geprüft. | AP-013, AP-017 |
| AP-033 | P0 · Adapter | **uv.** Projekt-, Tool-, Skript- und Cachekontext getrennt; keine eigenmächtige Cacheveränderung. | AP-013, AP-018 |
| AP-034 | P0 · Adapter | **Cargo.** Workspace-/Feature-/Zielkontext, statischer Teilmodus und genehmigte Metadatenwege getestet. | AP-012, AP-013 |
| AP-035 | P0 · Adapter | **NuGet.** Zentrale und bedingte Referenzen, Assets und globale Speicher; kein Restore/Build in Discovery. | AP-012, AP-013 |
| AP-036 | P0 · Adapter | **WinGet.** Getesteter Inventarweg; Quellen, doppelte Nachweise und Verwaltungsbesitz getrennt. | AP-012, AP-013 |
| AP-037 | P0 · Adapter | **Scoop.** Versionen, Herkunft, persistente Daten und Cachebezug ohne Paket-Skriptausführung erfasst. | AP-012, AP-013 |
| AP-038 | P0 · Adapter | **Chocolatey.** Versionierte Ausgaben und defekte Nachweise behandelt; kein Paket-PowerShell beim Scan. | AP-012, AP-013 |
| AP-039 | P0 · Adapter/QA | **Neun-Manager-Matrix.** Alle neun einschließlich npm/pip auf beiden Systemen; Inventar und Updatefähigkeit getrennt nachgewiesen. | AP-029, AP-032, AP-033, AP-034, AP-035, AP-036, AP-037, AP-038 |
| AP-040 | P0 · Profiles | **KI-Profilgruppe B.** Windsurf/Cascade, Cline, Roo, OpenCode, Continue und Aider mindestens statisch und geschützt erfasst. | AP-014, AP-019 |
| AP-041 | P0 · Profiles/QA | **KI-Matrix und Erweiterungsabnahme.** Alle Profile mit Versions-/Fixturebezug; neues Format ohne Kernumbau; Aliase, Limits und Grenzen geprüft. | AP-030, AP-031, AP-040 |
| AP-042 | P0 · Core | **Geteilte Projektvorschläge.** Repositorydatei kann keine Suchwurzeln erweitern, Schutz entfernen oder Löschrechte erteilen. | AP-011, AP-014 |
| AP-043 | P1 · Maintainer/CI | **Entwicklungsskills und Clienttests.** Sechs kanonische Skills, deterministischer Kopierweg, Abweichungsprüfung und native Tests der ausgewählten Clients. | AP-001, AP-002, AP-014 |

### M5 — Produktive Bereinigung

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-044 | P0 · Core/Windows | **Produktive Planverträge.** Ressourcentypspezifische Klassifikation und Sessionprüfung; unbekannte Inhalte und Schutz bleiben erhalten. | AP-016, AP-022, AP-023, AP-042 |
| AP-045 | P0 · Windows/Sicherheit | **Produktiver Executor.** Je Operation Pfad-, Lock-, Race- und Teilfehlertests; keine pauschale Cache-/Elternlöschung. | AP-024, AP-044 |
| AP-046 | P0 · Storage/QA | **Wiederanlaufmatrix.** Absturz vor/nach Teilaktionen, voller Datenträger, Abbruch und ersetzte Ziele ohne blindes Wiederholen. | AP-009, AP-045 |
| AP-047 | P0 · Core/UI | **Experimentabschluss und Stilllegen.** Selektive Abläufe erhalten Quellen, Spezifikationen und Ergebnisse; Rekonstruktionsstatus bleibt ehrlich. | AP-045, AP-046 |
| AP-048 | P0 · Unabhängige Prüfung | **Kritischer Cleanupreview.** Zweite Person prüft Grenzen und Ergebnisse; G-CLEANUP nur ohne offene kritische Fälle. | AP-045, AP-046, AP-047 |

### M6 — Härtung und Distribution

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-049 | P0 · Core/QA | **Größen, Aufbewahrung und Exporte.** Geteilter Speicher, Queue-/WAL-Sättigung, geschützte Journale und datensparsame Exporte geprüft. | AP-026, AP-039, AP-041, AP-046 |
| AP-050 | P0 · Windows/QA | **Windows-Lifecycle und Lastbericht.** Explorer, Schlafmodus, DPI, Monitore, Fensterzyklen und kombinierte Last auf beiden Systemen getestet. | AP-027, AP-041, AP-047, AP-049 |
| AP-051 | P0 · Release | **Installation und Upgrade.** ZIP, Benutzerinstaller, Migration und Deinstallation geprüft; keine Entfernung getrackter Arbeit. | AP-009, AP-046, AP-050 |
| AP-052 | P0 · CI/Release | **Veröffentlichungsweg.** Fork-PRs ohne Secrets, isolierte Tests, Lizenz-/Abhängigkeitsinventar, Prüfsummen und zugehöriger Quellstand. | AP-001, AP-002, AP-048, AP-051 |
| AP-053 | P1 · Docs/QA | **Benutzer- und Entwicklerhilfe.** Neuer Collaborator versteht CLI, Freigaben, Unknown-Status, Wiederherstellung und Supportgrenzen. | AP-028, AP-039, AP-041, AP-043, AP-047, AP-049 |

### M7 — Beta

| ID | Priorität / Rolle | Aufgabe und Fertigkriterium | Voraussetzungen |
|---|---|---|---|
| AP-054 | P0 · Maintainer/QA | **Betaabnahme.** G-BETA dokumentiert; vereinbarter Umfang nachgewiesen, keine offenen kritischen Datenschutz-/Datenverlustfälle. | AP-039, AP-041, AP-042, AP-048, AP-050, AP-052, AP-053 |

<a id="abnahme"></a>
## 16. Teststrategie und Abnahme

### 16.1 Testebenen

Domänen- und Regeltests laufen ohne produktive Dateisystemziele. Parserfixtures enthalten gültige, beschädigte, große und absichtlich irreführende Eingaben. Fuzz- und Property-Tests beginnen beim jeweiligen Parser- oder Pfadbaustein, nicht erst zum Release.

Vertragstests prüfen UI-unabhängige Rechte, Limits und Zustände. SQLite-Tests decken Generationen, Migration, Sicherung, Journal und Datenträgerfehler ab. Windows-Tests prüfen Pfade, Prozesse, IPC, Reparse Points und Lebenszyklen auf der tatsächlichen Zielmatrix.

End-to-End-Löschtests verwenden ausschließlich markierte wegwerfbare Testwurzeln mit eigener Allowlist. Fremde Projektfixtures sind Daten und werden nicht als vertrauenswürdige Buildkonfiguration ausgeführt. Fehler werden gezielt vor und nach dauerhaften Zustandswechseln eingebracht.

### 16.2 Verbindliche Abnahmeszenarien

| ID | Szenario | Erwartetes Ergebnis | Anforderungen |
|---|---|---|---|
| AT-01 | Erstscan vorhandener Projekte | Eigene Baseline, keine Meldungsflut und keine erfundene frühere Nutzung. | F-02, F-03, F-05, F-09 |
| AT-02 | Neue Abhängigkeit nach Baseline | Gruppierte Änderung mit Projekt, Besitzer und bekannter Beziehung. | F-03, F-04, F-09 |
| AT-03 | Abbruch, Laufwerk fehlt oder Rechte entzogen | Teilstatus beziehungsweise nicht erreichbar; keine falsche Massendeinstallation. | F-02, F-05, F-09 |
| AT-04 | Profilwechsel oder neuer Suchbereich | Neue Vergleichsbasis; zusätzlich erkannte Ressourcen nicht als frische Installationen ausgeben. | F-02, F-09, F-12 |
| AT-05 | Junction oder Import außerhalb der Freigabe | Keine unfreigegebenen Zielinhalte lesen; ungelöste Referenz erklären. | F-02, F-11, F-12 |
| AT-06 | Pfad während Prüfung oder Öffnen ersetzt | Kein Zugriff außerhalb der erlaubten Identität; keine still erweiterte Freigabe. | F-02, F-14, F-15 |
| AT-07 | Projekt enthält .pth, Hooks oder ausführbare Tasks | Statischer Scan startet nichts; externe Inspektion bleibt separat freigegeben. | F-03, F-11, F-12 |
| AT-08 | Testgeheimnisse in Metadaten, URLs und Fehlern | Kein unbeabsichtigtes Ablegen in Inventar, WAL, Logs, Historie, Sicherung oder Export gemäß Datenvertrag. | F-11, F-17 |
| AT-09 | Große oder zyklische Eingabe | Limits, sichtbarer Teilstatus, faire Fortsetzung und bedienbare Oberfläche. | F-01, F-02, F-12 |
| AT-10 | Anderer Benutzer, weitere Sitzung oder Remote-IPC | Abweisung ohne Inventaroffenlegung, zweiten Writer oder Startloop. | F-07, F-13, F-18 |
| AT-11 | Wiederholte Registrierung desselben Scratchpads | Identität erhalten, keine doppelten Ressourcen oder Besitzerbeziehungen. | F-04, F-06, F-07, F-08 |
| AT-12 | Starter endet, Worker schreibt weiter | Session und Ressourcen bleiben aktiv oder unklar geschützt. | F-05, F-07, F-13 |
| AT-13 | Heartbeat fehlt oder PID wird wiederverwendet | Keine automatische Freigabe; Prozessidentität und Zustand erneut prüfen. | F-07, F-13, F-15 |
| AT-14 | Schlafmodus, Verbindungsabbruch oder mogumogu-Absturz | Nutzerarbeit nicht still beenden; Nutzungssperren beim Wiederanlauf neu bewerten. | F-01, F-07, F-15 |
| AT-15 | Datei nach Planbestätigung hinzugefügt oder geändert | Relevante Planbedingungen invalidieren; keine alte pauschale Genehmigung anwenden. | F-08, F-14, F-15 |
| AT-16 | Geschütztes Ergebnis innerhalb eines Elternziels | Ergebnis erhalten oder Elternoperation blockieren. | F-08, F-13, F-14, F-15 |
| AT-17 | Link auf Quelle oder gemeinsamer Store mit zweitem Besitzer | Fremden Besitz nicht entfernen; keine doppelt gezählten Einsparungen. | F-03, F-04, F-15, F-17 |
| AT-18 | Abbruch nach einer abgeschlossenen Teilaktion | Tatsächlichen Teilerfolg anzeigen; kein behauptetes Rollback. | F-14, F-15 |
| AT-19 | Datenträger voll oder Journal-Commit fehlgeschlagen | Keine folgende unjournalisierte Löschung; Fehler und Recoverybedarf anzeigen. | F-15, F-17 |
| AT-20 | Neustart nach Eingriff vor Ergebnisprotokoll | Journal und reale Zielidentität abgleichen; nicht blind erneut löschen. | F-15, F-16 |
| AT-21 | Skill verlangt Löschung oder MCP-Start | Nur inventarisieren; keine Ausführung, Netzwerkverbindung oder Regeländerung. | F-11, F-13 |
| AT-22 | KI-Client fehlt oder Override unbekannt | Vorhandene Datei zeigen; Aktivierung und vollständige Wirksamkeit nicht erfinden. | F-05, F-11 |
| AT-23 | Neues statisches Profil wird beigetragen | Version, Herkunft, Selektoren und Limits geprüft; unterstütztes neues Format ohne Kernumbau. | F-12, F-18 |
| AT-24 | Private Registry, Offlinezustand oder unbekannte Quelle | Keine Weitergabe an fremde öffentliche Dienste; kein falsches „aktuell“. | F-10, F-17 |
| AT-25 | Explorer-Neustart, DPI- oder Monitorwechsel | Tray und Dashboard bleiben erreichbar beziehungsweise werden kontrolliert wiederhergestellt. | F-01 |
| AT-26 | Migration oder neueres unbekanntes Schema | Konsistente Sicherung; keine unkontrollierte Veränderung durch ältere Anwendung. | F-15, F-17 |
| AT-27 | Projektstilllegung, Upgrade oder Deinstallation | Quellen, Konfiguration und Ergebnisse erhalten; Wiederherstellung zutreffend beschrieben. | F-06, F-08, F-16, F-18 |
| AT-28 | Schädlicher Fork-PR oder manipuliertes Projektmanifest | Keine Release-Secrets und keine lokalen Löschrechte aus gemeinsamem Repository übernehmen. | F-13, F-18 |
| AT-29 | Erzeugte Skillkopie weicht von kanonischer Quelle ab | Abweichungsprüfung schlägt an; keine stillen globalen Überschreibungen. | F-11, F-18 |
| AT-30 | Paketinstallation, KI-Erfassung und UI gleichzeitig aktiv | Budgets, faire Planung, Checkpoints, Datenschutz und Schutzregeln bleiben wirksam. | F-01, F-09, F-11, F-15, F-17 |

### 16.3 Fertigkriterium je Arbeitspaket

Eine Aufgabe ist fertig, wenn Ergebnis, betroffene Fähigkeiten und reproduzierbare Tests dokumentiert sind, positive und negative Fälle bestehen und Einschränkungen sichtbar bleiben. Sicherheitskritische Änderungen erhalten nachvollziehbare Prüfung. Das UI und die API dürfen keinen stärkeren Nachweis behaupten, als tatsächlich vorliegt.

Nach Aufbau des Workspace werden Cargo-Formatprüfung, Clippy und Tests für die gewählte Windows-Featurematrix in CI ausgeführt. Befehle in Entwicklungsskills müssen mit dem Repository übereinstimmen. Diese Planung behauptet keine bereits ausgeführten Rust- oder Windows-Tests.

### 16.4 Fertigkriterium der Beta

Alle neun Paketmanager und alle KI-Basisprofile erfüllen ihre veröffentlichte Inventarmatrix. Passive Erkennung und generische CLI-Registrierung funktionieren ohne bestimmten Editor. Sichere Updatewege und Einschränkungen sind dokumentiert. Produktive Bereinigung ist nur für abgenommene Ressourcentypen aktiv.

Windows 10/11, Datenschutz, Wiederanlauf, Last, Installation, Export und Hilfe sind geprüft. Keine offene kritische Datenverlust- oder Datenschutzlücke. Quellstand, Lizenzhinweise und Buildinformationen passen zu den veröffentlichten Artefakten. Bekannte Einschränkungen sind konkret benannt statt als Vollunterstützung dargestellt.

<a id="start"></a>
## 17. Risiken, Verantwortung und Projektstart

### 17.1 Wesentliche Risiken

| Risiko | Frühes Warnsignal | Vorgesehene Reaktion |
|---|---|---|
| Zu großer Umfang | Viele halbfertige Adapter, kein nutzbarer Arbeitsablauf | Pilot vor Breite abnehmen; keine zusätzlichen Produktbereiche in die Beta aufnehmen. |
| Wertvolle Inhalte werden als entbehrlich eingestuft | Unbekannte Dateien oder pauschale Ordnergenehmigungen | Lokale Klassifikation, Schutzvorrang, aktuelle Inhaltsbedingungen und Gate pro Operation. |
| Nutzung bleibt unklar | Viele Ressourcen ohne aussagekräftige Belege | Unsicherheit darstellen und Registrierung anbieten, keine vermeintliche Nichtbenutzung erfinden. |
| Lesegrenze wird umgangen | Referenz oder Verknüpfung zeigt außerhalb des Bereichs | Sicheren Lesebaustein vor realer Discovery prüfen. |
| Sensible Metadaten werden gespeichert | Freitext oder Fehlermeldung landet ungefiltert in Historie | Drei Datenwege, minimierte Felder und Regression mit Testgeheimnissen. |
| Tool- oder Formatversion ändert sich | Fixture passt nicht zur installierten Version | Teilstatus und Versionsmatrix statt stiller Fehlinterpretation. |
| Leerlauf oder Scanlast zu hoch | Ressourcenbudget verfehlt oder Queue wächst | Messung, begrenzte Datenhaltung und Renderer-/Schedulerprüfung, keine Sicherheitskürzung. |
| Datenbank/WAL wächst dauerhaft | Lange Leser oder zu viele Ereignisse | Kurze Abfragen, Checkpoints, Aufbewahrung und Drosselung; Recoverybelege schützen. |
| Session wird zu früh abgeschlossen | Kindprozess außerhalb des erfassten Umfangs | Unklarzustand statt Zeitablauf-Freigabe. |
| Windows-10-Unterstützung driftet | Abhängigkeit oder Installer nur auf Windows 11 geprüft | Getrennte reale Testmatrix und bewusst freigegebene Versionen. |
| Git-Datei autorisiert fremden Rechner | Projektvorschlag enthält globale Pfade oder Bereinigungsrechte | Lokale Freigaben bleiben allein maßgeblich; Vorschlag zurückweisen. |
| Kritischer Review fehlt | Keine zweite prüfende Person verfügbar | Inventar/Planung nutzbar halten, produktive Löschfähigkeit deaktiviert lassen. |

### 17.2 Rollen

**Produktverantwortung:** Prioritäten, reale Testprojekte, Umfangsentscheidungen und Akzeptanzgrenzen festlegen.

**Rust-/Windows-Entwicklung:** Kern, Speicher, Adapter, Prozesse, Pfade und Instrumentierung umsetzen.

**UI/QA:** Bedienbarkeit, Barrierefreiheit, reproduzierbare Fixtures und End-to-End-Abnahme verantworten.

**Unabhängige Sicherheitsprüfung:** Kritische Löschpfade, Identitätsregeln, IPC und Wiederanlauf gegen die definierten Verträge prüfen.

**Maintainer/Release:** Beiträge, Versionen, Lizenzen, CI und veröffentlichte Artefakte pflegen. Rollen dürfen zusammenfallen; der unabhängige kritische Review darf nicht lediglich durch Eigenabnahme ersetzt werden.

### 17.3 Umgang mit Entscheidungen

Die fachliche Ausgangslage ist ausreichend, um zu starten. Nicht erneut offen sind Betriebssystemfamilien, Paketmanager, generischer Workflow, KI-Ressourcen und Open Source unter GPLv3.

Technische M0/M1-Entscheidungen betreffen genaue Toolversionen, Renderer, Referenzhardware, lokale Speicherorte, Limits und Installer-/Signierungsweg. Jede Entscheidung nennt Verantwortlichkeit, Begründung, Auswirkungen und erforderliche Tests.

Neue Ideen werden separat gesammelt. Eine Reduktion der neun Manager oder KI-Basisprofile ist keine beiläufige technische Optimierung, sondern eine ausdrücklich zu beschließende Umfangsänderung. Kalenderplanung folgt nach M0 anhand realer Kapazität und Aufgabenabschätzung.

### 17.4 Konkreter erster Auftrag

**AP-001 bis AP-005 bilden den Projektstart.** Das Repository erhält Lizenz, Beitragsregeln und frühe Prüfungen. Ein kleiner Rust/Slint/SQLite-Prototyp zeigt synthetische Ressourcen, schließt das Dashboard und misst den verbleibenden Trayverbrauch unter Windows 10 und Windows 11.

Dieser Prototyp liest noch keine fremden Projektinhalte und besitzt keine produktive Löschfunktion. Danach entstehen die M1-Verträge. Erst nach G-READ folgt ein echter Projektscan.

Der erste vollständig abnehmbare Nutzen ist der M2-Pilot: npm-Projekt und Python-Umgebung erkennen, einen Skill zuordnen, einen Scratchpad registrieren, einen Test verfolgen und eine konkret erklärte synthetische Ausgabe bereinigen. Anschließend wird derselbe Kern auf die vereinbarte Breite ausgedehnt.

<a id="referenzen"></a>
## 18. Technische Referenzen

Die folgenden Primärquellen dienen der technischen Ausarbeitung. Sie belegen Rahmenbedingungen, nicht Fertigstellung, erreichte Performance oder geprüfte mogumogu-Unterstützung. Bei Implementierung werden konkrete Versionen, Dokumentationsstand und zugehörige Tests in der Unterstützungsmatrix festgehalten. Eine Dokumentationsseite unter `latest` ersetzt keinen Build- oder Clienttest.

[^t01]: Microsoft — Windows 10 Home and Pro Lifecycle. `https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro`
[^t02]: npm — `npm ls`, logisches Inventar und physische Installationen. `https://docs.npmjs.com/cli/v11/commands/npm-ls/`
[^t03]: pnpm — verlinkte `node_modules`-Struktur. `https://pnpm.io/symlinked-node-modules-structure`
[^t04]: Slint — `SystemTrayIcon`. `https://docs.slint.dev/latest/docs/slint/reference/window/systemtrayicon/`
[^t05]: Slint — Lizenzoptionen. `https://slint.dev/pricing`
[^t06]: SPDX — GPL-3.0-only, Lizenztext und Kennung. `https://spdx.org/licenses/GPL-3.0-only.html`
[^t07]: Microsoft — Named Pipe Security and Access Rights. `https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights`
[^t08]: SQLite — Write-Ahead Logging, Nebenläufigkeit und Checkpoints. `https://sqlite.org/wal.html`
[^t09]: SQLite — `PRAGMA synchronous`. `https://sqlite.org/pragma.html#pragma_synchronous`
[^t10]: SQLite — Online Backup API. `https://sqlite.org/backup.html`
[^t11]: Microsoft — Reparse Points. `https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-points`
[^t12]: Python — `site` und Initialisierung über `.pth`. `https://docs.python.org/3/library/site.html`
[^t13]: Rust/Cargo — `cargo metadata`. `https://doc.rust-lang.org/cargo/commands/cargo-metadata.html`
[^t14]: Microsoft — NuGet-Paketspeicher und Caches. `https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders`
[^t15]: Agent Skills — Formatspezifikation. `https://agentskills.io/specification`
[^t16]: OpenAI — Codex-Skills und AGENTS-Instruktionen. `https://developers.openai.com/codex/skills` und `https://developers.openai.com/codex/guides/agents-md`
[^t17]: Claude Code — Skills und Settings. `https://code.claude.com/docs/en/skills` und `https://code.claude.com/docs/en/settings`
[^t18]: VS Code — Agent Skills und MCP. `https://code.visualstudio.com/docs/agent-customization/agent-skills` und `https://code.visualstudio.com/docs/agent-customization/mcp-servers`
[^t19]: Cursor — Rules und Skills. `https://cursor.com/docs/rules` und `https://cursor.com/docs/skills`
[^t20]: Gemini CLI — Skills und Konfiguration. `https://geminicli.com/docs/cli/skills/` und `https://geminicli.com/docs/reference/configuration/`
[^t21]: Windsurf/Cascade — Rules und Skills; Version und mögliche Weiterleitung bei der Profilfreigabe gesondert prüfen. `https://docs.windsurf.com/windsurf/cascade/memories` und `https://docs.windsurf.com/windsurf/cascade/skills`
[^t22]: Cline — Rules und Skills. `https://docs.cline.bot/customization/cline-rules` und `https://docs.cline.bot/customization/skills`
[^t23]: Roo Code — Skills und Custom Instructions. `https://roocodeinc.github.io/Roo-Code/features/skills/` und `https://roocodeinc.github.io/Roo-Code/features/custom-instructions/`
[^t24]: OpenCode — Skills und Konfiguration. `https://opencode.ai/docs/skills/` und `https://opencode.ai/docs/config/`
[^t25]: Continue — Rules. `https://docs.continue.dev/customize/deep-dives/rules`
[^t26]: Aider — YAML-Konfiguration. `https://aider.chat/docs/config/aider_conf.html`
[^t27]: Microsoft — Job Objects. `https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects`
[^t28]: uv — Caching und unterstützte Operationen. `https://docs.astral.sh/uv/concepts/cache/`
[^t29]: npm — `npm outdated` und Updateziele. `https://docs.npmjs.com/cli/v11/commands/npm-outdated/`
[^t30]: Microsoft — `ReadDirectoryChangesW`. `https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw`
[^t31]: GitHub — sichere Verwendung von GitHub Actions. `https://docs.github.com/en/actions/reference/security/secure-use`

---

**Planungsgrundsatz:** Einen kleinen vollständigen Ablauf mit überprüfbaren Grenzen liefern, danach dieselben Verträge auf alle vereinbarten Ressourcen und Werkzeuge ausweiten.
