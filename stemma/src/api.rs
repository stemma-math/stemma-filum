//! Talking to an instance.
//!
//! Signing in uses the device authorization flow: this program asks for a
//! code, the person types it into a browser they are already signed in on,
//! and only then does a credential come back here. Nothing has to reach
//! this machine's own address, which is what makes it work over SSH and
//! inside a container.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use stemma_core::PROTOCOL_VERSION;
use stemma_core::protocol::ProtocolVersion;

/// The header both sides announce their protocol in.
const PROTOCOL_HEADER: &str = stemma_core::PROTOCOL_HEADER;

/// How long to wait on a request before giving up.
const TIMEOUT: Duration = Duration::from_secs(20);

/// An instance, as this program talks to it.
pub struct Server {
    address: String,
    agent: ureq::Agent,
}

/// What an instance says when a machine asks to sign in.
#[derive(Debug, Clone, Deserialize)]
pub struct SignIn {
    /// The secret this machine keeps.
    pub device_code: String,
    /// The code the person reads and types.
    pub user_code: String,
    /// Where they go to type it.
    pub verification_uri: String,
    /// The same place, with the code already in it.
    pub verification_uri_complete: String,
    /// How long they have, in seconds.
    pub expires_in: u32,
    /// How often to ask again, in seconds.
    pub interval: u32,
}

/// What an instance says when the machine asks whether it may come in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Nobody has answered yet.
    Waiting,
    /// Somebody refused.
    Refused,
    /// Nobody answered in time.
    RanOut,
    /// Here is the credential.
    Admitted {
        /// The credential to keep.
        token: String,
        /// Whose account this machine now acts for.
        user: String,
    },
}

/// One group this machine can work in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GroupRemote {
    /// The group's short name, which is also its directory.
    pub slug: String,
    /// What the group calls itself.
    pub name: String,
    /// What this person may do in it.
    pub role: String,
    /// Where the group's own repository is.
    pub shared: String,
    /// Where this person's repository inside it is.
    pub personal: String,
}

/// What the instance says this machine can work in.
#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    /// Whose account this is.
    pub user: String,
    /// The groups, with their repositories.
    pub groups: Vec<GroupRemote>,
}

/// What the instance knows about this machine as a node.
#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    /// Its identifier here.
    pub machine: String,
    /// What it calls itself.
    pub label: String,
    /// Whether the instance takes it to be there.
    pub presence: stemma_core::machine::Presence,
    /// The groups it serves.
    pub groups: Vec<String>,
    /// What it last reported finding.
    pub agents: Vec<stemma_core::machine::Detected>,
}

/// Accepted source ready to be written by its author's machine.
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptedProposal {
    /// Complete accepted source.
    pub source: String,
    /// Complete accepted Lean module, when formal work changed.
    #[serde(default)]
    pub lean_source: Option<String>,
    /// Group whose clone holds it.
    pub group: String,
    /// Shared or private repository.
    pub side: String,
    /// Line directory.
    pub line: String,
    /// Exact base commit reviewed.
    pub commit: String,
}

/// One machine offered as a deliberate choice for a group.
#[derive(Debug, Clone, Deserialize)]
pub struct OfferedMachine {
    /// Stable machine identifier.
    pub machine: String,
    /// Owner-given label.
    pub label: String,
    /// Awake or asleep.
    pub presence: String,
    /// Every known agent, including unavailable ones with their reason.
    pub agents: Vec<stemma_core::machine::Detected>,
}

#[derive(Deserialize)]
struct MachineChoices {
    machines: Vec<OfferedMachine>,
}

/// One job, as the machine that took it is told about it.
#[derive(Debug, Clone, Deserialize)]
pub struct Handed {
    /// Its identifier.
    pub job: String,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The commit to work against.
    pub commit: String,
    /// What is being asked.
    pub task: String,
    /// Which agent to use.
    pub agent: String,
    /// Who asked, and answers for it.
    pub launched_by: String,
}

/// One job in a listing.
#[derive(Debug, Clone, Deserialize)]
pub struct Listed {
    /// Its identifier.
    pub job: String,
    /// The group's short name.
    pub group: String,
    /// Which line of work.
    pub line: String,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Where it has got to.
    pub state: String,
    /// One line about it.
    pub said: String,
}

