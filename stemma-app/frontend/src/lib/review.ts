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

/** What a reviewed block's change is called where it is marked. */
export function changeLabel(row: ReviewedBlock): string {
  if (row.outdated) return "Outdated";
  return { added: "Added", removed: "Removed", edited: "Edited", kept: "Unchanged" }[row.change];
}

/** A block, named as a reader finds it: a statement by its environment,
 * siglum and title; a proof by the statement it proves, found with
 * `find`; prose by how it begins. */
export function blockName(block: Block | null, find: (id: string) => Block | undefined = () => undefined): string {
  if (!block) return "Block";
  if (block.kind === "prose") {
    const opening = block.content.replace(/\s+/g, " ").trim();
    if (!opening) return "Prose";
    return `Prose · “${opening.length > 40 ? `${opening.slice(0, 40).trimEnd()}…` : opening}”`;
  }
  if (block.kind === "proof") {
    const proved = find(block.of);
    const named = proved && proved.kind !== "proof" && proved.kind !== "prose" ? proved.siglum ?? proved.environment : null;
    return named ? `Proof of ${named}` : "Proof";
  }
  return [block.environment, block.siglum, block.title].filter(Boolean).join(" · ");
}

/** One run of a word-level difference. */
export type Run = { change: "kept" | "added" | "removed"; text: string };

/** The longest the two texts may be, in words multiplied, before they are
 * shown as one removed and one added run instead of word by word. */
const LONGEST = 250_000;

/** How `before` became `after`, piece by piece, spaces kept, so that
 * the runs read back as either text. */
export function wordDiff(before: string, after: string): Run[] {
  // Words, runs of space and single marks: `$x$` and `[a,b]` are
  // compared piece by piece, as a reader of the source sees them.
  const pieces = (text: string) => text.match(/\s+|[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu) ?? [];
  const was = pieces(before);
  const now = pieces(after);
  if (was.length * now.length > LONGEST) {
    return [
      ...(before ? [{ change: "removed" as const, text: before }] : []),
      ...(after ? [{ change: "added" as const, text: after }] : []),
    ];
  }

  // The longest common subsequence, from the end, so the walk below goes
  // forwards.
  const width = now.length + 1;
  const common = new Uint32Array((was.length + 1) * width);
  for (let i = was.length - 1; i >= 0; i -= 1) {
    for (let j = now.length - 1; j >= 0; j -= 1) {
      common[i * width + j] = was[i] === now[j]
        ? common[(i + 1) * width + j + 1] + 1
        : Math.max(common[(i + 1) * width + j], common[i * width + j + 1]);
    }
  }

  const runs: Run[] = [];
  const push = (change: Run["change"], text: string) => {
    const last = runs[runs.length - 1];
    if (last?.change === change) last.text += text;
    else runs.push({ change, text });
  };
  let i = 0;
  let j = 0;
  while (i < was.length || j < now.length) {
    if (i < was.length && j < now.length && was[i] === now[j]) {
      push("kept", was[i]);
      i += 1;
      j += 1;
    } else if (j < now.length && (i === was.length || common[i * width + j + 1] >= common[(i + 1) * width + j])) {
      push("added", now[j]);
      j += 1;
    } else {
      push("removed", was[i]);
      i += 1;
    }
  }
  return runs;
}
