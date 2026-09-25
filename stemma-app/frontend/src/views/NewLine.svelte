<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";

  let title = $state("");
  let description = $state("");
  let seen = $state<"me" | "group">("me");

  function create(event: SubmitEvent) {
    event.preventDefault();
    const group = app.group!;
    const about = description.trim() || null;
    void app.changeLines(() => api.newLine(group, title, about, seen === "group"));
  }
</script>

{#if app.group}
  <div class="pane pane--narrow">
    <h1>New line</h1>
    <form class="stack" onsubmit={create}>
      <!-- svelte-ignore a11y_autofocus -->
      <label>Title<input type="text" bind:value={title} required autofocus /></label>
      <label>Description<textarea bind:value={description} rows="3"></textarea>
        <span class="quiet">Optional. What this line is about, in a sentence or two.</span></label>
      <fieldset>
        <legend>Visibility</legend>
        <label class="choice"><input type="radio" bind:group={seen} value="me" /><span>Only me</span></label>
        <label class="choice"><input type="radio" bind:group={seen} value="group" /><span>Everybody in {app.group}</span></label>
      </fieldset>
      <div class="row__actions">
        <button type="submit" class="primary">Create</button>
        <button type="button" class="quiet-button" onclick={() => app.goTo("welcome")}>Cancel</button>
      </div>
    </form>
  </div>
{:else}
  <div class="pane"><p class="quiet">No groups are available on this machine.</p></div>
{/if}
