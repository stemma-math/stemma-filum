<script lang="ts">
  import { onMount } from "svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import { tell } from "../lib/status.svelte";
  import type { Moved, Review, Verdict } from "../lib/types";

  const readRequest = latestRequest();
  let loaded = $state(false);
  let problem = $state<string | null>(null);
  let reading = $state<Review | null>(null);

  async function load() {
    reading = null;
    try { await app.refreshWaiting(); problem = null; } catch (error) { problem = said(error); }
    loaded = true;
  }

  async function read(proposal: string) {
    const active = readRequest();
    try {
      const next = await api.proposal(proposal);
      if (active()) reading = next;
    } catch (error) {
      tell(said(error));
    }
  }

  async function decide(proposal: string, verdict: Verdict) {
    try { tell(await api.decideProposal(proposal, verdict)); } catch (error) { tell(said(error)); return; }
    if (verdict === "accept") {
      try { tell(await api.applyProposal(proposal)); } catch (error) { tell(`accepted, and not written here: ${said(error)}`); }
    }
    await load();
  }

  async function write(proposal: string) {
    try { tell(await api.applyProposal(proposal)); } catch (error) { tell(said(error)); }
  }

  /** Classes for one diff row; a frozen run is framed as a whole. */
  function rowClass(rows: Moved[], at: number) {
    const row = rows[at];
    let names = `diff__row diff__row--${row.change}`;
    if (row.frozen) {
      names += " diff__row--held";
      if (!rows[at - 1]?.frozen) names += " diff__row--held-first";
      if (!rows[at + 1]?.frozen) names += " diff__row--held-last";
    }
    return names;
  }

  const sign = { added: "+", removed: "−", kept: " " } as const;

  onMount(load);
</script>

<h2>Reviews</h2>
<p class="quiet">Agent proposals waiting for review.</p>
<div>
  {#if problem}
    {problem}
  {:else if !loaded}
    Loading...
  {:else}
    {#each app.waiting as one (one.proposal)}
      <div class="row">
        <div>
          <p class="row__name">{one.line}</p>
          <p class="quiet">{one.task} · {one.agent} · {one.vendor} {one.model}</p>
        </div>
        <div class="row__actions"><button class="button" onclick={() => read(one.proposal)}>Open</button></div>
      </div>
    {:else}
      <p class="quiet">There are no proposals awaiting your review.</p>
    {/each}
  {/if}
</div>
<section>
  {#if reading}
    {@const review = reading}
    <p class="label">{review.line} · {review.task}</p>
    <p class="quiet identifier">{review.vendor} · {review.model} · prompt {review.prompt_revision}</p>
    <p class="label">What changed · {review.extent}</p>
    <div class="diff">
      {#each review.rows as row, at (at)}
        <div class={rowClass(review.rows, at)}>
          <span class="diff__at">{row.before ?? ""}</span>
          <span class="diff__at">{row.after ?? ""}</span>
          <span class="diff__mark">{sign[row.change]}</span>
          <span class="diff__text">{row.text}</span>
        </div>
      {:else}
        <pre class="diff__text">{review.source}</pre>
      {/each}
    </div>
    {#if review.lean_source}
      <p class="label">Lean module</p>
      <pre class="diff">{review.lean_source}</pre>
    {/if}
    <p class="label">What was checked</p>
    <ul class="findings">
      {#each review.findings as finding, at (at)}<li>{finding}</li>{/each}
    </ul>
    {#if review.state === "awaiting_review"}
      <div class="row__actions">
        <button class="button" onclick={() => decide(review.proposal, "reject")}>Reject</button>
        <button class="button" onclick={() => decide(review.proposal, "ask_again")}>Ask again</button>
        <button class="button button--primary" onclick={() => decide(review.proposal, "accept")}>Accept</button>
      </div>
      <p class="quiet">Accepting records you as the reviewer and preserves the model information.</p>
    {:else}
      <p class="quiet">This proposal was {review.state}.</p>
      <button class="button" onclick={() => write(review.proposal)}>Apply</button>
    {/if}
  {/if}
</section>
