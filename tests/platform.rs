//! Windows primitives used by sessions and IPC (AT-10, AT-13).
#![cfg(windows)]

use mogumogu::config::Config;
use mogumogu::domain::ProcessIdentity;
use mogumogu::ipc::Client;
use mogumogu::platform::{self, Job};
use std::process::Command;
use std::time::Duration;

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

/// The CLI's liveness probe connects and closes at once. When it does so
/// before the owner's first ConnectNamedPipe, the single pipe instance must
/// still serve the next client instead of answering "busy" forever.
#[test]
fn a_client_that_leaves_before_the_first_accept_does_not_wedge_the_pipe() {
    let name = format!(r"\\.\pipe\mogumogu-test-early-{}", std::process::id());
    let listener = platform::PipeListener::bind(&name).unwrap();
    drop(platform::pipe_connect(&name, Duration::from_secs(2)).unwrap().expect("instance listens after bind"));
    assert!(listener.accept().is_err(), "the vanished client is reported, not served");
    let client = {
        let name = name.clone();
        std::thread::spawn(move || platform::pipe_connect(&name, Duration::from_secs(5)))
    };
    let connection = listener.accept().expect("the next client is served");
    drop(connection);
    assert!(client.join().unwrap().unwrap().is_some(), "the client got through");
}

/// A busy pipe proves a running owner: it is serving another request.
/// Treating it as "no owner" made the CLI start a second one and fail.
#[test]
fn a_busy_owner_counts_as_running() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::new(Some(dir.path().to_path_buf()), false).unwrap();
    let name = config.pipe_name().unwrap();
    let _listener = platform::PipeListener::bind(&name).unwrap();
    // The only instance is taken by a client the owner is still serving.
    let _held = platform::pipe_connect(&name, Duration::from_secs(2)).unwrap().expect("instance listens");
    assert!(Client::new(&config).unwrap().owner_running());
}
