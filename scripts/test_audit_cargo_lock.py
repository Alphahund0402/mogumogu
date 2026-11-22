"""Synthetic tests for Cargo.lock offline RustSec/CVE auditing and triage."""
from datetime import date
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile

import audit_cargo_lock as audit
import watch_advisories as watcher

SAMPLE_LOCK = """version = 4

[[package]]
name = "mogumogu"
version = "0.1.0"

[[package]]
name = "libsqlite3-sys"
version = "0.38.2"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "pin_utils"
version = "0.1.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
"""

SAMPLE_TRIAGE = """[[advisory]]
id = "RUSTSEC-2026-0459"
crate = "libsqlite3-sys"
version = "0.38.2"
scope = "windows-build"
kind = "vulnerability"
expires = "2026-12-31"
reason = "Pinned sqlite crate awaiting reviewed upgrade."

[[advisory]]
id = "RUSTSEC-2026-0441"
crate = "pin-utils"
version = "0.1.1"
scope = "windows-build"
kind = "unmaintained"
expires = "2026-11-01"
reason = "Transitive unmaintained dependency."
"""


def write_crates_feed(cache: Path, advisories: list[dict], mtime: float | None = None) -> Path:
    archive = watcher.db_path(cache, "crates.io")
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w") as out:
        for advisory in advisories:
            out.writestr(f"{advisory['id']}.json", json.dumps(advisory))
    if mtime is not None:
        os.utime(archive, (mtime, mtime))
    return archive


