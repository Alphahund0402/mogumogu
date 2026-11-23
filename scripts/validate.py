#!/usr/bin/env python3
"""Static source/fixture/schema contracts. NOT a compiler and NOT a security proof.

Python 3.11+, standard library only. Reads only this repository.
Usage: python scripts/validate.py
"""
import ast
from contextlib import contextmanager
from pathlib import Path
import hashlib
import json
import os
import re
import sqlite3
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = json.loads((ROOT / 'assets/demo.json').read_text(encoding='utf-8'))
MIGRATIONS = sorted((ROOT / 'migrations').glob('*.sql'))
ADVISORIES = ROOT / 'packages/mogumogu-advisories'


def python_effects(path):
    """Imported modules, called names and whether any call passes `shell=`; parsed, never imported."""
    tree = ast.parse(path.read_text(encoding='utf-8'), filename=str(path))
    imports, calls, shell = set(), set(), False
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            imports.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            imports.add(node.module)
            imports.update(f'{node.module}.{alias.name}' for alias in node.names)
        elif isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name):
                calls.add(node.func.id)
            elif isinstance(node.func, ast.Attribute) and isinstance(node.func.value, ast.Name):
                calls.add(f'{node.func.value.id}.{node.func.attr}')
            shell = shell or any(keyword.arg == 'shell' for keyword in node.keywords)
    return imports, calls, shell


def rust_sources():
    return {p: p.read_text(encoding='utf-8') for p in (ROOT / 'src').rglob('*.rs')}


def _after_literal(code, i):
    """Index after a comment, string, raw string or char literal starting at `i`, else None."""
    if code[i] not in '/"\'rb':
        return None
    if code.startswith('//', i):
        end = code.find('\n', i)
        return len(code) if end < 0 else end
    if code.startswith('/*', i):
        depth, j = 0, i
        while j < len(code):
            if code.startswith('/*', j):
                depth, j = depth + 1, j + 2
            elif code.startswith('*/', j):
                depth, j = depth - 1, j + 2
                if depth == 0:
                    return j
            else:
                j += 1
        raise AssertionError('unterminated block comment')
    if i and (code[i - 1].isalnum() or code[i - 1] == '_'):
        return None
    raw = re.match(r'b?r(#*)"', code[i:i + 300])
    if raw:
        end = code.find('"' + raw.group(1), i + raw.end())
        if end < 0:
            raise AssertionError('unterminated raw string')
        return end + 1 + len(raw.group(1))
    j = i + 1 if code.startswith('b', i) and code[i + 1:i + 2] in ('"', "'") else i
    if code[j] == '"':
        j += 1
        while code[j] != '"':
            j += 2 if code[j] == '\\' else 1
        return j + 1
    if code[j] == "'":
        if code[j + 1] == '\\':
            return code.index("'", j + 3) + 1
        if code[j + 2:j + 3] == "'":
            return j + 3
    return None  # a lifetime or plain code


def _item_end(code, start):
    """End of the item that the attribute at `start` belongs to: its closing
    brace, or the semicolon of an item without a body."""
    depth, i = 0, start
    while i < len(code):
        after = _after_literal(code, i)
        if after is not None:
            i = after
            continue
        char = code[i]
        if char in '([{':
            depth += 1
        elif char in ')]}':
            depth -= 1
            if char == '}' and depth == 0:
                return i + 1
        elif char == ';' and depth == 0:
            return i + 1
        i += 1
    raise AssertionError(f'end of the cfg(test) item at offset {start} not found')


