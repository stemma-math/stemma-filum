import { test } from "node:test";
import assert from "node:assert/strict";
import { highlightTree } from "@lezer/highlight";
import { typst, typstHighlighter } from "../src/lib/typst.ts";

/** The class each highlighted stretch of `source` is drawn with. */
function classes(source) {
  const found = new Map();
  highlightTree(typst.language.parser.parse(source), typstHighlighter, (from, to, names) => {
    found.set(source.slice(from, to), names);
  });
  return found;
}

test("a line environment reads as a call with named arguments and a body", () => {
  const seen = classes(`#theorem(id: "thm:first", central: true, uses: ("lem:a", "lem:b"))[Every $f: G -> H$ factors.]`);
  assert.equal(seen.get("#"), "tt-hash");
  assert.equal(seen.get("theorem"), "tt-call");
  for (const name of ["id", "central", "uses"]) assert.equal(seen.get(name), "tt-argument");
  for (const value of [`"thm:first"`, "true", `"lem:a"`]) assert.equal(seen.get(value), "tt-value");
  assert.equal(seen.get("$"), "tt-dollar");
  // Neighbouring pieces of an equation are drawn as one stretch.
  assert.ok([...seen].some(([text, names]) => text.includes("G") && names.includes("tt-math")));
  // The prose of the body stays prose.
  assert.equal(seen.get("Every"), undefined);
  assert.equal(seen.get("factors."), undefined);
});

test("every environment of a line highlights the same way", () => {
  for (const kind of ["definition", "theorem", "lemma", "proposition", "corollary", "remark", "proof", "question"]) {
    assert.equal(classes(`#${kind}[Text.]`).get(kind), "tt-call", kind);
  }
});

test("the notation import and comments are told apart", () => {
  const seen = classes(`#import "@stemma/notation:0.1.0": *\n// a note`);
  assert.equal(seen.get("import"), "tt-keyword");
  assert.equal(seen.get("// a note"), "tt-comment");
});
