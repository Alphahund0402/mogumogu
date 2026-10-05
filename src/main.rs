#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! mogumogu desktop: the owner process (tray + on-demand dashboard), or the
//! same owner without UI (`--headless`, for automation and CI).
mod desktop;

use mogumogu::config::Config;
use mogumogu::domain::Snapshot;
use mogumogu::ipc::Client;
use mogumogu::owner::{Owner, OwnerEvents};
use mogumogu::service::Request;
use serde_json::Value;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};

const USAGE: &str = "Aufruf: mogumogu [--tray | --show | --headless] [--demo] [--data-dir ABSOLUTER_PFAD]";

struct Options {
    data_dir: Option<PathBuf>,
    demo: bool,
    show: bool,
    headless: bool,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options { data_dir: None, demo: false, show: false, headless: false };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--demo") => options.demo = true,
            Some("--show") => options.show = true,
            Some("--tray") => options.show = false,
            Some("--headless") => options.headless = true,
            Some("--data-dir") => {
                options.data_dir = Some(PathBuf::from(args.next().ok_or("--data-dir benötigt einen Pfad.")?))
            }
            _ => return Err(USAGE.into()),
        }
    }
    Ok(options)
}

/// Headless owner: same core, IPC and watcher, ends on `owner stop`.
struct Headless(Mutex<Sender<()>>);

impl OwnerEvents for Headless {
    fn snapshot(&self, _: Snapshot) {}
    fn outcome(&self, _: &Request, _: &mogumogu::Result<Value>) {}
    fn busy(&self, _: bool) {}
    fn show_dashboard(&self) {}
    fn hide_dashboard(&self) {}
    fn shutdown(&self) {
        if let Ok(sender) = self.0.lock() {
            let _ = sender.send(());
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    let config = Config::new(options.data_dir.clone(), options.demo)?;
    std::fs::create_dir_all(&config.directory)?;
    // One owner per data directory. The OS releases the lock with the process.
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(config.instance_path())?;
    if lock.try_lock().is_err() {
        // Bring the running owner forward instead of starting a second writer.
        if !options.headless && Client::new(&config).and_then(|c| c.call(Request::ShowDashboard)).is_ok() {
            return Ok(());
        }
        return Err("mogumogu läuft bereits für dieses Datenverzeichnis. Bitte das Symbol im Windows-Tray öffnen; \
                    zum Moduswechsel zuerst über dessen Menü beenden."
            .into());
    }
    if options.headless {
        let (sender, receiver) = mpsc::channel();
        let owner = Owner::start(config, Headless(Mutex::new(sender)))?;
        let _ = receiver.recv();
        drop(owner);
    } else {
        desktop::run(config, options.show)?;
    }
    drop(lock);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mogumogu: {error}");
            mogumogu::platform::show_error("mogumogu", &error.to_string());
            ExitCode::FAILURE
        }
    }
}
