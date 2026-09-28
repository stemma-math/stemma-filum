// What the instance says changed, and looking again at what it touched.
//
// The native side keeps one stream open and passes every notice here.
// A notice names what changed; what is shown is read again from the
// source, never taken from the notice. Lines are the engine's: it takes
// in what changed and says so after every round. Coming back to the
// window or to the network looks at everything once, in case the stream
// missed it.

import { api, onHeard, onMirrored } from "./api";
import { app } from "./app.svelte";
import { jobs } from "./jobs.svelte";
import { subscriptions } from "./lifecycle";
import { sameLine } from "./lines";
import { tell } from "./status.svelte";
import type { Heard, ServerEvent } from "./types";

/** How long to gather notices before acting on them: a burst of writing
 * is one look, not one per keystroke that reached the server. */
const GATHER = 300;

/** What a notice asks to look at again. */
type Look = "everything" | "lines" | "claim" | "jobs" | "proposals" | "offers";

class Live {
  /** Whether the stream to the instance is open. Null until it has tried. */
  connected = $state<boolean | null>(null);
  /** Why it is not, when it is not. */
  reason = $state<string | null>(null);

  #pending = new Set<Look>();
  #newProposals = new Set<string>();
  #timer = 0;

  /** Starts listening; returns how to stop. */
  start(): () => void {
    const listeners = subscriptions();
    listeners.add(onHeard((heard) => this.#hear(heard)));
    listeners.add(onMirrored((mirrored) => void app.mirror(mirrored)));

    const everything = () => this.#soon("everything");
    const seen = () => {
      if (document.visibilityState === "visible") everything();
    };
    window.addEventListener("focus", everything);
    window.addEventListener("online", everything);
    document.addEventListener("visibilitychange", seen);

    return () => {
      listeners.stop();
      window.clearTimeout(this.#timer);
      window.removeEventListener("focus", everything);
      window.removeEventListener("online", everything);
      document.removeEventListener("visibilitychange", seen);
    };
  }

  #hear(heard: Heard) {
    switch (heard.heard) {
      case "connected":
        this.connected = true;
        this.reason = null;
        this.#soon("everything");
        break;
      case "lost":
        this.connected = false;
        this.reason = heard.reason;
        break;
      case "changed":
        this.#changed(heard.event);
        break;
    }
  }

  #changed(event: ServerEvent) {
    switch (event.kind) {
      case "group":
        this.#soon("lines");
        break;
      case "claim": {
        const open = app.line?.opened.entry;
        if (open && sameLine(open, { ...event, name: event.line })) this.#soon("claim");
        break;
      }
      case "job":
        this.#soon("jobs");
        break;
      case "proposal":
        if (event.state === "awaiting_review" && !app.waiting.some((one) => one.proposal === event.proposal)) {
          this.#newProposals.add(event.proposal);
        }
        this.#soon("proposals");
        break;
      case "formal_audit": {
        const open = app.line?.opened.entry;
        if (open && open.group === event.group && open.name === event.line) {
          tell(
            event.state === "audit_withdrawn"
              ? `The formal audit for ${event.block} was withdrawn; dependent work should be checked.`
              : `The formal statement for ${event.block} was audited.`,
          );
        }
        break;
      }
      case "machine":
        this.#soon("jobs");
        this.#soon("offers");
        break;
      case "reset":
        this.#soon("everything");
        break;
    }
  }

  #soon(what: Look) {
    if (!app.signed) return;
    this.#pending.add(what);
    window.clearTimeout(this.#timer);
    this.#timer = window.setTimeout(() => void this.#look(), GATHER);
  }

  async #look() {
    const wanted = new Set(this.#pending);
    this.#pending.clear();
    const newProposals = new Set(this.#newProposals);
    this.#newProposals.clear();
    const all = wanted.has("everything");
    const looks: Promise<unknown>[] = [];

    if (all || wanted.has("lines")) looks.push(app.lookAgain());
    if (all || wanted.has("claim")) looks.push(app.line?.lookAtClaim() ?? Promise.resolve());
    if (all || wanted.has("jobs")) looks.push(jobs.refresh());
    if (all || wanted.has("proposals")) looks.push((async () => {
      const waiting = await app.refreshWaiting();
      for (const proposal of newProposals) {
        const arrived = waiting.find((one) => one.proposal === proposal);
        if (arrived) tell(`New proposal for ${arrived.line}.`);
      }
      await app.refreshReview();
    })());
    if (all || wanted.has("offers")) {
      const line = app.line;
      if (line) looks.push(api.offers(line.opened.entry.group).then((offers) => line.offersChanged(offers)));
    }

    // Whatever cannot be read now is read again on the next notice.
    await Promise.allSettled(looks);
  }
}

export const live = new Live();
