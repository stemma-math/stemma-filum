import "@stemma-math/identity/fonts.css";
import "@stemma-math/identity/tokens.css";
import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { said } from "./lib/api";
import { tell } from "./lib/status.svelte";

// Uncaught failures go to the message bar rather than failing silently.
window.addEventListener("error", (event) => tell(event.message));
window.addEventListener("unhandledrejection", (event) => tell(said(event.reason)));

mount(App, { target: document.getElementById("app")! });
