// Application-wide state: who is signed in, which group and view are
// showing, the open line, and moving between them.

import { api, said } from "./api";
import { latestRequest } from "./lifecycle";
import { LineSession } from "./line.svelte";
import { fail, tell } from "./status.svelte";
import type { Known, Signed, Stepped, Waiting } from "./types";

/** What fills the middle of the window. */
export type Stage = "welcome" | "editor" | "new-line" | "vault";

/** Which page of the settings dialog is showing. */
export type Panel = "machine" | "reviews" | "account" | "appearance";

/** How an open line is shown: its blocks to edit, or the typeset page. */
export type View = "blocks" | "pdf";

/** What the last sync did, as the sidebar reports it. */
export type Synced = {
  /** Whether one is running now. */
  running: boolean;
  /** Each repository's outcome, from the last sync that answered. */
  stepped: Stepped[] | null;
  /** Why the last sync failed, when it did. */
  failed: string | null;
};

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
  line = $state<LineSession | null>(null);
  view = $state<View>("blocks");
  synced = $state<Synced>({ running: false, stepped: null, failed: null });

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
      try { this.waiting = await api.proposals(); } catch { this.waiting = []; }
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
    const leaving = this.stage === "editor" ? this.line : null;
    try {
      await leaving?.save();
      const opened = await api.openLine(path);
      const offers = await api.offers(opened.entry.group).catch(() => []);
      if (!active()) return;
      if (leaving) {
        await leaving.flush();
        await leaving.record();
        if (!active()) return;
      }
      this.line = new LineSession(opened, offers);
      this.stage = "editor";
    } catch (error) {
      fail(error);
    }
  }

  /** Syncs every repository, then rereads groups and lines whatever the
   * outcome: a partial sync may still have brought something. The
   * sidebar shows how it went; only a failure, or its end, is a toast. */
  async sync() {
    if (this.synced.running) return;
    const wasFailing = this.synced.failed !== null;
    this.synced.running = true;
    let stepped: Stepped[] = [];
    let failed: unknown = null;
    try { stepped = await api.sync(); } catch (error) { failed = error; }
    await this.#loadGroups();
    await this.#loadLines();
    this.synced.running = false;
    if (failed) {
      this.synced.failed = said(failed);
      fail(failed);
      return;
    }
    this.synced.failed = null;
    this.synced.stepped = stepped;
    if (wasFailing) tell("Synced");
  }

  /** Runs a command that changes which lines exist, then syncs. */
  async changeLines(work: () => Promise<string>) {
    try {
      tell(await work());
      await this.sync();
      await this.goTo("welcome");
    } catch (error) {
      fail(error);
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
    this.waiting = await api.proposals();
  }

  /** Flushes and records the open line before the window closes. Throws
   * when the edits could not be saved, so the close can be refused. */
  async beforeClose() {
    const line = this.stage === "editor" ? this.line : null;
    if (!line) return;
    line.lock(true);
    try {
      await line.flush();
      await line.record();
    } finally {
      line.lock(false);
    }
  }
}

/** How a sync went, most urgent first: a headline, the detail under it,
 * and how much it asks of the reader. */
export function syncReport(stepped: Stepped[]): { tone: "good" | "attention"; headline: string; detail: string } {
  const where = (one: Stepped) => `${one.group}/${one.side}`;
  const parted = stepped.filter((one) => one.parted);
  if (parted.length) {
    return {
      tone: "attention",
      headline: "Conflict",
      detail: `${parted.map(where).join(", ")}: both versions changed. Open the line to resolve the conflict.`,
    };
  }
  const dirty = stepped.filter((one) => one.dirty);
  if (dirty.length) {
    return {
      tone: "attention",
      headline: "Uncommitted changes",
      detail: `${dirty.map(where).join(", ")}: uncommitted changes were not synced. A line is committed when you release it.`,
    };
  }
  const recorded = stepped.flatMap((one) => one.recorded);
  if (recorded.length) {
    return { tone: "good", headline: "Synced", detail: `Committed before syncing: ${recorded.join(", ")}.` };
  }
  const moved = stepped.filter((one) => one.outcome !== "up to date");
  return {
    tone: "good",
    headline: "Synced",
    detail: moved.length ? moved.map((one) => `${one.outcome} · ${where(one)}`).join("; ") : "Everything is up to date.",
  };
}

export const app = new App();
