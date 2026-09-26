<script lang="ts">
  // One tooltip for the whole window: what `data-tip` says on the element
  // under the pointer or with the keyboard focus, after half a second, with
  // its shortcut from `data-shortcut`.
  const DELAY = 500;

  let shown = $state<{ text: string; shortcut: string | null; x: number; y: number; below: boolean } | null>(null);
  let timer = 0;
  let over: Element | null = null;

  function place(target: Element) {
    const text = target.getAttribute("data-tip");
    if (!text) return;
    const box = target.getBoundingClientRect();
    const below = box.top < 48;
    shown = {
      text,
      shortcut: target.getAttribute("data-shortcut"),
      x: box.left + box.width / 2,
      y: below ? box.bottom + 8 : box.top - 8,
      below,
    };
  }

  function start(target: Element | null) {
    window.clearTimeout(timer);
    over = target;
    shown = null;
    if (target) timer = window.setTimeout(() => place(target), DELAY);
  }

  const tipped = (node: EventTarget | null) => (node instanceof Element ? node.closest("[data-tip]") : null);
</script>

<svelte:window
  onpointerover={(event) => { const target = tipped(event.target); if (target !== over) start(target); }}
  onpointerdown={() => start(null)}
  onfocusin={(event) => { const target = tipped(event.target); if (target?.matches(":focus-visible")) start(target); }}
  onfocusout={() => start(null)}
  onkeydown={(event) => { if (event.key === "Escape") start(null); }}
/>

{#if shown}
  <div
    class="tooltip tooltip--floating"
    class:tooltip--below={shown.below}
    role="tooltip"
    style:left="{shown.x}px"
    style:top="{shown.y}px"
  >
    {shown.text}{#if shown.shortcut}<kbd>{shown.shortcut}</kbd>{/if}
  </div>
{/if}
