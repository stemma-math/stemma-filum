//! The engine, running: rounds with the instance, on their own.
//!
//! One loop keeps this machine's lines in step: it sends a line a couple
//! of seconds after it was last saved, takes in what the instance says
//! changed, applies what its owner accepted, builds and publishes what a
//! stretch of writing came to once it is over, and tries again when
//! something is waiting. After every round it says where things stand.
//!
//! The application and the daemon both run it, and one at a time: the
//! application holds the machine for as long as it is open, and the
//! daemon's engine only works while it is closed, so changes still leave
//! with the window shut.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use stemma_core::event::{ClaimChange, Event};
use stemma_core::repository::Side;

use super::{Instance, Mirror, PAUSE, Report, Standing, State, Which};
use crate::home::Home;

/// How long to wait before trying again when something is waiting.
const AGAIN: Duration = Duration::from_secs(30);

/// How often an idle engine looks whether anything is owed.
const IDLE: Duration = Duration::from_secs(60);

/// How long writing may go quiet before it is built: as long as the
/// instance waits before it takes the writing to be over.
pub const QUIET: Duration = Duration::from_secs(10 * 60);

/// Which program an engine runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The application, which holds the machine while it is open.
    Window,
    /// The daemon, which works only while the application is closed.
    Background,
}

/// Something that should make the engine look.
#[derive(Debug, Clone)]
pub enum Nudge {
    /// A line was saved.
    Saved,
    /// The stream to the instance opened; anything may have changed.
    Connected,
    /// The instance said something changed.
    Heard(Event),
    /// Somebody wants a round now.
    Now,
}

/// Where things stand after a round.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Said {
    /// Whether the round reached the instance.
    pub connected: bool,
    /// Why it did not, when it did not.
    pub reason: Option<String>,
    /// Every line here that is not sent yet or needs a decision.
    pub lines: Vec<Standing>,
    /// What the round did.
    pub report: Report,
    /// The repositories whose pages were built and published.
    pub published: Vec<String>,
}

/// A way to nudge an engine, and the engine's end of it.
pub fn channel() -> (Sender<Nudge>, Receiver<Nudge>) {
    std::sync::mpsc::channel()
}

/// What the engine has to do next.
#[derive(Debug, Default)]
struct Due {
    /// A round that also brings the files at every repository's root.
    full: bool,
    /// A round.
    round: bool,
    /// Accepted proposals may be waiting to be applied.
    proposals: bool,
    /// Lines let go since the last round, by group, repository and name.
    released: Vec<(String, Side, String)>,
    /// Libraries whose dependants all need checking again.
    libraries: Vec<(String, Side)>,
    /// When a line that was saved is ready to leave.
    send_at: Option<Instant>,
    /// When to try again because something is waiting.
    again_at: Option<Instant>,
}

impl Due {
    fn now(&self) -> bool {
        let now = Instant::now();
        self.full
            || self.round
            || self.send_at.is_some_and(|at| at <= now)
            || self.again_at.is_some_and(|at| at <= now)
    }

    fn wait(&self) -> Duration {
        let now = Instant::now();
        [self.send_at, self.again_at]
            .into_iter()
            .flatten()
            .map(|at| at.saturating_duration_since(now))
            .min()
            .unwrap_or(IDLE)
            .min(IDLE)
    }

    fn take(&mut self, nudge: Nudge) {
        match nudge {
            Nudge::Saved => self.send_at = Some(Instant::now() + PAUSE),
            Nudge::Connected => {
                self.full = true;
                self.proposals = true;
            }
            Nudge::Now => self.round = true,
            Nudge::Heard(event) => match event {
                Event::Line { .. } => self.round = true,
                Event::Claim {
                    group,
                    side,
                    line,
                    change: ClaimChange::Released,
                } => {
                    self.released.push((group, side, line));
                    self.round = true;
                }
                Event::Library { group, side, .. } => {
                    self.libraries.push((group, side));
                    self.full = true;
                }
                Event::Group { .. } | Event::Reset => {
                    self.full = true;
                    self.proposals = true;
                }
                Event::Proposal { state, .. } if state == "accepted" => {
                    self.proposals = true;
                    self.round = true;
                }
                _ => {}
            },
        }
    }
}

