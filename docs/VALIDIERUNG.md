# Validierung · mogumogu 0.1.0

Stand: 5. Oktober 2026. Alle Prüfungen liefen auf Windows 10 Pro 22H2 (Build 19045, x64) mit Rust 1.98.0 (MSVC). Was nicht ausgeführt wurde, steht ausdrücklich unten.

## Ergänzung vom 24. Oktober (3): keine Begrüßung bei leerem Inventar

Ein leeres lokales Inventar öffnete bei jedem Start den geführten Einstieg
„Dein Inventar ist noch leer“; „Später“ galt nur bis zum nächsten Start. Die
Übersicht hat dafür schon einen Leerzustand, und für viele Projekte ist die
Laufwerkssuche mit gesammelter Übernahme der bessere Weg. Entfernt sind daher
die automatische Öffnung, der Willkommensschritt, die Einstellung „Geführter
Einstieg“ samt `settings set guide` (ihr einziger CLI-Schalter) und das
ungenutzte `SettingToggle`. **Projekt hinzufügen** (Werkzeugleiste, `Strg+N`)
beginnt jetzt direkt bei der Ordnerwahl; der Leerzustand der Übersicht bietet
daneben **Laufwerk durchsuchen**. Gespeicherte Einstellungen mit
`guide_enabled` bleiben lesbar, das Feld wird ignoriert.

## Ergänzung vom 24. Oktober (2): offene Lücken geschlossen

**Fünf Ökosysteme:** Adapter für Go-Module, Bun, Yarn (v1 und Berry), Gradle
und Maven, statisch und ohne Werkzeugaufruf; package.json zählt zum Manager des
Lockfiles daneben, Bun und Yarn beanspruchen ihr node_modules, Gradles `build`
und Mavens `target` sind Ausgaben und werden nicht durchlaufen. Der Adapterstand
`adapters-4` beginnt eine neue Vergleichsbasis; damit entfällt auch die
einmalige Ersetzungsmeldung aus der Pfadredaktion. Auf I: hat Sparkle jetzt 27
deklarierte und 46 aufgelöste Go-Module (vorher 0), koharu 1.882 aufgelöste
Bun-Pakete (vorher keine Versionen), lumina 45 deklarierte Gradle-Abhängigkeiten
(vorher keine). Bleibt: Mitglieder eines Bun- oder pnpm-Workspaces ohne
eigenes Lockfile zählen ihre package.json zu npm; Buildskripte werden nicht
ausgewertet; Go-Modulcache und Bun-Cache sind noch nicht messbar (Schema v6
lässt neue Cache-Arten nur mit Migration zu).

**Namen mit Hash:** In einem Pfadnamen gilt ein Lauf nur noch als Token, wenn
eines seiner Wörter mindestens 32 Zeichen lang ist, Buchstaben und Ziffern mischt
und kein Hex in einer Schreibweise ist; bekannte Tokenformate werden vorher wie
im Freitext redigiert. Auf I: sind danach 0 von 1.738 Projektfunden und 0 von
23.942 Einträgen der Projekterfassungen redigiert (vorher 167 und 244). Diese
Lockerung der Datenminimierung braucht wie die Pfadredaktion je Name einen
zweiten Review.

**Gefundene Projekte gesammelt übernehmen:** **Funde → Projekte → Alle
übernehmen …** (CLI `project accept-all`) zeigt die sicheren äußeren Projekte
vollständiger Projektsuchen und registriert sie erst nach Bestätigung per
Fingerabdruck, jeweils mit identitätsgebundener Lesefreigabe; ein nicht
freigebbarer Ordner wird mit Grund ausgelassen und nicht registriert. Auf I:
bot die Vorschau 18 Projekte an (alle elf echten und sieben weitere Checkouts),
die Übernahme dauerte 0,1 s, ein falscher Fingerabdruck änderte nichts.

Tests: je Adapter positive und negative Fixtures, eine Erfassung mit allen fünf
Ökosystemen samt nicht durchlaufenen Ausgaben, Namen mit Hash und Tokens, die
gesammelte Übernahme mit falschem Fingerabdruck, ausgelassenem Ordner und ohne
Doppelangebot. `validate.py` (15 Verträge), `cargo fmt`, `cargo clippy
--all-targets -D warnings` und **190 Funktionstests** bestanden. Native Bilder
und der Layout-Audit wurden nicht neu erzeugt.

## Ergänzung vom 24. Oktober: realer Bestand, Lücken und Verschlankung

