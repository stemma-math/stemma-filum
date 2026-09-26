<script lang="ts">
  import Dialog from "../components/Dialog.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  let sharing = $state(false);
  let deleting = $state(false);
  let travels = $state<"today" | "everything">("today");
  let writer = $state<"author" | "anyone">("author");
  let typed = $state("");

  const title = $derived(line.document?.title || line.opened.entry.name);

  function share(event: SubmitEvent) {
    event.preventDefault();
    sharing = false;
    const path = line.path;
    void app.changeLines(() => api.shareLine(path, travels === "everything", writer === "author"));
  }

  function deleteLine(event: SubmitEvent) {
    event.preventDefault();
    if (typed.trim() !== title) return;
    deleting = false;
    const path = line.path;
    void app.changeLines(() => api.deleteLine(path));
  }
</script>

{#if line.mine}
  <p class="panel__text">Only you can see this line. Sharing moves it to the group's repository and can't be undone.</p>
  <div class="privacy__actions">
    <button type="button" class="button privacy__share" onclick={() => (sharing = true)}>Share with the group</button>
    <button type="button" class="button button--danger" onclick={() => { typed = ""; deleting = true; }}>Delete</button>
  </div>

  <Dialog bind:open={sharing} title="Share line">
    <form id="share-line" class="dialog__form" onsubmit={share}>
      <p class="dialog__text">
        Sharing moves “{title}” to {line.opened.entry.group}'s repository, where everyone in the group can see it. This can't be undone.
      </p>
      <fieldset class="fieldset">
        <legend>History to share</legend>
        <label class="choice"><input type="radio" bind:group={travels} value="today" /><span>Today's state only</span></label>
        <label class="choice"><input type="radio" bind:group={travels} value="everything" /><span>Everything from the beginning</span></label>
      </fieldset>
      <fieldset class="fieldset">
        <legend>Who can write</legend>
        <label class="choice"><input type="radio" bind:group={writer} value="author" /><span>Only me; others send proposals</span></label>
        <label class="choice"><input type="radio" bind:group={writer} value="anyone" /><span>Anyone in the group, one at a time</span></label>
      </fieldset>
    </form>
    {#snippet actions()}
      <button type="button" class="button" onclick={() => (sharing = false)}>Cancel</button>
      <button type="submit" form="share-line" class="button button--primary">Share line</button>
    {/snippet}
  </Dialog>

  <Dialog bind:open={deleting} title="Delete line">
    <form id="delete-line" class="dialog__form" onsubmit={deleteLine}>
      <p class="dialog__text">“{title}” is removed from this machine and from your repository. Its history is kept.</p>
      <label class="field">
        <span class="field__label">Type the line's title to confirm</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="input" bind:value={typed} autocomplete="off" autofocus />
      </label>
    </form>
    {#snippet actions()}
      <button type="button" class="button" onclick={() => (deleting = false)}>Cancel</button>
      <button type="submit" form="delete-line" class="button button--primary button--danger" disabled={typed.trim() !== title}>Delete line</button>
    {/snippet}
  </Dialog>
{:else}
  <p class="panel__text">Shared with {line.opened.entry.group}. Everyone in the group can see this line; one person edits it at a time.</p>
{/if}
