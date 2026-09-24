<script lang="ts">
  import { api, said } from "../lib/api";
  import type { LineSession } from "../lib/line.svelte";
  import { tell } from "../lib/status.svelte";
  import { jobs } from "./jobs.svelte";

  let { line }: { line: LineSession } = $props();

  const options = $derived(
    line.offers.flatMap((machine) => machine.agents.map((agent) => ({ machine, agent }))),
  );
  let chosen = $state("");

  $effect(() => {
    if (!chosen) {
      const first = options.find(({ agent }) => !agent.reason) ?? options[0];
      if (first) chosen = `${first.machine.machine}|${first.agent.name}`;
    }
  });

  async function ask() {
    try {
      const [machine, agent] = chosen.split("|");
      await line.save();
      tell(await api.launch(line.path, line.task, agent, machine));
      await jobs.refresh();
    } catch (error) {
      tell(said(error));
    }
  }
</script>

{#if options.length}
  <select><option value={line.task}>{line.task}</option></select>
  <select bind:value={chosen}>
    {#each options as { machine, agent } (`${machine.machine}|${agent.name}`)}
      <option value="{machine.machine}|{agent.name}" disabled={!!agent.reason}>
        {machine.label} · {machine.presence} · {agent.called}{agent.reason ? ` · ${agent.reason}` : ""}
      </option>
    {/each}
  </select>
  <button disabled={options.every(({ agent }) => agent.reason)} onclick={ask}>Ask</button>
{:else}
  <p class="quiet">No machine works for this group yet.</p>
{/if}
