//! Talking to an instance.
//!
//! Signing in uses the device authorization flow: this program asks for a
//! code, the person types it into a browser they are already signed in on,
//! and only then does a credential come back here. Nothing has to reach
//! this machine's own address, which is what makes it work over SSH and
//! inside a container.
//!
//! Every exchange goes through [`Server`], which refuses an answer from
//! anything that does not say it speaks this build's protocol. The types
//! that travel are defined in a module of their own and re-exported here.

pub mod events;
mod reaching;
mod wire;

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use stemma_core::PROTOCOL_VERSION;
use stemma_core::protocol::{Behind, ProtocolVersion};
use ureq::Body;
use ureq::http::Response;

use crate::remedy::Remedy;

pub use wire::*;

/// The header both sides announce their protocol in.
const PROTOCOL_HEADER: &str = stemma_core::PROTOCOL_HEADER;

/// How long to wait on a request before giving up.
const TIMEOUT: Duration = Duration::from_secs(20);

/// How long the whole of reaching an instance may take.
///
/// A name can resolve to more than one address, and the client tries
/// them in turn, giving the first the largest share of this. Which
/// address goes first is therefore most of the answer, and that is
/// settled in `reaching` before the client sees them; this is what is
/// left for the case where none of them answers. Connecting is fast or
/// it is not going to happen.
const CONNECTING: Duration = Duration::from_secs(6);

/// How long one stream may last before it is taken for dead.
const STREAM: Duration = Duration::from_secs(11 * 60);

/// What an answer the client cannot read is reported as.
const UNDERSTOOD: &str = "the server sent an answer this build does not understand";

/// An instance, as this program talks to it.
pub struct Server {
    address: String,
    agent: ureq::Agent,
}

/// The one agent every instance is reached through.
///
/// An agent keeps the connections it has opened, and a connection that
/// is already open costs a fraction of one that is not: the handshake
/// is most of a short request. A fresh agent for every command throws
/// that away and pays it again, so there is one, and it lives as long
/// as the program does.
static REACHING: OnceLock<ureq::Agent> = OnceLock::new();

/// The agent a stream of events is heard through.
///
/// Not the one above: a stream is one answer that lasts minutes, and a
/// limit on the whole exchange would cut every one of them short. What
/// it has instead is a limit just past the ten minutes the instance
/// keeps a stream open, so a stream that went silent is noticed.
static LISTENING: OnceLock<ureq::Agent> = OnceLock::new();

/// The agent for ordinary requests, made the first time anything asks
/// for it.
fn agent() -> ureq::Agent {
    REACHING
        .get_or_init(|| {
            built(
                ureq::Agent::config_builder()
                    .timeout_global(Some(TIMEOUT))
                    .timeout_connect(Some(CONNECTING)),
            )
        })
        .clone()
}

/// The agent for streams of events.
fn listening() -> ureq::Agent {
    LISTENING
        .get_or_init(|| {
            built(
                ureq::Agent::config_builder()
                    .timeout_connect(Some(CONNECTING))
                    .timeout_recv_response(Some(TIMEOUT))
                    .timeout_recv_body(Some(STREAM)),
            )
        })
        .clone()
}

/// An agent with the settings every one of them shares.
fn built(config: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>) -> ureq::Agent {
    let config = config
        // A refusal carries a body that says which refusal it is, and
        // the device flow says "not yet" with one of them.
        .http_status_as_error(false)
        .build();

    ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::new(),
        reaching::HappyEyeballs::default(),
    )
}

/// The authorization header a credential is sent in.
fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// An answer read as the type expected of it.
fn understood<T: DeserializeOwned>(response: &mut Response<Body>) -> Result<T> {
    response.body_mut().read_json().context(UNDERSTOOD)
}

