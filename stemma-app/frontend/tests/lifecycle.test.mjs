import { test } from "node:test";
import assert from "node:assert/strict";
import { latestRequest, serially, subscriptions } from "../src/lib/lifecycle.ts";

/** Lets every promise that is already settled run its callbacks. */
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

test("writes stay ordered even when the first request is slow or fails", async () => {
  const enqueue = serially();
  const written = [];
  let release;
  const blocked = new Promise((resolve) => { release = resolve; });
  const first = enqueue(async () => { await blocked; written.push("first"); throw new Error("failed"); });
  const second = enqueue(async () => { written.push("second"); return "saved"; });
  await Promise.resolve();
  assert.deepEqual(written, []);
  release();
  await assert.rejects(first, /failed/);
  assert.equal(await second, "saved");
  assert.deepEqual(written, ["first", "second"]);
});

test("only the latest asynchronous response may update its view", () => {
  const request = latestRequest();
  const first = request();
  assert.equal(first(), true);
  const second = request();
  assert.equal(first(), false);
  assert.equal(second(), true);
});

test("stopping stops every listener, even one that starts afterwards", async () => {
  const stopped = [];
  const listeners = subscriptions();
  let late;
  listeners.add(() => stopped.push("at once"));
  listeners.add(Promise.resolve(() => stopped.push("soon")));
  listeners.add(new Promise((resolve) => { late = resolve; }));
  await settled();

  listeners.stop();
  assert.deepEqual(stopped.sort(), ["at once", "soon"]);

  late(() => stopped.push("late"));
  await settled();
  assert.deepEqual(stopped.sort(), ["at once", "late", "soon"]);

  listeners.stop();
  assert.equal(stopped.length, 3);
});
