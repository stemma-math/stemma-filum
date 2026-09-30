import { test } from "node:test";
import assert from "node:assert/strict";
import {
  blockId, cited, dropIndex, insert, move, moveTo, pairs, placeAt, proofOf, references, remove, setProof, siglumPrefix,
} from "../src/lib/blocks.ts";

const line = () => ({ format: 1, title: "A line", blocks: [] });

test("a block identifier is a ULID the core accepts", () => {
  const id = blockId(0x01_8f_00_00_00_00, new Uint8Array(10).fill(255));
  assert.match(id, /^blk:[0-7][0-9A-HJKMNP-TV-Z]{25}$/);
  assert.equal(id.slice(4, 14), "01HW000000");
  assert.equal(id.slice(14), "ZZZZZZZZZZZZZZZZ");
  assert.notEqual(blockId(), blockId());
});

test("a theorem arrives with its proof and a definition without one", () => {
  const document = line();
  const theorem = insert(document, 0, "theorem");
  const definition = insert(document, 0, "definition");

  assert.deepEqual(document.blocks.map((block) => block.kind), ["definition", "theorem", "proof"]);
  assert.equal(proofOf(document, theorem.id).of, theorem.id);
  assert.equal(proofOf(document, definition.id), undefined);
  assert.equal(definition.central, true);
  assert.equal(theorem.central, false);
});

test("turning a proof on adds it after its statement and off deletes it", () => {
  const document = line();
  const definition = insert(document, 0, "definition");
  insert(document, 1, "prose");

  setProof(document, definition.id, true);
  assert.deepEqual(document.blocks.map((block) => block.kind), ["definition", "proof", "prose"]);
  assert.equal(definition.proof, true);

  setProof(document, definition.id, false);
  assert.deepEqual(document.blocks.map((block) => block.kind), ["definition", "prose"]);
  assert.equal(definition.proof, false);
});

test("deleting a statement takes its proof and every use of it", () => {
  const document = line();
  const lemma = insert(document, 0, "theorem");
  const theorem = insert(document, 2, "theorem");
  theorem.uses = [lemma.id];

  remove(document, lemma.id);

  assert.deepEqual(document.blocks.map((block) => block.kind), ["theorem", "proof"]);
  assert.deepEqual(theorem.uses, []);
});

test("deleting a proof says its statement is not proved here", () => {
  const document = line();
  const theorem = insert(document, 0, "theorem");

  remove(document, proofOf(document, theorem.id).id);

  assert.equal(theorem.proof, false);
  assert.equal(document.blocks.length, 1);
});

test("a block moves one place and not past either end", () => {
  const document = line();
  const first = insert(document, 0, "prose");
  const second = insert(document, 1, "prose");

  move(document, first.id, -1);
  assert.deepEqual(document.blocks.map((block) => block.id), [first.id, second.id]);
  move(document, first.id, 1);
  assert.deepEqual(document.blocks.map((block) => block.id), [second.id, first.id]);
});

test("a statement and its proof share a colour wherever the proof is", () => {
  const document = line();
  const first = insert(document, 0, "theorem");
  const second = insert(document, 2, "theorem");
  move(document, proofOf(document, first.id).id, 1);
  move(document, proofOf(document, first.id).id, 1);

  const colours = pairs(document);
  assert.equal(colours.get(first.id), colours.get(proofOf(document, first.id).id));
  assert.notEqual(colours.get(first.id), colours.get(second.id));
});

test("citations are read the way the core reads them", () => {
  assert.deepEqual(
    references("By @def:group and @thm:main. Mail ana@example.org. `@lem:raw` // @lem:comment\n/* @lem:block */ \\@lem:escaped then @lem:last:"),
    ["def:group", "thm:main", "lem:last"],
  );
});

test("what a statement cites is found in its text and in its proof", () => {
  const document = line();
  const group = insert(document, 0, "definition");
  group.siglum = "def:group";
  const theorem = insert(document, 1, "theorem");
  proofOf(document, theorem.id).content = "By @def:group, and @thm:elsewhere.";

  assert.deepEqual(cited(document, theorem.id), [group.id]);
});

test("the siglum prefix follows the environment", () => {
  assert.equal(siglumPrefix({ kind: "definition", environment: "Construction" }), "def");
  assert.equal(siglumPrefix({ kind: "theorem", environment: "Lemma" }), "lem");
  assert.equal(siglumPrefix({ kind: "theorem", environment: "Claim" }), "thm");
});

test("a dragged block lands where it was dropped", () => {
  const document = line();
  const [a, b, c] = [0, 1, 2].map((at) => insert(document, at, "prose"));
  const order = () => document.blocks.map((block) => block.id);

  moveTo(document, a.id, 3);
  assert.deepEqual(order(), [b.id, c.id, a.id]);
  moveTo(document, a.id, 0);
  assert.deepEqual(order(), [a.id, b.id, c.id]);
  moveTo(document, c.id, 1);
  assert.deepEqual(order(), [a.id, c.id, b.id]);
  moveTo(document, c.id, 2);
  assert.deepEqual(order(), [a.id, c.id, b.id], "dropping a block next to itself leaves it there");
});

test("a block is placed at the position it ends up in", () => {
  const document = line();
  document.blocks = ["a", "b", "c", "d"].map((id) => ({ kind: "prose", id, content: "" }));
  const order = () => document.blocks.map((block) => block.id).join("");
  placeAt(document, "a", 2);
  assert.equal(order(), "bcad");
  placeAt(document, "d", 0);
  assert.equal(order(), "dbca");
  placeAt(document, "c", 2);
  assert.equal(order(), "dbca");
  placeAt(document, "b", 3);
  assert.equal(order(), "dcab");
});

test("a dragged block lands past the blocks whose middle it has crossed", () => {
  const heights = [100, 40, 60];
  assert.equal(dropIndex(heights, 10, -30), 0);
  assert.equal(dropIndex(heights, 10, 49), 0);
  assert.equal(dropIndex(heights, 10, 50), 1);
  assert.equal(dropIndex(heights, 10, 129), 1);
  assert.equal(dropIndex(heights, 10, 130), 2);
  assert.equal(dropIndex(heights, 10, 900), 3);
  assert.equal(dropIndex([], 10, 50), 0);
});
