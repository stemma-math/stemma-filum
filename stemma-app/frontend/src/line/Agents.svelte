<script lang="ts">
  import type { LineSession } from "../lib/line.svelte";
  import { common, tasksFor } from "../lib/tasks";

  let { line, onask }: { line: LineSession; onask: (tasks: string[]) => void } = $props();

  const chosen = $derived(line.document?.blocks.filter((block) => line.selected.has(block.id)) ?? []);
  const offered = $derived(
    line.document
      ? common(chosen.map((block) => tasksFor(line.document!, block, (id) => line.nodeOf(id))))
      : [],
  );
</script>

{#if line.agents.length}
  <label class="field">
    <select bind:value={line.agent} aria-label="Agent">
      {#each line.agents as agent (agent.value)}
        <option value={agent.value} disabled={!!agent.reason}>
          {agent.label}{agent.reason ? ` · ${agent.reason}` : ""}
        </option>
      {/each}
    </select>
  </label>

  {#if chosen.length}
    <p class="quiet">
      {chosen.length === 1 ? "1 block selected" : `${chosen.length} blocks selected`}
      · <button type="button" class="link-button" onclick={() => line.selected.clear()}>Clear</button>
    </p>
    {#if offered.length}
      <div class="agent-tasks">
        {#each offered as task (task.verb)}
          <button type="button" onclick={() => onask(task.tasks)}>{task.label}</button>
        {/each}
      </div>
      {#if chosen.length > 1}
        <p class="quiet">Each selected block gets its own job.</p>
      {/if}
    {:else}
      <p class="quiet">No task applies to every selected block.</p>
    {/if}
  {:else}
    <p class="quiet">Select blocks to ask an agent about several at once, or use ✦ on a block.</p>
  {/if}
{:else}
  <p class="quiet">No machine works for this group yet.</p>
{/if}
