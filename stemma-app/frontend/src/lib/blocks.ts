// Operations on a line's blocks. They change the document they are given,
// so they work the same on a plain object and on a Svelte state proxy.

import type { Block, BlockKind, LineDocument, Proof, Statement } from "./types";

/** The environments offered for a theorem; any other name may be typed. */
export const THEOREM_ENVIRONMENTS = ["Theorem", "Lemma", "Proposition", "Corollary"];

/** The environments offered for a definition; any other name may be typed. */
export const DEFINITION_ENVIRONMENTS = ["Definition", "Construction", "Notation"];

/** The kinds a person adds directly; a proof comes with its statement. */
export const ADDABLE: { kind: Exclude<BlockKind, "proof">; label: string; hint: string }[] = [
  { kind: "prose", label: "Prose", hint: "Intuition, intentions, how one idea leads to the next." },
  { kind: "definition", label: "Definition", hint: "Something given a meaning. Central by default." },
  { kind: "theorem", label: "Theorem", hint: "A result: theorem, lemma, proposition… Comes with its proof." },
];

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** A new block identifier: `blk:` and a ULID, as `stemma_core::id::BlockId`. */
export function blockId(now = Date.now(), random: Uint8Array = crypto.getRandomValues(new Uint8Array(10))): string {
  let time = "";
  let left = now;
  for (let digit = 0; digit < 10; digit += 1) {
    time = CROCKFORD[left % 32] + time;
    left = Math.floor(left / 32);
  }
  const bits = Array.from(random, (byte) => byte.toString(2).padStart(8, "0")).join("");
  let rest = "";
  for (let digit = 0; digit < 16; digit += 1) rest += CROCKFORD[parseInt(bits.slice(digit * 5, digit * 5 + 5), 2)];
  return `blk:${time}${rest}`;
}

/** A new, empty block of a kind a person adds. */
export function newBlock(kind: Exclude<BlockKind, "proof">): Block {
  const id = blockId();
  switch (kind) {
    case "prose":
      return { kind, id, content: "" };
    case "definition":
      return { kind, id, environment: "Definition", central: true, proof: false, content: "" };
    case "theorem":
      return { kind, id, environment: "Theorem", central: false, proof: true, content: "" };
  }
}

export function isStatement(block: Block): block is Statement {
  return block.kind === "definition" || block.kind === "theorem";
}

export function statementOf(document: LineDocument, id: string): Statement | undefined {
  const found = document.blocks.find((block) => block.id === id);
  return found && isStatement(found) ? found : undefined;
}

/** Adds a block at `at`; a theorem arrives with its empty proof after it. */
export function insert(document: LineDocument, at: number, kind: Exclude<BlockKind, "proof">): Block {
  const block = newBlock(kind);
  const added: Block[] = [block];
  if (isStatement(block) && block.proof) added.push(emptyProof(block.id));
  document.blocks.splice(Math.max(0, Math.min(at, document.blocks.length)), 0, ...added);
  return block;
}

function emptyProof(of: string): Proof {
  return { kind: "proof", id: blockId(), of, content: "" };
}

/** Deletes a block. A statement takes its proofs with it; deleting a proof
 * says its statement is not proved here. */
export function remove(document: LineDocument, id: string) {
  const block = document.blocks.find((one) => one.id === id);
  if (!block) return;
  if (block.kind === "proof") {
    setProof(document, block.of, false);
    return;
  }
  document.blocks = document.blocks.filter((one) => one.id !== id && !(one.kind === "proof" && one.of === id));
  for (const other of document.blocks) {
    if (isStatement(other) && other.uses?.includes(id)) other.uses = other.uses.filter((used) => used !== id);
  }
}

/** Whether a statement carries a proof. Turning it on adds an empty proof
 * block after the statement; turning it off deletes its proof blocks. */
