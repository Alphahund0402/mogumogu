//! In-process owner for end-to-end tests: the test process is the owner,
//! the real CLI binary is the client. Its recycle bin is a test directory.
use super::TestBin;
use mogumogu::config::Config;
use mogumogu::domain::Snapshot;
use mogumogu::owner::{Owner, OwnerEvents};
use mogumogu::service::{Core, Request};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

pub struct Quiet;

impl OwnerEvents for Quiet {
    fn snapshot(&self, _: Snapshot) {}
    fn outcome(&self, _: &Request, _: &mogumogu::Result<Value>) {}
    fn busy(&self, _: bool) {}
    fn show_dashboard(&self) {}
    fn hide_dashboard(&self) {}
    fn shutdown(&self) {}
}

pub fn start_owner(data: &Path) -> Owner {
    start_owner_with_bin(data).0
}

/// The owner and the test bin it moves folders into.
pub fn start_owner_with_bin(data: &Path) -> (Owner, TestBin) {
    let mut core = Core::open(Config::new(Some(data.to_path_buf()), false).unwrap()).unwrap();
    let bin = TestBin::new(data.with_extension("papierkorb"));
    core.use_recycler(Box::new(bin.clone()));
    (Owner::start_with(core, Quiet).expect("owner starts"), bin)
}

/// Runs the real CLI against `data`; never auto-starts another owner.
pub fn cli(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mogumogu-cli"))
        .arg("--data-dir")
        .arg(data)
        .arg("--json")
        .arg("--no-start")
        .args(args)
        .output()
        .expect("cli runs")
}

/// Parses the versioned JSON envelope and returns `result`, panicking with
/// the CLI output on failure.
pub fn ok(output: Output) -> Value {
    let text = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| panic!("no JSON: {text} / {}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(value["ok"], true, "{text}");
    value["result"].clone()
}

pub fn err(output: Output) -> (String, i32) {
    let text = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("no JSON: {text}"));
    assert_eq!(value["ok"], false, "{text}");
    (value["code"].as_str().unwrap_or_default().to_string(), output.status.code().unwrap_or(-1))
}
