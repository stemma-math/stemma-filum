//! The daemon as a service: the file that tells systemd or launchd to run
//! it, and starting, stopping and removing it.
//!
//! What is supervised is the command line, not whatever asked for it to
//! be installed, and the file names the command line by the link that
//! outlives the application bundle.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::home::{HOME_VARIABLE, Home};

/// What supervises this machine, where anything does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervisor {
    /// A Linux machine running systemd, with a user manager.
    Systemd,
    /// A Mac, with launchd.
    Launchd,
    /// Something else, which this cannot write a unit for.
    Unknown,
}

impl Supervisor {
    /// What is here.
    pub fn here() -> Self {
        if Path::new("/run/systemd/system").is_dir() {
            return Self::Systemd;
        }
        if cfg!(target_os = "macos") {
            return Self::Launchd;
        }

        Self::Unknown
    }
}

/// What the unit is called.
pub const UNIT_NAME: &str = "stemma.service";

/// What the launch agent is called.
pub const AGENT_NAME: &str = "org.stemma.daemon.plist";

/// What the launch agent is known to launchd as.
const LABEL: &str = "org.stemma.daemon";

/// Where the service stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Nothing is installed.
    NotInstalled,
    /// It is installed and not running.
    Stopped,
    /// It is running, as this process where the supervisor says which.
    Running(Option<u32>),
}

/// Puts the daemon under whatever supervises this machine, from the
/// command line.
pub fn install(home: &Home, out: &mut impl Write) -> Result<()> {
    if Supervisor::here() == Supervisor::Unknown {
        writeln!(
            out,
            "This machine is run by neither systemd nor launchd, so there is \
             nothing here to install into."
        )?;
        writeln!(out, "Run `stemma daemon run` under a service manager.")?;

        return Ok(());
    }

    let path = write_service(home, &super::setup::stable_program()?)?;
    writeln!(out, "Wrote {}.", path.display())?;

    // Failing to start it is not failing to install it: the file is
    // written, and starting it again is one command.
    match start(home) {
        Ok(()) => writeln!(
            out,
            "The daemon is running and will start with this machine."
        )?,
        Err(error) => writeln!(out, "The service is installed but not started: {error:#}")?,
    }

    Ok(())
}

/// Installs the service to run `executable`, and starts it.
pub fn put_in_place(home: &Home, executable: &Path) -> Result<PathBuf> {
    let path = write_service(home, executable)?;
    start(home)?;

    Ok(path)
}

/// Writes the service's file, for `executable` to be what it runs.
fn write_service(home: &Home, executable: &Path) -> Result<PathBuf> {
    // What is supervised is the command line, not whatever asked for it
    // to be installed. Asked from the application, `current_exe` would
    // put the application itself in the plist, and every login would
    // open a window.
    let (Some(path), Some(text)) = (service_file(home)?, service_text(home, executable)) else {
        bail!(
            "this machine is run by neither systemd nor launchd, so the daemon cannot be \
             installed as a service here"
        );
    };

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }
    std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;

    Ok(path)
}

/// Where the service's file is, where this machine has a supervisor.
pub fn service_file(_home: &Home) -> Result<Option<PathBuf>> {
    let home = || std::env::var_os("HOME").context("this account has no home directory");

    Ok(match Supervisor::here() {
        Supervisor::Systemd => Some(
            Path::new(&home()?)
                .join(".config/systemd/user")
                .join(UNIT_NAME),
        ),
        Supervisor::Launchd => Some(
            Path::new(&home()?)
                .join("Library/LaunchAgents")
                .join(AGENT_NAME),
        ),
        Supervisor::Unknown => None,
    })
}

/// What the service's file says when it runs `executable`.
pub fn service_text(home: &Home, executable: &Path) -> Option<String> {
    let moved = std::env::var(HOME_VARIABLE).ok();

    match Supervisor::here() {
        Supervisor::Systemd => Some(unit(executable, moved.as_deref())),
        Supervisor::Launchd => Some(plist(executable, home, moved.as_deref())),
        Supervisor::Unknown => None,
    }
}

/// Where the service stands.
pub fn service(home: &Home) -> Service {
    let installed = service_file(home)
        .ok()
        .flatten()
        .is_some_and(|path| path.is_file());
    if !installed {
        return Service::NotInstalled;
    }

    match Supervisor::here() {
        Supervisor::Launchd => {
            let Ok(said) = supervise("launchctl", &["print", &launchd_target()]) else {
                return Service::Stopped;
            };
            if !said.contains("state = running") {
                return Service::Stopped;
            }
            Service::Running(field(&said, "pid = "))
        }
        Supervisor::Systemd => {
            let Ok(said) = supervise(
                "systemctl",
                &[
                    "--user",
                    "show",
                    "--property=ActiveState,MainPID",
                    UNIT_NAME,
                ],
            ) else {
                return Service::Stopped;
            };
            if !said.contains("ActiveState=active") {
                return Service::Stopped;
            }
            Service::Running(field(&said, "MainPID=").filter(|pid| *pid != 0))
        }
        Supervisor::Unknown => Service::NotInstalled,
    }
}

