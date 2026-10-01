import { test } from "node:test";
import assert from "node:assert/strict";
import { blockName, outcome, reviewedBlocks, wordDiff } from "../src/lib/review.ts";

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

test("a word-level difference reads back as either text", () => {
  const before = "A group $G$ is abelian when every pair commutes.";
  const after = "A group $G$ is commutative when every two elements commute.";
  const runs = wordDiff(before, after);
  assert.equal(runs.filter((run) => run.change !== "added").map((run) => run.text).join(""), before);
  assert.equal(runs.filter((run) => run.change !== "removed").map((run) => run.text).join(""), after);
  assert.ok(runs.some((run) => run.change === "removed" && run.text.includes("abelian")));
  assert.equal(runs[0].change, "kept");
});

test("a block is named by its environment and siglum", () => {
  const statement = { kind: "definition", id: "b", environment: "Definition", siglum: "def:heap", title: "Heap", central: true, proof: false, content: "" };
  assert.equal(blockName(statement), "Definition · def:heap · Heap");
  const proof = { kind: "proof", id: "p", of: "b", content: "" };
  assert.equal(blockName(proof, (id) => (id === "b" ? statement : undefined)), "Proof of def:heap");
  assert.equal(blockName(proof), "Proof");
  assert.equal(blockName({ kind: "prose", id: "q", content: "We begin\n with $x$." }), "Prose · “We begin with $x$.”");
});
