//! Windows primitives used by cleanup and IPC (AT-13).
#![cfg(windows)]

use mogumogu::config::Config;
use mogumogu::ipc::Client;
use mogumogu::platform::{self, DirHandle};
use std::ffi::OsStr;
use std::fs;
use std::time::Duration;

/// Cleanup moves a folder by path while it holds the folder's parent open.
/// That is only safe because a pinned parent cannot be renamed and Windows
/// refuses to rename a folder with open handles below it: no folder on the
/// way can be swapped for a junction, while the folder itself still moves.
#[test]
fn a_pinned_folder_holds_the_way_but_lets_its_children_move() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("project");
    fs::create_dir_all(base.join("web/node_modules/dep")).unwrap();
    let parent = {
        let root = DirHandle::open_root(&base, false).unwrap();
        root.open_dir_pinned(OsStr::new("web")).unwrap()
    };
    assert!(fs::rename(&base, temp.path().join("moved")).is_err(), "the folder above stays");
    assert!(fs::rename(base.join("web"), base.join("web-old")).is_err(), "the pinned parent stays");
    fs::rename(base.join("web/node_modules"), base.join("web/node_modules-moved")).unwrap();
    drop(parent);
    fs::rename(base.join("web"), base.join("web-old")).unwrap();

    // An ordinary handle does not pin its own folder.
    let root = DirHandle::open_root(&base, false).unwrap();
    let open = root.open_dir(OsStr::new("web-old")).unwrap();
    drop(root);
    fs::rename(base.join("web-old"), base.join("web")).unwrap();
    drop(open);
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
    // The client verifies the server while still connected; disconnecting
    // first would fail that check (ERROR_PIPE_NOT_CONNECTED), not the fix.
    assert!(client.join().unwrap().unwrap().is_some(), "the client got through");
    drop(connection);
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
