"""SARIF 2.1.0 report generator for GitHub Code Scanning, GitLab SAST, and IDEs."""
import hashlib
import json
from pathlib import Path

from ._io import atomic_write_text

SARIF_SCHEMA = "https://json.schemastore.org/sarif-2.1.0.json"
SARIF_VERSION = "2.1.0"
TOOL_NAME = "mogumogu-advisories"
TOOL_VERSION = "0.1.0"
TOOL_URI = "https://osv.dev/"


def _fingerprint(ecosystem: str, package: str, version: str, rule_id: str) -> str:
    payload = f"{ecosystem}\0{package}\0{version}\0{rule_id}".encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def render_sarif(report: dict) -> dict:
    """Converts an audit report dictionary into a SARIF 2.1.0 JSON object."""
    rules_by_id: dict[str, dict] = {}
    results: list[dict] = []

    def add_finding(row: dict, status: str) -> None:
        advisory_ids = row.get("advisory_ids", [])
        cve_ids = row.get("cve_ids", [])
        rule_id = advisory_ids[0] if advisory_ids else "OSV-UNKNOWN"
        eco = row.get("ecosystem", "crates.io")
        pkg = row.get("package", row.get("crate", "unknown"))
        ver = row.get("version", "")
        url = row.get("url", f"https://osv.dev/vulnerability/{rule_id}")

        if rule_id not in rules_by_id:
            tags = ["security", "vulnerability", f"ecosystem:{eco}", *cve_ids]
            cve_suffix = f" ({', '.join(cve_ids)})" if cve_ids else ""
            rules_by_id[rule_id] = {
                "id": rule_id,
                "name": rule_id,
                "shortDescription": {"text": f"OSV Advisory {rule_id}{cve_suffix}"},
                "fullDescription": {
                    "text": (
                        f"Bekannte Schwachstelle oder Wartungshinweis {rule_id}{cve_suffix} "
                        f"im Ökosystem {eco}. Details siehe {url}."
                    )
                },
                "helpUri": url,
                "properties": {"tags": tags},
            }

        ids_label = ", ".join(advisory_ids)
        cves_label = f" (CVEs: {', '.join(cve_ids)})" if cve_ids else ""
        if status == "expired":
            msg = (
                f"[{eco}] {pkg} {ver}: {ids_label}{cves_label} — "
                f"Triage-Frist abgelaufen am {row.get('expires', '?')} ({row.get('reason', '')})"
            )
            level = "error"
        elif status == "triaged":
            msg = (
                f"[{eco}] {pkg} {ver}: {ids_label}{cves_label} — "
                f"befristet getriagt bis {row.get('expires', '?')} ({row.get('reason', '')})"
            )
            level = "note"
        else:
            msg = f"[{eco}] {pkg} {ver}: {ids_label}{cves_label} — ungetriagtes OSV-Advisory"
            level = "error"

        locations = [
            {
                "physicalLocation": {
                    "artifactLocation": {"uri": loc},
                    "region": {"startLine": 1},
                }
            }
            for loc in (row.get("locations") or ["Cargo.lock"])
        ]
        result_obj: dict = {
            "ruleId": rule_id,
            "level": level,
            "message": {"text": msg},
            "locations": locations,
            "partialFingerprints": {
                "primaryLocationLineHash": _fingerprint(eco, pkg, ver, rule_id),
            },
            "properties": {
                "ecosystem": eco,
                "package": pkg,
                "version": ver,
                "advisory_ids": advisory_ids,
                "cve_ids": cve_ids,
                "triage_status": status,
            },
        }
        if status == "triaged":
            result_obj["suppressions"] = [{
                "kind": "external",
                "status": "accepted",
                "justification": (
                    f"{row.get('scope', 'project')} · {row.get('kind', 'advisory')} · "
                    f"befristet bis {row.get('expires', '?')}: {row.get('reason', '')}"
                ),
            }]
        results.append(result_obj)

    for row in report.get("untriaged", []):
        add_finding(row, "untriaged")
    for row in report.get("expired", []):
        add_finding(row, "expired")
    for row in report.get("triaged", []):
        add_finding(row, "triaged")

    return {
        "$schema": SARIF_SCHEMA,
        "version": SARIF_VERSION,
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": TOOL_NAME,
                        "version": TOOL_VERSION,
                        "informationUri": TOOL_URI,
                        "rules": [rules_by_id[key] for key in sorted(rules_by_id)],
                    }
                },
                "results": results,
                "properties": {
                    "checked_at": report.get("checked_at"),
                    "complete": report.get("complete", False),
                    "packages_total": report.get("packages_total", report.get("crates_total", 0)),
                    "packages_checked": report.get("packages_checked", report.get("crates_checked", 0)),
                    "skipped": report.get("skipped", report.get("non_registry_skipped", 0)),
                    "feeds": report.get("feeds", {}),
                },
            }
        ],
    }


def write_sarif(path: Path, report: dict) -> None:
    payload = json.dumps(render_sarif(report), indent=2, ensure_ascii=False) + "\n"
    atomic_write_text(path, payload, prefix="mogumogu-sarif-", filename="report.sarif")
