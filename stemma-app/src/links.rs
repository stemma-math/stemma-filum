//! Opening the application from a `stemma://` link.
//!
//! A link to a proposal opens its review. The link is kept until the
//! webview takes it, so one that arrives before the webview listens, or
//! while nobody is signed in, is not lost; the event only tells a window
//! that is already listening to come and take it.

use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;

/// The name the window listens for when a proposal link is opened, with
/// the proposal's identifier as its payload.
pub const OPEN_REVIEW_EVENT: &str = "stemma:open-review";

/// The proposal a link asked to review, until the webview takes it.
#[derive(Default)]
pub struct PendingReview(Mutex<Option<String>>);

impl PendingReview {
    fn put(&self, proposal: String) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(proposal);
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Keeps the link the application was opened with, and every link
/// opened while it runs.
pub fn listen<R: Runtime>(app: &AppHandle<R>) -> Result<(), tauri_plugin_deep_link::Error> {
    if let Some(proposal) = app
        .deep_link()
        .get_current()?
        .and_then(|urls| urls.iter().find_map(|url| proposal_link(url.as_str())))
    {
        app.state::<PendingReview>().put(proposal);
    }

    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        if let Some(proposal) = event
            .urls()
            .iter()
            .find_map(|url| proposal_link(url.as_str()))
        {
            handle.state::<PendingReview>().put(proposal.clone());
            let _ = handle.emit(OPEN_REVIEW_EVENT, proposal);
        }
    });
    Ok(())
}

/// Takes the proposal a link asked to review, if one is waiting.
#[tauri::command]
pub fn take_open_review(state: tauri::State<'_, PendingReview>) -> Option<String> {
    state.take()
}

/// The proposal a `stemma://proposal/<id>` link names, when it is one.
fn proposal_link(url: &str) -> Option<String> {
    let id = url
        .strip_prefix("stemma://proposal/")?
        .trim_end_matches('/');
    id.parse::<stemma_core::id::ProposalId>()
        .ok()
        .map(|id| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::{PendingReview, proposal_link};

    #[test]
    fn only_proposal_links_open_a_review() {
        let id = "prp:01K5YQ4TQ2VZ8H3M9N0P1R2S3T";
        assert_eq!(
            proposal_link(&format!("stemma://proposal/{id}")),
            Some(id.to_owned())
        );
        assert_eq!(
            proposal_link(&format!("stemma://proposal/{id}/")),
            Some(id.to_owned())
        );
        assert!(proposal_link("stemma://settings").is_none());
        assert!(proposal_link("stemma://proposal/anything").is_none());
    }

    #[test]
    fn a_pending_review_is_taken_once_and_the_latest_link_wins() {
        let pending = PendingReview::default();
        pending.put("prp:first".to_owned());
        pending.put("prp:second".to_owned());

        assert_eq!(pending.take().as_deref(), Some("prp:second"));
        assert_eq!(pending.take(), None);
    }
}
