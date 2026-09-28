//! Signing this machine in and out.
//!
//! Stemma does nothing for somebody it does not know: every command that
//! touches an instance goes through the credential kept here, and there is
//! exactly one way to get one.

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::api::{Answer, Server};
use crate::credentials::Credentials;
use crate::home::Home;
use crate::remedy::Remedy;

/// What this machine calls itself when it asks to be let in.
///
/// The person approving is looking at a list of things asking for their
/// account, so it has to be something they recognise.
pub fn machine_label() -> String {
    let name = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());

    match name {
        Some(name) => format!("stemma on {name}"),
        None => "stemma on an unnamed machine".to_owned(),
    }
}

/// A sign-in somebody has to approve before it means anything.
///
/// Kept apart from the waiting so that a window can show the code and
/// stay answerable meanwhile. The command line does the same thing with
/// a loop around it.
#[derive(Debug, Clone)]
pub struct Beginning {
    /// The instance being signed in to.
    pub address: String,
    /// What the instance handed this machine.
    pub sign_in: crate::api::SignIn,
}

/// What asking the instance again came back with.
#[derive(Debug, Clone)]
pub enum Collected {
    /// Nobody has answered yet.
    Waiting,
    /// Somebody said yes, and the credential is kept.
    Signed(Credentials),
    /// Somebody said no.
    Refused,
    /// Nobody answered in time.
    RanOut,
}

/// Asks an instance to let this machine in.
pub fn begin(address: &str) -> Result<Beginning> {
    let server = Server::at(address)?;

    Ok(Beginning {
        address: server.address().to_owned(),
        sign_in: server.start_sign_in(&machine_label())?,
    })
}

/// Asks once whether somebody has answered, and keeps the credential
/// when they have.
pub fn collect(home: &Home, beginning: &Beginning) -> Result<Collected> {
    let server = Server::at(&beginning.address)?;

    Ok(
        match server.collect_sign_in(&beginning.sign_in.device_code)? {
            Answer::Waiting => Collected::Waiting,
            Answer::Refused => Collected::Refused,
            Answer::RanOut => Collected::RanOut,
            Answer::Admitted { token, user } => {
                let credentials = Credentials {
                    server: server.address().to_owned(),
                    user,
                    token,
                };

                credentials.write(&home.credentials_file())?;

                Collected::Signed(credentials)
            }
        },
    )
}

/// Signs this machine in, waiting for somebody to approve it.
pub fn log_in(home: &Home, address: &str, out: &mut impl Write) -> Result<Credentials> {
    let server = Server::at(address)?;
    let sign_in = server.start_sign_in(&machine_label())?;

    writeln!(out, "Open this and type the code:")?;
    writeln!(out)?;
    writeln!(out, "  {}", sign_in.verification_uri)?;
    writeln!(out, "  {}", sign_in.user_code)?;
    writeln!(out)?;
    writeln!(
        out,
        "Or open {} directly.",
        sign_in.verification_uri_complete
    )?;
    writeln!(out, "Waiting...")?;
    out.flush()?;

    let credentials = wait_for_answer(&server, &sign_in, out)?;
    credentials.write(&home.credentials_file())?;

    writeln!(out, "Signed in as {}.", credentials.user)?;

    Ok(credentials)
}

/// Asks the instance, at the pace it asked for, until somebody answers.
fn wait_for_answer(
    server: &Server,
    sign_in: &crate::api::SignIn,
    out: &mut impl Write,
) -> Result<Credentials> {
    let interval = Duration::from_secs(u64::from(sign_in.interval.max(1)));
    let deadline = Instant::now() + Duration::from_secs(u64::from(sign_in.expires_in));

    loop {
        match server.collect_sign_in(&sign_in.device_code)? {
            Answer::Admitted { token, user } => {
                return Ok(Credentials {
                    server: server.address().to_owned(),
                    user,
                    token,
                });
            }
            Answer::Refused => bail!("that sign-in was refused"),
            Answer::RanOut => bail!("nobody answered in time; run `stemma login` again"),
            Answer::Waiting => {}
        }

        if Instant::now() >= deadline {
            let _ = writeln!(out);
            bail!("nobody answered in time; run `stemma login` again");
        }

        std::thread::sleep(interval);
    }
}

/// Gives the credential back and forgets it.
pub fn log_out(home: &Home, out: &mut impl Write) -> Result<()> {
    let path = home.credentials_file();

    let Some(credentials) = Credentials::read(&path)? else {
        writeln!(out, "This machine is not signed in.")?;
        return Ok(());
    };

    // The local copy goes whatever the instance says: a machine that
    // cannot reach its instance must still be able to stop holding a
    // credential.
    let revocation =
        Server::at(&credentials.server).and_then(|server| server.sign_out(&credentials.token));
    Credentials::forget(&path)?;
    if let Err(error) = revocation {
        writeln!(out, "The server could not be notified: {error:#}")?;
    }

    writeln!(out, "This machine is no longer signed in.")?;

    Ok(())
}

/// Says who this machine acts for, asking the instance rather than
/// trusting the file.
pub fn whoami(home: &Home, out: &mut impl Write) -> Result<bool> {
    let Some(credentials) = Credentials::read(&home.credentials_file())? else {
        writeln!(out, "This machine is not signed in. Run `stemma login`.")?;
        return Ok(false);
    };

    let server = Server::at(&credentials.server)?;

    match server.whoami(&credentials.token)? {
        Some(who) => {
            writeln!(out, "{} ({})", who.user, who.display_name)?;
            writeln!(out, "on {}", credentials.server)?;
            Ok(true)
        }
        None => {
            writeln!(
                out,
                "The credential this machine holds is no longer accepted by {}.",
                credentials.server
            )?;
            writeln!(out, "Run `stemma login` again.")?;
            Ok(false)
        }
    }
}

/// The credential this machine holds, for the commands that need one.
pub fn required(home: &Home) -> Result<Credentials> {
    Credentials::read(&home.credentials_file())?
        .ok_or(Remedy::SignIn)
        .context("this machine is not signed in")
}

/// The credential this machine holds and the instance it is for.
pub fn connect(home: &Home) -> Result<(Credentials, crate::api::Server)> {
    let credentials = required(home)?;
    let server = crate::api::Server::at(&credentials.server)?;
    Ok((credentials, server))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_names_itself_after_the_machine() {
        let label = machine_label();

        assert!(label.starts_with("stemma on "));
        assert!(!label.ends_with("on "));
    }

    #[test]
    fn a_machine_with_no_credential_is_told_what_to_run() {
        let home = crate::home::Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            "/nonexistent/stemma",
        )]));

        let mut out = Vec::new();
        assert!(!whoami(&home, &mut out).unwrap());
        assert!(String::from_utf8(out).unwrap().contains("stemma login"));

        assert!(required(&home).is_err());
    }
}
