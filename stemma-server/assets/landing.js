// The front page's light, and the passage between it and the sign-in.
//
// The front page draws a dome of dark with the brand light rising under
// it. Going in, the light withdraws towards the top edge and the page
// turns into the sign-in, light or dark as the reader chose. The sign-in and the registration are on
// this page, so the passage is one movement; the address follows it, and
// the browser's back and forward buttons play it either way. Reached
// directly, the sign-in is a page of its own and nothing moves.

import { light } from "./light.js";

const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The drawing's settings. */
const P = { int: 0.7, size: 1.18, cy: 0.75, haze: 0.32, grain: 0.36, speed: reduce ? 0 : 1, wob: 1, rim: 1 };

/** How long the whole passage takes, in milliseconds. */
const DURATION = reduce ? 500 : 1600;

const smooth = (from, to, x) => {
    const k = Math.min(Math.max((x - from) / (to - from), 0), 1);
    return k * k * (3 - 2 * k);
};

/** Starts fast, so the first frame already answers the click. */
const easeOut = (k) => 1 - Math.pow(1 - Math.min(Math.max(k, 0), 1), 3);

const doorway = document.querySelector(".doorway");
const canvas = document.getElementById("doorway-light");
const hero = document.querySelector(".doorway__hero");
const stage = document.querySelector(".doorway__stage");
const inside = document.querySelector(".doorway__inside");
const panels = [...document.querySelectorAll(".doorway__panel")];

const drawn = canvas ? light(canvas, P) : null;
if (drawn) document.body.classList.add("lit");

const mark = window.StemmaLogo && stage
    ? window.StemmaLogo.mount(stage.querySelector("canvas"), {
        mode: 2,
        intensity: 1,
        speed: 1,
        onBreath: (value) => stage.style.setProperty("--glow", value.toFixed(3)),
        onFail: () => stage.classList.add("still"),
    })
    : null;
if (!mark) stage?.classList.add("still");

/** Where each view lives. */
const PATHS = { home: "/", login: "/login", register: "/register" };

const viewAt = (path) => Object.keys(PATHS).find((view) => PATHS[view] === path) ?? "home";

/** How far in the passage is: 0 on the front page, 1 at the sign-in. */
let at = 0;
/** Where it is heading, and since when. */
let heading = null;

/** Lays the page out for a point of the passage. */
function show(point) {
    const gone = smooth(0, 0.22, point);
    const come = smooth(0.45, 0.85, point);
    hero.style.opacity = String(1 - gone);
    hero.style.transform = `scale(${1 - 0.035 * gone})`;
    inside.style.opacity = String(come);
    inside.style.visibility = point > 0.001 ? "visible" : "hidden";
    const card = inside.querySelector(".doorway__panel:not([hidden]) > *");
    if (card) card.style.transform = `translateY(${((1 - come) * 16).toFixed(2)}px)`;
    document.body.classList.toggle("inside", point > 0.5);
}

/** Shows the sign-in or the registration inside. */
function choose(view) {
    for (const panel of panels) panel.hidden = panel.dataset.panel !== view;
    inside.querySelectorAll("[data-view]").forEach((link) => {
        if (!link.closest(".nav")) return;
        if (link.dataset.view === view) link.setAttribute("aria-current", "page");
        else link.removeAttribute("aria-current");
    });
}

function settle() {
    const isInside = at === 1;
    hero.inert = isInside;
    inside.inert = !isInside;
    document.querySelectorAll(".pressed").forEach((pressed) => pressed.classList.remove("pressed"));
    if (isInside) {
        mark?.pause();
        const first = inside.querySelector(".doorway__panel:not([hidden]) input:not([type=hidden])");
        if (first && window.matchMedia("(pointer: fine)").matches) first.focus({ preventScroll: true });
    } else {
        mark?.play();
    }
}

/** Goes to a view, moving the light when it means going in or out. */
function go(view, { instant = false } = {}) {
    const target = view === "home" ? 0 : 1;
    if (view !== "home") choose(view);
    const title = view === "home" ? doorway.dataset.title : panels.find((panel) => panel.dataset.panel === view)?.dataset.title;
    if (title) document.title = title;
    if (instant) {
        at = target;
        heading = null;
        show(at);
        drawn?.draw(at);
        settle();
        return;
    }
    if (target === at && !heading) return;
    hero.inert = true;
    inside.inert = true;
    mark?.play();
    heading = { from: at, to: target, since: performance.now() };
}

// One loop draws everything: the light breathing on the front page, and
// the passage. At the sign-in, once still, it draws nothing.
let last = performance.now();
function frame(now) {
    const seconds = Math.min((now - last) / 1000, 0.1);
    last = now;
    if (heading) {
        const span = Math.abs(heading.to - heading.from) * DURATION;
        const k = span ? Math.min((now - heading.since) / span, 1) : 1;
        at = heading.from + (heading.to - heading.from) * easeOut(k);
        show(at);
        if (k === 1) {
            at = heading.to;
            heading = null;
            settle();
        }
    }
    if (drawn && (heading || at < 1)) {
        drawn.advance(seconds, !!heading);
        drawn.draw(at);
    }
    requestAnimationFrame(frame);
}

document.addEventListener("click", (event) => {
    const link = event.target instanceof Element ? event.target.closest("a[data-view]") : null;
    if (!link || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    const view = link.dataset.view;
    if (view === viewAt(location.pathname)) return;
    link.classList.add("pressed");
    history.pushState({ view }, "", PATHS[view]);
    go(view);
});

// A link inside the sign-in to the registration, or back, stays here.
inside.querySelectorAll('a[href="/login"], a[href="/register"]').forEach((link) => {
    if (!link.dataset.view) link.dataset.view = viewAt(link.getAttribute("href"));
});

window.addEventListener("popstate", () => go(viewAt(location.pathname)));

// A page brought back from the browser's cache is shown as its address
// says, without moving.
window.addEventListener("pageshow", (event) => {
    if (event.persisted) go(viewAt(location.pathname), { instant: true });
});

window.addEventListener("resize", () => {
    if (drawn && at === 1) drawn.draw(1);
});

// The light withdraws to the page's own colours, which change with the
// appearance.
function recolour() {
    drawn?.recolour();
    if (drawn && at === 1) drawn.draw(1);
}
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", recolour);
document.addEventListener("stemma:appearance", recolour);

go(viewAt(location.pathname), { instant: true });
requestAnimationFrame(frame);
