<script lang="ts">
  import { app } from "../lib/app.svelte";
  import type { Known } from "../lib/types";

  let menuOpen = $state(false);
  let menu = $state<HTMLElement>();
  let toggle = $state<HTMLElement>();

  function away(event: PointerEvent) {
    const at = event.target as Node;
    if (!menu?.contains(at) && !toggle?.contains(at)) menuOpen = false;
  }

  $effect(() => {
    if (!menuOpen) return;
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  });

  function choose(group: string) {
    menuOpen = false;
    void app.chooseGroup(group);
  }

  const here = $derived(app.stage === "editor" ? app.line?.path : undefined);
</script>

{#snippet list(lines: Known[], empty: string)}
  {#each lines as line (line.path)}
    <button class="line" class:line--here={line.path === here} onclick={() => app.open(line.path)}>
      <span class="line__name">{line.title ?? line.name}</span>
      {#if line.description}<span class="line__description">{line.description}</span>{/if}
    </button>
  {:else}
    <p class="rail__empty">{empty}</p>
  {/each}
{/snippet}

<aside class="rail">
  <button
    bind:this={toggle}
    class="rail__group"
    aria-haspopup="menu"
    aria-expanded={menuOpen}
    onclick={() => (menuOpen = !menuOpen)}
  >
    <span class="rail__group-name">{app.group ?? "No group"}</span>
    <span class="rail__chevron" aria-hidden="true"></span>
  </button>
  {#if menuOpen}
    <div bind:this={menu} class="menu" role="menu">
      {#each app.groups as group (group)}
        <button role="menuitem" class:here={group === app.group} onclick={() => choose(group)}>{group}</button>
      {:else}
        <p class="rail__empty">No groups on this machine.</p>
      {/each}
    </div>
  {/if}
  <div class="rail__body">
    {#if app.group}
      <button class="rail__entry rail__entry--make" onclick={() => app.goTo("new-line")}>
        <span class="rail__entry-mark" aria-hidden="true">+</span>New line
      </button>
      <p class="rail__label">Private</p>
      {@render list(app.railLines.mine, "No private lines yet.")}
      <p class="rail__label">Shared</p>
      {@render list(app.railLines.ours, "Nothing has been shared yet.")}
    {:else}
      <p class="rail__empty">No groups are on this machine yet. Sync to bring them here.</p>
    {/if}
  </div>
  <div class="rail__foot">
    <button class="rail__entry" onclick={() => app.goTo("vault")}>
      <span class="rail__entry-mark" aria-hidden="true">§</span>Definitions
    </button>
  </div>
</aside>
