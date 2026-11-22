"""High-level offline auditing functions for libraries and CI/CD pipelines.

Exit code contract:
  0: complete offline audit, no untriaged or expired advisories
  1: one or more untriaged or expired advisories found
  2: incomplete audit (no packages, missing/corrupt/stale feed, parse/scanner error)
"""
from datetime import date, datetime, timezone
from pathlib import Path
import time

from .feeds import feed_info
from .lockfiles import ScanResult, parse_lockfile, parse_package_list, scan_lockfiles
from .models import Advisory, Package, TriageEntry
from .scanner import match_packages
from .triage import evaluate_findings, load_triage


def audit_packages(
    packages: set[Package],
    cache: Path,
    scanner: Path,
    triage: list[TriageEntry] | None = None,
    *,
    locations: dict[Package, set[str]] | None = None,
    lockfiles: list[str] | None = None,
    skipped: int = 0,
    warnings: list[str] | None = None,
    max_age_days: int = 7,
    allow_stale: bool = False,
    today: date | None = None,
    now: float | None = None,
    matcher=None,
) -> tuple[int, dict]:
    """Evaluates a set of exact packages against local OSV archives and triage rules."""
    now = time.time() if now is None else now
    today = datetime.fromtimestamp(now, timezone.utc).date() if today is None else today
    checked_at = datetime.fromtimestamp(now, timezone.utc).isoformat(timespec="seconds")
    triage_entries = triage or []
    warn_list = list(warnings or ())
    lockfile_list = list(lockfiles or ())

    ecosystems = sorted({pkg.ecosystem for pkg in packages})
    feeds = {eco: feed_info(cache, eco, now=now, max_age_days=max_age_days) for eco in ecosystems}

    if not packages:
        return 2, {
            "checked_at": checked_at,
            "today": today.isoformat(),
            "complete": False,
            "error": "Keine prüfbaren Paketversionen gefunden.",
            "feeds": feeds,
            "lockfiles": lockfile_list,
            "packages_total": 0,
            "packages_checked": 0,
            "skipped": skipped,
            "warnings": warn_list,
            "untriaged": [],
            "expired": [],
            "triaged": [],
            "unused_triage": [],
        }

    ready = {pkg for pkg in packages if feeds[pkg.ecosystem]["status"] in ("aktuell", "veraltet")}
    match_fn = match_packages if matcher is None else matcher
    matches: dict[Package, set[Advisory]] = match_fn(scanner, cache, ready) if ready else {}
    evaluated = evaluate_findings(matches, triage_entries, today, locations=locations)

    missing_feeds = [eco for eco, info in feeds.items() if info["status"] not in ("aktuell", "veraltet")]
    stale_feeds = [eco for eco, info in feeds.items() if info["status"] == "veraltet"]
    stale_blocked = bool(stale_feeds) and not allow_stale
    complete = (
        len(matches) == len(packages)
        and not missing_feeds
        and not stale_blocked
        and not warn_list
    )

    report = {
        "checked_at": checked_at,
        "today": today.isoformat(),
        "complete": complete,
        "feeds": feeds,
        "lockfiles": lockfile_list,
        "packages_total": len(packages),
        "packages_checked": len(matches),
        "skipped": skipped,
        "warnings": warn_list,
        **evaluated,
    }
    # Convenience single-feed view when auditing crates.io only (Cargo.lock compat).
    if ecosystems == ["crates.io"]:
        report["feed"] = feeds["crates.io"]
        report["crates_total"] = len(packages)
        report["crates_checked"] = len(matches)
        report["non_registry_skipped"] = skipped

    if not complete:
        code = 2
    elif evaluated["untriaged"] or evaluated["expired"]:
        code = 1
    else:
        code = 0
    return code, report


def audit_project(
    target: Path,
    cache: Path,
    scanner: Path,
    *,
    triage_path: Path | None = None,
    explicit_lockfiles: list[Path] | None = None,
    packages_file: Path | None = None,
    ecosystems_filter: set[str] | None = None,
    max_age_days: int = 7,
    allow_stale: bool = False,
    today: date | None = None,
    now: float | None = None,
    matcher=None,
) -> tuple[int, dict]:
    """Scans a project directory, lockfile(s), or JSON package list offline."""
    if packages_file is not None:
        scanned = parse_package_list(packages_file)
    else:
        scanned = scan_lockfiles(target, explicit_files=explicit_lockfiles)

    if ecosystems_filter:
        kept_packages = {pkg for pkg in scanned.packages if pkg.ecosystem in ecosystems_filter}
        kept_locations = {pkg: locs for pkg, locs in scanned.locations.items() if pkg in kept_packages}
        filtered_out = len(scanned.packages) - len(kept_packages)
        scanned = ScanResult(
            packages=kept_packages,
            locations=kept_locations,
            lockfiles=scanned.lockfiles,
            skipped=scanned.skipped + filtered_out,
            warnings=scanned.warnings,
        )

    triage = load_triage(triage_path)
    return audit_packages(
        scanned.packages,
        cache,
        scanner,
        triage,
        locations=scanned.locations,
        lockfiles=scanned.lockfiles,
        skipped=scanned.skipped,
        warnings=scanned.warnings,
        max_age_days=max_age_days,
        allow_stale=allow_stale,
        today=today,
        now=now,
        matcher=matcher,
    )


