// Undo and redo over whole states of a line, kept as the JSON they save as.

export class History {
  #past: string[] = [];
  #future: string[] = [];
  #current: string;
  readonly #limit: number;

  constructor(initial: string, limit = 200) {
    this.#current = initial;
    this.#limit = limit;
  }

  get canUndo() {
    return this.#past.length > 0;
  }

  get canRedo() {
    return this.#future.length > 0;
  }

  /** Records a new state. Anything that had been undone is forgotten. */
  record(next: string) {
    if (next === this.#current) return;
    this.#past.push(this.#current);
    if (this.#past.length > this.#limit) this.#past.shift();
    this.#current = next;
    this.#future = [];
  }

  /** The state before the current one, which becomes current. */
  undo(): string | null {
    const previous = this.#past.pop();
    if (previous === undefined) return null;
    this.#future.push(this.#current);
    this.#current = previous;
    return previous;
  }

  /** The state undone last, which becomes current again. */
  redo(): string | null {
    const next = this.#future.pop();
    if (next === undefined) return null;
    this.#past.push(this.#current);
    this.#current = next;
    return next;
  }
}
