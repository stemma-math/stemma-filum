<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import { blockName, changeLabel, reviewedBlocks, wordDiff, type ReviewedBlock } from "../lib/review";
  import { reviewPage } from "../lib/reviewpage";
  import { fail, succeed } from "../lib/status.svelte";
  import { LABELS } from "../lib/tasks";
  import type { LineDocument, Review as ReviewData, Verdict } from "../lib/types";

  let { line, review }: { line: LineSession; review: ReviewData } = $props();
  let view = $state<"typeset" | "source">("typeset");
  let beforePage = $state<string | null>(null);
  let afterPage = $state<string | null>(null);
  let rendering = $state(false);
  let acting = $state(false);
  let note = $state("");
  let handoffTo = $state("");
  const mine = $derived(review.reviewer === app.signed?.user);

  function parse(source: string | null): LineDocument | null {
    if (!source) return null;
    try { return JSON.parse(source) as LineDocument; } catch { return null; }
  }

  const current = $derived(parse(review.current) ?? line.document);
  const offered = $derived(parse(review.source));
  const rows = $derived(current && offered && review.merged
    ? reviewedBlocks(current, offered, review.merged, review.outdated) : []);
  const changed = $derived(rows.filter((row) => row.change !== "kept" || row.outdated));
  const counts = $derived(
    (["edited", "added", "removed", "outdated"] as const)
      .map((kind) => ({
        kind,
        count: rows.filter((row) => (kind === "outdated" ? row.outdated : !row.outdated && row.change === kind)).length,
      }))
      .filter((one) => one.count > 0),
  );
  /** Any block of the line before or after the proposal, by identifier. */
  const find = (id: string) =>
    review.merged?.blocks.find((block) => block.id === id) ?? current?.blocks.find((block) => block.id === id);
  const named = (row: ReviewedBlock) => blockName(row.after ?? row.before, find);
  const asked = $derived.by(() => {
    const [verb, about] = review.task.split("@");
    const anchored = about ? find(about) : undefined;
    return { words: LABELS[verb] ?? verb, about: anchored ? blockName(anchored, find) : about ?? null };
  });
  const page = $derived(beforePage && afterPage
    ? reviewPage(beforePage, afterPage, rows, document.documentElement.dataset.theme) : null);

  /** The rows of the source view: each change, and the unchanged blocks
   * between changes counted rather than shown. */
  const sourceRows = $derived.by(() => {
    const shown: ({ row: ReviewedBlock } | { unchanged: number })[] = [];
    let unchanged = 0;
    for (const row of rows) {
      if (row.change === "kept" && !row.outdated) { unchanged += 1; continue; }
      if (unchanged) shown.push({ unchanged });
      unchanged = 0;
      shown.push({ row });
    }
    if (unchanged) shown.push({ unchanged });
    return shown;
  });

  onMount(() => {
    let alive = true;
    void (async () => {
      if (!current || !review.merged) return;
      rendering = true;
      try {
        const [before, after] = await Promise.all([
          api.reviewPage(line.path, current),
          api.reviewPage(line.path, review.merged),
        ]);
        if (alive) { beforePage = before; afterPage = after; }
      } catch (error) {
        if (alive) fail(error);
      } finally {
        if (alive) rendering = false;
      }
    })();
    return () => { alive = false; };
  });

  async function decide(verdict: Verdict) {
    acting = true;
    try {
      await line.flush();
      if (verdict === "ask_again") await api.sendNow();
      succeed(await api.decideProposal(review.proposal, verdict, note.trim() || null));
      if (verdict === "accept") {
        await line.reread();
        await app.lookAgain();
      }
      await app.refreshWaiting();
      await app.refreshReview();
    } catch (error) {
      fail(error);
    } finally {
      acting = false;
    }
  }

  async function handoff() {
    if (!handoffTo) return;
    acting = true;
    try {
      await api.handoffProposal(review.proposal, handoffTo);
      await app.refreshWaiting();
      await app.refreshReview();
      succeed("Review handed to the group member.");
    } catch (error) { fail(error); }
    finally { acting = false; }
  }
