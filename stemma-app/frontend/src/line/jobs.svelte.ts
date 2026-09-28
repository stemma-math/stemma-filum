import { api, said } from "../lib/api";
import type { Job } from "../lib/types";

/** This machine's jobs, shared by the editor's register and the settings. */
export const jobs = $state({
  list: [] as Job[],
  problem: null as string | null,
  async refresh() {
    try {
      jobs.list = (await api.machine())?.jobs ?? [];
      jobs.problem = null;
    } catch (error) {
      jobs.problem = said(error);
    }
  },
});