/// One field of an answer, read as the type expected of it.
fn field<T: DeserializeOwned>(mut answer: serde_json::Value, name: &str) -> Result<T> {
    let value = answer
        .get_mut(name)
        .map(serde_json::Value::take)
        .unwrap_or_default();

    serde_json::from_value(value).context(UNDERSTOOD)
}

/// The words an answer carries, or none when it carries none that can be
/// read.
fn words(response: &mut Response<Body>) -> String {
    response.body_mut().read_to_string().unwrap_or_default()
}

impl Server {
    /// An instance at this address.
    pub fn at(address: &str) -> Result<Self> {
        let address = normalise(address)?;

        Ok(Self {
            address,
            agent: agent(),
        })
    }

    /// The address this program is talking to.
    pub fn address(&self) -> &str {
        &self.address
    }

    // ---- Signing in and out ----

    /// Asks the instance to start signing this machine in.
    pub fn start_sign_in(&self, label: &str) -> Result<SignIn> {
        let mut response = self.answered(
            self.agent
                .post(self.url("/api/device/start"))
                .send_json(serde_json::json!({ "label": label })),
        )?;

        if response.status() != 200 {
            bail!(
                "{} refused to start signing this machine in ({})",
                self.address,
                response.status()
            );
        }

        understood(&mut response)
    }

    /// Asks whether somebody has answered yet.
    pub fn collect_sign_in(&self, device_code: &str) -> Result<Answer> {
        let mut response = self.answered(
            self.agent
                .post(self.url("/api/device/collect"))
                .send_json(serde_json::json!({ "device_code": device_code })),
        )?;
        let answer: serde_json::Value = understood(&mut response)?;

        if response.status() == 200 {
            let field = |name: &str| {
                answer[name]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("the server's answer is missing `{name}`"))
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
        let mut response = self.get(token, "/api/me")?;

        match response.status().as_u16() {
            200 => understood(&mut response).map(Some),
            403 | 401 => Ok(None),
            status => Err(self.unexpected(status)),
        }
    }

    /// Gives a credential back.
    pub fn sign_out(&self, token: &str) -> Result<()> {
        let mut response = self.post_empty(token, "/api/logout")?;

        match response.status().as_u16() {
            // An already invalid credential also leaves this machine signed out.
            200 | 204 | 401 | 403 => Ok(()),
            status => bail!(
                "{} could not revoke the credential ({status}): {}",
                self.address,
                summarise(&words(&mut response))
            ),
        }
    }

    /// The groups this machine can work in.
    pub fn workspace(&self, token: &str) -> Result<Workspace> {
        let mut response = self.get(token, "/api/workspace")?;

        match response.status().as_u16() {
            200 => understood(&mut response),
            403 | 401 => Err(Remedy::SignIn).with_context(|| {
                format!(
                    "{} no longer accepts this machine's credential",
                    self.address
                )
            }),
            status => Err(self.unexpected(status)),
        }
    }

    // ---- Artefacts ----

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
        let mut response = self.answered(
            self.agent
                .put(self.url(&format!(
                    "/api/groups/{}/{}/artefacts/{}/{}",
                    segment(group),
                    segment(side),
                    segment(kind),
                    segment(commit)
                )))
                .header("authorization", bearer(token))
                .header("content-type", "application/json")
                .send(body),
        )?;

        match response.status().as_u16() {
            200 | 201 => Ok(()),
            status => bail!(
                "{} would not keep it ({status}): {}",
                self.address,
                words(&mut response).trim()
            ),
        }
    }

    // ---- This machine as a node ----

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
        let mut response = self.get(token, "/api/machines")?;

