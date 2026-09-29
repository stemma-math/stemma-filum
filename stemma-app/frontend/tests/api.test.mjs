import { test } from "node:test";
import assert from "node:assert/strict";
import { api, onHeard, onOpenReview, onViewChosen } from "../src/lib/api.ts";

/** A shell that records every command and event listener asked of it. */
function shell() {
  const calls = [];
  const listeners = new Map();
  globalThis.window = {
    __TAURI__: {
      core: { invoke: async (command, args) => { calls.push({ command, args }); return null; } },
      event: {
        listen: async (name, handler) => {
          listeners.set(name, handler);
          return () => listeners.delete(name);
        },
      },
    },
  };
  return { calls, listeners };
}

test("without the application behind it, a command fails and nothing is heard", async () => {
  globalThis.window = {};
  await assert.rejects(api.whoami(), /no application behind it/);
  const stop = await onHeard(() => {});
  assert.equal(typeof stop, "function");
  stop();
});

test("an agent task is launched on the chosen machine with no custom request", async () => {
  const { calls } = shell();
  await api.launch("/w/a/private/lines/l/line.json", "prove@blk:1", "claude", "mac", null, "high");
  assert.deepEqual(calls, [{
    command: "launch",
    args: {
      path: "/w/a/private/lines/l/line.json",
      task: "prove@blk:1",
      agent: "claude",
      here: false,
      machine: "mac",
      model: null,
      effort: "high",
      request: null,
      grantBlocks: [],
      grantLibrary: false,
    },
  }]);
});

test("a custom task carries its request and what it may change", async () => {
  const { calls } = shell();
  await api.launch("/p", "custom", "claude", "mac", "opus", null, { request: "Tidy it", blocks: ["blk:1"], library: true });
  const { request, grantBlocks, grantLibrary } = calls[0].args;
  assert.deepEqual({ request, grantBlocks, grantLibrary }, { request: "Tidy it", grantBlocks: ["blk:1"], grantLibrary: true });
});

test("events are heard by the names the shell sends them with", async () => {
  const { listeners } = shell();
  const heard = [];
  const stopView = await onViewChosen((view) => heard.push(view));
  const stopReview = await onOpenReview(() => heard.push("review"));

  listeners.get("view")({ payload: "pdf" });
  listeners.get("stemma:open-review")({ payload: "prp:1" });
  assert.deepEqual(heard, ["pdf", "review"]);

  stopView();
  stopReview();
  assert.equal(listeners.size, 0);
});
