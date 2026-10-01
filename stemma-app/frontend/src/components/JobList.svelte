<script lang="ts">
  // Jobs, sorted by what they ask of the reader: what is under way and
  // what waits for this machine's owner are in view, each a short row whose
  // details open on request; what has finished is put away under one line.
  import { api, said } from "../lib/api";
  import { failed, sorted, standing } from "../lib/joblist";
  import { fail } from "../lib/status.svelte";
  import { LABELS } from "../lib/tasks";
  import type { Job } from "../lib/types";
  import Glyph from "./Glyph.svelte";

  let { jobs, empty }: { jobs: Job[]; empty: string } = $props();
  let outputs = $state<Record<string, string>>({});
  let opened = $state<Record<string, boolean>>({});

  const groups = $derived(sorted(jobs));
  const failures = $derived(groups.finished.filter(failed).length);

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
      fail(error);
    }
  }

  async function resolve(job: Job) {
    try {
      await api.clearJob(job.id);
      job.owed = false;
    } catch (error) {
      fail(error);
    }
  }

  /** `prove@thm:main` → the task's words and what it is about. */
  function asked(task: string): { words: string; about: string | null } {
    const [verb, about] = task.split("@");
    const words = LABELS[verb] ?? verb;
    return { words, about: about ?? null };
  }

  /** The one line worth reading without opening the job. */
  function news(job: Job): string | null {
    if (job.state === "running") return job.cancel_requested ? "Cancellation requested" : job.progress;
    return job.said || null;
  }
</script>

{#snippet row(job: Job)}
  {@const task = asked(job.task)}
  {@const state = standing(job)}
  {@const told = news(job)}
  <li class="job" class:job--attention={job.owed}>
    <div class="job__head">
      <span class="job__task">{task.words}</span>
      <span class="chip {state.tone}">
        {#if job.state === "running"}<span class="spinner" aria-hidden="true"></span>{/if}{state.words}
      </span>
    </div>
    <span class="job__meta">
      {#if task.about}<span class="identifier">{task.about}</span> · {/if}{job.agent}
    </span>
    {#if told}<span class="job__said">{told}</span>{/if}
    <div class="job__actions">
      {#if job.owed}
        <button type="button" class="button button--small" onclick={() => resolve(job)}>Mark as resolved</button>
      {/if}
      {#if job.state === "queued" || job.state === "running"}
        <button type="button" class="button button--link" onclick={() => cancel(job)}>{job.cancel_requested ? "Cancel again" : "Cancel"}</button>
      {/if}
      <button type="button" class="button button--link job__details" aria-expanded={!!opened[job.id]}
        onclick={() => (opened[job.id] = !opened[job.id])}>{opened[job.id] ? "Hide details" : "Details"}</button>
    </div>
    {#if opened[job.id]}
      <div class="job__more">
        {#if job.state === "running" && job.said}<span class="job__said">{job.said}</span>{/if}
        {#if job.workspace}<span class="identifier job__workspace">{job.workspace}</span>{/if}
        {#if job.raw_output}
          <button type="button" class="button button--link job__output" aria-expanded={outputs[job.id] !== undefined} onclick={() => show(job.id)}>
            {outputs[job.id] === undefined ? "View output" : "Hide output"}
          </button>
          {#if outputs[job.id] !== undefined}<pre class="job__raw">{outputs[job.id]}</pre>{/if}
        {/if}
        <span class="identifier job__id">{job.id}</span>
      </div>
    {/if}
  </li>
{/snippet}

<div class="job-list">
  {#if groups.active.length || groups.attention.length}
    <ul class="jobs">
      {#each groups.attention as job (job.id)}{@render row(job)}{/each}
      {#each groups.active as job (job.id)}{@render row(job)}{/each}
    </ul>
  {:else}
    <p class="empty-box"><Glyph name="sparkle" small />{empty}</p>
  {/if}

  {#if groups.finished.length}
    <details class="disclosure job-list__finished">
      <summary>
        Finished · {groups.finished.length}{#if failures}<span class="job-list__failed">{` · ${failures} failed`}</span>{/if}
      </summary>
      <ul class="jobs">
        {#each groups.finished as job (job.id)}{@render row(job)}{/each}
      </ul>
    </details>
  {/if}
</div>