**Messung am realen Bestand:** Laufwerkssuche über I: (626 GB belegt) und elf
echte Projekte in einem eigenen Datenverzeichnis, Release-Build; Zahlen im
[Messbericht](MESSBERICHT.md#realer-bestand-24-oktober-2026). Die Messung fand
die folgenden Lücken.

**Pfadredaktion:** `privacy::redact` hielt einen ganzen relativen Pfad für ein
Token, sobald er ohne Punkt mindestens 32 Zeichen lang war und eine Ziffer
enthielt (`/` gehört zur Base64-Zeichenklasse). Auf I: waren dadurch 85 % der
Projektfunde redigiert und nicht registrierbar, in Projekterfassungen 13 % aller
Einträge. `privacy::path` redigiert jetzt Name für Name; ein Token innerhalb
eines Namens bleibt redigiert (etwa ein 40-stelliger Commit-Hash im Ordnernamen).
Pfad und Kontext von Inventareinträgen verwenden es. Weil der Schlüssel eines
Eintrags aus dem redigierten Pfad entsteht, meldet die erste vollständige
Erfassung nach dem Update betroffene Einträge einmal als entfernt und neu;
Behalten-Entscheidungen hängen an der Paketidentität und bleiben.

**Caches mit eigenem Namen:** Ein uv-Cache und Hugging-Face-Caches wurden
betreten; ihre entpackten Pakete erschienen als 1.800 Skripte, Scratchpads und
Projekte. Ein Ordner mit `CACHEDIR.TAG` ist jetzt ein Cache-Fund, dessen Inhalt
nicht betreten wird; benannte Ausgaben wie `target` behalten ihre Art.

**Reihenfolge und Sicherheit:** Bei gleicher Sicherheit stehen Teilprojekte nach
den äußeren Projekten (ein Workspace füllte sonst die 200 Zeilen). Ein Repository
mit eigener CI gilt auch ohne starkes Manifest als sicher (CMake-/Make-Projekte).
Veraltete Beschreibungen von Watcher, Pause und hängenden Experimenten in
ARCHITEKTUR, ENTWICKLUNG und `main.rs` sind korrigiert.

**Entfernt:** die Ordnerstruktur unter **Speicher** samt `storage tree` (ein
allgemeiner Dateibrowser, den Explorer besser kann und der weder Projekte,
Abhängigkeiten noch KI-Zugriff verwaltet; den Speicher eines Projekts zeigt der
Aufräumbericht) und die Suchpfadprüfung auf WinGet, Scoop und Chocolatey, deren
Listen schon am 20. Oktober entfielen. Der Aufräumbericht ist jetzt Reiter 1.

Tests: Pfade Name für Name, Cache-Marker als Blatt, Teilprojekte nach äußeren
Projekten, CMake-Repository mit CI. `validate.py` (15 Verträge), `cargo fmt`,
`cargo clippy --all-targets -D warnings` und **178 Funktionstests** bestanden;
die fünf Tests der Ordnerstruktur entfallen. Native Bilder und der Layout-Audit
wurden für diese Änderung nicht neu erzeugt.

**Offen gebliebene Lücken:** Go-Module, Bun- und Yarn-Lockfiles sowie
Gradle/Maven werden nicht inventarisiert (auf I: betrifft das drei der elf
Projekte); gefundene Projekte lassen sich nur einzeln registrieren und
freigeben; Ordnernamen mit Hash bleiben redigiert und damit nur manuell
registrierbar.

## Ergänzung vom 21. Oktober: Lückenprüfung

**Behoben:** Der Prüfhinweis „Inaktiv“ beruhte nach dem Wegfall der Sessions
nur noch auf dem Alter einer Ressource und ist entfernt, samt den
wirkungslosen Prüfzeiträumen und dem verwaisten Aktivitätsfeld. Registrierte
und übernommene Scratchpads erhalten dafür einen Prüftermin (7 Tage).

**Kontextfilter in der persönlichen Datei:** Geschrieben wird nur noch
`.claude/settings.local.json`; die geteilte `.claude/settings.json` bleibt, ihre
Verbote werden gelesen und nicht wiederholt (laut Claude-Code-Dokumentation
werden Listen über alle Dateien zusammengeführt). Weil Claude Code die Datei
nur dann aus Git ausschließt, wenn es sie selbst anlegt, weist die Vorschau in
Git-Projekten ohne passenden `.gitignore`-Eintrag darauf hin. **Kontextfilter
für alle …** (CLI `ai filter --all`) zeigt alle Projekte mit Lesefreigabe in
einer Vorschau mit einem Fingerabdruck und schreibt jedes Projekt für sich;
Projekte mit unlesbarer oder ungültiger Datei werden mit Grund ausgelassen.
Der Papierkorb-Plan sagt jetzt, dass Speicher erst beim Leeren frei wird.
Tests: persönliche Datei statt geteilter, Git-Hinweis, Sammelschritt mit
Auslassung und falschem Fingerabdruck. Danach **174 Funktionstests** und ein
Audit über **448 Zustände** ohne Kandidaten; Bild:
[KI-Zugriff](screenshots/wolke-ki-zugriff.png).

**Testdaten im Aufräumbericht:** Testdaten haben eine eigene Summe (auch
`test_data`, `__snapshots__` und `data`/`resources`/`samples`/`assets` direkt in
einem Testordner); „Viele Testdaten“ steht ab 1 GB oder ab 100 MB, wenn sie
mindestens die Hälfte des eigenen Projektinhalts ausmachen. Tests: Summe über
verstreute Ordner, Schwelle mit unvollständigem Durchgang. Danach **176
Funktionstests**; Bild: [Aufräumbericht](screenshots/wolke-aufraeumen.png).

**Zweiter Review von Lese- und Schreibgrenze:** Laufwerkssuche und Ablageorte
ohne blockierenden Befund, Kontextfilter mit zwei, beide behoben. (1) Die
Hilfsdatei hatte einen festen Namen und wurde über den Pfad geschrieben; ein
Hardlink oder Symlink an diesem Namen hätte den Inhalt außerhalb des Projekts
abgelegt. Jetzt wird `.claude` ohne Löschfreigabe gehalten, die Hilfsdatei mit
eindeutigem Namen relativ zu diesem Handle neu angelegt (schlägt fehl, wenn dort
etwas liegt) und mit `NtSetInformationFile` im selben Ordner über das Ziel
umbenannt; ein Ziel, das selbst eine Verknüpfung ist oder weitere Hardlinks hat,
wird abgelehnt, und bei jedem Fehler wird die Hilfsdatei gelöscht. Direkt vor
dem Ersetzen wird geprüft, ob Claude Code die Datei inzwischen geändert hat.
(2) `Read(!.env.example)` hätte laut Claude-Code-Dokumentation frühere
Verbote derselben Datei aufgehoben und ist entfernt. Außerdem: Fingerabdruck an
Projekt und Ordneridentität gebunden, keine Regeln für redigierte Namen oder
Namen mit Platzhalterzeichen, Einzeldateien mit genauer Regel, keine Begrenzung
auf 200 Funde, Testdaten aus dem Aufräumbericht, verständliche Fehlermeldungen,
`validate.py` lässt Dateischreiben nur im eigenen Datenverzeichnis und über
`replace_file` zu und erkennt nun auch `#[cfg(all(test, …))]`-Module. Aus dem
Lesegrenzen-Review umgesetzt: Die Auslassungen der Laufwerkssuche gelten auch
unter einem Projektmarker an Laufwerks- oder Profilwurzel, fremde Profile unter
`Users` bleiben außen vor, Suchordner und Bestände nur auf lokalen Festplatten,
`%TEMP%` wird maskiert. Offen und dokumentiert: Ordner, die während einer
Laufwerkssuche unlesbar werden, können frühere Funde als entfernt melden;
Namen in `%TEMP%\claude` kodieren Projektpfade; Schlüsselreihenfolge der
Einstellungsdatei ändert sich beim Schreiben (Werte bleiben); Git-Ausnahmen
außerhalb der Projekt-`.gitignore` sieht die Vorschau nicht. Tests: Hardlink am
Ziel, schreibgeschütztes Ziel ohne Rückstand, keine Negationsregel, Fingerabdruck
je Projekt, Regeln für Ordner und Dateien, fremde Profile, maskiertes `%TEMP%`.

**Paket-Caches** (Schema v6): eigene Freigabeart für npm-, pip- und uv-Cache,
Cargo-Registry, Gradle-Caches, Maven-Repository, pnpm-Store und Yarn-Cache in
einer neuen Spalte `cache`, ohne die Tabelle umzubauen. Der Systembefund bietet
sie mit „Lesen erlauben“ an; sie werden nur gemessen und stehen im
Aufräumbericht mit dem Aufräumbefehl des Werkzeugs. Danach **181
Funktionstests**.

**Zweite Runde des Schreibgrenzen-Reviews:** keine blockierenden Befunde; beide
Lücken gelten als geschlossen (nachgestellt mit Hardlinks an Hilfs- und
Zielname, schreibgeschütztem Ziel, `.claude` als Junction und 60 Junction-
Tauschversuchen während des Schreibens, von denen keiner etwas außerhalb
traf). Umgesetzt aus den nicht blockierenden Punkten: Ablehnungen nennen ihren
eigenen Grund statt „Zugriff verweigert“; Regeln auch für im Inventar
redigierte lange Namen, sobald ein Aufräumbericht die vollen Namen kennt;
lokale Festplatte wird am tatsächlichen Ort des Ordners geprüft
(`final_path`), nicht nur am Laufwerksbuchstaben; Testdaten über eine
gemeinsame Konstante; `NtSetInformationFile` in der Allowlist; die Anleitung
nennt genau, wo Claude Codes Leseverbote greifen. Bleibt: Ein Absturz zwischen
Anlegen und Umbenennen hinterlässt die Hilfsdatei; ein Schreiben von Claude
Code im Millisekundenfenster vor dem Umbenennen ginge verloren.

## Ergänzung vom 20. Oktober: Verschlankung und bessere Projekt-/Scratchpad-Erkennung

**Entfernt** (Phase 1): Updatehinweise über das Netz samt Feature `network`,
HTTP-Client und Quellen-Cache; Ordnerüberwachung (Watcher), Tray-Hinweise,
Tageszusammenfassung, Ruhezeiten, Pause und Autostart; WinGet-, Scoop- und
Chocolatey-Listen samt Registry-Lesecode; der redigierte Export. Bestehende
Datenbanken bleiben lesbar: entfernte Einstellungen werden ignoriert,
Freigaben für Programmlisten beim Start einmal widerrufen (Test
`approvals_for_installed_program_lists_are_retired_once`), ein Starteintrag,
der genau diese Installation startet, wird einmal gelöscht. Das Schema bleibt
v4; alte Tabellen bleiben ungenutzt. `validate.py` erlaubt keinen HTTP-Client
mehr. In `src/` und `ui/` entfallen netto rund 1.800 Zeilen, obwohl Phase 2 Erkennungslogik und Tests ergänzt.

**Erkennung** (Phase 2): Projektfunde tragen eine Sicherheit (hoch, mittel,
gering) aus Markern, Versionsverwaltung, Lockfile und Quellordnern, dazu
Hinweise (Tests, CI, Editor, KI-Dateien). Teilprojekte zeigen auf ihr äußeres
Projekt, Archiv-Downloads und Funde unter `Downloads` werden abgestuft,
mitgelieferter Fremdcode ist ein Abhängigkeitsfund. Scratchpads erkennt
mogumogu zusätzlich an Vorlagen- und Kopiennamen und an Datendateien; auf
Laufwerken erscheinen einzelne Skripte in Ablageorten oder mit Wegwerfnamen.
Der Systembefund bietet Arbeitsordner von Claude Code, JetBrains-Scratches und
`%TEMP%` je mit eigener Namenssuche an. Funde sind nach Sicherheit sortiert und
zeigen ihr letztes Änderungsdatum (auf den Tag gerundet).

`cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
15 statische Verträge und **166 Funktionstests** mit `--no-default-features`
bestanden, dazu `cargo test --bin mogumogu`. Der native Audit in
`target/layout-audit-p2` umfasst **426 Zustände** mit null Kandidaten; ein
erster Lauf fand 162 Überläufe, weil die Ablageorte bei 900 × 560 die Liste aus
dem Fenster schoben (jetzt begrenzt und scrollbar). Native Bilder:
[Funde mit Ablageorten](screenshots/wolke-funde-scratchpads.png),
[Einstellungen](screenshots/wolke-einstellungen-schlank.png),
[Übersicht](screenshots/wolke-uebersicht.png).

**Papierkorb statt Löschen, ohne Sessions** (Phase 4): Verwaltete Läufe
(`mogumogu-cli run`), Sessions samt Job-Objekten und Prozessidentitäten, das
Anlegen von Scratchpads, „Als entbehrlich markieren“ und der Executor, der
Dateien einzeln löschte, sind entfernt (netto rund 530 Zeilen weniger in `src/`,
`ui/` und `migrations/`). Schema v5 löscht nach der automatischen Sicherung die
Sessiontabelle sowie Manifest und Journal des alten Wegs, setzt offene alte
Pläne auf „Ungültig geworden“ und legt `recycle_targets` an (ein Journalzustand
je Ordner). Ein Plan entsteht aus einem frischen Aufräumbericht und enthält nur
wiederherstellbar belegte Ordner mit ihrem Beleg. Vor der Ausführung und erneut
direkt vor jedem Verschieben prüft der Executor an Handles Weg, Identität,
Beleg, `.mogumogu-keep` und die Testwurzel; verschoben wird der ganze Ordner per
`SHFileOperationW` mit `FOF_ALLOWUNDO` und `FOF_WANTNUKEWARNING`, während Wurzel
und Elternordner ohne Löschfreigabe offen sind. Beim Schreiben der Tests fiel
auf, dass ein gewöhnlicher Handle den eigenen Ordner nicht gegen Umbenennen
schützt; deshalb öffnet `DirHandle::open_dir_pinned` den Elternordner jetzt
ohne `FILE_SHARE_DELETE` (Test
`a_pinned_folder_holds_the_way_but_lets_its_children_move`). Ebenso prüfte die
G5-Sperre zunächst nur den Pfad: Ein über eine Junction in der Testwurzel
registriertes Projekt hätte sie umgangen. Jetzt geht sie den Weg von der
Testwurzel handle-relativ bis zum Ordner und vergleicht dessen Identität (Test
`a_project_reached_through_a_junction_in_the_test_root_stays_locked`). Zehn
Windows-Tests mit eigenem Test-Papierkorb decken Erfolg, nicht planbare Ordner,
die G5-Sperre samt Junction, fehlenden Beleg, ersetzten Ordner, Junction, `.mogumogu-keep`, beide
Absturzpunkte und einen fehlschlagenden oder untätigen Papierkorb ab; die
End-to-End-Tests über die CLI enden vor dem Verschieben, weil der
Besitzerprozess den echten Papierkorb verwendet. `validate.py` erlaubt
`SHFileOperationW` nur in `platform/windows.rs` und `platform::recycle` nur im
Executor; `remove_file`, `remove_dir`, `remove_dir_all`, `Command::new` und
`SetFileInformationByHandle` kommen nicht mehr vor. Danach **165
Funktionstests**, 15 statische Verträge, `cargo clippy` ohne Befund und ein
Audit über **448 Zustände** ohne Kandidaten; Bild:
[Papierkorb-Plan](screenshots/wolke-papierkorb-plan.png).

**Getrennter Review des Papierkorb-Wegs, erste Runde:** zwei blockierende
Befunde, beide behoben. (1) Windows liest `foo.\node_modules` als
`foo\node_modules`; mit einer Junction `foo` neben dem Ordner `foo.` hätte die
Shell einen Ordner außerhalb des Projekts verschoben. Jetzt werden Namen mit
Punkt oder Leerzeichen am Ende, reservierten Zeichen oder Gerätenamen
abgelehnt, und die Shell bekommt den echten Ort des gehaltenen Elternordners
(`GetFinalPathNameByHandleW`), nur auf festen Laufwerken (keine RAM-Disk) und
unter 260 Zeichen. (2) Einige Regeln urteilten nur nach dem Namen
(`coverage/` als Quellpaket, `build/` neben `package.json`, `.cache`), und
`.mogumogu-keep` oder `.git` tief im Ordner blieben unbeachtet. Jetzt braucht
ein mehrdeutiger Name die Spur seines Werkzeugs im Ordner, `.cache` und `build`
neben `package.json` sind „Prüfen“, und vor dem Verschieben wird der ganze
Inhalt begrenzt geprüft: kein `.git`, kein `.mogumogu-keep`, keine
Verknüpfung, kein Platzhalter. Nicht blockierend und ebenfalls umgesetzt:
erneute Identitätsprüfung nach dem Journal-Commit, Abgleich zählt nie
gestartete Ordner nicht als verschoben, geschützte Ressourcen sperren,
Markierungsdatei der Testwurzel wird handle-relativ gelesen, die Allowlist in
`validate.py` kennt weitere Lösch- und Verschiebe-APIs, und der Besitzer der
End-to-End-Tests verschiebt in einen Test-Papierkorb (`Owner::start_with`).
Offen und dokumentiert: `FOF_NOERRORUI` bei teilweise gesperrten Ordnern,
der Nachfragedialog ohne Fenster blockiert den Kern, eine Markierungsdatei in
einem geklonten Repository plus `cleanup test-root` hebt die Sperre für diesen
Baum auf, und ob Git eine Ausgabe verfolgt, prüft mogumogu nicht. Danach **170
Funktionstests**.

**Zweite Runde:** keine blockierenden Befunde; der Reviewer hält beide Befunde
für geschlossen und wiederholte seine Reproduktionen (auch mit Leerzeichen am
Namensende und einer Junction oberhalb der Testwurzel). Umgesetzt aus den
nicht blockierenden Punkten: Regelversion `recycle-rules-2`, damit nach alten
Regeln bestätigte Pläne nicht laufen; strengere Spuren (`build` neben
Python-Projekten nicht mehr mit bloßem `lib`, `bin` nur mit `obj` daneben,
`playwright-report` mit `data`); Test für einen Tausch direkt nach dem
Journal-Commit (`a_folder_swapped_after_the_journal_commit_is_not_moved`).
Bleibt dokumentiert: `dist`/`out` brauchen nur die Build-Datei daneben;
Spuren sind Namen, keine Inhalte; ein großer Plan kann die CLI-Zeitgrenze von
120 s überschreiten, während der Besitzer weiterarbeitet (Meldung falsch,
nichts verloren); zwischen der letzten Identitätsprüfung und dem Öffnen durch
die Shell bleibt ein kurzes Fenster, in dem ein ausgetauschter Ordner im
Papierkorb landen würde; ein `.mogumogu-keep` oberhalb der Projektwurzel zählt
nicht; geschützte Ressourcen werden per Pfadtext verglichen. Danach **171
Funktionstests**. **Die Freigabe durch den Nutzer steht aus; bis dahin bleibt
`PRODUCTION_ENABLED` aus.**

**Kontextfilter** (Phase 5b): Vorschau und bestätigtes Ergänzen von
`permissions.deny` in `.claude/settings.json` (`service::filters`). Das ist die
einzige Schreibstelle in Projekte; `validate.py` erlaubt `fs::rename` nur dort.
Tests: Vorschau ohne Wiederholung vorhandener Regeln, kein Schreiben mit
falschem Fingerabdruck, Zusammenführen ohne Verlust anderer Einstellungen,
zweite Vorschau leer, Verweigerung hinter einer Junction. **Der zweite Review
dieser neuen Schreibgrenze steht aus.**

**KI-Zugriff** (Phase 5a): Die Parser lesen aus KI-Einstellungen nur Zahlen und
Schalter: Berechtigungsmodus, zusätzliche Ordner (davon außerhalb), Lese-/
Edit-Verbote, Sandbox, Hooks, MCP-Server mit Ordner außerhalb und neu
Ignore-Dateien von Cursor, Gemini, Windsurf und Aider (Katalog 2026-10-20.1,
Parser `ignore`). **KI-Konfiguration → Zugriff** stuft je Ordner ein. Tests:
Merkmale ohne gespeicherte Pfade oder Muster, Einstufung über Erfassung und
Snapshot. Danach **172 Funktionstests** und ein Audit über **448 Zustände**
ohne Kandidaten; Bild: [KI-Zugriff](screenshots/wolke-ki-zugriff.png).

**Aufräumbericht** (Phase 3): **Speicher → Aufräumen** misst Projekte mit
Lesefreigabe in einem begrenzten Durchgang und teilt sie in wiederherstellbar
(Wiederherstellungsbeleg daneben, Werkzeugcache, Test-Ausgabe), prüfen und
Versionsverwaltung; dazu ruhende Projekte und freigegebene globale Bestände.
**Alle übernehmen** erfasst gefundene Scratchpads geschützt mit Prüftermin.
`AGENTS.md` nennt den Wiederherstellungsbeleg jetzt ausdrücklich als einzigen
Grund für „wiederherstellbar“. Tests: Klassen und Summen des Durchgangs,
Ablehnung einer ersetzten Wurzel, nur freigegebene Projekte, keine Änderung
auf der Platte, Widerruf blendet den Bericht aus, einmalige Übernahme mit
vermutetem Besitzer. Danach **170 Funktionstests** und ein Audit über
**437 Zustände** ohne Kandidaten; Bild:
[Aufräumbericht](screenshots/wolke-aufraeumen.png).

Nicht ausgeführt: eine Laufwerkssuche über ein echtes Laufwerk und eine
Bedienung im echten Fenster nach diesen Änderungen; die Punkte stehen in
Abschnitt L von [WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md). Der zweite Review der
Lesegrenze für Laufwerke und Ablageorte steht weiter aus.

## Ergänzung vom 19. Oktober: Laufwerkssuche, Scratchpad-Erkennung, ohne Experimente-Seite

Die Seite „Experimente“ ist entfernt; das Dashboard hat sechs Ansichten.
Scratchpads stehen unter **Projekte → Ressourcen**, eine hängende Session wird
in ihrem Dialog begründet freigegeben, die Übersicht führt mit „Ressourcen
ansehen“ dorthin. „Scratchpad anlegen“ und der Scratchpad-Ort sind nur noch per
CLI erreichbar (`scratch create`, `settings set scratch-root`).

Neu ist die Laufwerkssuche ([Entwicklungsfunde](ENTWICKLUNGSFUNDE.md#laufwerkssuche)):
`validation::drive_root` lässt eine Laufwerkswurzel nur für die Namenssuche zu,
`approve_system` nur auf lokalen Festplatten und identitätsgebunden. Die
Erkennung lässt System-, Programm- und Werkzeugordner aus, zählt fehlende
Zugriffsrechte als ausgelassen, listet auf Laufwerken keine einzelnen losen
Skripte und meldet mögliche Scratchpads nach Namenswörtern oder losen Skripten.
Die Fundliste sortiert Projekte und Scratchpads zuerst. Ein ganzes Laufwerk wird
nie ausgemessen. **Diese Erweiterung der Lesegrenze braucht nach AGENTS.md
einen zweiten Review; er steht aus.**

Neue Tests: Laufwerksausschlüsse und Funde in einem nachgebildeten
Laufwerksbaum, unveränderte Suchordner, Namenswörter, Auslassungen je Ort,
Budget ohne Dateibytes, Wurzelvalidierung, Ablehnung nicht vorhandener
Laufwerke und anderer Freigabearten, Suchstatus in der Speicherübersicht,
Messverbot und Reihenfolge der Fundliste. `cargo fmt --all -- --check`,
`cargo clippy --locked --all-targets -- -D warnings`, 15 statische Verträge und
**173 Funktionstests** mit `--no-default-features --features network`
bestanden. Der native Audit in `target/layout-audit-drive` umfasst
**416 Zustände** (sechs Seiten) mit null Kandidaten bei
`scripts/analyze_layout.py --strict`. Native Bilder:
[Laufwerkssuche mit Ergebnisleiste](screenshots/wolke-laufwerkssuche.png),
[hängende Session im Ressourcendialog](screenshots/wolke-session-haengt.png),
[Übersicht](screenshots/wolke-uebersicht.png).

Durchsatz: Eine Namenssuche über einen synthetischen Baum mit 100.100 Einträgen
dauerte im Debug-Build bei warmem Dateisystemcache 0,75 Sekunden einschließlich
CLI-Aufruf. Das ist kein Messwert für kalte Caches oder Festplatten mit
Drehteller. Nicht ausgeführt: eine Laufwerkssuche über ein echtes Laufwerk.
Dafür hätten echte Benutzerordner gelesen werden müssen; sie steht als
Abnahmepunkt in Abschnitt K von [WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md).

## Ergänzung vom 18. Oktober: Zusammenführung auf sieben Ansichten

Die Oberfläche wurde von elf auf sieben Ansichten zusammengeführt; Inventar und
Begründung stehen im [Oberflächeninventar](UI-UX-PRUEFUNG.md#zusammenführung-auf-sieben-ansichten--18-oktober-2026).
Geändert wurde nur die Darstellung in `ui/` und `src/desktop/`.

Der native Audit in `target/layout-audit-consolidation` umfasst **445 Zustände**
(sieben Seiten, alle Reiter, Leer- und Filterzustände, Dialoge, Einstieg,
Erfassung, Suche, 100/150/200 %, Breitenwechsel und Scroll-Enden) mit 162.942
Komponenteninstanzen und 27.934 sichtbaren Textblöcken.
`scripts/analyze_layout.py --strict` bestand mit **null Kandidaten**. Ein erster
Lauf fand 72 Kandidaten: Bei 900 × 560 schoben „Werkzeuge & globale Bestände“
und die Übersicht mit Ergebnisleiste die Statusleiste aus dem Fenster; die
Mindesthöhen wurden reduziert. Der Tastaturcheck des Audits bestand.

`cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
alle 15 statischen Verträge und 162 Funktionstests mit
`--no-default-features --features network` bestanden.
`design/screens/render.ps1` rendert wieder alle Ansichten und Reiter.

Nachtrag: Die Bedienung im echten Fenster (W10, 200 % DPI, eigenes
Datenverzeichnis, synthetische Ordner) ist inzwischen über UI Automation und an
das Fenster gesendete Nachrichten durchgeführt; Ergebnisse in Abschnitt J von
[WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md). Dabei behoben: Der Ordnerdialog nahm
`GetForegroundWindow()` als Besitzer und hing sich damit an ein fremdes Fenster,
wenn mogumogu nicht vorne war (jetzt das eigene Dashboard); ein automatisch
gesetzter Anzeigename folgte keiner neuen Ordnerwahl; in den Funden stand die
Projektwurzel als „Projekt · .“ und ein per Name gefundener Ordner zusätzlich
als erfasste Umgebung; Lesefreigaben schnitten den Ordnernamen am Pfadende ab;
der Exportdialog sagte „1 Projekte“. Narrator und Windows 11 bleiben offen.

Windows-CI (19. Oktober): End-to-End-Tests scheiterten zeitweise mit
„All pipe instances are busy“ nach 120 s. Ein IPC-Mitschnitt auf dem Runner
zeigte, dass der Server nie eine Verbindung annahm: Die Owner-Prüfung der CLI
verband und schloss, bevor der Server erstmals `ConnectNamedPipe` aufrief, und
`accept()` setzte die einzige Instanz danach nicht zurück. Behoben in
`PipeListener::accept`; außerdem gilt eine belegte Pipe jetzt als laufender
Owner, statt einen zweiten zu starten. Zwei Plattformtests stellen beide Fälle
deterministisch nach. Vorher scheiterte 1 von 16 wiederholten Gesamtläufen auf
dem Runner (zuvor 3 von 3 CI-Läufen); mit der Korrektur trat in 32 Gesamtläufen
kein Pipe-Stau mehr auf. Davon unabhängig scheiterte
einmal `windows_cleanup::crash_after_a_completed_batch_reports_partial_success`
(alle 100 Dateien noch vorhanden); vermutlich hält auf dem Runner kurz ein
anderer Prozess die frisch erzeugten Dateien offen. Das Bereinigungsmodul
wurde nicht verändert; der Test braucht den zweiten Review (G5).

## Ergänzung vom 18. Oktober: UI/UX und Informationsdarstellung

Alle elf nativen Seiten sowie Tabs, Details, Formulare, Einstieg und Statusanzeigen
wurden auf Informationshierarchie, Verdichtung und vollständige Erreichbarkeit
geprüft. Das [Oberflächeninventar](UI-UX-PRUEFUNG.md) nennt die einzelnen Änderungen.
Die erhöhten Listenabstände und das gleichmäßige Sprechblasen-Padding bleiben bestehen.

Der vollständige native Audit in `target/layout-audit-ui-ux-final` umfasst
**518 Zustände**, 206.839 Komponenteninstanzen, 36.061 sichtbare Textblöcke und
133.335 Geschwisterabstände. `scripts/analyze_layout.py --png --strict` bestand
mit **null Prüfkandidaten**. Enthalten sind 100/150/200-%-Skalierung, lange Texte,
Breitenwechsel, Scroll-Enden, unbekannte Größen, Leer- und Filterzustände sowie
per Pointer-Ereignis geöffnete Details. Der integrierte Tastaturcheck bestätigt
Projektwahl per Pfeiltaste und Tab zum Namenfeld.

Die abschließende Korrektur der Paketfilter wurde zusätzlich in 60 nativen
Zuständen unter `target/layout-audit-ui-ux-packages-final` geprüft: ebenfalls
null Kandidaten (22.806 Komponenten, 4.014 sichtbare Texte). Bei 900×560 beginnt
die Paketkarte nun bei y191 statt y223; der Abstand zur Filterleiste beträgt
14 px und die Liste gewinnt 32 px Höhe. `cargo fmt --all -- --check` und
`cargo clippy --locked --all-targets -- -D warnings` bestanden.

Die 162 Funktionstests mit `--no-default-features --features network --target
x86_64-pc-windows-msvc` und alle 15 statischen Verträge bestanden. Der gesonderte
Referenzlasttest wurde bei dieser Darstellungsänderung nicht erneut ausgeführt.
Ein anfänglicher Testaufruf ohne explizites Ziel traf die noch laufende Audit-EXE;
der oben genannte vollständige Lauf mit MSVC-Ziel bestand anschließend.

Der Analyzer berücksichtigt nun Slints transparente, nicht clippende
Visibility-Wrapper: Deren 0×0-Geometrie bildet keine Inhaltsgrenze; geprüft wird
die tatsächliche übergeordnete Layoutfläche. Echte Höhenfehler in Sessions,
Planordnern und aufgeklappten Laufwerksdetails wurden in den Komponenten behoben.

Native Beispielbilder: [KI-Liste](screenshots/ui-ux-ki.png),
[Pakete](screenshots/ui-ux-pakete.png),
[Speicher](screenshots/ui-ux-speicher.png),
[Einstellungen](screenshots/ui-ux-einstellungen.png). Es sind synthetische Daten.
Screenreader- und Windows-11-Abnahme bleiben offen.

Der abschließende Windows-Release wurde gebaut und mit `build.ps1 -NoRun
-SkipTests` nach `dist/mogumogu` kopiert; beide EXE-Dateien stimmen per Hash mit
den Release-Artefakten überein. Die lokale Instanz wurde nach erneuter Prüfung
auf null offene Sessions geordnet beendet und mit sichtbarem Dashboard neu gestartet.

## Ergänzung vom 18. Oktober: Entwicklungsfunde, Speicher, Skill-Inhalte

Die Windows-Testsuite (`cargo test --locked --no-default-features --features network`,
im nativen Build mit explizitem MSVC-Ziel) bestand mit 162 Tests und einem
ausdrücklich ignorierten Referenzlasttest: 88 Bibliotheks-, zwei CLI- und 72
Integrationstests. Der Referenzlasttest wurde anschließend separat im Release-Modus ausgeführt und bestand ebenfalls. Neu bzw. erweitert sind die Funde/Zuordnung (acht Tests),
Speicher/Ordnerstruktur (sieben) und Skill-Inhalte (sechs). Die 15 statischen
Verträge und Clippy mit `-D warnings` bestanden ebenfalls.

Die neuen Lesewege wurden zusätzlich durch einen jeweils anderen Agenten
geprüft. Dabei korrigiert: doppelte Projektpfade in Laufwerkssummen,
unbegrenztes Laden von Paketmetadaten für die Fundliste, Rekonstruktion
redigierter Pfade, Freigabewiderruf im Viewer und veraltete Ordnerlisten beim
Laufwerkswechsel. Dies ist kein menschlicher Sicherheitsnachweis.

Die native Slint-Softwaredarstellung wurde über `examples/layout_audit.rs`
in 54 Zuständen (900×560, 1080×680 und 1440×900, kurze/lange Texte,
Speicher/Ordnerstruktur/Skill-Viewer/Projektwähler) gerendert und mit
`scripts/analyze_layout.py` geometrisch geprüft: keine Kollisionskandidaten. Die
Ergebnisdateien liegen unter `target/layout-audit-development`. Sichtprüfungen
der echten Slint-Komponenten: [Speicher](screenshots/wolke-speicher.png),
[Ordnerstruktur](screenshots/wolke-ordnerstruktur.png),
[Skill-Inhalt](screenshots/wolke-skillinhalt.png). Diese Bilder verwenden nur
synthetische Daten und stammen nicht aus der HTML-Vorschau.

Ein zusätzlicher nativer Fokus-Probelauf bestätigte den Projektwähler beim
Zuordnen (Pfeiltaste wechselt Auswahl, Tab erreicht den Zweck) und den sichtbaren
ersten Eingabefokus bei normalen Formularen. Alle Sprechblasen verwenden einen
gemeinsamen Innenabstand von 14 px, getrennt von der gezeichneten Spitze.

Windows 11, Screenreader und vollständige interaktive Desktopabnahme bleiben
offen. Umfang und Grenzen: [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md).

## Ausgeführt und bestanden (ursprünglicher Prüfstand)

| Bereich | Ergebnis | Befehl / Beleg |
|---|---|---|
| Nativer Build | Beide EXEs, Release, `--locked` | `.\build.ps1 -NoRun` |
| Formatierung | ohne Abweichung | `cargo fmt --all -- --check` |
| Lint | 0 Warnungen, inkl. `undocumented_unsafe_blocks` | `cargo clippy --locked --all-targets -- -D warnings` |
| Unit-Tests | 71 bestanden | Domäne, Parser, Adapter, Katalog, Redaktion, Protokoll, Donut-Geometrie, CLI-Parser |
| Kernverträge | 22 bestanden | `tests/core.rs`, `tests/core/windows_cleanup.rs`: Migration v1→v2 mit Sicherung, neueres Schema abgewiesen, Generationen, Geheimnisse weder in DB noch WAL, Snapshot/CLI-Review identisch, Absturzmatrix der Bereinigung |
| Windows-Lesegrenze | 8 bestanden | `tests/safe_reading.rs`: echte Junctions, ersetzte Wurzel, Übergröße, Limits, venv/node_modules ohne Ausführung |
| Plattform | 3 bestanden | `tests/platform.rs`: benannte Job-Objekte, PID-Wiederverwendung über Startzeit, Benutzer-SID |
| Ende-zu-Ende | 6 bestanden | `tests/end_to_end.rs`: Besitzer im Testprozess + echte CLI über die Pipe, verwaltete Läufe, Bereinigung in Testwurzel, Negativfälle |
| Referenzlast | bestanden | `tests/performance.rs` (10 000 Pakete), siehe [Messbericht](MESSBERICHT.md) |
| Statische Verträge | bestanden | `python scripts/validate.py` (Schema, Katalog, Skill-Kopien, Allowlists für Löschen/Prozesse/`unsafe`) |
| Headless-Smoke-Test | bestanden | Besitzer `--headless`, Registrierung per CLI, `owner stop`; registrierter Pfad wurde nicht angelegt |
| Native Oberfläche | Screenshots aller acht Bereiche in Demo- und Lokalmodus | [`docs/screenshots/`](screenshots/) – Aufnahmen der echten App, nicht der HTML-Vorschau |
| Fensterlebenszyklus | bestanden | Fensterkreuz (WM_CLOSE) schließt nur das Fenster; Zweitstart holt das bestehende Fenster nach vorn und endet mit Code 0; Detaildialog per Mausklick, Esc schließt; `owner stop` beendet ohne Kindprozesse |
| Tastatur | bestanden | Alt+1…8 wechselt Bereiche (synthetische Eingaben), Fokusrahmen sichtbar |
| Lokaler Durchlauf | bestanden | Dieses Repository registriert, freigegeben, erfasst (542 Cargo-Pakete, 14 KI-Artefakte), Scratchpad angelegt, verwalteter Lauf `completed_verified` |
| Ressourcenmessung | bestanden | `scripts/measure.ps1`, [Messbericht](MESSBERICHT.md) |

Während der Validierung gefundene und behobene Fehler (Auswahl): Marker `requirements*.txt` erkannte `requirements.txt` nicht; Inventarschlüssel enthielt redaktionspflichtigen Rohtext; frisch gestartete Sessions wurden vor dem Andocken als „unklar“ markiert; Snapshot-Abfrage skalierte mit Projekte × Inventar; Pixel-Schrifteinbettung ignorierte Schriftgewichte; fehlender Initialfokus verhinderte Tastaturkürzel; Ressourcenliste und Einstellungsseite hatten Layoutfehler.

## Ergänzungen: Container und UI-Modulaufbau

Am 5. Oktober 2026 zusätzlich geprüft: `scripts/check.py --offline` auf Windows,
`build.ps1 -NoRun`, Docker-Ziel `validated` und
`docker compose run --build --rm checks`. Compose läuft mit UID/GID 10001,
schreibgeschütztem Image und zwei Cache-Volumes. Ein weiterer Compose-Lauf
mit `--core --offline` bestand ohne Dependency-Downloads. Die Docker-Prüfung
enthält 16 portable Kernverträge; die sechs Windows-Session-/Cleanup-Verträge
bleiben im nativen Lauf aktiv. Beide Featurevarianten (mit/ohne Netzwerk) bestanden.

Die native Referenzlast und 100 Fensterzyklen wurden wiederholt, siehe
[Messbericht](MESSBERICHT.md). Die neuen Tastaturflächen, Suche, Formularfehler
und Kontrastwerte sind implementiert und nativ kompiliert; visuelle Abnahme
und Screenreader-Prüfung dieser Änderungen bleiben offen
([Zusatzprüfungen](WINDOWS-ABNAHME.md#e-zusatzprüfungen-nach-dem-modulumbau)).
Vorhandene Screenshots dokumentieren den früheren UI-Stand.

## Ergänzung: Kompaktes Windows-Layout (17. Oktober 2026)

`scripts/check.py --offline` bestand erneut auf Windows: statische Verträge,
Formatierung, Clippy, Kern mit und ohne Netzwerkfeature und Desktop-Binärtests.
Anschließend baute `build.ps1 -NoRun -SkipTests` beide nativen Release-EXEs mit
`--locked`; die Tests waren bereits im gemeinsamen Prüflauf bestanden.
Die Oberfläche wurde mit dem aktuellen Slint-1.18.1-Software-Renderer und den
echten Snapshot- und Dialogaufbereitungen geprüft: alle acht Bereiche in Demo-
und Lokalmodus bei 1080 × 680 und 900 × 560 Pixeln (Maßstab 1). Zusätzlich wurden
ein Ressourcendialog mit vier Aktionen, ein Formular mit erhaltenen Fehlerwerten,
der Beschäftigt-Zustand und eine Liste mit 20 Plänen gerendert und visuell geprüft.

Direkt eingespeiste Slint-Tastenereignisse bestätigten Strg+F, Strg+N mit
Demo-Sperre, F1, F5, Alt+1…8 aus einem Suchfeld, die Sperre bei offenem Dialog
und Fokus nach Esc sowie programmatischem Dialogschluss. Alle Schließwege
verwenden nun dieselbe Funktion zur Fokuswiederherstellung.

Die [aktuelle Abbildung](screenshots/dashboard-compact-render.png) ist ein Render
der nativen Slint-Oberfläche, ohne Systemfensterrahmen. Der Windows-Automationshelfer
lieferte das gestartete Vorschaufenster nicht als bedienbares Ziel. Deshalb sind
diese Renders und Tastenereignisse **keine interaktive Windows-Abnahme**; neue
manuelle Punkte stehen in [WINDOWS-ABNAHME.md, Abschnitt F](WINDOWS-ABNAHME.md#f-kompaktes-fenster-17-oktober-2026).
Frühere Screenshots bleiben historische Belege. DPI, Narrator und Windows 11
wurden für dieses Layout nicht abgenommen.

## Paketbelege (17. Oktober 2026)

`scripts/check.py --offline` prüft den erweiterten Kern mit/ohne Netzwerk,
Clippy, Format, statische Verträge und den nativen Test-Build. Neue Fixtures:

- `tests/package_evidence.rs`: parallele Versionen/Quellen, private Quellen,
  Backoff, erhaltene Warnungen bei Offlinebetrieb, Cache-/Befehlsreferenzen,
  Behalten ohne Warnungsunterdrückung, zeitabhängige Neubewertung sowie
  Migration v2→v3 mit konsistenter Sicherung und Invalidierung des alten Caches.
- `tests/package_scopes.rs`: Dateinamensuche ohne Inhaltslesen, getrennte
  Registrierung/Freigabe, Junctions, ersetzte Wurzel, RECORD-Pfade und `.pth`
  nicht verfolgt/ausgeführt; globale npm-/Cargo-Belege.
- Parserfixtures: genaue Versionsordnung, npm-Abkündigung, Python-Projektstatus
  und Release-Yanks, Cargo-Yanks, NuGet-Alternative, minimale CI-Befehlsnamen,
  unbekannte Installationsformate ergeben keinen leeren Erfolg.

Die tatsächlich kompilierte Slint-Oberfläche wurde mit synthetischen Daten
gerendert: alle acht Bereiche in Demo/Lokal bei beiden Fenstergrößen, dazu
Adapter, Paketdetails und leere Projektsuche; die bisherigen Tastatur-/Modal-
Prüfungen bestanden. [Paketansicht mit Testdaten](screenshots/package-review-render.png)
ist ein Software-Renderer-Bild, kein Screenshot eines OS-Fensters. Neue manuelle
Bedienpunkte stehen in [WINDOWS-ABNAHME, Abschnitt G](WINDOWS-ABNAHME.md#g-paketbelege-und-globale-bereiche-17-oktober-2026).

Zusätzlicher isolierter CLI-/IPC-Durchlauf: Dateinamensuche, Kandidatenregistrierung,
separate Inhaltsfreigabe, globale npm-Erfassung, zwei Versionen desselben Pakets
und Behalten aus Windows PowerShell bestanden; Netzwerk blieb aus. Die
PowerShell-Schlüssel sind jetzt undurchsichtige IDs, keine zu quotierenden JSON-Objekte.

Ein Capture-Test mit einem absichtlich langlebigen Headless-Testbesitzer deckte
geerbte Windows-Pipehandles beim CLI-Autostart auf. Die eigene Besitzer-EXE wird
jetzt über `CreateProcessW` mit explizitem Programmpfad und ohne Handle-Vererbung
gestartet. Nach der Korrektur kehrte erfasstes JSON in 623 ms zurück, während der
Besitzer weiterlief; ein Datenpfad mit Leerzeichen blieb erhalten und Shutdown
beendete den Testbesitzer. Dies ist ein Prozess-/IPC-Test, keine interaktive
Tray-Abnahme. Der normale GUI-Tray ließ sich in dieser Toolsitzung nicht zuverlässig
abnehmen; die gestarteten isolierten Testprozesse wurden beendet.

## Ergänzung: Architektur und Statusabfragen (17. Oktober 2026)

`scripts/check.py --offline` bestand nach dem Architekturumbau: 15 statische
Verträge, Formatierung, Clippy, Kern mit und ohne Netzwerkfeature sowie native
UI-Binärtests. `cargo rustdoc --locked --offline --no-default-features --lib --
-D warnings` erzeugte die Bibliotheksdokumentation ohne Warnungen. Lokale
Quellverweise der [Architekturkarte](ARCHITEKTUR.md) wurden aufgelöst.
`build.ps1 -NoRun -SkipTests` baute anschließend beide nativen Release-EXEs;
die Tests waren zuvor über den gemeinsamen Runner vollständig ausgeführt.
Der abschließende CLI-/IPC-Durchlauf mit diesen EXEs bestand in einer neuen,
isolierten Testdatenbank: Statusgesamtzahlen, Projektsuche, Registrierung,
separate Lesefreigabe, Scans, zwei Paketversionen und Behalten. Netzwerk blieb
aus; der Headless-Testbesitzer wurde danach beendet.

Die Paketprüfung behält ihre öffentliche `Core`-Schnittstelle. Datenzugriff und
Cache liegen im Service-Modul, Referenzindex und Berichtberechnung in privaten
Auswertungsmodulen ohne Kern-, Repository- oder Netzwerkclient. Die bestehenden
Paket-/Scope-Fixtures bestanden. Die Lese- und Löschimplementierungen sowie
Migrationen wurden bei diesem Umbau nicht geändert.

Der neue Statusvertrag in `tests/core.rs` legt alle sechs Sessionzustände und
200 neuere abgeschlossene Sessions an. Der Dashboard-Ausschnitt enthält nur
abgeschlossene Sessions; die Statusabfrage zählt trotzdem die vier älteren
weiterhin sperrenden Sessions. Der Sessionabgleich bleibt vor der Zählung aktiv.
Die Referenzlast misst Status und Snapshot getrennt; Werte im
[Messbericht](MESSBERICHT.md).

Ein früher Gesamtlauf meldete im bestehenden Test
`crash_after_a_completed_batch_reports_partial_success` einmal 100 übrige Dateien
statt eines Teilerfolgs. Der isolierte Test, acht Wiederholungen des vollständigen
Kernvertragstests und die anschließenden Gesamtläufe bestanden. Die Ursache
dieses intermittierenden Fehlers ist nicht bestätigt; die Assertion wurde nicht
abgeschwächt und der Löschpfad nicht geändert. G5 bleibt offen.

## Mochi-Identität und japanische Schrift (18. Oktober 2026)

Die ausgewählte Mochi-Tafel wurde über feste Ausschnitte übernommen; der Export
prüft Quellhash und Pixelgleichheit. Fenster und Sidebar verwenden das Kopf-Icon,
der Tray dessen 64-px-Ableitung. Die gezeichneten Buchstaben bleiben als Wortmarke
erhalten. Für die Bedienelemente sind Zen Maru Gothic Regular/Bold mit vollständigem
Zeichenvorrat eingebettet, inklusive fester Herkunft, Hashes und OFL-Lizenz.

`scripts/check.py --offline` bestand: 15 statische Verträge, Format, Clippy mit
`-D warnings`, Kern mit/ohne Netzwerk sowie Desktop-Binärtest. Die Font-Allowlist
erlaubt nur die beiden dokumentierten Originalfonts und prüft ihre Hashes und
die unveränderte Lizenz. Eine zusätzliche Prüfung der Unicode-Tabellen fand
alle geprüften deutschen Zeichen sowie Hiragana, Katakana und Kanji in beiden
Gewichten.

42 Frames der tatsächlich kompilierten Slint-Oberfläche wurden mit synthetischen
Daten gerendert: alle acht Bereiche in Demo/Lokal bei 1080 × 680 und 900 × 560,
ergänzt um Dialog-, Formular- und Paketansichten. Die bestehenden Tastatur- und
Modalprüfungen bestanden. Visuell geprüft wurden die Übersicht, die schmale
Einstellungsseite und ein japanischer Projektname. Belege:
[Mochi-Übersicht](screenshots/dashboard-mochi-render.png) und
[japanischer Projektname bei Mindestgröße](screenshots/japanese-project-render.png).
Die Bilder stammen vom nativen Software-Renderer und haben keinen OS-Fensterrahmen.
Interaktive Windows-/IME- und DPI-Abnahme bleibt separat in
[WINDOWS-ABNAHME, Abschnitt H](WINDOWS-ABNAHME.md#h-mochi-und-japanische-schrift-18-oktober-2026).

`build.ps1 -NoRun -SkipTests` baute anschließend beide nativen Release-EXEs;
die Tests waren bereits über den gemeinsamen Runner ausgeführt. Eine Prüfung
der gepackten `dist/mogumogu/mogumogu.exe` bestätigte alle neun ICO-Frames
bytegleich im Windows-Ressourcenverzeichnis sowie die drei Original-PNG-Exporte
und beide vollständigen TTF-Dateien im Binary. Die beigelegte
`FONT-LICENSE.txt` stimmt per SHA-256 mit der Original-OFL-Datei überein.

## Mochi-Ausdrücke je Ansicht (18. Oktober 2026)

Die drei kleinen Ausdruckszeichnungen wurden als pixelgleiche Ausschnitte der
unveränderten Originaltafel exportiert. `Mascot` in `ui/components.slint` bindet
den Seitenindex an die Pose: Übersicht fröhlich, Projekte/Ressourcen mit Ordner,
Pakete/KI/Prüfliste konzentriert sowie Aktivität/Einstellungen neutral. Die
Layoutgrenzen und die dekorative Accessibility-Rolle bleiben gleich.

`scripts/check.py --offline` bestand vollständig: 15 statische Verträge, Format,
Clippy, Kern mit/ohne Netzwerk und Desktop-Binärtest. Mit dem neu kompilierten
Slint-Layout wurden erneut 42 native Frames für Demo/Lokal und beide Fenstergrößen
erzeugt; Tastatur- und Modalprüfungen bestanden. Die Übersicht, Paketansicht und
Einstellungsseite wurden visuell geprüft. Belege: [fröhlich](screenshots/dashboard-mochi-render.png),
[konzentriert](screenshots/mascot-focused-render.png), [neutral](screenshots/mascot-neutral-render.png).
Dies sind Ausgaben des nativen Software-Renderers, keine OS-Fensteraufnahmen.

Der native Release-Build mit `cargo build --locked --offline --release --bins
--target x86_64-pc-windows-msvc` bestand. Wegen der noch laufenden bisherigen
App wurde die neue Fassung mit CLI und Lizenzen unter `dist/mogumogu-expressions`
bereitgestellt. Die Prüfung der fertigen EXE bestätigte die sechs PNG-Assets,
beide vollständigen Fonts und alle neun Windows-Iconframes bytegleich; die
beigelegte Fontlizenz entspricht dem Original. Zum Verwenden wird die bisherige
Instanz regulär im Tray beendet und anschließend die neue EXE gestartet.

## Vereinheitlichte Mochi-Zeichnungen v2 (18. Oktober 2026)

Die vier bisherigen Posen wurden mit dem eingebauten `image_gen`-Werkzeug in
einem gemeinsamen 2×2-Zeichensatz angeglichen. Der vollständige
[Prompt](../design/brand/consistent-v2-prompt.md) und die unveränderte PNG-Ausgabe
bleiben nachvollziehbar; die ursprünglichen Einzelzeichnungen sind archiviert.
Icon, Wortmarke und Fontdateien stammen weiterhin aus denselben Quellen.

Die Anwendung verwendet `assets/brand/mascots-v2.png` (1254 × 1254 Pixel).
`Mascot` wählt native Bildausschnitte von 577 × 577 Pixeln aus den vier
627-Pixel-Zellen, jeweils mit demselben 25-Pixel-Innenrand. Damit sind Maßstab
und Konturbehandlung bei der Anzeige vergleichbar; die Seitenzuordnung bleibt
im Darstellungscode.

`scripts/check.py --offline` bestand vollständig: 15 statische Verträge,
Formatierung, Clippy, Kern mit/ohne Netzwerk und Desktop-Binärtest. 42 Frames
des tatsächlich kompilierten Slint-Layouts wurden erneut in Demo/Lokal und bei
1080 × 680 sowie 900 × 560 gerendert; Tastatur- und Modalprüfungen bestanden.
Alle vier Posen wurden visuell geprüft: [fröhlich](screenshots/dashboard-mochi-render.png),
[mit Ordner](screenshots/mascot-folder-render.png), [konzentriert](screenshots/mascot-focused-render.png),
[neutral](screenshots/mascot-neutral-render.png). Die Bilder stammen vom nativen
Software-Renderer; eine interaktive OS-/DPI-Abnahme ist separat dokumentiert.

`build.ps1 -NoRun -SkipTests` baute nach dem vollständigen Prüflauf beide nativen
Release-EXEs und stellte sie regulär unter `dist/mogumogu` bereit. Die fertige
EXE enthält den unveränderten v2-Bogen, die bisherigen Icon-/Wortmarkendateien,
Tray und beide vollständigen Fonts. Alle neun Windows-Iconframes sind bytegleich
zum bisherigen ICO; die vier archivierten v1-Posen werden nicht zusätzlich in
die EXE eingebettet. Die beigelegte OFL-Datei wurde per SHA-256 abgeglichen.

## Gemessene Mochi-Konturen (18. Oktober 2026)

Die [Messung im nativen Renderer](../design/brand/measurement/README.md) umfasst
alle acht Ansichten, zwei Fenstergrößen und fünf DPI-Stufen (100/125/150/200/400 %).
Alle 80 Posenvergleiche sind bei derselben DPI-Stufe pixelgleich über ihre
zugehörigen Ansichten und beide Fenstergrößen. Acht 100-%-Ausschnitte stimmen
zusätzlich bytegleich mit den bereits geprüften nativen Demo-Frames überein.

Je 43 korrespondierende Kopf-/Ohrkonturen wurden quer zum Strich als
Halbwertsbreite gemessen. Mediane bei 400 %, zurückgerechnet auf logische Pixel:
Neutral 1,69, Fröhlich 1,73, Ordner 1,62, Laptop 1,71. Der größte Abstand beträgt
0,107 px bzw. 6,6 % relativ zur dünnsten Pose; eine exakt konstante Strichstärke
wird nicht behauptet. Native Kalibrierlinien von 0,75/1/1,5/2 px werden bei
400 % auf 0,01 px genau zurückgemessen. Rohwerte, Quellhashes und Messstellen
liegen beim Bericht. Die interaktive Windows-/Monitor-Abnahme bleibt separat.

Die fertige Release-EXE unter `dist/mogumogu` wurde erneut geprüft: v2-Bogen,
Original-Icon/-Wortmarke, Tray und beide japanischen Fonts sind eingebettet;
alle neun Windows-Iconframes sind bytegleich. Die archivierten v1-Posen sind
nicht zusätzlich in der EXE enthalten.

## Systembefund, Einstieg, Experimente, Tray und Mochi-Wolke (18. Oktober 2026)

`python scripts/check.py --offline` bestand vollständig: `validate.py` (15
Verträge), Formatierung, Clippy mit `-D warnings`, Tests mit und ohne
Netzwerkfeature (83 bzw. 80 Kern-Unit-Tests plus Integrationstests) und der
Desktop-Binärtest. Neue Tests:

| Test | Prüft |
|---|---|
| `inventory::system` (2 Unit-Tests, nur Windows) | Befehle, Shims, Bestände und KI-Ordner aus einer synthetischen Umgebung; relative Suchpfade übersprungen; kein Benutzername in Orten; Zuordnung von Ordnern zu Katalogpräfixen |
| `user_ai_folder_is_read_only_along_catalog_routes` | Persönlicher KI-Ordner: nur Katalogrouten betreten, Verläufe und Zugangsdateien nie gelesen |
| `user_folders_are_entered_only_along_catalog_routes` | Routenberechnung aus dem Katalog, ein Platzhalter je Segment |
| `v3_scopes_keep_their_meaning_and_user_folders_get_their_own_realm` | Migration 004 ändert bestehende Freigaben nicht; Benutzerordner eigener Bereich |
| `stuck_sessions_are_named_and_released_only_without_live_processes` | Hängende Sessions benannt; Freigabe gesperrt bei Aktivität, lebendem Prozess oder „läuft sehr lange“ |
| `search_spans_areas_and_needs_two_characters` | Suche über Bereiche, `LIKE`-Sonderzeichen maskiert |
| `export_writes_a_new_redacted_file_and_never_overwrites` | Export in neue Datei, vorhandenes Ziel ergibt Konflikt |
| `new_settings_are_validated_and_a_pause_keeps_the_choice` | Ruhezeiten, Zusammenfassungsstunde, Pause; alte Einstellungen behalten Standardwerte |
| Domänentests in `session.rs`, `settings.rs`, `clock.rs` | Hängen-Klassen, Ruhezeiten über Mitternacht, lokale Stunden aus dem Versatz |

Auf dem Entwicklungsrechner (W10) lief der Systembefund in 9,7 ms. Seine Funde
entsprachen der Beobachtung in [SYSTEMERKENNUNG](SYSTEMERKENNUNG.md), darunter der
pyenv-Shim ohne globale Version und das ältere uv-Tool-Layout.

Native Renderings über `design/screens/render.ps1` mit synthetischen Daten wurden
visuell geprüft: alle zehn Ansichten, Paket-Erklärpanel, hängendes Experiment,
Suche, laufende Erfassung, Pause, alle Einstiegsschritte, 900 × 560 und 150 %.
Dabei behoben: abgeschnittene Knöpfe in Hinweiskarten, zu niedriges Suchfenster,
überlaufende Chips und Tabellenspalten bei 900 × 560 und sichtbare
Papierkanten der Figur auf weißen Karten. Belege:
[Übersicht](screenshots/wolke-uebersicht.png),
[900 × 560](screenshots/wolke-uebersicht-900x560.png),
[150 %](screenshots/wolke-uebersicht-150.png),
[Pakete](screenshots/wolke-pakete.png),
[hängendes Experiment](screenshots/wolke-experiment-haengt.png),
[System](screenshots/wolke-system.png), [Suche](screenshots/wolke-suche.png),
[Erfassung](screenshots/wolke-erfassung.png), [Pause](screenshots/wolke-pausiert.png),
[Einstieg](screenshots/wolke-einstieg.png),
[Lesefreigabe im Einstieg](screenshots/wolke-einstieg-freigabe.png).

Die Stimmungsposen erscheinen zusätzlich neben den zugehörigen Entscheidungen:
besorgt im Detail eines hängenden Experiments, schlafend auf der Karte
„Überwachung“ während der Pause, mampfend (40 px) in der Fortschrittsleiste.
Der Bogen ist vollständig und byte-identisch in `mogumogu.exe` eingebettet
(Slint `EmbedFiles`, per Bytevergleich geprüft).

Der Stimmungsbogen ist deterministisch: Zwei Läufe von
`design/brand/moods/derive.py` ergaben denselben SHA-256; der v2-Bogen blieb
unverändert (`a58212b3…`).

Einmal beobachtet: `windows_cleanup::crash_after_journal_commit_is_reconciled_not_repeated`
schlug in einem Gesamtlauf fehl (`plans_recovery` ≠ 1), danach in zehn weiteren
Läufen (einzeln und im Paket) nicht mehr. Bereinigungscode und Test sind in dieser
Phase unverändert. Ursache noch offen; vermutlich wurde ein Plan wegen
gleichzeitiger Dateizugriffe vor der Ausführung abgebrochen.

## Nicht ausgeführt

| Bereich | Grund |
|---|---|
| Bedienung von Systembefund, Einstieg, Tray-Pause, Autostart und Ordnerdialog im echten Fenster | Manuelle Abnahme ausstehend ([WINDOWS-ABNAHME](WINDOWS-ABNAHME.md), Abschnitt I) |
| Zweiter Review der erweiterten Lesegrenze (Systembefund, `user-ai`) | Nach AGENTS.md erforderlich; erfordert eine zweite Person |
| Linux-Kernprüfung im Container für diese Phase | Nicht ausgeführt; plattformabhängige neue Tests sind auf Windows beschränkt, die Stubs in `platform/fallback.rs` wurden nur per Abgleich der Signaturen geprüft |
| Windows 11 | Kein Windows-11-System verfügbar |
| Tray-Klick und Tray-Menü per Maus | Nicht automatisiert; Menüaktionen sind über dieselben Anwendungsfälle per CLI getestet |
| DPI 125–200 %, Mehrmonitor, Explorer-Neustart, Schlafmodus | Manuelle Abnahme ausstehend; Slint meldet das Tray-Symbol nach `TaskbarCreated` selbst neu an |
| Screenreader (Narrator) | Accessibility-Rollen und -Beschriftungen gesetzt, nicht mit Narrator abgenommen |
| Reale Toolversionen der Paketmanager | Fixtures statt installierter npm/pnpm/uv/NuGet/Scoop/Chocolatey-Bestände (außer Cargo dieses Repos) |
| Updatehinweise gegen echte Registries | Netzwerkpfad ist implementiert, aber standardmäßig aus; in Tests ohne Netz geprüft |
| GitHub-Windows-CI | Workflow eingerichtet, erster Lauf nach dem Push |
| Unabhängiger Review der Löschpfade (G5) | Erfordert eine zweite Person |
