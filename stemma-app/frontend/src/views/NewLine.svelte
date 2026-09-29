<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";

  let title = $state("");
  let description = $state("");
  let seen = $state<"me" | "group">("me");
  /** Whether the line is being created, so it is not asked for twice. */
  let creating = $state(false);

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (creating) return;
    const group = app.group!;
    const about = description.trim() || null;
    creating = true;
    try {
      await app.changeLines(() => api.newLine(group, title, about, seen === "group"));
    } finally {
      creating = false;
    }
  }
</script>

{#if app.group}
  <div class="pane" data-tauri-drag-region>
    <header class="pane__head">
      <span class="identifier pane__path">{app.group}</span>
      <h1 class="pane__title">New line</h1>
    </header>
    <form class="card pane__card stack" onsubmit={create}>
      <label class="field">
        <span class="field__label">Title</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="input" type="text" bind:value={title} required autofocus />
      </label>
      <label class="field">
        <span class="field__label">Description</span>
        <textarea class="input" bind:value={description} rows="3"></textarea>
        <span class="field__hint">Optional. What this line is about, in a sentence or two.</span>
      </label>
      <fieldset class="fieldset">
        <legend>Visibility</legend>
        <label class="choice"><input type="radio" bind:group={seen} value="me" /><span>Only me</span></label>
        <label class="choice"><input type="radio" bind:group={seen} value="group" /><span>Everybody in {app.group}</span></label>
      </fieldset>
      <div class="row__actions">
        <button type="button" class="button" onclick={() => app.goTo("welcome")}>Cancel</button>
        <button type="submit" class="button button--primary" disabled={creating}>Create line</button>
      </div>
    </form>
  </div>
{:else}
  <div class="pane"><p class="empty-box">No groups are available on this machine.</p></div>
{/if}
