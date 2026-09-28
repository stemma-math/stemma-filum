//! The sync engine, for as long as the application runs.
//!
//! One thread keeps this machine's lines in step with the instance: it
//! sends a line a couple of seconds after the editor last saved it, takes
//! in what the instance says changed, and every so often tries again
//! when something is waiting. After every round it tells the window where
//! things stand, which is all the window knows about synchronising.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use stemma::mirror::{Instance, Mirror, PAUSE, Report, Standing, State, Which};
use stemma_core::event::{ClaimChange, Event};
use tauri::{AppHandle, Emitter, Runtime};

use crate::home;

/// The name the window listens for.
pub const EVENT: &str = "stemma:mirror";

/// How long to wait before trying again when something is waiting.
const AGAIN: Duration = Duration::from_secs(30);

/// Something that should make the engine look.
#[derive(Debug, Clone)]
pub enum Nudge {
    /// The editor saved a line.
    Saved,
    /// The stream to the instance opened; anything may have changed.
    Connected,
    /// The instance said something changed.
    Heard(Event),
    /// Somebody wants a round now.
    Now,
}

/// Where things stand, as the window is told.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Said {
    /// Whether the last round reached the instance.
    pub connected: bool,
    /// Why it did not, when it did not.
    pub reason: Option<String>,
    /// Every line here that is not sent yet or needs a decision.
    pub lines: Vec<Standing>,
    /// What the last round did.
    pub report: Report,
}

static NUDGES: OnceLock<Sender<Nudge>> = OnceLock::new();
static LAST: Mutex<Option<Said>> = Mutex::new(None);

/// Wakes the engine.
pub fn nudge(nudge: Nudge) {
    if let Some(nudges) = NUDGES.get() {
        let _ = nudges.send(nudge);
    }
}

/// Where things stood after the last round, or where the lines stand now
/// when there has not been one.
pub fn last() -> Said {
    if let Some(said) = LAST.lock().ok().and_then(|last| last.clone()) {
        return said;
    }

    Said {
        lines: unsent(),
        ..Said::default()
    }
}

/// Starts the engine, for as long as the application runs.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let (nudges, heard) = std::sync::mpsc::channel();
    let _ = NUDGES.set(nudges);
    let app = app.clone();

    std::thread::Builder::new()
        .name("stemma-engine".to_owned())
        .spawn(move || run(&app, &heard))
        .expect("a thread to keep lines in step on");
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
            .unwrap_or(Duration::from_secs(3600))
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
                Event::Line { .. }
                | Event::Claim {
                    change: ClaimChange::Released,
                    ..
                } => self.round = true,
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

fn run<R: Runtime>(app: &AppHandle<R>, heard: &Receiver<Nudge>) {
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
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        // Whatever else arrived meanwhile is part of this round.
        while let Ok(nudge) = heard.try_recv() {
            due.take(nudge);
        }

        let said = round(&mut due);
        if let Ok(mut last) = LAST.lock() {
            *last = Some(said.clone());
        }
        let _ = app.emit(EVENT, said);
    }
}

/// One round with the instance.
fn round(due: &mut Due) -> Said {
    let home = home();
    let full = std::mem::take(&mut due.full);
    let proposals = std::mem::take(&mut due.proposals);
    due.round = false;
    due.send_at = None;
    due.again_at = None;

    let reached = Mirror::at(&home).and_then(|mirror| {
        let instance = Instance::connect(&home)?;
        let mut report = mirror.step(&instance, full, &Which::Paused(PAUSE));

        if proposals && report.stopped.is_none() {
            // What cannot be applied now is still waiting on the instance,
            // and is looked at again with the next full round.
            if let Err(error) = stemma::proposals::apply_waiting(&mirror, &instance) {
                report.stopped = Some(format!("{error:#}"));
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

    let lines = unsent();
    let unsent_with = |troubled: bool| {
        lines
            .iter()
            .any(|line| line.state == State::NotSent && line.trouble.is_some() == troubled)
    };

    if reason.is_some() {
        due.full = full;
        due.proposals = proposals;
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
    }
}

/// Every line here that is not sent yet or needs a decision.
fn unsent() -> Vec<Standing> {
    Mirror::at(&home())
        .and_then(|mirror| mirror.standing())
        .map(|lines| {
            lines
                .into_iter()
                .filter(|line| line.state != State::Sent)
                .collect()
        })
        .unwrap_or_default()
}
