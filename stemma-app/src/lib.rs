//! The desktop application's native side.
//!
//! Each command translates a request from the webview into the same
//! operations the `stemma` command line uses; repository rules, source
//! analysis and job launching stay in the client library, and the webview
//! only presents. Commands are grouped by area in the modules below.
//!
//! Every command is declared `async` so that Tauri runs it off the main
//! thread: they touch the network, run git and typeset documents, and none
//! of that may block the window.

mod account;
mod claims;
mod compose;
mod editing;
mod machine;
#[cfg(target_os = "macos")]
mod menu;
mod reviews;
mod vault;
mod workspace;

use stemma::home::{Environment, Home};

/// Starts the desktop application.
pub fn run() {
    let builder = tauri::Builder::default();

    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::chosen);

    builder
        .manage(account::SigningIn::default())
        .invoke_handler(tauri::generate_handler![
            account::whoami,
            account::sign_in_start,
            account::sign_in_poll,
            account::sign_out,
            account::open_link,
            workspace::sync,
            workspace::groups,
            workspace::managed_groups,
            workspace::lines,
            workspace::new_line,
            workspace::delete_line,
            workspace::share_line,
            editing::open_line,
            editing::save_line,
            editing::record_line,
            compose::preview,
            claims::claim,
            claims::release,
            claims::request_claim,
            claims::parted,
            claims::settle,
            machine::machine,
            machine::job_output,
            machine::offers,
            machine::launch,
            machine::node_init,
            machine::daemon_install,
            machine::install_cli,
            reviews::proposals,
            reviews::proposal,
            reviews::decide_proposal,
            reviews::apply_proposal,
            vault::vault,
            vault::vault_admit,
            vault::vault_bridge,
            vault::vault_audit,
        ])
        .run(tauri::generate_context!())
        .expect("the Stemma application could not start");
}

/// The client's home, as the command line would locate it.
fn home() -> Home {
    Home::locate(&Environment::from_process())
}

/// An error as the webview shows it.
fn said(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// An `anyhow` error with its whole chain of context.
fn failed(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/// Runs one of the client's commands and returns what it printed.
fn carried_out(
    work: impl FnOnce(&Home, &mut Vec<u8>) -> anyhow::Result<()>,
) -> Result<String, String> {
    let mut written = Vec::new();
    work(&home(), &mut written).map_err(failed)?;
    String::from_utf8(written).map_err(said)
}

/// The signed-in session: the server to talk to and the token to use.
struct Session {
    server: stemma::api::Server,
    token: String,
}

impl Session {
    fn open() -> Result<Self, String> {
        let credentials = stemma::session::required(&home()).map_err(said)?;
        Ok(Self {
            server: stemma::api::Server::at(&credentials.server).map_err(said)?,
            token: credentials.token,
        })
    }
}
