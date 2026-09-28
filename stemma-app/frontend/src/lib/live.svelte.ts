// What the instance says changed, and looking again at what it touched.
//
// The native side keeps one stream open and passes every notice here.
// A notice names what changed; what is shown is read again from the
// source, never taken from the notice. Coming back to the window or to
// the network looks at everything once, in case the stream missed it.

import { jobs } from "../line/jobs.svelte";
import { api } from "./api";
import { app } from "./app.svelte";
import type { ServerEvent } from "./types";

type Said =
  | { heard: "connected" }
  | { heard: "changed"; event: ServerEvent }
  | { heard: "lost"; reason: string };

/** How long to gather notices before acting on them: a burst of writing
 * is one look, not one per keystroke that reached the server. */
const GATHER = 300;

class Live {
  /** Whether the stream to the instance is open. Null until it has tried. */
  connected = $state<boolean | null>(null);
  /** Why it is not, when it is not. */
  reason = $state<string | null>(null);

  #pending = new Set<string>();
  #timer = 0;

  /** Starts listening; returns how to stop. */
  async start(): Promise<() => void> {
    const listen = window.__TAURI__?.event?.listen;
    const stopHearing = listen
      ? await listen<Said>("stemma:heard", (event) => this.#hear(event.payload))
      : () => {};

    const everything = () => this.#soon("everything");
    const seen = () => {
      if (document.visibilityState === "visible") everything();
    };
    window.addEventListener("focus", everything);
    window.addEventListener("online", everything);
    document.addEventListener("visibilitychange", seen);

    return () => {
      stopHearing();
      window.removeEventListener("focus", everything);
      window.removeEventListener("online", everything);
      document.removeEventListener("visibilitychange", seen);
    };
  }

  #hear(said: Said) {
    switch (said.heard) {
      case "connected":
        this.connected = true;
        this.reason = null;
        this.#soon("everything");
        break;
      case "lost":
        this.connected = false;
        this.reason = said.reason;
        break;
      case "changed":
        this.#changed(said.event);
        break;
    }
  }

  #changed(event: ServerEvent) {
    switch (event.kind) {
      case "line":
      case "group":
        this.#soon("lines");
        break;
      case "claim": {
        const open = app.line?.opened.entry;
        if (open && open.group === event.group && open.side === event.side && open.name === event.line) {
          this.#soon("claim");
        }
        break;
      }
      case "job":
        this.#soon("jobs");
        break;
      case "proposal":
        this.#soon("proposals");
        break;
      case "machine":
        this.#soon("jobs");
        this.#soon("offers");
        break;
      case "reset":
        this.#soon("everything");
        break;
    }
  }

  #soon(what: string) {
    if (!app.signed) return;
    this.#pending.add(what);
    window.clearTimeout(this.#timer);
    this.#timer = window.setTimeout(() => void this.#look(), GATHER);
  }

  async #look() {
    const wanted = new Set(this.#pending);
    this.#pending.clear();
    const all = wanted.has("everything");
    const looks: Promise<unknown>[] = [];

    if (all || wanted.has("lines")) looks.push(app.lookAgain());
    if (all || wanted.has("claim")) looks.push(app.line?.lookAtClaim() ?? Promise.resolve());
    if (all || wanted.has("jobs")) looks.push(jobs.refresh());
    if (all || wanted.has("proposals")) looks.push(app.refreshWaiting().catch(() => {}));
    if (all || wanted.has("offers")) {
      const line = app.line;
      if (line) looks.push(api.offers(line.opened.entry.group).then((offers) => line.offersChanged(offers)).catch(() => {}));
    }

    await Promise.allSettled(looks);
  }
}

export const live = new Live();
