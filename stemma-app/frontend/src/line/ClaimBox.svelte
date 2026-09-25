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
        <span class="quiet">expires in {Math.max(1, Math.ceil(claim.left / 60))} min</span>
      {/if}
      {#if claim.standing === "open"}
        <button onclick={() => line.takeClaim()}>Claim</button>
      {:else if line.writing}
        <button onclick={() => line.release()}>Release</button>
      {:else}
        <button disabled={claim.asked} onclick={() => line.requestClaim()}>{claim.asked ? "Requested" : "Request"}</button>
      {/if}
    </div>
  {:else}
    No active claim
  {/if}
</div>
