<script lang="ts">
  import { onDestroy } from "svelte";
  import Brand from "../components/Brand.svelte";
  import Entrance from "../components/Entrance.svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import type { Beginning } from "../lib/types";

  const attempt = latestRequest();
  let server = $state("");
  let problem = $state<string | null>(null);
  let beginning = $state<Beginning | null>(null);
  let timer = 0;

  function restart(reason: string | null = null) {
    attempt();
    window.clearTimeout(timer);
    beginning = null;
    problem = reason;
  }

  async function begin(event: SubmitEvent) {
    event.preventDefault();
    const address = server.trim();
    if (!address) return;
    const active = attempt();
    try {
      const started = await api.signInStart(address);
      if (!active()) return;
      beginning = started;
      schedule(active, started.interval);
    } catch (error) {
      restart(said(error));
    }
  }

  function schedule(active: () => boolean, interval: number) {
    timer = window.setTimeout(() => poll(active, interval), Math.max(2, interval) * 1000);
  }

  async function poll(active: () => boolean, interval: number) {
    if (!active()) return;
    let answer: string;
    try { answer = await api.signInPoll(); } catch (error) { answer = said(error); }
    if (!active()) return;
    if (answer === "signed") return app.boot();
    if (answer === "waiting" || answer === "nothing") return schedule(active, interval);
    restart(
      answer === "refused" ? "That sign-in was refused."
        : answer === "ran_out" ? "Nobody answered in time."
        : answer,
    );
  }

  onDestroy(() => {
    attempt();
    window.clearTimeout(timer);
  });
</script>

<Entrance>
  <Brand />
  {#if beginning}
    <span class="entrance-kicker">Secure connection</span>
    <h1>Type this code</h1>
    <p class="code">{beginning.code}</p>
    <p class="quiet">at {beginning.url}</p>
    <button onclick={() => api.openLink(beginning!.url_complete).catch(() => {})}>Open it in the browser</button>
    <p class="quiet">Waiting for approval in your browser...</p>
    <button class="quiet-button" onclick={() => restart()}>Use a different instance</button>
  {:else}
    <span class="entrance-kicker">Desktop workspace</span>
    <h1>Connect this machine</h1>
    <p class="quiet">Enter the address of your Stemma instance. You will confirm access in your browser.</p>
    {#if problem}<p class="problem">{problem}</p>{/if}
    <form onsubmit={begin}>
      <!-- svelte-ignore a11y_autofocus -->
      <input type="text" bind:value={server} placeholder="stemma.example.org" aria-label="Instance address" autofocus />
      <button type="submit">Continue</button>
    </form>
  {/if}
</Entrance>