class CargoLockAuditTests(unittest.TestCase):
    def test_parse_cargo_lock_skips_local_package_and_normalizes_crates(self):
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "Cargo.lock"
            lock.write_text(SAMPLE_LOCK, encoding="utf-8")
            packages, non_registry, invalid = audit.parse_cargo_lock(lock)
        self.assertEqual(non_registry, 1)
        self.assertEqual(invalid, 0)
        self.assertEqual(packages, {
            watcher.Package("crates.io", "libsqlite3-sys", "0.38.2"),
            watcher.Package("crates.io", "pin-utils", "0.1.1"),
            watcher.Package("crates.io", "serde", "1.0.219"),
        })

    def test_standard_triage_beside_the_lockfile_is_used_unless_turned_off(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            lock = root / "Cargo.lock"
            lock.write_text(SAMPLE_LOCK, encoding="utf-8")
            self.assertIsNone(audit.resolve_triage(lock, None))
            standard = root / "security" / "advisory-triage.toml"
            standard.parent.mkdir()
            standard.write_text(SAMPLE_TRIAGE, encoding="utf-8")
            explicit = root / "other.toml"
            explicit.write_text(SAMPLE_TRIAGE, encoding="utf-8")
            self.assertEqual(audit.resolve_triage(lock, None), standard)
            self.assertEqual(audit.resolve_triage(lock, explicit), explicit)
            self.assertIsNone(audit.resolve_triage(lock, explicit, no_triage=True))

    def test_triage_distinguishes_active_expired_untriaged_and_unused(self):
        with tempfile.TemporaryDirectory() as directory:
            triage_path = Path(directory) / "triage.toml"
            triage_path.write_text(SAMPLE_TRIAGE, encoding="utf-8")
            entries = audit.load_triage(triage_path)
        sqlite = watcher.Package("crates.io", "libsqlite3-sys", "0.38.2")
        pin = watcher.Package("crates.io", "pin-utils", "0.1.1")
        serde = watcher.Package("crates.io", "serde", "1.0.219")
        matches = {
            sqlite: {watcher.Advisory(("GHSA-1", "RUSTSEC-2026-0459"), ("CVE-2099-0001",))},
            pin: {watcher.Advisory(("RUSTSEC-2026-0441",), ())},
            serde: {watcher.Advisory(("RUSTSEC-2099-9999",), ("CVE-2099-9999",))},
        }
        result = audit.evaluate_findings(matches, entries, date(2026, 11, 22))
        self.assertEqual(len(result["triaged"]), 1)
        self.assertEqual(result["triaged"][0]["triage_id"], "RUSTSEC-2026-0459")
        self.assertEqual(len(result["expired"]), 1)
        self.assertEqual(result["expired"][0]["triage_id"], "RUSTSEC-2026-0441")
        self.assertEqual(len(result["untriaged"]), 1)
        self.assertEqual(result["untriaged"][0]["crate"], "serde")
        self.assertEqual(result["unused_triage"], [])

        # Before expiry and without the untriaged crate, everything passes.
        clean = audit.evaluate_findings({sqlite: matches[sqlite]}, entries, date(2026, 10, 1))
        self.assertEqual(clean["untriaged"], [])
        self.assertEqual(clean["expired"], [])
        self.assertEqual(len(clean["triaged"]), 1)
        self.assertEqual(len(clean["unused_triage"]), 1)

    def test_stale_feed_fails_closed_unless_allowed(self):
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock = root / "Cargo.lock"
            lock.write_text(SAMPLE_LOCK, encoding="utf-8")
            write_crates_feed(root, [{"id": "RUSTSEC-2026-0001"}], mtime=now - 10 * watcher.DAY)
            packages, _, _ = audit.parse_cargo_lock(lock)
            with patch.object(watcher, "match", return_value={p: set() for p in packages}):
                code, report = audit.run_audit(lock, root, Path("scanner"), None, now=now)
                self.assertEqual(code, 2)
                self.assertFalse(report["complete"])
                self.assertEqual(report["feed"]["status"], "veraltet")

                code_allowed, report_allowed = audit.run_audit(
                    lock, root, Path("scanner"), None, allow_stale=True, now=now,
                )
                self.assertEqual(code_allowed, 0)
                self.assertTrue(report_allowed["complete"])

    def test_untriaged_or_expired_returns_exit_one(self):
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock = root / "Cargo.lock"
            lock.write_text(SAMPLE_LOCK, encoding="utf-8")
            triage = root / "triage.toml"
            triage.write_text(SAMPLE_TRIAGE, encoding="utf-8")
            write_crates_feed(root, [{"id": "RUSTSEC-2026-0459"}], mtime=now)
            sqlite = watcher.Package("crates.io", "libsqlite3-sys", "0.38.2")
            pin = watcher.Package("crates.io", "pin-utils", "0.1.1")
            serde = watcher.Package("crates.io", "serde", "1.0.219")
            mocked = {
                sqlite: {watcher.Advisory(("RUSTSEC-2026-0459",), ("CVE-2099-0001",))},
                pin: {watcher.Advisory(("RUSTSEC-2026-0441",), ())},
                serde: set(),
            }
            with patch.object(watcher, "match", return_value=mocked):
                code_ok, _ = audit.run_audit(lock, root, Path("scanner"), triage, today=date(2026, 10, 15), now=now)
                self.assertEqual(code_ok, 0)
                code_expired, report = audit.run_audit(
                    lock, root, Path("scanner"), triage, today=date(2026, 11, 22), now=now,
                )
                self.assertEqual(code_expired, 1)
                self.assertEqual(len(report["expired"]), 1)

    @unittest.skipUnless(os.getenv("MOGUMOGU_TEST_OSV_SCANNER"), "optional official OSV-Scanner binary")
    def test_official_scanner_offline_cargo_lock_audit(self):
        scanner = Path(os.environ["MOGUMOGU_TEST_OSV_SCANNER"]).resolve()
        now = time.time()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock = root / "Cargo.lock"
            lock.write_text(SAMPLE_LOCK, encoding="utf-8")
            triage = root / "triage.toml"
            triage.write_text(SAMPLE_TRIAGE, encoding="utf-8")
            write_crates_feed(
                root,
                [
                    {
                        "id": "RUSTSEC-2026-0459",
                        "aliases": ["CVE-2099-0459"],
                        "modified": "2026-11-21T00:00:00Z",
                        "affected": [{
                            "package": {"ecosystem": "crates.io", "name": "libsqlite3-sys"},
                            "versions": ["0.38.2"],
                        }],
                    },
                    {
                        "id": "RUSTSEC-2026-0441",
                        "modified": "2026-11-21T00:00:00Z",
                        "affected": [{
                            "package": {"ecosystem": "crates.io", "name": "pin-utils"},
                            "versions": ["0.1.1"],
                        }],
                    },
                ],
                mtime=now,
            )
            code_ok, report_ok = audit.run_audit(lock, root, scanner, triage, today=date(2026, 10, 15), now=now)
            self.assertEqual(code_ok, 0)
            self.assertEqual(len(report_ok["triaged"]), 2)
            self.assertEqual(report_ok["triaged"][0]["cve_ids"], ["CVE-2099-0459"])

            # Without triage file, both advisories fail the audit.
            code_fail, report_fail = audit.run_audit(lock, root, scanner, None, today=date(2026, 10, 15), now=now)
            self.assertEqual(code_fail, 1)
            self.assertEqual(len(report_fail["untriaged"]), 2)


if __name__ == "__main__":
    unittest.main()
