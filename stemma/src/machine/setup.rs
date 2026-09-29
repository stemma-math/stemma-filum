//! Setting this machine up, keeping it set up, and taking it apart.
//!
//! The application carries the command line inside its bundle, and the
//! bundle moves: it is dragged to another folder, or replaced by an
//! update. So nothing outside it names the bundle. `~/.local/bin/stemma`
//! is a link to the command it carries, the service runs that link, and
//! each launch of the application points the link at itself again.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::home::Home;
use crate::machine::daemon::Started;
use crate::machine::program;
use crate::machine::service::{self, Service};

/// The directory the command line is linked into, which needs no
/// administrator rights.
pub fn bin_directory() -> Result<PathBuf> {
    let home = dirs::home_dir().context("this account has no home directory")?;

    Ok(home.join(".local").join("bin"))
}

/// Where the command line is linked.
pub fn link_path() -> Result<PathBuf> {
    Ok(bin_directory()?.join(program::NAME))
}

/// What is where the command line is linked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Nothing.
    Missing,
    /// A link, to this.
    To(PathBuf),
    /// Something that is not a link.
    Other,
}

/// What is where the command line is linked.
pub fn link() -> Result<Link> {
    let path = link_path()?;

    Ok(match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Link::Missing,
        Err(error) => {
            return Err(error).with_context(|| format!("cannot look at {}", path.display()));
        }
        Ok(about) if about.file_type().is_symlink() => Link::To(
            std::fs::read_link(&path).with_context(|| format!("cannot read {}", path.display()))?,
        ),
        Ok(_) => Link::Other,
    })
}

/// Whether the link leads to `carried`.
pub fn links_to(target: &Path, carried: &Path) -> bool {
    match (target.canonicalize(), carried.canonicalize()) {
        (Ok(target), Ok(carried)) => target == carried,
        _ => false,
    }
}

/// Links the command line into [`bin_directory`], replacing whatever is
/// there.
pub fn link_to(carried: &Path) -> Result<PathBuf> {
    let directory = bin_directory()?;
    let placed = link_path()?;

    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;
    if std::fs::symlink_metadata(&placed).is_ok() {
        std::fs::remove_file(&placed)
            .with_context(|| format!("cannot replace {}", placed.display()))?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(carried, &placed)
        .with_context(|| format!("cannot link {}", placed.display()))?;
    #[cfg(not(unix))]
    std::fs::copy(carried, &placed)
        .with_context(|| format!("cannot write {}", placed.display()))?;

    Ok(placed)
}

/// Removes the link, and only a link: a program somebody put there
/// themselves is theirs. Answers whether there was one.
pub fn unlink() -> Result<bool> {
    if !matches!(link()?, Link::To(_)) {
        return Ok(false);
    }

    let path = link_path()?;
    std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;

    Ok(true)
}

/// The command line to name to something that outlives the application
/// bundle: the link, when it leads anywhere, and otherwise the command
/// line as [`program::command_line`] finds it.
pub fn stable_program() -> Result<PathBuf> {
    let link = link_path()?;
    if program::is_a_program(&link) {
        return Ok(link);
    }

    program::command_line()
}

/// A shell whose profile can put [`bin_directory`] on the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    /// zsh, the default on a Mac.
    Zsh,
    /// bash.
    Bash,
    /// fish.
    Fish,
}

/// The line a profile is given, and the comment above it that marks it
/// as Stemma's to remove.
const MARK: &str = "# Added by Stemma";

impl Shell {
    const ALL: [Self; 3] = [Self::Zsh, Self::Bash, Self::Fish];

    /// The shell this account logs in with, when it is one of these.
    pub fn login() -> Option<Self> {
        let shell = std::env::var_os("SHELL")?;

        match Path::new(&shell).file_name()?.to_str()? {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::Fish => "fish",
        }
    }

    /// The file it reads when a terminal opens.
    pub fn profile(self) -> Result<PathBuf> {
        let home = dirs::home_dir().context("this account has no home directory")?;
        let variable = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };

