//! Signing in and out, and opening links in the browser.

use std::sync::Mutex;

use serde::Serialize;
use stemma::account::session::Collected;

use crate::{carried_out, failed, home, said};

/// Who this machine acts for.
#[derive(Serialize)]
pub struct Signed {
    server: String,
    user: String,
}

/// A sign-in waiting for somebody to approve it in their browser.
#[derive(Serialize)]
pub struct Beginning {
    code: String,
    url: String,
    url_complete: String,
    interval: u32,
}

/// The sign-in this window is in the middle of, if any.
#[derive(Default)]
pub struct SigningIn(Mutex<Option<stemma::account::session::Beginning>>);

/// Who this machine acts for, when anybody signed it in.
#[tauri::command(async)]
pub fn whoami() -> Result<Option<Signed>, String> {
    let credentials = stemma::account::credentials::Credentials::read(&home().credentials_file())
        .map_err(failed)?;

    Ok(credentials.map(|credentials| Signed {
        server: credentials.server,
        user: credentials.user,
    }))
}

/// Reads the account's appearance from the instance when reachable.
#[tauri::command(async)]
pub fn appearance() -> Result<Option<String>, String> {
    let credentials = stemma::account::session::required(&home()).map_err(failed)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(failed)?;
    let account = server
        .whoami(&credentials.token)
        .map_err(failed)?
        .ok_or_else(|| "This machine is no longer signed in.".to_owned())?;
    Ok(account.appearance)
}

/// Saves the account's appearance for the app and the web.
#[tauri::command(async)]
pub fn set_appearance(appearance: String) -> Result<(), String> {
    if !matches!(appearance.as_str(), "system" | "light" | "dark") {
        return Err("Choose System, Light or Dark.".to_owned());
    }
    let credentials = stemma::account::session::required(&home()).map_err(failed)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(failed)?;
    server
        .set_appearance(&credentials.token, &appearance)
        .map_err(failed)
}

/// Starts the device flow against an instance.
#[tauri::command(async)]
pub fn sign_in_start(
    server: String,
    signing_in: tauri::State<'_, SigningIn>,
) -> Result<Beginning, String> {
    let beginning = stemma::account::session::begin(server.trim()).map_err(failed)?;
    let answer = Beginning {
        code: beginning.sign_in.user_code.clone(),
        url: beginning.sign_in.verification_uri.clone(),
        url_complete: beginning.sign_in.verification_uri_complete.clone(),
        interval: beginning.sign_in.interval.max(1),
    };

    *signing_in.0.lock().map_err(said)? = Some(beginning);

    Ok(answer)
}

/// Asks once whether the pending sign-in was approved. Answers `nothing`,
/// `waiting`, `signed`, `refused` or `ran_out`.
#[tauri::command(async)]
pub fn sign_in_poll(signing_in: tauri::State<'_, SigningIn>) -> Result<&'static str, String> {
    let Some(beginning) = signing_in.0.lock().map_err(said)?.clone() else {
        return Ok("nothing");
    };

    Ok(
        match stemma::account::session::collect(&home(), &beginning).map_err(failed)? {
            Collected::Waiting => "waiting",
            Collected::Signed(_) => {
                *signing_in.0.lock().map_err(said)? = None;
                "signed"
            }
            Collected::Refused => "refused",
            Collected::RanOut => "ran_out",
        },
    )
}

/// Revokes the credential and forgets it.
#[tauri::command(async)]
pub fn sign_out() -> Result<String, String> {
    carried_out(stemma::account::session::log_out)
}

/// Opens an HTTP(S) link in the system browser, where the person can see
/// the address they are approving a sign-in at.
#[tauri::command(async)]
pub fn open_link(url: String) -> Result<(), String> {
    let url = browser_url(&url)?;
    let opener = match std::env::consts::OS {
        "macos" => "open",
        "windows" => "explorer",
        _ => "xdg-open",
    };

    let status = std::process::Command::new(opener)
        .arg(url.as_str())
        .status()
        .map_err(|error| format!("cannot open {url}: {error}"))?;
    if !status.success() {
        return Err(format!("cannot open {url}: {opener} exited with {status}"));
    }
    Ok(())
}

/// Accepts only absolute HTTP(S) URLs, so the opener is never handed a
/// file, a custom scheme or something it would read as a flag.
fn browser_url(given: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(given).map_err(said)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only HTTP and HTTPS browser links can be opened".to_owned());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::browser_url;

    #[test]
    fn browser_links_cannot_open_local_files_or_custom_handlers() {
        assert!(browser_url("https://example.org/approve?code=abc").is_ok());
        assert!(browser_url("http://localhost:8080/approve").is_ok());
        for url in [
            "file:///tmp/program",
            "javascript:alert(1)",
            "custom:run",
            "--help",
        ] {
            assert!(browser_url(url).is_err(), "{url}");
        }
    }
}
