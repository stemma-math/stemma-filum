<script lang="ts">
  import { onMount } from "svelte";
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import type { Disagreement, Keep } from "../lib/types";

  let { line }: { line: LineSession } = $props();

  let found = $state<Disagreement[]>([]);
  let problem = $state<string | null>(null);
  let kept = $state<Record<string, Keep>>({});

  const key = (one: Disagreement) => `${one.siglum}|${one.part}`;
  const complete = $derived(found.every((one) => kept[key(one)]));

  onMount(async () => {
    try { found = await api.parted(line.path); } catch (error) { problem = said(error); }
  });

  async function settle() {
    try {
      await api.settle(line.path, found.map((one) => ({ siglum: one.siglum, part: one.part, keep: kept[key(one)] })));
      await app.open(line.path);
    } catch (error) {
      problem = said(error);
    }
  }
</script>

{#if problem || found.length}
  <section>
    <p class="label">Two versions</p>
    {#if problem}
      {problem}
    {:else}
      {#each found as one (key(one))}
        <div class="job">
          <strong>{one.mark}</strong><span class="quiet">{one.said}</span>
          <pre class="version">{one.here ?? "it is not here"}</pre>
          <pre class="version">{one.there ?? "it is not there"}</pre>
          <span>
            <button class:kept={kept[key(one)] === "here"} onclick={() => (kept[key(one)] = "here")}>Keep this machine's</button>
            <button class:kept={kept[key(one)] === "there"} onclick={() => (kept[key(one)] = "there")}>Keep the other</button>
          </span>
        </div>
      {/each}
      <button disabled={!complete} onclick={settle}>Bring them together</button>
      <p class="quiet">The version you do not select will remain in this machine's history.</p>
    {/if}
  </section>
{/if}
