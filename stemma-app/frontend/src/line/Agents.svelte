<script lang="ts">
  // Which agent is asked, and what it can be asked about the blocks
  // selected.
  import Bubble from "../components/Bubble.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { isStatement, nameOf } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";
  import { common, tasksFor } from "../lib/tasks";
  import type { Block } from "../lib/types";

  let { line, onask }: { line: LineSession; onask: (tasks: string[]) => void } = $props();

  let choosing = $state(false);

  /** Every agent on every machine that works for this group. */
  const choices = $derived(
    line.offers.flatMap((machine) =>
      machine.agents.map((agent) => ({
        value: `${machine.machine}|${agent.name}`,
        agent: agent.called,
        machine: machine.label,
        reason: agent.reason,
      })),
    ),
  );
  const chosenAgent = $derived(choices.find((one) => one.value === line.agent));

  const chosen = $derived(line.document?.blocks.filter((block) => line.selected.has(block.id)) ?? []);
  const offered = $derived(
    line.document
      ? common(chosen.map((block) => tasksFor(line.document!, block, (id) => line.nodeOf(id))))
      : [],
  );

  function named(block: Block): string {
    if (isStatement(block)) return nameOf(block);
    if (block.kind === "proof") {
      const of = line.document?.blocks.find((one) => one.id === block.of);
      return of && isStatement(of) ? `Proof of ${of.environment}` : "Proof";
    }
    const text = block.content.trim().split("\n")[0];
    return text ? `Prose · ${text.slice(0, 40)}` : "Prose";
  }
</script>

{#if choices.length}
  <div class="agent-pick">
    <button type="button" class="agent-pick__button" aria-haspopup="menu" aria-expanded={choosing} onclick={() => (choosing = !choosing)}>
      <span class="agent-pick__icon"><Glyph name="machine" /></span>
      <span class="agent-pick__words">
        <span class="agent-pick__agent">{chosenAgent?.agent ?? "Choose an agent"}</span>
        {#if chosenAgent}<span class="agent-pick__machine">on {chosenAgent.machine}</span>{/if}
      </span>
      <Glyph name="chevron-up-down" small />
    </button>
    <Bubble bind:open={choosing} label="Agent" menu>
      {#each choices as choice (choice.value)}
        <button
          type="button"
          class="menu__item menu__item--described"
          role="menuitemcheckbox"
          aria-checked={choice.value === line.agent}
          disabled={!!choice.reason}
          onclick={() => { line.agent = choice.value; choosing = false; }}
        >
          <span>{choice.agent}</span>
          <span class="menu__description">on {choice.machine}{choice.reason ? ` · ${choice.reason}` : ""}</span>
        </button>
      {/each}
    </Bubble>
  </div>

  {#if chosen.length}
    <div class="selection">
      <div class="selection__head">
        <span class="selection__count">{chosen.length === 1 ? "1 block selected" : `${chosen.length} blocks selected`}</span>
        <button type="button" class="button button--link" onclick={() => line.selected.clear()}>Clear</button>
      </div>
      <ul class="selection__list">
        {#each chosen.slice(0, 4) as block (block.id)}<li>{named(block)}</li>{/each}
        {#if chosen.length > 4}<li class="quiet">and {chosen.length - 4} more</li>{/if}
      </ul>
      {#if offered.length}
        <div class="selection__tasks">
          {#each offered as task, index (task.verb)}
            <button type="button" class="button button--small" class:button--primary={index === 0} onclick={() => onask(task.tasks)}>
              <Glyph name="sparkle" small />{task.label}
            </button>
          {/each}
        </div>
        {#if chosen.length > 1}<p class="selection__note">Each selected block gets its own job.</p>{/if}
      {:else}
        <p class="selection__note">No task applies to every selected block.</p>
      {/if}
    </div>
  {:else}
    <p class="panel__text">Select blocks to ask an agent about several at once, or use <Glyph name="sparkles" small /> on a block.</p>
  {/if}
{:else}
  <p class="panel__text">No machine works for this group yet.</p>
{/if}
