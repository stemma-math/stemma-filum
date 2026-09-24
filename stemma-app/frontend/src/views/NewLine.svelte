<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";

  let title = $state("");
  let question = $state("");
  let seen = $state<"me" | "group">("me");

  function start(event: SubmitEvent) {
    event.preventDefault();
    const group = app.group!;
    void app.changeLines(() => api.newLine(group, title, question, seen === "group"));
  }
</script>

{#if app.group}
  <div class="pane pane--narrow">
    <h1>Start a line</h1>
    <p class="quiet">A line is one question and the work of answering it. It begins on this machine.</p>
    <form class="stack" onsubmit={start}>
      <!-- svelte-ignore a11y_autofocus -->
      <label>Title<input type="text" bind:value={title} required autofocus /></label>
      <label>The question<textarea bind:value={question} rows="3" required></textarea>
        <span class="quiet">Describe the research question this line will address.</span></label>
      <fieldset>
        <legend>Who can see it</legend>
        <label class="choice"><input type="radio" bind:group={seen} value="me" /><span>Only me</span></label>
        <label class="choice"><input type="radio" bind:group={seen} value="group" /><span>Everybody in {app.group}</span></label>
      </fieldset>
      <div class="row__actions">
        <button type="submit" class="primary">Start it</button>
        <button type="button" class="quiet-button" onclick={() => app.goTo("welcome")}>Not now</button>
      </div>
    </form>
  </div>
{:else}
  <div class="pane"><p class="quiet">No groups are available on this machine.</p></div>
{/if}
