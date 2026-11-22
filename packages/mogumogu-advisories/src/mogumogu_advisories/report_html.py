"""Self-contained, CSP-hardened HTML reports for watcher and standalone project audits."""
from collections import defaultdict
from datetime import datetime, timezone
from html import escape
from pathlib import Path
from urllib.parse import quote

from ._io import atomic_write_text

SCAN_LABELS = {
    "complete": "Vollständig erfasst",
    "not_scanned": "Erfassung ausstehend",
    "incomplete": "Erfassung unvollständig",
    "unavailable": "Ordner nicht erreichbar",
    "identity_changed": "Ordneridentität geändert",
    "not_approved": "Lesefreigabe ausstehend",
}
STYLE = """
:root { color-scheme: light; font: 16px/1.55 "Segoe UI", sans-serif; color: #182532; background: #f4f6f9; }
body { margin: 0; }
main { max-width: 74rem; margin: auto; padding: 1.5rem clamp(1rem, 4vw, 3rem) 3rem; }
h1 { font-size: clamp(1.65rem, 4vw, 2.4rem); margin-bottom: .2rem; }
h2 { font-size: 1.25rem; margin-top: 1.6rem; }
p { margin: .45rem 0 .9rem; }
a { color: #155275; text-underline-offset: .18em; }
small, .muted { color: #465668; }
.summary, details { background: white; border: 1px solid #cfd7e1; border-radius: .55rem; }
.summary { padding: .7rem 1rem; }
.warning { border-inline-start: .3rem solid #b47315; }
.danger { border-inline-start: .3rem solid #b3261e; }
.ok { border-inline-start: .3rem solid #1b6e3c; }
details { margin: .65rem 0; padding: .65rem .85rem; }
summary { cursor: pointer; font-weight: 600; }
.table { overflow-x: auto; }
table { width: 100%; border-collapse: collapse; text-align: left; background: white; }
td, th { padding: .55rem .7rem; border-bottom: 1px solid #dce2ea; vertical-align: top; }
th { background: #eef2f7; }
code { font-size: .93em; overflow-wrap: anywhere; }
.identifiers { display: flex; flex-wrap: wrap; gap: .25rem .6rem; }
"""


def text(value) -> str:
    return escape(str(value), quote=True)


def timestamp(at: float) -> str:
    return datetime.fromtimestamp(at, timezone.utc).strftime("%Y-%m-%d %H:%M UTC")


def _identifier_links(advisory_ids, cve_ids) -> str:
    cve_set = set(cve_ids)
    leads = [
        f'<a href="https://osv.dev/vulnerability/{quote(identifier, safe="")}">{text(identifier)}</a>'
        for identifier in advisory_ids
        if identifier not in cve_set
    ]
    leads.extend(
        f'<a href="https://www.cve.org/CVERecord?id={quote(identifier, safe="")}">{text(identifier)}</a>'
        for identifier in cve_ids
    )
    return f'<div class="identifiers">{" ".join(leads)}</div>'


def _wrap_document(title: str, body: list[str]) -> str:
    return (
        '<!doctype html><html lang="de"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; style-src \'unsafe-inline\'; '
        f'base-uri \'none\'; form-action \'none\'"><title>{text(title)}</title>'
        f'<style>{STYLE}</style></head><body><main>{"".join(body)}</main></body></html>'
    )


def finding_table(findings) -> str:
    if not findings:
        return '<p class="muted">Keine Hinweise im geprüften Umfang.</p>'
    rows = []
    for finding, evidence in findings:
        labels = {
            ("Installiert" if item["state"] == "installed" else "Aufgelöst")
            + (" · öffentliche Quelle" if item["public_source"] else " · Herkunft offen")
            for item in evidence
        }
        if not evidence and finding["user_reported_tool"]:
            labels.add("Eigene Werkzeugliste")
        rows.append(
            f'<tr><td><code>{text(finding["name"])}</code><br><small>{text(finding["ecosystem"])}</small></td>'
            f'<td><code>{text(finding["version"])}</code></td><td>{"<br>".join(sorted(labels))}</td>'
            f'<td>{_identifier_links(finding["advisory_ids"], finding["cve_ids"])}</td></tr>'
        )
    return (
        '<div class="table"><table><thead><tr><th>Abhängigkeit</th><th>Version</th>'
        '<th>Beleg</th><th>Advisories und CVEs</th></tr></thead><tbody>'
        + "".join(rows) + "</tbody></table></div>"
    )


