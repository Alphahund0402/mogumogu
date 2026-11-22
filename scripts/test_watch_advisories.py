"""Synthetic advisory fixtures. Tests leave real installations and data untouched."""
from contextlib import redirect_stdout
from io import BytesIO, StringIO
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile

import watch_advisories as watcher


class AdvisoryWatcherTests(unittest.TestCase):
    def test_exact_identity_and_scope_evidence(self):
        self.assertIsNone(watcher.Package.parse({"ecosystem": "npm", "name": "@scope/package", "version": "1.x"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "npm", "name": "pkg", "version": "1.0.0-01"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "Packagist", "name": "monolog/monolog", "version": "dev-main"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "Packagist", "name": "monolog/monolog", "version": "1.0.0-dev"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "Packagist", "name": "php", "version": "8.3.0"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "CRAN", "name": "1invalid", "version": "1.0"}))
        self.assertIsNone(watcher.Package.parse({"ecosystem": "CRAN", "name": "Rcpp", "version": "1"}))
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "Packagist", "name": "Monolog/Monolog", "version": "v3.5.0"}),
            watcher.Package("Packagist", "monolog/monolog", "3.5.0"),
        )
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "RubyGems", "name": "RedCloth", "version": "4.3.4"}),
            watcher.Package("RubyGems", "RedCloth", "4.3.4"),
        )
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "Pub", "name": "http", "version": "0.11.0+1"}),
            watcher.Package("Pub", "http", "0.11.0+1"),
        )
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "Hex", "name": "phoenix_ecto", "version": "4.4.3"}),
            watcher.Package("Hex", "phoenix_ecto", "4.4.3"),
        )
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "CRAN", "name": "Rcpp", "version": "1.0-12"}),
            watcher.Package("CRAN", "Rcpp", "1.0-12"),
        )
        self.assertEqual(
            watcher.Package.parse({"ecosystem": "Julia", "name": "Bzip2_jll", "version": "1.0.8"}),
            watcher.Package("Julia", "Bzip2_jll", "1.0.8"),
        )
        rows = [
            {"ecosystem": "PyPI", "name": "My_Package", "version": "1.0", "scope_id": 1, "public_source": True},
            {"ecosystem": "PyPI", "name": "my.package", "version": "1.0", "scope_id": 2, "public_source": False},
        ]
        page = {
            "basis": "fixture", "packages": rows, "skipped": 1, "current_scopes": 2, "incomplete_scopes": 0,
            "oldest_scan_at": 1, "scope_limit_reached": False, "item_limit_reached": False, "next_offset": None,
        }
        response = SimpleNamespace(returncode=0, stdout=json.dumps({"v": 1, "ok": True, "result": page}))
        with patch.object(watcher.subprocess, "run", return_value=response) as run:
            packages, coverage = watcher.inventory(Path("mogumogu-cli.exe"), None)
        self.assertEqual(packages, {
            watcher.Package("PyPI", "my-package", "1.0"): {
                watcher.Evidence(1, "installed", True), watcher.Evidence(2, "installed", False),
            },
        })
        self.assertEqual(coverage["skipped"], 1)
        self.assertIn("--no-start", run.call_args.args[0])

    def test_only_matching_active_advisories_make_it_into_findings(self):
        package = watcher.Package("npm", "@fixture/test", "1.0.0")
        row = {
            "package": package.fields(),
            "vulnerabilities": [
                {"id": "MOCK-1", "aliases": ["CVE-2099-0001"]},
                {"id": "MOCK-2", "aliases": ["CVE-2099-0001"]},
                {"id": "MOCK-3", "aliases": ["CVE-2099-0002"], "withdrawn": "2026-11-20"},
            ],
            "groups": [{"ids": ["MOCK-1", "MOCK-2", "MOCK-3"]}],
        }
        results = {"results": [{"packages": [row, row]}]}
        self.assertEqual(watcher.read_matches(results, {package}), {
            package: {watcher.Advisory(("MOCK-1", "MOCK-2"), ("CVE-2099-0001",))},
        })
        self.assertEqual(watcher.read_matches(results, set()), {})

    def test_matching_requires_a_usable_feed_and_keeps_stale_matches(self):
        npm = watcher.Package("npm", "fixture", "1.0.0")
        python = watcher.Package("PyPI", "fixture", "1.0")
        now = time.time()
        coverage = {
            "oldest_scan_at": now, "incomplete_scopes": 0, "current_scopes": 2, "skipped": 0,
            "item_limit_reached": False, "scope_limit_reached": False,
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archived = watcher.db_path(root, "npm")
            archived.parent.mkdir(parents=True)
            with zipfile.ZipFile(archived, "w") as output:
                output.writestr("MOCK-1.json", "{}")
            os.utime(archived, (now - 9 * watcher.DAY, now - 9 * watcher.DAY))
            args = SimpleNamespace(cli=None, scanner=None, cache=root, refresh=False, tools=None, data_dir=None,
                                   projects=False, project=None, scan=False)
            packages = {npm: {watcher.Evidence(1, "installed", True)}, python: {watcher.Evidence(2, "installed", False)}}
            with (
                patch.object(watcher, "inventory", return_value=(packages, coverage)),
                patch.object(watcher, "match", return_value={npm: {watcher.Advisory(("MOCK-1",))}}) as match,
            ):
                report = watcher.check(args)
        match.assert_called_once_with(None, root, {npm})
        self.assertEqual(report["feeds"], {"PyPI": "fehlt", "npm": "veraltet"})
        self.assertTrue(report["partial"])
        self.assertEqual(report["checked"], 1)
        self.assertEqual(report["findings"][0]["public_scope_ids"], [1])

    def test_feed_download_keeps_prior_copy_on_failure(self):
        url = "https://osv-vulnerabilities.storage.googleapis.com/npm/all.zip"
        with tempfile.TemporaryDirectory() as directory:
            saved = Path(directory) / "all.zip"
            saved.write_bytes(b"previous fixture")
            bad = BytesIO(b"not a ZIP")
            bad.geturl = lambda: url
            bad.headers = {}
            with patch.object(watcher, "urlopen", return_value=bad) as request:
                with self.assertRaises(zipfile.BadZipFile):
                    watcher.download_feed(saved, "npm")
            self.assertEqual(saved.read_bytes(), b"previous fixture")
            request.assert_called_once_with(url, timeout=15)

    def test_watch_emits_changes_without_repeating_the_same_report(self):
        first = {"checked": 1, "partial": False, "findings": []}
        changed = {**first, "findings": [{"advisory_ids": ["MOCK-1"]}]}
        args = ["watch_advisories.py", "watch", "--cache", ".", "--cli", "cli", "--scanner", "scanner"]
        output = StringIO()
        with (
            patch.object(watcher.sys, "argv", args),
            patch.object(watcher, "executable", Path),
            patch.object(watcher, "report", side_effect=[first, first, changed]),
            patch.object(watcher.time, "sleep", side_effect=[None, None, KeyboardInterrupt()]),
            redirect_stdout(output),
            self.assertRaises(KeyboardInterrupt),
        ):
            watcher.main()
        reports = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual([report["findings"] for report in reports], [[], changed["findings"]])

    def test_rescan_only_uses_granted_project_scopes_and_honors_cancel(self):
        selection = ["projects"]
        projects = [
            {"id": 1, "scope_id": None, "scan_allowed": False},
            {"id": 2, "scope_id": 5, "scan_allowed": True},
            {"id": 3, "scope_id": 6, "scan_allowed": True},
            {"id": 4, "scope_id": 7, "scan_allowed": True},
        ]
        replies = [{"projects": projects}, watcher.WatchError("fixture offline"), [{"cancelled": True}]]
        with patch.object(watcher, "cli_call", side_effect=replies) as call:
            self.assertEqual(watcher.scan_projects(None, None, selection), ([2], True))
        self.assertEqual([c.args[-1] for c in call.call_args_list], [
            ["advisories", "projects"], ["scan", "--scope", "5"], ["scan", "--scope", "6"],
        ])

    def test_project_html_has_cve_links_and_visible_gaps(self):
        package = watcher.Package("npm", "fixture", "1.0.0")
        now = time.time()
        projects = [
            {"id": 1, "name": "<fixture>", "scope_id": 7, "scan_state": "complete", "scan_finished_at": now},
            {"id": 2, "name": "Unapproved", "scope_id": None, "scan_state": "not_approved", "scan_finished_at": None},
        ]
        coverage = {
            "current_scopes": 1, "incomplete_scopes": 0, "oldest_scan_at": now, "scope_limit_reached": False,
            "item_limit_reached": False, "skipped": 2, "projects": projects, "skipped_by_scope": {7: 2},
        }
        evidence = {watcher.Evidence(7, "resolved", True), watcher.Evidence(7, "installed", False)}
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(cli=None, scanner=None, cache=Path(directory), refresh=False, tools=None,
                                   data_dir=None, projects=True, project=None, scan=False)
            with (
                patch.object(watcher, "inventory", return_value=({package: evidence}, coverage)),
                patch.object(watcher, "feed_status", return_value="aktuell"),
                patch.object(watcher, "match", return_value={package: {watcher.Advisory(("MOCK-1",), ("CVE-2099-0001",))}}),
            ):
                result = watcher.check(args)
            path = Path(directory) / "result.html"
            watcher.write_html(path, {"checked_at": "fixture-time", **result})
            html = path.read_text(encoding="utf-8")
        self.assertTrue(result["partial"])
        self.assertEqual(result["projects"][0]["checked"], 1)
        self.assertEqual(result["projects"][0]["skipped"], 2)
        self.assertIn("&lt;fixture&gt;", html)
        self.assertNotIn("<fixture>", html)
        self.assertIn("https://www.cve.org/CVERecord?id=CVE-2099-0001", html)
        self.assertIn("Aufgelöst", html)
        self.assertIn("Installiert", html)
        self.assertIn("Lesefreigabe ausstehend", html)

    @unittest.skipUnless(os.getenv("MOGUMOGU_TEST_OSV_SCANNER"), "optional official OSV-Scanner binary")
    def test_official_scanner_offline_with_synthetic_advisory_databases(self):
        scanner = Path(os.environ["MOGUMOGU_TEST_OSV_SCANNER"]).resolve()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packages = {
                watcher.Package("crates.io", "fixture-crate", "1.0.0"),
                watcher.Package("Go", "stdlib", "1.24.1"),
                watcher.Package("Go", "github.com/example/Fixture", "1.0.0"),
                watcher.Package("npm", "@fixture/test", "1.0.0"),
                watcher.Package("PyPI", "fixture-package", "1.0"),
                watcher.Package("NuGet", "fixture.package", "1.0.0"),
                watcher.Package("Maven", "com.example:fixture", "1.0.RELEASE"),
                watcher.Package("Packagist", "fixture/pkg", "3.5.0"),
                watcher.Package("RubyGems", "FixtureGem", "2.1.0"),
                watcher.Package("Pub", "fixture_pub", "0.11.0+1"),
                watcher.Package("Hex", "fixture_hex", "1.7.10"),
                watcher.Package("CRAN", "FixtureR", "1.2-3"),
                watcher.Package("Julia", "FixtureJL", "1.0.8"),
            }
            fixed = watcher.Package("npm", "@fixture/test", "2.0.0")
            fixed_packagist = watcher.Package("Packagist", "fixture/pkg", "4.0.0")
            expected = {fixed: set(), fixed_packagist: set()}
            for index, package in enumerate(sorted(packages)):
                advisory_id = f"MOGUMOGU-TEST-{index}"
                cve_id = f"CVE-2099-{index + 1000}"
                expected[package] = {watcher.Advisory((advisory_id,), (cve_id,))}
                database = watcher.db_path(root, package.ecosystem)
                database.parent.mkdir(parents=True, exist_ok=True)
                advisory = {
                    "id": advisory_id, "aliases": [cve_id], "modified": "2026-11-21T00:00:00Z",
                    "affected": [{
                        "package": {"ecosystem": package.ecosystem, "name": package.name},
                        "versions": [package.version],
                    }],
                }
                if package.ecosystem == "npm":
                    advisory["affected"][0]["ranges"] = [{
                        "type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": fixed.version}],
                    }]
                elif package.ecosystem == "Packagist":
                    advisory["affected"][0]["ranges"] = [{
                        "type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": fixed_packagist.version}],
                    }]
                with zipfile.ZipFile(database, "a") as output:
                    output.writestr(f"{advisory_id}.json", json.dumps(advisory))
            matches = watcher.match(scanner, root, packages | {fixed, fixed_packagist})
            self.assertEqual(matches, expected)

            # End-to-end offline check() + HTML report pipeline using exported JSON shape.
            now = time.time()
            page = {
                "basis": "e2e-basis",
                "current_scopes": 1,
                "incomplete_scopes": 0,
                "oldest_scan_at": now,
                "scope_limit_reached": False,
                "item_limit_reached": False,
                "skipped": 1,
                "skipped_by_scope": {"10": 1},
                "next_offset": None,
                "projects": [{
                    "id": 1, "name": "polyglot-app", "scope_id": 10,
                    "scan_allowed": True, "scan_state": "complete", "scan_finished_at": now,
                }],
                "packages": [
                    {**package.fields(), "scope_id": 10, "public_source": True, "state": "resolved"}
                    for package in sorted(packages | {fixed, fixed_packagist})
                ],
            }
            args = SimpleNamespace(
                cli=Path("mogumogu-cli"), scanner=scanner, cache=root, refresh=False, tools=None,
                data_dir=None, projects=True, project=None, scan=False,
            )
            with patch.object(watcher, "cli_call", return_value=page):
                result = watcher.check(args)
            self.assertEqual(result["candidates"], len(packages) + 2)
            self.assertEqual(result["checked"], len(packages) + 2)
            self.assertEqual(len(result["findings"]), len(packages))
            self.assertTrue(result["partial"], "skipped item keeps partial flag visible")
            html_path = root / "e2e-report.html"
            watcher.write_html(html_path, {"checked_at": "2026-11-22T00:00:00+00:00", **result})
            html = html_path.read_text(encoding="utf-8")
            self.assertIn("polyglot-app", html)
            self.assertIn("1 Belege übersprungen", html)
            for index in range(len(packages)):
                self.assertIn(f"CVE-2099-{index + 1000}", html)


if __name__ == "__main__":
    unittest.main()
