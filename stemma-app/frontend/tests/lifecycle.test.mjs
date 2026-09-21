import { test } from "node:test";
import assert from "node:assert/strict";
import { escapeHtml, serially, latestRequest } from "../src/lifecycle.ts";

test("untrusted text cannot break out of quoted attributes", () => {
  assert.equal(escapeHtml(`a" onclick='bad' <&>`), "a&quot; onclick=&#39;bad&#39; &lt;&amp;&gt;");
});

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
