// Setting this machine up and removing Stemma from it: the checklist's
// survey, whether anything on it needs attention, and which of the two
// dialogs is open.

import { api, said } from "./api";
import { fail, succeed } from "./status.svelte";
import type { Survey } from "./types";

class Setup {
  survey = $state<Survey | null>(null);
  problem = $state<string | null>(null);
  /** The checklist, as a dialog of its own. */
  open = $state(false);
  /** The removal dialog. */
  removing = $state(false);
  /** Set by the sign-in view, so the checklist opens once signed in. */
  afterSignIn = $state(false);

  #loading: Promise<void> | null = null;

  /** Whether any item needs attention, for the mark on the sidebar. */
  get attention(): boolean {
    const survey = this.survey;
    if (!survey) return false;
    return [survey.service, survey.command_line, survey.groups, survey.lean, ...survey.agents]
      .some((item) => item.state === "attention");
  }

  /** Looks at every item again. Calls made while one look is running
   * share it. */
  load(): Promise<void> {
    this.#loading ??= api.setupSurvey()
      .then((survey) => { this.survey = survey; this.problem = null; })
      .catch((error) => { this.problem = said(error); })
      .finally(() => { this.#loading = null; });
    return this.#loading;
  }

  /** Does one item and looks again. */
  async act(work: () => Promise<string>) {
    try {
      succeed(await work());
    } catch (error) {
      fail(error);
    }
    await this.load();
  }
}

export const setup = new Setup();