/// Runs the engine until every way of nudging it is gone, telling `told`
/// where things stand after every round.
pub fn run(home: &Home, role: Role, heard: &Receiver<Nudge>, mut told: impl FnMut(&Said)) {
    // The application keeps the machine for as long as it runs. Should a
    // second one be open, the first keeps it, and the second's rounds are
    // done all the same: the store is locked round by round.
    let _window = match role {
        Role::Window => super::Store::at(home.state_dir()).hold_window().ok(),
        Role::Background => None,
    };

    let mut due = Due {
        full: true,
        proposals: true,
        ..Due::default()
    };

    loop {
        if !due.now() {
            match heard.recv_timeout(due.wait()) {
                Ok(nudge) => {
                    due.take(nudge);
                    continue;
                }
                Err(RecvTimeoutError::Timeout) if !owed(home) => continue,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        // Whatever else arrived meanwhile is part of this round.
        while let Ok(nudge) = heard.try_recv() {
            due.take(nudge);
        }

        if role == Role::Background && super::Store::at(home.state_dir()).window_open() {
            // The application is doing this; what was asked for is still
            // owed when it closes. Until then the engine waits to be
            // nudged, or looks again after a while, rather than asking
            // the same question over and over.
            match heard.recv_timeout(IDLE) {
                Ok(nudge) => due.take(nudge),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            continue;
        }

        let said = round(home, &mut due);
        told(&said);
    }
}

/// Whether anything here waits to be sent or built, so an idle engine
/// has a reason for a round.
fn owed(home: &Home) -> bool {
    let Ok(mirror) = Mirror::at(home) else {
        return false;
    };

    let unsent = mirror
        .standing()
        .is_ok_and(|lines| lines.iter().any(|line| line.state == State::NotSent));
    let unbuilt = mirror
        .due_builds(&[], QUIET)
        .is_ok_and(|due| !due.is_empty());

    unsent || unbuilt
}

/// One round with the instance.
fn round(home: &Home, due: &mut Due) -> Said {
    let full = std::mem::take(&mut due.full);
    let proposals = std::mem::take(&mut due.proposals);
    let released = std::mem::take(&mut due.released);
    let libraries = std::mem::take(&mut due.libraries);
    due.round = false;
    due.send_at = None;
    due.again_at = None;

    let mut published = Vec::new();
    let reached = Mirror::at(home).and_then(|mirror| {
        for (group, side) in &libraries {
            mirror.rebuild(group, *side)?;
        }
        let instance = Instance::connect(home)?;
        let mut report = mirror.step(&instance, full, &Which::Paused(PAUSE));

        if report.stopped.is_none() {
            if proposals {
                // What cannot be applied now is still waiting on the
                // instance, and is looked at again with the next full round.
                if let Err(error) = crate::proposals::apply_waiting(&mirror, &instance) {
                    report.stopped = Some(format!("{error:#}"));
                }
            }

            match built(home, &mirror, &released) {
                Ok(done) => published = done,
                Err(error) => report.stopped = Some(format!("{error:#}")),
            }
        }

        Ok(report)
    });

    let (report, reason) = match reached {
        Ok(report) => {
            let reason = report.stopped.clone();
            (report, reason)
        }
        Err(error) => (Report::default(), Some(format!("{error:#}"))),
    };

    let lines = unsent(home);
    let unsent_with = |troubled: bool| {
        lines
            .iter()
            .any(|line| line.state == State::NotSent && line.trouble.is_some() == troubled)
    };

    if reason.is_some() {
        due.full = full;
        due.proposals = proposals;
        due.libraries = libraries;
        due.released = released;
        due.again_at = Some(Instant::now() + AGAIN);
    } else if unsent_with(false) {
        // Saved inside the pause: it leaves on the next round.
        due.again_at = Some(Instant::now() + PAUSE);
    } else if unsent_with(true) {
        // Refused for a reason that may pass, such as another machine
        // writing the line; its letting go also wakes the engine.
        due.again_at = Some(Instant::now() + AGAIN);
    }

    Said {
        connected: reason.is_none(),
        reason,
        lines,
        report,
        published,
    }
}

/// Builds and publishes whatever writing here is over.
fn built(home: &Home, mirror: &Mirror, released: &[(String, Side, String)]) -> Result<Vec<String>> {
    let released: Vec<String> = mirror
        .records()?
        .into_iter()
        .filter(|record| {
            released
                .iter()
                .any(|(group, side, name)| record.is(group, *side, name))
        })
        .map(|record| record.id)
        .collect();

    crate::build::publish_due(home, mirror, &released, QUIET)
}

/// Every line here that is not sent yet or needs a decision.
pub fn unsent(home: &Home) -> Vec<Standing> {
    Mirror::at(home)
        .and_then(|mirror| mirror.standing())
        .map(|lines| {
            lines
                .into_iter()
                .filter(|line| line.state != State::Sent)
                .collect()
        })
        .unwrap_or_default()
}
