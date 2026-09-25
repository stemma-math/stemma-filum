<script lang="ts">
  import Bubble from "../components/Bubble.svelte";
  import { cited, isStatement, nameOf } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";
  import type { Statement } from "../lib/types";

  let { line, statement }: { line: LineSession; statement: Statement } = $props();

  let open = $state(false);

  const others = $derived(
    (line.document?.blocks ?? []).filter((block): block is Statement => isStatement(block) && block.id !== statement.id),
  );
  const citing = $derived(line.document ? cited(line.document, statement.id) : []);
  const declared = $derived(statement.uses ?? []);
  const count = $derived(new Set([...declared, ...citing]).size);

  function toggle(id: string, yes: boolean) {
    const uses = (statement.uses ?? []).filter((used) => used !== id);
    statement.uses = yes ? [...uses, id] : uses;
    line.changed();
  }
</script>

<div class="uses">
  <button type="button" class="chip" aria-expanded={open} onclick={() => (open = !open)}>
    Uses{count ? ` · ${count}` : ""}
  </button>
  <Bubble bind:open label="Dependencies" align="end">
    {#if others.length}
      <p class="bubble__title">What this statement uses</p>
      <ul class="bubble__checks">
        {#each others as other (other.id)}
          {@const isCited = citing.includes(other.id)}
          <li>
            <label class="choice">
              <input
                type="checkbox"
                checked={declared.includes(other.id) || isCited}
                disabled={!line.editable || isCited}
                onchange={(event) => toggle(other.id, event.currentTarget.checked)}
              />
              <span>{nameOf(other)}</span>
              {#if isCited}<span class="quiet">cited in the text</span>{/if}
            </label>
          </li>
        {/each}
      </ul>
      <p class="quiet">Citing a statement with <code>@siglum</code> in the text or the proof adds it here.</p>
    {:else}
      <p class="quiet">There are no other definitions or theorems in this line yet.</p>
    {/if}
  </Bubble>
</div>
