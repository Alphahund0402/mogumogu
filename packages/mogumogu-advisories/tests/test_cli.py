"""Tests for simplified CLI defaults, auto-triage discovery, and --out bundles."""
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile

from mogumogu_advisories import (
    cli,
    db_path,
    default_cache_dir,
    default_scanner_path,
    discover_triage,
)


class CliDefaultsTests(unittest.TestCase):
    def test_env_defaults_and_triage_discovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            with patch.dict(os.environ, {
                "MOGUMOGU_OSV_CACHE": str(root / "cache"),
                "MOGUMOGU_OSV_SCANNER": str(root / "bin" / "osv-scanner"),
            }):
                self.assertEqual(default_cache_dir(), root / "cache")
                self.assertEqual(default_scanner_path(root / "other"), root / "bin" / "osv-scanner")

            self.assertIsNone(discover_triage(root))
            root_triage = root / "advisory-triage.toml"
            root_triage.write_text("", encoding="utf-8")
            self.assertEqual(discover_triage(root), root_triage)
            sec_triage = root / "security" / "advisory-triage.toml"
            sec_triage.parent.mkdir()
            sec_triage.write_text("", encoding="utf-8")
            self.assertEqual(discover_triage(root), sec_triage)

    def test_missing_scanner_fails_closed_with_helpful_hint(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            stderr = StringIO()
            with (
                patch.dict(os.environ, {"MOGUMOGU_OSV_CACHE": str(root / "cache")}, clear=False),
                patch.dict(os.environ, {}, clear=False) as env,
                redirect_stderr(stderr),
            ):
                env.pop("MOGUMOGU_OSV_SCANNER", None)
                code = cli.main(["audit", str(root)])
        self.assertEqual(code, 2)
        self.assertIn("--update", stderr.getvalue())

    @unittest.skipUnless(os.getenv("MOGUMOGU_TEST_OSV_SCANNER"), "optional official OSV-Scanner binary")
    def test_minimal_cli_audit_with_env_defaults_auto_triage_and_out_dir(self):
        scanner = Path(os.environ["MOGUMOGU_TEST_OSV_SCANNER"]).resolve()
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            repo = root / "project"
            (repo / "security").mkdir(parents=True)
            (repo / "Cargo.lock").write_text(
                '[[package]]\nname = "demo-crate"\nversion = "1.0.0"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
                encoding="utf-8",
            )
            (repo / "security" / "advisory-triage.toml").write_text(
                '[[advisory]]\nid = "RUSTSEC-2099-0001"\ncrate = "demo-crate"\nversion = "1.0.0"\n'
                'expires = "2099-12-31"\nreason = "Auto-discovered triage."\n',
                encoding="utf-8",
            )
            cache = root / "cache"
            archive = db_path(cache, "crates.io")
            archive.parent.mkdir(parents=True)
            with zipfile.ZipFile(archive, "w") as out:
                out.writestr("RUSTSEC-2099-0001.json", json.dumps({
                    "id": "RUSTSEC-2099-0001", "aliases": ["CVE-2099-4242"], "modified": "2026-11-21T00:00:00Z",
                    "affected": [{"package": {"ecosystem": "crates.io", "name": "demo-crate"}, "versions": ["1.0.0"]}],
                }))
            os.utime(archive, (now, now))
            out_dir = root / "reports"
            stdout = StringIO()
            with (
                patch.dict(os.environ, {
                    "MOGUMOGU_OSV_CACHE": str(cache),
                    "MOGUMOGU_OSV_SCANNER": str(scanner),
                }),
                redirect_stdout(stdout),
            ):
                code = cli.main(["audit", str(repo), "--out", str(out_dir)])
            self.assertEqual(code, 0)
            self.assertIn("security/advisory-triage.toml", stdout.getvalue())
            self.assertTrue((out_dir / "audit.json").is_file())
            self.assertTrue((out_dir / "audit.html").is_file())
            self.assertTrue((out_dir / "audit.sarif").is_file())
            sarif = json.loads((out_dir / "audit.sarif").read_text(encoding="utf-8"))
            self.assertEqual(sarif["version"], "2.1.0")
            self.assertEqual(len(sarif["runs"][0]["results"]), 1)
            self.assertIn("suppressions", sarif["runs"][0]["results"][0])


if __name__ == "__main__":
    unittest.main()
