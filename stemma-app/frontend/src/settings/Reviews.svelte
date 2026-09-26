<script lang="ts">
  import { onMount } from "svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import { fail, tell } from "../lib/status.svelte";
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
      fail(error);
    }
  }

  async function decide(proposal: string, verdict: Verdict) {
    try { tell(await api.decideProposal(proposal, verdict)); } catch (error) { fail(error); return; }
    if (verdict === "accept") {
      try { tell(await api.applyProposal(proposal)); } catch (error) { fail(`Accepted, and not written here: ${said(error)}`); }
    }
    await load();
  }

  async function write(proposal: string) {
    try { tell(await api.applyProposal(proposal)); } catch (error) { fail(error); }
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

<h3 class="settings__title">Reviews</h3>
<p class="setting__note">Agent proposals waiting for review.</p>
<div class="reviews">
  {#if problem}
    <p class="setting__note setting__note--failed">{problem}</p>
  {:else if !loaded}
    <div class="skeleton setting__skeleton"></div>
  {:else}
    {#each app.waiting as one (one.proposal)}
      <div class="row">
        <div>
          <p class="row__name">{one.line}</p>
          <p class="row__meta"><span class="identifier">{one.task}</span> · {one.agent} · {one.vendor} {one.model}</p>
        </div>
        <div class="row__actions"><button type="button" class="button button--small" onclick={() => read(one.proposal)}>Open</button></div>
      </div>
    {:else}
      <p class="empty-box">There are no proposals awaiting your review.</p>
    {/each}
  {/if}
</div>
{#if reading}
  {@const review = reading}
  <section class="review">
    <h4 class="setting__title">{review.line} · <span class="identifier">{review.task}</span></h4>
    <p class="row__meta identifier">{review.vendor} · {review.model} · prompt {review.prompt_revision}</p>
    <h4 class="setting__title">What changed <span class="count">{review.extent}</span></h4>
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
      <h4 class="setting__title">Lean module</h4>
      <pre class="diff">{review.lean_source}</pre>
    {/if}
    <h4 class="setting__title">What was checked</h4>
    <ul class="findings">
      {#each review.findings as finding, at (at)}<li>{finding}</li>{/each}
    </ul>
    {#if review.state === "awaiting_review"}
      <div class="row__actions">
        <button type="button" class="button button--small" onclick={() => decide(review.proposal, "reject")}>Reject</button>
        <button type="button" class="button button--small" onclick={() => decide(review.proposal, "ask_again")}>Ask again</button>
        <button type="button" class="button button--primary button--small" onclick={() => decide(review.proposal, "accept")}>Accept</button>
      </div>
      <p class="setting__note">Accepting records you as the reviewer and preserves the model information.</p>
    {:else}
      <p class="setting__note">This proposal was {review.state}.</p>
      <div class="row__actions"><button type="button" class="button button--small" onclick={() => write(review.proposal)}>Apply</button></div>
    {/if}
  </section>
{/if}
