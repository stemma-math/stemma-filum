import { test } from "node:test";
import assert from "node:assert/strict";
import { pieces } from "../src/lib/links.ts";

test("words without an address are one plain piece", () => {
  assert.deepEqual(pieces("Nothing to follow."), [{ text: "Nothing to follow." }]);
});

test("an address becomes a link, and the sentence's full stop stays outside", () => {
  assert.deepEqual(pieces("See https://stemma.uv.es/help. Then retry."), [
    { text: "See " },
    { text: "https://stemma.uv.es/help", href: "https://stemma.uv.es/help" },
    { text: ". Then retry." },
  ]);
});

test("an address in brackets keeps its path and leaves the bracket", () => {
  assert.deepEqual(pieces("(at http://localhost:8080/a/b?c=d)"), [
    { text: "(at " },
    { text: "http://localhost:8080/a/b?c=d", href: "http://localhost:8080/a/b?c=d" },
    { text: ")" },
  ]);
});

test("other schemes are left as words", () => {
  assert.deepEqual(pieces("file:///etc/passwd"), [{ text: "file:///etc/passwd" }]);
});
