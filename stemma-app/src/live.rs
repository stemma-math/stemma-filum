//! Hearing the instance, and passing it on to the window.
//!
//! One thread for as long as the application runs, following the stream
//! the `stemma` library opens and handing every notice to the webview,
//! which decides what to look at again.

use serde::Serialize;
use stemma::events::Heard;
use stemma_core::event::Event;
use tauri::{AppHandle, Emitter, Runtime};

use crate::home;

/// The name the window listens for.
pub const EVENT: &str = "stemma:heard";

/// What the window is told.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "heard", rename_all = "snake_case")]
enum Said {
    /// The stream is open; anything may have changed while it was not.
    Connected,
    /// Something changed.
    Changed {
        /// What.
        event: Event,
    },
    /// The stream dropped. It is opened again by itself.
    Lost {
        /// Why, in words.
        reason: String,
    },
}

impl From<Heard> for Said {
    fn from(heard: Heard) -> Self {
        match heard {
            Heard::Connected => Self::Connected,
            Heard::Changed(event) => Self::Changed { event },
            Heard::Lost(reason) => Self::Lost { reason },
        }
    }
}

/// Starts listening, for as long as the application runs.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();

    std::thread::Builder::new()
        .name("stemma-events".to_owned())
        .spawn(move || {
            let stop = std::sync::atomic::AtomicBool::new(false);

            stemma::events::follow(&home(), &stop, |heard| {
                let _ = app.emit(EVENT, Said::from(heard));
            });
        })
        .expect("a thread to listen on");
}