def render_html(report: dict) -> str:
    """Renders a mogumogu watcher report (`watch_advisories`) as HTML."""
    body = [
        "<h1>Schwachstellenbericht</h1>",
        f'<p class="muted">{text(report["checked_at"])}</p>',
    ]
    if report.get("error"):
        body.append(f'<p class="summary warning">{text(report["error"])}</p>')
    else:
        status = "Teilabdeckung" if report["partial"] else "Auswertung abgeschlossen"
        style = "summary warning" if report["partial"] else "summary"
        body.append(
            f'<p class="{style}"><strong>{status}</strong> · {report["checked"]} von {report["candidates"]} '
            f'Einträgen geprüft · {len(report["findings"])} Hinweise je Paketversion</p>'
        )
        if report.get("scan_cancelled"):
            body.append('<p class="summary warning">Erfassung abgebrochen. Der Bericht nutzt verfügbare Stände.</p>')
        for ecosystem, feed_state in report["feeds"].items():
            body.append(f'<span class="muted">{text(ecosystem)} {text(feed_state)} · </span>')
        if report["refresh_failed"]:
            body.append(
                '<p class="summary warning">Advisory-Abruf fehlgeschlagen für '
                f'{text(", ".join(report["refresh_failed"]))}.</p>'
            )
        inventory = report["inventory"]
        if inventory["scope_limit_reached"] or inventory["item_limit_reached"]:
            body.append('<p class="summary warning">Inventarlimit erreicht. Projekte einzeln prüfen.</p>')

        projects = report.get("projects")
        if projects is None:
            body.append("<h2>Installierte Pakete und Werkzeuge</h2>")
            if report["checked"]:
                body.append(finding_table([(finding, finding["evidence"]) for finding in report["findings"]]))
        else:
            by_scope = defaultdict(list)
            tools = []
            for finding in report["findings"]:
                scopes = defaultdict(list)
                for item in finding["evidence"]:
                    scopes[item["scope_id"]].append(item)
                for scope, items in scopes.items():
                    by_scope[scope].append((finding, items))
                if finding["user_reported_tool"]:
                    tools.append((finding, []))
            body.append("<h2>Projekte</h2>")
            if not projects:
                body.append('<p class="muted">Keine registrierten Projekte.</p>')
            for project in sorted(projects, key=lambda p: (not by_scope[p["scope_id"]], p["name"].casefold(), p["id"])):
                hits = by_scope[project["scope_id"]]
                label = SCAN_LABELS.get(project["scan_state"], "Erfassungsstand offen")
                stamp = project["scan_finished_at"]
                coverage = [label if stamp is None else f"{label} am {timestamp(stamp)}"]
                if project["stale"]:
                    coverage.append("Inventar veraltet")
                if project["scan_failed"]:
                    coverage.append("Erfassung fehlgeschlagen")
                if project["skipped"]:
                    coverage.append(f'{project["skipped"]} Belege übersprungen')
                if project["advisory_gaps"]:
                    coverage.append(f'{project["advisory_gaps"]} Versionen mit fehlender oder veralteter Abdeckung')
                if project["scan_state"] == "complete" and not project["candidates"]:
                    coverage.append("Keine unterstützten Versionsbelege")
                body.append(
                    f'<details{" open" if hits else ""}><summary>{text(project["name"])} <small>#{project["id"]} · '
                    f'{project["checked"]}/{project["candidates"]} geprüft · {len(hits)} Hinweise</small></summary>'
                    f'<p class="muted">{text(" · ".join(coverage))}</p>'
                    f'{finding_table(hits) if project["checked"] else ""}</details>'
                )
            if tools:
                body.append("<h2>Eigene Werkzeugliste</h2>")
                body.append(finding_table(tools))
        body.append(
            '<p class="muted">Grundlage sind exakte Versionen und die verfügbaren OSV-Daten. '
            'Aufgelöste Versionen stammen aus Paketmetadaten. Prüfe Treffer anhand der verlinkten Advisories.</p>'
        )
    return _wrap_document("Schwachstellenbericht · mogumogu", body)


def write_html(path: Path, report: dict) -> None:
    atomic_write_text(path, render_html(report), prefix="mogumogu-report-", filename="bericht.html")


