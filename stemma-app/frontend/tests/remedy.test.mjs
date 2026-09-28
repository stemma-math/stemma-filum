import { test } from "node:test";
import assert from "node:assert/strict";
import { remedyOf, said } from "../src/lib/api.ts";

test("an error marked with a remedy is read as the remedy and the sentence", () => {
  const error = "remedy:set-up:stemma.example.org does not know this machine";
  assert.deepEqual(remedyOf(error), { remedy: "set-up", message: "stemma.example.org does not know this machine" });
  assert.equal(said(error), "stemma.example.org does not know this machine");
  assert.deepEqual(remedyOf(new Error("cannot reach the server")), { remedy: null, message: "cannot reach the server" });
});
