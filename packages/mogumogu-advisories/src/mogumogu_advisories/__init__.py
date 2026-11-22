"""Offline-first OSV/CVE vulnerability auditing library for projects and CI/CD pipelines.

Zero third-party runtime dependencies (Python 3.11+ standard library only).
"""
from .audit import audit_cargo_lock, audit_packages, audit_project, print_audit_summary
from .feeds import (
    DAY,
    MAX_AGE,
    check_archive,
    db_path,
    default_cache_dir,
    download_feed,
    feed_info,
    feed_status,
    refresh_feeds,
)
from .lockfiles import (
    SUPPORTED_LOCKFILES,
    ScanResult,
    discover_lockfiles,
    parse_lockfile,
    parse_package_list,
    scan_lockfiles,
)
from .models import (
    ECOSYSTEMS,
    PROJECT_ECOSYSTEMS,
    Advisory,
    Evidence,
    Package,
    TriageEntry,
    WatchError,
    normalize_package_name,
    normalize_version,
)
from .report_html import render_audit_html, render_html, write_audit_html, write_html
from .report_sarif import SARIF_SCHEMA, SARIF_VERSION, render_sarif, write_sarif
from .scanner import (
    OSV_SCANNER_CHECKSUMS,
    OSV_SCANNER_VERSION,
    default_scanner_path,
    detect_platform,
    download_scanner,
    executable,
    match_packages,
    read_matches,
)
from .triage import discover_triage, evaluate_findings, load_triage

__version__ = "0.1.0"

__all__ = [
    "DAY",
    "ECOSYSTEMS",
    "MAX_AGE",
    "OSV_SCANNER_CHECKSUMS",
    "OSV_SCANNER_VERSION",
    "PROJECT_ECOSYSTEMS",
    "SARIF_SCHEMA",
    "SARIF_VERSION",
    "SUPPORTED_LOCKFILES",
    "Advisory",
    "Evidence",
    "Package",
    "ScanResult",
    "TriageEntry",
    "WatchError",
    "__version__",
    "audit_cargo_lock",
    "audit_packages",
    "audit_project",
    "check_archive",
    "db_path",
    "default_cache_dir",
    "default_scanner_path",
    "detect_platform",
    "discover_lockfiles",
    "discover_triage",
    "download_feed",
    "download_scanner",
    "evaluate_findings",
    "executable",
    "feed_info",
    "feed_status",
    "load_triage",
    "match_packages",
    "normalize_package_name",
    "normalize_version",
    "parse_lockfile",
    "parse_package_list",
    "print_audit_summary",
    "read_matches",
    "refresh_feeds",
    "render_audit_html",
    "render_html",
    "render_sarif",
    "scan_lockfiles",
    "write_audit_html",
    "write_html",
    "write_sarif",
]
