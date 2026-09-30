import { test } from "node:test";
import assert from "node:assert/strict";
import { failed, ofLine, sorted, standing } from "../src/lib/joblist.ts";

const job = (id, state, extra = {}) => ({
  id, state, group: "algebra", side: "shared", line: "groups", outcome: null, owed: false, task: "fill", agent: "claude",
  said: "", raw_output: false, workspace: null, progress: null, cancel_requested: false, ...extra,
});

test("what is under way and what is owed stay in view, the rest is finished", () => {
  const jobs = [
    job("a", "running"),
    job("b", "done", { outcome: "produced" }),
    job("c", "done", { outcome: "could_not_try", owed: true }),
    job("d", "queued"),
    job("e", "withdrawn"),
    job("f", "done", { outcome: "could_not_try" }),
  ];
  const { active, attention, finished } = sorted(jobs);
  assert.deepEqual(active.map((one) => one.id), ["a", "d"]);
  assert.deepEqual(attention.map((one) => one.id), ["c"]);
  assert.deepEqual(finished.map((one) => one.id), ["b", "e", "f"]);
});

test("a line's jobs are those on its group, side and name", () => {
  const jobs = [job("a", "done"), job("b", "done", { side: "private" }), job("c", "done", { line: "rings" }), job("d", "done", { group: "topology" })];
  assert.deepEqual(ofLine(jobs, { group: "algebra", side: "shared", name: "groups" }).map((one) => one.id), ["a"]);
});

test("how a job ended decides its words and whether it failed", () => {
  assert.deepEqual(standing(job("a", "done", { outcome: "broke" })), { words: "Failed", tone: "chip--danger" });
  assert.deepEqual(standing(job("a", "done", { outcome: "produced" })), { words: "Done", tone: "chip--success" });
  assert.equal(standing(job("a", "done", { outcome: "nothing_found" })).words, "Nothing found");
  assert.equal(standing(job("a", "running")).words, "Running");
  assert.ok(failed(job("a", "done", { outcome: "broke" })));
  assert.ok(!failed(job("a", "done", { outcome: "nothing_found" })));
});
