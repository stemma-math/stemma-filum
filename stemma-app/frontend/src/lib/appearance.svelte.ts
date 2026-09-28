// The window's appearance: light, dark, or whichever the system is in.
// The choice belongs to this machine, so the window keeps it itself.

/** What the appearance setting can be. */
export type Appearance = "system" | "light" | "dark";

const KEY = "stemma.appearance";

function kept(): Appearance {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    // Storage that cannot be read leaves the system in charge.
  }
  return "system";
}

/** Sets the root's `data-theme`, which the tokens follow; without one they
 * follow the system. */
function show(choice: Appearance) {
  if (choice === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}

class Look {
  choice = $state<Appearance>(kept());

  constructor() {
    show(this.choice);
  }

  choose(next: Appearance) {
    this.choice = next;
    show(next);
    try {
      if (next === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, next);
    } catch {
      // Not kept, but still shown until the window closes.
    }
  }
}

export const appearance = new Look();
