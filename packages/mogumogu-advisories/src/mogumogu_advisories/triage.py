"""Dated TOML triage manifest loader and finding evaluator (multi-ecosystem)."""
from datetime import date
from pathlib import Path
import tomllib

from .models import (
    ADVISORY_ID,
    PROJECT_ECOSYSTEMS,
    Advisory,
    Package,
    TriageEntry,
    normalize_package_name,
    normalize_version,
)


def discover_triage(target: Path) -> Path | None:
    """Finds a standard triage manifest beside `target` without following symlinks."""
    resolved = Path(target).resolve()
    base = resolved if resolved.is_dir() else resolved.parent
    for candidate in (base / "security" / "advisory-triage.toml", base / "advisory-triage.toml"):
        if candidate.is_file() and not candidate.is_symlink():
            return candidate
    return None


def load_triage(path: Path | None) -> list[TriageEntry]:
    """Loads dated advisory exemptions from a TOML manifest.

    Supports both `package` + `ecosystem` for multi-ecosystem projects and the
    shorthand `crate` (defaulting `ecosystem = "crates.io"`).
    """
    if path is None:
        return []
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    entries: list[TriageEntry] = []
    seen: set[tuple[str, str, str, str]] = set()
    for idx, row in enumerate(data.get("advisory", []), 1):
        if not isinstance(row, dict):
            raise ValueError(f"Triage-Eintrag #{idx}: Tabelle erwartet.")
        raw_id = row.get("id")
        raw_pkg = row.get("package") or row.get("crate")
        raw_eco = row.get("ecosystem") or ("crates.io" if row.get("crate") else None)
        raw_ver = row.get("version")
        raw_expires = row.get("expires")
        raw_reason = row.get("reason")
        missing = [
            label
            for label, val in (
                ("id", raw_id),
                ("ecosystem", raw_eco),
                ("package/crate", raw_pkg),
                ("version", raw_ver),
                ("expires", raw_expires),
                ("reason", raw_reason),
            )
            if not isinstance(val, str) or not val.strip()
        ]
        if missing:
            raise ValueError(f"Triage-Eintrag #{idx}: Pflichtfelder fehlen ({', '.join(missing)}).")
        advisory_id = raw_id.strip()
        if not ADVISORY_ID.fullmatch(advisory_id):
            raise ValueError(f"Triage-Eintrag #{idx}: ungültige Advisory-ID {advisory_id!r}.")
        ecosystem = raw_eco.strip()
        if ecosystem not in PROJECT_ECOSYSTEMS:
            raise ValueError(f"Triage-Eintrag #{idx}: nicht unterstütztes Ökosystem {ecosystem!r}.")
        package = normalize_package_name(ecosystem, raw_pkg.strip())
        version = normalize_version(ecosystem, raw_ver.strip())
        expires = date.fromisoformat(raw_expires.strip())
        scope = str(row.get("scope", "project")).strip() or "project"
        kind = str(row.get("kind", "advisory")).strip() or "advisory"
        key = (advisory_id, ecosystem, package, version)
        if key in seen:
            raise ValueError(
                f"Triage-Eintrag #{idx}: doppelter Eintrag für {advisory_id} ({ecosystem} {package} {version})."
            )
        seen.add(key)
        entries.append(TriageEntry(
            advisory_id=advisory_id,
            ecosystem=ecosystem,
            package=package,
            version=version,
            scope=scope,
            kind=kind,
            expires=expires,
            reason=raw_reason.strip(),
        ))
    return entries


def evaluate_findings(
    matches: dict[Package, set[Advisory]],
    triage: list[TriageEntry],
    today: date,
    *,
    locations: dict[Package, set[str]] | None = None,
) -> dict:
    """Classifies matched advisories into untriaged, expired, triaged, and unused."""
    untriaged: list[dict] = []
    expired: list[dict] = []
    triaged: list[dict] = []
    matched_triage: set[tuple[str, str, str, str]] = set()
    loc_map = locations or {}

    for package in sorted(matches):
        pkg_locations = sorted(loc_map.get(package, ()))
        for advisory in sorted(matches[package]):
            entry = next(
                (
                    item for item in triage
                    if item.ecosystem == package.ecosystem
                    and item.package == package.name
                    and item.version == package.version
                    and item.advisory_id in advisory.ids
                ),
                None,
            )
            record = {
                "ecosystem": package.ecosystem,
                "package": package.name,
                "crate": package.name,
                "version": package.version,
                "advisory_ids": list(advisory.ids),
                "cve_ids": list(advisory.cves),
                "url": f"https://osv.dev/vulnerability/{advisory.ids[0]}",
                "locations": pkg_locations,
            }
            if entry is None:
                untriaged.append(record)
            else:
                matched_triage.add((entry.advisory_id, entry.ecosystem, entry.package, entry.version))
                record.update(
                    triage_id=entry.advisory_id,
                    scope=entry.scope,
                    kind=entry.kind,
                    expires=entry.expires.isoformat(),
                    reason=entry.reason,
                )
                if entry.expires < today:
                    expired.append(record)
                else:
                    triaged.append(record)

    unused_triage = [
        {
            "id": item.advisory_id,
            "ecosystem": item.ecosystem,
            "package": item.package,
            "crate": item.package,
            "version": item.version,
            "expires": item.expires.isoformat(),
        }
        for item in triage
        if (item.advisory_id, item.ecosystem, item.package, item.version) not in matched_triage
    ]
    return {
        "untriaged": untriaged,
        "expired": expired,
        "triaged": triaged,
        "unused_triage": unused_triage,
    }
