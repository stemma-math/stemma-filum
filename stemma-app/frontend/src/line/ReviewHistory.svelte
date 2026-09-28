<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import type { LineSession } from "../lib/line.svelte";
  import { outcome } from "../lib/review";
  import type { LineProposal } from "../lib/types";

  let { line }: { line: LineSession } = $props();
  let history = $state<LineProposal[]>([]);
  const request = latestRequest();

  async function refresh(path: string) {
    const active = request();
    try {
      const found = await api.lineHistory(path);
      if (active()) history = found;
    } catch {
      if (active()) history = [];
    }
  }

  // Read again whenever the proposals waiting for review change.
  $effect(() => {
    void app.waiting;
    void app.recent;
    void refresh(line.path);
  });
</script>

<section class="panel__section">
  <h2 class="panel__title">Proposal history</h2>
  {#each history as entry (entry.proposal)}
    <button type="button" class="history__proposal" onclick={() => app.openReview(entry.proposal)}>
      <span>{entry.task} · {outcome(entry)}</span>
      <span class="quiet identifier">{entry.agent} · {entry.vendor} {entry.model} · instructions {entry.instructions_version}</span>
      {#if entry.explanation}<span class="quiet">{entry.explanation}</span>{/if}
      {#if entry.decided_by}<span class="quiet">Decided by {entry.decided_by}{entry.note ? ` · ${entry.note}` : ""}</span>{/if}
    </button>
  {:else}
    <p class="quiet">No proposals for this line yet.</p>
  {/each}
</section>
