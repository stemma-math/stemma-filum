<script lang="ts">
  // A small panel that opens beside the button that asked for it, and
  // closes on a click outside it or on Escape.
  import type { Snippet } from "svelte";

  let {
    open = $bindable(false),
    label,
    align = "start",
    children,
  }: { open: boolean; label: string; align?: "start" | "end"; children: Snippet } = $props();

  let panel = $state<HTMLElement>();

  function outside(event: MouseEvent) {
    if (open && panel && !panel.parentElement?.contains(event.target as Node)) open = false;
  }

  function escape(event: KeyboardEvent) {
    if (open && event.key === "Escape") open = false;
  }
</script>

<svelte:window onclick={outside} onkeydown={escape} />

{#if open}
  <div class="bubble bubble--{align}" role="dialog" aria-label={label} bind:this={panel}>
    {@render children()}
  </div>
{/if}
