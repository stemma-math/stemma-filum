//! The shell every page is drawn in.

use maud::{DOCTYPE, Markup, html};

use super::assets::{
    GLYPHS_PATH, LANDING_PATH, MANIFEST_PATH, MARK_ANIMATED_PATH, MARK_ON_DARK_PATH,
    MARK_ON_LIGHT_PATH, SHELL_PATH, STYLESHEET_PATH,
};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::Membership;
use crate::store::machines::Machine;
use crate::store::users::User;

/// What the sidebar offers, whatever page is open.
///
/// The tree is the same on every signed-in page, so it is read once here
/// rather than by each page that remembers to, and a page that forgets
/// cannot quietly serve a shorter one.
#[derive(Debug, Default, Clone)]
pub struct Workspace {
    /// The groups this person belongs to.
    pub groups: Vec<Membership>,
    /// The machines they have connected, each with what it is waiting for
    /// them to deal with.
    pub machines: Vec<(Machine, usize)>,
    /// Whether they may ask for a group at all.
    pub may_ask: bool,
    /// How many group requests wait for a maintainer, when the reader is
    /// one.
    pub pending_requests: usize,
    /// When the reader's picture last changed, if they have one.
    pub face: Option<String>,
    /// When each of their groups' pictures last changed, for those with one.
    pub badges: std::collections::HashMap<stemma_core::id::GroupId, String>,
}

/// The shell for a page somebody is signed in to, with its tree read.
pub async fn around(
    instance: &Instance,
    signed: &SignedIn,
    title: impl Into<String>,
) -> Result<Chrome, WebError> {
    let policy = instance.policy().await?;
    let name = super::front::instance_name(instance, &policy);

    let user = signed.user.id.clone();
    let (groups, face, badges) = instance
        .read(move |connection| {
            let groups = crate::store::groups::for_user(connection, &user)?;
            let face = crate::store::pictures::user_changed(connection, &user)?;

            let mut badges = std::collections::HashMap::new();
            for membership in &groups {
                let id = &membership.group.id;
                if let Some(changed) = crate::store::pictures::group_changed(connection, id)? {
                    badges.insert(id.clone(), changed);
                }
            }

            Ok((groups, face, badges))
        })
        .await?;

    let machines = super::machines::owned_by(instance, &signed.user.id).await?;

    let pending_requests = match signed.maintainer {
        true => instance
            .read(crate::store::groups::open_requests)
            .await?
            .len(),
        false => 0,
    };

    Ok(Chrome::anonymous(title, name)
        .signed_in(signed.user.clone(), signed.maintainer)
        .holding(Workspace {
            groups,
            machines,
            may_ask: policy.group_requests_open,
            pending_requests,
            face,
            badges,
        }))
}

/// What the shell needs to know about this page.
pub struct Chrome {
    /// What the browser tab says, before the instance's name.
    pub title: String,
    /// What the instance calls itself.
    pub instance: String,
    /// Which destination is the current one.
    pub current: String,
    /// What the sidebar offers.
    pub workspace: Workspace,
    /// Who is reading, when anybody is.
    pub visitor: Option<User>,
    /// Whether they operate this instance.
    pub maintainer: bool,
    /// Whether anybody may create an account right now.
    pub registration_open: bool,
    /// Whether the bar offers a way in at all.
    ///
    /// An instance with no accounts offers none: a log-in link there would
    /// be offering to sign in as somebody who does not exist.
    pub offer_ways_in: bool,
    /// Whether this is the instance's full-screen front door.
    pub landing: bool,
}

