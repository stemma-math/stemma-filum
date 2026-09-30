// The signed-in shell: the light behind the sidebar's foot, the sidebar
// as a drawer on a narrow window, previews of a chosen picture, and short
// actions in dialogs.

import { light } from "./light.js";

const toggle = document.querySelector("[data-drawer-toggle]");
const sidebar = document.getElementById("sidebar");

function setDrawer(open) {
    document.body.classList.toggle("drawer-open", open);
    toggle?.setAttribute("aria-expanded", String(open));
    if (open) sidebar?.querySelector("a, button")?.focus();
    else if (document.activeElement && sidebar?.contains(document.activeElement)) toggle?.focus();
}

// The front page's light, as one still frame: the same dome and haze,
// rising behind the appearance switch and the account. It does not move,
// because the sidebar is where the work is.
const glow = sidebar?.querySelector(".sidebar__light");
const lit = glow instanceof HTMLCanvasElement
    ? light(glow, { int: 0.9, size: 1.18, cy: 0.75, haze: 0.32, grain: 0.36, speed: 0, wob: 1, rim: 1 })
    : null;
if (lit && glow) {
    lit.draw(0);
    sidebar?.classList.add("sidebar--lit");
    new ResizeObserver(() => lit.draw(0)).observe(glow);
}

toggle?.addEventListener("click", () => setDrawer(!document.body.classList.contains("drawer-open")));

document.querySelectorAll("[data-drawer-close]").forEach((element) => {
    element.addEventListener("click", () => setDrawer(false));
});

document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && document.body.classList.contains("drawer-open")) setDrawer(false);
});

// A picture is shown as it will look, in the avatar's shape and crop,
// once it is chosen and before it is saved; it can be swapped for another
// or put back. Without JavaScript the form is a file field and a button.
document.querySelectorAll("form.picture-form").forEach((form) => {
    const input = form.querySelector("input[type=file]");
    const current = form.querySelector(".picture-form__current");
    const chosen = form.querySelector(".picture-form__chosen");
    const image = chosen?.querySelector("img");
    const choose = form.querySelector("[data-picture-choose]");
    const save = form.querySelector("[data-picture-save]");
    const cancel = form.querySelector("[data-picture-cancel]");
    const remove = form.querySelector("[form=remove-picture]");
    const hint = form.querySelector("[data-picture-hint]");
    if (!(input instanceof HTMLInputElement) || !chosen || !image || !choose || !save || !cancel) return;

    form.classList.add("picture-form--live");
    const first = choose.textContent;
    const rules = hint?.textContent ?? "";
    let address = null;

    function show() {
        const file = input.files?.[0] ?? null;
        if (address) URL.revokeObjectURL(address);
        address = file ? URL.createObjectURL(file) : null;
        if (address) image.setAttribute("src", address);
        else image.removeAttribute("src");
        chosen.hidden = !file;
        if (current) current.hidden = !!file;
        choose.hidden = false;
        choose.textContent = file ? "Choose another" : first;
        save.hidden = !file;
        save.textContent = "Save the picture";
        cancel.hidden = !file;
        if (remove) remove.hidden = !!file;
        if (hint) hint.textContent = file ? "This is how it will look. It is not saved until you save it." : rules;
    }

    choose.addEventListener("click", () => input.click());
    cancel.addEventListener("click", () => {
        input.value = "";
        show();
    });
    input.addEventListener("change", show);
    show();
});

// Short actions open in a dialog over the page. Each has its own address,
// which is a page when it is opened directly or without JavaScript; a link
// to it marked `data-dialog-link` fetches that page and shows its panel
// here instead. The panel's form posts from the dialog: an answer that
// sends the reader elsewhere is followed, and one that is a panel again —
// the form with what went wrong, or what was done — takes the dialog's
// place. A panel marked `data-dialog-changed` means the page behind is out
// of date, so it is drawn again when the dialog closes.

