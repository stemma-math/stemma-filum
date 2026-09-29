<script lang="ts">
  // The typeset line, a page at a time: zoom, where you are, and what went
  // wrong when the last change did not typeset.
  import Message from "../components/Message.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { app } from "../lib/app.svelte";
  import { blockCard } from "../lib/dom";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  const STEPS = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];
  let zoom = $state(100);
  let page = $state(1);
  let pages = $state(0);
  let sheet = $state<HTMLElement>();

  const errors = $derived(line.preview?.diagnostics.filter((one) => one.error) ?? []);
  const stale = $derived(errors.length > 0 && !!line.preview?.svg);

  function step(by: 1 | -1) {
    const at = STEPS.findIndex((one) => one >= zoom);
    const next = STEPS[Math.min(Math.max((at < 0 ? STEPS.length - 1 : at) + by, 0), STEPS.length - 1)];
    zoom = next;
  }

  // Counts the pages and follows which one is most in view.
  $effect(() => {
    void line.preview?.svg;
    if (!sheet) return;
    const drawn = [...sheet.querySelectorAll<SVGElement>(":scope > svg")];
    pages = drawn.length;
    const seen = new Map<Element, number>();
    const watcher = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) seen.set(entry.target, entry.intersectionRatio);
        let best = 0;
        let most = -1;
        drawn.forEach((one, index) => {
          const ratio = seen.get(one) ?? 0;
          if (ratio > most) { most = ratio; best = index; }
        });
        page = best + 1;
      },
      { threshold: [0, 0.25, 0.5, 0.75, 1] },
    );
    drawn.forEach((one) => watcher.observe(one));
    return () => watcher.disconnect();
  });

  function goTo(block: string | null) {
    app.view = "blocks";
    if (!block) return;
    line.selected.clear();
    line.selected.add(block);
    requestAnimationFrame(() => blockCard(block)?.scrollIntoView({ block: "center" }));
  }
</script>

<section class="preview" aria-label="PDF">
  <div class="preview__bar">
    <button type="button" class="button button--icon button--small" aria-label="Zoom out" data-tip="Zoom out"
      disabled={zoom <= STEPS[0]} onclick={() => step(-1)}><Glyph name="minus" /></button>
    <span class="preview__zoom identifier">{zoom}%</span>
    <button type="button" class="button button--icon button--small" aria-label="Zoom in" data-tip="Zoom in"
      disabled={zoom >= STEPS[STEPS.length - 1]} onclick={() => step(1)}><Glyph name="add" /></button>
    {#if pages}
      <span class="preview__divider" aria-hidden="true"></span>
      <span class="preview__page">Page {page} of {pages}</span>
    {/if}
  </div>

  {#if stale || line.previewError}
    <Message kind="warning" class="preview__notice"
      text={line.previewError ?? `This change does not typeset: ${errors[0].message}. The page shows the last version that did.`}>
      {#snippet actions()}
        {#if !line.previewError && errors[0].block}
          <button type="button" class="button button--link" onclick={() => goTo(errors[0].block)}>Go to block</button>
        {/if}
      {/snippet}
    </Message>
  {/if}

  <div class="preview__scroll">
    {#if line.preview?.svg}
      <div class="preview__sheet" class:preview__sheet--stale={stale} style:zoom={zoom / 100} bind:this={sheet}>
        {@html line.preview.svg}
      </div>
    {:else if !line.previewError}
      <div class="preview__sheet preview__sheet--empty">
        <div class="skeleton preview__skeleton"></div>
        <p class="quiet">The typeset line will appear here.</p>
      </div>
    {/if}
  </div>
</section>