def _audit_table(rows: list[dict], *, include_triage: bool) -> str:
    if not rows:
        return '<p class="muted">Keine Einträge.</p>'
    trs = []
    for row in rows:
        pkg = row.get("package", row.get("crate", "?"))
        eco = row.get("ecosystem", "crates.io")
        locs = "<br>".join(f"<code>{text(loc)}</code>" for loc in row.get("locations", [])) or "—"
        triage_col = ""
        if include_triage:
            triage_col = (
                f'<td><strong>{text(row.get("scope", ""))}</strong> · {text(row.get("kind", ""))}<br>'
                f'<small>Frist: {text(row.get("expires", ""))} · {text(row.get("reason", ""))}</small></td>'
            )
        trs.append(
            f'<tr><td><code>{text(pkg)}</code><br><small>{text(eco)}</small></td>'
            f'<td><code>{text(row["version"])}</code></td>'
            f'<td>{locs}</td>'
            f'<td>{_identifier_links(row["advisory_ids"], row["cve_ids"])}</td>'
            f'{triage_col}</tr>'
        )
    extra_header = "<th>Triage / Befristung</th>" if include_triage else ""
    return (
        '<div class="table"><table><thead><tr><th>Paket</th><th>Version</th>'
        f'<th>Fundstelle</th><th>Advisories und CVEs</th>{extra_header}</tr></thead><tbody>'
        + "".join(trs) + "</tbody></table></div>"
    )


def render_audit_html(report: dict) -> str:
    """Renders a standalone `audit_project` / `audit_cargo_lock` report as HTML."""
    body = [
        "<h1>Sicherheits-Audit (OSV / CVE)</h1>",
        f'<p class="muted">{text(report["checked_at"])}</p>',
    ]
    if report.get("error"):
        body.append(f'<p class="summary warning">{text(report["error"])}</p>')
    else:
        untriaged = report.get("untriaged", [])
        expired = report.get("expired", [])
        triaged = report.get("triaged", [])
        if not report.get("complete"):
            cls, state = "summary warning", "Unvollständiger Audit"
        elif untriaged or expired:
            cls, state = "summary danger", "Handlungsbedarf (blockiert)"
        else:
            cls, state = "summary ok", "Prüfung bestanden"
        total = report.get("packages_total", report.get("crates_total", 0))
        checked = report.get("packages_checked", report.get("crates_checked", 0))
        skipped = report.get("skipped", report.get("non_registry_skipped", 0))
        body.append(
            f'<p class="{cls}"><strong>{state}</strong> · {checked}/{total} Pakete geprüft '
            f'({skipped} übersprungen) · {len(untriaged)} ungetriagt · {len(expired)} abgelaufen · '
            f'{len(triaged)} befristet getriagt</p>'
        )
        feeds = report.get("feeds", {})
        if feeds:
            feed_rows = "".join(
                f'<tr><td><code>{text(eco)}</code></td><td>{text(info["status"])}</td>'
                f'<td>{text(info.get("modified_at") or "—")}</td>'
                f'<td>{text(info.get("age_days") if info.get("age_days") is not None else "—")} / '
                f'{text(info.get("max_age_days", 7))} Tage</td></tr>'
                for eco, info in sorted(feeds.items())
            )
            body.append(
                "<h2>OSV-Datenstand</h2><div class=\"table\"><table><thead>"
                "<tr><th>Ökosystem</th><th>Status</th><th>Letzte Änderung</th><th>Alter / Limit</th></tr>"
                f"</thead><tbody>{feed_rows}</tbody></table></div>"
            )
        if untriaged:
            body.append("<h2>Ungetriagte Advisories</h2>")
            body.append(_audit_table(untriaged, include_triage=False))
        if expired:
            body.append("<h2>Abgelaufene Triage-Einträge</h2>")
            body.append(_audit_table(expired, include_triage=True))
        if triaged:
            body.append("<details><summary>Befristet getriagte Advisories "
                        f"<small>({len(triaged)})</small></summary>"
                        f"{_audit_table(triaged, include_triage=True)}</details>")
        if report.get("warnings"):
            items = "".join(f"<li>{text(w)}</li>" for w in report["warnings"])
            body.append(f'<h2>Warnungen</h2><ul>{items}</ul>')
        body.append(
            '<p class="muted">Grundlage sind exakte Lockfile-Versionen und lokale OSV-Archive. '
            'Ein bestandener Lauf deckt ausschließlich die geprüften Paketversionen und den genannten Datenstand ab.</p>'
        )
    return _wrap_document("Sicherheits-Audit · mogumogu-advisories", body)


def write_audit_html(path: Path, report: dict) -> None:
    atomic_write_text(path, render_audit_html(report), prefix="mogumogu-report-", filename="bericht.html")
