<script lang="ts">
  import { onMount } from "svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import { reviewedBlocks } from "../lib/review";
  import { fail, tell } from "../lib/status.svelte";
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

  function markedPage(page: string, side: "before" | "after") {
    const styles = rows.flatMap((row) => {
      if (!/^blk:[0-9A-Z]+$/.test(row.id)) return [];
      const label = row.outdated ? "Outdated" : row.change === "added" ? "Proposed" : row.change === "removed" ? "Removed" : "Edited";
      const visible = side === "before" ? row.before : row.after;
      if (!visible || (row.change === "kept" && !row.outdated)) return [];
      const color = row.outdated ? "#aa6b10" : row.change === "added" ? "#168452" : row.change === "removed" ? "#9b4242" : "#376db1";
      const selector = `section[id="${row.id}"]`;
      return [`${selector} { outline: 2px solid ${color}; outline-offset: 5px; position: relative; }`,
        `${selector}::before { content: "${label}"; display: block; color: ${color}; font: 600 12px sans-serif; margin-bottom: 4px; }`,
        row.change === "removed" && side === "before" ? `${selector} { text-decoration: line-through; opacity: 0.7; }` : ""];
    }).join("\n");
    return page.replace("</head>", `<style>${styles}</style></head>`);
  }

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
      tell(await api.decideProposal(review.proposal, verdict, note.trim() || null));
      if (verdict === "accept") {
        tell(await api.applyProposal(review.proposal));
        await line.reread();
        await app.lookAgain();
      }
      await app.refreshWaiting();
      app.review = await api.proposal(review.proposal, line.opened.source);
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
      app.review = await api.proposal(review.proposal, line.opened.source);
      tell("Review handed to the group member.");
    } catch (error) { fail(error); }
    finally { acting = false; }
  }
</script>

<div class="review-line">
  <div class="review-line__head">
    <button type="button" class="button button--small" onclick={() => (app.review = null)}>Back to line</button>
    <div>
      <h2>Review proposal · {review.task}</h2>
      <p class="quiet identifier">{review.agent} · {review.vendor} · {review.model}{review.effort ? ` · ${review.effort}` : ""} · instructions {review.prompt_revision}</p>
    </div>
    <div class="segmented" role="group" aria-label="Proposal view">
      <button type="button" aria-pressed={view === "typeset"} onclick={() => (view = "typeset")}>Typeset</button>
      <button type="button" aria-pressed={view === "source"} onclick={() => (view = "source")}>Source</button>
    </div>
  </div>

  {#if review.outdated.length}
    <p class="notice notice--warning" role="alert">This proposal is outdated in {review.outdated.length} block{review.outdated.length === 1 ? "" : "s"}. Ask again on the current line.</p>
  {/if}

  {#if view === "typeset"}
    <div class="review-line__pages">
      <section><h3>Before</h3>{#if beforePage}<iframe class="review-line__sheet" sandbox="" title="The current typeset line" srcdoc={markedPage(beforePage, "before")}></iframe>{:else if rendering}<p>Typesetting…</p>{:else}<p class="quiet">No typeset version is available.</p>{/if}</section>
      <section><h3>With proposal</h3>{#if afterPage}<iframe class="review-line__sheet" sandbox="" title="The line with the proposal" srcdoc={markedPage(afterPage, "after")}></iframe>{:else if rendering}<p>Typesetting…</p>{:else}<p class="quiet">No typeset version is available.</p>{/if}</section>
    </div>
  {:else}
    <div class="review-line__blocks">
      {#each rows as row (row.id)}
        <article class:review-line__block--added={row.change === "added"} class:review-line__block--removed={row.change === "removed"} class:review-line__block--edited={row.change === "edited"} class:review-line__block--outdated={row.outdated}>
          <p class="identifier review-line__block-label">{row.before?.kind ?? row.after?.kind} · {row.id} · {row.outdated ? "outdated" : row.change === "added" ? "proposed" : row.change}</p>
          {#if row.before && row.change !== "kept"}<div><span class="quiet">Before</span><pre>{row.before.content}</pre></div>{/if}
          {#if row.after}<div><span class="quiet">{row.change === "edited" ? "After" : row.change === "added" ? "Proposed" : "Current"}</span><pre>{row.after.content}</pre></div>{/if}
          {#if row.outdated && row.offered}<div><span class="quiet">Agent proposed</span><pre>{row.offered.content}</pre></div>{/if}
        </article>
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
    <p class="quiet">{review.extent}</p>
  </section>
  {#if review.operations.length}
    <section class="panel__section"><h2 class="panel__title">Operations</h2><pre class="diff">{JSON.stringify(review.operations, null, 2)}</pre></section>
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
