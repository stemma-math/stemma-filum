// The signed-in shell: the sidebar as a drawer on a narrow window, and
// dialogs opened from a button that names them.

const toggle = document.querySelector("[data-drawer-toggle]");
const sidebar = document.getElementById("sidebar");

function setDrawer(open) {
    document.body.classList.toggle("drawer-open", open);
    toggle?.setAttribute("aria-expanded", String(open));
    if (open) sidebar?.querySelector("a, button")?.focus();
    else if (document.activeElement && sidebar?.contains(document.activeElement)) toggle?.focus();
}

toggle?.addEventListener("click", () => setDrawer(!document.body.classList.contains("drawer-open")));

document.querySelectorAll("[data-drawer-close]").forEach((element) => {
    element.addEventListener("click", () => setDrawer(false));
});

document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && document.body.classList.contains("drawer-open")) setDrawer(false);
});

// `data-dialog="id"` on a button opens that `dialog`; without JavaScript
// the button is a link to the section the dialog stands in for. Closing
// returns focus to the button, which the browser does for a modal dialog.
document.querySelectorAll("[data-dialog]").forEach((opener) => {
    const dialog = document.getElementById(opener.dataset.dialog);
    if (!(dialog instanceof HTMLDialogElement)) return;
    opener.addEventListener("click", (event) => {
        event.preventDefault();
        dialog.showModal();
    });
});

document.querySelectorAll("dialog [data-dialog-close]").forEach((closer) => {
    closer.addEventListener("click", () => closer.closest("dialog")?.close());
});

// A dialog closes on a click on its backdrop, as it does on Escape.
document.querySelectorAll("dialog.dialog").forEach((dialog) => {
    dialog.addEventListener("click", (event) => {
        if (event.target === dialog) dialog.close();
    });
});
