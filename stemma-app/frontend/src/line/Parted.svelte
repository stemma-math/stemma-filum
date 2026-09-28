<script lang="ts">
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
  const deciding = $derived(app.stateOf(line.path) === "needs_decision");

  // Looked at again whenever the line starts or stops needing a decision.
  $effect(() => {
    if (!deciding) {
      found = [];
      return;
    }
    api.parted(line.path).then((conflicts) => {
      found = conflicts;
      problem = null;
    }, (error) => (problem = said(error)));
  });

  async function keep() {
    try {
      await api.settle(line.path, found.map((one) => ({ block: one.block, keep: kept[one.block] })));
      await app.lookAgain();
      await line.reread();
    } catch (error) {
      problem = said(error);
    }
  }
</script>

{#if problem || found.length}
  <section class="panel__section conflicts">
    <h2 class="panel__title"><Glyph name="warning" small />Needs a decision</h2>
    {#if problem}
      <p class="panel__text panel__text--failed">{problem}</p>
    {:else}
      <p class="panel__text">These blocks were changed on this machine and elsewhere. Choose which version to keep; everything else was brought together already.</p>
      {#each found as one (one.block)}
        <div class="conflict">
          <strong>{one.label}</strong>
          <span class="panel__text">{one.said}</span>
          <div class="segmented segmented--stacked" role="group" aria-label="Version to keep">
            <button type="button" aria-pressed={kept[one.block] === "here"} onclick={() => (kept[one.block] = "here")}>This machine's</button>
            <button type="button" aria-pressed={kept[one.block] === "there"} onclick={() => (kept[one.block] = "there")}>The other one</button>
          </div>
          <pre class="conflict__version">{one.here ?? "Deleted"}</pre>
          <pre class="conflict__version">{one.there ?? "Deleted"}</pre>
        </div>
      {/each}
      <button type="button" class="button button--primary button--small" disabled={!complete} onclick={keep}>Keep these</button>
      <p class="panel__text">The version you do not keep stays in the line's history.</p>
    {/if}
  </section>
{/if}
