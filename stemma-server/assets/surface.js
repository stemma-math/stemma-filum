const transitionKey = "stemma-surface-transition";

if (sessionStorage.getItem(transitionKey)) {
    sessionStorage.removeItem(transitionKey);
    document.body.classList.add("surface-arriving");
    document.documentElement.classList.remove("surface-pending");
}

document.querySelectorAll("[data-surface-exit]").forEach((link) => {
    link.addEventListener("click", (event) => {
        if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        document.body.classList.add("surface-leaving");
        sessionStorage.setItem(transitionKey, "true");
        const delay = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 360;
        window.setTimeout(() => window.location.assign(link.href), delay);
    });
});

// A page restored from the back/forward cache comes back wearing the classes
// it wore when it was left, and a page somebody navigated away from was left
// mid-transition: the surface drawn over it and its own content faded out.
// Going back has to show the page, not the state it was abandoned in.
window.addEventListener("pageshow", (event) => {
    if (!event.persisted) return;

    sessionStorage.removeItem(transitionKey);
    document.documentElement.classList.remove("surface-pending");
    document.body.classList.remove("surface-leaving", "surface-arriving");
});