export function setProof(document: LineDocument, id: string, yes: boolean) {
  const statement = statementOf(document, id);
  if (!statement) return;
  statement.proof = yes;
  const has = document.blocks.some((block) => block.kind === "proof" && block.of === id);
  if (yes && !has) {
    const at = document.blocks.findIndex((block) => block.id === id);
    document.blocks.splice(at + 1, 0, emptyProof(id));
  } else if (!yes && has) {
    document.blocks = document.blocks.filter((block) => !(block.kind === "proof" && block.of === id));
  }
}

/** The proof of a statement, when it has one. */
export function proofOf(document: LineDocument, id: string): Proof | undefined {
  return document.blocks.find((block): block is Proof => block.kind === "proof" && block.of === id);
}

/** Moves a block one place up (`-1`) or down (`1`). */
export function move(document: LineDocument, id: string, by: -1 | 1) {
  const at = document.blocks.findIndex((block) => block.id === id);
  const to = at + by;
  if (at < 0 || to < 0 || to >= document.blocks.length) return;
  const [block] = document.blocks.splice(at, 1);
  document.blocks.splice(to, 0, block);
}

/** Which colour pairs each statement with its proof: statements carrying a
 * proof are numbered in order, and the proof shares the number. */
export function pairs(document: LineDocument): Map<string, number> {
  const numbered = new Map<string, number>();
  for (const block of document.blocks) {
    if (isStatement(block) && block.proof) numbered.set(block.id, numbered.size);
  }
  for (const block of document.blocks) {
    if (block.kind === "proof" && numbered.has(block.of)) numbered.set(block.id, numbered.get(block.of)!);
  }
  return numbered;
}

/** Every label a piece of Typst markup cites with `@`, as
 * `stemma_core::document::references` reads them. */
export function references(content: string): string[] {
  const found: string[] = [];
  let at = 0;
  while (at < content.length) {
    const here = content[at];
    if (here === "\\") {
      at += 2;
    } else if (here === "`") {
      let fence = "";
      while (content[at] === "`") { fence += "`"; at += 1; }
      const end = content.indexOf(fence, at);
      at = end < 0 ? content.length : end + fence.length;
    } else if (content.startsWith("//", at)) {
      const end = content.indexOf("\n", at);
      at = end < 0 ? content.length : end;
    } else if (content.startsWith("/*", at)) {
      const end = content.indexOf("*/", at + 2);
      at = end < 0 ? content.length : end + 2;
    } else if (here === "@" && (at === 0 || !/[A-Za-z0-9]/.test(content[at - 1]))) {
      let end = at + 1;
      while (end < content.length && /[A-Za-z0-9_:.-]/.test(content[end])) end += 1;
      const label = content.slice(at + 1, end).replace(/[.:]+$/, "");
      if (label) found.push(label);
      at = Math.max(end, at + 1);
    } else {
      at += 1;
    }
  }
  return found;
}

/** The statements a statement cites in its text or its proof, by block. */
export function cited(document: LineDocument, id: string): string[] {
  const statement = statementOf(document, id);
  if (!statement) return [];
  const bySiglum = new Map<string, string>();
  for (const block of document.blocks) {
    if (isStatement(block) && block.siglum) bySiglum.set(block.siglum, block.id);
  }
  const texts = [statement.content, ...document.blocks.filter((block) => block.kind === "proof" && block.of === id).map((block) => block.content)];
  const found: string[] = [];
  for (const label of texts.flatMap(references)) {
    const block = bySiglum.get(label);
    if (block && block !== id && !found.includes(block)) found.push(block);
  }
  return found;
}

/** What a statement is called in a list: its environment, title and siglum. */
export function nameOf(statement: Statement): string {
  const title = statement.title ? ` (${statement.title})` : "";
  const siglum = statement.siglum ? ` · ${statement.siglum}` : "";
  return `${statement.environment}${title}${siglum}`;
}

/** The siglum prefix a statement's kind and environment call for. */
export function siglumPrefix(statement: Statement): string {
  if (statement.kind === "definition") return "def";
  switch (statement.environment.trim().toLowerCase()) {
    case "lemma": return "lem";
    case "proposition": return "prop";
    case "corollary": return "cor";
    default: return "thm";
  }
}
