import { test } from "node:test";
import assert from "node:assert/strict";
import { outcome, reviewedBlocks } from "../src/lib/review.ts";

const block = (id, content) => ({ kind: "prose", id, content });
const document = (...blocks) => ({ format: 1, title: "Line", blocks });

test("review places additions, removals, edits and outdated blocks in the line", () => {
  const current = document(block("a", "old"), block("b", "remove"), block("c", "changed here"));
  const offered = document(block("a", "new"), block("x", "insert"), block("c", "agent change"));
  const merged = document(block("a", "new"), block("x", "insert"), block("c", "changed here"));
  const rows = reviewedBlocks(current, offered, merged, ["c"]);
  assert.deepEqual(rows.map((row) => [row.id, row.change, row.outdated]), [
    ["a", "edited", false], ["x", "added", false], ["b", "removed", false], ["c", "kept", true],
  ]);
  assert.equal(rows[3].offered.content, "agent change");
});

test("a review's outcome is its decision, unless the line left it outdated", () => {
  assert.equal(outcome({ state: "awaiting_review", decision: null }), "awaiting_review");
  assert.equal(outcome({ state: "decided", decision: "accept" }), "accept");
  assert.equal(outcome({ state: "outdated", decision: "accept" }), "outdated");
});
