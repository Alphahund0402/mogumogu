#!/usr/bin/env python3
"""Static source/fixture/schema contracts. NOT a compiler and NOT a security proof.

Python 3.11+, standard library only. Reads only this repository.
Usage: python scripts/validate.py
"""
from contextlib import contextmanager
from pathlib import Path
import json
import re
import sqlite3
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = json.loads((ROOT / 'assets/demo.json').read_text(encoding='utf-8'))
MIGRATIONS = sorted((ROOT / 'migrations').glob('*.sql'))


def rust_sources():
    return {p: p.read_text(encoding='utf-8') for p in (ROOT / 'src').rglob('*.rs')}


@contextmanager
def database():
    """In-memory database with all migrations; always closed afterwards."""
    db = sqlite3.connect(':memory:')
    try:
        db.execute('PRAGMA foreign_keys=ON')
        for migration in MIGRATIONS:
            db.executescript(migration.read_text(encoding='utf-8'))
        yield db
    finally:
        db.close()


class Contracts(unittest.TestCase):
    def test_manifest_and_pinned_toolchain(self):
        manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))
        self.assertEqual(manifest['package']['license'], 'GPL-3.0-only')
        self.assertEqual(manifest['dependencies']['slint']['version'], '=1.18.1')
        self.assertEqual(manifest['dependencies']['rusqlite']['version'], '=0.40.2')
        self.assertIn('bundled', manifest['dependencies']['rusqlite']['features'])
        toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text(encoding='utf-8'))
        self.assertRegex(toolchain['toolchain']['channel'], r'^\d+\.\d+\.\d+$', 'toolchain pinned (F-03)')
        self.assertTrue((ROOT / 'Cargo.lock').is_file(), 'Cargo.lock checked in (F-03)')

    def test_schema_migrations_are_consistent(self):
        self.assertEqual([m.name[:3] for m in MIGRATIONS], [f'{i:03}' for i in range(1, len(MIGRATIONS) + 1)])
        with database() as db:
            self.assertEqual(db.execute('PRAGMA integrity_check').fetchone()[0], 'ok')
            self.assertEqual(db.execute('PRAGMA foreign_key_check').fetchall(), [])
            for state in ['deleted', 'safe-to-delete']:
                with self.assertRaises(sqlite3.IntegrityError):
                    db.execute("INSERT INTO cleanup_plans(state,created_at,rule_version,fingerprint) VALUES(?,0,'r','f')", (state,))

    def test_registration_defaults_unknown_and_protected(self):
        with database() as db:
            db.execute("INSERT INTO projects(name,path,origin) VALUES('Example','C:\\dev\\example','registered')")
            self.assertEqual(db.execute('SELECT state,bytes,packages,protected FROM projects').fetchone(), ('unknown', None, None, 1))
            db.execute("INSERT INTO resources(project_id,name,path,kind,evidence,origin) VALUES(1,'t','C:\\dev\\example\\t','temporary','x','registered')")
            self.assertEqual(db.execute('SELECT protected, expendable FROM resources').fetchone(), (1, 0))

    def test_demo_counts_and_totals(self):
        self.assertEqual(len(FIXTURE['projects']), 6)
        self.assertEqual(sum(p['packages'] for p in FIXTURE['projects']), 428)
        self.assertEqual(sum(r['bytes'] for r in FIXTURE['resources']), 142_000_000_000)
        self.assertEqual(sum(r['state'] == 'review' for r in FIXTURE['resources']), 3)
        self.assertTrue(all(x['protected'] for x in FIXTURE['resources'] + FIXTURE['projects']))

    def test_nine_adapters_without_cleanup_capability(self):
        sources = list((ROOT / 'src/adapters').glob('*.rs'))
        descriptors = [p.read_text(encoding='utf-8') for p in sources if 'static DESCRIPTOR' in p.read_text(encoding='utf-8')]
        self.assertEqual(len(descriptors), 9)
        for text in descriptors:
            self.assertIn('cleanup: Support::Unsupported', text)

    def test_ai_catalog_is_declarative_and_complete(self):
        catalog = tomllib.loads((ROOT / 'profiles/ai-catalog.toml').read_text(encoding='utf-8'))
        ids = [p['id'] for p in catalog['profile']]
        self.assertEqual(len(ids), len(set(ids)))
        for expected in ['agent-skills', 'codex', 'claude-code', 'copilot', 'cursor', 'gemini', 'windsurf',
                         'cline', 'roo', 'opencode', 'continue', 'aider']:
            self.assertIn(expected, ids)
        for profile in catalog['profile']:
            self.assertEqual(set(profile) - {'id', 'name', 'status', 'source', 'max_bytes', 'detect'}, set())
            for detection in profile['detect']:
                pattern = detection['pattern']
                self.assertNotIn('..', pattern)
                self.assertNotIn('**', pattern)
                self.assertFalse(pattern.startswith('/') or ':' in pattern or '\\' in pattern, pattern)

    def test_destructive_and_process_apis_stay_in_reviewed_modules(self):
        # Lexical allowlist: deletion only through the identity-checked
        # platform handle, processes only for managed runs and owner start.
        allowed = {
            'remove_dir_all(': set(), 'remove_file(': set(),
            'Command::new(': {'src/sessions/run.rs', 'src/bin/cli/main.rs'},
            'SetFileInformationByHandle': {'src/platform/windows.rs'},
            'unsafe ': {'src/platform/windows.rs'},
            'ureq::': {'src/updates/http.rs'},
            'reqwest': set(),
        }
        for path, text in rust_sources().items():
            rel = path.relative_to(ROOT).as_posix()
            code = '\n'.join(line for line in text.splitlines() if not line.strip().startswith('//'))
            if '#[cfg(test)]' in code:
                code = code[:code.index('#[cfg(test)]')]
            for needle, modules in allowed.items():
                if needle in code:
                    self.assertIn(rel, modules, f'{needle} in {rel}')

    def test_every_unsafe_block_is_documented(self):
        text = (ROOT / 'src/platform/windows.rs').read_text(encoding='utf-8')
        lines = text.splitlines()
        for number, line in enumerate(lines):
            if re.search(r'\bunsafe\s*\{', line) and 'fn ' not in line:
                window = '\n'.join(lines[max(0, number - 6):number + 1])
                self.assertIn('SAFETY:', window, f'line {number + 1}')

    def test_ui_assets_and_includes_resolve(self):
        for slint in (ROOT / 'ui').rglob('*.slint'):
            for name in re.findall(r'@image-url\("([^"]+)"\)', slint.read_text(encoding='utf-8')):
                self.assertTrue((slint.parent / name).resolve().is_file(), f'{slint.name}: {name}')
        for path, text in rust_sources().items():
            for name in re.findall(r'include_str!\("([^"]+)"\)', text):
                self.assertTrue((path.parent / name).resolve().is_file(), f'{path.name}: {name}')

    def test_skills_and_client_copies_do_not_drift(self):
        canonical = sorted((ROOT / '.agents/skills').glob('*/SKILL.md'))
        self.assertEqual(len(canonical), 6)
        for skill in canonical:
            text = skill.read_text(encoding='utf-8')
            self.assertTrue(text.startswith('---\n'))
            self.assertRegex(text, r'(?m)^name: [a-z0-9-]+$')
            self.assertRegex(text, r'(?m)^description: .+')
            copy = ROOT / '.claude/skills' / skill.parent.name / 'SKILL.md'
            self.assertTrue(copy.is_file(), f'missing copy {copy}')
            self.assertEqual(copy.read_text(encoding='utf-8'), text, f'{copy} differs; run scripts/sync_skills.py')

    def test_html_preview_is_independent_and_uses_the_fixture(self):
        html = (ROOT / 'design/dashboard.html').read_text(encoding='utf-8')
        self.assertNotRegex(html, r'<(?:script|link|img)[^>]+(?:src|href)=["\']https?://')
        self.assertNotRegex(html, r'\b(?:fetch|XMLHttpRequest|WebSocket)\s*\(')
        match = re.search(r'const FIXTURE=(.*?);\n', html, re.S)
        self.assertIsNotNone(match)
        self.assertEqual(json.loads(match.group(1)), FIXTURE)

    def test_no_fonts_binaries_or_databases_in_the_tree(self):
        for f in ROOT.rglob('*'):
            parts = f.relative_to(ROOT).parts
            if not f.is_file() or any(x in {'target', 'dist', '.git'} for x in parts):
                continue
            self.assertNotIn(f.suffix.lower(), {'.ttf', '.otf', '.woff', '.woff2', '.exe', '.dll', '.sqlite3', '.db'}, str(f))

    def test_plan_ids_and_documents(self):
        plan = (ROOT / 'docs/IMPLEMENTIERUNGSPLAN.md').read_text(encoding='utf-8')
        ids = re.findall(r'^\| (F-\d+) \|', plan, re.M)
        self.assertEqual(len(ids), 28)
        self.assertEqual(len(ids), len(set(ids)))
        for name in ['README.md', 'LICENSE', 'THIRD_PARTY.md', 'SECURITY.md', 'CONTRIBUTING.md', 'AGENTS.md', 'CLAUDE.md',
                     'docs/VALIDIERUNG.md', 'docs/WINDOWS-ABNAHME.md', 'docs/UNTERSTUETZUNGSMATRIX.md',
                     'docs/ANLEITUNG.md', 'build.ps1', 'start.ps1', 'build.cmd', 'scripts/measure.ps1',
                     'scripts/release.ps1']:
            self.assertTrue((ROOT / name).is_file(), name)
        for doc in ROOT.glob('*.md'):
            for target in re.findall(r'\]\(((?!https?://|#)[^)#]+)', doc.read_text(encoding='utf-8')):
                self.assertTrue((ROOT / target).exists(), f'{doc.name}: {target}')


if __name__ == '__main__':
    unittest.main(verbosity=2)
