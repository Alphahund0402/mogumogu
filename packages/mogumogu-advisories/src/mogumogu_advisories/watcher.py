"""mogumogu inventory watcher integrating with `mogumogu-cli` and offline OSV data."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys
import time
from urllib.request import urlopen

from .feeds import (
    DAY,
    DOWNLOAD_SECONDS,
    MAX_AGE,
    MAX_ARCHIVE,
    MAX_ARCHIVE_EXPANDED,
    check_archive,
    db_path,
    download_feed as _download_feed,
    feed_status,
    refresh_feeds,
)
from .models import (
    ADVISORY_ID,
    CRAN_VERSION,
    CVE_ID,
    ECOSYSTEMS,
    MAVEN_COORDINATE,
    MODULE_PATH,
    NUGET_VERSION,
    PACKAGIST_NAME,
    PACKAGIST_VERSION,
    PEP440,
    PROJECT_ECOSYSTEMS,
    RUBYGEMS_VERSION,
    SEMVER,
    SEMVER_NUMBER,
    SEMVER_PRE,
    Advisory,
    Evidence,
    Package,
    WatchError,
)
from .report_html import write_html
from .scanner import MAX_RESULTS, executable, match_packages, read_matches

MAX_PAGES = 40


def download_feed(path: Path, ecosystem: str) -> None:
    _download_feed(path, ecosystem, opener=urlopen)


def refresh(cache: Path, only_due: bool, ecosystems=ECOSYSTEMS) -> dict[str, str]:
    return refresh_feeds(cache, only_due=only_due, ecosystems=ecosystems, opener=urlopen)


def match(scanner: Path, cache: Path, packages: set[Package]) -> dict[Package, set[Advisory]]:
    return match_packages(scanner, cache, packages, runner=subprocess.run)


def cli_call(cli, data_dir, args):
    command = [str(cli), "--no-start", "--json"]
    if data_dir:
        command += ["--data-dir", str(data_dir)]
    result = subprocess.run(command + args, capture_output=True, timeout=125, check=False)
    page = json.loads(result.stdout)
    if result.returncode or not page["ok"] or page["v"] != 1:
        raise WatchError("Inventar nicht verfügbar oder geändert. mogumogu und CLI müssen denselben aktuellen Stand haben.")
    return page["result"]


def scan_projects(cli, data_dir, selection):
    first = cli_call(cli, data_dir, ["advisories", *selection])
    failed = []
    for project in first["projects"]:
        if not project["scan_allowed"]:
            continue
        try:
            scans = cli_call(cli, data_dir, ["scan", "--scope", str(project["scope_id"])])
        except (WatchError, OSError, ValueError, subprocess.SubprocessError):
            failed.append(project["id"])
            continue
        if any(scan.get("cancelled") for scan in scans):
            return failed, True
    return failed, False


def inventory(cli, data_dir, selection=None):
    query = ["advisories", *(selection or ["inventory"])]
    packages = {}
    coverage = {}
    skipped_by_scope = Counter()
    offset = 0
    for _ in range(MAX_PAGES):
        page_args = ["--offset", str(offset), "--basis", basis] if offset else []
        page = cli_call(cli, data_dir, query + page_args)
        if not offset:
            basis = page["basis"]
            coverage = {key: page[key] for key in (
                "current_scopes", "incomplete_scopes", "oldest_scan_at", "scope_limit_reached",
            )}
            coverage["skipped"] = 0
            if query[1] == "projects":
                coverage["projects"] = page["projects"]
        if page["basis"] != basis:
            raise WatchError("Inventar hat sich während des Abrufs geändert.")
        coverage["skipped"] += page["skipped"]
        skipped_by_scope.update({int(scope): count for scope, count in page.get("skipped_by_scope", {}).items()})
        for row in page["packages"]:
            package = Package.parse(row)
            scope_id = row["scope_id"]
            if package is None:
                coverage["skipped"] += 1
                skipped_by_scope[scope_id] += 1
                continue
            evidence = Evidence(scope_id, row.get("state", "installed"), row["public_source"])
            packages.setdefault(package, set()).add(evidence)
        next_offset = page["next_offset"]
        if next_offset is None:
            coverage["item_limit_reached"] = page["item_limit_reached"]
            if "projects" in coverage:
                coverage["skipped_by_scope"] = skipped_by_scope
            return packages, coverage
        if type(next_offset) is not int or next_offset <= offset:
            raise WatchError("Ungültige Inventarseite.")
        offset = next_offset
    raise WatchError("Seitenlimit für das Installationsinventar erreicht.")


def manual_tools(path):
    if path is None:
        return set()
    with path.open("rb") as source:
        text = source.read((128 << 10) + 1)
    if len(text) > 128 << 10:
        raise WatchError("Werkzeugliste zu groß.")
    rows = json.loads(text)
    if not isinstance(rows, list) or len(rows) > 500:
        raise WatchError("Die Werkzeugliste muss eine Liste mit höchstens 500 Einträgen sein.")
    tools = set()
    for index, row in enumerate(rows, 1):
        if not isinstance(row, dict) or row.keys() != {"ecosystem", "name", "version"}:
            raise WatchError(f"Werkzeugliste, Zeile {index}: Ökosystem, Name und genaue Version erforderlich.")
        package = Package.parse(row)
        if package is None:
            raise WatchError(f"Werkzeugliste, Eintrag {index}: Paketidentität oder Version nicht unterstützt.")
        tools.add(package)
    return tools


def project_coverage(projects, skipped, packages, checked, feeds, scan_failed, now):
    rows = [
        {**project, "skipped": skipped.get(project["scope_id"], 0), "candidates": 0, "checked": 0, "advisory_gaps": 0}
        for project in projects
    ]
    by_scope = {row["scope_id"]: row for row in rows if row["scope_id"] is not None}
    for package, evidence in packages.items():
        for scope_id in {item.scope_id for item in evidence}:
            if scope_id in by_scope:
                row = by_scope[scope_id]
                row["candidates"] += 1
                row["checked"] += package in checked
                row["advisory_gaps"] += package not in checked or feeds[package.ecosystem] != "aktuell"
    for row in rows:
        finished = row["scan_finished_at"]
        row["stale"] = finished is not None and now - finished > MAX_AGE
        row["scan_failed"] = row["id"] in scan_failed
        row["partial"] = bool(
            row["scan_state"] != "complete" or row["stale"] or row["scan_failed"]
            or not row["candidates"] or row["skipped"] or row["advisory_gaps"]
        )
    return rows


def check(args):
    using_projects = args.projects or args.project is not None
    selection = ["projects"] if using_projects else ["inventory"]
    if args.project is not None:
        selection += ["--project", str(args.project)]
    ecosystems = PROJECT_ECOSYSTEMS if using_projects else ECOSYSTEMS
    refresh_failed = refresh(args.cache, only_due=True, ecosystems=ecosystems) if args.refresh else {}
    scan_failed, scan_cancelled = scan_projects(args.cli, args.data_dir, selection) if args.scan else ([], False)
    packages, coverage = inventory(args.cli, args.data_dir, selection)
    tools = manual_tools(args.tools)
    candidates = set(packages) | tools
    now = time.time()
    feeds = {
        ecosystem: feed_status(db_path(args.cache, ecosystem), now)
        for ecosystem in sorted({p.ecosystem for p in candidates})
    }
    ready = {p for p in candidates if feeds[p.ecosystem] in ("aktuell", "veraltet")}
    checked = match(args.scanner, args.cache, ready)
    findings = []
    for package in sorted(checked):
        evidence = sorted(packages.get(package, ()))
        public = {item.scope_id for item in evidence if item.public_source}
        possible = {item.scope_id for item in evidence if not item.public_source}
        for advisory in sorted(checked[package]):
            findings.append({
                **package.fields(), "advisory_ids": advisory.ids, "cve_ids": advisory.cves,
                "url": f"https://osv.dev/vulnerability/{advisory.ids[0]}",
                "public_scope_ids": sorted(public), "possible_scope_ids": sorted(possible),
                "evidence": [
                    {"scope_id": item.scope_id, "state": item.state, "public_source": item.public_source}
                    for item in evidence
                ],
                "user_reported_tool": package in tools,
            })
    projects = coverage.pop("projects", None)
    project_rows = project_coverage(
        projects, coverage.pop("skipped_by_scope"), packages, checked, feeds, scan_failed, now,
    ) if projects is not None else []
    old_scan = coverage["oldest_scan_at"]
    inventory_stale = old_scan is not None and now - old_scan > MAX_AGE
    partial = bool(
        not candidates or candidates != checked.keys() or refresh_failed or inventory_stale
        or coverage["incomplete_scopes"] or coverage["skipped"] or coverage["scope_limit_reached"]
        or coverage["item_limit_reached"] or any(status != "aktuell" for status in feeds.values())
        or scan_cancelled or any(project["partial"] for project in project_rows)
    )
    result = {
        "inventory": {**coverage, "stale": inventory_stale}, "feeds": feeds, "refresh_failed": refresh_failed,
        "candidates": len(candidates), "checked": len(checked), "partial": partial, "findings": findings,
        "possible_packages": sum(any(not item.public_source for item in items) for items in packages.values()),
        "manual_tools": len(tools),
    }
    if projects is not None:
        result.update(projects=project_rows, scan_cancelled=scan_cancelled)
    return result


def report(args):
    try:
        return check(args)
    except WatchError as error:
        return {"partial": True, "error": str(error)}
    except (OSError, KeyError, TypeError, ValueError, subprocess.SubprocessError):
        return {"partial": True, "error": "Inventar, Werkzeugliste oder Scanner-Ausgabe konnte nicht verarbeitet werden."}


def main(argv=None):
    parser = argparse.ArgumentParser(description="Check mogumogu project dependencies and installations against local OSV data.")
    parser.add_argument("mode", choices=("refresh", "check", "watch"))
    parser.add_argument("--cache", type=Path, required=True, help="Verzeichnis der öffentlichen OSV-Datenbanken")
    parser.add_argument("--cli", type=executable, help="Pfad zur aktuellen mogumogu-cli.exe")
    parser.add_argument("--scanner", type=executable, help="Pfad zum verifizierten OSV-Scanner v2")
    parser.add_argument("--data-dir", type=Path, help="abweichendes Datenverzeichnis des Besitzers")
    parser.add_argument("--tools", type=Path, help="optionale private JSON-Liste exakt bekannter Werkzeugversionen")
    parser.add_argument("--interval-minutes", type=int, default=360, help="Abstand im Watch-Modus (Standard 360)")
    parser.add_argument("--refresh", action="store_true", help="öffentliche OSV-Datenbanken nach einem Tag aktualisieren")
    target = parser.add_mutually_exclusive_group()
    target.add_argument("--projects", action="store_true", help="alle registrierten Projekte mit Abdeckung prüfen")
    target.add_argument("--project", type=int, metavar="ID", help="ein registriertes Projekt prüfen")
    parser.add_argument("--scan", action="store_true", help="bei check zuerst die freigegebenen Projekte erfassen")
    parser.add_argument("--html", type=Path, metavar="DATEI", help="lokalen HTML-Bericht schreiben und bei Änderungen ersetzen")
    args = parser.parse_args(argv)
    if args.project is not None and args.project < 1:
        parser.error("--project erwartet eine positive Projekt-ID.")
    if args.scan and (args.mode != "check" or not (args.projects or args.project)):
        parser.error("--scan ist für check mit --projects oder --project ID verfügbar.")
    if args.html and (args.mode == "refresh" or args.html.suffix.lower() != ".html"):
        parser.error("--html erwartet eine .html-Datei bei check oder watch.")
    args.cache = args.cache.resolve()
    if args.mode == "refresh":
        ecosystems = PROJECT_ECOSYSTEMS if args.projects or args.project else ECOSYSTEMS
        failed = refresh(args.cache, only_due=False, ecosystems=ecosystems)
        print(json.dumps({"refresh_failed": failed}))
        return 2 if failed else 0
    if not args.cli or not args.scanner or args.interval_minutes < 1:
        parser.error("check/watch benötigen --cli und --scanner sowie ein Intervall von mindestens einer Minute.")
    previous = None
    while True:
        current = report(args)
        if current != previous:
            ts = datetime.now(timezone.utc).isoformat(timespec="seconds")
            output = {"checked_at": ts, **current}
            if args.html:
                try:
                    write_html(args.html, output)
                except OSError:
                    output.update(partial=True, report_error="HTML-Bericht konnte nicht geschrieben werden.")
            print(json.dumps(output), flush=True)
            previous = None if "report_error" in output else current
        if args.mode == "check":
            return 2 if output["partial"] else int(bool(output["findings"]))
        time.sleep(args.interval_minutes * 60)


__all__ = [
    "ADVISORY_ID",
    "CRAN_VERSION",
    "CVE_ID",
    "DAY",
    "DOWNLOAD_SECONDS",
    "ECOSYSTEMS",
    "MAVEN_COORDINATE",
    "MAX_AGE",
    "MAX_ARCHIVE",
    "MAX_ARCHIVE_EXPANDED",
    "MAX_PAGES",
    "MAX_RESULTS",
    "MODULE_PATH",
    "NUGET_VERSION",
    "PACKAGIST_NAME",
    "PACKAGIST_VERSION",
    "PEP440",
    "PROJECT_ECOSYSTEMS",
    "RUBYGEMS_VERSION",
    "SEMVER",
    "SEMVER_NUMBER",
    "SEMVER_PRE",
    "Advisory",
    "Evidence",
    "Package",
    "WatchError",
    "check",
    "check_archive",
    "cli_call",
    "db_path",
    "download_feed",
    "executable",
    "feed_status",
    "inventory",
    "main",
    "manual_tools",
    "match",
    "project_coverage",
    "read_matches",
    "refresh",
    "report",
    "scan_projects",
    "subprocess",
    "sys",
    "time",
    "urlopen",
    "write_html",
]
