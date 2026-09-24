<script lang="ts">
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();
</script>

<div class="line-head__claim">
  {#if line.claimError}
    {line.claimError}
  {:else if line.claim}
    {@const claim = line.claim}
    <div class="job">
      <span>{claim.said}</span>
      {#if claim.left !== null}
        <span class="quiet">it lapses in {Math.max(1, Math.ceil(claim.left / 60))} minutes</span>
      {/if}
      {#if claim.standing === "open"}
        <button onclick={() => line.takeClaim()}>Write this line</button>
      {:else if line.writing}
        <button onclick={() => line.letGo()}>Let it go</button>
      {:else}
        <button disabled={claim.asked} onclick={() => line.askForLine()}>{claim.asked ? "Asked for" : "Ask for it"}</button>
      {/if}
    </div>
  {:else}
    No active claim
  {/if}
</div>
