<script lang="ts">
  // Who is writing the line, and the one thing to do about it.
  import Glyph from "../components/Glyph.svelte";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  const minutes = (left: number | null) => (left === null ? null : Math.max(1, Math.ceil(left / 60)));
</script>

<div class="claim" data-tip={line.claim?.said ?? null}>
  {#if line.claimFailed}
    <Glyph name="alert" small />
    <span class="claim__words claim__words--failed">Sync failed</span>
    <button type="button" class="button button--small" onclick={() => line.takeClaim()}>Retry</button>
  {:else if line.claim}
    {@const claim = line.claim}
    {#if claim.standing === "open"}
      <Glyph name="unlock" small />
      <span class="claim__words">Nobody is writing</span>
      <button type="button" class="button button--primary button--small" onclick={() => line.takeClaim()}>Claim</button>
    {:else if line.writing}
      <span class="claim__dot" aria-hidden="true"></span>
      <span class="claim__words">
        You're writing{#if minutes(claim.left)}<span class="claim__left"> · {minutes(claim.left)} min left</span>{/if}
      </span>
      <button type="button" class="button button--small" onclick={() => line.release()}>Release</button>
    {:else}
      <Glyph name="lock" small />
      <span class="claim__words">{claim.label ? `${claim.label} is writing` : claim.said}</span>
      <button type="button" class="button button--small" disabled={claim.asked} onclick={() => line.requestClaim()}>
        {claim.asked ? "Requested" : "Request"}
      </button>
    {/if}
  {:else}
    <Glyph name="unlock" small />
    <span class="claim__words">No active claim</span>
  {/if}
</div>