/// What a machine says to launch a job.
#[derive(Debug, Clone, Serialize)]
pub struct Asking {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The commit to work against.
    pub commit: String,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Which machine, where one was chosen.
    pub machine: Option<String>,
}

/// Who a credential acts for.
#[derive(Debug, Clone, Deserialize)]
pub struct Whoami {
    /// The account.
    pub user: String,
    /// What to call them.
    pub display_name: String,
}

impl Server {
    /// An instance at this address.
    pub fn at(address: &str) -> Result<Self> {
        let address = normalise(address)?;

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            // A refusal carries a body that says which refusal it is, and
            // the device flow says "not yet" with one of them.
            .http_status_as_error(false)
            .build()
            .into();

        Ok(Self { address, agent })
    }

    /// The address this program is talking to.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Asks the instance to start signing this machine in.
    pub fn start_sign_in(&self, label: &str) -> Result<SignIn> {
        let mut response = self
            .agent
            .post(self.url("/api/device/start"))
            .send_json(serde_json::json!({ "label": label }))
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        if response.status() != 200 {
            bail!(
                "{} refused to start signing this machine in ({})",
                self.address,
                response.status()
            );
        }

        response
            .body_mut()
            .read_json()
            .context("the instance answered with something this build does not understand")
    }

    /// Asks whether somebody has answered yet.
    pub fn collect_sign_in(&self, device_code: &str) -> Result<Answer> {
        let mut response = self
            .agent
            .post(self.url("/api/device/collect"))
            .send_json(serde_json::json!({ "device_code": device_code }))
            .with_context(|| format!("cannot reach {}", self.address))?;

        let answer: serde_json::Value = response
            .body_mut()
            .read_json()
            .context("the instance answered with something this build does not understand")?;

        if response.status() == 200 {
            let field = |name: &str| {
                answer[name]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("the instance's answer left out `{name}`"))
            };

            return Ok(Answer::Admitted {
                token: field("access_token")?,
                user: field("user")?,
            });
        }

