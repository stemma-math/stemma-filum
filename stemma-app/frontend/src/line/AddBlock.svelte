<script lang="ts">
  import Bubble from "../components/Bubble.svelte";
  import { ADDABLE, insert } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";

  let {
    line,
    at,
    label = "Add block",
    compact = false,
  }: { line: LineSession; at: number; label?: string; compact?: boolean } = $props();

  let open = $state(false);

  function add(kind: (typeof ADDABLE)[number]["kind"]) {
    if (!line.document) return;
    insert(line.document, at, kind);
    open = false;
    line.changed();
  }
</script>

<div class="add-block" class:add-block--compact={compact}>
  <button
    type="button"
    class={compact ? "add-block__between" : "add-block__button"}
    disabled={!line.editable}
    aria-expanded={open}
    title={label}
    onclick={() => (open = !open)}
  >
    {#if compact}<span aria-hidden="true">+</span><span class="visually-hidden">{label}</span>{:else}+ {label}{/if}
  </button>
  <Bubble bind:open label="Block type">
    <ul class="bubble__choices">
      {#each ADDABLE as choice (choice.kind)}
        <li>
          <button type="button" class="bubble__choice" onclick={() => add(choice.kind)}>
            <strong>{choice.label}</strong>
            <span>{choice.hint}</span>
          </button>
        </li>
      {/each}
    </ul>
  </Bubble>
</div>
