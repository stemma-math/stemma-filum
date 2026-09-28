import { test } from "node:test";
import assert from "node:assert/strict";
import { lineLabel, sameLine, statesChanged, touchedLines } from "../src/lib/lines.ts";

const known = (name, state = "sent") => ({ group: "algebra", side: "private", name, state });

const round = (lines, report = {}) => ({
  connected: true,
  reason: null,
  lines: lines.map((line, index) => ({ id: `lin:${index}`, trouble: null, ...line })),
  report: { arrived: [], sent: [], merged: [], deciding: [], gone: [], aside: [], waiting: [], stopped: null, ...report },
  published: [],
});

test("a line is named as the engine names it in its reports", () => {
  assert.equal(lineLabel(known("quotients")), "algebra/private/quotients");
  assert.ok(sameLine(known("quotients"), { group: "algebra", side: "private", name: "quotients" }));
  assert.ok(!sameLine(known("quotients"), { group: "algebra", side: "shared", name: "quotients" }));
});

test("the rail is stale when a line starts or stops needing something", () => {
  const shown = [known("quotients"), known("groups", "not_sent")];

  assert.ok(!statesChanged(shown, round([known("groups", "not_sent")])));
  assert.ok(statesChanged(shown, round([])), "a line that was sent since");
  assert.ok(statesChanged(shown, round([known("groups", "not_sent"), known("quotients", "needs_decision")])));
});

test("the lines a round touched are what arrived, merged or went", () => {
  const touched = touchedLines(round([], {
    arrived: ["algebra/private/a"],
    merged: ["algebra/shared/b"],
    gone: ["algebra/private/c"],
    sent: ["algebra/private/d"],
  }));

  assert.deepEqual([...touched].sort(), ["algebra/private/a", "algebra/private/c", "algebra/shared/b"]);
});
