<script lang="ts">
  import Message from "../components/Message.svelte";
  import { flip } from "svelte/animate";
  import { dropIndex, isStatement, move, placeAt, remove } from "../lib/blocks";
  import { app } from "../lib/app.svelte";
  import { blockCard, blockText } from "../lib/dom";
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

  /** The block being dragged, and where it would land as its position
   * among the others. The list shows it there, faded, while a copy of it
   * follows the pointer. */
  let drag = $state<{ id: string; to: number } | null>(null);

  /** The blocks in the order shown: the line's, or the one a drop would
   * leave. */
  const shown = $derived.by(() => {
    const dragged = drag && document.blocks.find((block) => block.id === drag!.id);
    if (!drag || !dragged) return document.blocks;
    const others = document.blocks.filter((block) => block !== dragged);
    return [...others.slice(0, drag.to), dragged, ...others.slice(drag.to)];
  });

  let list = $state<HTMLElement>();

  /** How far the pointer goes before a press on a grip becomes a drag. */
  const THRESHOLD = 4;
  /** How close to the top or bottom of the view the pointer scrolls it. */
  const EDGE = 56;

  /** A press on a grip, while it lasts. What is measured when the drag
   * begins stays fixed until it ends, so the landing place depends on the
   * pointer alone. */
  let held: {
    id: string;
    pointer: number;
    /** Where the pointer was pressed, and where it is now. */
    from: number;
    y: number;
    /** From the top of the block to the pointer. */
    grab: number;
    height: number;
    /** The other blocks' heights, in order, and the space between two. */
    heights: number[];
    gap: number;
    /** From the top of the list to the first block. */
    first: number;
    ghost: HTMLElement | null;
    frame: number;
  } | null = null;

  function grab(id: string, event: PointerEvent) {
    if (!line.editable || !list || event.button !== 0 || held) return;
    const slot = slotOf(id);
    if (!slot) return;
    // A drawn drag rather than the browser's: the browser's offers the block
    // to every field it passes over, which shows the pointer for a copy.
    event.preventDefault();
    list.setPointerCapture(event.pointerId);
    const grab = event.clientY - slot.getBoundingClientRect().top;
    held = { id, pointer: event.pointerId, from: event.clientY, y: event.clientY, grab, height: 0, heights: [], gap: 0, first: 0, ghost: null, frame: 0 };
  }

  function slotOf(id: string) {
    return list?.querySelector<HTMLElement>(`:scope > [data-slot="${id}"]`) ?? null;
  }

  function begin() {
    const card = held && blockCard(held.id);
    const slots = [...(list?.querySelectorAll<HTMLElement>(":scope > .blocks__slot") ?? [])];
    if (!held || !list || !card || !slots.length) return;
    const { id } = held;
    const top = list.getBoundingClientRect().top;
    held.first = slots[0].getBoundingClientRect().top - top;
    held.heights = slots.filter((slot) => slot.dataset.slot !== id).map((slot) => slot.getBoundingClientRect().height);
    held.height = slotOf(id)?.getBoundingClientRect().height ?? 0;
    held.gap = parseFloat(getComputedStyle(list).rowGap) || 0;

    const box = card.getBoundingClientRect();
    const ghost = card.cloneNode(true) as HTMLElement;
    ghost.classList.add("block--ghost");
    ghost.removeAttribute("data-block");
    ghost.setAttribute("aria-hidden", "true");
    ghost.inert = true;
    Object.assign(ghost.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px` });
    window.document.body.append(ghost);
    held.ghost = ghost;

    window.document.documentElement.classList.add("dragging-block");
    drag = { id, to: document.blocks.findIndex((block) => block.id === id) };
    held.frame = requestAnimationFrame(follow);
  }

  /** Once a frame while dragging: scrolls near the edges, moves the copy
   * with the pointer and makes room where it would land. */
  function follow() {
    if (!held || !drag || !list) return;
    const view = list.closest<HTMLElement>(".writing");
    if (view) {
      const box = view.getBoundingClientRect();
      const past = held.y < box.top + EDGE ? held.y - box.top - EDGE : held.y > box.bottom - EDGE ? held.y - box.bottom + EDGE : 0;
      if (past) view.scrollTop += Math.max(-1, Math.min(1, past / EDGE)) * 14;
    }
    if (held.ghost) held.ghost.style.translate = `0 ${held.y - held.from}px`;
    const centre = held.y - held.grab + held.height / 2 - (list.getBoundingClientRect().top + held.first);
    const to = dropIndex(held.heights, held.gap, centre);
    if (to !== drag.to) drag = { id: drag.id, to };
    held.frame = requestAnimationFrame(follow);
  }

  function pointerMove(event: PointerEvent) {
    if (!held || event.pointerId !== held.pointer) return;
    held.y = event.clientY;
    if (!drag && Math.abs(held.y - held.from) > THRESHOLD) begin();
  }

  /** Ends the press: a drag puts the block where it was shown, unless it
   * was cancelled. */
  function finish(dropping: boolean) {
    if (!held) return;
    const { id, frame, ghost } = held;
    held = null;
    cancelAnimationFrame(frame);
    ghost?.remove();
    window.document.documentElement.classList.remove("dragging-block");
    const to = drag?.to;
    drag = null;
    if (dropping && to !== undefined && to !== document.blocks.findIndex((block) => block.id === id)) {
      edit(() => placeAt(document, id, to));
    }
  }

  /** The keyboard over the blocks, while no field has it. */
  function keys(event: KeyboardEvent) {
    if (event.key === "Escape" && held) {
      event.preventDefault();
      finish(false);
      return;
    }
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
        blockCard(next.id)?.scrollIntoView({ block: "nearest" });
      }
      return;
    }
    if (event.key === "Enter" && current) {
      event.preventDefault();
      blockText(current)?.focus();
      return;
    }
    if (event.key === "Backspace" && (event.metaKey || event.ctrlKey) && current) {
      event.preventDefault();
      const block = blocks.find((one) => one.id === current);
      if (block) askToDelete(block);
    }
  }
</script>

<svelte:window
  onkeydown={keys}
  onpointermove={pointerMove}
  onpointerup={(event) => { if (event.pointerId === held?.pointer) finish(true); }}
  onpointercancel={(event) => { if (event.pointerId === held?.pointer) finish(false); }}
  onblur={() => finish(false)}
/>

<div class="blocks" bind:this={list}>
  {#if line.general.length}
    <div class="blocks__problems">
      {#each line.general as problem, index (index)}<Message kind="error" text={problem} />{/each}
    </div>
  {/if}

  {#if document.blocks.length === 0}
    <div class="blocks__empty">
      <p>This line has no blocks yet.</p>
      <p class="quiet">Use Add block below to write the first one.</p>
    </div>
  {:else}
    {#each shown as block (block.id)}
      {@const partner = partners.get(block.id)}
      <div
        class="blocks__slot"
        class:blocks__slot--proposed={app.proposalBlocks.includes(block.id)}
        class:blocks__slot--landing={drag?.id === block.id}
        data-slot={block.id}
        animate:flip={{ duration: 180 }}
      >
        <BlockCard
          {line}
          {block}
          at={document.blocks.indexOf(block)}
          number={numbers.get(block.id) ?? null}
          proves={block.kind === "proof" ? called(block.of) : null}
          partnered={!!partner && (line.selected.has(partner) || focused === partner)}
          anySelected={line.selected.size > 0}
          {onask}
          onselect={(adding) => select(block.id, adding)}
          onfocus={(yes) => (focused = yes ? block.id : focused === block.id ? null : focused)}
          ondelete={(which) => askToDelete(which ?? block)}
          ongrab={(event) => grab(block.id, event)}
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
