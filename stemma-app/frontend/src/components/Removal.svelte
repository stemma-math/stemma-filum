<script lang="ts">
  // Removing Stemma from this machine, one step at a time: what is not
  // sent is sent or knowingly given up; the service, the command line and
  // the machine on the server go; then the owner decides about the data
  // kept here and about the application itself.
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { setup } from "../lib/setup.svelte";
  import Dialog from "./Dialog.svelte";

  type Stage = "confirm" | "sending" | "unsent" | "leaving" | "data" | "deleting" | "finished";

  let stage = $state<Stage>("confirm");
  let unsent = $state<string[]>([]);
  let done = $state<string[]>([]);
  let data = $state<string[]>([]);
  let problem = $state<string | null>(null);
  let trashable = $state(false);

  /** Whether the machine has already been signed out, after which the
   * window cannot go back to how it was. */
  const left = $derived(stage === "data" || stage === "deleting" || stage === "finished");
  const working = $derived(stage === "sending" || stage === "leaving" || stage === "deleting");

  $effect(() => {
    if (setup.removing) return;
    // A step that has started finishes where it can be seen.
    if (working) {
      setup.removing = true;
      return;
    }
    // Closed after signing out: the window starts again, signed out.
    if (left) void app.boot();
    stage = "confirm";
    problem = null;
  });

  async function send() {
    problem = null;
    // The open line is saved before anything is sent.
    if (!(await app.goTo("welcome"))) return;
    app.panel = null;
    stage = "sending";
    unsent = await api.removalSend().catch(() => []);
    if (unsent.length) stage = "unsent";
    else await leave();
  }

  async function leave() {
    stage = "leaving";
    try {
      done = await api.removalLeave();
    } catch (error) {
      problem = said(error);
      stage = "confirm";
      return;
    }
    data = await api.removalData().catch(() => []);
    trashable = await api.canTrash().catch(() => false);
    stage = data.length ? "data" : "finished";
  }

  async function deleteData() {
    stage = "deleting";
    try {
      await api.removalDeleteData();
      done = [...done, "Everything Stemma kept on this machine is deleted."];
    } catch (error) {
      problem = said(error);
    }
    stage = "finished";
  }

  async function trash() {
    try { await api.trashAndQuit(); } catch (error) { problem = said(error); }
  }
</script>

<Dialog bind:open={setup.removing} title="Remove Stemma from this machine">
  <div class="stack">
    {#if stage === "confirm"}
      <p>Stemma will send what is not on the server yet, stop and remove the background service and the command line, take this machine back on the server and sign out.</p>
      <p class="setting__note">Your lines stay on the server. Afterwards you can choose whether to delete what Stemma keeps on this machine.</p>
    {:else if stage === "sending"}
      <p class="checklist__waiting"><span class="spinner" aria-hidden="true"></span>Sending your changes to the server…</p>
    {:else if stage === "unsent"}
      <p>These lines are not on the server yet. Removing Stemma now loses what was written in them here.</p>
      <ul class="checklist__paths">
        {#each unsent as line (line)}<li class="identifier">{line}</li>{/each}
      </ul>
    {:else if stage === "leaving"}
      <p class="checklist__waiting"><span class="spinner" aria-hidden="true"></span>Removing Stemma…</p>
    {:else}
      <ul class="checklist__done">
        {#each done as sentence (sentence)}<li>{sentence}</li>{/each}
      </ul>
      {#if stage === "data"}
        <p>Delete what Stemma keeps on this machine? The lines are on the server.</p>
        <ul class="checklist__paths">
          {#each data as path (path)}<li class="identifier">{path}</li>{/each}
        </ul>
      {:else if stage === "deleting"}
        <p class="checklist__waiting"><span class="spinner" aria-hidden="true"></span>Deleting…</p>
      {:else}
        <p>Stemma is removed from this machine.{#if trashable} The application itself can go to the Trash.{/if}</p>
      {/if}
    {/if}
    {#if problem}<p class="setting__note setting__note--failed">{problem}</p>{/if}
  </div>
  {#snippet actions()}
    {#if stage === "confirm"}
      <button type="button" class="button" onclick={() => (setup.removing = false)}>Cancel</button>
      <button type="button" class="button button--primary button--danger" onclick={send}>Remove</button>
    {:else if stage === "unsent"}
      <button type="button" class="button" onclick={() => (setup.removing = false)}>Cancel</button>
      <button type="button" class="button" onclick={send}>Try Again</button>
      <button type="button" class="button button--primary button--danger" onclick={leave}>Remove Anyway</button>
    {:else if stage === "data"}
      <button type="button" class="button" onclick={() => (stage = "finished")}>Keep</button>
      <button type="button" class="button button--primary button--danger" onclick={deleteData}>Delete</button>
    {:else if stage === "finished"}
      <button type="button" class="button" onclick={() => api.quit()}>Quit</button>
      {#if trashable}
        <button type="button" class="button button--primary" onclick={trash}>Move to Trash and Quit</button>
      {/if}
    {:else if working}
      <button type="button" class="button" disabled>Cancel</button>
    {/if}
  {/snippet}
</Dialog>
