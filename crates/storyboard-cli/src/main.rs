use std::io::Write;

use clap::{Parser, error::ErrorKind};
use serde_json::json;
use storyboard_cli::{args::Cli, commands, ops_api};

fn main() {
    match Cli::try_parse() {
        Ok(cli) => {
            let pretty = cli.pretty;
            if let storyboard_cli::args::Command::Mcp(args) = cli.command {
                if let Err(error) = storyboard_cli::mcp::run(&args.workspace) {
                    let _ = writeln!(std::io::stderr(), "{}", ops_api::map_error(error));
                    std::process::exit(1);
                }
                return;
            }
            let is_show = matches!(cli.command, storyboard_cli::args::Command::Show(_));
            match commands::run(cli.command) {
                Ok(value) => print(value, pretty, is_show, 0),
                Err(error) => print(ops_api::map_error(error), pretty, false, 1),
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
        }
        Err(error) => {
            let message = error.to_string();
            let mapped = if write_verb_is_missing_base_version() {
                ops_api::ApiError::MissingBaseVersion
            } else {
                ops_api::ApiError::Usage { message }
            };
            print(ops_api::map_error(mapped), false, false, 2)
        }
    };
}

fn print(value: serde_json::Value, pretty: bool, is_show: bool, code: i32) {
    let encoded = if pretty && is_show {
        ops_api::pretty_show(&value)
    } else {
        let result = serde_json::to_string(&value);
        result.unwrap_or_else(|error| json!({"error": {"kind": "serialization", "message": error.to_string(), "hint": "report this error"}}).to_string())
    };
    let output = if code == 0 {
        &mut std::io::stdout() as &mut dyn Write
    } else {
        &mut std::io::stderr() as &mut dyn Write
    };
    let _ = writeln!(output, "{encoded}");
    if code != 0 {
        std::process::exit(code);
    }
}

fn write_verb_is_missing_base_version() -> bool {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let is_write_verb = args.iter().any(|arg| {
        matches!(
            arg.to_str(),
            Some(
                "add-scene"
                    | "add-shot"
                    | "add-panel"
                    | "add-text"
                    | "add-image"
                    | "add-take"
                    | "add-cut"
                    | "update-cut"
                    | "remove-cut"
                    | "add-clip"
                    | "update-clip"
                    | "remove-clip"
                    | "set-shot"
                    | "move-shot"
                    | "select-take"
                    | "trim"
                    | "undo"
                    | "note"
                    | "reply"
                    | "resolve"
            )
        )
    });
    is_write_verb
        && !args.iter().any(|arg| {
            arg == "--base-version"
                || arg
                    .to_str()
                    .is_some_and(|value| value.starts_with("--base-version="))
        })
}
