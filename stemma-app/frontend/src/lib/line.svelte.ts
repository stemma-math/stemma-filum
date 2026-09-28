// One open line: its blocks being edited and saved, its claim, the blocks
// selected for an agent, and what is composed from it. Saving is all the
// window does; the engine sends what is saved.

import { SvelteSet } from "svelte/reactivity";
import { api, said } from "./api";
import { History } from "./history";
import { latestRequest, serially } from "./lifecycle";
import { fail, tell } from "./status.svelte";
import type { BlockCheck, Claim, CustomTask, Diagnostic, LineDocument, Node, Offer, OpenLine, Preview } from "./types";

const SAVE_AFTER = 450;
const PREVIEW_AFTER = 650;
/** How long a pause in typing has to be to close one step of the history. */
const STEP_AFTER = 600;

export type SaveState = "saved" | "saving" | "not saved";

/** One agent that can be asked, on the machine that offers it. */
export type AgentChoice = {
  /** `machine|agent`, as `LineSession.agent` holds it. */
  value: string;
  /** What the agent is called. */
  agent: string;
  /** What its machine is called. */
  machine: string;
  /** Why it cannot be used now, when it cannot. */
  reason: string | null;
  models: string[];
  efforts: string[];
};

export class LineSession {
  opened = $state<OpenLine>() as OpenLine;
  /** The line being edited; null when the file on disk is not a line. */
  document = $state<LineDocument | null>(null);
  /** Whether this machine holds the claim, and so may edit. */
  writing = $state(false);
  saveState = $state<SaveState>("saved");
  claim = $state<Claim | null>(null);
  claimError = $state<string | null>(null);
  preview = $state<Preview | null>(null);
  previewError = $state<string | null>(null);
  /** The blocks an agent will be asked about. */
  readonly selected = new SvelteSet<string>();
  /** The agent that is asked, as `machine|agent`. */
  agent = $state("");
  /** Optional model and effort overrides for the chosen agent. */
  model = $state("");
  effort = $state("");

  offers = $state<Offer[]>([]);

  canUndo = $state(false);
  canRedo = $state(false);

  #locked = $state(false);
  #history: History;
  #stepTimer = 0;
  /** The document as last written to disk, and its blocks in that order,
   * which is what the faults on `opened` count by. */
  #saved = "";
  #savedOrder = $state<string[]>([]);
  #attached = true;
  #saveTimer = 0;
  #previewTimer = 0;
  #savesInFlight = 0;
  #enqueueSave = serially();
  #previewRequest = latestRequest();

  constructor(opened: OpenLine, offers: Offer[]) {
    this.opened = opened;
    this.offers = offers;
    this.document = opened.document ? structuredClone(opened.document) : null;
    this.#history = new History(this.#text() ?? "");
    this.#remember(opened);
    this.choose(this.#usable());
  }

  /** Every agent on every machine that works for this group. */
  get agents(): AgentChoice[] {
    return this.offers.flatMap((machine) =>
      machine.agents.map((agent) => ({
        value: `${machine.machine}|${agent.name}`,
        agent: agent.called,
        machine: machine.label,
        reason: agent.reason,
        models: agent.models,
        efforts: agent.efforts,
      })),
    );
  }

  /** The agent that is asked, when there is one. */
  get chosenAgent(): AgentChoice | undefined {
    return this.agents.find((one) => one.value === this.agent);
  }

  /** The first agent that can be used, or else the first there is. */
  #usable(): string {
    return (this.agents.find((one) => !one.reason) ?? this.agents[0])?.value ?? "";
  }

  /** Asks `agent` from now on, with the first model and effort it declares. */
  choose(agent: string) {
    this.agent = agent;
    const chosen = this.chosenAgent;
    this.model = chosen?.models[0] ?? "";
    this.effort = chosen?.efforts[0] ?? "";
  }

  /** Asks the chosen agent to do each task, one job per task. Everything
   * is saved first, because the job works on what the server has, and
   * what is saved is sent before the job is asked for. */
  async ask(tasks: string[], custom?: CustomTask): Promise<number> {
    const [machine, agent] = this.agent.split("|");
    if (!machine || !agent) {
      tell("Choose an agent first.");
      return 0;
    }
    await this.flush();
    let started = 0;
    for (const task of tasks) {
      try {
        await api.launch(this.path, task, agent, machine, this.model || null, this.effort || null, custom ?? null);
        started += 1;
      } catch (error) {
        fail(error);
        return started;
      }
    }
    tell(started === 1 ? "Job started." : `${started} jobs started.`);
    return started;
  }

