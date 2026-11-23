# Projekt abschließen: Spezifikation

Stand: 23. November 2026. **Entwurf, nichts davon ist umgesetzt.** Die
Entscheidungen in [Abschnitt 8](#8-offene-entscheidungen) stehen aus.

Ist ein Projekt fertig, zeigt mogumogu in einem Abschlussbericht, was bleiben
muss, was in den Papierkorb kann und worüber du entscheiden musst, besonders
bei wachsenden Testdaten. Abschließen ist eine eigene Entscheidung, getrennt vom
Aufräumen erzeugter Dateien ([Implementierungsplan, Abschnitt 10, Schritt 3](IMPLEMENTIERUNGSPLAN.md#10-nächste-schritte)).
Archivieren, Verschieben und Löschen des Projekts bleiben außerhalb des Umfangs.

## 1. Grundsätze

- **Abschließen ändert keinen Schutz.** Das Projekt bleibt geschützt, keine
  Haltung endet, und nichts wird wiederherstellbar, was es vorher nicht war.
  Neue Regeln dürfen nur enger machen ([AGENTS.md](../AGENTS.md)).
- **Keine neue Lesegrenze.** Der Bericht verbindet, was es schon gibt: einen
  frischen Aufräumbericht, die offenen Punkte, den Berichtsverlauf, Haltungen,
  Läufe, überwachte Datenbanken und Docker-Einbindungen.
- **Entfernt wird wie bisher** nur über einen Papierkorb-Plan aus dem
  Aufräumbericht, bestätigt im Dashboard, bis G5 nur in Wegwerf-Testwurzeln
  ([Projektbewusste Bereinigung](PROJEKTBEWUSSTE-BEREINIGUNG.md)).
- **„Abgeschlossen“ heißt nie „kann weg“.** Unbekannt bleibt unbekannt, und
  „erledigt“ in den offenen Punkten bleibt kein Löschurteil.

## 2. Zustände

| Zustand | Bedeutung |
|---|---|
| Aktiv | Standard, auch für alle Projekte nach dem Upgrade |
| Abgeschlossen | Du erklärst: fertig, das Ergebnis bleibt |

**Reaktivieren** setzt ein abgeschlossenes Projekt zurück auf aktiv und ändert
sonst nichts. Jeder Wechsel ist ein datiertes Ereignis mit optionaler Notiz und
dem Fingerabdruck des Berichts, auf dem er beruht. „Stilllegen“ als dritter
Zustand ist Entscheidung E1.

## 3. Abschlussbericht

„Projekt abschließen …“ auf der Projektseite erfasst das Projekt frisch und zeigt
vier Teile. Bestätigt wird per Fingerabdruck wie bei einem Papierkorb-Plan;
ändert sich der Stand vor der Bestätigung, ist ein neuer Bericht nötig.

**A. Ist die Arbeit gesichert?** Die Punkte der Gruppe „Gesichert“: Commits,
Remote, Commits auf dem Remote, zurückgelegte und vorgemerkte Änderungen,
geänderte Dateien, Dateien nur auf diesem Rechner, weitere Arbeitskopien. Offene
und unbekannte Punkte stehen zuerst. Jeder muss ausdrücklich bestätigt werden,
etwa „3 Commits liegen nur hier“. Unbekannt zählt nie als erledigt.

**B. Bleibt ohne Frage.** Versionierte Dateien und der übrige eigene Inhalt,
Projektdateien, alles mit einer Haltung (behalten, Lauf, überwachte Datenbank,
Docker-Einbindung) und geschützte Ressourcen. Gezeigt mit Größe, damit sichtbar
wird, was das Projekt dauerhaft belegt.

**C. Du entscheidest.** Alles der Klasse „Prüfen“: Testdaten, Datenbanken, Logs,
große Dateien, Umgebungen oder Ausgaben ohne Datei, die sie neu baut, und Reste
von KI-Agenten. Je Ordner Größe, letzte Änderung, Wachstum der letzten 90 Tage
aus dem Berichtsverlauf und der Beleg. Wahl je Ordner: „Behalten“ (wie heute,
als behaltener Ordner) oder „offen lassen“. Ob ein Ordner aus diesem Teil in den
Papierkorb darf, ist Entscheidung E3.

**D. Kann in den Papierkorb.** Alles der Klasse „Wiederherstellbar“ mit Beleg
und Platz, vorausgewählt. Führt in den gewohnten Papierkorb-Plan mit Journal,
erneuter Prüfung und G5-Sperre. Daneben steht, wie viel Platz frei würde und
wie viel bleibt.

**Außerhalb des Projekts**, nur zur Kenntnis: Läufe dieses Projekts, die noch
nicht freigegeben sind; Container, die Projektordner einbinden. Später auch
globale Caches und Werkzeuge, die nur dieses Projekt nutzte, sobald es
„Genutzt von“ gibt (Plan, Abschnitt 10, Schritt 1).

## 4. Nach dem Abschluss

- Die Projekttabelle filtert nach „Abgeschlossen“. Die Übersicht zählt
  abgeschlossene Projekte getrennt und zeigt für sie keinen Hinweis „ruhend“,
  weil Ruhe hier erwartet ist.
- **Seit dem Abschluss geändert:** Zeigt ein späterer Bericht neuere
  Änderungszeiten, einen anderen `HEAD` oder Wachstum, bietet die Projektseite
  „Reaktivieren“ an. Automatisch ändert sich nichts.
- Abgeschlossene Projekte bleiben in „Alles erfassen“, sonst gäbe es den
  Hinweis nicht.

## 5. Scratchpads

Heute hält ein geschütztes Scratchpad jeden Plan für Ordner darin auf, auch
`node_modules` neben einem Lockfile ([gate.rs](../src/cleanup/gate.rs),
`unprotected`). Der Plan fragt, wie ein geschütztes Scratchpad Pläne darin
zulassen kann. Optionen, Entscheidung E4:

- **A. Bleibt so.** Am sichersten und einfachsten; Erzeugtes in Scratchpads
  räumst du selbst auf.
- **B. Schalter je Scratchpad: „Erzeugtes darin darf in Pläne“.** Nur
  „Wiederherstellbar“ mit Beleg, nur im Dashboard einschaltbar; das Scratchpad
  selbst und alles unter „Prüfen“ bleiben geschützt. Lockert eine Regel und
  braucht den zweiten Review.
- **C. Ein abgeschlossenes Scratchpad** (eigener Zustand wie in Abschnitt 2)
  lässt Pläne für Wiederherstellbares darin zu. Lockert ebenfalls und koppelt
  zwei Entscheidungen.

## 6. Daten und Schnittstellen

- **Schema v11:** Tabelle `project_lifecycle` (`project_id`, `state` in
  `active`/`finished`, `at`, `note` bis 500 Zeichen, `report_fingerprint`),
  höchstens 50 Einträge je Projekt. Der neueste Eintrag ist der Zustand; ohne
  Eintrag ist ein Projekt aktiv. Sicherung vor der Migration wie gewohnt, kein
  Projekt wechselt dabei den Zustand. Die Demo-Datenbank bekommt eigene
  Beispieldaten.
- **Anwendungsfälle** in `service/`: `FinishReport { project_id }` (nur lesend),
  `FinishProject { project_id, confirmation }` (Fingerabdruck),
  `ReactivateProject { project_id }`.
- **CLI:** `project finish-report --project ID` zeigt den Bericht. Ob die CLI
  abschließen darf, ist Entscheidung E2.

## 7. Abnahme

| Szenario | Muss gelten |
|---|---|
| Projekt abschließen | `protected`, Haltungen, behaltene Ordner und die Menge des Wiederherstellbaren sind vorher und nachher gleich |
| Ordner unter „Prüfen“ | Kommt ohne E3 nie in einen Plan, auch nicht nach dem Abschluss |
| Offener oder unbekannter Punkt unter „Gesichert“ | Abschließen nur nach ausdrücklicher Bestätigung genau dieses Punkts |
| Stand ändert sich zwischen Bericht und Bestätigung | Abgelehnt, neuer Bericht nötig |
| Änderung nach dem Abschluss | Hinweis „Seit dem Abschluss geändert“, kein automatischer Wechsel |
| Reaktivieren | Nur der Zustand ändert sich |
| Upgrade v10 → v11 | Sicherung, alle Projekte aktiv, Demo und lokal getrennt |
| Abschließen über die Pipe | Nach E2 abgelehnt oder erlaubt, in beiden Fällen getestet |

## 8. Offene Entscheidungen

| Nr. | Frage | Empfehlung |
|---|---|---|
| E1 | Eigener Zustand „stillgelegt“ (pausiert, Rückkehr erwartet) neben „abgeschlossen“? | Nein, zuerst nur „abgeschlossen“ und Reaktivieren. Ein zweiter Zustand lohnt erst, wenn er etwas anderes bewirkt. |
| E2 | Darf die CLI (und damit ein Agent) abschließen? | Nein, nur das Dashboard, wie das Freigeben eines Laufs. Bericht und Reaktivieren auch per CLI, denn beides lenkt nur Aufmerksamkeit. |
| E3 | Dürfen Ordner unter „Prüfen“ beim Abschluss nach Einzelbestätigung in den Papierkorb? | Nein. Das lockert die Grundregel, dass nur ein Wiederherstellungsbeleg „wiederherstellbar“ trägt. Der Bericht zeigt Pfad, Größe und Wachstum; gelöscht wird außerhalb von mogumogu. |
| E4 | Scratchpads: A, B oder C aus Abschnitt 5? | B, mit zweitem Review. A ist die sichere Rückfallebene. |
| E5 | Blockiert ein offener Punkt unter „Gesichert“ das Abschließen? | Nein, er muss aber ausdrücklich bestätigt werden. |

## 9. Reihenfolge

1. **Abschlussbericht, nur lesend:** CLI und Dialog auf der Projektseite, ohne
   Schemaänderung. Liefert schon den Hauptnutzen.
2. **Zustand und Verlauf** (Schema v11), Filter und der Hinweis nach dem
   Abschluss.
3. **Scratchpads** nach E4, mit zweitem Review.
