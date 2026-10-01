// Laying a proposal over the line it would change, and saying where its
// review has got to.

import type { Block, LineDocument } from "./types";

/** Where a proposal's review has got to, in a word: the decision once
 * there is one, unless the line moved on and left it outdated. */
export function outcome(proposal: { state: string; decision: string | null }): string {
  return proposal.state === "outdated" ? "outdated" : proposal.decision ?? proposal.state;
}

export type ReviewedBlock = {
  id: string;
  before: Block | null;
  after: Block | null;
  offered: Block | null;
  change: "kept" | "added" | "removed" | "edited";
  outdated: boolean;
};

/** Place proposal additions beside their neighbours and keep removals where
 * they stood. The server has already done the three-way merge. */
export function reviewedBlocks(current: LineDocument, offered: LineDocument, merged: LineDocument, outdated: string[]): ReviewedBlock[] {
  const before = new Map(current.blocks.map((block) => [block.id, block]));
  const after = new Map(merged.blocks.map((block) => [block.id, block]));
  const proposed = new Map(offered.blocks.map((block) => [block.id, block]));
  const order = current.blocks.map((block) => block.id);
  let previous: string | null = null;
  for (const block of offered.blocks) {
    if (!order.includes(block.id)) {
      const at = previous ? order.indexOf(previous) + 1 : 0;
      order.splice(at, 0, block.id);
    }
    previous = block.id;
  }
  return order.map((id) => {
    const was = before.get(id) ?? null;
    const now = after.get(id) ?? null;
    return {
      id,
      before: was,
      after: now,
      offered: proposed.get(id) ?? null,
      change: !was ? "added" : !now ? "removed" : JSON.stringify(was) === JSON.stringify(now) ? "kept" : "edited",
      outdated: outdated.includes(id),
    };
  });
}
