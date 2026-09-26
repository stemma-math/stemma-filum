<script lang="ts">
  // A panel that opens beside the button that asked for it: a menu or a
  // popover. It closes on a click outside it or on Escape, which returns
  // focus to that button. In a menu the arrow keys move between items.
  import { tick, type Snippet } from "svelte";

  let {
    open = $bindable(false),
    label,
    align = "start",
    place = "below",
    menu = false,
    children,
  }: {
    open: boolean;
    label: string;
    align?: "start" | "end" | "center";
    place?: "below" | "above";
    menu?: boolean;
    children: Snippet;
  } = $props();

  let panel = $state<HTMLElement>();

  const items = () => [...(panel?.querySelectorAll<HTMLElement>("[role=menuitem]:not(:disabled), [role=menuitemcheckbox]:not(:disabled)") ?? [])];

  $effect(() => {
    if (!open || !menu) return;
    void tick().then(() => items()[0]?.focus());
  });

  function outside(event: MouseEvent) {
    if (open && panel && !panel.parentElement?.contains(event.target as Node)) open = false;
  }

  function keys(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.stopPropagation();
      open = false;
      (panel?.parentElement?.querySelector("[aria-expanded]") as HTMLElement | null)?.focus();
      return;
    }
    if (!menu || !panel?.contains(document.activeElement)) return;
    const all = items();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const go = (to: number) => { event.preventDefault(); all[(to + all.length) % all.length]?.focus(); };
    if (event.key === "ArrowDown") go(at + 1);
    else if (event.key === "ArrowUp") go(at - 1);
    else if (event.key === "Home") go(0);
    else if (event.key === "End") go(all.length - 1);
  }
</script>

<svelte:window onclick={outside} onkeydown={keys} />

{#if open}
  <div
    class="bubble bubble--{align} bubble--{place}"
    class:menu
    role={menu ? "menu" : "dialog"}
    aria-label={label}
    bind:this={panel}
  >
    {@render children()}
  </div>
{/if}
