<script lang="ts">
  import { onDestroy } from "svelte";
  import Brand from "../components/Brand.svelte";
  import Entrance from "../components/Entrance.svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import { setup } from "../lib/setup.svelte";
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
    if (answer === "signed") {
      setup.afterSignIn = true;
      return app.boot();
    }
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
    <h1 class="entrance__title">Type this code</h1>
    <p class="entrance__text">Open the address below in your browser, sign in, and type the code to let this machine act for you.</p>
    <p class="entrance__code identifier">{beginning.code}</p>
    <p class="entrance__text identifier">{beginning.url}</p>
    <div class="row__actions">
      <button type="button" class="button button--primary" onclick={() => api.openLink(beginning!.url_complete).catch(() => {})}>Open it in the browser</button>
      <button type="button" class="button" onclick={() => restart()}>Use a different server</button>
    </div>
    <p class="entrance__waiting"><span class="spinner" aria-hidden="true"></span>Waiting for approval in your browser…</p>
  {:else}
    <h1 class="entrance__title">Connect this machine</h1>
    <p class="entrance__text">Enter the address of your Stemma server. You will confirm access in your browser.</p>
    <form class="stack" onsubmit={begin}>
      <label class="field">
        <span class="field__label">Server address</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="input" type="text" bind:value={server} placeholder="stemma.example.org" autofocus
          aria-invalid={problem ? "true" : undefined} aria-describedby={problem ? "sign-in-problem" : undefined} />
        {#if problem}<span class="field__error" id="sign-in-problem">{problem}</span>{/if}
      </label>
      <div class="row__actions"><button class="button button--primary" type="submit">Continue</button></div>
    </form>
  {/if}
</Entrance>
