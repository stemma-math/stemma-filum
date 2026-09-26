import "@stemma-math/identity/fonts.css";
import "@stemma-math/identity/tokens.css";
import "@stemma-math/identity/components.css";
import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { fail } from "./lib/status.svelte";

// macOS draws its window buttons over the sidebar, which leaves room.
if (navigator.userAgent.includes("Mac")) document.documentElement.classList.add("mac");

// Uncaught failures become toasts rather than failing silently.
window.addEventListener("error", (event) => fail(event.message));
window.addEventListener("unhandledrejection", (event) => fail(event.reason));

mount(App, { target: document.getElementById("app")! });
