//! The shell every page is drawn in.
//!
//! Navigation is words, the active destination is marked by a rule on the
//! edge it belongs to, and the mark is a stemma in three strokes: a heavy
//! stem with two hairlines descending from it, which is both the grammar of
//! the product — weight is commitment — and literally a descent tree.

use maud::{DOCTYPE, Markup, PreEscaped, html};

use super::WebError;
use super::assets::{DESCENT_PATH, STYLESHEET_PATH};
use crate::store::users::User;

/// What the shell needs to know about this page.
pub struct Chrome {
    /// What the browser tab says, before the instance's name.
    pub title: String,
    /// What the instance calls itself.
    pub instance: String,
    /// Which destination is the current one.
    pub current: &'static str,
    /// Who is reading, when anybody is.
    pub visitor: Option<User>,
    /// Whether they operate this instance.
    pub maintainer: bool,
    /// Whether anybody may create an account right now.
    pub registration_open: bool,
    /// Whether this page has a descent on it to draw.
    pub draws: bool,
    /// Whether the bar offers a way in at all.
    ///
    /// An instance with no accounts offers none: a log-in link there would
    /// be offering to sign in as somebody who does not exist.
    pub offer_ways_in: bool,
}

impl Chrome {
    /// The shell for a page nobody is signed in to.
    pub fn anonymous(title: impl Into<String>, instance: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            instance: instance.into(),
            current: "",
            visitor: None,
            maintainer: false,
            registration_open: false,
            draws: false,
            offer_ways_in: true,
        }
    }

    /// Takes the ways in out of the bar, for a page that is the way in.
    pub fn without_ways_in(mut self) -> Self {
        self.offer_ways_in = false;
        self
    }

    /// Says that this page has a descent to draw.
    pub fn drawing(mut self) -> Self {
        self.draws = true;
        self
    }

    /// Marks which destination this page is.
    pub fn at(mut self, current: &'static str) -> Self {
        self.current = current;
        self
    }

    /// Says who is reading.
    pub fn signed_in(mut self, user: User, maintainer: bool) -> Self {
        self.visitor = Some(user);
        self.maintainer = maintainer;
        self
    }

    /// Says whether an account can be created.
    pub fn registration(mut self, open: bool) -> Self {
        self.registration_open = open;
        self
    }
}

/// Draws a page.
pub fn page(chrome: &Chrome, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (chrome.title) " — " (chrome.instance) }
                link rel="stylesheet" href=(STYLESHEET_PATH);
                @if chrome.draws {
                    script src=(DESCENT_PATH) defer {}
                }
            }
            body {
                (masthead(chrome))
                main class="sheet" { (content) }
                (foot(chrome))
            }
        }
    }
}

/// The bar across the top.
fn masthead(chrome: &Chrome) -> Markup {
    html! {
        header class="masthead" {
            div class="masthead__inner" {
                a class="wordmark" href="/" {
                    (mark())
                    span { "stemma" }
                }

                nav class="nav" {
                    @match &chrome.visitor {
                        Some(user) => {
                            (destination("/", "Home", chrome.current))
                            @if chrome.maintainer {
                                (destination("/admin", "Admin", chrome.current))
                            }
                            span class="quiet identifier" { (user.id.to_string()) }
                            form method="post" action="/logout" {
                                button class="button button--quiet" type="submit" { "Log out" }
                            }
                        }
                        None if chrome.offer_ways_in => {
                            (destination("/login", "Log in", chrome.current))
                            @if chrome.registration_open {
                                (destination("/register", "Create an account", chrome.current))
                            }
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

/// One place to go.
fn destination(href: &str, words: &str, current: &str) -> Markup {
    html! {
        @if href == current {
            a href=(href) aria-current="page" { (words) }
        } @else {
            a href=(href) { (words) }
        }
    }
}

/// The line at the foot of every page.
fn foot(chrome: &Chrome) -> Markup {
    html! {
        footer class="foot" {
            div class="foot__inner" {
                span { (chrome.instance) }
                span { "stemma " (env!("CARGO_PKG_VERSION")) }
            }
        }
    }
}

/// The hidden field that says a form came from a page we served.
pub fn signature(signed: &crate::web::visitor::SignedIn) -> Markup {
    html! {
        input type="hidden" name="form_token" value=(signed.session.csrf_token);
    }
}

/// The mark: a heavy stem with two hairlines descending from it.
///
/// Written out rather than built element by element, because the elements
/// of an SVG close themselves and the HTML ones around them do not.
pub fn mark() -> Markup {
    PreEscaped(
        r#"<svg class="mark" width="14" height="17" viewBox="0 0 14 17" fill="none" aria-hidden="true">
            <path d="M2 1 V16" stroke="currentColor" stroke-width="2.2" />
            <path d="M2 5 H8 V9.5" stroke="currentColor" stroke-width="1" />
            <path d="M2 10.5 H12 V15" stroke="currentColor" stroke-width="1" />
        </svg>"#
            .to_owned(),
    )
}

/// The page shown when a request could not be answered.
pub fn failure(error: &WebError) -> Markup {
    let chrome = Chrome::anonymous(
        error.status().canonical_reason().unwrap_or("Error"),
        "Stemma",
    );

    page(
        &chrome,
        html! {
            div class="stack" {
                h1 { (error.status().as_u16()) " " (error.status().canonical_reason().unwrap_or("")) }
                p { (error.explain()) }
                p { a href="/" { "Go to the front page" } }
            }
        },
    )
}
