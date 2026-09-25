<script lang="ts">
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { tell } from "../lib/status.svelte";
  import type { Machine } from "../lib/types";

  let machine = $state<Machine | null>(null);
  let problem = $state<string | null>(null);
  let offered = $state<string[]>([...app.groups]);

  async function load() {
    try { machine = await api.machine(); problem = null; } catch (error) { problem = said(error); }
  }

  async function run(work: () => Promise<string>) {
    try { tell(await work()); } catch (error) { tell(said(error)); }
  }

  async function offer(event: SubmitEvent) {
    event.preventDefault();
    await run(() => api.nodeInit(offered));
    await load();
  }

  onMount(load);
</script>

<h2>This machine</h2>
<div class="setting">
  {#if problem}
    {problem}
  {:else if machine}
    <p class="row__name">{machine.label}</p>
    <p class="quiet">{machine.presence}</p>
    {#each machine.agents as agent (agent.name)}
      <div class="agent" class:out={agent.reason}>{agent.called} <code>{agent.reason ?? agent.availability}</code></div>
    {/each}
  {:else}
    Loading...
  {/if}
</div>
<section class="setting">
  <p class="label">Command line</p>
  <p class="quiet">Install the bundled <code>stemma</code> command to manage this workspace from a terminal.</p>
  <div class="row__actions"><button class="button" onclick={() => run(api.installCli)}>Install</button></div>
</section>
<section class="setting">
  <p class="label">Background service</p>
  <p class="quiet">The daemon accepts jobs while this machine is running, without the application open.</p>
  <div class="row__actions"><button class="button" onclick={() => run(api.daemonInstall)}>Install the daemon</button></div>
</section>
<section class="setting">
  <p class="label">Run jobs for groups</p>
  <form class="stack" onsubmit={offer}>
    <div>
      {#each app.groups as group (group)}
        <label class="choice"><input type="checkbox" bind:group={offered} value={group} /><span>{group}</span></label>
      {:else}
        <p class="quiet">No groups are available on this machine.</p>
      {/each}
    </div>
    <div class="row__actions"><button class="button" type="submit">Save</button></div>
  </form>
</section>
<section class="setting">
  <p class="label">Jobs</p>
  {#if machine}
    <JobList jobs={machine.jobs} empty="No jobs have been assigned to this machine." />
  {:else}
    No jobs
  {/if}
</section>
