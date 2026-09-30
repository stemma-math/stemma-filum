// The appearance switch, without leaving the page.
//
// The switch is a form that posts the choice and comes back, which is
// how it works without JavaScript. Here the choice is applied in place,
// with a cross-fade, and still posted so the cookie keeps it for the
// next page. Anything drawn from the page's colours listens for
// `stemma:appearance` and draws again.

const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

/** Puts the page in the chosen appearance and presses its buttons. */
function apply(choice) {
    const root = document.documentElement;
    if (choice === "light" || choice === "dark") root.dataset.theme = choice;
    else delete root.dataset.theme;
    document.querySelectorAll("form.appearance button[name=appearance]").forEach((button) => {
        button.setAttribute("aria-pressed", String(button.value === choice));
    });
    document.dispatchEvent(new CustomEvent("stemma:appearance", { detail: choice }));
}

/** Changes the appearance, fading from the old one to the new. */
function change(choice) {
    if (reduce.matches) {
        apply(choice);
        return;
    }
    if (document.startViewTransition) {
        document.startViewTransition(() => apply(choice));
        return;
    }
    const root = document.documentElement;
    root.classList.add("appearance-changing");
    apply(choice);
    setTimeout(() => root.classList.remove("appearance-changing"), 320);
}

document.addEventListener("submit", (event) => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement) || !form.classList.contains("appearance")) return;
    const button = event.submitter;
    if (!(button instanceof HTMLButtonElement) || button.name !== "appearance") return;
    event.preventDefault();
    if (button.getAttribute("aria-pressed") === "true") return;

    change(button.value);

    // The answer is a way back to this page, which is already here.
    const body = new URLSearchParams(new FormData(form, button));
    fetch(form.action, { method: "POST", body, redirect: "manual", credentials: "same-origin" }).catch(() => {});
});
