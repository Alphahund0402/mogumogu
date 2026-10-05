//! In-process owner for end-to-end tests: the test process is the owner,
//! the real CLI binary is the client.
use mogumogu::config::Config;
use mogumogu::domain::Snapshot;
use mogumogu::owner::{Owner, OwnerEvents};
use mogumogu::service::Request;
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
    Owner::start(Config::new(Some(data.to_path_buf()), false).unwrap(), Quiet).expect("owner starts")
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

/// Like [`cli`], but stdout goes to a file, so background processes that
/// inherit the handle cannot delay the test.
pub fn cli_to_file(data: &Path, args: &[&str]) -> Value {
    let out = data.with_extension(format!("out-{}.json", std::process::id()));
    let file = std::fs::File::create(&out).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_mogumogu-cli"))
        .arg("--data-dir")
        .arg(data)
        .args(["--json", "--no-start"])
        .args(args)
        .stdout(file)
        .stderr(std::process::Stdio::null())
        .status()
        .expect("cli runs");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(status.success(), "{text}");
    let value: Value = serde_json::from_str(&text).unwrap();
    value["result"].clone()
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
