// Application-wide state: who is signed in, which group and view are
// showing, the open line, and moving between them.

import { api, auditedBridges, said } from "./api";
import { latestRequest } from "./lifecycle";
import { LineSession } from "./line.svelte";
import { tell } from "./status.svelte";
import type { Known, Signed, Stepped, Waiting } from "./types";

/** What fills the middle of the window. */
export type Stage = "welcome" | "editor" | "new-line" | "vault";

/** Which page of the settings dialog is showing. */
export type Panel = "machine" | "reviews" | "account";

/** How often a held claim is renewed. */
const RENEW = 5 * 60 * 1000;

class App {
  /** False until the first `whoami` has answered. */
  ready = $state(false);
  signed = $state<Signed | null>(null);
  stage = $state<Stage>("welcome");
  panel = $state<Panel | null>(null);
  groups = $state<string[]>([]);
  /** The group the rail shows; one at a time. */
  group = $state<string | null>(null);
  lines = $state<Known[]>([]);
  waiting = $state<Waiting[]>([]);
  line = $state<LineSession | null>(null);

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
      tell(said(error));
      return;
    }
    window.clearInterval(this.#renewal);
    this.panel = null;
    this.signed = null;
    await this.boot();
  }

  async #loadGroups() {
    try { this.groups = await api.groups(); } catch { this.groups = []; }
    if (this.group && !this.groups.includes(this.group)) this.group = null;
    this.group ??= this.groups[0] ?? null;
  }

  async #loadLines() {
    try {
      this.lines = await api.lines();
    } catch (error) {
      tell(said(error));
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
   * one, so it is put down first. */
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
      const [offers, bridges] = await Promise.all([
        api.offers(opened.entry.group).catch(() => []),
        auditedBridges(opened.entry.group),
      ]);
      if (!active()) return;
      if (leaving) {
        await leaving.flush();
        await leaving.record();
        if (!active()) return;
      }
      this.line = new LineSession(opened, offers, bridges);
      this.stage = "editor";
    } catch (error) {
      tell(said(error));
    }
  }

  /** Syncs every repository, then rereads groups and lines whatever the
   * outcome: a partial sync may still have brought something. */
  async sync() {
    tell("Syncing this machine...");
    let stepped: Stepped[] = [];
    let failed: unknown = null;
    try { stepped = await api.sync(); } catch (error) { failed = error; }
    await this.#loadGroups();
    await this.#loadLines();
    if (failed) {
      tell(said(failed));
      return;
    }
    tell(syncReport(stepped));
  }

  /** Runs a command that changes which lines exist, then syncs. */
  async changeLines(work: () => Promise<string>) {
    try {
      tell(await work());
      await this.sync();
      await this.goTo("welcome");
    } catch (error) {
      tell(said(error));
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

/** The one sentence worth showing after a sync, most urgent first. */
function syncReport(stepped: Stepped[]): string {
  const where = (one: Stepped) => `${one.group}/${one.side}`;
  const parted = stepped.filter((one) => one.parted);
  if (parted.length) {
    return `${parted.map(where).join(", ")}: both versions changed. Open the line to resolve the conflict.`;
  }
  const dirty = stepped.filter((one) => one.dirty);
  if (dirty.length) {
    return `${dirty.map(where).join(", ")}: there is work here that is not in the history yet, so it has not gone anywhere. A line you are writing is written when you let it go.`;
  }
  const recorded = stepped.flatMap((one) => one.recorded);
  if (recorded.length) return `Written into history before syncing: ${recorded.join(", ")}.`;
  return stepped.map((one) => `${where(one)}: ${one.outcome}`).join(" · ") || "Everything is up to date.";
}

export const app = new App();
