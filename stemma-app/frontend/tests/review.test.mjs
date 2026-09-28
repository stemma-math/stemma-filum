import { test } from "node:test";
import assert from "node:assert/strict";
import { reviewedBlocks, touchedBlocks } from "../src/lib/review.ts";

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

test("new blocks mark the standing block after which they arrive", () => {
  const base = document(block("a", "same"), block("b", "old"));
  const offered = document(block("a", "same"), block("x", "new"), block("b", "changed"));
  assert.deepEqual(touchedBlocks(base, offered, base), ["a", "b"]);
});

test("an addition at the beginning marks the next standing block", () => {
  const base = document(block("a", "same"));
  assert.deepEqual(touchedBlocks(base, document(block("x", "new"), block("a", "same")), base), ["a"]);
});
