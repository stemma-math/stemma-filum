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

  function takeOut() {
    if (!window.confirm("Take this line out of your repository?\n\nEvery version of it stays in the history.")) return;
    const path = line.path;
    void app.changeLines(() => api.deleteLine(path));
  }
</script>

{#if line.mine}
  <p class="quiet">Only you can read this line. Sharing moves it into the group's repository, and it cannot be taken back.</p>
  <div class="row__actions">
    <button onclick={() => (sharing = true)}>Share with the group</button>
    <button class="quiet-button" onclick={takeOut}>Take it out</button>
  </div>
  {#if sharing}
    <form class="sharing stack" onsubmit={share}>
      <p class="quiet">Move this line to the shared group repository.</p>
      <fieldset>
        <legend>What the group sees of what you have written</legend>
        <label class="choice"><input type="radio" bind:group={travels} value="today" /><span>Today's state only</span></label>
        <label class="choice"><input type="radio" bind:group={travels} value="everything" /><span>Everything from the beginning</span></label>
      </fieldset>
      <fieldset>
        <legend>Who can write</legend>
        <label class="choice"><input type="radio" bind:group={writer} value="author" /><span>Only me. Everybody else proposes</span></label>
        <label class="choice"><input type="radio" bind:group={writer} value="anyone" /><span>Anybody in the group, one at a time</span></label>
      </fieldset>
      <div class="row__actions">
        <button type="submit">Move it to the group</button>
        <button type="button" class="quiet-button" onclick={() => (sharing = false)}>Not now</button>
      </div>
    </form>
  {/if}
{:else}
  <p class="quiet">Everybody in {line.opened.entry.group} reads this line. One person writes it at a time.</p>
{/if}
