//! Removing Stemma from this machine.
//!
//! The steps are the same from the application and from the command
//! line, and each is here on its own so the application can ask its
//! questions between them: what was written reaches the server first;
//! then the service, the link and the profile line go; the server takes
//! the machine back; the credential is forgotten; and last, if the owner
//! says so, everything Stemma kept here.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::account::credentials::Credentials;
use crate::api::Server;
use crate::home::Home;
use crate::mirror::{Mirror, Standing};

/// Sends what can be sent, and answers the lines that are still not on
/// the server.
pub fn send(home: &Home) -> Vec<Standing> {
    if Credentials::read(&home.credentials_file())
        .ok()
        .flatten()
        .is_some()
    {
        let _ = crate::mirror::run(home, &mut std::io::sink());
    }

    crate::mirror::engine::unsent(home)
}

/// Stops and removes the service, the command line's link and the line
/// added to a shell profile. Answers what was done, in sentences.
pub fn take_down(home: &Home) -> Result<Vec<String>> {
    let mut done = Vec::new();

    if crate::machine::service::remove(home)? {
        done.push("The background service is stopped and removed.".to_owned());
    }
    if crate::machine::setup::unlink()? {
        done.push(format!(
            "{} is removed.",
            crate::machine::setup::link_path()?.display()
        ));
    }
    for profile in crate::machine::setup::remove_lines()? {
        done.push(format!(
            "The Stemma line is removed from {}.",
            profile.display()
        ));
    }

    Ok(done)
}

/// Takes this machine back on the server and forgets the credential.
/// Answers what was done, in sentences.
///
/// A server out of reach does not keep the credential here: the machine
/// can still be taken back from its page on the web.
pub fn leave(home: &Home) -> Result<Vec<String>> {
    let path = home.credentials_file();
    let Some(credentials) = Credentials::read(&path)? else {
        return Ok(vec!["This machine was not signed in.".to_owned()]);
    };

    let mut done = Vec::new();
    let told = Server::at(&credentials.server).and_then(|server| {
        let retired = server.retire_node(&credentials.token)?;
        // Taking the machine back revokes its credential too; one that
        // never offered to work gives it back itself.
        if !retired {
            server.sign_out(&credentials.token)?;
        }
        Ok(retired)
    });

    match told {
        Ok(true) => done.push(format!(
            "{} no longer counts this machine among yours.",
            credentials.server
        )),
        Ok(false) => {}
        Err(error) => done.push(format!(
            "{} could not be told, so the machine is still listed there: {error:#}",
            credentials.server
        )),
    }

    Credentials::forget(&path)?;
    done.push(format!(
        "This machine is signed out of {}.",
        credentials.server
    ));

    Ok(done)
}

/// Everything Stemma keeps on this machine that is there now: the lines'
/// repositories, state, caches, configuration and job workspaces.
pub fn local_data(home: &Home) -> Vec<PathBuf> {
    let mut found = Vec::new();

    // The repositories are where the configuration says, so they are
    // found before the configuration goes.
    if let Ok(mirror) = Mirror::at(home)
        && let Ok(repositories) = mirror.repositories()
    {
        for (group, side) in repositories {
            if let Some(directory) = mirror.repository(&group, side).parent() {
                found.push(directory.to_owned());
            }
        }
    }

    found.extend([
        home.state_dir().to_owned(),
        home.cache_dir().to_owned(),
        home.config_dir().to_owned(),
        std::env::temp_dir().join("stemma-jobs"),
    ]);

    found.sort();
    found.dedup();
    found.retain(|path| path.exists());
    found
}

/// Deletes everything [`local_data`] names, and the workspace when that
/// leaves it empty.
pub fn delete_data(home: &Home) -> Result<()> {
    let workspace = crate::config::Config::read(&home.config_file())
        .map(|config| config.workspace_or(home.workspace()))
        .unwrap_or_else(|_| home.workspace().to_owned());

    for path in local_data(home) {
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("cannot delete {}", path.display()))?;
    }

    // Only when empty: the workspace may be a folder somebody chose, with
    // other things in it.
    let _ = std::fs::remove_dir(&workspace);

    Ok(())
}

/// What `stemma uninstall` was told in advance.
#[derive(Debug, Clone, Copy, Default)]
pub struct Told {
    /// Remove even if some lines are not on the server yet.
    pub discard_unsent: bool,
    /// Delete what Stemma kept here: yes, no, or ask.
    pub delete_data: Option<bool>,
}

