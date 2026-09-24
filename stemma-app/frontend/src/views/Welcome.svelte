<script lang="ts">
  import Mark from "../components/Mark.svelte";
  import { app } from "../lib/app.svelte";
</script>

<div class="welcome">
  <div class="welcome__mark"><Mark /></div>
  <p class="welcome__group">{app.group ?? "No group"}</p>
  {#if app.group}
    <p class="quiet">Choose a line on the left, or start one.</p>
    <button class="primary" onclick={() => app.goTo("new-line")}>Start a line</button>
  {:else}
    <p class="quiet">Sync this machine to bring your groups here.</p>
  {/if}
</div>
