// The window's appearance: light, dark, or whichever the system is in.
// The choice is shared with the account when the instance is reachable.

import { api } from "./api";
import { serially } from "./lifecycle";

/** What the appearance setting can be. */
export type Appearance = "system" | "light" | "dark";

const KEY = "stemma.appearance";
const PENDING = "stemma.appearance.pending";
const send = serially();

function kept(): Appearance {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    // Storage that cannot be read leaves the system in charge.
  }
  return "system";
}

function pending(): boolean {
  try { return localStorage.getItem(PENDING) === "1"; } catch { return false; }
}

/** Sets the root's `data-theme`, which the tokens follow; without one they
 * follow the system. */
function show(choice: Appearance) {
  if (choice === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}

class Look {
  choice = $state<Appearance>(kept());
  #pending = pending();

  constructor() {
    show(this.choice);
  }

  async refresh() {
    const remote = await api.appearance();
    if (this.#pending || remote === null) {
      if (this.#pending || this.choice !== "system") await this.#save(this.choice);
      return;
    }
    this.choice = remote;
    show(remote);
    try {
      if (remote === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, remote);
    } catch {
      // The account still keeps the choice.
    }
  }

  async choose(next: Appearance) {
    this.choice = next;
    show(next);
    this.#pending = true;
    try {
      if (next === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, next);
      localStorage.setItem(PENDING, "1");
    } catch {
      // Not kept, but still shown until the window closes.
    }
    await this.#save(next);
  }

  async #save(next: Appearance) {
    await send(() => api.setAppearance(next));
    if (this.choice !== next) return;
    this.#pending = false;
    try { localStorage.removeItem(PENDING); } catch { /* The remote choice was saved. */ }
  }
}

export const appearance = new Look();
