// When an open line takes its claim without being asked.

import type { Claim } from "./types";

/** Whether a line that is now free is taken, as opening it would have
 * been: only when this machine was waiting for someone else to let it
 * go. A line this machine released stays released. */
export function takenWhenFree(before: Claim | null, now: Claim): boolean {
  return now.standing === "open" && before?.standing === "waiting";
}
