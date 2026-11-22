"""Unit tests for SARIF 2.1.0 and CSP-hardened standalone HTML audit reports."""
import json
from pathlib import Path
import tempfile
import unittest

from mogumogu_advisories import SARIF_VERSION, render_audit_html, render_sarif, write_audit_html, write_sarif


SAMPLE_REPORT = {
    "checked_at": "2026-11-22T08:00:00+00:00",
    "today": "2026-11-22",
    "complete": True,
    "feeds": {
        "crates.io": {
            "ecosystem": "crates.io",
            "status": "aktuell",
            "modified_at": "2026-11-21T18:00:00+00:00",
            "age_days": 0.58,
            "max_age_days": 7,
        }
    },
    "lockfiles": ["Cargo.lock"],
    "packages_total": 3,
    "packages_checked": 3,
    "skipped": 1,
    "warnings": [],
    "untriaged": [{
        "ecosystem": "crates.io",
        "package": "demo-new",
        "crate": "demo-new",
        "version": "0.1.0",
        "advisory_ids": ["RUSTSEC-2099-0001"],
        "cve_ids": ["CVE-2099-0001"],
        "url": "https://osv.dev/vulnerability/RUSTSEC-2099-0001",
        "locations": ["Cargo.lock"],
    }],
    "expired": [{
        "ecosystem": "crates.io",
        "package": "demo-old",
        "crate": "demo-old",
        "version": "0.2.0",
        "advisory_ids": ["RUSTSEC-2099-0002"],
        "cve_ids": ["CVE-2099-0002"],
        "url": "https://osv.dev/vulnerability/RUSTSEC-2099-0002",
        "locations": ["Cargo.lock"],
        "triage_id": "RUSTSEC-2099-0002",
        "scope": "windows-build",
        "kind": "vulnerability",
        "expires": "2026-11-01",
        "reason": "Expired reason <script>alert(1)</script>",
    }],
    "triaged": [{
        "ecosystem": "crates.io",
        "package": "demo-triaged",
        "crate": "demo-triaged",
        "version": "0.3.0",
        "advisory_ids": ["RUSTSEC-2099-0003"],
        "cve_ids": [],
        "url": "https://osv.dev/vulnerability/RUSTSEC-2099-0003",
        "locations": ["Cargo.lock"],
        "triage_id": "RUSTSEC-2099-0003",
        "scope": "windows-build",
        "kind": "unmaintained",
        "expires": "2027-02-28",
        "reason": "Tracked transitive crate.",
    }],
    "unused_triage": [],
}


class ReportTests(unittest.TestCase):
    def test_sarif_210_structure_levels_and_suppressions(self):
        sarif = render_sarif(SAMPLE_REPORT)
        self.assertEqual(sarif["version"], SARIF_VERSION)
        self.assertEqual(len(sarif["runs"]), 1)
        run = sarif["runs"][0]
        driver = run["tool"]["driver"]
        self.assertEqual(driver["name"], "mogumogu-advisories")
        self.assertEqual(len(driver["rules"]), 3)
        by_rule = {r["ruleId"]: r for r in run["results"]}
        self.assertEqual(by_rule["RUSTSEC-2099-0001"]["level"], "error")
        self.assertNotIn("suppressions", by_rule["RUSTSEC-2099-0001"])
        self.assertEqual(by_rule["RUSTSEC-2099-0002"]["level"], "error")
        self.assertNotIn("suppressions", by_rule["RUSTSEC-2099-0002"])
        self.assertEqual(by_rule["RUSTSEC-2099-0003"]["level"], "note")
        self.assertEqual(by_rule["RUSTSEC-2099-0003"]["suppressions"][0]["status"], "accepted")
        # Fingerprints must be deterministic across repeated renders.
        sarif_again = render_sarif(SAMPLE_REPORT)
        self.assertEqual(
            [r["partialFingerprints"] for r in run["results"]],
            [r["partialFingerprints"] for r in sarif_again["runs"][0]["results"]],
        )

    def test_html_report_escapes_untrusted_text_and_writes_atomically(self):
        html = render_audit_html(SAMPLE_REPORT)
        self.assertIn("Content-Security-Policy", html)
        self.assertIn("https://www.cve.org/CVERecord?id=CVE-2099-0001", html)
        self.assertIn("&lt;script&gt;alert(1)&lt;/script&gt;", html)
        self.assertNotIn("<script>alert(1)</script>", html)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_sarif(root / "out.sarif", SAMPLE_REPORT)
            write_audit_html(root / "out.html", SAMPLE_REPORT)
            self.assertEqual(json.loads((root / "out.sarif").read_text(encoding="utf-8"))["version"], "2.1.0")
            self.assertIn("Handlungsbedarf", (root / "out.html").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
