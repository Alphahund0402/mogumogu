//! Ordering of concrete versions, never dependency resolution or compatibility.
use crate::domain::{UpdateStatus, package_ecosystem};
use std::cmp::Ordering;

pub fn compare(ecosystem: &str, installed: &str, latest: &str) -> UpdateStatus {
    match ordering(ecosystem, installed, latest) {
        Some(Ordering::Less) => UpdateStatus::UpdateAvailable,
        Some(_) => UpdateStatus::Current,
        None => UpdateStatus::Unknown,
    }
}

pub(super) fn ordering(ecosystem: &str, a: &str, b: &str) -> Option<Ordering> {
    match package_ecosystem(ecosystem) {
        "python" => Some(a.parse::<pep440_rs::Version>().ok()?.cmp(&b.parse().ok()?)),
        "npm" | "cargo" => {
            let parse = |s: &str| s.trim().trim_start_matches('v').parse::<semver::Version>().ok();
            Some(parse(a)?.cmp_precedence(&parse(b)?))
        }
        "nuget" => {
            let parse = |s: &str| -> Option<(Vec<u64>, semver::Prerelease)> {
                let s = s.split('+').next()?;
                let (core, pre) = s.split_once('-').unwrap_or((s, ""));
                let mut parts = core.split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok()?;
                if !(1..=4).contains(&parts.len()) {
                    return None;
                }
                parts.resize(4, 0);
                Some((parts, semver::Prerelease::new(&pre.to_ascii_lowercase()).ok()?))
            };
            Some(parse(a)?.cmp(&parse(b)?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ecosystems_keep_their_ordering_rules() {
        for (eco, a, b) in [
            ("npm", "1.0.0-beta.1", "1.0.0"),
            ("cargo", "1.0.0-rc.1", "1.0.0"),
            ("pip", "1.0rc1", "1.0"),
            ("uv", "1.0.dev2", "1.0a1"),
            ("python", "1.0", "1.0.post1"),
            ("python", "1.0", "1!0.1"),
            ("nuget", "1.0.0-alpha.9", "1.0.0-alpha.10"),
            ("nuget", "1.2.3", "1.2.3.1"),
        ] {
            assert_eq!(compare(eco, a, b), UpdateStatus::UpdateAvailable, "{eco}: {a} < {b}");
        }
        assert_eq!(compare("npm", "1.0.0+build.1", "1.0.0+build.2"), UpdateStatus::Current);
        assert_eq!(compare("python", "1.0+local", "1.0"), UpdateStatus::Current);
        assert_eq!(compare("nuget", "1.2", "1.2.0"), UpdateStatus::Current);
        assert_eq!(compare("npm", "garbage", "1.0.0"), UpdateStatus::Unknown);
        assert_eq!(compare("unknown", "1", "2"), UpdateStatus::Unknown);
    }
}
