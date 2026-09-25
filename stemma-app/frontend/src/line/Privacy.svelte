<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  let sharing = $state(false);
  let travels = $state<"today" | "everything">("today");
  let writer = $state<"author" | "anyone">("author");

  function share(event: SubmitEvent) {
    event.preventDefault();
    const path = line.path;
    void app.changeLines(() => api.shareLine(path, travels === "everything", writer === "author"));
  }

  function deleteLine() {
    if (!window.confirm("Delete this line?\n\nIts history is kept.")) return;
    const path = line.path;
    void app.changeLines(() => api.deleteLine(path));
  }
</script>

{#if line.mine}
  <p class="quiet">Only you can see this line. Sharing moves it to the group's repository and cannot be undone.</p>
  <div class="row__actions">
    <button class="button" onclick={() => (sharing = true)}>Share with the group</button>
    <button class="button button--quiet" onclick={deleteLine}>Delete</button>
  </div>
  {#if sharing}
    <form class="sharing stack" onsubmit={share}>
      <p class="quiet">Move this line to the shared group repository.</p>
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
      <div class="row__actions">
        <button class="button" type="submit">Share</button>
        <button type="button" class="button button--quiet" onclick={() => (sharing = false)}>Cancel</button>
      </div>
    </form>
  {/if}
{:else}
  <p class="quiet">Everyone in {line.opened.entry.group} can see this line. One person edits it at a time.</p>
{/if}
