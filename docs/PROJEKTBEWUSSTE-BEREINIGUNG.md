# Projektbewusste Bereinigung: Spezifikation und Umsetzung

Stand: 31. Oktober 2026. Grundlage ist die externe Spezifikation „Project-Aware
Cleanup Engine“ (v2, Windows zuerst, mit KI-Agenten, Scratchpads und Docker).
Dieses Dokument ordnet ihre Anforderungen dem Code zu und hält fest, was
bewusst nicht umgesetzt ist. Verbindlich bleiben [AGENTS.md](../AGENTS.md), die
[Unterstützungsmatrix](UNTERSTUETZUNGSMATRIX.md) und G5: Produktive Bereinigung
bleibt gesperrt (`cleanup::PRODUCTION_ENABLED = false`).

Der Grundsatz beider Seiten ist derselbe: Alt, groß, ignoriert oder ohne
gefundenen Verweis heißt nicht entbehrlich. Ein Fund ist Evidenz, keine
Erlaubnis. mogumogu geht dabei einen Schritt weiter als die Spezifikation:
Läufe, ihr Zustand, überwachte Datenbanken und Docker können Ordner nur
**schützen**, nie freigeben. Eine Haltung endet nur, wenn du sie im Dashboard
aufhebst; danach gelten wieder die gewöhnlichen Regeln des Aufräumberichts.
Eine Aufbewahrungsfrist, nach der etwas entfernbar würde, gibt es nicht.

## Module der Spezifikation

| Spezifikation (§3) | mogumogu |
|---|---|
| `ProjectRegistry` | Projekte und getrennte, identitätsgebundene Lesefreigaben ([storage/projects.rs](../src/storage/projects.rs), [storage/scopes.rs](../src/storage/scopes.rs)) |
| `FilesystemAdapter` | [fsread.rs](../src/fsread.rs) und [platform/windows.rs](../src/platform/windows.rs): handle-relativ, ohne Reparse Points, Identität aus Volume und 128-Bit-ID |
| `InventoryStore` | Generationen ([storage/generations.rs](../src/storage/generations.rs)) und der Verlauf der Aufräumberichte ([storage/history.rs](../src/storage/history.rs)) |
| `DetectorRegistry` | Regeln in [inventory/tidy.rs](../src/inventory/tidy.rs): Name, Datei daneben, Werkzeugspur darin |
| `ReferenceAnalyzer` | Git-Index ([inventory/git.rs](../src/inventory/git.rs), [parsers/git_index.rs](../src/parsers/git_index.rs), mit geprüfter Prüfsumme); Paketbelege der [Paketprüfung](PAKETPRUEFUNG.md) |
| `OwnershipResolver` | Besitzerbelege, Projektzuordnung; Läufe je Projekt; Docker-Einbindungen je Projekt |
| `DecisionEngine` | Klasse nach dem Durchgang (`TidyWalk::settle`) und [cleanup/planner.rs](../src/cleanup/planner.rs) |
| `PolicyGuard` | [cleanup/gate.rs](../src/cleanup/gate.rs) und [cleanup/holds.rs](../src/cleanup/holds.rs): Weg, Identität, Beleg, Git-Stand, Schutz, Haltungen, Testwurzel bis G5 |
| `CleanupExecutor` | [cleanup/executor.rs](../src/cleanup/executor.rs): Journal vor jeder Bewegung, Abgleich nach Absturz |
| `MetricsService` | logische Größe, belegter Platz (Hardlinks ab 64 KiB einmal), freier Speicher des Laufwerks vor und nach einem Plan, Wachstum aus dem Verlauf, Größenverlauf überwachter Datenbanken, Docker-eigene Speicherbilanz getrennt davon |
| `AgentRunRegistry`, `ArtifactProvenanceStore` | [storage/runs.rs](../src/storage/runs.rs), [service/runs.rs](../src/service/runs.rs): Läufe und Artefakte, die ein Harness meldet |
| `HarnessIntegration` | CLI `run …` ([commands.rs](../src/bin/cli/commands.rs)), nur wenn eingeschaltet; Label `io.mogumogu.run` an Docker-Objekten |
| `DockerAdapter` | [docker/](../src/docker/mod.rs): nur `GET` über die Named Pipe eines freigegebenen lokalen Kontexts |
| (Erweiterung) Datenbanküberwachung | [service/databases.rs](../src/service/databases.rs), [inventory/databases.rs](../src/inventory/databases.rs), [storage/databases.rs](../src/storage/databases.rs): registrierte Datenbanken, nur aus Metadaten gemessen und gehalten |
| `RetentionPolicy`, `GarbageCollectionPlanner` | bewusst nicht: nichts wird nach einer Frist entfernbar (siehe unten) |
| `LocalAIAdvisor` | nicht umgesetzt |

