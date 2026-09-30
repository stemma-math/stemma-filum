<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { jobs } from "../lib/jobs.svelte";

  onMount(() => void jobs.refresh());
</script>

{#if jobs.problem}
  <Message kind="error" text={jobs.problem} />
{:else}
  <JobList jobs={jobs.list} empty="Nothing is running." />
{/if}
