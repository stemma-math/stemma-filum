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
    if let Err(error) =
        Server::at(&credentials.server).and_then(|server| server.sign_out(&credentials.token))
    {
        writeln!(out, "The instance could not be told: {error:#}")?;
    }

    Credentials::forget(&path)?;
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
        .context("this machine is not signed in: run `stemma login`")
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
