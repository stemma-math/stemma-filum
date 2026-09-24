<script lang="ts">
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { jobs } from "./jobs.svelte";

  onMount(() => void jobs.refresh());
</script>

{#if jobs.problem}
  {jobs.problem}
{:else}
  <JobList jobs={jobs.list} empty="Nothing is running here." />
{/if}
