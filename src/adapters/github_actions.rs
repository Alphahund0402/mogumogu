//! GitHub Actions: `uses:` references in workflows (.github/workflows/*.yml,
//! handed over by the scanner) and in action metadata (action.yml), plus the
//! container and service images of workflow jobs. Nothing is run, resolved
//! or fetched; expressions and matrix values stay unknown. The
//! security-relevant fact is whether a reference is pinned to a full commit
//! SHA; whether a tag or branch has moved since is not known.
use super::docker::{Images, KeyPath, Unique, count, image_value};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::lines;
use std::collections::HashSet;

pub struct GitHubActions;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "github-actions",
    name: "GitHub Actions",
    markers: &["action.yml", "action.yaml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        ".github/workflows/*.yml (uses in Schritten und Jobs, Job-Container und -Dienste)",
        "action.yml/action.yaml (composite, docker)",
    ],
    limits: "Workflows werden nicht ausgeführt; Ausdrücke, Matrix-Werte, YAML-Anker und Inhalte aufgerufener Aktionen bleiben unbekannt, als Quelle gilt github.com. Ob ein Tag oder Branch inzwischen auf einen anderen Commit zeigt, wird nicht geprüft; Commit-SHAs werden auf 12 Zeichen gekürzt gespeichert.",
};

impl Adapter for GitHubActions {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        bounded(action(file))
    }
}

/// Version and pinning of a Git ref. A full commit SHA counts as pinned and
/// is kept as its first 12 hex digits: the full one would read as a secret.
pub(super) fn git_ref(reference: &str) -> (String, bool) {
    if reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
        (reference[..12].to_ascii_lowercase(), true)
    } else {
        (reference.to_string(), false)
    }
}

/// References of a workflow file; called from the scanner.
pub(crate) fn workflow_uses(rel: &str, text: &str) -> AdapterResult {
    let (rel_dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let file = FoundFile { name, rel_dir, content: text, siblings: &[] };
    bounded(workflow(&file))
}

/// `uses:` references and images of one workflow or action file.
struct Uses {
    rel_path: String,
    actions: Unique,
    images: Images,
    total: usize,
    pinned: usize,
    unknown: usize,
    /// References already handled in a place.
    seen: HashSet<(String, String)>,
}

impl Uses {
    fn new(rel_path: String) -> Self {
        let images = Images::new(rel_path.clone(), None, "Standardwert aus Variable");
        Self { rel_path, actions: Unique::default(), images, total: 0, pinned: 0, unknown: 0, seen: HashSet::new() }
    }

    /// One `uses:` value; `place` is the job id or `composite`.
    fn add(&mut self, value: &str, place: &str, reusable: bool) {
        let value = value.trim();
        if value.is_empty() || self.actions.full() {
            return;
        }
        if !self.seen.insert((value.to_string(), format!("{reusable}|{place}"))) {
            return;
        }
        if let Some(image) = value.strip_prefix("docker://") {
            self.total += 1;
            self.images.add(image, "github-actions", place, "Docker-Aktion");
            return;
        }
        let kind = if reusable { "Wiederverwendbarer Workflow · " } else { "" };
        let item = if value.starts_with("./") || value.starts_with("../") || value.starts_with("$/") {
            InventoryItem::new(
                ItemCategory::Package,
                "github-actions",
                value.trim_end_matches('/'),
                self.rel_path.as_str(),
            )
            .source(SourceClass::Path, None)
            .detail(if reusable {
                "Workflow im eigenen Repository"
            } else {
                "Aktion im eigenen Repository"
            })
        } else {
            let Some((name, reference)) = value.rsplit_once('@').filter(|(name, reference)| valid(name, reference))
            else {
                self.unknown += 1;
                return;
            };
            let (version, pinned) = git_ref(reference);
            self.pinned += usize::from(pinned);
            let pin = if pinned { "auf Commit gepinnt" } else { "Tag oder Branch, nicht gepinnt" };
            InventoryItem::new(ItemCategory::Package, "github-actions", name, self.rel_path.as_str())
                .version(version)
                .source(SourceClass::Git, Some("github.com".into()))
                .detail(format!("{kind}{pin}"))
        };
        let mut item = item.state(InstallState::Declared);
        // Reusable workflows share one context; the calling job tells them apart.
        let (context, qualifier) = if reusable { ("reusable-workflow", place) } else { (place, "") };
        item.evidence.context = context.into();
        item.evidence.qualifier = qualifier.into();
        self.total += 1;
        self.actions.push(item);
    }

    fn image(&mut self, value: &str, place: &str, label: &str) {
        self.images.add(value, "github-actions", place, label);
    }

    /// Manifest detail on references, then the items behind the manifest.
    fn finish(self, file: &FoundFile<'_>, mut detail: String) -> Vec<InventoryItem> {
        if self.total > 0 {
            detail.push_str(&format!(" · {}", count(self.total, "uses-Verweis", "uses-Verweise")));
        }
        if self.pinned > 0 {
            detail.push_str(&format!(", davon {} auf Commit gepinnt", self.pinned));
        }
        if self.unknown > 0 {
            detail.push_str(&format!(" · {} nicht erkannt", count(self.unknown, "uses-Angabe", "uses-Angaben")));
        }
        detail.push_str(&self.images.note("Ausdrücken oder YAML-Ankern"));
        let mut items = vec![manifest("github-actions", file, detail)];
        items.extend(self.actions.items);
        items.extend(self.images.unique.items);
        items
    }
}

/// `owner/repo[/path]` and a ref without spaces.
fn valid(name: &str, reference: &str) -> bool {
    let segments: Vec<&str> = name.split('/').collect();
    let plain = |part: &str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    segments.len() >= 2
        && segments[..2].iter().all(|part| plain(part))
        && segments.iter().all(|part| !part.is_empty() && !part.contains(char::is_whitespace))
        && !reference.is_empty()
        && reference.len() <= 255
        && !reference.contains(char::is_whitespace)
}

fn workflow(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut uses = Uses::new(file.rel_path());
    let mut path = KeyPath::default();
    let (mut found, mut jobs) = (false, 0);
    for entry in lines::yaml_entries(file.content).take(200_000) {
        path.enter(&entry);
        match (path.keys(), entry.key) {
            ([], Some("jobs")) => found = true,
            (["jobs"], Some(_)) => jobs += 1,
            (["jobs", job], Some("uses")) => uses.add(entry.value, job, true),
            (["jobs", job, "steps"], Some("uses")) => uses.add(entry.value, job, false),
            (["jobs", job], Some("container")) | (["jobs", job, "container"], Some("image")) => {
                uses.image(image_value(entry.value, "image"), job, "Job-Container")
            }
            (["jobs", job, "services"], Some(_)) if entry.value.starts_with('{') => {
                uses.image(image_value(entry.value, "image"), job, "Job-Dienst")
            }
            (["jobs", job, "services", _], Some("image")) => {
                uses.image(image_value(entry.value, "image"), job, "Job-Dienst")
            }
            _ => {}
        }
        path.push(&entry);
    }
    if !found {
        return Vec::new();
    }
    uses.finish(file, format!("Workflow · {}", count(jobs, "Job", "Jobs")))
}

fn action(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut uses = Uses::new(file.rel_path());
    let mut path = KeyPath::default();
    let (mut found, mut using, mut dockerfile) = (false, "", false);
    for entry in lines::yaml_entries(file.content).take(200_000) {
        path.enter(&entry);
        match (path.keys(), entry.key) {
            ([], Some("runs")) => found = true,
            (["runs"], Some("using")) => using = entry.value,
            (["runs"], Some("image")) => match entry.value.strip_prefix("docker://") {
                Some(image) => uses.image(image, "action", "Docker-Aktion"),
                None => dockerfile = !entry.value.is_empty(),
            },
            (["runs", "steps"], Some("uses")) => uses.add(entry.value, "composite", false),
            _ => {}
        }
        path.push(&entry);
    }
    if !found {
        return Vec::new();
    }
    let detail = match using.to_ascii_lowercase().as_str() {
        "composite" => "Composite-Aktion".to_string(),
        "docker" if dockerfile => "Docker-Aktion · Image aus eigenem Dockerfile".to_string(),
        "docker" => "Docker-Aktion".to_string(),
        node if node.starts_with("node") => format!("JavaScript-Aktion ({node})"),
        other => format!("Aktion ({})", if other.is_empty() { "ohne using" } else { other }),
    };
    uses.finish(file, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const WORKFLOW: &str = r#"name: CI
on:
  push:
    branches: [main]
jobs:
  test:
    runs-on: ubuntu-latest
    container:
      image: node:20-bookworm
      credentials:
        username: ${{ github.actor }}
        password: ${{ secrets.GITHUB_TOKEN }}
    services:
      redis:
        image: redis:7
      db:
        image: ${{ matrix.db }}
    steps:
      - uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11 # v4.1.1
      - name: Setup node
        uses: actions/setup-node@v4
        with:
          node-version: 20
          uses: not/a-step@v1
      - uses: actions/aws/ec2@main
      - uses: ./.github/actions/setup
      - uses: $/.github/actions/cache
      - uses: docker://alpine:3.20
      - uses: no-ref
      - run: |
          echo "uses: evil/action@v1"
          uses: fake/inside-run@v1
  lint:
    runs-on: ubuntu-latest
    container: python:3.12-slim
    steps:
      - uses: actions/checkout@v4
      - uses: actions/checkout@v3
  call:
    uses: octo-org/example-repo/.github/workflows/reusable.yml@v1.2.0
    secrets: inherit
  local-call:
    uses: ./.github/workflows/local.yml
"#;

    #[test]
    fn workflow_uses_jobs_images_and_pinning() {
        let items = workflow_uses(".github/workflows/ci.yml", WORKFLOW).unwrap();
        assert_eq!(items[0].name, "ci.yml");
        assert_eq!(items[0].rel_path, ".github/workflows/ci.yml");
        assert_eq!(
            items[0].detail,
            "Workflow · 4 Jobs · 10 uses-Verweise, davon 1 auf Commit gepinnt · 1 uses-Angabe nicht erkannt · 1 Image aus Ausdrücken oder YAML-Ankern (nicht aufgelöst)"
        );
        let action = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.ecosystem == "github-actions" && i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} in {context}: {items:#?}"))
        };
        let checkout = action("actions/checkout", "test");
        assert_eq!(checkout.version.as_deref(), Some("b4ffde65f463"));
        assert_eq!(checkout.detail, "auf Commit gepinnt");
        assert_eq!((checkout.source, checkout.source_host.as_deref()), (SourceClass::Git, Some("github.com")));
        let setup = action("actions/setup-node", "test");
        assert_eq!((setup.version.as_deref(), setup.detail.as_str()), (Some("v4"), "Tag oder Branch, nicht gepinnt"));
        assert_eq!(action("actions/aws/ec2", "test").version.as_deref(), Some("main"));
        let local = action("./.github/actions/setup", "test");
        assert_eq!((local.source, local.version.as_deref()), (SourceClass::Path, None));
        assert_eq!(action("$/.github/actions/cache", "test").source, SourceClass::Path);
        let v4 = action("actions/checkout", "lint");
        let v3 = items.iter().find(|i| i.name == "actions/checkout" && i.evidence.qualifier == "v3").unwrap();
        assert_eq!((v4.evidence.qualifier.as_str(), v3.evidence.context.as_str()), ("", "lint"));
        let reusable = action("octo-org/example-repo/.github/workflows/reusable.yml", "reusable-workflow");
        assert_eq!((reusable.version.as_deref(), reusable.evidence.qualifier.as_str()), (Some("v1.2.0"), "call"));
        assert!(reusable.detail.starts_with("Wiederverwendbarer Workflow"));
        let local_call = action("./.github/workflows/local.yml", "reusable-workflow");
        assert_eq!((local_call.source, local_call.evidence.qualifier.as_str()), (SourceClass::Path, "local-call"));

        let docker: Vec<(&str, Option<&str>, &str)> = items
            .iter()
            .filter(|i| i.ecosystem == "docker")
            .map(|i| (i.name.as_str(), i.version.as_deref(), i.evidence.qualifier.as_str()))
            .collect();
        assert_eq!(
            docker,
            [
                ("node", Some("20-bookworm"), "test"),
                ("redis", Some("7"), "test"),
                ("alpine", Some("3.20"), "test"),
                ("python", Some("3.12-slim"), "lint")
            ]
        );
        assert!(items.iter().filter(|i| i.ecosystem == "docker").all(|i| i.evidence.context == "github-actions"));
        let debug = format!("{items:?}");
        for absent in ["evil", "fake", "not/a-step", "GITHUB_TOKEN", "b4ffde65f46336ab88eb"] {
            assert!(!debug.contains(absent), "{absent}");
        }
        assert!(workflow_uses(".github/workflows/x.yml", "name: not a workflow\n").unwrap().is_empty());

        let flow = "on: push\njobs:\n  a:\n    container: { image: \"node:22\" }\n    services:\n      cache: { image: redis:7.4, ports: [\"6379:6379\"] }\n      db:\n        image: *db_image\n    steps:\n      - uses: actions/checkout@v4\n";
        let items = workflow_uses(".github/workflows/flow.yaml", flow).unwrap();
        assert_eq!(
            items[0].detail,
            "Workflow · 1 Job · 1 uses-Verweis · 1 Image aus Ausdrücken oder YAML-Ankern (nicht aufgelöst)"
        );
        let images: Vec<(&str, Option<&str>)> =
            items.iter().filter(|i| i.ecosystem == "docker").map(|i| (i.name.as_str(), i.version.as_deref())).collect();
        assert_eq!(images, [("node", Some("22")), ("redis", Some("7.4"))]);
    }

    #[test]
    fn composite_docker_and_foreign_action_files() {
        let composite = "name: Setup\ndescription: Sets up the toolchain\nruns:\n  using: composite\n  steps:\n    - uses: actions/setup-python@v5\n      with:\n        python-version: '3.12'\n    - run: echo hi\n      shell: bash\n    - uses: docker://ghcr.io/acme/tool:1.2\n";
        let items = GitHubActions.parse_file(&file("action.yml", composite)).unwrap();
        assert_eq!(items[0].detail, "Composite-Aktion · 2 uses-Verweise");
        let python = items.iter().find(|i| i.name == "actions/setup-python").unwrap();
        assert_eq!((python.evidence.context.as_str(), python.version.as_deref()), ("composite", Some("v5")));
        let tool = items.iter().find(|i| i.name == "ghcr.io/acme/tool").unwrap();
        assert_eq!((tool.ecosystem.as_str(), tool.evidence.qualifier.as_str()), ("docker", "composite"));

        let docker =
            "name: Hello\ndescription: Greets\nruns:\n  using: 'docker'\n  image: 'docker://debian:stretch-slim'\n";
        let items = GitHubActions.parse_file(&file("action.yaml", docker)).unwrap();
        assert_eq!(items[0].detail, "Docker-Aktion");
        assert_eq!((items[1].name.as_str(), items[1].version.as_deref()), ("debian", Some("stretch-slim")));
        let build = "name: Build\ndescription: x\nruns:\n  using: docker\n  image: Dockerfile\n";
        let items = GitHubActions.parse_file(&file("action.yml", build)).unwrap();
        assert_eq!((items.len(), items[0].detail.as_str()), (1, "Docker-Aktion · Image aus eigenem Dockerfile"));
        let node = "name: Node\ndescription: x\nruns:\n  using: node20\n  main: dist/index.js\n";
        assert!(
            GitHubActions.parse_file(&file("action.yml", node)).unwrap()[0]
                .detail
                .starts_with("JavaScript-Aktion (node20)")
        );
        let foreign = "name: deploy\nsteps:\n  - command: deploy\n    uses: something@v1\n";
        assert!(GitHubActions.parse_file(&file("action.yml", foreign)).unwrap().is_empty());
        let workflow =
            "on: push\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n";
        assert!(
            GitHubActions.parse_file(&file("action.yml", workflow)).unwrap().is_empty(),
            "the scanner reads workflows"
        );
        assert_eq!(git_ref("B4FFDE65F46336AB88EB53BE808477A3936BAE11"), ("b4ffde65f463".into(), true));
        assert_eq!(git_ref("v4"), ("v4".into(), false));
    }
}
