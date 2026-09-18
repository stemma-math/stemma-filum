//! The systemd unit.
//!
//! The unit is written by the installer rather than kept as a file in the
//! repository, because half of it depends on the machine: where the binary
//! landed, which account it runs as, and where its state lives.
//!
//! Directory creation and ownership are left to systemd's own
//! `StateDirectory=` and friends wherever the layout is the standard one, so
//! that there is one place keeping those permissions right instead of two.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The name of the unit an ordinary installation writes.
pub const UNIT_NAME: &str = "stemma-server.service";

/// Where systemd looks for units an administrator installed.
pub const UNIT_DIRECTORY: &str = "/etc/systemd/system";

/// The state directory systemd manages by itself, given `StateDirectory=`.
const MANAGED_STATE_DIR: &str = "/var/lib/stemma";

/// What the unit needs to know about this machine.
#[derive(Debug, Clone)]
pub struct UnitSettings {
    /// The binary to run.
    pub executable: std::path::PathBuf,
    /// The configuration file to run it against.
    pub config_path: std::path::PathBuf,
    /// The file holding secrets, if it is there.
    pub env_path: std::path::PathBuf,
    /// The account to run as.
    pub user: String,
    /// The state directory.
    pub data_dir: std::path::PathBuf,
}

impl UnitSettings {
    /// The unit file.
    pub fn render(&self) -> String {
        let executable = self.executable.display();
        let config_path = self.config_path.display();
        let env_path = self.env_path.display();
        let user = &self.user;

        // A standard layout is handed to systemd, which then creates the
        // directory and keeps its ownership right. Anywhere else, the
        // installer has already created it and the unit only has to be
        // allowed to write there.
        let state = if self.data_dir == Path::new(MANAGED_STATE_DIR) {
            "StateDirectory=stemma".to_owned()
        } else {
            format!("ReadWritePaths={}", self.data_dir.display())
        };

        format!(
            "[Unit]\n\
             Description=Stemma server\n\
             After=network-online.target\n\
             Wants=network-online.target\n\
             \n\
             [Service]\n\
             Type=exec\n\
             ExecStart={executable} --config {config_path} serve\n\
             User={user}\n\
             Group={user}\n\
             EnvironmentFile=-{env_path}\n\
             {state}\n\
             ConfigurationDirectory=stemma\n\
             CacheDirectory=stemma\n\
             RuntimeDirectory=stemma\n\
             Restart=on-failure\n\
             RestartSec=5s\n\
             \n\
             # The process serves HTTP and writes one directory. Everything\n\
             # else on the machine is out of its reach.\n\
             NoNewPrivileges=true\n\
             PrivateTmp=true\n\
             ProtectSystem=strict\n\
             ProtectHome=true\n\
             ProtectKernelTunables=true\n\
             ProtectControlGroups=true\n\
             RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX\n\
             RestrictSUIDSGID=true\n\
             LockPersonality=true\n\
             \n\
             [Install]\n\
             WantedBy=multi-user.target\n"
        )
    }

    /// Where the unit file goes.
    pub fn unit_path(&self) -> std::path::PathBuf {
        Path::new(UNIT_DIRECTORY).join(UNIT_NAME)
    }
}

/// Whether this machine is run by systemd.
pub fn is_available() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

/// Runs `systemctl` with the given arguments.
fn systemctl(arguments: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .args(arguments)
        .status()
        .with_context(|| format!("cannot run systemctl {}", arguments.join(" ")))?;

    if !status.success() {
        bail!("systemctl {} failed", arguments.join(" "));
    }

    Ok(())
}

/// Tells systemd to re-read its unit files.
pub fn reload() -> Result<()> {
    systemctl(&["daemon-reload"])
}

/// Starts the unit at boot, without starting it now.
pub fn enable() -> Result<()> {
    systemctl(&["enable", UNIT_NAME])
}

/// Stops the unit from starting at boot, and does not fail if it never did.
pub fn disable() -> Result<()> {
    let _ = systemctl(&["disable", UNIT_NAME]);
    Ok(())
}

/// Stops the unit, and does not fail if it was not running.
pub fn stop() -> Result<()> {
    let _ = systemctl(&["stop", UNIT_NAME]);
    Ok(())
}

/// What systemd says about the unit, in one word.
pub fn state() -> Option<String> {
    let output = Command::new("systemctl")
        .args(["is-active", UNIT_NAME])
        .output()
        .ok()?;

    let state = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!state.is_empty()).then_some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(data_dir: &str) -> UnitSettings {
        UnitSettings {
            executable: "/usr/local/bin/stemma-server".into(),
            config_path: "/etc/stemma/server.toml".into(),
            env_path: "/etc/stemma/server.env".into(),
            user: "stemma".to_owned(),
            data_dir: data_dir.into(),
        }
    }

    #[test]
    fn the_unit_runs_serve_in_the_foreground_under_the_service_account() {
        let unit = settings(MANAGED_STATE_DIR).render();

        assert!(unit.contains(
            "ExecStart=/usr/local/bin/stemma-server --config /etc/stemma/server.toml serve"
        ));
        assert!(unit.contains("Type=exec"));
        assert!(unit.contains("User=stemma"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn the_standard_layout_is_left_to_systemd() {
        let unit = settings(MANAGED_STATE_DIR).render();

        assert!(unit.contains("StateDirectory=stemma"));
        assert!(!unit.contains("ReadWritePaths="));
    }

    #[test]
    fn a_state_directory_elsewhere_is_granted_explicitly() {
        let unit = settings("/srv/stemma").render();

        assert!(unit.contains("ReadWritePaths=/srv/stemma"));
        assert!(!unit.contains("StateDirectory="));
    }

    #[test]
    fn a_missing_secrets_file_does_not_stop_the_unit() {
        // The leading dash is what makes the file optional, and an instance
        // with no secrets is the ordinary case.
        assert!(
            settings(MANAGED_STATE_DIR)
                .render()
                .contains("EnvironmentFile=-/etc/stemma/server.env")
        );
    }
}
