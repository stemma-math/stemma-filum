// Application-wide state: who is signed in, which group and view are
// showing, the open line, and moving between them.

import { api } from "./api";
import { latestRequest } from "./lifecycle";
import { LineSession } from "./line.svelte";
import { touchedBlocks } from "./review";
import { fail, tell } from "./status.svelte";
import type { Known, LineDocument, LineState, Mirrored, Review, Signed, Waiting } from "./types";

/** What fills the middle of the window. */
export type Stage = "welcome" | "editor" | "new-line" | "vault" | "reviews";

/** Which page of the settings dialog is showing. */
export type Panel = "machine" | "account" | "appearance";

/** How an open line is shown: its blocks to edit, or the typeset page. */
export type View = "blocks" | "pdf";

/** How often a held claim is renewed. */
const RENEW = 5 * 60 * 1000;

class App {
  /** False until the first `whoami` has answered. */
  ready = $state(false);
  signed = $state<Signed | null>(null);
  stage = $state<Stage>("welcome");
  panel = $state<Panel | null>(null);
  groups = $state<string[]>([]);
  /** The groups whose details this person may change on the web. */
  managed = $state<string[]>([]);
  /** The group the rail shows; one at a time. */
  group = $state<string | null>(null);
  lines = $state<Known[]>([]);
  waiting = $state<Waiting[]>([]);
  recent = $state<Waiting[]>([]);
  review = $state<Review | null>(null);
  proposalBlocks = $state<string[]>([]);
  line = $state<LineSession | null>(null);
  view = $state<View>("blocks");
  /** Where synchronisation stands; null until the engine has said. */
  mirrored = $state<Mirrored | null>(null);

  #openRequest = latestRequest();
  #renewal = 0;

  /** Lines of the current group, split by who can see them. */
  get railLines() {
    const here = this.lines.filter((line) => line.group === this.group);
    return {
      mine: here.filter((line) => line.side === "private"),
      ours: here.filter((line) => line.side !== "private"),
    };
  }

  /** Where one line stands, by its path. */
  stateOf(path: string): LineState {
    return this.lines.find((line) => line.path === path)?.state ?? "sent";
  }

  /** The open line's heading, or its directory name when it has none. */
  get lineTitle() {
    const entry = this.line?.opened.entry;
    if (!entry) return "";
    return this.lines.find((line) => line.path === entry.path)?.title ?? entry.name;
  }