impl Chrome {
    /// The shell for a page nobody is signed in to.
    pub fn anonymous(title: impl Into<String>, instance: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            instance: instance.into(),
            current: String::new(),
            workspace: Workspace::default(),
            visitor: None,
            maintainer: false,
            registration_open: false,
            offer_ways_in: true,
            landing: false,
        }
    }

    /// Takes the ways in out of the bar, for a page that is the way in.
    pub fn without_ways_in(mut self) -> Self {
        self.offer_ways_in = false;
        self
    }

    /// Makes this page the full-screen front door.
    pub fn landing(mut self) -> Self {
        self.landing = true;
        self
    }

    /// Marks which destination this page is.
    pub fn at(mut self, current: impl Into<String>) -> Self {
        self.current = current.into();
        self
    }

    /// Gives the shell the tree to draw.
    pub fn holding(mut self, workspace: Workspace) -> Self {
        self.workspace = workspace;
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
    let body_class = if chrome.landing {
        "landing-page"
    } else if chrome.visitor.is_some() {
        "product-page"
    } else {
        "auth-page"
    };

    html! {
        (DOCTYPE)
        html lang="en" data-theme=[super::appearance::chosen().theme()] {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (chrome.title) " | " (chrome.instance) }
                link rel="icon" href="/favicon.ico" sizes="48x48";
                link rel="icon" href="/favicon.svg" type="image/svg+xml";
                link rel="apple-touch-icon" href="/apple-touch-icon.png";
                link rel="manifest" href=(MANIFEST_PATH);
                meta name="theme-color" content=(stemma_identity::THEME_COLOUR);
                link rel="stylesheet" href=(STYLESHEET_PATH);
                @if chrome.visitor.is_some() {
                    script src=(SHELL_PATH) type="module" {}
                } @else if chrome.landing {
                    script src=(MARK_ANIMATED_PATH) {}
                    script src=(LANDING_PATH) type="module" {}
                }
            }
            body class=(body_class) {
                @if chrome.visitor.is_some() {
                    div class="shell" {
                        (sidebar(chrome))
                        div class="shell__scrim" data-drawer-close {}
                        div class="shell__work" {
                            (mobile_bar(chrome))
                            main class="page" id="content" { (content) }
                        }
                    }
                } @else if chrome.landing {
                    main class="sheet" { (content) }
                } @else {
                    (masthead(chrome))
                    main class="sheet" { (content) }
                    (foot(chrome))
                }
            }
        }
    }
}

/// The bar across the top of a page nobody is signed in to.
fn masthead(chrome: &Chrome) -> Markup {
    html! {
        header class="masthead" {
            a class="brand" href="/" {
                (mark_on_page())
                span class="wordmark" { "Stemma" }
            }

            nav class="nav" aria-label="Account" {
                @if chrome.offer_ways_in {
                    (destination("/login", "Log in", &chrome.current))
                    @if chrome.registration_open {
                        (destination("/register", "Create an account", &chrome.current))
                    }
                }
            }
        }
    }
}

/// The product's persistent navigation.
///
/// Every destination in it is a page of its own. Groups and machines are
/// the two lists somebody actually works in, so they are always open
/// under their label rather than sending the reader to a list to pick
/// from. The brand stays at the top and the reader at the foot; the lists
/// between them scroll.
fn sidebar(chrome: &Chrome) -> Markup {
    let workspace = &chrome.workspace;

    html! {
        aside class="sidebar" id="sidebar" {
            div class="sidebar__head" {
                a class="brand" href="/" {
                    (mark_on_dark())
                    span class="brand__words" {
                        span class="wordmark" { "Stemma" }
                        span class="brand__instance" { (chrome.instance) }
                    }
                }
                span class="sidebar__version" { "v" (env!("CARGO_PKG_VERSION")) }
            }

            nav class="sidebar__nav" aria-label="Workspace" {
                (item("/", glyph("home"), "Overview", 0, &chrome.current))

                div class="sidebar__section" {
                    span { "Groups" }
                    @if workspace.may_ask {
                        a class="sidebar__add" href="/groups/new" aria-label="Request a research group"
                          title="Request a research group"
                          aria-current=[("/groups/new" == chrome.current).then_some("page")] {
                            (glyph("add"))
                        }
                    }
                }
                @for membership in &workspace.groups {
                    (item(
                        &format!("/g/{}", membership.group.slug()),
                        super::pictures::badge(
                            &membership.group.id,
                            &membership.group.name,
                            workspace.badges.get(&membership.group.id).map(String::as_str),
                            "sidebar__badge",
                        ),
                        &membership.group.name,
                        0,
                        &chrome.current,
                    ))
                }
                @if workspace.groups.is_empty() {
                    p class="sidebar__empty" { "No groups yet." }
                }

                div class="sidebar__section" { span { "Machines" } }
                @for (machine, owed) in &workspace.machines {
                    (item(
                        &format!("/machines/{}", machine.id),
                        glyph("machine"),
                        &machine.label,
                        *owed,
                        &chrome.current,
                    ))
                }
                @if workspace.machines.is_empty() {
                    p class="sidebar__empty" { "No machines connected." }
                }

                @if chrome.maintainer {
                    div class="sidebar__section" { span { "Instance" } }
                    (item("/admin", glyph("shield"), "Administration", workspace.pending_requests, &chrome.current))
                }
            }

            (super::appearance::switch("appearance--sidebar"))

            @if let Some(user) = &chrome.visitor {
                div class="user-card" {
                    a class="user-card__me" href="/profile" title="Your profile"
                      aria-current=[("/profile" == chrome.current).then_some("page")] {
                        (super::pictures::face(&user.id, &user.display_name, workspace.face.as_deref(), ""))
                        span class="user-card__who" {
                            span class="user-card__name" {
                                span { (user.display_name) }
                                @if chrome.maintainer {
                                    span class="user-card__role" { "Admin" }
                                }
                            }
                            span class="identifier" { (user.id.to_string()) }
                        }
                    }
                    form method="post" action="/logout" {
                        button class="user-card__out" type="submit" aria-label="Log out" title="Log out" {
                            (glyph("log-out"))
                        }
                    }
                }
            }
        }
    }
}

