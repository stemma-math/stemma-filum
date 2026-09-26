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
// the button is a link to the section the dialog stands in for. Opening
// focuses the dialog rather than its first control, unless a field is
// marked `autofocus`. Closing returns focus to the button, which the
// browser does for a modal dialog.
document.querySelectorAll("[data-dialog]").forEach((opener) => {
    const dialog = document.getElementById(opener.dataset.dialog);
    if (!(dialog instanceof HTMLDialogElement)) return;
    opener.addEventListener("click", (event) => {
        event.preventDefault();
        dialog.showModal();
        if (!dialog.querySelector("[autofocus]")) {
            dialog.tabIndex = -1;
            dialog.focus();
        }
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

// A form that does something hard to take back names the consequence and
// asks first: `data-confirm-title`, `data-confirm-text` and the exact verb
// in `data-confirm-verb`. Without JavaScript it simply submits.
document.querySelectorAll("form[data-confirm-title]").forEach((form) => {
    form.addEventListener("submit", (event) => {
        if (form.dataset.confirmed) return;
        event.preventDefault();

        const dialog = document.createElement("dialog");
        dialog.className = "dialog";
        const body = document.createElement("div");
        body.className = "dialog__body";
        const title = document.createElement("h2");
        title.className = "dialog__title";
        title.textContent = form.dataset.confirmTitle;
        const text = document.createElement("p");
        text.className = "dialog__text";
        text.textContent = form.dataset.confirmText ?? "";
        const actions = document.createElement("div");
        actions.className = "dialog__actions";
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.className = "button";
        cancel.textContent = "Cancel";
        const confirm = document.createElement("button");
        confirm.type = "button";
        confirm.className = "button button--primary button--danger";
        confirm.textContent = form.dataset.confirmVerb ?? "Confirm";
        actions.append(cancel, confirm);
        body.append(title, text, actions);
        dialog.append(body);
        document.body.append(dialog);

        cancel.addEventListener("click", () => dialog.close());
        dialog.addEventListener("click", (click) => {
            if (click.target === dialog) dialog.close();
        });
        dialog.addEventListener("close", () => dialog.remove());
        confirm.addEventListener("click", () => {
            form.dataset.confirmed = "true";
            dialog.close();
            form.requestSubmit(event.submitter);
        });
        dialog.showModal();
        cancel.focus();
    });
});
