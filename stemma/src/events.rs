//! Hearing what changes on the instance, as it changes.
//!
//! One stream per program, held open for as long as it runs: the
//! instance says what changed and where, and whoever is listening asks for
//! the thing itself. A stream that drops is opened again from the last
//! notice heard, so nothing in between is missed, and one the instance
//! can no longer bring up to date says so with a reset.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use stemma_core::event::Event;

use crate::home::Home;
use crate::session;

/// What a listener hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// The stream is open. Anything may have changed while it was not,
    /// so whoever listens should look at everything once.
    Connected,
    /// Something changed.
    Changed(Event),
    /// The stream dropped, and why. It is opened again by itself.
    Lost(String),
}

/// How long to wait before opening a stream again, by how many times in
/// a row it has failed: soon at first, then less often, never giving up.
const AGAIN: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// Listens until `stop` is set, telling `heard` everything.
///
/// Blocks, so it belongs on a thread of its own. It reads the credential
/// again every time it connects, so signing out and back in is followed.
pub fn follow(home: &Home, stop: &AtomicBool, mut heard: impl FnMut(Heard)) {
    let mut last: Option<i64> = None;
    let mut failures = 0usize;

    while !stop.load(Ordering::Relaxed) {
        let connected = session::connect(home)
            .and_then(|(credentials, server)| server.listen(&credentials.token, last));

        let reason = match connected {
            Ok(stream) => {
                failures = 0;
                match read(stream, stop, &mut last, &mut heard) {
                    // The instance closed it, as it does every few
                    // minutes: open it again at once.
                    Ok(()) => continue,
                    Err(error) => format!("{error:#}"),
                }
            }
            Err(error) => format!("{error:#}"),
        };

        if stop.load(Ordering::Relaxed) {
            break;
        }

        heard(Heard::Lost(reason));
        let wait = AGAIN[failures.min(AGAIN.len() - 1)];
        failures += 1;
        pause(wait, stop);
    }
}

/// Waits, looking up now and then in case it is told to stop.
fn pause(wait: Duration, stop: &AtomicBool) {
    let step = Duration::from_millis(200);
    let mut waited = Duration::ZERO;

    while waited < wait && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        waited += step;
    }
}

/// Reads one stream to its end.
fn read(
    stream: impl BufRead,
    stop: &AtomicBool,
    last: &mut Option<i64>,
    heard: &mut impl FnMut(Heard),
) -> Result<()> {
    let mut frame = Frame::default();

    for line in stream.lines() {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }

        let line = line?;
        let Some(done) = frame.take(&line) else {
            continue;
        };

        if let Some(id) = done.id {
            *last = Some(id);
        }

        match done.name.as_deref() {
            Some("ready") => heard(Heard::Connected),
            _ => {
                // A notice this build cannot read is from a newer instance,
                // and what it would have said is caught up with on the
                // next look at everything.
                if let Ok(event) = serde_json::from_str::<Event>(&done.data) {
                    heard(Heard::Changed(event));
                }
            }
        }
    }

    Ok(())
}

/// One notice being read, line by line, in the event-stream format.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Frame {
    id: Option<i64>,
    name: Option<String>,
    data: String,
}

impl Frame {
    /// Takes one line; hands the notice over when the line ends it.
    fn take(&mut self, line: &str) -> Option<Frame> {
        if line.is_empty() {
            let done = std::mem::take(self);
            // A frame with nothing in it is the keep-alive's comment.
            return (!done.data.is_empty() || done.name.is_some()).then_some(done);
        }

        if line.starts_with(':') {
            return None;
        }

        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);

        match field {
            "id" => self.id = value.parse().ok(),
            "event" => self.name = Some(value.to_owned()),
            "data" => {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
            _ => {}
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemma_core::event::LineChange;
    use stemma_core::repository::Side;

    #[test]
    fn a_stream_is_read_notice_by_notice_and_remembers_where_it_was() {
        let stream = "event: ready\nid: 7\ndata: {}\n\n\
                      :\n\n\
                      id: 8\ndata: {\"kind\":\"line\",\"group\":\"algebra\",\"side\":\"shared\",\
                      \"line\":\"quotients\",\"change\":\"written\",\"revision\":3}\n\n\
                      id: 9\ndata: {\"kind\":\"something new\"}\n\n";
        let mut last = None;
        let mut heard = Vec::new();

        read(
            stream.as_bytes(),
            &AtomicBool::new(false),
            &mut last,
            &mut |said| heard.push(said),
        )
        .unwrap();

        assert_eq!(
            heard,
            [
                Heard::Connected,
                Heard::Changed(Event::Line {
                    group: "algebra".to_owned(),
                    side: Side::Shared,
                    line: "quotients".to_owned(),
                    change: LineChange::Written,
                    revision: Some(3),
                }),
            ]
        );
        assert_eq!(
            last,
            Some(9),
            "even the one it could not read counts as heard"
        );
    }

    #[test]
    fn a_notice_on_several_lines_is_one_notice() {
        let mut frame = Frame::default();

        assert_eq!(frame.take("data: one"), None);
        assert_eq!(frame.take("data:two"), None);
        let done = frame.take("").unwrap();

        assert_eq!(done.data, "one\ntwo");
    }
}
