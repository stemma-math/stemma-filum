import { test } from "node:test";
import assert from "node:assert/strict";
import { insert, proofOf } from "../src/lib/blocks.ts";
import { common, tasksFor } from "../src/lib/tasks.ts";

function line() {
  const document = { format: 1, title: "A line", blocks: [] };
  const prose = insert(document, 0, "prose");
  const definition = insert(document, 1, "definition");
  const lemma = insert(document, 2, "theorem");
  const main = insert(document, 4, "theorem");
  main.central = true;
  const sigla = new Map([[definition.id, "def:group"], [lemma.id, "thm:lemma"], [main.id, "thm:main"]]);
  const nodeOf = (block) => (sigla.has(block) ? { siglum: sigla.get(block) } : undefined);
  return { document, prose, definition, lemma, main, nodeOf };
}

const verbs = (offered) => offered.map((one) => one.verb);

test("each kind of block is offered what makes sense for it", () => {
  const { document, prose, definition, lemma, main, nodeOf } = line();

  assert.deepEqual(tasksFor(document, prose, nodeOf).map((one) => one.task), [`develop@${prose.id}`]);
  assert.deepEqual(verbs(tasksFor(document, definition, nodeOf)), ["formalize"]);
  assert.deepEqual(verbs(tasksFor(document, lemma, nodeOf)), ["prove", "formalize", "refute", "rewrite", "promote"]);
  assert.deepEqual(verbs(tasksFor(document, main, nodeOf)), ["prove", "formalize", "refute", "fill"]);
  assert.deepEqual(tasksFor(document, proofOf(document, main.id), nodeOf).map((one) => one.task), ["prove@thm:main"]);
});

test("a statement is offered nothing while the line does not read", () => {
  const { document, lemma } = line();
  assert.deepEqual(tasksFor(document, lemma, () => undefined), []);
});

test("several blocks are offered what all of them can be asked", () => {
  const { document, lemma, main, prose, nodeOf } = line();
  const both = common([tasksFor(document, lemma, nodeOf), tasksFor(document, main, nodeOf)]);

  assert.deepEqual(verbs(both), ["prove", "formalize", "refute"]);
  assert.deepEqual(both[0].tasks, ["prove@thm:lemma", "prove@thm:main"]);
  assert.deepEqual(common([tasksFor(document, prose, nodeOf), tasksFor(document, main, nodeOf)]), []);
});
