<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import Checklist from "../components/Checklist.svelte";
  import JobList from "../components/JobList.svelte";
  import { api, said } from "../lib/api";
  import { setup } from "../lib/setup.svelte";
  import type { Machine } from "../lib/types";

  let machine = $state<Machine | null>(null);
  let loaded = $state(false);
  let problem = $state<string | null>(null);

  async function load() {
    try { machine = await api.machine(); problem = null; } catch (error) { problem = said(error); }
    loaded = true;
  }

  onMount(load);
</script>

<h3 class="settings__title">This machine</h3>
<Checklist />
<section class="setting">
  <h4 class="setting__title">
    Jobs
    {#if machine}
      <span class="chip" class:chip--success={machine.presence === "awake"}>{machine.presence === "awake" ? "Online" : "Offline"}</span>
    {/if}
  </h4>
  {#if problem}
    <Message kind="error" text={problem} />
  {:else if machine}
    <JobList jobs={machine.jobs} empty="No jobs have been assigned to this machine." />
  {:else if loaded}
    <p class="setting__note">This machine does not work for any group yet.</p>
  {:else}
    <div class="skeleton setting__skeleton"></div>
  {/if}
</section>
<section class="setting">
  <h4 class="setting__title">Remove Stemma</h4>
  <p class="setting__note">Stops the background service, removes the command line, takes this machine back on the server and signs out.</p>
  <div class="row__actions">
    <button type="button" class="button button--small button--danger" onclick={() => (setup.removing = true)}>Remove Stemma from This Machine</button>
  </div>
</section>
