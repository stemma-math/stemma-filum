<script lang="ts">
  import { onMount } from "svelte";
  import JobList from "../components/JobList.svelte";
  import { jobs } from "./jobs.svelte";

  onMount(() => void jobs.refresh());
</script>

{#if jobs.problem}
  <p class="panel__text panel__text--failed">{jobs.problem}</p>
{:else}
  <JobList jobs={jobs.list} empty="Nothing is running." />
{/if}
