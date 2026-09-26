<script lang="ts">
  import { onMount } from "svelte";
  import Glyph from "../components/Glyph.svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import type { Conflict, Keep } from "../lib/types";

  let { line }: { line: LineSession } = $props();

  let found = $state<Conflict[]>([]);
  let problem = $state<string | null>(null);
  let kept = $state<Record<string, Keep>>({});

  const complete = $derived(found.every((one) => kept[one.block]));

  onMount(async () => {
    try { found = await api.parted(line.path); } catch (error) { problem = said(error); }
  });

  async function merge() {
    try {
      await api.settle(line.path, found.map((one) => ({ block: one.block, keep: kept[one.block] })));
      await app.open(line.path);
    } catch (error) {
      problem = said(error);
    }
  }
</script>

{#if problem || found.length}
  <section class="panel__section conflicts">
    <h2 class="panel__title"><Glyph name="warning" small />Conflicts</h2>
    {#if problem}
      <p class="panel__text panel__text--failed">{problem}</p>
    {:else}
      <p class="panel__text">This machine and the server both changed these blocks. Choose which version to keep.</p>
      {#each found as one (one.block)}
        <div class="conflict">
          <strong>{one.label}</strong>
          <span class="panel__text">{one.said}</span>
          <div class="segmented segmented--stacked" role="group" aria-label="Version to keep">
            <button type="button" aria-pressed={kept[one.block] === "here"} onclick={() => (kept[one.block] = "here")}>This machine's</button>
            <button type="button" aria-pressed={kept[one.block] === "there"} onclick={() => (kept[one.block] = "there")}>The server's</button>
          </div>
          <pre class="conflict__version">{one.here ?? "Deleted"}</pre>
          <pre class="conflict__version">{one.there ?? "Deleted"}</pre>
        </div>
      {/each}
      <button type="button" class="button button--primary button--small" disabled={!complete} onclick={merge}>Merge</button>
      <p class="panel__text">The version you do not keep stays in this machine's history.</p>
    {/if}
  </section>
{/if}
