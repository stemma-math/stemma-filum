<script lang="ts">
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { fail, tell } from "../lib/status.svelte";
  import type { Machine } from "../lib/types";

  let machine = $state<Machine | null>(null);
  let problem = $state<string | null>(null);
  let offered = $state<string[]>([...app.groups]);

  async function load() {
    try { machine = await api.machine(); problem = null; } catch (error) { problem = said(error); }
  }

  async function run(work: () => Promise<string>) {
    try { tell(await work()); } catch (error) { fail(error); }
  }

  async function offer(event: SubmitEvent) {
    event.preventDefault();
    await run(() => api.nodeInit(offered));
    await load();
  }

  onMount(load);
</script>

<h3 class="settings__title">This machine</h3>
<div class="setting">
  {#if problem}
    <p class="setting__note setting__note--failed">{problem}</p>
  {:else if machine}
    <p class="setting__name">{machine.label}</p>
    <span class="chip" class:chip--success={machine.presence === "awake"}>
      {machine.presence === "awake" ? "Online" : "Offline"}
    </span>
    <ul class="agents">
      {#each machine.agents as agent (agent.name)}
        <li class:agents__out={agent.reason}>
          <span>{agent.called}</span>
          <span class="identifier">{agent.reason ?? agent.availability}</span>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="skeleton setting__skeleton"></div>
  {/if}
</div>
<section class="setting">
  <h4 class="setting__title">Command line</h4>
  <p class="setting__note">Install the bundled <code>stemma</code> command to manage this workspace from a terminal.</p>
  <div class="row__actions"><button type="button" class="button button--small" onclick={() => run(api.installCli)}>Install</button></div>
</section>
<section class="setting">
  <h4 class="setting__title">Background service</h4>
  <p class="setting__note">The daemon accepts jobs while this machine is running, without the application open.</p>
  <div class="row__actions"><button type="button" class="button button--small" onclick={() => run(api.daemonInstall)}>Install the daemon</button></div>
</section>
<section class="setting">
  <h4 class="setting__title">Run jobs for groups</h4>
  <form class="stack" onsubmit={offer}>
    <div class="stack stack--tight">
      {#each app.groups as group (group)}
        <label class="choice"><input type="checkbox" bind:group={offered} value={group} /><span class="identifier">{group}</span></label>
      {:else}
        <p class="setting__note">No groups are available on this machine.</p>
      {/each}
    </div>
    <div class="row__actions"><button class="button button--small" type="submit">Save</button></div>
  </form>
</section>
<section class="setting">
  <h4 class="setting__title">Jobs</h4>
  {#if machine}
    <JobList jobs={machine.jobs} empty="No jobs have been assigned to this machine." />
  {:else}
    <div class="skeleton setting__skeleton"></div>
  {/if}
</section>
