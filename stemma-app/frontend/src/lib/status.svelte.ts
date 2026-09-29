// Toasts: what just happened, over everything else in the window. Success
// and information go away by themselves once their time is up; a warning
// or a failure stays until it is closed. Every message the application
// raises goes through here, so they all look alike.

import { remedyOf } from "./api";

/** The four kinds of message, as the identity draws them. */
export type Kind = "error" | "warning" | "success" | "info";

export type Toast = {
  id: number;
  kind: Kind;
  message: string;
  /** How long it stays, in milliseconds; null while it is not closed. */
  life: number | null;
  action?: { label: string; run: () => void };
};

/** How long news stays, in milliseconds: long enough to read it twice. The
 * time stops while the pointer is over it. */
const NEWS = 8000;

export const status = $state({ toasts: [] as Toast[] });

let next = 0;

function show(kind: Kind, message: string, action?: Toast["action"]) {
  if (!message) return;
  const id = (next += 1);
  const life = kind === "success" || kind === "info" ? NEWS : null;
  // The same message twice in a row is one toast.
  status.toasts = [...status.toasts.filter((one) => one.message !== message), { id, kind, message, life, action }];
}

/** Says something worth knowing that nobody asked for. */
export function tell(message: string, action?: Toast["action"]) {
  show("info", message, action);
}

/** Says that what was asked was done. */
export function succeed(message: string, action?: Toast["action"]) {
  show("success", message, action);
}

/** Says something to keep in mind; it stays until closed. */
export function warn(message: string, action?: Toast["action"]) {
  show("warning", message, action);
}

/** What each remedy an error can carry does, as a button beside it. The
 * application registers them, since they reach into its state. */
let remedies: Record<string, Toast["action"]> = {};

export function setRemedies(given: Record<string, Toast["action"]>) {
  remedies = given;
}

/** Says what went wrong; it stays until closed. An error with a remedy
 * carries the button that applies it. */
export function fail(error: unknown) {
  const { remedy, message } = remedyOf(error);
  show("error", message, remedy ? remedies[remedy] : undefined);
}

export function dismiss(id: number) {
  status.toasts = status.toasts.filter((one) => one.id !== id);
}
