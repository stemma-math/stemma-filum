//! The `stemma-server` command: the program that runs an instance.

use std::process::ExitCode;

use stemma_core::PROTOCOL_VERSION;

/// The name the command reports itself under.
const NAME: &str = "stemma-server";

/// The version of this build.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {
            println!("{NAME} {VERSION}, speaking protocol {PROTOCOL_VERSION}.");
            println!("This build has no commands yet.");
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("{NAME} {VERSION} (protocol {PROTOCOL_VERSION})");
            ExitCode::SUCCESS
        }
        Some(argument) => {
            eprintln!("{NAME}: `{argument}` is not a command. This build has none yet.");
            ExitCode::from(2)
        }
    }
}
