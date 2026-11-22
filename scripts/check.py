#!/usr/bin/env python3
"""Shared local/container checks. Python 3.11+, no third-party packages.

Windows includes the native UI; --core (automatic on Linux) checks the portable
core only. Neither mode replaces the manual Windows acceptance tests.
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(command, *, env=None):
    print(f"\n> {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core', action='store_true', help='skip the Windows desktop feature')
    parser.add_argument('--offline', action='store_true', help='use only already cached Cargo dependencies')
    args = parser.parse_args()
    core = args.core or sys.platform != 'win32'
    lock = ['--locked', *(['--offline'] if args.offline else [])]
    features = ['--no-default-features']

    toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text(encoding='utf-8'))['toolchain']['channel']
    actual = subprocess.check_output(['rustc', '--version'], cwd=ROOT, text=True).split()[1]
    if actual != toolchain:
        parser.error(f'Rust {toolchain} is required; found {actual}. Update the container and toolchain together.')

    print(f"Checking {'portable core' if core else 'Windows core and desktop'} with Rust {actual}.", flush=True)
    run([sys.executable, 'scripts/validate.py'])
    run([sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_*.py'])
    lib_env = dict(os.environ)
    lib_env['PYTHONPATH'] = str(ROOT / 'packages' / 'mogumogu-advisories' / 'src')
    run(
        [sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'packages/mogumogu-advisories/tests', '-p', 'test_*.py'],
        env=lib_env,
    )
    run(['cargo', 'fmt', '--all', '--', '--check'])
    run(['cargo', 'clippy', *lock, '--all-targets', *(features if core else []), '--', '-D', 'warnings'])
    run(['cargo', 'test', *lock, *features])
    if not core:
        run(['cargo', 'test', *lock, '--bin', 'mogumogu'])
    print('\nAll checks passed.', flush=True)


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(error.returncode)
    except FileNotFoundError as error:
        sys.exit(f'Required tool or input is missing: {error.filename}')
    except KeyboardInterrupt:
        sys.exit(130)
