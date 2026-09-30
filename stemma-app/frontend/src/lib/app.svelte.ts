// Application-wide state: who is signed in, which group and view are
// showing, the open line, and moving between them.

import { api } from "./api";
import { latestRequest } from "./lifecycle";
import { LineSession } from "./line.svelte";
import { lineLabel, sameLine, statesChanged, touchedLines, withLineSummary, type Named } from "./lines";
import { touchedBlocks } from "./review";
import { fail, succeed, warn } from "./status.svelte";
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
  #groupsRequest = latestRequest();
  #linesRequest = latestRequest();
  #waitingRequest = latestRequest();
  #touchedRequest = latestRequest();
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

  updateLineSummary(path: string, document: LineDocument) {
    this.lines = withLineSummary(this.lines, path, document);
  }

  /** The open line's heading, or its directory name when it has none. */
  get lineTitle() {
    const entry = this.line?.opened.entry;
    if (!entry) return "";
    return this.lines.find((line) => line.path === entry.path)?.title ?? entry.name;
  }

  /** The line open in the editor, if the editor is showing. */
  get #openLine(): LineSession | null {
    return this.stage === "editor" ? this.line : null;
  }

  /** Reads who this machine acts for and, when signed in, loads the rail
   * and opens a proposal a link asked for meanwhile. */
  async boot() {
    this.ready = false;
    window.clearInterval(this.#renewal);
    try { this.signed = await api.whoami(); } catch { this.signed = null; }
    if (this.signed) {
      try { await this.refreshWaiting(); } catch { this.waiting = []; this.recent = []; }
      try { this.mirrored = await api.standing(); } catch { this.mirrored = null; }
      await this.#loadGroups();
      await this.#loadLines();
      this.stage = "welcome";
      this.#renewal = window.setInterval(() => {
        if (this.#openLine?.writing) void this.#openLine.takeClaim();
      }, RENEW);
    }
    this.ready = true;
    await this.openPendingReview();
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
    const active = this.#groupsRequest();
    // Offline, nothing is offered: the page it opens is on the server.
    const [groups, managed] = await Promise.all([
      api.groups().catch((): string[] => []),
      api.managedGroups().catch((): string[] => []),
    ]);
    if (!active()) return;
    this.groups = groups;
    this.managed = managed;
    if (this.group && !groups.includes(this.group)) this.group = null;
    this.group ??= groups[0] ?? null;
  }

  async #loadLines() {
    const active = this.#linesRequest();
    try {
      const lines = await api.lines();
      if (active()) this.lines = lines;
    } catch (error) {
      if (!active()) return;
      fail(error);
      this.lines = [];
    }
  }

  /** The path of a line on this machine, if it is here. */
  #pathOf(line: Named): string | undefined {
    return this.lines.find((known) => sameLine(known, line))?.path;
  }

  /** Moves to another view, putting the open line down first. False when
   * the line could not be saved and the move was abandoned. */
  async goTo(next: Stage): Promise<boolean> {
    if (this.stage === "editor" && next !== "editor" && this.line) {
      if (!(await this.line.leave())) return false;
      this.line = null;
    }
    this.#openRequest();
    if (next !== "editor") {
      this.review = null;
      this.proposalBlocks = [];
    }
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
    const leaving = this.#openLine;
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
      const where = { group: offered.group, side: offered.side, name: offered.line };
      let path = this.#pathOf(where);
      if (!path) {
        await this.lookAgain();
        path = this.#pathOf(where);
      }
      if (!path) throw new Error(`The line ${offered.line} is not on this machine yet.`);
      if (this.#openLine?.path !== path) await this.open(path);
      else await this.#openLine.flush();

      const line = this.#openLine;
      if (line?.path !== path) return;
      const review = await api.proposal(proposal, line.opened.source);
      if (this.#openLine === line) this.review = review;
    } catch (error) {
      fail(error);
    }
  }

  /** Opens the proposal a `stemma://` link asked for, if one is waiting
   * and somebody is signed in to review it. */
  async openPendingReview() {
    if (!this.ready || !this.signed) return;
    const proposal = await api.takeOpenReview().catch(() => null);
    if (proposal) await this.openReview(proposal);
  }

  /** Reads the review being shown again, against the open line as it is
   * now. A review closed meanwhile stays closed. */
  async refreshReview() {
    const shown = this.review;
    if (!shown) return;
    const review = await api.proposal(shown.proposal, this.line?.opened.source ?? null);
    if (this.review?.proposal === shown.proposal) this.review = review;
  }

  /** Runs a command that changes which lines exist; it brings the change
   * to this machine before it answers. */
  async changeLines(work: () => Promise<string>) {
    try {
      succeed(await work());
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
    const touched = touchedLines(said);
    if (touched.size || statesChanged(this.lines, said)) await this.lookAgain();

    const open = this.#openLine;
    if (open && touched.has(lineLabel(open.opened.entry))) {
      if (this.lines.some((line) => line.path === open.path)) await open.reread();
      else await this.goTo("welcome");
      if (this.#openLine === open) {
        // A review that cannot be read now stays until the next round.
        await this.refreshReview().catch(() => {});
      }
    }
    for (const aside of said.report.aside) {
      warn(`${aside.line}: a version was kept at ${aside.path}, because ${aside.why}.`);
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

  /** Reads the proposals waiting for this person again, and answers them.
   * When reads overlap, only the latest is shown. */
  async refreshWaiting(): Promise<Waiting[]> {
    const active = this.#waitingRequest();
    const reviews = await api.proposals();
    if (active()) {
      this.waiting = reviews.waiting;
      this.recent = reviews.recent;
      await this.refreshTouched();
    }
    return reviews.waiting;
  }

  /** Marks the standing blocks affected by any waiting offer for this line. */
  async refreshTouched() {
    const active = this.#touchedRequest();
    const line = this.#openLine;
    if (!line?.document) {
      this.proposalBlocks = [];
      return;
    }
    const relevant = this.waiting.filter((one) => sameLine(line.opened.entry, { ...one, name: one.line }));
    const marks = new Set<string>();
    await Promise.all(relevant.map(async (one) => {
      try {
        const review = await api.proposal(one.proposal);
        if (!review.stands) return;
        const base = JSON.parse(review.stands) as LineDocument;
        const offered = JSON.parse(review.source) as LineDocument;
        if (!line.document) return;
        for (const id of touchedBlocks(base, offered, line.document)) marks.add(id);
      } catch { /* A pending review may have been decided meanwhile. */ }
    }));
    if (active() && this.line === line) this.proposalBlocks = [...marks];
  }

  /** Saves the open line and sends what can be sent before the window
   * closes. Throws when the edits could not be saved, so the close can be
   * refused; what cannot be sent now is kept here and leaves later. */
  async beforeClose() {
    const line = this.#openLine;
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
