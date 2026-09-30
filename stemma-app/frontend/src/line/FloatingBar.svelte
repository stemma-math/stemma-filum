<script lang="ts">
  // Always at the foot of the blocks: adding one, and asking where the line
  // could go next.
  import Bubble from "../components/Bubble.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { ADDABLE, insert, isStatement } from "../lib/blocks";
  import { blockCard, blockText } from "../lib/dom";
  import type { LineSession } from "../lib/line.svelte";
  import type { LineDocument } from "../lib/types";

  let { line, document, onask }: { line: LineSession; document: LineDocument; onask: (tasks: string[]) => void } = $props();

  let adding = $state(false);

  /** A new block goes after the last one selected, and after that one's
   * proof when it follows it; with nothing selected, at the end. */
  function position(): number {
    const blocks = document.blocks;
    let at = -1;
    blocks.forEach((block, index) => { if (line.selected.has(block.id)) at = index; });
    if (at < 0) return blocks.length;
    const chosen = blocks[at];
    const next = blocks[at + 1];
    if (isStatement(chosen) && next?.kind === "proof" && next.of === chosen.id) return at + 2;
    return at + 1;
  }

  function add(kind: (typeof ADDABLE)[number]["kind"]) {
    adding = false;
    const block = insert(document, position(), kind);
    line.selected.clear();
    line.selected.add(block.id);
    line.changed();
    requestAnimationFrame(() => {
      blockCard(block.id)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      blockText(block.id)?.focus();
    });
  }
</script>

<div class="floating-bar">
  <div class="floating-bar__inner">
    <span class="floating-bar__add">
      <button type="button" class="floating-bar__button" aria-haspopup="menu" aria-expanded={adding}
        disabled={!line.editable} onclick={() => (adding = !adding)}>
        <Glyph name="add" small />Add block<Glyph name="chevron-down" small />
      </button>
      <Bubble bind:open={adding} label="Block type" place="above" menu>
        {#each ADDABLE as choice (choice.kind)}
          <button type="button" class="menu__item menu__item--described" role="menuitem" onclick={() => add(choice.kind)}>
            <span class="block__label block__label--{choice.kind}">{choice.label}</span>
            <span class="menu__description">{choice.hint}</span>
          </button>
        {/each}
      </Bubble>
    </span>
    <span class="floating-bar__divider" aria-hidden="true"></span>
    <button type="button" class="floating-bar__button floating-bar__button--accent" disabled={!line.offers.length || line.launching}
      data-tip={line.offers.length ? "Ask the chosen agent about the whole line" : "No machine works for this group yet"}
      onclick={() => onask(["explore"])}>
      <Glyph name="sparkle" small />Explore next steps
    </button>
  </div>
</div>
