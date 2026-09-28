// Toasts: one line saying what just happened, at the bottom of the
// window. News goes away by itself; a failure stays until it is closed.

import { remedyOf } from "./api";

export type Toast = {
  id: number;
  message: string;
  error: boolean;
  action?: { label: string; run: () => void };
};

/** How long news stays, in milliseconds. */
const NEWS = 5000;

export const status = $state({ toasts: [] as Toast[] });

let next = 0;

function show(message: string, error: boolean, action?: Toast["action"]) {
  if (!message) return;
  const id = (next += 1);
  // The same message twice in a row is one toast.
  status.toasts = [...status.toasts.filter((one) => one.message !== message), { id, message, error, action }];
  if (!error) window.setTimeout(() => dismiss(id), NEWS);
}

/** Says what just happened. */
export function tell(message: string, action?: Toast["action"]) {
  show(message, false, action);
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
  show(message, true, remedy ? remedies[remedy] : undefined);
}

export function dismiss(id: number) {
  status.toasts = status.toasts.filter((one) => one.id !== id);
}
