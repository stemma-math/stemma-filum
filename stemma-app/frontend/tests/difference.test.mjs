import { test } from "node:test";
import assert from "node:assert/strict";
import { difference } from "../src/lib/difference.ts";

const apply = (text, { from, to, insert }) => text.slice(0, from) + insert + text.slice(to);

test("only the part that differs is replaced", () => {
  assert.deepEqual(difference("A group is a set.", "A group is a monoid."), { from: 13, to: 16, insert: "monoid" });
  assert.deepEqual(difference("same", "same"), { from: 4, to: 4, insert: "" });
});

test("the edit turns one text into the other whatever they share", () => {
  for (const [before, after] of [["", "x"], ["x", ""], ["aaa", "aa"], ["abc", "xbcx"], ["ab", "ba"]]) {
    assert.equal(apply(before, difference(before, after)), after, `${before} → ${after}`);
  }
});
