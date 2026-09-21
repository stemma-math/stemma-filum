const transitionKey = "stemma-surface-transition";

if (sessionStorage.getItem(transitionKey)) {
    sessionStorage.removeItem(transitionKey);
    document.body.classList.add("surface-arriving");
}

document.querySelectorAll("[data-surface-exit]").forEach((link) => {
    link.addEventListener("click", (event) => {
        if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        document.body.classList.add("surface-leaving");
        sessionStorage.setItem(transitionKey, "true");
        const delay = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 420;
        window.setTimeout(() => window.location.assign(link.href), delay);
    });
});
