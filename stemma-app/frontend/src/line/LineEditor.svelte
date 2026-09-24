<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import ClaimBox from "./ClaimBox.svelte";
  import Launcher from "./Launcher.svelte";
  import LineJobs from "./LineJobs.svelte";
  import Nodes from "./Nodes.svelte";
  import Parted from "./Parted.svelte";
  import Preview from "./Preview.svelte";
  import Privacy from "./Privacy.svelte";

  let { line }: { line: LineSession } = $props();
  let writingArea: HTMLElement;

  onMount(() => {
    line.attach(writingArea);
    void (async () => {
      await line.takeClaim();
      await line.render();
    })();
    return () => line.detach();
  });
</script>

<main class="apparatus">
  <header class="line-head">
    <div>
      <p class="line-head__name">{app.lineTitle}</p>
      <p class="quiet identifier">{line.opened.entry.group} · {line.opened.entry.name}</p>
    </div>
    <span class="seen {line.mine ? 'seen--mine' : 'seen--ours'}">{line.mine ? "Yours alone" : "Shared"}</span>
    <span class="spacer"></span>
    <span class="quiet">{line.saveState}</span>
    <ClaimBox {line} />
  </header>
  <section class="writing" bind:this={writingArea}></section>
  <Preview {line} />
  <aside id="register">
    <section>
      <p class="label">Apparatus</p>
      <Nodes {line} />
      <pre id="source-diagnostics">{line.diagnostics}</pre>
    </section>
    <Parted {line} />
    <section><p class="label">Ask a machine</p><Launcher {line} /></section>
    <section><p class="label">Jobs</p><LineJobs /></section>
    <section><p class="label">Who can see it</p><Privacy {line} /></section>
  </aside>
</main>
