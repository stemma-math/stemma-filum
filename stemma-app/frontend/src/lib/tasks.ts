// What an agent can be asked about a block, as the task strings
// `stemma_core::job::Task` reads.

import { isStatement } from "./blocks.ts";
import type { Block, LineDocument, Node } from "./types";

/** One thing an agent can be asked, and the label a button shows. */
export type Offered = { verb: string; label: string; task: string };

export const LABELS: Record<string, string> = {
  prove: "Write the proof",
  formalize: "Formalize in Lean",
  back: "Back this box in Lean",
  realign: "Realign the narrative",
  refute: "Find a counterexample",
  rewrite: "Rewrite rigorously",
  develop: "Develop into mathematics",
  explore: "Explore next steps",
};

const offer = (verb: string, anchor: string): Offered => ({ verb, label: LABELS[verb], task: `${verb}@${anchor}` });

/** What can be asked about one block. Stable block identity anchors the
 * request; the node lookup still ensures the complete line reads. */
export function tasksFor(document: LineDocument, block: Block, nodeOf: (block: string) => Node | undefined): Offered[] {
  if (block.kind === "prose") return [offer("develop", block.id)];

  const statement = block.kind === "proof" ? document.blocks.find((one) => one.id === block.of) : block;
  if (!statement || !isStatement(statement)) return [];
  if (!nodeOf(statement.id)) return [];
  const anchor = statement.id;
  if (block.kind === "proof") return [offer("prove", anchor)];

  const theorem = statement.kind === "theorem";
  return [
    ...(statement.proof ? [offer("prove", anchor)] : []),
    ...(statement.central ? [offer("formalize", anchor), offer("realign", anchor)] : []),
    ...(theorem ? [offer("refute", anchor)] : []),
    ...(!statement.central ? [offer("rewrite", anchor)] : []),
    ...(theorem && statement.central ? [offer("back", anchor)] : []),
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
