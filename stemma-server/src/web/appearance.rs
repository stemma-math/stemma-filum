//! Light, dark, or whichever the reader's system is in.
//!
//! The choice is a cookie, read before any page is drawn so the page
//! arrives in it, and changed by a form, so the switch works with
//! JavaScript off like everything else here. Without the cookie the
//! stylesheet follows the system.

use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::visitor::safe_destination;
use super::{Instance, cookies};

/// The cookie that holds the choice.
pub const COOKIE: &str = "stemma_appearance";

/// How long a choice is kept: a year, renewed whenever it is made.
const KEPT: i64 = 365 * 24 * 60 * 60;

/// What the reader asked the pages to look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    /// As the system is, and as it changes.
    #[default]
    System,
    /// Light, whatever the system says.
    Light,
    /// Dark, whatever the system says.
    Dark,
}

impl Appearance {
    /// Every choice, in the order the switch offers them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The word the cookie and the form use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// What the switch says.
    fn words(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Reads a choice; anything unknown is the system's.
    pub fn parse(word: &str) -> Self {
        match word {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    /// The root's `data-theme`, when it has to say one.
    pub fn theme(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::Light => Some("light"),
            Self::Dark => Some("dark"),
        }
    }

    /// The choice a request carries.
    fn of(headers: &HeaderMap) -> Self {
        cookies::read(headers, COOKIE)
            .map(Self::parse)
            .unwrap_or_default()
    }
}

/// What the page being drawn needs to know about its request.
#[derive(Debug, Clone, Default)]
struct Asked {
    /// The appearance the reader chose.
    appearance: Appearance,
    /// Where the page is, for the switch to come back to.
    path: String,
}

tokio::task_local! {
    static ASKED: Asked;
}

/// Carries the reader's choice to whatever draws the page.
///
/// The shell is drawn by every handler and given no request, so the
/// choice travels beside the request rather than through each of them.
pub async fn remember(request: Request, next: Next) -> Response {
    let asked = Asked {
        appearance: Appearance::of(request.headers()),
        path: request
            .uri()
            .path_and_query()
            .map(|path| path.as_str().to_owned())
            .unwrap_or_else(|| "/".to_owned()),
    };

    ASKED.scope(asked, next.run(request)).await
}

/// The appearance of the page being drawn.
pub fn chosen() -> Appearance {
    ASKED
        .try_with(|asked| asked.appearance)
        .unwrap_or_default()
}

/// Where the page being drawn is.
fn here() -> String {
    ASKED
        .try_with(|asked| asked.path.clone())
        .unwrap_or_else(|_| "/".to_owned())
}

/// What the switch sends.
#[derive(Debug, Deserialize)]
pub struct Choice {
    /// The appearance chosen.
    pub appearance: String,
    /// The page to go back to.
    #[serde(default)]
    pub back: Option<String>,
}

/// Keeps a choice and goes back to the page it was made on.
///
/// There is no form token: the worst another site can do with this is
/// change somebody's colours.
pub async fn choose(
    axum::extract::State(instance): axum::extract::State<Instance>,
    axum::Form(choice): axum::Form<Choice>,
) -> Response {
    let secure = instance.settings.public_url.is_secure();
    let cookie = match Appearance::parse(&choice.appearance) {
        Appearance::System => cookies::clear(COOKIE, secure),
        other => cookies::set(COOKIE, other.as_str(), KEPT, secure),
    };

    let mut headers = HeaderMap::new();
    cookies::attach(&mut headers, cookie);

    (
        headers,
        Redirect::to(&safe_destination(choice.back.as_deref())),
    )
        .into_response()
}

/// The switch: three buttons, the chosen one pressed.
pub fn switch(class: &str) -> Markup {
    let chosen = chosen();

    html! {
        form class={ "appearance " (class) } method="post" action="/appearance" {
            input type="hidden" name="back" value=(here());
            div class="segmented" role="group" aria-label="Appearance" {
                @for choice in Appearance::ALL {
                    button type="submit" name="appearance" value=(choice.as_str())
                           aria-pressed=(if choice == chosen { "true" } else { "false" }) {
                        (choice.words())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_reads_back_as_itself() {
        for choice in Appearance::ALL {
            assert_eq!(Appearance::parse(choice.as_str()), choice);
        }
    }

    #[test]
    fn anything_else_is_left_to_the_system() {
        for word in ["", "Dark", "sepia", "light; Path=/"] {
            assert_eq!(Appearance::parse(word), Appearance::System, "took `{word}`");
        }
    }

    #[test]
    fn only_a_forced_choice_names_a_theme() {
        assert_eq!(Appearance::System.theme(), None);
        assert_eq!(Appearance::Light.theme(), Some("light"));
        assert_eq!(Appearance::Dark.theme(), Some("dark"));
    }

    #[test]
    fn a_page_drawn_outside_a_request_follows_the_system() {
        assert_eq!(chosen(), Appearance::System);
        assert_eq!(here(), "/");
    }
}
