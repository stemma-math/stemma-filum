// This machine's jobs, shared by the editor's panel and whatever else
// shows them.

import { api, said } from "./api";
import { latestRequest } from "./lifecycle";
import type { Job } from "./types";

class Jobs {
  list = $state<Job[]>([]);
  problem = $state<string | null>(null);

  #request = latestRequest();

  /** Reads the jobs again. When looks overlap, only the latest is shown. */
  async refresh() {
    const active = this.#request();
    try {
      const machine = await api.machine();
      if (!active()) return;
      this.list = machine?.jobs ?? [];
      this.problem = null;
    } catch (error) {
      if (active()) this.problem = said(error);
    }
  }
}

export const jobs = new Jobs();
