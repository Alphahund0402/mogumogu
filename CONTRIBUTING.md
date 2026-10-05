# Beitragen

Die gemeinsame technische Grundlage steht in `AGENTS.md`. Bitte kleine PRs mit klarem
beobachtbarem Verhalten, Grenzen und Tests erstellen. UI, Datenmodell und Sicherheitsregeln
nicht in einem undurchsichtigen Großumbau ändern.

Vor jedem PR: `python scripts/validate.py`, `cargo fmt --all -- --check`,
`cargo clippy --locked --all-targets -- -D warnings` und
`cargo test --locked --no-default-features --features network`. Schemaänderungen erhalten eine
neue Migration in `migrations/` plus Test; bestehende Migrationen werden nie geändert.
Neue Paketformate brauchen positive und negative Fixtures; neue KI-Formate werden als Eintrag
in `profiles/ai-catalog.toml` ergänzt.

Änderungen an `src/cleanup/`, `src/fsread.rs` oder `src/platform/windows.rs` benötigen den
Review einer zweiten Person. Produktive Bereinigung bleibt deaktiviert, bis Gate G5
(unabhängiger Review der Löschpfade) dokumentiert bestanden ist.

Projektlizenz: GPL-3.0-only. Beiträge müssen damit vereinbar sein. Keine proprietären
Assets, Kundendaten, Schlüssel, Fontdateien oder persönlichen Inventare einchecken.
