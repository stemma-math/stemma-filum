<script lang="ts">
  import { api, said } from "../lib/api";
  import type { Job } from "../lib/types";

  let { jobs, empty }: { jobs: Job[]; empty: string } = $props();
  let outputs = $state<Record<string, string>>({});

  async function show(job: string) {
    try { outputs[job] = await api.jobOutput(job); } catch (error) { outputs[job] = said(error); }
  }
</script>

{#each jobs as job (job.id)}
  <div class="job">
    <code>{job.id}</code>
    <span>{job.task} · {job.agent}</span>
    <span>{job.said}</span>
    {#if job.raw_output}
      <button class="raw" onclick={() => show(job.id)}>raw output</button>
      <pre class="raw-output" hidden={outputs[job.id] === undefined}>{outputs[job.id]}</pre>
    {/if}
  </div>
{:else}
  {empty}
{/each}
