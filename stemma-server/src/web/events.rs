//! Telling machines what changed, as it changes.
//!
//! One long answer per watcher, in the event-stream format browsers and
//! programs both read: a notice per change the watcher may see, each with
//! its place in the order, so a watcher that drops can say where it was
//! and hear what it missed. When what it missed is no longer kept it is
//! told to start over instead.
//!
//! A stream ends on its own after a while and the watcher reconnects,
//! which is when its credential is looked at again: a machine taken back
//! stops hearing within minutes, without the stream checking on every
//! notice.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event as Sent, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use stemma_core::event::{Event, LAST_EVENT_HEADER};
use stemma_core::id::UserId;
use tokio::time::Instant;

use super::api::Credential;
use super::{Instance, WebError};
use crate::store::events::{self, Notice};

/// How long one stream lasts before the watcher is asked to come back.
const LASTS: Duration = Duration::from_secs(10 * 60);

/// How long a quiet stream waits before looking again anyway, in case a
/// change was made somewhere that does not ring.
const LOOKS_AGAIN: Duration = Duration::from_secs(15);

/// Where a watcher says it was.
#[derive(Debug, Default, Deserialize)]
pub struct Since {
    /// The last notice it saw, when it cannot send the header.
    pub since: Option<i64>,
}

/// The stream.
pub async fn stream(
    State(instance): State<Instance>,
    credential: Credential,
    headers: HeaderMap,
    Query(asked): Query<Since>,
) -> Result<Sse<impl Stream<Item = Result<Sent, Infallible>>>, WebError> {
    let said = headers
        .get(LAST_EVENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .or(asked.since);

    let (latest, too_old) = instance
        .peek(move |connection| {
            let latest = events::latest(connection)?;
            let too_old = match said {
                Some(since) => events::is_too_old(connection, since)?,
                None => false,
            };

            Ok((latest, too_old))
        })
        .await?;

    // A watcher that says nothing is starting now and hears from now on;
    // one that was here before hears what it missed, or is told it
    // missed too much.
    let from = match (said, too_old) {
        (Some(since), false) => since.min(latest),
        _ => latest,
    };

    let mut first = VecDeque::from([ready(latest)]);
    if too_old {
        first.push_back(sent(&Notice {
            id: latest,
            event: Event::Reset,
        }));
    }

    let watching = Watching {
        instance,
        reader: credential.user.id,
        last: from,
        waiting: first,
        ends: Instant::now() + LASTS,
    };

    Ok(Sse::new(stream::unfold(watching, next)).keep_alive(KeepAlive::default()))
}

/// Where one stream is.
struct Watching {
    instance: Instance,
    reader: UserId,
    /// The last notice handed over.
    last: i64,
    /// Notices read and not yet handed over.
    waiting: VecDeque<Sent>,
    /// When this stream stops.
    ends: Instant,
}

/// The next thing to send, waiting for it if nothing is ready.
async fn next(mut watching: Watching) -> Option<(Result<Sent, Infallible>, Watching)> {
    loop {
        if let Some(ready) = watching.waiting.pop_front() {
            return Some((Ok(ready), watching));
        }

        if Instant::now() >= watching.ends {
            return None;
        }

        // Listening starts before the reading, so a change made while
        // this reads is still heard afterwards.
        let changed = watching.instance.changed.clone();
        let rung = changed.notified();
        tokio::pin!(rung);
        rung.as_mut().enable();

        let reader = watching.reader.clone();
        let since = watching.last;
        let found = watching
            .instance
            .peek(move |connection| events::after(connection, since, &reader))
            .await;

        match found {
            Ok(notices) if !notices.is_empty() => {
                for notice in &notices {
                    watching.waiting.push_back(sent(notice));
                }
                watching.last = notices.last().map_or(since, |notice| notice.id);
                continue;
            }
            Ok(_) => {}
            // A read that failed ends the stream, and the watcher comes
            // back to one that works.
            Err(_) => return None,
        }

        let wait = LOOKS_AGAIN.min(watching.ends.saturating_duration_since(Instant::now()));
        tokio::select! {
            () = &mut rung => {}
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// The first thing every stream says: that it is listening, and from
/// where, so a watcher knows what to name when it comes back.
fn ready(latest: i64) -> Sent {
    Sent::default()
        .event("ready")
        .id(latest.to_string())
        .data("{}")
}

/// One notice, as it goes over the wire.
fn sent(notice: &Notice) -> Sent {
    Sent::default()
        .id(notice.id.to_string())
        .data(serde_json::to_string(&notice.event).unwrap_or_else(|_| "{}".to_owned()))
}
