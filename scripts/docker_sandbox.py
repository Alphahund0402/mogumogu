#!/usr/bin/env python3
"""Isolated Docker test & benchmark sandbox builder and runner.

Python 3.11+, standard library only.
- `--build <dir>`: runs formatting/Clippy and compiles core test + release
  benchmark binaries into `<dir>` inside the immutable image layer.
- `--run <dir>`: executes the pre-built tests and benchmarks completely
  offline (`network_mode: none`), read-only, non-root, using ephemeral `/tmp`.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(command, *, cwd=ROOT, env=None):
    print(f"\n> {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def collect_test_binaries(command):
    """Runs `cargo test --no-run --message-format=json` and returns built executables."""
    print(f"\n> {' '.join(command)}", flush=True)
    proc = subprocess.Popen(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
        text=True,
    )
    found = []
    assert proc.stdout is not None
    for line in proc.stdout:
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get('reason') == 'compiler-artifact' and msg.get('executable') and msg.get('profile', {}).get('test'):
            target = msg.get('target', {})
            found.append({
                'name': target.get('name', Path(msg['executable']).name),
                'kind': target.get('kind', []),
                'path': Path(msg['executable']),
            })
    code = proc.wait()
    if code != 0:
        raise subprocess.CalledProcessError(code, command)
    return found


def read_cgroup(path):
    p = Path(path)
    if not p.is_file():
        return None
    try:
        return p.read_text(encoding='utf-8').strip()
    except OSError:
        return None


def print_environment(scanner: Path | None):
    cpu_max = read_cgroup('/sys/fs/cgroup/cpu.max')
    mem_max = read_cgroup('/sys/fs/cgroup/memory.max')
    pids_max = read_cgroup('/sys/fs/cgroup/pids.max')
    net_devs = sorted(p.name for p in Path('/sys/class/net').iterdir()) if Path('/sys/class/net').is_dir() else []
    print('=== Isolierte Docker-Testumgebung ===')
    print(f'System:     {platform.system()} {platform.release()} ({platform.machine()})')
    print(f'UID/GID:    {os.getuid()}:{os.getgid()} · TMPDIR={os.environ.get("TMPDIR", "/tmp")}')
    print(f'CPUs:       os.cpu_count()={os.cpu_count()} · cgroup cpu.max={cpu_max or "unbegrenzt"}')
    print(f'RAM/PIDs:   memory.max={mem_max or "unbegrenzt"} · pids.max={pids_max or "unbegrenzt"}')
    print(f'Netzwerk:   Interfaces={net_devs} (nur lo = offline / network_mode: none)')
    print(f'OSV-Scanner: {scanner if scanner else "nicht gebündelt"}')
    print('Hinweis:    Prüft den portablen Kern, statische Verträge, den Offline-OSV-Abgleich und den Release-Benchmark.')
    print('            Windows-API-Tests (Tray, Named Pipe, NtCreateFile, Papierkorb) laufen nur nativ.')
    print('=====================================\n', flush=True)


def build_sandbox(out_dir: Path):
    toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text(encoding='utf-8'))['toolchain']['channel']
    actual = subprocess.check_output(['rustc', '--version'], cwd=ROOT, text=True).split()[1]
    if actual != toolchain:
        raise SystemExit(f'Rust {toolchain} required; found {actual}.')

    run(['cargo', 'fmt', '--all', '--', '--check'])
    run(['cargo', 'clippy', '--locked', '--no-default-features', '--all-targets', '--', '-D', 'warnings'])

    tests_dir = out_dir / 'tests'
    bench_dir = out_dir / 'bench'
    tests_dir.mkdir(parents=True, exist_ok=True)
    bench_dir.mkdir(parents=True, exist_ok=True)

    debug_bins = collect_test_binaries([
        'cargo', 'test', '--locked', '--no-default-features', '--no-run', '--message-format=json',
    ])
    manifest = {'toolchain': actual, 'tests': [], 'bench': None}
    for index, item in enumerate(debug_bins):
        dest = tests_dir / f'{index:02d}-{item["name"]}'
        shutil.copy2(item['path'], dest)
        os.chmod(dest, 0o755)
        manifest['tests'].append({'name': item['name'], 'kind': item['kind'], 'bin': dest.name})

    bench_bins = collect_test_binaries([
        'cargo', 'test', '--locked', '--release', '--no-default-features',
        '--test', 'performance', '--no-run', '--message-format=json',
    ])
    if not bench_bins:
        raise SystemExit('Release benchmark binary was not produced.')
    bench_dest = bench_dir / 'performance'
    shutil.copy2(bench_bins[-1]['path'], bench_dest)
    os.chmod(bench_dest, 0o755)
    manifest['bench'] = bench_dest.name

    (out_dir / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    print(f'\nSandbox built in {out_dir}: {len(manifest["tests"])} test binaries + release benchmark.', flush=True)


def run_sandbox(out_dir: Path, run_tests: bool, run_bench: bool, runs: int):
    scanner = out_dir / 'bin' / 'osv-scanner'
    scanner_path = scanner if scanner.is_file() else None
    print_environment(scanner_path)
    manifest = json.loads((out_dir / 'manifest.json').read_text(encoding='utf-8'))
    env = dict(os.environ)
    env.setdefault('TMPDIR', '/tmp')
    env.setdefault('RUST_BACKTRACE', '1')
    if scanner_path is not None:
        env['MOGUMOGU_TEST_OSV_SCANNER'] = str(scanner_path)

    if run_tests:
        run([sys.executable, 'scripts/validate.py'], env=env)
        run([sys.executable, '-B', '-m', 'unittest', 'discover', '-v', '-s', 'scripts', '-p', 'test_*.py'], env=env)
        lib_env = dict(env)
        lib_env['PYTHONPATH'] = str(ROOT / 'packages' / 'mogumogu-advisories' / 'src')
        run([sys.executable, '-B', '-m', 'unittest', 'discover', '-v', '-s', 'packages/mogumogu-advisories/tests', '-p', 'test_*.py'], env=lib_env)
        for entry in manifest['tests']:
            binary = str(out_dir / 'tests' / entry['bin'])
            run([binary], env=env)

    if run_bench:
        bench_bin = str(out_dir / 'bench' / manifest['bench'])
        for i in range(1, runs + 1):
            print(f'\n--- Isolierter Benchmark-Lauf {i}/{runs} (Rust {manifest["toolchain"]}, release) ---', flush=True)
            run([bench_bin, '--ignored', '--nocapture', '--exact', 'portable_core_benchmark'], env=env)

    print('\nAlle isolierten Sandbox-Prüfungen erfolgreich abgeschlossen.', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--build', type=Path, help='compile test and benchmark binaries into this directory')
    group.add_argument('--run', type=Path, help='execute pre-built tests/benchmarks from this directory')
    parser.add_argument('--all', action='store_true', help='run static checks, unit/core tests, and benchmarks (default)')
    parser.add_argument('--tests', action='store_true', help='run static checks and unit/core tests only')
    parser.add_argument('--bench', action='store_true', help='run the release benchmark only')
    parser.add_argument('--runs', type=int, default=3, help='number of benchmark repetitions (default: 3)')
    args = parser.parse_args()

    if args.build:
        build_sandbox(args.build)
    else:
        run_tests = args.tests or args.all or (not args.tests and not args.bench)
        run_bench = args.bench or args.all or (not args.tests and not args.bench)
        run_sandbox(args.run, run_tests=run_tests, run_bench=run_bench, runs=max(1, args.runs))


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(error.returncode)
    except KeyboardInterrupt:
        sys.exit(130)
