<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import Checklist from "../components/Checklist.svelte";
  import JobList from "../components/JobList.svelte";
  import { api, said } from "../lib/api";
  import { space } from "../lib/joblist";
  import { setup } from "../lib/setup.svelte";
  import { fail, succeed } from "../lib/status.svelte";
  import type { Machine } from "../lib/types";

  let machine = $state<Machine | null>(null);
  let loaded = $state(false);
  let problem = $state<string | null>(null);
  let name = $state("");
  let renaming = $state(false);

  async function load() {
    try {
      machine = await api.machine();
      name = machine?.label ?? "";
      problem = null;
    } catch (error) { problem = said(error); }
    loaded = true;
  }

  async function keepRunning(event: Event) {
    if (!machine) return;
    const wanted = (event.currentTarget as HTMLInputElement).checked;
    try {
      await api.setWhileClosed(wanted);
      machine.while_closed = wanted;
    } catch (error) {
      (event.currentTarget as HTMLInputElement).checked = machine.while_closed;
      fail(error);
    }
  }

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    if (!machine || !name.trim() || name.trim() === machine.label) return;
    renaming = true;
    try {
      machine.label = await api.renameMachine(name.trim());
      name = machine.label;
      succeed(`This machine is now called ${machine.label}.`);
    } catch (error) {
      fail(error);
    } finally {
      renaming = false;
    }
  }

  onMount(load);
</script>

<h3 class="settings__title">This machine</h3>
{#if machine}
  <section class="setting">
    <h4 class="setting__title">Name</h4>
    <form class="machine-name" onsubmit={rename}>
      <input class="input" bind:value={name} maxlength="80" aria-label="Machine name" required />
      <button type="submit" class="button button--small" disabled={renaming || !name.trim() || name.trim() === machine.label}>Rename</button>
    </form>
    <p class="setting__note">How this machine appears to you and your groups.</p>
  </section>
{/if}
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
    <label class="choice">
      <input type="checkbox" checked={machine.while_closed} onchange={keepRunning} />
      Keep running jobs while the app is closed
    </label>
    <p class="setting__note">
      {#if machine.while_closed}
        The background service keeps this machine online, running jobs and sending your lines while the app is closed.
      {:else}
        This machine goes offline when you quit the app: a job already running finishes first, and the service starts again when the app opens.
      {/if}
    </p>
    <JobList jobs={machine.jobs} empty="No jobs have been assigned to this machine." />
    <p class="setting__note">
      {#if machine.workspaces.count}
        Job workspaces take {space(machine.workspaces.bytes)} ({machine.workspaces.count === 1 ? "1 workspace" : `${machine.workspaces.count} workspaces`}).
      {:else}
        No job workspaces are kept on this machine.
      {/if}
      A workspace is removed once its proposal is decided, or a day after its job ended with nothing to review. Past {space(machine.workspaces.limit)}, the oldest are removed first.
    </p>
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
