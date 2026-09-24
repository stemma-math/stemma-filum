// One open line: its source in the editor, saving and recording it, its
// claim, and what is composed from it.

import { draw } from "@stemma-math/identity/descent.js";
import { api, said } from "./api";
import { sourceEditor, type SourceEditor } from "./editor";
import { latestRequest, serially } from "./lifecycle";
import { tell } from "./status.svelte";
import type { Claim, Offer, OpenLine } from "./types";

const SAVE_AFTER = 450;
const PREVIEW_AFTER = 650;

/** The diagnostic `stemma_core::notation::how_to_add_it` produces. */
const NO_NOTATION = "does not say where its environments come from";

export type Pane = "document" | "descent";
export type SaveState = "saved" | "saving" | "not saved" | "written";

export class LineSession {
  opened = $state<OpenLine>() as OpenLine;
  /** Whether this machine holds the claim, and so may edit. */
  writing = $state(false);
  saveState = $state<SaveState>("saved");
  claim = $state<Claim | null>(null);
  claimError = $state<string | null>(null);
  showing = $state<Pane>("document");
  preview = $state<{ svg: string; diagnostics: string[] } | null>(null);
  previewError = $state<string | null>(null);
  /** The task the launcher offers; a node's button anchors it. */
  task = $state("explore");

  readonly offers: Offer[];
  /** Pairs of sigla joined by an audited bridge, for the descent. */
  readonly bridges: string[][];

  #editor: SourceEditor | null = null;
  #figure: HTMLElement | null = null;
  #descent: unknown = null;
  #saveTimer = 0;
  #previewTimer = 0;
  #savesInFlight = 0;
  #enqueueSave = serially();
  #previewRequest = latestRequest();
  #descentRequest = latestRequest();

  constructor(opened: OpenLine, offers: Offer[], bridges: string[][]) {
    this.opened = opened;
    this.offers = offers;
    this.bridges = bridges;
  }

  get path() {
    return this.opened.entry.path;
  }

  get mine() {
    return this.opened.entry.side === "private";
  }

  /** Sigla Lean verified in the last compiled module. */
  get verified(): Set<string> {
    return new Set(this.opened.lean?.compiled ? this.opened.lean.statements : []);
  }

  get missingNotation() {
    return this.opened.diagnostics.some((one) => one.includes(NO_NOTATION));
  }

  /** Source and Lean diagnostics, one per line. */
  get diagnostics() {
    const lean = this.opened.lean;
    return [...this.opened.diagnostics, ...(lean && !lean.compiled ? [lean.diagnostic] : [])].join("\n");
  }

  /** Mounts the source editor in `parent`. */
  attach(parent: HTMLElement) {
    this.#editor?.destroy();
    this.#editor = sourceEditor(parent, this.opened.source, () => this.#changed());
    this.#editor.writable(this.writing);
  }

  /** Tears the editor down; pending timers are cancelled. */
  detach() {
    window.clearTimeout(this.#saveTimer);
    window.clearTimeout(this.#previewTimer);
    this.#editor?.destroy();
    this.#editor = null;
    this.#figure = null;
  }

  /** Locks or unlocks the editor without changing the claim. */
  lock(locked: boolean) {
    this.#editor?.writable(!locked && this.writing);
  }

  #text() {
    return this.#editor?.text() ?? null;
  }

  #attached() {
    return this.#editor?.dom.isConnected ?? false;
  }

  #changed() {
    if (!this.writing) return;
    this.saveState = "saving";
    window.clearTimeout(this.#saveTimer);
    window.clearTimeout(this.#previewTimer);
    this.#saveTimer = window.setTimeout(() => {
      this.save().catch((error) => tell(said(error)));
    }, SAVE_AFTER);
    this.#previewTimer = window.setTimeout(() => {
      void (this.showing === "descent" ? this.drawDescent() : this.render());
    }, PREVIEW_AFTER);
  }

  /** Writes the editor's text to disk if it differs from what is saved. */
  async save() {
    const text = this.#text();
    if (text === null || (text === this.opened.source && this.#savesInFlight === 0)) return;
    const path = this.path;
    window.clearTimeout(this.#saveTimer);
    this.#savesInFlight += 1;
    try {
      const saved = await this.#enqueueSave(() => api.saveLine(path, text));
      if (!this.#attached()) return;
      this.opened = saved;
      if (this.#text() === text) this.saveState = "saved";
    } catch (error) {
      if (this.#attached()) this.saveState = "not saved";
      throw error;
    } finally {
      this.#savesInFlight -= 1;
    }
  }

  /** Saves until the disk matches the editor, including edits made while
   * an earlier save was in flight. */
  async flush() {
    while (this.#attached() && (this.#text() !== this.opened.source || this.#savesInFlight > 0)) {
      await this.save();
    }
  }

  /** Commits the line into its clone's history. Done at the end of a
   * stretch of writing (letting go, leaving, closing), not on every save. */
  async record() {
    try {
      if (await api.recordLine(this.path)) this.saveState = "written";
    } catch (error) {
      tell(`This line is saved on this machine and not in its history yet: ${said(error)}`);
    }
  }

  /** Puts the line down: flushed to disk, then recorded. False when the
   * last edits could not be saved. */
  async leave(): Promise<boolean> {
    try {
      await this.flush();
    } catch (error) {
      tell(said(error));
      return false;
    }
    window.clearTimeout(this.#saveTimer);
    window.clearTimeout(this.#previewTimer);
    await this.record();
    this.writing = false;
    return true;
  }

  takeClaim() {
    return this.#claimWith(() => api.claim(this.path));
  }

  async letGo() {
    await this.save();
    await this.record();
    return this.#claimWith(() => api.letGo(this.path));
  }

  askForLine() {
    return this.#claimWith(() => api.askForLine(this.path));
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
    this.#editor?.writable(this.writing);
  }

  /** Writes the notation import into the file and shows the result. */
  async addNotation() {
    const path = this.path;
    try {
      await this.save();
      const next = await api.addNotation(path);
      if (this.path !== path) return;
      this.opened = next;
      this.#editor?.replace(next.source);
      await this.render();
      tell("The notation import was added to this line.");
    } catch (error) {
      tell(said(error));
    }
  }

  /** Composes the editor's text into the document preview. */
  async render() {
    const active = this.#previewRequest();
    const text = this.#text();
    if (text === null) return;
    try {
      const preview = await api.preview(this.path, text);
      if (!active() || !this.#attached() || this.#text() !== text) return;
      this.preview = preview;
      this.previewError = null;
    } catch (error) {
      if (!active() || !this.#attached()) return;
      this.previewError = said(error);
    }
  }

  /** Where the descent is drawn. */
  setFigure(figure: HTMLElement | null) {
    this.#figure = figure;
  }

  show(pane: Pane) {
    this.showing = pane;
    if (pane === "descent") void this.drawDescent();
  }

  /** Draws the descent of the editor's text. The previous drawing is passed
   * on so that a box split by a promotion animates. */
  async drawDescent() {
    const figure = this.#figure;
    const text = this.#text();
    if (!figure || text === null) return;
    const active = this.#descentRequest();
    try {
      const next = await api.descent(text, [...this.verified], this.bridges);
      if (!active() || !figure.isConnected || this.#text() !== text) return;
      draw(figure, next, this.#descent);
      this.#descent = next;
    } catch (error) {
      tell(said(error));
    }
  }
}
