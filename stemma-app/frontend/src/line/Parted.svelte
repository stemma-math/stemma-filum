<script lang="ts">
  import { onMount } from "svelte";
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
  <section>
    <p class="label">Conflicts</p>
    {#if problem}
      {problem}
    {:else}
      {#each found as one (one.block)}
        <div class="job">
          <strong>{one.label}</strong><span class="quiet">{one.said}</span>
          <pre class="version">{one.here ?? "Deleted"}</pre>
          <pre class="version">{one.there ?? "Deleted"}</pre>
          <span>
            <button class:kept={kept[one.block] === "here"} onclick={() => (kept[one.block] = "here")}>Keep this machine's</button>
            <button class:kept={kept[one.block] === "there"} onclick={() => (kept[one.block] = "there")}>Keep the server's</button>
          </span>
        </div>
      {/each}
      <button disabled={!complete} onclick={merge}>Merge</button>
      <p class="quiet">The version you do not keep stays in this machine's history.</p>
    {/if}
  </section>
{/if}
