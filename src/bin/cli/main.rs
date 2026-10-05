//! mogumogu-cli: a short-lived client of the owner process. It never writes
//! the database itself (F-13); every command goes through the local pipe.
mod args;
mod commands;
mod output;

use commands::{Action, HELP, View};
use mogumogu::config::Config;
use mogumogu::ipc::Client;
use mogumogu::service::Request;
use mogumogu::{Error, Result};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

struct Global {
    demo: bool,
    data_dir: Option<PathBuf>,
    json: bool,
    no_start: bool,
}

fn split_globals(mut args: Vec<String>) -> Result<(Global, Vec<String>)> {
    let mut global = Global { demo: false, data_dir: None, json: false, no_start: false };
    loop {
        match args.first().map(String::as_str) {
            Some("--demo") => global.demo = true,
            Some("--json") => global.json = true,
            Some("--no-start") => global.no_start = true,
            Some("--data-dir") => {
                if args.len() < 2 {
                    return Err(Error::invalid("--data-dir benötigt einen Pfad."));
                }
                global.data_dir = Some(PathBuf::from(args.remove(1)));
            }
            _ => break,
        }
        args.remove(0);
    }
    Ok((global, args))
}

/// Starts the owner (tray, no dashboard) next to this executable and waits
/// for its pipe. Never starts a second owner: the pipe is checked first.
fn ensure_owner(client: &Client, global: &Global) -> Result<()> {
    if client.owner_running() {
        return Ok(());
    }
    if global.no_start {
        return Err(Error::unsupported("Kein Besitzerprozess aktiv (--no-start)."));
    }
    let exe =
        std::env::current_exe()?.parent().map(|dir| dir.join("mogumogu.exe")).filter(|p| p.is_file()).ok_or_else(
            || Error::unsupported("mogumogu.exe wurde neben mogumogu-cli.exe nicht gefunden; bitte zuerst starten."),
        )?;
    let mut command = std::process::Command::new(exe);
    command.arg("--tray");
    if global.demo {
        command.arg("--demo");
    }
    if let Some(dir) = &global.data_dir {
        command.arg("--data-dir").arg(dir);
    }
    command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if client.owner_running() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(Error::unsupported("Besitzerprozess hat nicht rechtzeitig geantwortet."))
}

fn execute(global: &Global, action: Action) -> Result<(View, serde_json::Value)> {
    if let Action::Static(value) = action {
        return Ok((View::Json, value));
    }
    let config = Config::new(global.data_dir.clone(), global.demo)?;
    let mut client = Client::new(&config)?;
    match action {
        Action::OwnerStatus => {
            let running = client.owner_running();
            Ok((View::Json, serde_json::json!({ "owner_running": running })))
        }
        Action::OwnerStop => {
            if !client.owner_running() {
                return Ok((View::Json, serde_json::json!({ "owner_running": false })));
            }
            client.call(Request::Shutdown)?;
            Ok((View::Json, serde_json::json!({ "stopped": true })))
        }
        Action::Run { scratch_id, program, args } => {
            ensure_owner(&client, global)?;
            let report = mogumogu::sessions::run_managed(&mut client, scratch_id, &program, &args, global.json)?;
            Ok((View::Json, serde_json::to_value(report)?))
        }
        Action::SettingsSet { key, value } => {
            ensure_owner(&client, global)?;
            let mut settings = serde_json::from_value(client.call(Request::GetSettings)?)?;
            commands::apply_setting(&mut settings, &key, &value)?;
            Ok((View::Json, client.call(Request::SaveSettings { settings })?))
        }
        Action::Call(request, view) => {
            ensure_owner(&client, global)?;
            Ok((view, client.call(request)?))
        }
        Action::Help | Action::Static(_) => unreachable!("handled before"),
    }
}

fn exit_code(error: &Error) -> u8 {
    match error.code() {
        "invalid" | "json" => 2,
        "blocked" => 3,
        "not_found" => 4,
        "conflict" => 5,
        "unsupported" => 6,
        _ => 1,
    }
}

fn main() -> ExitCode {
    let raw: Result<Vec<String>> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string().map_err(|_| Error::invalid("Argument enthält nicht darstellbare Zeichen.")))
        .collect();
    let parsed = raw.and_then(split_globals);
    let json = parsed.as_ref().is_ok_and(|(g, _)| g.json);
    let result = parsed.and_then(|(global, args)| {
        let action = commands::parse(&args)?;
        if matches!(action, Action::Help) {
            print!("{HELP}");
            return Ok(None);
        }
        execute(&global, action).map(Some)
    });
    match result {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some((view, value))) => {
            println!("{}", if json { output::json_ok(&value) } else { output::human(view, &value) });
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                println!("{}", output::json_error(&error));
            } else {
                eprintln!("{error}");
            }
            ExitCode::from(exit_code(&error))
        }
    }
}
