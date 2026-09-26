<script lang="ts">
  import { flip } from "svelte/animate";
  import { isStatement, move, moveTo, remove } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";
  import type { Block, LineDocument } from "../lib/types";
  import BlockCard from "./BlockCard.svelte";
  import Confirm from "./Confirm.svelte";

  let { line, document, onask }: { line: LineSession; document: LineDocument; onask: (tasks: string[]) => void } = $props();

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

  /** A statement and its proof belong together; each points at the other. */
  const partners = $derived.by(() => {
    const paired = new Map<string, string>();
    for (const block of document.blocks) {
      if (block.kind === "proof") {
        paired.set(block.id, block.of);
        paired.set(block.of, block.id);
      }
    }
    return paired;
  });

  /** The block whose text or fields have the keyboard. */
  let focused = $state<string | null>(null);

  function edit(change: () => void) {
    change();
    line.changed();
  }

  /** Selects a block: alone, or added to the others with ⇧ or ⌘. */
  function select(id: string, adding: boolean) {
    if (adding) {
      if (line.selected.has(id)) line.selected.delete(id);
      else line.selected.add(id);
    } else if (!(line.selected.size === 1 && line.selected.has(id))) {
      line.selected.clear();
      line.selected.add(id);
    }
  }

  const only = () => (line.selected.size === 1 ? [...line.selected][0] : null);

  let deleting = $state<Block | null>(null);

  /** Deletes a block, asking first when it has something written in it. */
  function askToDelete(block: Block) {
    if (!line.editable) return;
    if (block.content.trim()) deleting = block;
    else deleteNow(block);
  }

  function deleteNow(block: Block) {
    line.selected.delete(block.id);
    edit(() => remove(document, block.id));
  }

  function editing(target: EventTarget | null) {
    return (target as HTMLElement | null)?.closest("input, textarea, select, [contenteditable=true], .bubble, dialog");
  }

  /** The keyboard over the blocks, while no field has it. */
  function keys(event: KeyboardEvent) {
    if (editing(event.target) || event.defaultPrevented) return;
    const blocks = document.blocks;
    const current = only();
    const at = current ? blocks.findIndex((block) => block.id === current) : -1;

    if (event.key === "Escape" && line.selected.size) {
      line.selected.clear();
      return;
    }
    if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !event.metaKey && !event.ctrlKey) {
      const by = event.key === "ArrowUp" ? -1 : 1;
      if (event.altKey) {
        if (current && line.editable) {
          event.preventDefault();
          edit(() => move(document, current, by));
        }
        return;
      }
      event.preventDefault();
      const to = at < 0 ? (by > 0 ? 0 : blocks.length - 1) : Math.min(Math.max(at + by, 0), blocks.length - 1);
      const next = blocks[to];
      if (next) {
        select(next.id, false);
        window.document.querySelector(`[data-block="${next.id}"]`)?.scrollIntoView({ block: "nearest" });
      }
      return;
    }
    if (event.key === "Enter" && current) {
      event.preventDefault();
      (window.document.querySelector(`[data-block="${current}"] .cm-content`) as HTMLElement | null)?.focus();
      return;
    }
    if (event.key === "Backspace" && (event.metaKey || event.ctrlKey) && current) {
      event.preventDefault();
      const block = blocks.find((one) => one.id === current);
      if (block) askToDelete(block);
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="blocks">
  {#if line.general.length}
    <ul class="blocks__problems">
      {#each line.general as problem, index (index)}<li>{problem}</li>{/each}
    </ul>
  {/if}

  {#if document.blocks.length === 0}
    <div class="blocks__empty">
      <p>This line has no blocks yet.</p>
      <p class="quiet">Use Add block below to write the first one.</p>
    </div>
  {:else}
    {#each document.blocks as block, at (block.id)}
      {@const partner = partners.get(block.id)}
      <div class="blocks__slot" animate:flip={{ duration: 180 }}>
        <BlockCard
          {line}
          {block}
          {at}
          number={numbers.get(block.id) ?? null}
          proves={block.kind === "proof" ? called(block.of) : null}
          partnered={!!partner && (line.selected.has(partner) || focused === partner)}
          anySelected={line.selected.size > 0}
          {onask}
          onselect={(adding) => select(block.id, adding)}
          onfocus={(yes) => (focused = yes ? block.id : focused === block.id ? null : focused)}
          ondelete={(which) => askToDelete(which ?? block)}
          onmove={(id, to) => edit(() => moveTo(document, id, to))}
        />
      </div>
    {/each}
  {/if}
</div>

<Confirm
  open={deleting !== null}
  title={deleting?.kind === "proof" ? "Delete this proof?" : "Delete this block?"}
  text={deleting?.kind === "proof"
    ? "The statement will be marked as not proved here."
    : deleting && isStatement(deleting) && deleting.proof
      ? "Its proof is deleted with it."
      : "What is written in it is deleted. Undo brings it back."}
  verb={deleting?.kind === "proof" ? "Delete proof" : "Delete block"}
  onconfirm={() => { if (deleting) deleteNow(deleting); deleting = null; }}
  oncancel={() => (deleting = null)}
/>