def production_rust(text):
    """Lexical checks skip line comments and every item compiled only for tests
    (`#[cfg(test)]`, `#[cfg(all(test, …))]`), wherever it sits in the file."""
    code = '\n'.join(line for line in text.splitlines() if not line.strip().startswith('//'))
    kept, i = [], 0
    for marker in re.finditer(r'(?m)^[ \t]*#\[cfg\((?:all\()?test\b[^\]]*\]', code):
        if marker.start() < i:
            continue  # inside an item already skipped
        kept.append(code[i:marker.start()])
        i = _item_end(code, marker.end())
    kept.append(code[i:])
    return ''.join(kept)


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
        self.assertEqual(manifest['dependencies']['rusqlite']['version'], '=0.41.0')
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

    def test_forty_adapters_without_cleanup_capability(self):
        sources = list((ROOT / 'src/adapters').glob('*.rs'))
        descriptors = [p.read_text(encoding='utf-8') for p in sources if 'static DESCRIPTOR' in p.read_text(encoding='utf-8')]
        self.assertEqual(len(descriptors), 40)
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
        # Lexical allowlist: nothing is deleted directly; cleanup only moves
        # verified folders to the recycle bin, and only the executor asks for
        # it. No program is started besides the owner (in `platform`).
        allowed = {
            'remove_dir_all(': set(), 'remove_file(': set(), 'remove_dir(': set(),
            'Command::new(': set(),
            # Rename and discard of the one temporary file `replace_file` creates.
            'SetFileInformationByHandle': {'src/platform/windows.rs'},
            # The recycle bin: the shell's file operation with the sink that
            # refuses deleting for good (`platform::recycle`), nothing else.
            'FileOperation': {'src/platform/windows.rs'}, 'SHCreateItemFromParsingName': {'src/platform/windows.rs'},
            'CoCreateInstance': {'src/platform/windows.rs'}, 'perform_operations': {'src/platform/windows.rs'},
            'SHFileOperation': set(),
            'platform::recycle(': {'src/cleanup/executor.rs'},
            # Real places of holds: metadata only, and only to block.
            'real_location(': {'src/platform/windows.rs', 'src/platform/fallback.rs', 'src/platform/mod.rs'},
            'real_place(': {'src/platform/mod.rs', 'src/cleanup/holds.rs', 'src/docker/mod.rs'},
            # Calls to `recycle` by any other name or import.
            'recycle(': {'src/cleanup/executor.rs', 'src/platform/windows.rs', 'src/platform/fallback.rs'},
            # `DeleteFile` only as the disposition field that discards that temporary file.
            'DeleteFile': {'src/platform/windows.rs'}, 'RemoveDirectory': set(), 'MoveFile': set(), 'ReplaceFile': set(),
            'FILE_DELETE_ON_CLOSE': set(),
            'FileDispositionInfo': {'src/platform/windows.rs'}, 'FileRenameInfo': {'src/platform/windows.rs'},
            'NtSetInformationFile': {'src/platform/windows.rs'},
            'fs::remove': set(),
            'unsafe ': {'src/platform/windows.rs'},
            'ureq::': set(),
            # The only write into a user's project: the confirmed context filter,
            # through the held `.claude` handle. Everything else writes only the
            # app's own data directory.
            'replace_file(': {'src/service/filters.rs', 'src/platform/windows.rs', 'src/platform/fallback.rs'},
            'create_dir_pinned(': {'src/service/filters.rs', 'src/platform/windows.rs', 'src/platform/fallback.rs'},
            'fs::rename(': set(), 'fs::write(': set(), 'File::create(': set(), 'fs::copy(': set(),
            'create_dir_all(': {'src/main.rs', 'src/storage/mod.rs', 'src/storage/migrations.rs'},
            'OpenOptions': {'src/main.rs'},
            'reqwest': set(),
        }
        for path, text in rust_sources().items():
            rel = path.relative_to(ROOT).as_posix()
            code = production_rust(text)
            for needle, modules in allowed.items():
                if needle in code:
                    self.assertIn(rel, modules, f'{needle} in {rel}')
        # Never deleted for good: no dialog that offers it, a sink that refuses
        # every item Windows would not recycle, attached before anything runs.
        windows = production_rust((ROOT / 'src/platform/windows.rs').read_text(encoding='utf-8'))
        self.assertNotIn('FOF_WANTNUKEWARNING', windows)
        self.assertIn('TSF_DELETE_RECYCLE_IF_POSSIBLE', windows)
        self.assertLess(windows.index('(vtbl.advise)'), windows.index('(vtbl.perform_operations)'))

    def test_domain_and_presentation_stay_independent_of_io_and_storage(self):
        # Dependency checks complement the destructive-API allowlist. They
        # are deliberately lexical, not a Rust parser or a security proof.
        io = r'\bstd::(?:\{[^}]*\b)?(?:fs|io|net|process)\b'
        domain_io = r'\bcrate::(?:\{[^}]*\b)?(?:cleanup|config|fsread|inventory|ipc|owner|platform|profiles|service|storage)\b'
        presentation_io = r'\bmogumogu::(?:\{[^}]*\b)?(?:cleanup|fsread|inventory|storage)\b'
        for path, text in rust_sources().items():
            rel = path.relative_to(ROOT).as_posix()
            code = production_rust(text)
            with self.subTest(module=rel):
                if not rel.startswith('src/storage/'):
                    sqlite = code
                    if rel == 'src/domain/mod.rs':
                        sqlite = re.sub(r'\brusqlite::(?:types::\w+|Result)\b', '', sqlite)
                    elif rel == 'src/error.rs':
                        sqlite = re.sub(r'\brusqlite::Error\b', '', sqlite)
                    self.assertNotRegex(sqlite, r'\brusqlite\b', 'SQLite stays in storage')
                if rel.startswith('src/domain/'):
                    self.assertNotRegex(code, io, 'domain has no I/O')
                    self.assertNotRegex(code, domain_io, 'domain cannot depend on application modules')
                if rel.startswith('src/desktop/'):
                    self.assertNotRegex(code, io, 'presentation has no direct I/O')
                    self.assertNotRegex(code, presentation_io, 'presentation calls use cases through service')

    def test_docker_and_runs_stay_read_only(self):
        # The engine pipe is used by the one GET client; no other method, no
        # path that changes Docker, and no SQL that removes runs or artifacts.
        for path, text in rust_sources().items():
            rel = path.relative_to(ROOT).as_posix()
            code = production_rust(text)
            if 'EnginePipe' in code:
                self.assertIn(rel, {'src/docker/http.rs', 'src/platform/windows.rs', 'src/platform/fallback.rs'})
            if rel.startswith('src/docker/'):
                for method in ['"POST', '"PUT', '"DELETE', '"PATCH', '"HEAD', 'POST ', 'PUT ', 'DELETE ', 'PATCH ']:
                    self.assertNotIn(method, code, f'{method} in {rel}')
                for verb in ['/prune', '/kill', '/stop', '/start', '/restart', '/exec', '/attach', '/pause',
                             '/create', '/update', '/rename', '/build', '/push', '/pull', '/commit', '/load']:
                    self.assertNotIn(verb, code, f'{verb} in {rel}')
            if rel.startswith('src/storage/'):
                self.assertNotRegex(code, r'(?i)DELETE\s+FROM\s+(agent_runs|run_artifacts)\b', rel)
                self.assertNotRegex(code, r'(?i)DROP\s+TABLE', rel)
        http = (ROOT / 'src/docker/http.rs').read_text(encoding='utf-8')
        self.assertEqual(production_rust(http).count('"GET '), 1, 'one request line, always GET')

    def test_advisory_watcher_goes_online_and_starts_programs_only_where_documented(self):
        # The watcher is a separate, optional process (AGENTS.md): stdlib only,
        # downloads only OSV archives and the pinned scanner, starts only the
        # offline scanner and `mogumogu-cli --no-start`. Parsed, not a proof.
        package = ADVISORIES / 'src/mogumogu_advisories'
        online = {'feeds.py', 'scanner.py', 'watcher.py'}
        processes = {'scanner.py', 'watcher.py'}
        downloads = {'feeds.py': 'osv-vulnerabilities.storage.googleapis.com', 'scanner.py': 'github.com'}
        never = {'socket', 'ssl', 'ctypes', 'pickle', 'marshal', 'importlib', 'multiprocessing', 'pty', 'asyncio',
                 'ftplib', 'smtplib', 'telnetlib', 'xmlrpc', 'requests', 'urllib3', 'httpx', 'aiohttp'}
        wrappers = ['watch_advisories.py', 'audit_cargo_lock.py', 'advisory_report.py']
        files = {path.name: path for path in package.glob('*.py')}
        files.update({f'scripts/{name}': ROOT / 'scripts' / name for name in wrappers})
        files['_build_backend.py'] = ADVISORIES / '_build_backend.py'
        self.assertTrue(online <= set(files), 'renamed watcher modules: update this contract')
        for name, path in files.items():
            imports, calls, shell = python_effects(path)
            roots = {module.split('.')[0] for module in imports}
            with self.subTest(module=name):
                self.assertEqual(roots & never, set())
                if name not in online:
                    self.assertEqual({m for m in imports if m.startswith(('urllib.request', 'http.client'))}, set())
                if name not in processes:
                    self.assertNotIn('subprocess', roots)
                self.assertEqual({c for c in calls if c in {'eval', 'exec', '__import__', 'compile'}
                                  or c.startswith(('os.system', 'os.popen', 'os.startfile', 'os.exec', 'os.spawn'))},
                                 set())
                self.assertFalse(shell, 'no shell=')
        for name, host in downloads.items():
            text = files[name].read_text(encoding='utf-8')
            self.assertNotIn('http://', text, name)
            self.assertEqual(set(re.findall(r'https://([^/\s"\'{}]+)', text)), {host}, name)
        self.assertNotIn('urlopen', python_effects(files['watcher.py'])[1], 'watcher only passes the opener on')
        self.assertIn('"--offline"', files['scanner.py'].read_text(encoding='utf-8'))
        self.assertIn('"--no-start"', files['watcher.py'].read_text(encoding='utf-8'))
        report = files['report_html.py'].read_text(encoding='utf-8').lower()
        for tag in ['<script', '<link', '<img', '<iframe', '<object', '@import', 'src=']:
            self.assertNotIn(tag, report, 'the HTML report loads nothing')
        project = tomllib.loads((ADVISORIES / 'pyproject.toml').read_text(encoding='utf-8'))
        self.assertEqual(project['project']['dependencies'], [])
        self.assertEqual(project['build-system']['requires'], [])

    def test_loosening_protection_and_deciding_stay_with_the_person(self):
        # Ending or loosening protection, approving or running a plan and
        # finishing a project are refused over the pipe (owner.rs); the CLI
        # never sends them. Lexical, like the other contracts.
        owner = production_rust((ROOT / 'src/owner.rs').read_text(encoding='utf-8'))
        for request in ['Request::RunRelease', 'Request::DbUnregister', 'Request::KeepFolder { keep: false',
                        'Request::DockerRevoke', 'Request::RunsAllow { allowed: true }',
                        'Request::ScratchPlansInside { allowed: true', 'Request::CleanupApprove',
                        'Request::CleanupApply', 'Request::FinishProject']:
            self.assertIn(request, owner)
        cli = production_rust((ROOT / 'src/bin/cli/commands.rs').read_text(encoding='utf-8'))
        self.assertNotIn('Request::FinishProject', cli)
        opened = re.findall(r'ScratchPlansInside \{[^}]*\}', cli)
        self.assertTrue(opened and all('allowed: false' in found for found in opened), opened)

    def test_every_unsafe_block_is_documented(self):
        text = (ROOT / 'src/platform/windows.rs').read_text(encoding='utf-8')
        lines = text.splitlines()
        for number, line in enumerate(lines):
            if re.search(r'\bunsafe\s*\{', line) and 'fn ' not in line:
                window = '\n'.join(lines[max(0, number - 6):number + 1])
                self.assertIn('SAFETY:', window, f'line {number + 1}')

    def test_package_analysis_stays_free_of_effects(self):
        # The service facade loads records; this subtree only calculates.
        forbidden = [r'\b(?:Core|Repository|Registry|HttpRegistry)\b',
                     r'\bstd::(?:\{[^}]*\b)?(?:fs|io|net|process)\b',
                     r'\bcrate::(?:\{[^}]*\b)?(?:cleanup|config|fsread|inventory|ipc|owner|platform)\b']
        for path in (ROOT / 'src/service/packages').rglob('*.rs'):
            code = production_rust(path.read_text(encoding='utf-8'))
            for pattern in forbidden:
                self.assertNotRegex(code, pattern, f'package analysis must use supplied metadata: {path.name}')

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

    def test_only_licensed_fonts_and_no_binaries_or_databases_in_the_tree(self):
        # Prune build output before descending: a warm Cargo cache can contain
        # thousands of files, none of which belong to this source contract.
        excluded = {'target', 'dist', '.git'}
        forbidden = {'.ttf', '.otf', '.woff', '.woff2', '.exe', '.dll', '.sqlite3', '.db'}
        font_root = ROOT / 'assets/fonts/zen-maru-gothic'
        source = json.loads((font_root / 'SOURCE.json').read_text(encoding='utf-8'))
        self.assertEqual(source['license'], 'OFL-1.1')
        allowed_fonts = {'ZenMaruGothic-Regular.ttf', 'ZenMaruGothic-Bold.ttf'}
        self.assertEqual(set(source['files']), allowed_fonts | {'OFL.txt'})
        for name, digest in source['files'].items():
            self.assertEqual(hashlib.sha256((font_root / name).read_bytes()).hexdigest(), digest, name)
        for directory, subdirs, filenames in os.walk(ROOT):
            subdirs[:] = [name for name in subdirs if name not in excluded]
            for name in filenames:
                path = Path(directory) / name
                if path.parent == font_root and name in allowed_fonts:
                    continue
                self.assertNotIn(path.suffix.lower(), forbidden, str(path))

    def test_plan_ids_and_documents(self):
        plan = (ROOT / 'docs/IMPLEMENTIERUNGSPLAN.md').read_text(encoding='utf-8')
        ids = re.findall(r'^\| (F-\d+) \|', plan, re.M)
        self.assertEqual(len(ids), 28)
        self.assertEqual(len(ids), len(set(ids)))
        for name in ['README.md', 'LICENSE', 'THIRD_PARTY.md', 'SECURITY.md', 'CONTRIBUTING.md', 'AGENTS.md', 'CLAUDE.md',
                     'docs/VALIDIERUNG.md', 'docs/WINDOWS-ABNAHME.md', 'docs/UNTERSTUETZUNGSMATRIX.md',
                     'docs/ANLEITUNG.md', 'build.ps1', 'start.ps1', 'start.cmd', 'build.cmd', 'scripts/measure.ps1',
                     'scripts/release.ps1', 'docs/ARCHITEKTUR.md']:
            self.assertTrue((ROOT / name).is_file(), name)
        for doc in [*ROOT.glob('*.md'), *(ROOT / 'docs').glob('*.md'), *ROOT.glob('packages/*/README.md')]:
            for target in re.findall(r'\]\(((?!https?://|#|mailto:)[^)#]+)', doc.read_text(encoding='utf-8')):
                self.assertTrue((doc.parent / target).exists(), f'{doc.name}: {target}')


if __name__ == '__main__':
    unittest.main(verbosity=2)