Die vier Ergebnisse der Spezifikation (§5) entsprechen: `ELIGIBLE_FOR_APPROVED_CLEANUP`
einem wiederherstellbaren Ordner in einem Plan ohne Blocker, `REVIEW_REQUIRED`
der Klasse „Prüfen“, `PROTECTED` allem, was nie ein Eintrag wird (Quellen,
Testcode, Lockfiles, KI-Konfiguration), geschützten Ressourcen,
`.mogumogu-keep` und gehaltenen Ordnern, `UNSUPPORTED` einem Blocker (kein
Papierkorb, Verknüpfung darin, Pfad über 259 Zeichen, Docker nicht erreichbar).

## Umgesetzt

- **Git als Referenz (§5, §9.4).** Ein wiederherstellbarer Ordner in einem
  Git-Repository zählt nur, wenn kein Index auf dem Weg eine Datei darin führt.
  Der Index wird statisch gelesen, nie über `git`, und seine Prüfsumme (SHA-1
  oder SHA-256) wird geprüft. Ein unlesbarer Stand bleibt unbekannt und macht
  den Ordner zu „Prüfen“. Plan und Ausführer prüfen das erneut. Innerhalb eines
  Entwurfs oder einer Ausführung wird ein geprüfter Index bis 32 MB nur einmal
  gelesen und nach Identität, Größe und letzter Änderung wiedererkannt; jedes
  Ziel öffnet die Datei trotzdem neu. Regeln:
  [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md#aufräumbericht).
- **Haltungen (§6, §9.13, §21).** Was an seinem Ort bleiben muss, gilt für den
  echten Ort des Ordners und damit für jedes Projekt, das ihn erreicht: Ordner,
  die du behältst (Schema v8), Artefakte registrierter Läufe, überwachte
  Datenbanken (Schema v10) und Ordner, die ein Container eines freigegebenen
  Docker-Kontexts einbindet. Eine Haltung umfasst den Ordner, alles darin und
  jeden Ordner, der ihn enthält. Ihr echter Ort wird je Vorgang einmal
  bestimmt; ein symbolischer Link oder eine Junction, die auf eine Freigabe
  oder ein nicht festes Laufwerk führt, wird dabei nicht verfolgt.
- **KI-Läufe (§17–§19).** Kooperierende Harnesses registrieren über die CLI
  Läufe, Artefakte und Docker-Objekte und melden den Zustand – aber erst, wenn
  du es im Dashboard einschaltest (ein Agent kann das nicht selbst). Jedes registrierte
  Artefakt ist geschützt, gleich in welchem Zustand der Lauf ist; der Bericht
  nennt den Lauf als belegte Herkunft. Kein Befehl entfernt einen Lauf oder ein
  Artefakt. Ein beendeter Lauf bleibt beendet. Einen beendeten Lauf kannst nur
  du im Dashboard **freigeben** (Schema v10): Lauf und Artefakte bleiben
  gespeichert, gehalten wird aber nichts mehr. Über CLI und Pipe ist das
  abgelehnt.
- **Überwachte Datenbanken (Erweiterung).** Du registrierst eine Datenbankdatei,
  einen Datenordner oder ein Docker-Volume eines freigegebenen Kontexts; die
  CLI darf das auch (`db register`). mogumogu verbindet sich mit keinem Server
  und liest keine Datenbankdatei: gemessen werden nur Größe, Dateizahl und
  letzte Änderung, bei einer Datei mit `-wal`, `-shm` und `-journal` daneben,
  bei einem Volume die Docker-Bilanz. Jeder vollständige Bericht misst Dateien
  und Ordner mit, Volumes nur auf Wunsch; je Datenbank bleiben 100 Messungen.
  Eine überwachte Datenbank ist gehalten. Vorschläge kommen aus bekannten
  Installationsordnern (nur Namen), den Datenbankdateien des letzten Berichts
  und Volume-Namen. Die Überwachung beenden kannst nur du im Dashboard.
- **Docker, nur lesend (§20).** Kontexte werden aus ihrer `meta.json` erkannt;
  freigeben lässt sich nur einer mit einer Named Pipe dieses Rechners. Gefragt
  werden Version, Container, Images, Volumes, Netze und auf Wunsch die
  Speicherbilanz (`/system/df`), alles über `GET` mit Frist. Die Freigabe ist an
  die Pipe gebunden; zeigt der Kontext später woanders hin, ist sie neu zu
  erteilen. Volumes gelten immer als geschützt. Die Docker-Bilanz steht getrennt
  vom freien Speicher des Laufwerks.
- **Reste von KI-Agenten (§4, §17, §18).** `.claude/worktrees`, `.playwright-mcp`,
  Aider-, Serena-, SpecStory-, Crush- und claude-flow-Ordner erscheinen mit
  eigener Summe, nie wiederherstellbar. Ohne Registrierung heißt ihre Herkunft
  „nur am Namen erkannt“.
- **Messen und Verlauf (§7, §12).** Logische Größe, belegter Platz, der freie
  Speicher vorher und nachher und je Projekt die letzten 30 Berichte. Daraus
  entsteht das Wachstum der letzten 90 Tage mit den am stärksten wachsenden
  Einträgen. Eine Datei ab 64 KiB, die über mehrere Hardlinks im Projekt
  erscheint, zählt beim belegten Platz einmal (an der Datei-ID); kleinere
  zählen weiter je Name, damit der Durchgang wenig Speicher braucht.
- Schon vorher vorhanden: Lesen erst nach Freigabe, keine Programmstarts,
  Wiederherstellungsbeleg, Fingerabdruck-Bestätigung, Revalidierung an gehaltenen
  Handles, dauerhaftes Journal, Abgleich nach Absturz, Papierkorb statt Löschen.

## Testszenarien der Spezifikation

| Szenario (§14, §22) | Verhalten | Nachweis |
|---|---|---|
| Bestätigter Werkzeugcache | Wiederherstellbar mit Spur | `new_test_outputs_and_caches_need_their_tool_trace` |
| `dist/` mit Handarbeit; `build/` neben `package.json` | Prüfen | `names_alone_do_not_make_hand_made_folders_recoverable` |
| Eingechecktes `dist/` oder `node_modules/` | Prüfen, nie planbar | `a_folder_with_versioned_files_is_never_planned_and_its_evidence_names_git` |
| `node_modules` ohne Lockfile | Prüfen | `classes_follow_names_and_the_files_beside_them` |
| Lokale SQLite-Datei | Prüfen (Datenbank) | `classes_follow_names_and_the_files_beside_them` |
| Alte Tests, dynamisch geladene Fixtures | Testcode wird nie ein Eintrag, Testdaten bleiben Prüfen | `test_code_never_becomes_an_item`, `test_data_are_summed_wherever_they_lie` |
| Junction auf dem Weg, ersetzter Ordner | Plan ungültig | `a_replaced_folder_or_a_junction_on_the_way_invalidates_the_plan` |
| Änderung zwischen Bestätigung und Ausführung | Plan ungültig | `a_missing_lockfile_after_approval_invalidates_the_plan`, `files_git_starts_to_track_after_approval_invalidate_the_plan` |
| Ordner nach dem Journal-Commit getauscht | Nicht verschoben | `a_folder_swapped_after_the_journal_commit_is_not_moved` |
| Gemeinsamer Cache mehrerer Projekte | Nur gemessen, nie im Plan | Aufräumbericht, globale Bestände |
| Papierkorb verweigert oder Ordner bleibt | Als nicht verschoben gemeldet | `a_failing_or_idle_recycle_bin_is_reported_and_leaves_the_folder` |
| Papierkorb statt Freigabe | Freier Speicher gemessen, kein Versprechen | `an_executed_plan_records_the_free_space_it_measured` |
| Worktree, Submodul, geteilter oder beschädigter Index | Git-Stand unbekannt, Prüfen | `an_unreadable_index_or_a_worktree_link_keeps_everything`, `partial_or_damaged_indexes_are_refused` |
| Paket eines Monorepos einzeln registriert (`.git` oberhalb) | Git-Stand unbekannt, nichts planbar | `a_project_inside_a_repository_it_cannot_read_moves_nothing` |
| Gemeinsamer Ordner zweier Projekte, zweite Schreibweise | Haltung gilt für den echten Ort | `a_keep_counts_for_every_project_that_reaches_the_folder` |
| Agent erzeugt Laufordner; Lauf läuft, pausiert oder ist fehlgeschlagen | Geschützt, gleich in welchem Zustand | `runs_are_refused_until_switched_on_and_only_add_protection` |
| Lauf-Manifest ohne eingeschaltete Registrierung | Abgelehnt | `runs_are_refused_until_switched_on_and_only_add_protection` |
| Beendeter Lauf soll wieder laufen | Abgelehnt | `finished_runs_stay_finished_and_paused_ones_go_on` |
| Container bindet einen Kandidaten ein, laufend oder gestoppt | Prüfen, nie im Plan | `folders_a_container_binds_in_stay_and_an_unreachable_engine_blocks` |
| Bind in anderer Schreibweise (Kurzname, Junction) oder über ein Volume | Gehalten | `a_bind_counts_under_the_spelling_docker_reports_and_through_volumes`, `a_bind_through_a_junction_spelling_holds_the_real_folder` |
| Engine antwortet zu viel oder in falscher Form | Unbekannt, blockiert, kein Absturz | `an_engine_that_answers_too_much_or_oddly_leaves_binds_unknown` |
| Agent schaltet die Registrierung selbst ein | Abgelehnt | `only_the_person_switches_runs_on` |
| Freigegebene Engine nicht erreichbar | Blocker: Einbindungen unbekannt | `folders_a_container_binds_in_stay_and_an_unreachable_engine_blocks` |
| Docker-Kontext zeigt auf einen entfernten Daemon | Nicht freigebbar, nie gefragt | `only_local_contexts_can_be_approved_and_a_moved_one_must_be_approved_again` |
| Kontext zeigt nach der Freigabe woanders hin | Nicht gefragt, neu freizugeben | `only_local_contexts_can_be_approved_and_a_moved_one_must_be_approved_again` |
| Inventar ändert etwas an Docker | Nur `GET` aus fester Liste | `the_inventory_asks_only_fixed_read_only_questions`, `test_docker_and_runs_stay_read_only` |
| Docker-Bilanz vs. Windows-Speicher | Getrennt angezeigt | Dialog „Docker“; `DockerUsage` |
| Docker-Desktop-VHDX | Nie geöffnet | `src/docker` liest keine Dateien außer `meta.json` |
| Wachsende Projekte | Verlauf und Wachstum | `reports_are_saved_and_show_what_grows`, `growth_compares_the_oldest_and_newest_complete_report_in_the_window` |
| Gefundener Text fordert zum Löschen auf | Bleibt Daten | `skill_references_inside_and_outside` |
| Zwei Hardlinks auf dieselben Daten | Belegter Platz einmal, ab 64 KiB | `hard_links_reserve_their_space_once` |
| Lauf beendet und vom Menschen freigegeben | Artefakte nicht mehr gehalten; laufender Lauf, zweite Freigabe und neue Artefakte abgelehnt | `a_finished_run_is_released_once_and_its_artifacts_are_no_longer_held` |
| Agent hebt Schutz auf (Lauf, Datenbank, behaltener Ordner, Docker-Kontext) | Über CLI und Pipe abgelehnt | `ending_a_monitoring_or_a_runs_protection_is_the_persons_decision` |
| Agent bestätigt oder führt einen Papierkorb-Plan aus, auch mit richtigem Fingerabdruck | Über CLI und Pipe abgelehnt, Plan bleibt Entwurf; im Dashboard bestätigt und verschoben | `pilot_registration_scan_and_a_recycle_plan` |
| SQLite-Datei in `target/` überwacht | Gehalten, mit WAL gemessen, Wachstum | `a_database_file_is_measured_with_its_companions_held_and_followed` |
| Postgres-Datenordner, Freigabe, fehlender Ort | Am Namen erkannt; Freigabe und fehlender Ort abgelehnt | `a_data_folder_is_told_by_its_names_and_measured_with_full_reports` |
| Datenbank-Volume in Docker | Vorgeschlagen, Größe aus `/system/df` | `suggestions_come_from_known_folders_project_files_and_volumes` |
| Echter Container bindet einen Projektordner ein | Gehalten; VM-Schreibweise auf den Windows-Ort abgebildet | `a_real_container_bind_holds_its_folder` (ignoriert, live gegen Docker Desktop 29.4.3) |
| Symbolischer Link auf eine Freigabe | Nicht verfolgt | `only_links_that_stay_on_local_disks_are_followed`; echter Link: `a_symbolic_link_to_a_share_is_not_followed` (braucht das Recht für symbolische Links); relatives Ziel unter einer Junction: `the_walk_reaches_the_place_windows_would_open` |
| Datenbank hinter einer Junction registrieren | Abgelehnt, für Junction, Ordner und Datei darunter | `a_database_behind_a_junction_is_refused` |

## Review

Ein getrennter Agent hat den Stand mit Git-Prüfung und Behalten geprüft und
vier blockierende Wege gefunden, alle mit Wegwerf-Tests nachgestellt und
behoben (Repository oberhalb der Freigabe, Behalten an der Projekt-ID, nur
innerstes Repository gefragt, Behalten als Textvergleich), dazu sechs nicht
blockierende. Ein zweiter Agentenreview der Läufe, des Docker-Inventars, des
Verlaufs und der Prüfsummenprüfung fand sieben blockierende Wege, ebenfalls
nachgestellt und behoben: Docker meldet Bind-Quellen in der Schreibweise des
Aufrufs (Kurzname, Junction), die Haltung galt aber nur am echten Ort; vor der
Freigabe registrierte Artefakte verloren danach ihren Schutz; die Container-
liste wurde stillschweigend gekürzt; eine Antwort falscher Form brachte den
Besitzerprozess zum Absturz; ein Kontext mit geändertem Endpunkt wurde weiter
gefragt; der Besitzer der Pipe wurde nicht geprüft; ein Agent konnte die
Registrierung selbst einschalten. Seitdem gilt eine Haltung unter der
gemeldeten Schreibweise und am echten Ort, und jedes Ziel wird am echten Ort
und in der registrierten Schreibweise verglichen; eine zu lange oder seltsame
Antwort ist „unbekannt“ und blockiert ([Validierung](VALIDIERUNG.md)). Ein
dritter Agentenreview von Freigabe, Datenbanken, Zwischenspeichern und
Linkprüfung fand zwei blockierende Wege (Artefakte nach der Freigabe wurden
angenommen, aber nicht gehalten; `cleanup unkeep` und `docker revoke` hoben
über die CLI einen Schutz auf) und zwei Linkfehler beim Registrieren und
Auflösen; alle behoben. Seitdem gilt: Schutz hinzufügen darf die CLI, Schutz
aufheben nur das Dashboard. Seit dem 31. Oktober 2026 bestätigt und führt auf
deine Entscheidung hin auch nur das Dashboard einen Papierkorb-Plan aus; über
CLI und Pipe ist das abgelehnt, auch mit richtigem Fingerabdruck. Die CLI
plant, bricht ab und gleicht ab. Der menschliche Review G5 ersetzt das nicht.

## Bewusst nicht umgesetzt

| Teil der Spezifikation | Grund |
|---|---|
| Aufbewahrungsfristen und Garbage Collection nach Laufende (§18, §21) | Ein Lauf darf nichts freigeben, nur schützen. Was ein Lauf meldet, bleibt geschützt, bis du den beendeten Lauf freigibst; auch danach geht Aufräumen nur über die Regeln des Aufräumberichts |
| Heartbeat, Lease, Prozessbeobachtung (§18) | mogumogu startet und überwacht keine Läufe; der Zustand ist, was der Harness meldet. Ohne Meldung ändert sich nichts |
| Entfernen von Docker-Objekten, `prune`, Stoppen (§20) | Nur lesend. Aufräumen in Docker geht über Docker selbst |
| Entfernte Docker-Kontexte | Nie gefragt |
| Lokales Sprachmodell (§5 Stufe 3) | Bräuchte einen lokalen HTTP-Endpunkt; mogumogu hat keine Netzwerkzugriffe, `validate.py` verbietet HTTP-Clients |
| Endgültiges Löschen (§7) | Nur der Papierkorb, ohne stillen Fallback |
| Linux-Adapter (§8) | Produkt ist Windows 10/11; `platform/fallback.rs` dient nur Kernprüfungen im Container |
| Importgraph, CI-Workflows, Knip (§5 Stufe 2) | Quellen und Tests werden nie Einträge; eine Aussage über ungenutzten Code ist kein Ziel |
| Prozess- und Handleprüfung (§6, Schritt 6) | Windows verweigert das Verschieben gesperrter Dateien; das Journal meldet den Fehler je Ordner |

## Offen

1. G5: Der unabhängige menschliche Review des Papierkorb-Wegs, jetzt mit
   Git-Prüfung, Haltungen (auch überwachten Datenbanken) und der Docker-Abfrage
   vor jedem Verschieben.
2. Hardlinks unter 64 KiB zählen beim belegten Platz weiter je Name, und ein
   Hardlink, der zwei Projekte verbindet, zählt in beiden.
3. Ein echter symbolischer Link auf eine Freigabe ist auf diesem Rechner nicht
   nachgestellt (kein Recht für symbolische Links); die Regel ist mit einer
   Attrappe geprüft.
4. Ein Git-Index über 32 MB wird je Ziel neu gelesen.
