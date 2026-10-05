//! Managed run (F-15), executed by the CLI process. Structured arguments
//! only (no shell string), TEMP/TMP routed to the session's temporary
//! folder for the child only, all descendants inside a named job object.
//! The job is created without KILL_ON_JOB_CLOSE: user work is never ended.
use crate::domain::ProcessIdentity;
use crate::platform::{self, Job};
use crate::{Error, Result};
use std::process::Command;
use std::time::{Duration, Instant};

const HEARTBEAT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(250);

/// What the owner hands to a starter for one managed session.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StartInfo {
    pub session_id: i64,
    pub temp_dir: String,
    pub results_dir: String,
    pub job_name: String,
}

/// Owner operations a starter needs; implemented over IPC by the CLI and
/// directly over `Core` in tests.
pub trait SessionControl {
    fn start(&mut self, scratch_id: i64, purpose: &str) -> Result<StartInfo>;
    fn attach(&mut self, session_id: i64, process: ProcessIdentity) -> Result<()>;
    fn heartbeat(&mut self, session_id: i64) -> Result<()>;
    fn report(&mut self, session_id: i64, exit_code: i64, remaining: i64) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RunReport {
    pub session_id: i64,
    pub exit_code: i64,
    /// Processes of the job still running after the main child ended.
    pub remaining_processes: i64,
}

/// Starts `program` as a managed run. With `stdout_to_stderr` the child's
/// standard output goes to stderr, keeping machine-readable stdout clean.
pub fn run_managed(
    control: &mut dyn SessionControl,
    scratch_id: i64,
    program: &str,
    args: &[String],
    stdout_to_stderr: bool,
) -> Result<RunReport> {
    if program.trim().is_empty() {
        return Err(Error::invalid("Programm fehlt. Aufruf: run --scratch ID -- PROGRAMM [ARGUMENTE]"));
    }
    let purpose =
        std::path::Path::new(program).file_name().map_or(program.into(), |n| n.to_string_lossy().into_owned());
    let info = control.start(scratch_id, &purpose)?;
    let job = Job::create(&info.job_name)?;
    // The starter joins the job first so the child inherits it atomically.
    job.assign_current_process()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .env("TEMP", &info.temp_dir)
        .env("TMP", &info.temp_dir)
        .env("MOGUMOGU_SESSION", info.session_id.to_string())
        .env("MOGUMOGU_RESULTS", &info.results_dir);
    if stdout_to_stderr {
        command.stdout(std::io::stderr());
    }
    let mut child =
        command.spawn().map_err(|e| Error::invalid(format!("Programm konnte nicht gestartet werden: {e}")))?;
    let identity = platform::child_identity(&child)?;
    control.attach(info.session_id, identity)?;
    let mut last_heartbeat = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if last_heartbeat.elapsed() >= HEARTBEAT {
            // A failed heartbeat is not fatal: the owner never releases a
            // session because of a missing heartbeat.
            let _ = control.heartbeat(info.session_id);
            last_heartbeat = Instant::now();
        }
        std::thread::sleep(POLL);
    };
    let exit_code = i64::from(status.code().unwrap_or(-1));
    // The starter itself is still in the job and is not counted.
    let remaining = i64::from(job.active_processes()?.saturating_sub(1));
    control.report(info.session_id, exit_code, remaining)?;
    Ok(RunReport { session_id: info.session_id, exit_code, remaining_processes: remaining })
}
