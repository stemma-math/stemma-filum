<script lang="ts">
  import Bubble from "../components/Bubble.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { cited, isStatement, nameOf } from "../lib/blocks";
  import type { LineSession } from "../lib/line.svelte";
  import type { Statement } from "../lib/types";

  let { line, statement }: { line: LineSession; statement: Statement } = $props();

  let open = $state(false);
  let filter = $state("");

  const others = $derived(
    (line.document?.blocks ?? []).filter((block): block is Statement => isStatement(block) && block.id !== statement.id),
  );
  const shown = $derived(
    others.filter((other) => nameOf(other).toLowerCase().includes(filter.trim().toLowerCase())),
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

<span class="uses">
  <button type="button" class="button button--quiet button--small uses__button" aria-haspopup="dialog" aria-expanded={open}
    data-tip="What this statement uses" onclick={() => (open = !open)}>
    <Glyph name="link" small />Uses{count ? ` ${count}` : ""}
  </button>
  <Bubble bind:open label="What this statement uses" align="end">
    <div class="uses__pop">
      <p class="uses__title">What this statement uses</p>
      {#if others.length}
        {#if others.length > 6}
          <input class="input uses__filter" type="search" placeholder="Filter" aria-label="Filter statements" bind:value={filter} />
        {/if}
        <ul class="uses__list">
          {#each shown as other (other.id)}
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
                {#if isCited}<span class="chip">Cited in the text</span>{/if}
              </label>
            </li>
          {/each}
        </ul>
        <p class="uses__note">Citing a statement with <code>@siglum</code> in the text or the proof adds it here.</p>
      {:else}
        <p class="uses__note">There are no other definitions or theorems in this line yet.</p>
      {/if}
    </div>
  </Bubble>
</span>
