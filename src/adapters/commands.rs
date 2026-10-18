//! A deliberately narrow command-name index, not a shell/YAML interpreter.
//! Arguments, environment values and complete command lines are never stored.
use crate::domain::{InventoryItem, ItemCategory};
use std::collections::BTreeSet;

pub(crate) fn basename(value: &str) -> Option<String> {
    let name = value.trim_matches(['\'', '"']).rsplit(['\\', '/']).next()?.to_ascii_lowercase();
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".cmd"))
        .or_else(|| name.strip_suffix(".ps1"))
        .unwrap_or(&name);
    (!name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    .then(|| name.to_string())
}
pub(crate) fn names(text: &str) -> Vec<String> {
    text.split(['\n', ';', '&', '|'])
        .filter_map(|statement| {
            let first = statement.split_whitespace().find(|s| !s.contains('='))?;
            basename(first)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(50)
        .collect()
}

pub(crate) fn workflow(rel: &str, text: &str) -> InventoryItem {
    let mut item = InventoryItem::new(ItemCategory::Manifest, "commands", "CI-Befehlsverweise", rel)
        .detail("Statische run-Befehlsnamen; Argumente und Bedingungen nicht ausgewertet");
    let mut block = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if block.is_some_and(|level| indent > level) {
            item.evidence.commands.extend(names(trimmed));
            continue;
        }
        block = None;
        if let Some(run) = trimmed.trim_start_matches("- ").strip_prefix("run:") {
            if run.trim_start().starts_with(['|', '>']) {
                block = Some(indent);
            } else {
                item.evidence.commands.extend(names(run.trim()));
            }
        }
    }
    item.evidence.commands.sort();
    item.evidence.commands.dedup();
    item.evidence.commands.truncate(50);
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_and_environment_values_are_not_indexed() {
        assert_eq!(names("TOKEN=secret tool --password secret && echo private"), ["echo", "tool"]);
        let item = workflow(
            ".github/workflows/check.yml",
            "steps:\n  - run: |\n      tool --token private\n      next arg\n  - name: unused\n    env:\n      SECRET: never\n",
        );
        assert_eq!(item.evidence.commands, ["next", "tool"]);
        assert!(!format!("{item:?}").contains("private"));
    }
}
