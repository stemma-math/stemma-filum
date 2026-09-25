import { test } from "node:test";
import assert from "node:assert/strict";
import { History } from "../src/lib/history.ts";

test("states come back in order and go forward again", () => {
  const history = new History("a");
  history.record("b");
  history.record("c");

  assert.equal(history.undo(), "b");
  assert.equal(history.undo(), "a");
  assert.equal(history.undo(), null);
  assert.equal(history.redo(), "b");
  assert.equal(history.redo(), "c");
  assert.equal(history.redo(), null);
});

test("a new change forgets what was undone, and a repeat is not a change", () => {
  const history = new History("a");
  history.record("b");
  history.undo();
  history.record("c");
  history.record("c");

  assert.equal(history.canRedo, false);
  assert.equal(history.undo(), "a");
  assert.equal(history.canUndo, false);
});

test("only so many states are kept", () => {
  const history = new History("0", 2);
  for (const state of ["1", "2", "3"]) history.record(state);

  assert.equal(history.undo(), "2");
  assert.equal(history.undo(), "1");
  assert.equal(history.undo(), null);
});
