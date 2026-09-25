<script lang="ts">
  import { onMount } from "svelte";
  import type { LineSession } from "../lib/line.svelte";
  import Agents from "./Agents.svelte";
  import Blocks from "./Blocks.svelte";
  import ClaimBox from "./ClaimBox.svelte";
  import { jobs } from "./jobs.svelte";
  import LineJobs from "./LineJobs.svelte";
  import Parted from "./Parted.svelte";
  import Preview from "./Preview.svelte";
  import Privacy from "./Privacy.svelte";

  let { line }: { line: LineSession } = $props();

  onMount(() => {
    void (async () => {
      await line.takeClaim();
      await line.render();
    })();
    return () => line.detach();
  });

  async function ask(tasks: string[]) {
    if (await line.ask(tasks)) await jobs.refresh();
  }

  const saving: Record<string, string> = {
    saved: "Saved",
    saving: "Saving…",
    "not saved": "Not saved",
    committed: "Committed",
  };
</script>

<main class="apparatus">
  <header class="line-head">
    <div>
      <p class="line-head__name">{line.document?.title || line.opened.entry.name}</p>
      <p class="quiet identifier">{line.opened.entry.group} · {line.opened.entry.name}</p>
    </div>
    <span class="seen {line.mine ? 'seen--mine' : 'seen--ours'}">{line.mine ? "Private" : "Shared"}</span>
    <span class="spacer"></span>
    <span class="quiet">{saving[line.saveState]}</span>
    <ClaimBox {line} />
  </header>
  <section class="writing">
    {#if line.document}
      <Blocks {line} document={line.document} onask={ask} />
    {:else}
      <div class="blocks">
        <p class="problem">This file is not a valid line, so it cannot be edited here.</p>
        {#each line.opened.faults as fault, index (index)}<p class="quiet">{fault.message}</p>{/each}
        <pre class="version">{line.opened.source}</pre>
      </div>
    {/if}
  </section>
  <Preview {line} />
  <aside id="register">
    <Parted {line} />
    <section><p class="label">Agents</p><Agents {line} onask={ask} /></section>
    <section><p class="label">Jobs</p><LineJobs /></section>
    <section><p class="label">Visibility</p><Privacy {line} /></section>
  </aside>
</main>