        match response.status().as_u16() {
            200 => understood(&mut response).map(Some),
            // Not registered is an answer and not a failure: this
            // machine has signed in and has not offered itself.
            403 => Ok(None),
            status => Err(self.unexpected(status)),
        }
    }

    /// Takes this machine back, and the credential it holds with it.
    /// Answers whether there was a machine to take back.
    pub fn retire_node(&self, token: &str) -> Result<bool> {
        let answer: serde_json::Value =
            self.ask(token, "/api/machines/retire", &serde_json::json!({}))?;

        Ok(answer["retired"].as_bool().unwrap_or(false))
    }

    /// Machines serving a group, for a conscious launcher choice.
    pub fn machines(&self, token: &str, group: &str) -> Result<Vec<OfferedMachine>> {
        let choices: MachineChoices =
            self.read(token, &format!("/api/groups/{}/machines", segment(group)))?;

        Ok(choices.machines)
    }

    // ---- Jobs ----

    /// Asks for whatever work this machine may have next.
    pub fn take_job(&self, token: &str) -> Result<Option<Handed>> {
        let mut response = self.post_empty(token, "/api/jobs/take")?;

        match response.status().as_u16() {
            200 => understood(&mut response).map(Some),
            204 => Ok(None),
            403 => Err(Remedy::SetUpMachine)
                .with_context(|| format!("{} does not know this machine", self.address)),
            status => Err(self.unexpected(status)),
        }
    }

    /// Says how a job ended.
    pub fn finish_job(
        &self,
        token: &str,
        job: &str,
        completion: &stemma_core::proposal::Completion,
    ) -> Result<()> {
        self.tell(
            token,
            &format!("/api/jobs/{}/finish", segment(job)),
            completion,
        )
    }

    /// Reports visible progress and learns whether the launcher cancelled.
    pub fn job_progress(
        &self,
        token: &str,
        job: &str,
        workspace: Option<&str>,
        progress: &str,
    ) -> Result<bool> {
        let answer: JobControl = self.ask(
            token,
            &format!("/api/jobs/{}/progress", segment(job)),
            &serde_json::json!({ "workspace": workspace, "progress": progress }),
        )?;

        Ok(answer.cancel_requested)
    }

    /// Cancels a queued job or asks its worker to stop it.
    pub fn cancel_job(&self, token: &str, job: &str) -> Result<()> {
        self.tell(
            token,
            &format!("/api/jobs/{}/cancel", segment(job)),
            &serde_json::json!({}),
        )
    }

    /// Puts a job in the queue.
    pub fn launch_job(&self, token: &str, asking: &Asking) -> Result<String> {
        let answer: serde_json::Value = self.ask(token, "/api/jobs", asking)?;

        answer["job"]
            .as_str()
            .map(str::to_owned)
            .context("the server did not say which job it created")
    }

    /// What this machine has in the queue.
    pub fn jobs(&self, token: &str) -> Result<Vec<Listed>> {
        field(self.read(token, "/api/jobs")?, "jobs")
    }

    // ---- Proposals ----

    /// Fetches a proposal this person accepted and may now write locally.
    pub fn accepted_proposal(&self, token: &str, proposal: &str) -> Result<AcceptedProposal> {
        let mut response = self.get(token, &format!("/api/proposals/{}", segment(proposal)))?;

        if response.status() != 200 {
            bail!(
                "{} would not release that proposal ({})",
                self.address,
                response.status()
            );
        }

        understood(&mut response)
    }

    /// What this person still has to review.
    pub fn proposals(&self, token: &str) -> Result<Vec<Waiting>> {
        field(self.read(token, "/api/proposals")?, "proposals")
    }

    /// Pending and recently decided reviews.
    pub fn reviews(&self, token: &str) -> Result<Reviews> {
        self.read(token, "/api/proposals")
    }

    /// The review history of one line.
    pub fn line_proposals(
        &self,
        token: &str,
        group: &str,
        side: &str,
        line: &str,
    ) -> Result<Vec<LineProposal>> {
        field(
            self.read(
                token,
                &format!(
                    "/api/proposals/history/{}/{}/{}",
                    segment(group),
                    segment(side),
                    segment(line)
                ),
            )?,
            "proposals",
        )
    }

    /// One proposal, with the version it would replace.
    pub fn proposal(&self, token: &str, proposal: &str) -> Result<Offered> {
        self.read(
            token,
            &format!("/api/proposals/{}/review", segment(proposal)),
        )
    }

    /// Records a reviewer's decision about a proposal.
    pub fn decide_proposal(
        &self,
        token: &str,
        proposal: &str,
        decision: stemma_core::proposal::Verdict,
    ) -> Result<()> {
        self.decide_proposal_with_note(token, proposal, decision, None)
    }

    /// Records a decision and optional reviewer note.
    pub fn decide_proposal_with_note(
        &self,
        token: &str,
        proposal: &str,
        decision: stemma_core::proposal::Verdict,
        note: Option<&str>,
    ) -> Result<()> {
        self.tell(
            token,
            &format!("/api/proposals/{}/decide", segment(proposal)),
            &serde_json::json!({ "decision": decision.as_str(), "note": note }),
        )
    }

    /// Assigns a pending shared-line review to a group member.
    pub fn handoff_proposal(&self, token: &str, proposal: &str, reviewer: &str) -> Result<()> {
        self.tell(
            token,
            &format!("/api/proposals/{}/handoff", segment(proposal)),
            &serde_json::json!({ "reviewer": reviewer }),
        )
    }

    /// What this person accepted and none of their machines applied yet.
    pub fn accepted_proposals(&self, token: &str) -> Result<Vec<ToApply>> {
        field(self.read(token, "/api/proposals/accepted")?, "proposals")
    }

    /// Says an accepted proposal was written, and as which revision.
    pub fn proposal_applied(&self, token: &str, proposal: &str, revision: u64) -> Result<()> {
        self.tell(
            token,
            &format!("/api/proposals/{}/applied", segment(proposal)),
            &serde_json::json!({ "revision": revision }),
        )
    }

    /// Says an accepted proposal came too late: a block it changes was
    /// changed since the revision it was made against.
    pub fn proposal_outdated(&self, token: &str, proposal: &str) -> Result<()> {
        self.tell(
            token,
            &format!("/api/proposals/{}/outdated", segment(proposal)),
            &serde_json::json!({}),
        )
    }

    // ---- Lines ----

    /// Every line this machine's owner can see, deleted ones too, and the
    /// revision each is at.
    pub fn lines(&self, token: &str) -> Result<Vec<Listing>> {
        field(self.read(token, "/api/lines")?, "lines")
    }

    /// One revision of a line: the one it is at, or the one named.
    pub fn revision(&self, token: &str, line: &str, number: Option<u64>) -> Result<Revision> {
        let path = match number {
            Some(number) => format!("/api/lines/{}?revision={}", segment(line), number),
            None => format!("/api/lines/{}", segment(line)),
        };

        self.read(token, &path)
    }

    /// Writes a line, as the revision after the one it was written from.
    ///
    /// A refusal is an answer, not a failure: each says what the machine
    /// needs to put it right. Only not reaching the instance is an error.
    pub fn write_line(&self, token: &str, line: &str, outgoing: &Outgoing) -> Result<Written> {
        let mut response = self.answered(
            self.agent
                .put(self.url(&format!("/api/lines/{}", segment(line))))
                .header("authorization", bearer(token))
                .send_json(outgoing),
        )?;

        let status = response.status().as_u16();
        let answer: serde_json::Value = response.body_mut().read_json().unwrap_or_default();
        let said = || {
            answer["error"]
                .as_str()
                .map_or_else(|| format!("refused ({status})"), str::to_owned)
        };
        let understood = || anyhow!(UNDERSTOOD);

        Ok(match status {
            200 => Written::Taken(answer["revision"].as_u64().ok_or_else(understood)?),
            409 => Written::Stale(Box::new(
                serde_json::from_value(answer["current"].clone()).map_err(|_| understood())?,
            )),
            423 => Written::NotHeld(answer["held_by"].as_str().map(str::to_owned)),
            410 => Written::Gone,
            400..=499 => Written::Refused(said()),
            _ => bail!("{} answered with {status}: {}", self.address, said()),
        })
    }

    /// The files at the root of one of a group's repositories, by path.
    pub fn repository_root(
        &self,
        token: &str,
        group: &str,
        side: &str,
    ) -> Result<BTreeMap<String, String>> {
        field(
            self.read(
                token,
                &format!("/api/repositories/{}/{}", segment(group), segment(side)),
            )?,
            "files",
        )
    }

    /// Takes a line of one's own out of the repository.
    pub fn delete_line(&self, token: &str, group: &str, line: &str) -> Result<()> {
        self.tell(
            token,
            &format!("/api/lines/{}/{}/delete", segment(group), segment(line)),
            &serde_json::json!({}),
        )
    }

    /// Starts a line.
    pub fn start_line(&self, token: &str, starting: &Starting) -> Result<Placed> {
        self.ask(token, "/api/lines", starting)
    }

    /// Moves one to the group.
    pub fn share_line(
        &self,
        token: &str,
        group: &str,
        line: &str,
        sharing: &Sharing,
    ) -> Result<Placed> {
        self.ask(
            token,
            &format!("/api/lines/{}/{}/share", segment(group), segment(line)),
            sharing,
        )
    }

    // ---- Claims ----

    /// Takes a line for this machine, unless another one has it.
    pub fn take_claim(&self, token: &str, about: &About) -> Result<Claimed> {
        self.ask(token, "/api/claims", about)
    }

    /// Says this machine is still writing.
    pub fn renew_claim(&self, token: &str, about: &About) -> Result<Claimed> {
        self.ask(token, "/api/claims/renew", about)
    }

    /// Lets a line go.
    pub fn release_claim(&self, token: &str, about: &About) -> Result<Claimed> {
        self.ask(token, "/api/claims/release", about)
    }

    /// Asks the machine writing a line to hand it back.
    pub fn ask_for_claim(&self, token: &str, about: &About) -> Result<Claimed> {
        self.ask(token, "/api/claims/ask", about)
    }

    /// Every line being written where this machine works.
    pub fn claims(&self, token: &str) -> Result<Vec<Writing>> {
        field(self.read(token, "/api/claims")?, "claims")
    }

    // ---- The vault ----

    /// Reads one group's immutable mathematical base.
    pub fn vault(&self, token: &str, group: &str) -> Result<Vault> {
        let mut response = self.get(token, &format!("/api/groups/{}/vault", segment(group)))?;

        if response.status() != 200 {
            bail!(
                "{} would not show that vault ({})",
                self.address,
                response.status()
            );
        }

        understood(&mut response)
    }

    /// Admits a locally verified definition.
    pub fn admit_vault_entry(
        &self,
        token: &str,
        group: &str,
        body: &serde_json::Value,
    ) -> Result<String> {
        let answer: serde_json::Value = self.ask(
            token,
            &format!("/api/groups/{}/vault/entries", segment(group)),
            body,
        )?;
        answer["entry"]
            .as_str()
            .map(str::to_owned)
            .context("the server did not identify the vault entry")
    }

    /// Admits a locally compiled unconditional bridge.
    pub fn admit_bridge(
        &self,
        token: &str,
        group: &str,
        body: &serde_json::Value,
    ) -> Result<String> {
        let answer: serde_json::Value = self.ask(
            token,
            &format!("/api/groups/{}/vault/bridges", segment(group)),
            body,
        )?;
        answer["bridge"]
            .as_str()
            .map(str::to_owned)
            .context("the server did not identify the bridge")
    }

    /// Records a signed correspondence action.
    pub fn audit_vault(
        &self,
        token: &str,
        group: &str,
        body: &serde_json::Value,
    ) -> Result<String> {
        let answer: serde_json::Value = self.ask(
            token,
            &format!("/api/groups/{}/vault/audits", segment(group)),
            body,
        )?;
        answer["audit"]
            .as_str()
            .map(str::to_owned)
            .context("the server did not identify the audit")
    }

    // ---- Events ----

    /// Opens the stream of what changes on the instance, from after the
    /// last notice this machine saw, or from now.
    pub fn listen(&self, token: &str, last: Option<i64>) -> Result<impl std::io::BufRead + use<>> {
        let mut request = listening()
            .get(self.url("/api/events"))
            .header("authorization", bearer(token))
            .header("accept", "text/event-stream");
        if let Some(last) = last {
            request = request.header(stemma_core::event::LAST_EVENT_HEADER, last.to_string());
        }

        let response = self.answered(request.call())?;

        match response.status().as_u16() {
            200 => Ok(std::io::BufReader::new(response.into_body().into_reader())),
            401 | 403 => bail!(
                "{} no longer accepts this machine's credential",
                self.address
            ),
            status => Err(self.unexpected(status)),
        }
    }

    // ---- Exchanges ----

    /// Reads something the instance offers to a machine.
    fn read<T: DeserializeOwned>(&self, token: &str, path: &str) -> Result<T> {
        let mut response = self.get(token, path)?;

        if response.status() != 200 {
            return Err(self.unexpected(response.status().as_u16()));
        }

        understood(&mut response)
    }

    /// Sends something as this machine, and reads the answer.
    ///
    /// A refusal carries a line saying which refusal it is, and that line
    /// is what somebody at a terminal needs, so it is not thrown away.
    fn ask<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T> {
        let mut response = self.answered(
            self.agent
                .post(self.url(path))
                .header("authorization", bearer(token))
                .send_json(body),
        )?;

        if response.status() != 200 {
            let status = response.status();
            bail!(
                "{} refused ({status}): {}",
                self.address,
                summarise(&words(&mut response))
            );
        }

        understood(&mut response)
    }

    /// Sends something as this machine, where the answer says nothing
    /// beyond having been taken.
    fn tell(&self, token: &str, path: &str, body: &impl Serialize) -> Result<()> {
        let _: serde_json::Value = self.ask(token, path, body)?;

        Ok(())
    }

    /// Asks for something as this machine.
    fn get(&self, token: &str, path: &str) -> Result<Response<Body>> {
        self.answered(
            self.agent
                .get(self.url(path))
                .header("authorization", bearer(token))
                .call(),
        )
    }

    /// Asks for something to be done as this machine, with nothing else
    /// to say.
    fn post_empty(&self, token: &str, path: &str) -> Result<Response<Body>> {
        self.answered(
            self.agent
                .post(self.url(path))
                .header("authorization", bearer(token))
                .send_empty(),
        )
    }

    /// The answer to a request, once it is known to come from an instance
    /// this build understands.
    fn answered(&self, sent: Result<Response<Body>, ureq::Error>) -> Result<Response<Body>> {
        let response = sent.with_context(|| format!("cannot reach {}", self.address))?;

        self.check_protocol(response.headers())?;

        Ok(response)
    }

    /// An answer this build did not expect.
    fn unexpected(&self, status: u16) -> anyhow::Error {
        anyhow!("{} answered with {status}", self.address)
    }

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

        if let Err(mismatch) = PROTOCOL_VERSION.check_against(spoken) {
            let update = match mismatch.who_is_behind() {
                Behind::Server => "the server has to be updated to this version",
                Behind::Client => "this app has to be updated to the server's version",
            };
            bail!("cannot talk to {}: {mismatch}; {update}", self.address);
        }

        Ok(())
    }
}

