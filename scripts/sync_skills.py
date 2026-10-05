#!/usr/bin/env python3
"""Copies the canonical skills (.agents/skills, used by Codex) to the
Claude Code location (.claude/skills). Deterministic and one-way; the
validator fails if a copy drifts. Never touches global settings.

Usage: python scripts/sync_skills.py
"""
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / '.agents' / 'skills'
TARGET = ROOT / '.claude' / 'skills'


def main() -> None:
    names = {p.parent.name for p in SOURCE.glob('*/SKILL.md')}
    for stale in [p for p in TARGET.glob('*') if p.is_dir() and p.name not in names]:
        shutil.rmtree(stale)
    for name in sorted(names):
        destination = TARGET / name
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(SOURCE / name / 'SKILL.md', destination / 'SKILL.md')
        print(f'{name}: synchronisiert')


if __name__ == '__main__':
    main()
