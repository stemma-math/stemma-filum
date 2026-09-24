<script lang="ts">
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  const verified = $derived(line.verified);
</script>

<div>
  {#each line.opened.line?.nodes ?? [] as node (node.siglum)}
    {@const central = node.centrality === "central"}
    <div class="mark" class:central>
      <code>{node.siglum}</code>
      <span>{node.centrality}</span>
      <span>{verified.has(node.siglum) ? "Lean verified" : "not formalized"}</span>
      {#if central}
        <button onclick={() => (line.task = `refute@${node.siglum}`)}>counterexample</button>
        <button onclick={() => (line.task = `formalize@${node.siglum}`)}>formalize in Lean</button>
      {:else}
        <button onclick={() => (line.task = `promote@${node.siglum}`)}>promote</button>
      {/if}
    </div>
  {/each}
  {#each line.opened.boxes as box, at (at)}
    {@const done = box.interior.length > 0 && box.interior.every((one) => verified.has(one))}
    {@const anchor = box.after ? `@${box.after}` : ""}
    <div class="mark boxmark">
      <span>black box · {done ? "verified" : box.state}</span>
      <button onclick={() => (line.task = `fill${anchor}`)}>fill this box</button>
      {#if box.interior.length && !done}
        <button onclick={() => (line.task = `verify${anchor}`)}>verify in Lean</button>
      {/if}
    </div>
  {/each}
</div>
{#if line.missingNotation}
  <div class="mark">
    <span>this line has no notation import, so it cannot compose</span>
    <button disabled={!line.writing} onclick={() => line.addNotation()}>
      {line.writing ? "add it" : "write this line to add it"}
    </button>
  </div>
{/if}
