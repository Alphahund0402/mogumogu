#!/usr/bin/env python3
"""Compatibility wrapper for auditing Cargo.lock backed by `mogumogu-advisories`."""
import argparse
from datetime import date
import json
from pathlib import Path
import sys
import time
import tomllib

_LIB_SRC = str(Path(__file__).resolve().parents[1] / "packages" / "mogumogu-advisories" / "src")
if _LIB_SRC not in sys.path:
    sys.path.insert(0, _LIB_SRC)

import watch_advisories as watcher  # noqa: E402
from mogumogu_advisories._io import atomic_write_text  # noqa: E402
from mogumogu_advisories.audit import audit_packages, print_audit_summary  # noqa: E402
from mogumogu_advisories.feeds import default_cache_dir, feed_info as _feed_info  # noqa: E402
from mogumogu_advisories.lockfiles import CRATES_IO_SOURCE, parse_cargo_lock_detailed  # noqa: E402
from mogumogu_advisories.models import TriageEntry, normalize_package_name  # noqa: E402
from mogumogu_advisories.report_html import write_audit_html  # noqa: E402
from mogumogu_advisories.report_sarif import write_sarif  # noqa: E402
from mogumogu_advisories.triage import evaluate_findings, load_triage  # noqa: E402


def normalize_crate(name: str) -> str:
    return normalize_package_name("crates.io", name)


def parse_cargo_lock(path: Path) -> tuple[set[watcher.Package], int, int]:
    return parse_cargo_lock_detailed(path)


def feed_info(cache: Path, now: float, max_age_days: int) -> dict:
    return _feed_info(cache, "crates.io", now=now, max_age_days=max_age_days)


def run_audit(
    lockfile: Path,
    cache: Path,
    scanner: Path,
    triage_path: Path | None,
    *,
    max_age_days: int = 7,
    allow_stale: bool = False,
    today: date | None = None,
    now: float | None = None,
) -> tuple[int, dict]:
    packages, non_registry, invalid = parse_cargo_lock_detailed(lockfile)
    triage = load_triage(triage_path)
    warnings = [f"{invalid} ungültige Paketzeilen in {lockfile.name}"] if invalid else []
    code, report = audit_packages(
        packages,
        cache,
        scanner,
        triage,
        locations={pkg: {lockfile.name} for pkg in packages},
        lockfiles=[lockfile.name],
        skipped=non_registry + invalid,
        warnings=warnings,
        max_age_days=max_age_days,
        allow_stale=allow_stale,
        today=today,
        now=now,
        matcher=watcher.match,
    )
    report["non_registry_skipped"] = non_registry
    report["invalid_skipped"] = invalid
    return code, report


def print_summary(report: dict) -> None:
    print_audit_summary(report, title="Cargo.lock Sicherheits-Audit (OSV / RustSec, offline)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lockfile", type=Path, default=Path("Cargo.lock"), help="Pfad zur Cargo.lock")
    parser.add_argument("--cache", type=Path, help="OSV-Datenbank-Cache (Standard: Benutzer-Cache)")
    parser.add_argument("--scanner", type=watcher.executable, help="Pfad zum verifizierten OSV-Scanner v2")
    parser.add_argument("--triage", type=Path, help="TOML-Datei mit befristeten Triage-Einträgen")
    parser.add_argument("--refresh", action="store_true", help="crates.io-Feed vor dem Audit aktualisieren")
    parser.add_argument("--refresh-only", action="store_true", help="nur crates.io-Feed herunterladen und prüfen")
    parser.add_argument("--max-age-days", type=int, default=7, help="maximales Feed-Alter in Tagen (Standard 7)")
    parser.add_argument("--allow-stale", action="store_true", help="veralteten Feed nicht als Fehler (Exit 2) werten")
    parser.add_argument("--today", type=date.fromisoformat, help="Bezugsdatum YYYY-MM-DD für Fristprüfungen")
    parser.add_argument("--json", type=Path, metavar="DATEI", help="JSON-Ergebnis zusätzlich in Datei schreiben")
    parser.add_argument("--html", type=Path, metavar="DATEI", help="HTML-Bericht zusätzlich schreiben")
    parser.add_argument("--sarif", type=Path, metavar="DATEI", help="SARIF-2.1.0-Bericht zusätzlich schreiben")
    args = parser.parse_args()

    cache = (args.cache or default_cache_dir()).resolve()
    if args.refresh or args.refresh_only:
        failed = watcher.refresh(cache, only_due=False, ecosystems=("crates.io",))
        if failed:
            print(json.dumps({"refresh_failed": failed}), file=sys.stderr)
            return 2
        if args.refresh_only:
            print(json.dumps({"refreshed": feed_info(cache, time.time(), args.max_age_days)}))
            return 0

    if not args.scanner:
        parser.error("--scanner ist für den Audit erforderlich (außer bei --refresh-only).")

    try:
        code, report = run_audit(
            args.lockfile.resolve(),
            cache,
            args.scanner,
            args.triage.resolve() if args.triage else None,
            max_age_days=max(1, args.max_age_days),
            allow_stale=args.allow_stale,
            today=args.today,
        )
    except (OSError, ValueError, tomllib.TOMLDecodeError, watcher.WatchError) as error:
        print(f"Audit-Fehler: {error}", file=sys.stderr)
        return 2

    if args.json:
        atomic_write_text(args.json.resolve(), json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    if args.html:
        write_audit_html(args.html.resolve(), report)
    if args.sarif:
        write_sarif(args.sarif.resolve(), report)
    print_summary(report)
    return code


__all__ = [
    "CRATES_IO_SOURCE",
    "TriageEntry",
    "evaluate_findings",
    "feed_info",
    "load_triage",
    "main",
    "normalize_crate",
    "parse_cargo_lock",
    "print_summary",
    "run_audit",
]

if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
