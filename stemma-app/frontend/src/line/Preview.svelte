<script lang="ts">
  import type { LineSession, Pane } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();
  let figure: HTMLElement;

  $effect(() => {
    line.setFigure(figure);
    return () => line.setFigure(null);
  });

  const panes: [Pane, string][] = [["document", "Document"], ["descent", "Descent"]];
</script>

<section class="preview">
  <div class="tabs">
    {#each panes as [pane, label] (pane)}
      <button class:here={line.showing === pane} onclick={() => line.show(pane)}>{label}</button>
    {/each}
  </div>
  <div id="page" hidden={line.showing !== "document"}>
    {#if line.previewError}
      <pre class="diagnostic">{line.previewError}</pre>
    {:else if line.preview}
      {@html line.preview.svg}
      {#if line.preview.diagnostics.length}
        <pre class="diagnostic">{line.preview.diagnostics.join("\n")}</pre>
      {/if}
    {:else}
      <p class="empty">The composed document will appear here.</p>
    {/if}
  </div>
  <figure class="descent" bind:this={figure} hidden={line.showing !== "descent"} aria-label="The descent of this line">
    <canvas class="descent__canvas" aria-hidden="true"></canvas>
  </figure>
</section>