def audit_cargo_lock(
    lockfile: Path,
    cache: Path,
    scanner: Path,
    triage_path: Path | None = None,
    *,
    max_age_days: int = 7,
    allow_stale: bool = False,
    today: date | None = None,
    now: float | None = None,
    matcher=None,
) -> tuple[int, dict]:
    """Audits a single Cargo.lock file against local crates.io OSV data."""
    scanned = parse_lockfile(lockfile)
    triage = load_triage(triage_path)
    return audit_packages(
        scanned.packages,
        cache,
        scanner,
        triage,
        locations=scanned.locations,
        lockfiles=scanned.lockfiles,
        skipped=scanned.skipped,
        warnings=scanned.warnings,
        max_age_days=max_age_days,
        allow_stale=allow_stale,
        today=today,
        now=now,
        matcher=matcher,
    )


def print_audit_summary(report: dict, *, title: str = "Sicherheits-Audit (OSV / CVE, offline)") -> None:
    print(f"=== {title} ===")
    print(f"Zeitpunkt:     {report['checked_at']}")
    feeds = report.get("feeds") or ({report["feed"]["ecosystem"]: report["feed"]} if "feed" in report else {})
    if feeds:
        parts = [
            f"{eco}: {info['status']} ({info['age_days']}d/{info['max_age_days']}d)"
            if info.get("age_days") is not None
            else f"{eco}: {info['status']}"
            for eco, info in sorted(feeds.items())
        ]
        print(f"OSV-Feeds:     {' · '.join(parts)}")
    if report.get("lockfiles"):
        print(f"Lockfiles:     {', '.join(report['lockfiles'])}")
    if report.get("triage_file"):
        print(f"Triage:        {report['triage_file']}")
    if report.get("error"):
        print(f"FEHLER:        {report['error']}")
        return
    total = report.get("packages_total", report.get("crates_total", 0))
    checked = report.get("packages_checked", report.get("crates_checked", 0))
    skipped = report.get("skipped", report.get("non_registry_skipped", 0))
    print(f"Geprüft:       {checked}/{total} Pakete ({skipped} nicht-öffentlich/übersprungen)")
    print(
        f"Ergebnis:      {len(report['untriaged'])} ungetriagt · {len(report['expired'])} abgelaufen"
        f" · {len(report['triaged'])} befristet getriagt · {len(report['unused_triage'])} erledigte Triage-Einträge"
    )
    for section_title, key in (
        ("UNGETRIAGTE ADVISORIES (blockiert)", "untriaged"),
        ("ABGELAUFENE TRIAGE-EINTRÄGE (blockiert)", "expired"),
        ("Befristet getriagte Advisories", "triaged"),
    ):
        rows = report[key]
        if not rows:
            continue
        print(f"\n{section_title}:")
        for row in rows:
            ids = ", ".join(row["advisory_ids"])
            cves = f" · CVEs: {', '.join(row['cve_ids'])}" if row["cve_ids"] else ""
            locs = f" · {', '.join(row['locations'])}" if row.get("locations") else ""
            meta = ""
            if "scope" in row:
                meta = f" · [{row['scope']} · {row['kind']} · bis {row['expires']}]"
            eco = row.get("ecosystem", "crates.io")
            pkg = row.get("package", row.get("crate", "?"))
            print(f"  - [{eco}] {pkg} {row['version']}: {ids}{cves}{locs}{meta}")
    if report.get("unused_triage"):
        print("\nHinweis: Nicht mehr zutreffende Triage-Einträge (können entfernt werden):")
        for row in report["unused_triage"]:
            eco = row.get("ecosystem", "crates.io")
            pkg = row.get("package", row.get("crate", "?"))
            print(f"  - {row['id']} ([{eco}] {pkg} {row['version']}, Frist {row['expires']})")
    if report.get("warnings"):
        print("\nHinweise / Lesewarnungen:")
        for warning in report["warnings"]:
            print(f"  - {warning}")
    if not report["complete"]:
        print("\nWARNUNG: Audit ist unvollständig (z. B. fehlender oder veralteter OSV-Feed oder Lesefehler).")
