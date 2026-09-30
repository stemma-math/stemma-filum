<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { ofLine } from "../lib/joblist";
  import { jobs } from "../lib/jobs.svelte";
  import type { LineSession } from "../lib/line.svelte";

  let { line }: { line: LineSession } = $props();

  onMount(() => void jobs.refresh());
</script>

{#if jobs.problem}
  <Message kind="error" text={jobs.problem} />
{:else}
  <JobList jobs={ofLine(jobs.list, line.opened.entry)} empty="Nothing is running on this line." />
{/if}
