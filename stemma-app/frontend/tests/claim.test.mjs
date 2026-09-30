import { test } from "node:test";
import assert from "node:assert/strict";
import { takenWhenFree } from "../src/lib/claim.ts";

const claim = (standing, extra = {}) => ({ standing, label: null, left: null, asked: false, said: "", ...extra });

test("a line this machine released stays released when the instance says it is free", () => {
  assert.equal(takenWhenFree(claim("open"), claim("open")), false);
});

test("a line this machine was waiting for is taken once it is free", () => {
  assert.equal(takenWhenFree(claim("waiting", { label: "ana's laptop" }), claim("open")), true);
  assert.equal(takenWhenFree(claim("waiting", { asked: true }), claim("open")), true);
});

test("a claim that lapsed or could not be reached is not taken again by itself", () => {
  assert.equal(takenWhenFree(claim("writing"), claim("open")), false);
  assert.equal(takenWhenFree(null, claim("open")), false);
});

test("a line that is not free is never taken", () => {
  assert.equal(takenWhenFree(claim("waiting"), claim("waiting")), false);
  assert.equal(takenWhenFree(claim("waiting"), claim("writing")), false);
});
