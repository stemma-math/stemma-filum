<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { said } from "../lib/api";
  import { outcome } from "../lib/review";

  let problem = $state<string | null>(null);
  onMount(() => { void app.refreshWaiting().catch((error) => { problem = said(error); }); });
</script>

<main class="reviews-view">
  <header class="reviews-view__head">
    <h1>Reviews <span class="count">{app.waiting.length}</span></h1>
    <p>Proposals waiting for your decision across all groups and lines.</p>
  </header>
  {#if problem}<Message kind="error" text={problem} />{/if}
  <section class="reviews-view__list" aria-label="Waiting proposals">
    <h2>Waiting</h2>
    {#each app.waiting as one (one.proposal)}
      <button type="button" class="reviews-view__item" onclick={() => app.openReview(one.proposal)}>
        <strong>{one.line}</strong><span class="identifier">{one.group} · {one.task}</span>
        <span class="quiet">{one.agent} · {one.vendor} {one.model}</span>
      </button>
    {:else}
      <p class="empty-box">There are no proposals awaiting your review.</p>
    {/each}
  </section>
  <section class="reviews-view__list" aria-label="Recent decisions">
    <h2>Recent decisions</h2>
    {#each app.recent as one (one.proposal)}
      <button type="button" class="reviews-view__item" onclick={() => app.openReview(one.proposal)}>
        <strong>{one.line}</strong><span class="identifier">{one.group} · {outcome(one)}</span>
        {#if one.note}<span class="quiet">{one.note}</span>{/if}
      </button>
    {:else}
      <p class="quiet">No recent decisions.</p>
    {/each}
  </section>
</main>
