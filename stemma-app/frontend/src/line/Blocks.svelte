<script lang="ts">
  import { DEFINITION_ENVIRONMENTS, THEOREM_ENVIRONMENTS, isStatement, pairs } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";
  import type { LineDocument } from "../lib/types";
  import AddBlock from "./AddBlock.svelte";
  import BlockCard from "./BlockCard.svelte";

  let { line, document, onask }: { line: LineSession; document: LineDocument; onask: (tasks: string[]) => void } = $props();

  const colours = $derived(pairs(document));

  /** Every statement's number on the page, which counts definitions and
   * theorems together as the Typst package does. */
  const numbers = $derived.by(() => {
    const numbered = new Map<string, number>();
    for (const block of document.blocks) if (isStatement(block)) numbered.set(block.id, numbered.size + 1);
    return numbered;
  });

  function called(id: string): string | null {
    const statement = document.blocks.find((block) => block.id === id);
    return statement && isStatement(statement) ? `${statement.environment} ${numbers.get(id)}` : null;
  }

  function edit(change: () => void) {
    change();
    line.changed();
  }
</script>

<datalist id="theorem-environments">
  {#each THEOREM_ENVIRONMENTS as environment (environment)}<option value={environment}></option>{/each}
</datalist>
<datalist id="definition-environments">
  {#each DEFINITION_ENVIRONMENTS as environment (environment)}<option value={environment}></option>{/each}
</datalist>

<div class="blocks">
  <div class="blocks__head">
    <input
      class="blocks__title"
      value={document.title}
      placeholder="Title"
      aria-label="Title"
      disabled={!line.editable}
      onchange={(event) => {
        const value = event.currentTarget.value.trim();
        if (value) edit(() => (document.title = value));
      }}
    />
    <textarea
      class="blocks__description"
      rows="2"
      placeholder="Description (optional)"
      aria-label="Description"
      disabled={!line.editable}
      value={document.description ?? ""}
      onchange={(event) => {
        const value = event.currentTarget.value.trim();
        edit(() => (value ? (document.description = value) : delete document.description));
      }}
    ></textarea>
  </div>

  {#if line.general.length}
    <ul class="blocks__problems">
      {#each line.general as problem, index (index)}<li>{problem}</li>{/each}
    </ul>
  {/if}

  {#if document.blocks.length === 0}
    <div class="blocks__empty">
      <p class="quiet">This line has no blocks yet.</p>
      <AddBlock {line} at={0} />
    </div>
  {:else}
    <div class="block__insert block__insert--first">
      <AddBlock {line} at={0} label="Add block above" compact />
    </div>
    {#each document.blocks as block, at (block.id)}
      <BlockCard
        {line}
        {block}
        {at}
        number={numbers.get(block.id) ?? null}
        proves={block.kind === "proof" ? called(block.of) : null}
        pair={colours.get(block.id)}
        {onask}
      />
    {/each}
    <div class="blocks__end">
      <AddBlock {line} at={document.blocks.length} />
      <button type="button" class="quiet-button" disabled={!line.offers.length} onclick={() => onask(["explore"])}>
        ✦ Explore next steps
      </button>
    </div>
  {/if}
</div>
