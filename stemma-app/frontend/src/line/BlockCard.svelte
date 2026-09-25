<script lang="ts">
  import { untrack } from "svelte";
  import Bubble from "../components/Bubble.svelte";
  import { isStatement, move, proofOf, remove, setProof, siglumPrefix } from "../lib/blocks";
  import { blockEditor, type BlockEditor } from "../lib/editor";
  import type { LineSession } from "../lib/line.svelte";
  import { tasksFor } from "../lib/tasks";
  import type { Block, Statement } from "../lib/types";
  import AddBlock from "./AddBlock.svelte";
  import Uses from "./Uses.svelte";

  let {
    line,
    block,
    at,
    number,
    proves,
    pair,
    onask,
    onmove,
  }: {
    line: LineSession;
    block: Block;
    /** Where it is, counting from zero. */
    at: number;
    /** Its number on the page, when it is a statement. */
    number: number | null;
    /** What a proof is of, as the page calls it. */
    proves: string | null;
    /** The colour it shares with its statement or its proof. */
    pair: number | undefined;
    onask: (tasks: string[]) => void;
    /** Moves a dragged block to a position, counted before the move. */
    onmove: (id: string, to: number) => void;
  } = $props();

  /** Where a block dragged over this one would land. */
  let dropping = $state<"above" | "below" | null>(null);

  const DRAGGED = "application/x-stemma-block";

  function dragStart(event: DragEvent) {
    event.dataTransfer?.setData(DRAGGED, block.id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function dragOver(event: DragEvent) {
    if (!line.editable || !event.dataTransfer?.types.includes(DRAGGED)) return;
    event.preventDefault();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    dropping = event.clientY < box.top + box.height / 2 ? "above" : "below";
  }

  function drop(event: DragEvent) {
    const dragged = event.dataTransfer?.getData(DRAGGED);
    const where = dropping;
    dropping = null;
    if (!dragged || !where || !line.editable) return;
    event.preventDefault();
    onmove(dragged, where === "above" ? at : at + 1);
  }

  const statement = $derived(isStatement(block) ? block : null);
  const node = $derived(line.nodeOf(block.kind === "proof" ? block.of : block.id));
  const faults = $derived(line.faultsOf(block.id));
  const diagnostics = $derived(line.diagnosticsOf(block.id));
  const selected = $derived(line.selected.has(block.id));
  const offered = $derived(line.document ? tasksFor(line.document, block, (id) => line.nodeOf(id)) : []);
  const verified = $derived(statement && node ? line.verified.has(node.siglum) : false);
  const count = $derived(line.document?.blocks.length ?? 0);

  let asking = $state(false);
  let siglumDraft = $state("");
  let siglumProblem = $state<string | null>(null);

  $effect(() => {
    siglumDraft = statement?.siglum ?? "";
  });

  let editor: BlockEditor | null = null;

  function mount(node: HTMLElement) {
    editor = blockEditor(node, untrack(() => block.content), {
      label: untrack(() => heading()),
      empty: untrack(() => placeholder()),
      writable: untrack(() => line.editable),
      changed: (text) => {
        block.content = text;
        line.changed({ typing: true });
      },
    });
    return () => {
      editor?.destroy();
      editor = null;
    };
  }

  $effect(() => {
    const yes = line.editable;
    editor?.writable(yes);
  });

  // Undo and redo change the content from outside the editor.
  $effect(() => {
    const content = block.content;
    if (editor && editor.text() !== content) editor.replace(content);
  });

  function heading(): string {
    switch (block.kind) {
      case "prose": return "Prose";
      case "proof": return proves ? `Proof of ${proves}` : "Proof";
      default: return `${block.environment}${number === null ? "" : ` ${number}`}`;
    }
  }

  function placeholder(): string {
    switch (block.kind) {
      case "prose": return "Intuition, intentions, how this connects to what comes before…";
      case "definition": return "What is being defined, in Typst.";
      case "theorem": return "The statement, in Typst.";
      case "proof": return "The proof. It can stay empty until somebody writes it.";
    }
  }

  function edit(change: () => void) {
    change();
    line.changed();
  }

  function setSiglum(value: string) {
    const written = value.trim();
    if (!statement) return;
    if (!written) {
      siglumProblem = null;
      edit(() => delete statement.siglum);
      return;
    }
    const kind = statement.kind === "definition" ? "def" : "(thm|lem|prop|cor)";
    if (!new RegExp(`^${kind}:[a-z0-9]+(-[a-z0-9]+)*$`).test(written)) {
      siglumProblem = `Write it as ${siglumPrefix(statement)}:name, with lowercase letters, digits and hyphens.`;
      return;
    }
    siglumProblem = null;
    edit(() => (statement.siglum = written));
  }

  function toggleProof(target: Statement, yes: boolean) {
    const proof = line.document ? proofOf(line.document, target.id) : undefined;
    if (!yes && proof?.content.trim() && !window.confirm("Delete this proof? The statement will be marked as not proved here.")) return;
    edit(() => line.document && setProof(line.document, target.id, yes));
  }

  function deleteBlock() {
    const what = block.kind === "proof"
      ? "Delete this proof? The statement will be marked as not proved here."
      : "Delete this block?";
    if (block.content.trim() && !window.confirm(what)) return;
    line.selected.delete(block.id);
    edit(() => line.document && remove(line.document, block.id));
  }

  function toggleSelected() {
    if (selected) line.selected.delete(block.id);
    else line.selected.add(block.id);
  }
</script>

<article
  class="block block--{block.kind}"
  class:block--central={statement?.central}
  class:block--selected={selected}
  class:block--faulty={faults.length > 0 || diagnostics.some((one) => one.error)}
  class:block--drop-above={dropping === "above"}
  class:block--drop-below={dropping === "below"}
  style:--pair={pair === undefined ? null : `var(--pair-${pair % 6})`}
  ondragover={dragOver}
  ondragleave={() => (dropping = null)}
  ondrop={drop}
>
  <div class="block__head">
    <span
      class="block__grip"
      class:block__grip--off={!line.editable}
      draggable={line.editable}
      role="button"
      tabindex="-1"
      title="Drag to move"
      aria-label="Drag to move"
      ondragstart={dragStart}
    >⋮⋮</span>
    <input
      type="checkbox"
      class="block__select"
      checked={selected}
      aria-label="Select for an agent"
      onchange={toggleSelected}
    />
    <span class="block__kind">{heading()}</span>

    <span class="spacer"></span>

    {#if statement}
      <label class="toggle" title="Central statements are frozen against agents">
        <input
          type="checkbox"
          checked={statement.central}
          disabled={!line.editable}
          onchange={(event) => {
            const yes = event.currentTarget.checked;
            edit(() => (statement.central = yes));
          }}
        />Central
      </label>
      <label class="toggle" title="Unmark only when there is no intention of proving it here">
        <input
          type="checkbox"
          checked={statement.proof}
          disabled={!line.editable}
          onchange={(event) => toggleProof(statement, event.currentTarget.checked)}
        />Proof
      </label>
      <Uses {line} {statement} />
    {/if}

    <span class="block__actions">
      <button type="button" class="icon" title="Move up" aria-label="Move up" disabled={!line.editable || at === 0}
        onclick={() => edit(() => line.document && move(line.document, block.id, -1))}>↑</button>
      <button type="button" class="icon" title="Move down" aria-label="Move down" disabled={!line.editable || at === count - 1}
        onclick={() => edit(() => line.document && move(line.document, block.id, 1))}>↓</button>
      <span class="ask">
        <button type="button" class="icon" title="Ask an agent" aria-label="Ask an agent" aria-expanded={asking}
          disabled={!offered.length} onclick={() => (asking = !asking)}>✦</button>
        <Bubble bind:open={asking} label="Ask an agent" align="end">
          <ul class="bubble__choices">
            {#each offered as task (task.task)}
              <li>
                <button type="button" class="bubble__choice" onclick={() => { asking = false; onask([task.task]); }}>
                  <strong>{task.label}</strong>
                </button>
              </li>
            {/each}
          </ul>
        </Bubble>
      </span>
      <button type="button" class="icon" title="Delete" aria-label="Delete" disabled={!line.editable} onclick={deleteBlock}>×</button>
    </span>
  </div>

  {#if statement}
    <div class="input block__fields">
      <input
        class="input block__field block__field--environment"
        list={statement.kind === "definition" ? "definition-environments" : "theorem-environments"}
        value={statement.environment}
        disabled={!line.editable}
        aria-label="Environment"
        onchange={(event) => {
          const value = event.currentTarget.value.trim();
          if (value) edit(() => (statement.environment = value));
        }}
      />
      <input
        class="input block__field"
        placeholder="Title"
        value={statement.title ?? ""}
        disabled={!line.editable}
        aria-label="Title"
        onchange={(event) => {
          const value = event.currentTarget.value.trim();
          edit(() => (value ? (statement.title = value) : delete statement.title));
        }}
      />
      <input
        class="input block__field block__field--siglum"
        placeholder={`${siglumPrefix(statement)}:name`}
        bind:value={siglumDraft}
        disabled={!line.editable}
        aria-label="Siglum"
        aria-invalid={siglumProblem !== null}
        onchange={(event) => setSiglum(event.currentTarget.value)}
      />
    </div>
  {/if}

  {#if statement || block.kind === "proof"}
    <div class="block__state">
      {#if statement?.central}<span class="state state--central">Central</span>{/if}
      {#if statement && !statement.proof}<span class="state">Not proved here</span>{/if}
      {#if statement?.proof}
        <span class="state">{node?.proved ? "Proof written" : "Proof to write"}</span>
      {/if}
      {#if statement}
        <span class="state" class:state--verified={verified}>{verified ? "Lean verified" : "Not formalized"}</span>
      {/if}
      {#if siglumProblem}<span class="state state--problem">{siglumProblem}</span>{/if}
    </div>
  {/if}

  <div class="block__content" {@attach mount}></div>

  {#if faults.length || diagnostics.length}
    <ul class="block__problems">
      {#each faults as fault, index (index)}<li>{fault}</li>{/each}
      {#each diagnostics as diagnostic, index (index)}
        <li class:warning={!diagnostic.error}>
          {diagnostic.line === null ? "" : `Line ${diagnostic.line}: `}{diagnostic.message}
        </li>
      {/each}
    </ul>
  {/if}

  <div class="block__insert">
    <AddBlock {line} at={at + 1} label="Add block below" compact />
  </div>
</article>