/// The bar a narrow window shows in place of the sidebar, with the button
/// that brings it out as a drawer.
fn mobile_bar(chrome: &Chrome) -> Markup {
    html! {
        header class="mobile-bar" {
            button class="mobile-bar__menu" type="button" aria-controls="sidebar"
                   aria-expanded="false" aria-label="Menu" data-drawer-toggle {
                (glyph("menu"))
            }
            a class="brand" href="/" {
                (mark_on_dark())
                span class="wordmark" { "Stemma" }
            }
            span class="brand__instance" { (chrome.instance) }
        }
    }
}

/// One place to go in the sidebar, with what is waiting there.
///
/// A count is only ever on a decision somebody owes, never on the state of
/// the mathematics, so a machine with nothing to answer carries nothing.
fn item(href: &str, icon: Markup, words: &str, owed: usize, current: &str) -> Markup {
    let here = href == current || (href != "/" && current.starts_with(&format!("{href}/")));

    html! {
        a class="sidebar__item" href=(href) aria-current=[here.then_some("page")] {
            (icon)
            span class="sidebar__words" { (words) }
            @if owed > 0 {
                span class="sidebar__count" aria-label={ (owed) " to deal with" } { (owed) }
            }
        }
    }
}

/// One place to go in the bar of a public page.
fn destination(href: &str, words: &str, current: &str) -> Markup {
    html! {
        a href=(href) aria-current=[(href == current).then_some("page")] { (words) }
    }
}

/// The line at the foot of a public page.
fn foot(chrome: &Chrome) -> Markup {
    html! {
        footer class="foot" {
            span { (chrome.instance) }
            span { "stemma " (env!("CARGO_PKG_VERSION")) }
            (super::appearance::switch("appearance--foot"))
        }
    }
}

/// A line glyph from the identity's sprite.
pub fn glyph(name: &str) -> Markup {
    html! {
        svg class="glyph" aria-hidden="true" {
            use href={ (GLYPHS_PATH) "#" (name) } {}
        }
    }
}

/// Up to two initials for a person or a group: the first letters of its
/// first two words, or the first two of a single word.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters: String = match words.as_slice() {
        [] => "S".to_owned(),
        [one] => one.chars().take(1).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };

    letters.to_uppercase()
}

/// The hidden CSRF field every form posted by a signed-in visitor carries.
pub fn signature(signed: &crate::web::visitor::SignedIn) -> Markup {
    html! {
        input type="hidden" name="form_token" value=(signed.session.csrf_token);
    }
}

/// The mark, for the navy surfaces.
///
/// An image rather than inline SVG: the drawing's gradients carry ids, and
/// two copies on one page would share them.
pub fn mark_on_dark() -> Markup {
    html! { img class="mark" src=(MARK_ON_DARK_PATH) alt=""; }
}

/// The mark, for light surfaces.
pub fn mark_on_light() -> Markup {
    html! { img class="mark" src=(MARK_ON_LIGHT_PATH) alt=""; }
}

/// The mark, for the page, which is light or dark as the reader chose:
/// both drawings, and the stylesheet shows the one that fits.
pub fn mark_on_page() -> Markup {
    html! {
        img class="mark mark--for-light" src=(MARK_ON_LIGHT_PATH) alt="";
        img class="mark mark--for-dark" src=(MARK_ON_DARK_PATH) alt="";
    }
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