/** The one dialog every short action is shown in. */
let modal = null;
/** Whether the page behind has to be drawn again on closing. */
let stale = false;

function dialog() {
    if (modal) return modal;
    modal = document.createElement("dialog");
    modal.className = "dialog";
    modal.tabIndex = -1;
    modal.setAttribute("aria-labelledby", "dialog-title");
    // A click on the backdrop closes it, as Escape does.
    modal.addEventListener("click", (event) => {
        if (event.target === modal) modal.close();
    });
    modal.addEventListener("close", () => {
        if (stale) location.reload();
        else modal.replaceChildren();
    });
    document.body.append(modal);
    return modal;
}

/** Shows a panel from a fetched page in the dialog. */
function show(panel) {
    const shown = dialog();
    if (panel.hasAttribute("data-dialog-changed")) stale = true;
    shown.replaceChildren(...[...panel.childNodes].map((node) => document.importNode(node, true)));
    for (const form of shown.querySelectorAll("form")) form.addEventListener("submit", send);
    if (!shown.open) shown.showModal();
    const first = shown.querySelector("[autofocus]");
    if (first instanceof HTMLElement) first.focus();
    else shown.focus();
}

/** Says, inside the dialog, that what was asked could not be done. */
function refuse(words) {
    const body = dialog().querySelector(".dialog__body");
    if (!body) return;
    body.querySelector(".message--error")?.remove();
    const message = document.createElement("div");
    message.className = "message message--error";
    message.setAttribute("role", "alert");
    const text = document.createElement("p");
    text.className = "message__text";
    text.textContent = words;
    message.append(text);
    body.querySelector(".dialog__head")?.after(message);
}

/** A fetched page, read into a document of its own. */
function parsed(html) {
    return new DOMParser().parseFromString(html, "text/html");
}

async function open(address) {
    let answer;
    try {
        answer = await fetch(address, { credentials: "same-origin" });
    } catch {
        location.assign(address);
        return;
    }
    const page = parsed(await answer.text());
    const panel = page.querySelector("[data-dialog-panel]");
    // Anything that is not a dialog's page is a page to go to.
    if (!panel || answer.redirected) {
        location.assign(answer.url);
        return;
    }
    show(panel);
}

async function send(event) {
    const form = event.currentTarget;
    if (!(form instanceof HTMLFormElement)) return;
    event.preventDefault();
    const submitter = event.submitter instanceof HTMLButtonElement ? event.submitter : null;
    const buttons = [...form.querySelectorAll("button")];
    buttons.forEach((button) => (button.disabled = true));
    const data = new FormData(form, submitter);
    const body = form.enctype === "multipart/form-data" ? data : new URLSearchParams(data);
    // A button's own `formaction`, where it has one, is where it posts.
    const action = submitter?.hasAttribute("formaction") ? submitter.formAction : form.action;
    let answer;
    try {
        answer = await fetch(action, { method: "POST", body, credentials: "same-origin" });
    } catch {
        buttons.forEach((button) => (button.disabled = false));
        refuse("The instance could not be reached. Check the connection and try again.");
        return;
    }
    // Done, and somewhere else to be.
    if (answer.redirected) {
        location.assign(answer.url);
        return;
    }
    const page = parsed(await answer.text());
    const panel = page.querySelector("[data-dialog-panel]");
    if (panel) {
        show(panel);
        return;
    }
    buttons.forEach((button) => (button.disabled = false));
    refuse(page.querySelector("main p")?.textContent?.trim() || "That could not be done.");
}

document.addEventListener("click", (event) => {
    if (!(event.target instanceof Element)) return;
    const closer = event.target.closest("dialog [data-dialog-close]");
    if (closer) {
        event.preventDefault();
        closer.closest("dialog")?.close();
        return;
    }
    const link = event.target.closest("a[data-dialog-link]");
    if (!(link instanceof HTMLAnchorElement)) return;
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (document.body.classList.contains("drawer-open")) setDrawer(false);
    open(link.href);
});
