// How jobs are shown: what is under way and what waits for this machine's
// owner stay in view; what has finished is put away.

import type { Entry, Job } from "./types";

export type Sorted = {
  /** Queued or running. */
  active: Job[];
  /** Ended in something the machine's owner has to fix. */
  attention: Job[];
  /** Everything else, newest first as the jobs came. */
  finished: Job[];
};

export function sorted(jobs: Job[]): Sorted {
  const found: Sorted = { active: [], attention: [], finished: [] };
  for (const job of jobs) {
    if (job.state === "queued" || job.state === "running") found.active.push(job);
    else if (job.owed) found.attention.push(job);
    else found.finished.push(job);
  }
  return found;
}

/** The jobs about one line. */
export function ofLine(jobs: Job[], entry: Pick<Entry, "group" | "side" | "name">): Job[] {
  return jobs.filter((job) => job.group === entry.group && job.side === entry.side && job.line === entry.name);
}

/** Whether a job that ended did not get its work done. */
export function failed(job: Job): boolean {
  return job.outcome === "could_not_try" || job.outcome === "broke";
}

/** Where a job stands, in a word or two, and the chip that says it. */
export function standing(job: Job): { words: string; tone: string } {
  switch (job.state) {
    case "queued": return { words: "Queued", tone: "" };
    case "running": return { words: "Running", tone: "chip--accent" };
    case "withdrawn": return { words: "Withdrawn", tone: "chip--dashed" };
    case "done":
      switch (job.outcome) {
        case "nothing_found": return { words: "Nothing found", tone: "" };
        case "could_not_try": return { words: "Could not run", tone: "chip--warning" };
        case "broke": return { words: "Failed", tone: "chip--danger" };
        default: return { words: "Done", tone: "chip--success" };
      }
    default: return { words: job.state, tone: "" };
  }
}