</script>

{#snippet diff(before: string, after: string)}
  <pre class="review-diff">{#each wordDiff(before, after) as run, index (index)}{#if run.change === "added"}<ins>{run.text}</ins>{:else if run.change === "removed"}<del>{run.text}</del>{:else}{run.text}{/if}{/each}</pre>
{/snippet}

<div class="review-line">
  <header class="review-line__head">
    <button type="button" class="button button--small" onclick={() => (app.review = null)}>Back to line</button>
    <div class="review-line__title">
      <h2>{asked.words}{#if asked.about}<span class="review-line__about">{asked.about}</span>{/if}</h2>
      <p class="review-line__summary">
        {#each counts as one (one.kind)}
          <span class="review-count review-count--{one.kind}">{one.count} {one.kind}</span>
        {:else}
          <span class="quiet">No block changes</span>
        {/each}
      </p>
    </div>
    <div class="segmented" role="group" aria-label="Proposal view">
      <button type="button" aria-pressed={view === "typeset"} onclick={() => (view = "typeset")}>Typeset</button>
      <button type="button" aria-pressed={view === "source"} onclick={() => (view = "source")}>Source</button>
    </div>
  </header>

  {#if review.outdated.length}
    <Message kind="warning" text="This proposal is outdated in {review.outdated.length} block{review.outdated.length === 1 ? '' : 's'}. Ask again on the current line." />
  {/if}

  {#if view === "typeset"}
    {#if page}
      <iframe class="review-line__sheet" sandbox="" title="The line with the proposal, its changes marked" srcdoc={page}></iframe>
    {:else if rendering}
      <div class="review-line__waiting"><span class="spinner" aria-hidden="true"></span>Typesetting…</div>
    {:else}
      <p class="quiet">No typeset version is available. The source view shows every change.</p>
    {/if}
  {:else}
    <div class="review-line__blocks">
      {#each sourceRows as shown, index (index)}
        {#if "unchanged" in shown}
          <p class="review-line__unchanged">{shown.unchanged} unchanged block{shown.unchanged === 1 ? "" : "s"}</p>
        {:else}
          {@const row = shown.row}
          <article class="review-block review-block--{row.outdated ? 'outdated' : row.change}">
            <header class="review-block__head">
              <span class="review-block__tag">{changeLabel(row)}</span>
              <span class="review-block__name">{named(row)}</span>
            </header>
            {#if row.change === "added" && row.after}
              {@render diff("", row.after.content)}
            {:else if row.change === "removed" && row.before}
              {@render diff(row.before.content, "")}
            {:else if row.change === "edited" && row.before && row.after}
              {@render diff(row.before.content, row.after.content)}
            {/if}
            {#if row.outdated && row.offered && row.before}
              <p class="review-block__note">The agent proposed this, and the block changed since:</p>
              {@render diff(row.before.content, row.offered.content)}
            {/if}
          </article>
        {/if}
      {:else}
        <pre class="source">{review.source}</pre>
      {/each}
    </div>
  {/if}
</div>

<aside class="panel review-panel" aria-label="Proposal explanation and decision">
  <section class="panel__section">
    <h2 class="panel__title">Agent explanation</h2>
    <p>{review.explanation || "No explanation supplied."}</p>
    {#if review.extent}<p class="quiet">{review.extent}</p>{/if}
    <p class="quiet identifier review-panel__meta">{review.agent} · {review.vendor} · {review.model}{review.effort ? ` · ${review.effort}` : ""} · instructions {review.prompt_revision}</p>
  </section>
  {#if changed.length}
    <section class="panel__section">
      <h2 class="panel__title">Changes</h2>
      <ul class="review-changes">
        {#each changed as row (row.id)}
          <li><span class="review-count review-count--{row.outdated ? 'outdated' : row.change}">{changeLabel(row)}</span>{named(row)}</li>
        {/each}
      </ul>
    </section>
  {/if}
  {#if review.findings.length}
    <section class="panel__section"><h2 class="panel__title">Checks</h2><ul class="findings">{#each review.findings as finding}<li>{finding}</li>{/each}</ul></section>
  {/if}
  {#if review.formal_review}
    <section class="panel__section">
      <h2 class="panel__title">Formal review</h2>
      {#if review.formal_review.impact.proofs_only}<p>Proofs only; statements and links are unchanged.</p>{/if}
      {#if review.formal_review.impact.statement_closures.length}<p>Statement closures: {review.formal_review.impact.statement_closures.join(", ")}</p>{/if}
      {#if review.formal_review.impact.line_links.length}<p>Line links: {review.formal_review.impact.line_links.join(", ")}</p>{/if}
      {#each review.formal_review.blocks as formal (formal.check.block)}
        <div class="formal-review"><p class="identifier">{formal.check.block} · {formal.check.state}</p>
          {#each formal.check.declarations as declaration (declaration.name)}<p class="identifier">{declaration.name} · {declaration.module}</p><pre>{declaration.statement}</pre>{#if declaration.reaches.length}<p>Reaches {declaration.reaches.join(", ")}</p>{/if}{/each}
          <p><strong>Agent reading:</strong> {formal.reading}</p>
        </div>
      {/each}
    </section>
  {/if}
  {#if review.lean_source}<section class="panel__section"><h2 class="panel__title">Lean module</h2><pre class="diff">{review.lean_source}</pre></section>{/if}
  {#if review.library_files.length}<section class="panel__section"><h2 class="panel__title">Library changes</h2>{#each review.library_files as file (file.path)}<p class="identifier">{file.path}</p><pre class="diff">{file.rows.map((row) => `${row.change === "added" ? "+" : row.change === "removed" ? "−" : " "} ${row.text}`).join("\n")}</pre>{/each}</section>{/if}
  {#if review.operations.length}
    <section class="panel__section">
      <details class="disclosure">
        <summary>Operations · {review.operations.length}</summary>
        <pre class="diff">{JSON.stringify(review.operations, null, 2)}</pre>
      </details>
    </section>
  {/if}
  <section class="panel__section panel__section--foot">
    {#if review.state === "awaiting_review"}
      {#if review.members.length > 1}
        <label class="setting__note" for="reviewer">Hand review to a group member</label>
        <select id="reviewer" bind:value={handoffTo}>
          <option value="">Choose a member</option>
          {#each review.members as member (member.id)}<option value={member.id}>{member.name}</option>{/each}
        </select>
        <button type="button" class="button button--small" disabled={acting || !handoffTo} onclick={handoff}>Hand over</button>
      {/if}
      {#if mine}
      <label class="setting__note" for="review-note">Reason or note for the next attempt</label>
      <textarea id="review-note" bind:value={note} rows="3" placeholder="Optional"></textarea>
      <div class="review-panel__actions">
        <button type="button" class="button button--small" disabled={acting} onclick={() => decide("reject")}>Reject</button>
        <button type="button" class="button button--small" disabled={acting} onclick={() => decide("ask_again")}>Ask again</button>
        <button type="button" class="button button--primary button--small" disabled={acting || review.outdated.length > 0} onclick={() => decide("accept")}>Accept whole proposal</button>
      </div>
      {:else}<p class="quiet">This review is assigned to {review.members.find((member) => member.id === review.reviewer)?.name ?? review.reviewer}.</p>{/if}
    {:else}
      <p>{review.state === "outdated" ? "Outdated" : review.decision ?? review.state}{review.decided_by ? ` · decided by ${review.decided_by}` : ""}{review.note ? ` · ${review.note}` : ""}</p>
    {/if}
  </section>
</aside>