  /** Reads who this machine acts for and, when signed in, loads the rail. */
  async boot() {
    this.ready = false;
    try { this.signed = await api.whoami(); } catch { this.signed = null; }
    if (this.signed) {
      try { await this.refreshWaiting(); } catch { this.waiting = []; this.recent = []; }
      try { this.mirrored = await api.standing(); } catch { this.mirrored = null; }
      await this.#loadGroups();
      await this.#loadLines();
      this.stage = "welcome";
      window.clearInterval(this.#renewal);
      this.#renewal = window.setInterval(() => {
        if (this.stage === "editor" && this.line?.writing) void this.line.takeClaim();
      }, RENEW);
    }
    this.ready = true;
  }

  async signOut() {
    if (!(await this.goTo("welcome"))) return;
    try {
      await api.signOut();
    } catch (error) {
      fail(error);
      return;
    }
    window.clearInterval(this.#renewal);
    this.panel = null;
    this.signed = null;
    await this.boot();
  }

  async #loadGroups() {
    try { this.groups = await api.groups(); } catch { this.groups = []; }
    // Offline, nothing is offered: the page it opens is on the server.
    try { this.managed = await api.managedGroups(); } catch { this.managed = []; }
    if (this.group && !this.groups.includes(this.group)) this.group = null;
    this.group ??= this.groups[0] ?? null;
  }

  async #loadLines() {
    try {
      this.lines = await api.lines();
    } catch (error) {
      fail(error);
      this.lines = [];
    }
  }

  /** Moves to another view, putting the open line down first. False when
   * the line could not be saved and the move was abandoned. */
  async goTo(next: Stage): Promise<boolean> {
    if (this.stage === "editor" && next !== "editor" && this.line) {
      if (!(await this.line.leave())) return false;
      this.line = null;
    }
    this.#openRequest();
    if (next !== "editor") this.review = null;
    if (next !== "editor") this.proposalBlocks = [];
    this.stage = next;
    return true;
  }

  /** Switches the rail to another group; the open line belongs to the old
   * one, so it is closed first. */
  async chooseGroup(next: string) {
    if (next === this.group) return;
    if (!(await this.goTo("welcome"))) return;
    this.group = next;
  }

  /** Opens a line in the editor, recording the one being left. */
  async open(path: string) {
    const active = this.#openRequest();
    this.review = null;
    const leaving = this.stage === "editor" ? this.line : null;
    try {
      await leaving?.save();
      const opened = await api.openLine(path);
      const offers = await api.offers(opened.entry.group).catch(() => []);
      if (!active()) return;
      if (leaving) {
        await leaving.flush();
        if (!active()) return;
      }
      this.line = new LineSession(opened, offers);
      this.stage = "editor";
      void this.refreshTouched();
    } catch (error) {
      fail(error);
    }
  }

  /** Opens a proposal in its line, then lays its comparison over the editor. */
  async openReview(proposal: string) {
    try {
      this.panel = null;
      const offered = await api.proposal(proposal);
      let path = this.lines.find((line) => line.group === offered.group && line.side === offered.side && line.name === offered.line)?.path;
      if (!path) {
        await this.lookAgain();
        path = this.lines.find((line) => line.group === offered.group && line.side === offered.side && line.name === offered.line)?.path;
      }
      if (!path) throw new Error(`The line ${offered.line} is not on this machine yet.`);
      if (this.stage !== "editor" || this.line?.path !== path) await this.open(path);
      else await this.line.flush();
      if (this.line?.path === path) this.review = await api.proposal(proposal, this.line.opened.source);
    } catch (error) {
      fail(error);
    }
  }

  /** Runs a command that changes which lines exist; it brings the change
   * to this machine before it answers. */
  async changeLines(work: () => Promise<string>) {
    try {
      tell(await work());
      await this.lookAgain();
      await this.goTo("welcome");
    } catch (error) {
      fail(error);
    }
  }

  /** Reads the groups and lines again, as this machine has them now. */
  async lookAgain() {
    if (!this.signed) return;
    await this.#loadGroups();
    await this.#loadLines();
  }

  /** The engine finished a round: what changed here is read again, and
   * the open line too when it was one of them. */
  async mirror(said: Mirrored) {
    this.mirrored = said;
    const report = said.report;
    const touched = [...report.arrived, ...report.merged, ...report.gone];
    const states = new Map(said.lines.map((line) => [`${line.group}/${line.side}/${line.name}`, line.state]));
    const stale = this.lines.some((line) => (states.get(`${line.group}/${line.side}/${line.name}`) ?? "sent") !== line.state);
    if (touched.length || stale) await this.lookAgain();

    const open = this.stage === "editor" ? this.line : null;
    const entry = open?.opened.entry;
    if (open && entry && touched.includes(`${entry.group}/${entry.side}/${entry.name}`)) {
      if (this.lines.some((line) => line.path === entry.path)) await open.reread();
      else await this.goTo("welcome");
    }
    if (this.review && open && entry && touched.includes(`${entry.group}/${entry.side}/${entry.name}`)) {
      try { this.review = await api.proposal(this.review.proposal, open.opened.source); } catch { /* Keep the last review until the next event. */ }
    }
    for (const aside of report.aside) {
      tell(`${aside.line}: a version was kept at ${aside.path}, because ${aside.why}.`);
    }
  }

  /** Opens a page of the web the signed-in server serves, in the browser:
   * the profile and a group's details are changed there. */
  async openOnWeb(path: string) {
    if (!this.signed) return;
    try {
      // Joined as the server joins its own links, so an instance under a
      // path keeps it.
      await api.openLink(`${this.signed.server.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`);
    } catch (error) {
      fail(error);
    }
  }

  async refreshWaiting() {
    const reviews = await api.proposals();
    this.waiting = reviews.waiting;
    this.recent = reviews.recent;
    await this.refreshTouched();
  }

  /** Marks the standing blocks affected by any waiting offer for this line. */
  async refreshTouched() {
    const line = this.stage === "editor" ? this.line : null;
    const entry = line?.opened.entry;
    if (!line?.document || !entry) { this.proposalBlocks = []; return; }
    const relevant = this.waiting.filter((one) => one.group === entry.group && one.side === entry.side && one.line === entry.name);
    const marks = new Set<string>();
    await Promise.all(relevant.map(async (one) => {
      try {
        const review = await api.proposal(one.proposal);
        if (!review.stands) return;
        const base = JSON.parse(review.stands) as LineDocument;
        const offered = JSON.parse(review.source) as LineDocument;
        for (const id of touchedBlocks(base, offered, line.document!)) marks.add(id);
      } catch { /* A pending review may have been decided meanwhile. */ }
    }));
    if (this.line === line) this.proposalBlocks = [...marks];
  }

  /** Saves the open line and sends what can be sent before the window
   * closes. Throws when the edits could not be saved, so the close can be
   * refused; what cannot be sent now is kept here and leaves later. */
  async beforeClose() {
    const line = this.stage === "editor" ? this.line : null;
    if (line) {
      line.lock(true);
      try {
        await line.flush();
      } finally {
        line.lock(false);
      }
    }
    await api.sendNow().catch(() => {});
  }
}

export const app = new App();
