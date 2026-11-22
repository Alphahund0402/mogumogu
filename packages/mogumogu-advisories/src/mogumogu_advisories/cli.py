"""Standalone CLI for `mogumogu-advisories` (projects, CI/CD pipelines, and Docker)."""
import argparse
from datetime import date
import json
from pathlib import Path
import sys
import tomllib

from ._io import atomic_write_text
from .audit import audit_project, print_audit_summary
from .feeds import default_cache_dir, feed_info, refresh_feeds
from .lockfiles import parse_package_list, scan_lockfiles
from .models import PROJECT_ECOSYSTEMS, WatchError
from .report_html import write_audit_html
from .report_sarif import write_sarif
from .scanner import OSV_SCANNER_VERSION, default_scanner_path, download_scanner, executable
from .triage import discover_triage
from .watcher import main as watcher_main


def _resolve_triage(args: argparse.Namespace, target: Path) -> Path | None:
    if args.no_triage:
        return None
    if args.triage is not None:
        return args.triage.resolve()
    return discover_triage(target)


def _display_rel(path: Path, base: Path) -> str:
    try:
        return path.relative_to(base).as_posix()
    except ValueError:
        return path.name


def _resolve_scanner(args: argparse.Namespace, cache: Path) -> Path | None:
    candidate = (args.scanner or default_scanner_path(cache)).resolve()
    if args.update and not candidate.is_file():
        try:
            asset, sha256, size = download_scanner(candidate)
            if not args.quiet:
                print(
                    f"OSV-Scanner geladen und geprüft: {asset} {OSV_SCANNER_VERSION} "
                    f"({size} Bytes, sha256={sha256[:12]}…)"
                )
        except (OSError, WatchError) as error:
            print(f"Scanner-Download fehlgeschlagen: {error}", file=sys.stderr)
            return None
    try:
        return executable(candidate)
    except argparse.ArgumentTypeError:
        print(
            f"OSV-Scanner nicht gefunden: {candidate}\n"
            "Tipp: einmalig `--update` ergänzen oder `mogumogu-advisories fetch-scanner` ausführen.",
            file=sys.stderr,
        )
        return None


def _refresh_audit_feeds(
    args: argparse.Namespace,
    target: Path,
    cache: Path,
    ecosystems_filter: set[str] | None,
) -> bool:
    if ecosystems_filter:
        to_refresh = sorted(ecosystems_filter)
    elif args.packages_file:
        scanned = parse_package_list(args.packages_file.resolve())
        to_refresh = sorted({pkg.ecosystem for pkg in scanned.packages})
    else:
        scanned = scan_lockfiles(target, explicit_files=args.lockfile)
        to_refresh = sorted({pkg.ecosystem for pkg in scanned.packages})
    if not to_refresh:
        return True
    failed = refresh_feeds(cache, only_due=False, ecosystems=to_refresh)
    if failed:
        print(json.dumps({"refresh_failed": failed}), file=sys.stderr)
        return False
    return True


def _write_audit_reports(args: argparse.Namespace, report: dict) -> None:
    out_dir = args.out.resolve() if args.out else None
    json_path = args.json.resolve() if args.json else (out_dir / "audit.json" if out_dir else None)
    html_path = args.html.resolve() if args.html else (out_dir / "audit.html" if out_dir else None)
    sarif_path = args.sarif.resolve() if args.sarif else (out_dir / "audit.sarif" if out_dir else None)
    if json_path:
        payload = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
        atomic_write_text(json_path, payload, prefix="mogumogu-json-", filename="audit.json")
    if html_path:
        write_audit_html(html_path, report)
    if sarif_path:
        write_sarif(sarif_path, report)


def _cmd_audit(args: argparse.Namespace) -> int:
    cache = (args.cache or default_cache_dir()).resolve()
    ecosystems_filter = set(args.ecosystem) if args.ecosystem else None
    target = args.target.resolve()

    scanner = _resolve_scanner(args, cache)
    if scanner is None:
        return 2
    if (args.refresh or args.update) and not _refresh_audit_feeds(args, target, cache, ecosystems_filter):
        return 2

    triage_path = _resolve_triage(args, target)
    try:
        code, report = audit_project(
            target,
            cache,
            scanner,
            triage_path=triage_path,
            explicit_lockfiles=[p.resolve() for p in args.lockfile] if args.lockfile else None,
            packages_file=args.packages_file.resolve() if args.packages_file else None,
            ecosystems_filter=ecosystems_filter,
            max_age_days=max(1, args.max_age_days),
            allow_stale=args.allow_stale,
            today=args.today,
        )
    except (OSError, ValueError, tomllib.TOMLDecodeError, WatchError) as error:
        print(f"Audit-Fehler: {error}", file=sys.stderr)
        return 2

    if triage_path is not None:
        base = target if target.is_dir() else target.parent
        report["triage_file"] = _display_rel(triage_path, base)
    _write_audit_reports(args, report)
    if not args.quiet:
        print_audit_summary(report)
    return code


def _cmd_refresh(args: argparse.Namespace) -> int:
    cache = (args.cache or default_cache_dir()).resolve()
    if args.all:
        ecosystems = list(PROJECT_ECOSYSTEMS)
    elif args.ecosystem:
        ecosystems = sorted(set(args.ecosystem))
    else:
        target = (args.project or Path(".")).resolve()
        scanned = scan_lockfiles(target)
        ecosystems = sorted({pkg.ecosystem for pkg in scanned.packages})
        if not ecosystems:
            print("Keine unterstützten Lockfiles gefunden (nutze --ecosystem oder --all).", file=sys.stderr)
            return 2

    failed = refresh_feeds(cache, only_due=args.only_due, ecosystems=ecosystems)
    summary = {
        "refreshed": {eco: feed_info(cache, eco) for eco in ecosystems if eco not in failed},
        "refresh_failed": failed,
    }
    print(json.dumps(summary, indent=2, ensure_ascii=False))
    return 2 if failed else 0