        Ok(match answer["error"].as_str() {
            Some("authorization_pending") | Some("slow_down") => Answer::Waiting,
            Some("access_denied") => Answer::Refused,
            Some("expired_token") => Answer::RanOut,
            other => bail!(
                "{} refused this sign-in: {}",
                self.address,
                other.unwrap_or("for a reason this build does not know")
            ),
        })
    }

    /// Who a credential acts for.
    pub fn whoami(&self, token: &str) -> Result<Option<Whoami>> {
        let mut response = self
            .agent
            .get(self.url("/api/me"))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        match response.status().as_u16() {
            200 => response
                .body_mut()
                .read_json()
                .map(Some)
                .context("the instance answered with something this build does not understand"),
            403 | 401 => Ok(None),
            status => bail!("{} answered with {status}", self.address),
        }
    }

    /// The groups this machine can work in.
    pub fn workspace(&self, token: &str) -> Result<Workspace> {
        let mut response = self
            .agent
            .get(self.url("/api/workspace"))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        match response.status().as_u16() {
            200 => response
                .body_mut()
                .read_json()
                .context("the instance answered with something this build does not understand"),
            403 | 401 => bail!(
                "{} no longer accepts this machine's credential: run `stemma login` again",
                self.address
            ),
            status => bail!("{} answered with {status}", self.address),
        }
    }

    /// Hands the instance something this machine built.
    pub fn publish(
        &self,
        token: &str,
        group: &str,
        side: &str,
        kind: &str,
        commit: &str,
        body: &str,
    ) -> Result<()> {
        let mut response = self
            .agent
            .put(self.url(&format!(
                "/api/groups/{group}/{side}/artefacts/{kind}/{commit}"
            )))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .send(body)
            .with_context(|| format!("cannot reach {}", self.address))?;

        match response.status().as_u16() {
            200 | 201 => Ok(()),
            status => {
                let said = response
                    .body_mut()
                    .read_to_string()
                    .unwrap_or_else(|_| String::new());

                bail!(
                    "{} would not keep it ({status}): {}",
                    self.address,
                    said.trim()
                )
            }
        }
    }

    /// Offers this machine to the instance as one that runs work.
    pub fn register_node(
        &self,
        token: &str,
        label: &str,
        agents: &[stemma_core::machine::Detected],
        groups: Option<&[String]>,
    ) -> Result<Node> {
        self.ask(
            token,
            "/api/machines",
            &serde_json::json!({
                "label": label,
                "agents": agents,
                "groups": groups,
            }),
        )
    }

    /// Says this machine is still there, and what it can do right now.
    pub fn beat(&self, token: &str, agents: &[stemma_core::machine::Detected]) -> Result<Node> {
        self.ask(
            token,
            "/api/machines/beat",
            &serde_json::json!({ "agents": agents }),
        )
    }

    /// What the instance thinks of this machine.
    pub fn node(&self, token: &str) -> Result<Option<Node>> {
        let mut response = self
            .agent
            .get(self.url("/api/machines"))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        match response.status().as_u16() {
            200 => response
                .body_mut()
                .read_json()
                .map(Some)
                .context("the instance answered with something this build does not understand"),
            // Not registered is an answer and not a failure: this
            // machine has signed in and has not offered itself.
            403 => Ok(None),
            status => bail!("{} answered with {status}", self.address),
        }
    }

    /// Asks for whatever work this machine may have next.
    pub fn take_job(&self, token: &str) -> Result<Option<Handed>> {
        let mut response = self
            .agent
            .post(self.url("/api/jobs/take"))
            .header("authorization", format!("Bearer {token}"))
            .send_empty()
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        match response.status().as_u16() {
            200 => response
                .body_mut()
                .read_json()
                .map(Some)
                .context("the instance answered with something this build does not understand"),
            204 => Ok(None),
            403 => bail!(
                "{} does not know this machine: run `stemma node init`",
                self.address
            ),
            status => bail!("{} answered with {status}", self.address),
        }
    }

    /// Says how a job ended.
    pub fn finish_job(
        &self,
        token: &str,
        job: &str,
        completion: &stemma_core::proposal::Completion,
    ) -> Result<()> {
        let _: serde_json::Value =
            self.ask(token, &format!("/api/jobs/{job}/finish"), completion)?;

        Ok(())
    }

    /// Fetches a proposal this person accepted and may now write locally.
    pub fn accepted_proposal(&self, token: &str, proposal: &str) -> Result<AcceptedProposal> {
        let mut response = self
            .agent
            .get(self.url(&format!("/api/proposals/{proposal}")))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;
        self.check_protocol(response.headers())?;
        if response.status() != 200 {
            bail!(
                "{} would not release that proposal ({})",
                self.address,
                response.status()
            );
        }
        response
            .body_mut()
            .read_json()
            .context("the instance answered with something this build does not understand")
    }

    /// Puts a job in the queue.
    pub fn launch_job(&self, token: &str, asking: &Asking) -> Result<String> {
        let answer: serde_json::Value = self.ask(token, "/api/jobs", asking)?;

        answer["job"]
            .as_str()
            .map(str::to_owned)
            .context("the instance did not say which job it made")
    }

    /// What this machine has in the queue.
    pub fn jobs(&self, token: &str) -> Result<Vec<Listed>> {
        let mut response = self
            .agent
            .get(self.url("/api/jobs"))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        if response.status() != 200 {
            bail!("{} answered with {}", self.address, response.status());
        }

        let answer: serde_json::Value = response
            .body_mut()
            .read_json()
            .context("the instance answered with something this build does not understand")?;

        serde_json::from_value(answer["jobs"].clone())
            .context("the instance answered with something this build does not understand")
    }

    /// Machines serving a group, for a conscious launcher choice.
    pub fn machines(&self, token: &str, group: &str) -> Result<Vec<OfferedMachine>> {
        let mut response = self
            .agent
            .get(self.url(&format!("/api/groups/{group}/machines")))
            .header("authorization", format!("Bearer {token}"))
            .call()
            .with_context(|| format!("cannot reach {}", self.address))?;
        self.check_protocol(response.headers())?;
        if response.status() != 200 {
            bail!("{} answered with {}", self.address, response.status());
        }
        Ok(response
            .body_mut()
            .read_json::<MachineChoices>()
            .context("the instance answered with something this build does not understand")?
            .machines)
    }

    /// Sends something as this machine, and reads the answer.
    ///
    /// A refusal carries a line saying which refusal it is, and that line
    /// is what somebody at a terminal needs, so it is not thrown away.
    fn ask<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T> {
        let mut response = self
            .agent
            .post(self.url(path))
            .header("authorization", format!("Bearer {token}"))
            .send_json(body)
            .with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        if response.status() != 200 {
            let status = response.status();
            let said = response
                .body_mut()
                .read_to_string()
                .unwrap_or_else(|_| String::new());

            bail!("{} refused ({status}): {}", self.address, summarise(&said));
        }

        response
            .body_mut()
            .read_json()
            .context("the instance answered with something this build does not understand")
    }

    /// Gives a credential back.
    pub fn sign_out(&self, token: &str) -> Result<()> {
        self.agent
            .post(self.url("/api/logout"))
            .header("authorization", format!("Bearer {token}"))
            .send_empty()
            .with_context(|| format!("cannot reach {}", self.address))?;

        Ok(())
    }

    /// One of this instance's addresses.
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.address)
    }

    /// Refuses to talk to an instance this build cannot understand.
    fn check_protocol(&self, headers: &ureq::http::HeaderMap) -> Result<()> {
        let Some(spoken) = headers
            .get(PROTOCOL_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<ProtocolVersion>().ok())
        else {
            bail!(
                "{} did not say which version of Stemma it speaks, so it is probably \
                 not a Stemma instance",
                self.address
            );
        };

        PROTOCOL_VERSION
            .check_against(spoken)
            .with_context(|| format!("cannot talk to {}", self.address))?;

        Ok(())
    }
}