        Ok(match self {
            Self::Zsh => variable("ZDOTDIR").unwrap_or(home).join(".zshrc"),
            // A terminal on a Mac starts a login shell, which reads
            // `.bash_profile` and not `.bashrc`.
            Self::Bash if cfg!(target_os = "macos") => home.join(".bash_profile"),
            Self::Bash => home.join(".bashrc"),
            Self::Fish => variable("XDG_CONFIG_HOME")
                .unwrap_or_else(|| home.join(".config"))
                .join("fish")
                .join("config.fish"),
        })
    }

    /// The line that puts [`bin_directory`] on its path.
    pub fn line(self) -> &'static str {
        match self {
            Self::Zsh | Self::Bash => r#"export PATH="$HOME/.local/bin:$PATH""#,
            Self::Fish => "set -gx PATH $HOME/.local/bin $PATH",
        }
    }

    /// Whether a terminal opened now would find the command line.
    ///
    /// The application's own path is no answer: it was started by the
    /// system, not from a shell. So the shell is asked, as a terminal
    /// would start it, and when it cannot be asked the profile is read.
    pub fn finds_commands(self) -> bool {
        let Ok(directory) = bin_directory() else {
            return false;
        };

        match self.path() {
            Some(path) => std::env::split_paths(&path).any(|entry| entry == directory),
            None => self.profile().is_ok_and(|profile| has_line(&profile, self)),
        }
    }

    /// The path a terminal opened now would have.
    fn path(self) -> Option<String> {
        const SAID: &str = "__stemma_path__";

        let script = match self {
            Self::Fish => format!("printf '{SAID}%s\\n' (string join : $PATH)"),
            Self::Zsh | Self::Bash => format!("printf '{SAID}%s\\n' \"$PATH\""),
        };
        let shell = std::env::var_os("SHELL")?;
        let mut command = Command::new(shell);
        command.args(["-i", "-l", "-c", &script]);

        let said = said_within(&mut command, Duration::from_secs(5))?;
        said.lines()
            .find_map(|line| line.strip_prefix(SAID))
            .map(str::to_owned)
    }

    /// Adds [`Shell::line`] to its profile, unless it is there. Answers
    /// the profile.
    pub fn add_line(self) -> Result<PathBuf> {
        let profile = self.profile()?;
        if has_line(&profile, self) {
            return Ok(profile);
        }

        let mut text = match std::fs::read_to_string(&profile) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", profile.display()));
            }
        };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("{MARK}\n{}\n", self.line()));

        if let Some(directory) = profile.parent() {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("cannot create {}", directory.display()))?;
        }
        std::fs::write(&profile, text)
            .with_context(|| format!("cannot write {}", profile.display()))?;

        Ok(profile)
    }
}

/// Whether a profile has the line Stemma added.
fn has_line(profile: &Path, shell: Shell) -> bool {
    std::fs::read_to_string(profile).is_ok_and(|text| without_line(&text, shell).is_some())
}

/// A profile's text without the line Stemma added, or nothing when it
/// has none. What surrounds it is left as it was.
fn without_line(text: &str, shell: Shell) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at = lines
        .windows(2)
        .position(|pair| pair[0].trim_end() == MARK && pair[1].trim_end() == shell.line())?;

    // The blank line put before it goes with it.
    let from = match at.checked_sub(1) {
        Some(before) if lines[before].trim().is_empty() => before,
        _ => at,
    };

    let mut kept: String = lines[..from].concat();
    kept.push_str(&lines[at + 2..].concat());
    Some(kept)
}

/// Removes the line Stemma added from every profile that has it.
/// Answers the profiles it was removed from.
pub fn remove_lines() -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();

    for shell in Shell::ALL {
        let profile = shell.profile()?;
        let Ok(text) = std::fs::read_to_string(&profile) else {
            continue;
        };
        if let Some(kept) = without_line(&text, shell) {
            std::fs::write(&profile, kept)
                .with_context(|| format!("cannot write {}", profile.display()))?;
            changed.push(profile);
        }
    }

    Ok(changed)
}

