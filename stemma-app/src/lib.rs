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
mod engine;
mod links;
mod live;
mod machine;
#[cfg(target_os = "macos")]
mod menu;
mod reviews;
mod setup;
mod vault;
mod workspace;

use stemma::home::{Environment, Home};

/// Starts the desktop application.
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_deep_link::init());

    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::chosen);

    builder
        .manage(account::SigningIn::default())
        .manage(links::PendingReview::default())
        .setup(|app| {
            links::listen(app.handle())?;
            engine::start(app.handle());
            live::start(app.handle());
            setup::repair_at_launch(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            account::whoami,
            account::sign_in_start,
            account::sign_in_poll,
            account::sign_out,
            account::open_link,
            workspace::standing,
            workspace::send_now,
            workspace::groups,
            workspace::managed_groups,
            workspace::lines,
            workspace::new_line,
            workspace::delete_line,
            workspace::share_line,
            editing::open_line,
            editing::save_line,
            compose::preview,
            compose::review_page,
            claims::claim,
            claims::claim_status,
            claims::release,
            claims::request_claim,
            claims::parted,
            claims::settle,
            machine::machine,
            machine::job_output,
            machine::cancel_job,
            machine::offers,
            machine::launch,
            machine::node_init,
            setup::setup_survey,
            setup::setup_service,
            setup::setup_link,
            setup::setup_path,
            setup::try_agent,
            setup::removal_send,
            setup::removal_leave,
            setup::removal_data,
            setup::removal_delete_data,
            setup::can_trash,
            setup::trash_and_quit,
            setup::quit,
            reviews::proposals,
            reviews::proposal,
            reviews::decide_proposal,
            reviews::apply_proposal,
            reviews::handoff_proposal,
            reviews::line_history,
            links::take_open_review,
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

/// An error as the webview shows it, for errors that carry no context:
/// reading a file, parsing what the webview sent.
fn said(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// An error from the client library, with its whole chain of context.
///
/// An error with a remedy loses the command the command line would name,
/// and is marked instead as `remedy:<slug>:`, which the webview turns into
/// the button that does the same.
fn failed(error: impl Into<anyhow::Error>) -> String {
    let error = error.into();
    let Some(remedy) = stemma::remedy::Remedy::of(&error) else {
        return format!("{error:#}");
    };

    let said: Vec<String> = error
        .chain()
        .filter(|cause| !cause.is::<stemma::remedy::Remedy>())
        .map(ToString::to_string)
        .collect();
    format!("remedy:{}:{}", remedy.slug(), said.join(": "))
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
        let credentials = stemma::account::session::required(&home()).map_err(failed)?;
        Ok(Self {
            server: stemma::api::Server::at(&credentials.server).map_err(failed)?,
            token: credentials.token,
        })
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use stemma::remedy::Remedy;

    use super::failed;

    #[test]
    fn an_error_with_a_remedy_names_its_button_and_not_a_command() {
        let error = Err::<(), _>(Remedy::SetUpMachine)
            .context("stemma.example.org does not know this machine")
            .unwrap_err();

        assert_eq!(
            failed(error),
            "remedy:set-up:stemma.example.org does not know this machine"
        );
        assert_eq!(failed(anyhow::anyhow!("out of reach")), "out of reach");
    }

    #[test]
    fn an_error_keeps_its_whole_chain_of_context() {
        let error = Err::<(), _>(anyhow::anyhow!("connection refused"))
            .context("cannot reach stemma.example.org")
            .unwrap_err();

        assert_eq!(
            failed(error),
            "cannot reach stemma.example.org: connection refused"
        );
    }
}
