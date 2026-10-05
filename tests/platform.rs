//! Windows primitives used by sessions and IPC (AT-10, AT-13).
#![cfg(windows)]

use mogumogu::domain::ProcessIdentity;
use mogumogu::platform::{self, Job};
use std::process::Command;

#[test]
fn named_jobs_can_be_reopened_and_count_processes() {
    let name = format!(r"Local\mogumogu-test-{}", std::process::id());
    let job = Job::create(&name).unwrap();
    let reopened = Job::open(&name).unwrap().expect("job visible by name");
    assert_eq!(reopened.active_processes().unwrap(), 0);
    let mut child = Command::new("cmd").args(["/C", "ping", "-n", "2", "127.0.0.1"]).spawn().unwrap();
    // Not assigned: the job stays empty, which proves counting is per job.
    assert_eq!(job.active_processes().unwrap(), 0);
    child.wait().unwrap();
    assert!(Job::create(&name).is_err(), "names are not silently shared");
    drop(job);
    drop(reopened);
    assert!(Job::open(&name).unwrap().is_none());
}

#[test]
fn pid_reuse_is_detected_by_creation_time() {
    let mut child = Command::new("cmd").args(["/C", "ping", "-n", "2", "127.0.0.1"]).spawn().unwrap();
    let identity = platform::child_identity(&child).unwrap();
    assert!(platform::process_alive(identity).unwrap());
    let forged = ProcessIdentity { created: identity.created + 1, ..identity };
    assert!(!platform::process_alive(forged).unwrap(), "same PID, other process");
    child.wait().unwrap();
    assert!(!platform::process_alive(identity).unwrap());
}

#[test]
fn user_and_session_are_known() {
    assert!(platform::current_user_sid().unwrap().starts_with("S-1-"));
    platform::current_session_id().unwrap();
}