/// Removes Stemma from this machine, from the command line.
///
/// Whatever was not decided in advance is asked when there is somebody
/// at a terminal to ask, and otherwise answered the careful way.
pub fn run(
    home: &Home,
    told: Told,
    asking: Option<&mut dyn BufRead>,
    out: &mut impl Write,
) -> Result<()> {
    let mut asking = asking;

    writeln!(out, "Sending what is not on the server yet...")?;
    out.flush()?;
    let unsent = send(home);
    if !unsent.is_empty() && !told.discard_unsent {
        writeln!(out, "These lines are not on the server yet:")?;
        for line in &unsent {
            writeln!(out, "  {}/{}/{}", line.group, line.side.as_str(), line.name)?;
        }
        let discard = match asking.as_deref_mut() {
            Some(input) => ask(input, out, "Remove Stemma anyway and lose them?")?,
            None => false,
        };
        if !discard {
            bail!(
                "nothing was removed. Send them with `stemma sync`, or pass \
                 --discard-unsent to lose them"
            );
        }
    }

    for said in take_down(home)? {
        writeln!(out, "{said}")?;
    }
    for said in leave(home)? {
        writeln!(out, "{said}")?;
    }

    settle_data(home, told, asking, out)
}

/// Deletes what Stemma kept here, or keeps it, as told or answered.
fn settle_data(
    home: &Home,
    told: Told,
    asking: Option<&mut dyn BufRead>,
    out: &mut impl Write,
) -> Result<()> {
    let data = local_data(home);
    if data.is_empty() {
        return Ok(());
    }

    if deletes(&data, told, asking, out)? {
        delete_data(home)?;
        writeln!(out, "Everything Stemma kept here is deleted.")?;
    } else {
        writeln!(out, "What Stemma kept here stays:")?;
        for path in &data {
            writeln!(out, "  {}", path.display())?;
        }
    }

    Ok(())
}

/// Whether to delete `data`: as told, as answered, or not.
fn deletes(
    data: &[PathBuf],
    told: Told,
    asking: Option<&mut dyn BufRead>,
    out: &mut impl Write,
) -> Result<bool> {
    match (told.delete_data, asking) {
        (Some(decided), _) => Ok(decided),
        (None, Some(input)) => {
            writeln!(out, "Stemma keeps these on this machine:")?;
            for path in data {
                writeln!(out, "  {}", path.display())?;
            }
            ask(input, out, "Delete them?")
        }
        (None, None) => Ok(false),
    }
}

/// Asks a yes-or-no question; anything but yes is no.
fn ask(input: &mut dyn BufRead, out: &mut impl Write, question: &str) -> Result<bool> {
    write!(out, "{question} [y/N] ")?;
    out.flush()?;

    let mut answer = String::new();
    input.read_line(&mut answer)?;

    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Whether a path is inside the application bundle `running` is in,
/// which is where the application can be moved to the Trash from.
pub fn bundle_of(running: &Path) -> Option<PathBuf> {
    running
        .ancestors()
        .find(|ancestor| {
            ancestor
                .extension()
                .is_some_and(|extension| extension == "app")
        })
        .map(Path::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> Home {
        let root =
            std::env::temp_dir().join(format!("stemma-uninstall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            root.display().to_string(),
        )]))
    }

    #[test]
    fn a_machine_never_signed_in_has_nothing_to_leave() {
        let home = home("never");

        assert_eq!(
            leave(&home).unwrap(),
            vec!["This machine was not signed in.".to_owned()]
        );
    }

    #[test]
    fn local_data_is_deleted_only_when_the_owner_says_so() {
        let data = [PathBuf::from("/somewhere/stemma")];
        let mut out = Vec::new();

        // Nobody at a terminal, and nothing said: it stays.
        assert!(!deletes(&data, Told::default(), None, &mut out).unwrap());

        let told = Told {
            delete_data: Some(true),
            ..Told::default()
        };
        assert!(deletes(&data, told, None, &mut out).unwrap());

        let mut answer: &[u8] = b"\n";
        assert!(!deletes(&data, Told::default(), Some(&mut answer), &mut out).unwrap());
        let mut answer: &[u8] = b"yes\n";
        assert!(deletes(&data, Told::default(), Some(&mut answer), &mut out).unwrap());
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("/somewhere/stemma")
        );
    }

    #[test]
    fn what_stemma_keeps_is_found_where_it_is() {
        let home = home("data");
        std::fs::create_dir_all(home.state_dir()).unwrap();

        let listed = local_data(&home);
        assert!(listed.contains(&home.state_dir().to_owned()));
        assert!(!listed.contains(&home.cache_dir().to_owned()));
    }

    #[test]
    fn the_bundle_is_found_from_the_program_inside_it() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Stemma.app/Contents/MacOS/stemma-app"
            )),
            Some(PathBuf::from("/Applications/Stemma.app"))
        );
        assert_eq!(bundle_of(Path::new("/usr/local/bin/stemma")), None);
    }
}