/// The number after `name` on a line of what a supervisor said.
fn field(said: &str, name: &str) -> Option<u32> {
    said.lines()
        .find_map(|line| line.trim().strip_prefix(name))
        .and_then(|value| value.trim().parse().ok())
}

/// Loads the service as its file now says, and starts it.
pub fn start(home: &Home) -> Result<()> {
    match Supervisor::here() {
        Supervisor::Systemd => {
            supervise("systemctl", &["--user", "daemon-reload"])?;
            supervise("systemctl", &["--user", "enable", UNIT_NAME])?;
            supervise("systemctl", &["--user", "restart", UNIT_NAME])?;
        }
        Supervisor::Launchd => {
            let path = service_file(home)?
                .context("this machine has no launch agent to load")?
                .display()
                .to_string();

            // Whatever was loaded goes first, so the file is read again.
            // Unloading finishes after the command returns, so loading
            // is tried a few times.
            let _ = supervise("launchctl", &["bootout", &launchd_target()]);
            let mut tried = 0;
            loop {
                match supervise("launchctl", &["bootstrap", &launchd_domain(), &path]) {
                    Ok(_) => break,
                    Err(_) if tried < 10 => {
                        tried += 1;
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Supervisor::Unknown => bail!("this machine has no service manager to start the daemon"),
    }

    Ok(())
}

/// Stops the daemon and starts it again, as its file says.
pub fn restart(home: &Home) -> Result<()> {
    match Supervisor::here() {
        Supervisor::Launchd if service(home) != Service::Stopped => {
            supervise("launchctl", &["kickstart", "-k", &launchd_target()]).map(drop)
        }
        _ => start(home),
    }
}

/// Stops the service and removes it. Answers whether there was one.
pub fn remove(home: &Home) -> Result<bool> {
    let Some(path) = service_file(home)? else {
        return Ok(false);
    };
    if !path.is_file() {
        return Ok(false);
    }

    match Supervisor::here() {
        Supervisor::Systemd => {
            let _ = supervise("systemctl", &["--user", "disable", "--now", UNIT_NAME]);
        }
        Supervisor::Launchd => {
            let _ = supervise("launchctl", &["bootout", &launchd_target()]);
        }
        Supervisor::Unknown => {}
    }

    std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
    if Supervisor::here() == Supervisor::Systemd {
        let _ = supervise("systemctl", &["--user", "daemon-reload"]);
    }

    Ok(true)
}

/// Runs a supervisor's command, answering what it printed or why it
/// failed.
fn supervise(program: &str, arguments: &[&str]) -> Result<String> {
    let ran = Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("cannot run {program}"))?;

    if !ran.status.success() {
        let said = String::from_utf8_lossy(&ran.stderr);
        bail!(
            "`{program} {}` failed: {}",
            arguments.join(" "),
            said.trim()
        );
    }

    Ok(String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// The launchd domain of this account's session.
fn launchd_domain() -> String {
    format!("gui/{}", current_uid().unwrap_or_default())
}

/// The launch agent, as launchd names it.
fn launchd_target() -> String {
    format!("{}/{LABEL}", launchd_domain())
}

/// This account's number, for the commands that want it.
fn current_uid() -> Option<String> {
    let output = Command::new("id").arg("-u").output().ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The systemd user unit.
fn unit(executable: &Path, moved: Option<&str>) -> String {
    let moved = match moved {
        // A machine keeping its Stemma somewhere else has to keep it
        // there when systemd starts it, or the daemon comes up as
        // somebody else.
        Some(root) => format!(
            "Environment={}\n",
            systemd_value(&format!("{HOME_VARIABLE}={root}"))
        ),
        None => String::new(),
    };
    let path = format!(
        "Environment={}\n",
        systemd_value(&format!("PATH={}", programs_path()))
    );

    format!(
        "[Unit]\n\
         Description=Stemma daemon\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={} daemon run\n\
         {path}\
         {moved}\
         Restart=on-failure\n\
         RestartSec=10s\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        systemd_command_word(&executable.display().to_string())
    )
}

/// A value in a unit file, as systemd reads it back.
///
/// systemd splits on whitespace and reads `%` as the start of a
/// specifier, so a value with either in it has to be escaped or it says
/// something else. A plain value is written as it is.
fn systemd_value(value: &str) -> String {
    let escaped = value.replace('%', "%%");

    if escaped
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '"' | '\'' | '\\'))
    {
        format!("\"{}\"", escaped.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        escaped
    }
}

/// One word of a unit's command line, which also reads `$` as the start
/// of a variable.
fn systemd_command_word(word: &str) -> String {
    systemd_value(&word.replace('$', "$$"))
}

/// The path the daemon is given to find programs with.
///
/// A service is not started from anybody's shell, so it inherits a path
/// of four system directories and nothing the person has installed. An
/// agent sitting in `/opt/homebrew/bin` is then not merely unusable but
/// reported to the group as not installed, which is a lie about their
/// machine. So the places agents are actually installed into travel
/// with the service, and the agents themselves get a path they can run
/// the tools they need from.
fn programs_path() -> String {
    crate::job::agents::where_programs_are()
        .iter()
        .map(|place| place.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// The launch agent.
fn plist(executable: &Path, home: &Home, moved: Option<&str>) -> String {
    let log = xml_text(&home.state_dir().join("daemon.log").display().to_string());
    let moved = match moved {
        Some(root) => format!(
            "<key>{HOME_VARIABLE}</key><string>{}</string>",
            xml_text(root)
        ),
        None => String::new(),
    };
    let environment = format!(
        "    <key>EnvironmentVariables</key>\n    \
         <dict><key>PATH</key><string>{}</string>{moved}</dict>\n",
        xml_text(&programs_path())
    );

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n    \
         <key>Label</key><string>{LABEL}</string>\n    \
         <key>ProgramArguments</key>\n    \
         <array><string>{}</string><string>daemon</string><string>run</string></array>\n    \
         <key>RunAtLoad</key><true/>\n    \
         <key>KeepAlive</key><true/>\n    \
         <key>StandardOutPath</key><string>{log}</string>\n    \
         <key>StandardErrorPath</key><string>{log}</string>\n\
         {environment}\
         </dict>\n\
         </plist>\n",
        xml_text(&executable.display().to_string()),
    )
}

/// Text as it can stand inside an element of a property list.
fn xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> Home {
        Home::locate(&crate::home::Environment::from_pairs([(
            HOME_VARIABLE,
            "/tmp/stemma-daemon-test",
        )]))
    }

    #[test]
    fn the_unit_runs_the_daemon_and_comes_back_after_a_failure() {
        let written = unit(Path::new("/usr/local/bin/stemma"), None);

        assert!(written.contains("ExecStart=/usr/local/bin/stemma daemon run"));
        assert!(written.contains("Restart=on-failure"));
        assert!(written.contains("WantedBy=default.target"));
        assert!(!written.contains("STEMMA_HOME"));
    }

    #[test]
    fn a_machine_that_keeps_stemma_elsewhere_keeps_it_there_when_it_is_started() {
        let written = unit(Path::new("/usr/local/bin/stemma"), Some("/data/stemma"));
        assert!(written.contains("Environment=STEMMA_HOME=/data/stemma"));

        let agent = plist(
            Path::new("/usr/local/bin/stemma"),
            &home(),
            Some("/data/stemma"),
        );
        assert!(agent.contains("<key>STEMMA_HOME</key><string>/data/stemma</string>"));
    }

    #[test]
    fn the_service_carries_a_path_an_agent_can_be_found_on() {
        let agent = plist(Path::new("/usr/local/bin/stemma"), &home(), None);
        let service = unit(Path::new("/usr/local/bin/stemma"), None);

        // What the system would otherwise give it, and where nothing is.
        for written in [&agent, &service] {
            assert!(written.contains("PATH"), "the service carries no path");
            assert!(
                written.contains("/opt/homebrew/bin") || written.contains("/usr/local/bin"),
                "the path holds nowhere an agent is installed"
            );
        }
    }

    #[test]
    fn the_launch_agent_names_the_program_and_keeps_it_running() {
        let written = plist(Path::new("/usr/local/bin/stemma"), &home(), None);

        assert!(written.contains("<string>/usr/local/bin/stemma</string>"));
        assert!(written.contains("<key>KeepAlive</key><true/>"));
        assert!(written.contains("daemon.log"));
    }

    #[test]
    fn a_program_in_an_awkward_place_is_still_one_word_to_systemd() {
        let written = unit(Path::new("/home/ana/My Apps/50% off/stemma"), None);

        assert!(
            written.contains("ExecStart=\"/home/ana/My Apps/50%% off/stemma\" daemon run"),
            "{written}"
        );
        assert_eq!(systemd_command_word("/usr/bin/stemma"), "/usr/bin/stemma");
        assert_eq!(systemd_command_word("a\"b"), "\"a\\\"b\"");
        assert_eq!(systemd_command_word("$HOME"), "$$HOME");
        assert_eq!(systemd_value("PATH=$HOME/bin"), "PATH=$HOME/bin");
    }

    #[test]
    fn a_program_in_an_awkward_place_is_still_text_to_launchd() {
        let written = plist(Path::new("/Users/ana/R&D <tools>/stemma"), &home(), None);

        assert!(
            written.contains("<string>/Users/ana/R&amp;D &lt;tools&gt;/stemma</string>"),
            "{written}"
        );
    }
}