/// The readable part of what an instance said when it refused.
///
/// A refusal is a page when a person is reading and a line when a
/// program is, and a wall of markup at a terminal says nothing.
fn summarise(said: &str) -> String {
    let text = said.trim();

    if !text.starts_with('<') {
        return text.to_owned();
    }

    let stripped: String = text
        .split('<')
        .filter_map(|piece| piece.split_once('>'))
        .map(|(_, words)| words)
        .collect::<Vec<_>>()
        .join(" ");

    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The address of an instance, in the one form the rest of this uses.
fn normalise(address: &str) -> Result<String> {
    let address = address.trim().trim_end_matches('/');

    if address.is_empty() {
        bail!("that is not the address of an instance");
    }

    let address = match address.contains("://") {
        true => address.to_owned(),
        // Somebody typing a host means the ordinary thing by it, and an
        // instance is behind a reverse proxy that terminates TLS.
        false => format!("https://{address}"),
    };

    if !address.starts_with("http://") && !address.starts_with("https://") {
        bail!("`{address}` is not an http or https address");
    }

    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_on_its_own_is_taken_as_encrypted() {
        assert_eq!(
            normalise("stemma.example.org").unwrap(),
            "https://stemma.example.org"
        );
        assert_eq!(
            normalise(" stemma.example.org/ ").unwrap(),
            "https://stemma.example.org"
        );
    }

    #[test]
    fn an_address_that_says_what_it_is_keeps_it() {
        assert_eq!(
            normalise("http://localhost:8080").unwrap(),
            "http://localhost:8080"
        );
        assert_eq!(
            normalise("https://example.org/stemma/").unwrap(),
            "https://example.org/stemma"
        );
    }

    #[test]
    fn a_refusal_meant_for_a_browser_is_read_back_as_a_line() {
        let page = "<html><body><h1>403 Forbidden</h1><p>This machine is not \
                    registered here.</p></body></html>";

        assert_eq!(
            summarise(page),
            "403 Forbidden This machine is not registered here."
        );
        assert_eq!(summarise("  plain words  "), "plain words");
    }

    #[test]
    fn something_that_is_not_an_address_is_refused() {
        for wrong in ["", "   ", "ftp://example.org", "ssh://example.org"] {
            assert!(normalise(wrong).is_err(), "took `{wrong}`");
        }
    }
}