  get path() {
    return this.opened.entry.path;
  }

  get mine() {
    return this.opened.entry.side === "private";
  }

  /** Whether the blocks may be changed right now. */
  get editable() {
    return this.writing && !this.#locked;
  }

  /** Stable blocks Lean verified in the last library check. */
  get verified(): Set<string> {
    return new Set(
      this.opened.lean?.compiled
        ? this.opened.lean.blocks.filter((result) => result.state === "verified").map((result) => result.block)
        : [],
    );
  }

  /** Lean's current printed result for one stable block. */
  formalOf(block: string): BlockCheck | undefined {
    return this.opened.lean?.blocks.find((result) => result.block === block);
  }

  /** What the line reads a block as, when it is a statement and the line reads. */
  nodeOf(block: string): Node | undefined {
    return this.opened.line?.nodes.find((node) => node.block === block);
  }

  /** What is wrong with one block, as of the last save. */
  faultsOf(block: string): string[] {
    return this.opened.faults
      .filter((fault) => fault.at !== null && this.#savedOrder[fault.at - 1] === block)
      .map((fault) => fault.message);
  }

  /** What Typst said about one block. */
  diagnosticsOf(block: string): Diagnostic[] {
    return this.preview?.diagnostics.filter((diagnostic) => diagnostic.block === block) ?? [];
  }

  /** What is wrong with the line and is not about any one block. */
  get general(): string[] {
    const lean = this.opened.lean;
    return [
      ...this.opened.faults.filter((fault) => fault.at === null).map((fault) => fault.message),
      ...(this.preview?.diagnostics.filter((diagnostic) => diagnostic.block === null).map((diagnostic) => diagnostic.message) ?? []),
      ...(lean && !lean.compiled ? [lean.diagnostic] : []),
    ];
  }

  /** Stops saving and composing; the window is leaving the line. */
  detach() {
    this.#attached = false;
    window.clearTimeout(this.#saveTimer);
    window.clearTimeout(this.#previewTimer);
    window.clearTimeout(this.#stepTimer);
  }

  /** Locks or unlocks editing without changing the claim. */
  lock(locked: boolean) {
    this.#locked = locked;
  }

  /** Called after any change to the blocks: records it for undo, and saves
   * and composes when the writer pauses. Typing is recorded as one step
   * per pause; anything else is a step of its own. */
  changed(options: { typing?: boolean } = {}) {
    if (!this.writing) return;
    window.clearTimeout(this.#stepTimer);
    if (options.typing) this.#stepTimer = window.setTimeout(() => this.#step(), STEP_AFTER);
    else this.#step();
    this.#schedule();
  }

  /** Puts back the state before the last step. */
  undo() {
    this.#restore(() => this.#history.undo());
  }

  /** Puts back the step undone last. */
  redo() {
    this.#restore(() => this.#history.redo());
  }

  #step() {
    const text = this.#text();
    if (text !== null) this.#history.record(text);
    this.canUndo = this.#history.canUndo;
    this.canRedo = this.#history.canRedo;
  }

  #restore(take: () => string | null) {
    if (!this.editable) return;
    window.clearTimeout(this.#stepTimer);
    this.#step();
    const state = take();
    this.canUndo = this.#history.canUndo;
    this.canRedo = this.#history.canRedo;
    if (state === null) return;
    this.document = JSON.parse(state) as LineDocument;
    this.#schedule();
  }

  #schedule() {
    this.saveState = "saving";
    window.clearTimeout(this.#saveTimer);
    window.clearTimeout(this.#previewTimer);
    this.#saveTimer = window.setTimeout(() => {
      this.save().catch((error) => fail(error));
    }, SAVE_AFTER);
    this.#previewTimer = window.setTimeout(() => void this.render(), PREVIEW_AFTER);
  }

  #text() {
    return this.document ? JSON.stringify(this.document) : null;
  }

  #remember(opened: OpenLine) {
    this.#saved = opened.document ? JSON.stringify(opened.document) : "";
    this.#savedOrder = opened.document?.blocks.map((block) => block.id) ?? [];
  }

  /** Writes the line to disk if it differs from what is saved. */
  async save() {
    const text = this.#text();
    if (text === null) return;
    if (text === this.#saved && this.#savesInFlight === 0) {
      // Edits that came back to what is on disk leave nothing to save.
      window.clearTimeout(this.#saveTimer);
      this.saveState = "saved";
      return;
    }
    const path = this.path;
    const document = $state.snapshot(this.document) as LineDocument;
    window.clearTimeout(this.#saveTimer);
    this.#savesInFlight += 1;
    try {
      const saved = await this.#enqueueSave(() => api.saveLine(path, document));
      if (!this.#attached) return;
      this.opened = saved;
      this.#remember(saved);
      if (this.#text() === text) this.saveState = "saved";
    } catch (error) {
      if (this.#attached) this.saveState = "not saved";
      throw error;
    } finally {
      this.#savesInFlight -= 1;
    }
  }

  /** Saves until the disk matches the editor, including edits made while
   * an earlier save was in flight. */
  async flush() {
    while (this.#attached && this.#text() !== null && (this.#text() !== this.#saved || this.#savesInFlight > 0)) {
      await this.save();
    }
  }

  /** Reads the line again, after it changed here because of what was
   * written elsewhere. Edits not saved yet are kept, and saved over it. */
  async reread() {
    if (!this.#attached || this.#text() !== this.#saved || this.#savesInFlight > 0) return;
    try {
      const opened = await api.openLine(this.path);
      if (!this.#attached || this.#text() !== this.#saved) return;
      this.opened = opened;
      this.document = opened.document ? structuredClone(opened.document) : null;
      this.#remember(opened);
      this.#history = new History(this.#text() ?? "");
      this.canUndo = false;
      this.canRedo = false;
      void this.render();
    } catch {
      // What is shown stays until the next look works.
    }
  }

  /** Puts the line down: everything saved. False when the last edits
   * could not be saved. */
  async leave(): Promise<boolean> {
    try {
      await this.flush();
    } catch (error) {
      fail(error);
      return false;
    }
    this.detach();
    this.writing = false;
    return true;
  }

  takeClaim() {
    return this.#claimWith(() => api.claim(this.path));
  }

  async release() {
    await this.flush();
    return this.#claimWith(() => api.release(this.path));
  }

  requestClaim() {
    return this.#claimWith(() => api.requestClaim(this.path));
  }

  /** Looks at who is writing the line again, taking nothing: the instance
   * said the claim changed. A line this machine was waiting for and is
   * now free is taken, as opening it would have. */
  async lookAtClaim() {
    try {
      const claim = await api.claimStatus(this.path);
      if (claim.standing === "open" && !this.writing) {
        await this.takeClaim();
        return;
      }
      this.claim = claim;
      this.claimError = null;
      this.writing = claim.standing === "writing";
    } catch {
      // What was shown stays until the next look works.
    }
  }

  /** The machines working for the group changed what they offer. */
  offersChanged(offers: Offer[]) {
    this.offers = offers;
    if (this.chosenAgent?.reason !== null) this.choose(this.#usable());
  }

  async #claimWith(ask: () => Promise<Claim>) {
    try {
      const claim = await ask();
      this.claim = claim;
      this.claimError = null;
      this.writing = claim.standing === "writing";
    } catch (error) {
      this.claim = null;
      this.claimError = said(error);
      this.writing = false;
    }
  }

  /** Composes the line into the document preview. */
  async render() {
    const active = this.#previewRequest();
    const text = this.#text();
    if (text === null || !this.document) return;
    const document = $state.snapshot(this.document) as LineDocument;
    try {
      const preview = await api.preview(this.path, document);
      if (!active() || !this.#attached || this.#text() !== text) return;
      // A document that does not compile keeps the last pages that did.
      this.preview = { svg: preview.svg ?? this.preview?.svg ?? null, diagnostics: preview.diagnostics };
      this.previewError = null;
    } catch (error) {
      if (!active() || !this.#attached) return;
      this.previewError = said(error);
    }
  }
}
