//! The sync engine, for as long as the application runs.
//!
//! The engine is the `stemma` library's, the same the daemon runs; here
//! it keeps the machine while the window is open, and after every round
//! tells the window where things stand, which is all the window knows
//! about synchronising.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use stemma::mirror::engine::{self, Role};
use tauri::{AppHandle, Emitter, Runtime};

use crate::home;

pub use stemma::mirror::engine::{Nudge, Said};

/// The name the window listens for.
pub const EVENT: &str = "stemma:mirror";

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
        lines: engine::unsent(&home()),
        ..Said::default()
    }
}

/// Starts the engine, for as long as the application runs.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let (nudges, heard) = engine::channel();
    let _ = NUDGES.set(nudges);
    let app = app.clone();

    std::thread::Builder::new()
        .name("stemma-engine".to_owned())
        .spawn(move || {
            engine::run(&home(), Role::Window, &heard, |said| {
                if let Ok(mut last) = LAST.lock() {
                    *last = Some(said.clone());
                }
                let _ = app.emit(EVENT, said);
            });
        })
        .expect("a thread to keep lines in step on");
}