/// What a program printed, if it finished within `limit`.
fn said_within(command: &mut Command, limit: Duration) -> Option<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Read beside the wait, so a program that says a lot does not block
    // on a full pipe.
    let mut stdout = child.stdout.take()?;
    let reading = std::thread::spawn(move || {
        let mut said = String::new();
        let _ = stdout.read_to_string(&mut said);
        said
    });

    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    reading.join().ok()
}

/// What repairing this machine at launch could not fix.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Repaired {
    /// Why the command line's link could not be fixed.
    pub link: Option<String>,
    /// Why the service could not be fixed.
    pub service: Option<String>,
}

/// Puts back what moving or updating the application broke, without
/// asking: the link points at `carried`, the command the application
/// carries; the service runs the link; and the daemon running is the
/// one the link leads to.
///
/// Nothing that was never set up is set up here: a machine whose owner
/// skipped the link or the service keeps skipping them.
pub fn repair(home: &Home, carried: &Path) -> Repaired {
    let mut repaired = Repaired::default();

    match link() {
        Ok(Link::To(target)) if !links_to(&target, carried) => {
            if let Err(error) = link_to(carried) {
                repaired.link = Some(format!("{error:#}"));
            }
        }
        Ok(_) => {}
        Err(error) => repaired.link = Some(format!("{error:#}")),
    }

    if let Err(error) = repair_service(home, carried) {
        repaired.service = Some(format!("{error:#}"));
    }

    repaired
}

/// The service's part of [`repair`].
fn repair_service(home: &Home, carried: &Path) -> Result<()> {
    if service::service(home) == Service::NotInstalled {
        return Ok(());
    }

    // The service runs the link, so a service needs one.
    if link()? == Link::Missing {
        link_to(carried)?;
    }

    let program = stable_program()?;
    let Some(path) = service::service_file(home)? else {
        return Ok(());
    };
    let wanted = service::service_text(home, &program);
    let written = std::fs::read_to_string(&path).ok();
    if written != wanted {
        service::put_in_place(home, &program)?;
        return Ok(());
    }

    match service::service(home) {
        Service::NotInstalled => Ok(()),
        Service::Stopped => service::start(home),
        Service::Running(pid) => {
            let program = program.canonicalize()?;
            let current =
                Started::read(home).is_some_and(|started| started.is_current(pid, &program));
            if current {
                return Ok(());
            }
            service::restart(home)
        }
    }
}

/// The service, as the checklist shows it.
pub fn service_standing(home: &Home) -> Result<Service> {
    if service::service_file(home)?.is_none() {
        bail!("this machine has no service manager Stemma can install into");
    }

    Ok(service::service(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_added_to_a_profile_comes_out_and_nothing_else_does() {
        let before = "alias ll='ls -l'\n";
        let added = format!("{before}\n{MARK}\n{}\n", Shell::Zsh.line());

        assert_eq!(without_line(&added, Shell::Zsh).as_deref(), Some(before));
        assert_eq!(without_line(before, Shell::Zsh), None);

        // Somebody's own line saying the same is theirs.
        let theirs = format!("{}\n", Shell::Zsh.line());
        assert_eq!(without_line(&theirs, Shell::Zsh), None);

        // Something they wrote after it stays.
        let after = format!("{MARK}\n{}\nexport EDITOR=vi\n", Shell::Zsh.line());
        assert_eq!(
            without_line(&after, Shell::Zsh).as_deref(),
            Some("export EDITOR=vi\n")
        );
    }

    #[test]
    fn a_program_that_does_not_finish_in_time_is_stopped() {
        let mut quick = Command::new("sh");
        quick.args(["-c", "echo here"]);
        assert_eq!(
            said_within(&mut quick, Duration::from_secs(5)).as_deref(),
            Some("here\n")
        );

        let mut slow = Command::new("sh");
        slow.args(["-c", "sleep 5"]);
        assert_eq!(said_within(&mut slow, Duration::from_millis(100)), None);
    }
}