def _cmd_fetch_scanner(args: argparse.Namespace) -> int:
    cache = (args.cache or default_cache_dir()).resolve()
    output = (args.output or default_scanner_path(cache)).resolve()
    try:
        asset, sha256, size = download_scanner(output, os_name=args.os, arch=args.arch)
    except (OSError, WatchError) as error:
        print(f"Download-Fehler: {error}", file=sys.stderr)
        return 2
    print(f"Verified {asset} {OSV_SCANNER_VERSION} ({size} bytes, sha256={sha256}) -> {output}")
    return 0


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="mogumogu-advisories",
        description="Offline-first OSV/CVE vulnerability audit library and CLI.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_audit = sub.add_parser("audit", help="Projektordner, Lockfile(s) oder Paketliste offline gegen OSV prüfen")
    p_audit.add_argument("target", nargs="?", type=Path, default=Path("."), help="Projektordner oder Lockfile (Standard: .)")
    p_audit.add_argument("--cache", type=Path, help="OSV-Cache (Standard: MOGUMOGU_OSV_CACHE oder lokaler Benutzer-Cache)")
    p_audit.add_argument("--scanner", type=Path, help="OSV-Scanner v2 (Standard: MOGUMOGU_OSV_SCANNER oder <cache>/bin)")
    p_audit.add_argument("--triage", type=Path, help="Triage-TOML (Standard: automatisch security/advisory-triage.toml oder advisory-triage.toml)")
    p_audit.add_argument("--no-triage", action="store_true", help="automatische Triage-Erkennung deaktivieren")
    p_audit.add_argument("--lockfile", type=Path, action="append", help="explizite Lockfile-Datei (mehrfach möglich)")
    p_audit.add_argument("--packages-file", type=Path, help="JSON-Datei mit [{ecosystem, name, version}]")
    p_audit.add_argument(
        "--ecosystem", action="append", choices=PROJECT_ECOSYSTEMS,
        help="auf bestimmte OSV-Ökosysteme eingrenzen (mehrfach möglich)",
    )
    p_audit.add_argument("--update", action="store_true", help="Scanner (falls fehlend) und benötigte OSV-Feeds vor dem Audit aktualisieren")
    p_audit.add_argument("--refresh", action="store_true", help="nur benötigte OSV-Feeds vor dem Audit aktualisieren")
    p_audit.add_argument("--max-age-days", type=int, default=7, help="maximales Feed-Alter in Tagen (Standard: 7)")
    p_audit.add_argument("--allow-stale", action="store_true", help="veralteten Feed nicht mit Exit 2 blockieren")
    p_audit.add_argument("--today", type=date.fromisoformat, help="Bezugsdatum YYYY-MM-DD für Fristprüfungen")
    p_audit.add_argument("--out", type=Path, metavar="ORDNER", help="audit.json, audit.html und audit.sarif gesammelt schreiben")
    p_audit.add_argument("--json", type=Path, metavar="DATEI", help="JSON-Bericht schreiben")
    p_audit.add_argument("--html", type=Path, metavar="DATEI", help="HTML-Bericht schreiben")
    p_audit.add_argument("--sarif", type=Path, metavar="DATEI", help="SARIF-2.1.0-Bericht schreiben")
    p_audit.add_argument("--quiet", action="store_true", help="keine Konsolenzusammenfassung ausgeben")

    p_ref = sub.add_parser("refresh", help="Öffentliche OSV-Datenbanken (all.zip) herunterladen und prüfen")
    p_ref.add_argument("--cache", type=Path, help="Zielverzeichnis (Standard: MOGUMOGU_OSV_CACHE oder Benutzer-Cache)")
    p_ref.add_argument("--ecosystem", action="append", choices=PROJECT_ECOSYSTEMS, help="Ökosystem laden (mehrfach möglich)")
    p_ref.add_argument("--project", type=Path, help="benötigte Ökosysteme aus Lockfiles ermitteln (Standard: .)")
    p_ref.add_argument("--all", action="store_true", help="alle 12 unterstützten Ökosysteme laden")
    p_ref.add_argument("--only-due", action="store_true", help="nur Feeds laden, die fehlen oder >= 1 Tag alt sind")

    p_fetch = sub.add_parser("fetch-scanner", help=f"Offiziellen OSV-Scanner {OSV_SCANNER_VERSION} SHA-256-geprüft laden")
    p_fetch.add_argument("--cache", type=Path, help="Cache-Verzeichnis für Standard-Ziel <cache>/bin")
    p_fetch.add_argument("--output", type=Path, help="Zielpfad der ausführbaren Datei (Standard: <cache>/bin/osv-scanner)")
    p_fetch.add_argument("--os", choices=("linux", "windows", "darwin"), help="Zielbetriebssystem (Standard: automatisch)")
    p_fetch.add_argument("--arch", choices=("amd64", "arm64"), help="Zielarchitektur (Standard: automatisch)")

    sub.add_parser("watch", help="mogumogu-Inventar-Watcher ausführen (reicht Argumente an watcher weiter)")
    return parser


def main(argv: list[str] | None = None) -> int:
    args_list = sys.argv[1:] if argv is None else list(argv)
    if args_list and args_list[0] == "watch":
        return watcher_main(args_list[1:])

    parsed = _build_parser().parse_args(args_list)
    if parsed.command == "audit":
        return _cmd_audit(parsed)
    if parsed.command == "refresh":
        return _cmd_refresh(parsed)
    if parsed.command == "fetch-scanner":
        return _cmd_fetch_scanner(parsed)
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
