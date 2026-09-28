<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import type { LineProposal } from "../lib/types";

  let { line }: { line: LineSession } = $props();
  let history = $state<LineProposal[]>([]);

  async function refresh() {
    try { history = await api.lineHistory(line.path); } catch { history = []; }
  }

  $effect(() => { void app.waiting; void app.recent; void refresh(); });
</script>

<section class="panel__section">
  <h2 class="panel__title">Proposal history</h2>
  {#each history as entry (entry.proposal)}
    <button type="button" class="history__proposal" onclick={() => app.openReview(entry.proposal)}>
      <span>{entry.task} · {entry.state === "outdated" ? "outdated" : entry.decision ?? entry.state}</span>
      <span class="quiet identifier">{entry.agent} · {entry.vendor} {entry.model} · instructions {entry.instructions_version}</span>
      {#if entry.explanation}<span class="quiet">{entry.explanation}</span>{/if}
      {#if entry.decided_by}<span class="quiet">Decided by {entry.decided_by}{entry.note ? ` · ${entry.note}` : ""}</span>{/if}
    </button>
  {:else}
    <p class="quiet">No proposals for this line yet.</p>
  {/each}
</section>