/// The readable part of what an instance said when it refused.
///
/// An instance that knows a program is asking answers with the sentence
/// alone. One that does not — an older one, or something else entirely
/// behind the same address — answers with a page, and a wall of markup
/// at a terminal says nothing, so what words it has are read out of it.
fn summarise(said: &str) -> String {
    let text = said.trim();

    if let Some(line) = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|answer| answer["error"].as_str().map(str::to_owned))
    {
        return line;
    }

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
    let address = address.trim();

    if address.is_empty() {
        bail!("that is not the address of a Stemma server");
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

    let parsed = url::Url::parse(&address).context("that is not the address of a Stemma server")?;
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!(
            "a server address needs a host and must not include credentials, a query or a fragment"
        );
    }

    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

/// A value as one segment of a request's path.
///
/// Identifiers, group names and line names never need escaping when they
/// are well formed, and then come out unchanged; one that is not cannot
/// reach another path by carrying a `/`, `?` or `..`.
fn segment(value: impl std::fmt::Display) -> String {
    let value = value.to_string();
    if value == "." || value == ".." {
        return value.replace('.', "%2E");
    }
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_name_is_its_own_path_segment() {
        for name in [
            "algebra",
            "shared",
            "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T",
            "rings-and-fields",
        ] {
            assert_eq!(segment(name), name);
        }
    }

    #[test]
    fn a_name_cannot_reach_another_path() {
        assert_eq!(segment("../admin"), "..%2Fadmin");
        assert_eq!(segment(".."), "%2E%2E");
        assert_eq!(segment("a?b=c"), "a%3Fb%3Dc");
        assert_eq!(segment("über"), "%C3%BCber");
    }

    fn reply_once(
        status: &str,
        protocol: Option<&str>,
        body: &str,
    ) -> (Server, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = Server::at(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let version = protocol
            .map(|version| format!("{PROTOCOL_HEADER}: {version}\r\n"))
            .unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {status}\r\n{version}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream.write_all(response.as_bytes()).unwrap();
        });
        (server, worker)
    }

    #[test]
    fn a_failed_remote_logout_is_reported() {
        let (server, worker) = reply_once(
            "500 Internal Server Error",
            Some(&PROTOCOL_VERSION.to_string()),
            r#"{"error":"revocation failed"}"#,
        );
        let error = server.sign_out("token").unwrap_err();
        worker.join().unwrap();
        assert!(error.to_string().contains("revocation failed"));
    }

    #[test]
    fn an_older_server_is_named_as_the_side_to_update() {
        let (server, worker) = reply_once(
            "400 Bad Request",
            Some("0.5"),
            r#"{"error":"A line needs the question it exists to answer."}"#,
        );
        let error = server.sign_out("token").unwrap_err();
        worker.join().unwrap();
        let said = error.to_string();
        assert!(said.contains("the server speaks 0.5"), "{said}");
        assert!(said.contains("the server has to be updated"), "{said}");
    }

    #[test]
    fn sign_in_collection_requires_the_stemma_protocol() {
        let (server, worker) = reply_once(
            "200 OK",
            None,
            r#"{"access_token":"token","user":"usr:ana"}"#,
        );
        assert!(server.collect_sign_in("device").is_err());
        worker.join().unwrap();
    }

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
    fn a_refusal_meant_for_a_program_is_taken_as_it_was_given() {
        let said =
            r#"{"error":"This machine is not registered here. Run `stemma node init` on it."}"#;

        assert_eq!(
            summarise(said),
            "This machine is not registered here. Run `stemma node init` on it."
        );

        // Anything else that happens to be JSON is still read as words.
        assert_eq!(
            summarise(r#"{"trouble":"elsewhere"}"#),
            r#"{"trouble":"elsewhere"}"#
        );
    }

    #[test]
    fn something_that_is_not_an_address_is_refused() {
        for wrong in [
            "",
            "   ",
            "ftp://example.org",
            "ssh://example.org",
            "https://",
            "https://bad host",
            "https://user:secret@example.org",
            "https://example.org?query",
            "https://example.org#fragment",
        ] {
            assert!(normalise(wrong).is_err(), "took `{wrong}`");
        }
    }
}
