"""Unit and offline OSV-Scanner integration tests for multi-ecosystem auditing."""
from datetime import date
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
import zipfile

from mogumogu_advisories import (
    audit_project,
    db_path,
    load_triage,
    write_audit_html,
    write_sarif,
)


def write_feed(cache: Path, ecosystem: str, advisories: list[dict], mtime: float) -> Path:
    archive = db_path(cache, ecosystem)
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w") as out:
        for advisory in advisories:
            out.writestr(f"{advisory['id']}.json", json.dumps(advisory))
    os.utime(archive, (mtime, mtime))
    return archive


class MultiEcosystemAuditTests(unittest.TestCase):
    def test_load_triage_supports_crate_shorthand_and_multi_ecosystem(self):
        toml_text = """
[[advisory]]
id = "RUSTSEC-2026-0459"
crate = "libsqlite3_sys"
version = "0.38.2"
scope = "windows-build"
kind = "vulnerability"
expires = "2026-12-31"
reason = "Pinned sqlite crate."

[[advisory]]
id = "GHSA-aaaa-bbbb-cccc"
ecosystem = "Packagist"
package = "Monolog/Monolog"
version = "v3.5.0"
expires = "2027-01-15"
reason = "Upgrade queued."
"""
        with tempfile.TemporaryDirectory() as directory:
            triage_path = Path(directory) / "triage.toml"
            triage_path.write_text(toml_text, encoding="utf-8")
            entries = load_triage(triage_path)
        self.assertEqual(len(entries), 2)
        self.assertEqual((entries[0].ecosystem, entries[0].package, entries[0].crate), ("crates.io", "libsqlite3-sys", "libsqlite3-sys"))
        self.assertEqual((entries[1].ecosystem, entries[1].package, entries[1].version), ("Packagist", "monolog/monolog", "3.5.0"))

    def test_empty_project_fails_closed_with_exit_two(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            code, report = audit_project(root, cache=root / "cache", scanner=Path("scanner"))
        self.assertEqual(code, 2)
        self.assertFalse(report["complete"])
        self.assertEqual(report["packages_total"], 0)

    def test_missing_feed_fails_closed_while_reporting_partial_matches(self):
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text(
                '[[package]]\nname = "serde"\nversion = "1.0.219"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
                encoding="utf-8",
            )
            (root / "package-lock.json").write_text(
                json.dumps({"packages": {"node_modules/lodash": {"version": "4.17.20"}}}),
                encoding="utf-8",
            )
            cache = root / "cache"
            # Only crates.io feed is present; npm feed is missing.
            write_feed(cache, "crates.io", [{"id": "RUSTSEC-2026-0001"}], now)

            def fake_match(_scanner, _cache, ready):
                return {pkg: set() for pkg in ready}

            code, report = audit_project(root, cache=cache, scanner=Path("scanner"), now=now, matcher=fake_match)
        self.assertEqual(code, 2)
        self.assertFalse(report["complete"])
        self.assertEqual(report["feeds"]["crates.io"]["status"], "aktuell")
        self.assertEqual(report["feeds"]["npm"]["status"], "fehlt")
        self.assertEqual(report["packages_checked"], 1)
        self.assertEqual(report["packages_total"], 2)

    @unittest.skipUnless(os.getenv("MOGUMOGU_TEST_OSV_SCANNER"), "optional official OSV-Scanner binary")
    def test_official_scanner_offline_polyglot_project_with_triage_sarif_and_html(self):
        scanner = Path(os.environ["MOGUMOGU_TEST_OSV_SCANNER"]).resolve()
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "repo"
            (project / "web").mkdir(parents=True)
            (project / "Cargo.lock").write_text(
                '[[package]]\nname = "demo-crate"\nversion = "1.0.0"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
                encoding="utf-8",
            )
            (project / "web" / "package-lock.json").write_text(
                json.dumps({"packages": {"node_modules/demo-js": {"version": "2.0.0"}}}),
                encoding="utf-8",
            )
            (project / "composer.lock").write_text(
                json.dumps({
                    "packages": [{
                        "name": "demo/php", "version": "v3.5.0",
                        "notification-url": "https://packagist.org/downloads/",
                    }],
                    "packages-dev": [],
                }),
                encoding="utf-8",
            )
            cache = root / "cache"
            write_feed(cache, "crates.io", [{
                "id": "RUSTSEC-2099-0001", "aliases": ["CVE-2099-1001"], "modified": "2026-11-21T00:00:00Z",
                "affected": [{"package": {"ecosystem": "crates.io", "name": "demo-crate"}, "versions": ["1.0.0"]}],
            }], now)
            write_feed(cache, "npm", [{
                "id": "GHSA-2099-aaaa-bbbb", "aliases": ["CVE-2099-1002"], "modified": "2026-11-21T00:00:00Z",
                "affected": [{"package": {"ecosystem": "npm", "name": "demo-js"}, "versions": ["2.0.0"]}],
            }], now)
            write_feed(cache, "Packagist", [{
                "id": "GHSA-2099-cccc-dddd", "aliases": ["CVE-2099-1003"], "modified": "2026-11-21T00:00:00Z",
                "affected": [{
                    "package": {"ecosystem": "Packagist", "name": "demo/php"},
                    "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "4.0.0"}]}],
                }],
            }], now)

            triage = root / "triage.toml"
            triage.write_text(
                '[[advisory]]\nid = "RUSTSEC-2099-0001"\ncrate = "demo-crate"\nversion = "1.0.0"\n'
                'expires = "2027-01-01"\nreason = "Scheduled upgrade."\n',
                encoding="utf-8",
            )
            code, report = audit_project(
                project, cache=cache, scanner=scanner, triage_path=triage, today=date(2026, 11, 22), now=now,
            )
            self.assertEqual(code, 1)
            self.assertTrue(report["complete"])
            self.assertEqual(len(report["triaged"]), 1)
            self.assertEqual(len(report["untriaged"]), 2)

            sarif_path = root / "audit.sarif"
            html_path = root / "audit.html"
            write_sarif(sarif_path, report)
            write_audit_html(html_path, report)

            sarif = json.loads(sarif_path.read_text(encoding="utf-8"))
            self.assertEqual(sarif["version"], "2.1.0")
            results = sarif["runs"][0]["results"]
            self.assertEqual(len(results), 3)
            by_rule = {r["ruleId"]: r for r in results}
            self.assertEqual(by_rule["GHSA-2099-aaaa-bbbb"]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"], "web/package-lock.json")
            self.assertIn("suppressions", by_rule["RUSTSEC-2099-0001"])
            self.assertNotIn("suppressions", by_rule["GHSA-2099-cccc-dddd"])

            html = html_path.read_text(encoding="utf-8")
            self.assertIn("CVE-2099-1001", html)
            self.assertIn("CVE-2099-1002", html)
            self.assertIn("CVE-2099-1003", html)
            self.assertIn("web/package-lock.json", html)


if __name__ == "__main__":
    unittest.main()
