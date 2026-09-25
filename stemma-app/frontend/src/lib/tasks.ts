// What an agent can be asked about a block, as the task strings
// `stemma_core::job::Task` reads.

import { isStatement } from "./blocks.ts";
import type { Block, LineDocument, Node } from "./types";

/** One thing an agent can be asked, and the label a button shows. */
export type Offered = { verb: string; label: string; task: string };

export const LABELS: Record<string, string> = {
  prove: "Write the proof",
  formalize: "Formalize in Lean",
  refute: "Find a counterexample",
  rewrite: "Rewrite rigorously",
  promote: "Make central",
  fill: "Fill the gap after it",
  develop: "Develop into mathematics",
  explore: "Explore next steps",
};

const offer = (verb: string, anchor: string): Offered => ({ verb, label: LABELS[verb], task: `${verb}@${anchor}` });

/** What can be asked about one block. A statement is named by the siglum
 * the line reads it as, so nothing can be asked about it while the line
 * does not read. */
export function tasksFor(document: LineDocument, block: Block, nodeOf: (block: string) => Node | undefined): Offered[] {
  if (block.kind === "prose") return [offer("develop", block.id)];

  const statement = block.kind === "proof" ? document.blocks.find((one) => one.id === block.of) : block;
  if (!statement || !isStatement(statement)) return [];
  const siglum = nodeOf(statement.id)?.siglum;
  if (!siglum) return [];
  if (block.kind === "proof") return [offer("prove", siglum)];

  const theorem = statement.kind === "theorem";
  return [
    ...(statement.proof ? [offer("prove", siglum)] : []),
    offer("formalize", siglum),
    ...(theorem ? [offer("refute", siglum)] : []),
    ...(!statement.central ? [offer("rewrite", siglum)] : []),
    ...(theorem && !statement.central ? [offer("promote", siglum)] : []),
    ...(theorem && statement.central ? [offer("fill", siglum)] : []),
  ];
}

/** The tasks every one of several blocks can be asked, by verb, each with
 * the task string for every block. */
export function common(offered: Offered[][]): { verb: string; label: string; tasks: string[] }[] {
  if (!offered.length) return [];
  const [first, ...rest] = offered;
  return first
    .filter((one) => rest.every((other) => other.some((task) => task.verb === one.verb)))
    .map((one) => ({
      verb: one.verb,
      label: one.label,
      tasks: offered.map((list) => list.find((task) => task.verb === one.verb)!.task),
    }));
}
