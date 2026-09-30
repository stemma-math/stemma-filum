<script lang="ts">
  // The typeset line, a page at a time: zoom, where you are, and what went
  // wrong when the last change did not typeset. The pages fit the width
  // until they are zoomed, with the buttons or a pinch.
  import Message from "../components/Message.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { app } from "../lib/app.svelte";
  import { blockCard } from "../lib/dom";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  const STEPS = [25, 33, 50, 67, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400];
  const LEAST = STEPS[0];
  const MOST = STEPS[STEPS.length - 1];
  let zoom = $state(100);
  /** Whether the pages follow the width of the view. */
  let fitting = $state(true);
  let page = $state(1);
  let pages = $state(0);
  let sheet = $state<HTMLElement>();
  let view = $state<HTMLElement>();

  const errors = $derived(line.preview?.diagnostics.filter((one) => one.error) ?? []);
  const stale = $derived(errors.length > 0 && !!line.preview?.svg);

  const bounded = (value: number) => Math.min(Math.max(value, LEAST), MOST);

  /** Zooms to `next`, in percent, keeping the point of the page under
   * `at` (in the view, by default its middle) where it is. */
  function zoomTo(next: number, at?: { x: number; y: number }) {
    next = bounded(next);
    if (!view || next === zoom) return;
    const box = view.getBoundingClientRect();
    const x = at ? at.x - box.left : box.width / 2;
    const y = at ? at.y - box.top : box.height / 2;
    const style = getComputedStyle(view);
    const left = parseFloat(style.paddingLeft) || 0;
    const top = parseFloat(style.paddingTop) || 0;
    const ratio = next / zoom;
    const scrollLeft = (view.scrollLeft + x - left) * ratio + left - x;
    const scrollTop = (view.scrollTop + y - top) * ratio + top - y;
    zoom = next;
    const scrolled = view;
    // The new size is laid out before the view can be scrolled to it.
    requestAnimationFrame(() => scrolled.scrollTo(scrollLeft, scrollTop));
  }

  function step(by: 1 | -1) {
    fitting = false;
    const next = by > 0 ? STEPS.find((one) => one > zoom + 0.5) : [...STEPS].reverse().find((one) => one < zoom - 0.5);
    zoomTo(next ?? (by > 0 ? MOST : LEAST));
  }

  /** The zoom at which the widest page fills the view's width. */
  function fit() {
    const drawn = sheet ? [...sheet.querySelectorAll<SVGSVGElement>(":scope > svg")] : [];
    if (!view || !drawn.length) return;
    const widest = Math.max(...drawn.map((one) => one.width.baseVal.value));
    const style = getComputedStyle(view);
    const room = view.clientWidth - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0);
    if (widest > 0 && room > 0) zoom = bounded(Math.floor((room / widest) * 100));
  }

  function fitWidth() {
    fitting = true;
    fit();
  }

  // While fitting, the zoom follows the view's width and the pages drawn.
  $effect(() => {
    void line.preview?.svg;
    if (!fitting || !view) return;
    fit();
    const watcher = new ResizeObserver(() => fit());
    watcher.observe(view);
    return () => watcher.disconnect();
  });

  /** A pinch on a trackpad: WebKit reports it as gestures, other engines
   * as a wheel with Control held. */
  let pinchFrom = 0;

  function gestureStart(event: Event) {
    event.preventDefault();
    pinchFrom = zoom;
  }

  function gestureChange(event: Event & { scale?: number; clientX?: number; clientY?: number }) {
    event.preventDefault();
    fitting = false;
    const at = event.clientX === undefined || event.clientY === undefined ? undefined : { x: event.clientX, y: event.clientY };
    zoomTo(pinchFrom * (event.scale ?? 1), at);
  }

  function wheel(event: WheelEvent) {
    if (!event.ctrlKey) return;
    event.preventDefault();
    fitting = false;
    zoomTo(zoom * Math.exp(-event.deltaY / 100), { x: event.clientX, y: event.clientY });
  }

  /** Listens with `passive: false`, which a pinch needs to keep the
   * window itself from zooming. */
  function pinchable(node: HTMLElement) {
    const options = { passive: false };
    node.addEventListener("wheel", wheel, options);
    node.addEventListener("gesturestart", gestureStart, options);
    node.addEventListener("gesturechange", gestureChange, options);
    return () => {
      node.removeEventListener("wheel", wheel);
      node.removeEventListener("gesturestart", gestureStart);
      node.removeEventListener("gesturechange", gestureChange);
    };
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
      disabled={zoom <= LEAST} onclick={() => step(-1)}><Glyph name="minus" /></button>
    <span class="preview__zoom identifier">{Math.round(zoom)}%</span>
    <button type="button" class="button button--icon button--small" aria-label="Zoom in" data-tip="Zoom in"
      disabled={zoom >= MOST} onclick={() => step(1)}><Glyph name="add" /></button>
    <button type="button" class="button button--small preview__fit" aria-pressed={fitting} onclick={fitWidth}>Fit width</button>
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

  <div class="preview__scroll" bind:this={view} {@attach pinchable}>
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
