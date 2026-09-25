<script lang="ts">
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();
</script>

<section class="preview">
  <div id="page">
    {#if line.previewError}
      <pre class="diagnostic">{line.previewError}</pre>
    {:else if line.preview?.svg}
      {@html line.preview.svg}
    {:else}
      <p class="empty">The typeset line will appear here.</p>
    {/if}
  </div>
</section>
