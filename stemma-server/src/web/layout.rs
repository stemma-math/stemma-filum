//! The shell every page is drawn in.
//!
//! Navigation is words, the active destination is marked by a rule on the
//! edge it belongs to, and the mark is a stemma in three strokes: a heavy
//! stem with two hairlines descending from it, which is both the grammar of
//! the product — weight is commitment — and literally a descent tree.

use maud::{DOCTYPE, Markup, PreEscaped, html};
use stemma_identity::{MARK_CENTRE, MARK_ORBIT};

use super::assets::{DESCENT_PATH, STYLESHEET_PATH, SURFACE_PATH};
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
    let groups = instance
        .read(move |connection| crate::store::groups::for_user(connection, &user))
        .await?;

    let machines = super::machines::owned_by(instance, &signed.user.id).await?;

    Ok(Chrome::anonymous(title, name)
        .signed_in(signed.user.clone(), signed.maintainer)
        .holding(Workspace {
            groups,
            machines,
            may_ask: policy.group_requests_open,
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
    /// Whether this page has a descent on it to draw.
    pub draws: bool,
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
            draws: false,
            offer_ways_in: true,
            landing: false,
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
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (chrome.title) " | " (chrome.instance) }
                script {
                    (PreEscaped(
                        "try{if(sessionStorage.getItem('stemma-surface-transition'))document.documentElement.classList.add('surface-pending')}catch(_){}"
                    ))
                }
                link rel="stylesheet" href=(STYLESHEET_PATH);
                @if chrome.draws {
                    script src=(DESCENT_PATH) type="module" {}
                }
                script src=(SURFACE_PATH) type="module" {}
            }
            body class=(body_class) {
                div class="surface-transition" aria-hidden="true" {}
                @if chrome.visitor.is_some() {
                    div class="product-shell" {
                        (sidebar(chrome))
                        div class="product-workspace" {
                            (topbar(chrome))
                            main class="sheet" { (content) }
                            (foot(chrome))
                        }
                    }
                } @else {
                    (masthead(chrome))
                    main class="sheet" { (content) }
                    (foot(chrome))
                }
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

                nav class="nav" hidden[chrome.landing] {
                    @match &chrome.visitor {
                        Some(user) => {
                            (destination("/", "Home", &chrome.current))
                            @if chrome.maintainer {
                                (destination("/admin", "Admin", &chrome.current))
                            }
                            span class="quiet identifier" { (user.id.to_string()) }
                            form method="post" action="/logout" {
                                button class="button button--quiet" type="submit" { "Log out" }
                            }
                        }
                        None if chrome.offer_ways_in => {
                            (destination("/login", "Log in", &chrome.current))
                            @if chrome.registration_open {
                                (destination("/register", "Create an account", &chrome.current))
                            }
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

/// The product's persistent navigation.
///
/// Every destination in it is a page of its own. Groups and machines are
/// the two trees somebody actually works in, so they hang open under their
/// heading rather than sending the reader to a list to pick from.
fn sidebar(chrome: &Chrome) -> Markup {
    let workspace = &chrome.workspace;

    html! {
        aside class="sidebar" {
            a class="wordmark" href="/" {
                (mark())
                span { "Stemma" }
            }
            p class="sidebar__instance" { (chrome.instance) }
            nav class="sidebar__nav" aria-label="Workspace" {
                p class="sidebar__label" { "Workspace" }
                (destination("/", "Overview", &chrome.current))

                details class="tree" data-empty[workspace.groups.is_empty()]
                        open[!workspace.groups.is_empty() || in_a_group(&chrome.current)] {
                    summary class="tree__head" { "Groups" }
                    div class="tree__body" {
                        @for membership in &workspace.groups {
                            (destination(
                                &format!("/g/{}", membership.group.slug()),
                                &membership.group.name,
                                &chrome.current,
                            ))
                        }
                        @if workspace.groups.is_empty() {
                            @if workspace.may_ask {
                                a class="tree__add" href="/groups/new" {
                                    span class="tree__plus" aria-hidden="true" { "+" }
                                    span { "Request a group" }
                                }
                            } @else {
                                p class="tree__empty" { "No groups yet." }
                            }
                        }
                    }
                }

                details class="tree" data-empty[workspace.machines.is_empty()]
                        open[!workspace.machines.is_empty()] {
                    summary class="tree__head" { "Machines" }
                    div class="tree__body" {
                        @for (machine, owed) in &workspace.machines {
                            (waypoint(
                                &format!("/machines/{}", machine.id),
                                &machine.label,
                                *owed,
                                &chrome.current,
                            ))
                        }
                        @if workspace.machines.is_empty() {
                            p class="tree__empty" {
                                "No machines connected."
                            }
                        }
                    }
                }
            }
            div class="sidebar__aside" {
                p { (chrome.instance) }
            }
        }
    }
}

/// Whether the reader is somewhere inside a group.
///
/// A line, a vault and the form that asks for a group are all group work,
/// so the groups tree arrives open even when there is nothing in it yet
/// and the only thing under it is the way to make one.
fn in_a_group(current: &str) -> bool {
    current.starts_with("/g/") || current == "/groups/new"
}

/// Account and instance controls stay above the current workspace.
fn topbar(chrome: &Chrome) -> Markup {
    html! {
        header class="topbar" {
            div {
                p class="topbar__eyebrow" { "Workspace" }
                p class="topbar__title" { (chrome.title) }
            }
            nav class="topbar__actions" aria-label="Account" {
                @if chrome.maintainer {
                    (destination("/admin", "Admin", &chrome.current))
                }
                @if let Some(user) = &chrome.visitor {
                    span class="avatar" aria-hidden="true" {
                        (user.display_name.chars().next().unwrap_or('S'))
                    }
                    span class="topbar__identity" {
                        strong { (user.display_name) }
                        span class="identifier" { (user.id.to_string()) }
                    }
                }
                form method="post" action="/logout" {
                    button class="button button--quiet" type="submit" { "Log out" }
                }
            }
        }
    }
}

/// One place to go, with what is waiting there.
///
/// A count is only ever on a decision somebody owes, never on the state of
/// the mathematics, so a machine with nothing to answer carries nothing.
fn waypoint(href: &str, words: &str, owed: usize, current: &str) -> Markup {
    html! {
        a href=(href) aria-current=[(href == current).then_some("page")] {
            span { (words) }
            @if owed > 0 {
                span class="count" aria-label={ (owed) " to deal with" } { (owed) }
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

/// How far past the mark's orbit the construction rules reach.
const RULE_REACH: f64 = 1.18;

/// The mark, from the identity.
pub fn mark() -> Markup {
    PreEscaped(stemma_identity::MARK_SVG.to_owned())
}

/// The construction the front door draws behind the mark.
///
/// The same box and the same units as [`mark`], so the ring *is* the node
/// orbit and the rules *are* the axes the nodes were set out on. Drawn here
/// rather than in the stylesheet because a ring built out of a border and a
/// border-radius is a second coordinate system, and two coordinate systems
/// for one drawing will come apart.
///
/// Each rule fades out at both ends rather than stopping, which is what a
/// drawn construction line does and a border cannot.
pub fn constellation() -> Markup {
    let reach = MARK_ORBIT * RULE_REACH;
    let (near, far) = (MARK_CENTRE - reach, MARK_CENTRE + reach);
    let top = MARK_CENTRE - MARK_ORBIT;

    // The rule at the top node's height is shorter: it is that node's own
    // line, not one of the axes.
    let (top_near, top_far) = (MARK_CENTRE - reach * 0.7, MARK_CENTRE + reach * 0.7);

    let rules = [
        ("across", (near, MARK_CENTRE), (far, MARK_CENTRE)),
        ("down", (MARK_CENTRE, near), (MARK_CENTRE, far)),
        ("top", (top_near, top), (top_far, top)),
    ];

    let fades: String = rules
        .iter()
        .map(|(name, (x1, y1), (x2, y2))| {
            format!(
                r#"<linearGradient id="stemma-rule-{name}" gradientUnits="userSpaceOnUse" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">
                <stop offset="0" stop-color="currentColor" stop-opacity="0" />
                <stop offset=".2" stop-color="currentColor" stop-opacity="1" />
                <stop offset=".8" stop-color="currentColor" stop-opacity="1" />
                <stop offset="1" stop-color="currentColor" stop-opacity="0" />
            </linearGradient>"#
            )
        })
        .collect();

    let lines: String = rules
        .iter()
        .map(|(name, (x1, y1), (x2, y2))| {
            format!(
                r#"<line class="construction__rule" stroke="url(#stemma-rule-{name})" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" />"#
            )
        })
        .collect();

    PreEscaped(format!(
        r#"<svg class="construction" viewBox="0 0 64 64" fill="none" aria-hidden="true">
            <defs>{fades}</defs>
            <circle class="construction__ring" cx="{MARK_CENTRE}" cy="{MARK_CENTRE}" r="{MARK_ORBIT}" />
            {lines}
        </svg>"#
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every node of the mark, as an offset from the centre of its box.
    fn nodes() -> Vec<(f64, f64)> {
        mark()
            .into_string()
            .split("<circle")
            .skip(1)
            .map(|node| {
                let value = |name: &str| -> f64 {
                    node.split(&format!("{name}=\""))
                        .nth(1)
                        .and_then(|rest| rest.split('"').next())
                        .and_then(|number| number.parse().ok())
                        .unwrap_or_else(|| panic!("a node should carry {name}"))
                };

                (value("cx") - MARK_CENTRE, value("cy") - MARK_CENTRE)
            })
            .collect()
    }

    /// The ring is the orbit, not a circle that looks like it.
    #[test]
    fn the_construction_draws_its_ring_on_the_node_orbit() {
        let drawn = constellation().into_string();
        let radius = drawn
            .split("class=\"construction__ring\"")
            .nth(1)
            .and_then(|rest| rest.split("r=\"").nth(1))
            .and_then(|rest| rest.split('"').next())
            .and_then(|number| number.parse::<f64>().ok())
            .expect("the ring should carry a radius");

        assert!(
            (radius - MARK_ORBIT).abs() < f64::EPSILON,
            "the ring is drawn at {radius}, and the nodes sit at {MARK_ORBIT}"
        );

        for (x, y) in nodes() {
            assert!(
                (x.hypot(y) - radius).abs() < 0.01,
                "the node at ({x}, {y}) is not on the ring"
            );
        }
    }

    /// Every rule fades at both ends, and every one is the far side of the
    /// orbit, so the construction reads as drawn rather than cropped.
    #[test]
    fn every_construction_rule_fades_out_past_the_ring() {
        let drawn = constellation().into_string();
        // The space matters: `<linearGradient` also begins with `<line`.
        let rules: Vec<&str> = drawn.split("<line ").skip(1).collect();

        assert_eq!(rules.len(), 3, "two axes and the top node's own line");

        for rule in &rules {
            let name = rule
                .split("url(#")
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .expect("a rule should be stroked with its fade");

            assert!(
                drawn.contains(&format!(r#"id="{name}""#)),
                "{name} is referenced and never defined"
            );
        }

        assert_eq!(
            drawn.matches("stop-opacity=\"0\"").count(),
            rules.len() * 2,
            "each rule should fade at both of its ends"
        );
    }
}
