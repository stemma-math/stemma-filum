<script lang="ts">
  import { api, said } from "../lib/api";
  import { LABELS } from "../lib/tasks";
  import type { Job } from "../lib/types";
  import Glyph from "./Glyph.svelte";

  let { jobs, empty }: { jobs: Job[]; empty: string } = $props();
  let outputs = $state<Record<string, string>>({});

  async function show(job: string) {
    if (outputs[job] !== undefined) {
      delete outputs[job];
      return;
    }
    try { outputs[job] = await api.jobOutput(job); } catch (error) { outputs[job] = said(error); }
  }

  async function cancel(job: Job) {
    try {
      await api.cancelJob(job.id);
      job.cancel_requested = true;
      if (job.state === "queued") job.state = "withdrawn";
    } catch (error) {
      outputs[job.id] = said(error);
    }
  }

  /** `prove@thm:main` → the task's words and what it is about. */
  function asked(task: string): { words: string; about: string | null } {
    const [verb, about] = task.split("@");
    const words = LABELS[verb] ?? verb;
    return { words, about: about ?? null };
  }

  const states: Record<string, { words: string; tone: string }> = {
    queued: { words: "Queued", tone: "" },
    running: { words: "Running", tone: "chip--accent" },
    done: { words: "Done", tone: "chip--success" },
    withdrawn: { words: "Withdrawn", tone: "chip--dashed" },
  };
</script>

{#if jobs.length}
  <ul class="jobs">
    {#each jobs as job (job.id)}
      {@const task = asked(job.task)}
      {@const state = states[job.state] ?? { words: job.state, tone: "" }}
      <li class="job">
        <div class="job__head">
          <span class="job__task">{task.words}</span>
          <span class="chip {state.tone}">
            {#if job.state === "running"}<span class="spinner" aria-hidden="true"></span>{/if}{state.words}
          </span>
        </div>
        <span class="job__meta">
          {#if task.about}<span class="identifier">{task.about}</span> · {/if}{job.agent}
        </span>
        {#if job.said}<span class="job__said">{job.said}</span>{/if}
        {#if job.progress}<span class="job__said">{job.progress}</span>{/if}
        {#if job.workspace}<span class="identifier">{job.workspace}</span>{/if}
        {#if (job.state === "queued" || job.state === "running") && !job.cancel_requested}
          <button type="button" class="button button--link job__output" onclick={() => cancel(job)}>Cancel</button>
        {:else if job.cancel_requested && job.state === "running"}
          <span class="job__said">Cancellation requested</span>
        {/if}
        {#if job.raw_output}
          <button type="button" class="button button--link job__output" aria-expanded={outputs[job.id] !== undefined} onclick={() => show(job.id)}>
            {outputs[job.id] === undefined ? "View output" : "Hide output"}
          </button>
          {#if outputs[job.id] !== undefined}<pre class="job__raw">{outputs[job.id]}</pre>{/if}
        {/if}
        <span class="identifier job__id">{job.id}</span>
      </li>
    {/each}
  </ul>
{:else}
  <p class="empty-box"><Glyph name="sparkle" small />{empty}</p>
{/if}
