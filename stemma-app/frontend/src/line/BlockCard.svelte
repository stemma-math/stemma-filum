<script lang="ts">
  import Message from "../components/Message.svelte";
  import { untrack } from "svelte";
  import Bubble from "../components/Bubble.svelte";
  import Glyph from "../components/Glyph.svelte";
  import { DEFINITION_ENVIRONMENTS, THEOREM_ENVIRONMENTS, isStatement, move, proofOf, setProof, siglumPrefix } from "../lib/blocks";
  import { blurOnEnter } from "../lib/dom";
  import { blockEditor, type BlockEditor } from "../lib/editor";
  import type { LineSession } from "../lib/line.svelte";
  import { tasksFor } from "../lib/tasks";
  import type { Block, Statement } from "../lib/types";
  import Uses from "./Uses.svelte";

  let {
    line,
    block,
    at,
    number,
    proves,
    partnered,
    anySelected,
    onask,
    onselect,
    onfocus,
    ondelete,
    ongrab,
  }: {
    line: LineSession;
    block: Block;
    /** Where it is, counting from zero. */
    at: number;
    /** Its number on the page, when it is a statement. */
    number: number | null;
    /** What a proof is of, as the page calls it. */
    proves: string | null;
    /** Whether the statement or proof it belongs with is selected or being edited. */
    partnered: boolean;
    /** Whether any block is selected, which shows every selection box. */
    anySelected: boolean;
    onask: (tasks: string[]) => void;
    onselect: (adding: boolean) => void;
    onfocus: (yes: boolean) => void;
    /** Deletes this block, or another one it stands for, asking first when
     * something is written in it. */
    ondelete: (which?: Block) => void;
    /** The grip was pressed: the block may be about to be dragged. */
    ongrab: (event: PointerEvent) => void;
  } = $props();

  const statement = $derived(isStatement(block) ? block : null);
  const node = $derived(line.nodeOf(block.kind === "proof" ? block.of : block.id));
  const faults = $derived(line.faultsOf(block.id));
  const diagnostics = $derived(line.diagnosticsOf(block.id));
  const selected = $derived(line.selected.has(block.id));
  const offered = $derived(line.document ? tasksFor(line.document, block, (id) => line.nodeOf(id)) : []);
  const verified = $derived(statement ? line.verified.has(block.id) : false);
  const formal = $derived(statement ? line.formalOf(block.id) : undefined);
  const backed = $derived(
    formal?.state === "verified" &&
      formal.declarations.length > 0 &&
      formal.declarations.every((declaration) => declaration.sorry_free),
  );
  const count = $derived(line.document?.blocks.length ?? 0);

  let asking = $state(false);
  let acting = $state(false);
  let typing = $state(false);
  let naming = $state(false);
  let siglumDraft = $state("");
  let siglumProblem = $state<string | null>(null);
  let titleDraft = $state("");

  $effect(() => {
    siglumDraft = statement?.siglum ?? "";
    titleDraft = statement?.title ?? "";
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

  /** Checks the siglum when the field is left, against its prefix and
   * every other statement in the line. */
  function setSiglum(value: string) {
    const written = value.trim();
    if (!statement) return;
    if (!written) {
      siglumProblem = null;
      edit(() => delete statement.siglum);
      return;
    }
    const prefix = siglumPrefix(statement);
    const kind = statement.kind === "definition" ? "def" : "(thm|lem|prop|cor)";
    if (!new RegExp(`^${kind}:[a-z0-9]+(-[a-z0-9]+)*$`).test(written)) {
      siglumProblem = `Use ${prefix}:name — lowercase letters, digits and hyphens.`;
      return;
    }
    const taken = line.document?.blocks.some((other) => other.id !== statement.id && isStatement(other) && other.siglum === written);
    if (taken) {
      siglumProblem = `${written} is already used in this line.`;
      return;
    }
    siglumProblem = null;
    edit(() => (statement.siglum = written));
  }

  function setTitle(value: string) {
    if (!statement) return;
    const title = value.trim();
    if ((statement.title ?? "") === title) return;
    edit(() => (title ? (statement.title = title) : delete statement.title));
  }

  function setEnvironment(value: string) {
    naming = false;
    typing = false;
    const environment = value.trim();
    if (!statement || !environment || environment === statement.environment) return;
    edit(() => (statement.environment = environment));
  }

  function toggleProof(target: Statement) {
    acting = false;
    const proof = line.document ? proofOf(line.document, target.id) : undefined;
    if (target.proof && proof?.content.trim()) {
      ondelete(proof);
      return;
    }
    edit(() => line.document && setProof(line.document, target.id, !target.proof));
  }

  function moveBy(by: -1 | 1) {
    acting = false;
    edit(() => line.document && move(line.document, block.id, by));
  }

  /** A click on the card selects it; the controls on it keep their own
   * clicks. */
  function pointer(event: PointerEvent) {
    const target = event.target as HTMLElement;
    if (target.closest("button, .bubble, .block__select, .block__grip, a")) return;
    onselect(event.shiftKey || event.metaKey || event.ctrlKey);
  }

  const environments = $derived(statement?.kind === "definition" ? DEFINITION_ENVIRONMENTS : THEOREM_ENVIRONMENTS);
  const siglumError = $derived(`siglum-${block.id}`);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<article
  class="block block--{block.kind}"
  class:block--selected={selected}
  class:block--partnered={partnered && !selected}
  class:block--selecting={anySelected}
  class:block--faulty={faults.length > 0 || diagnostics.some((one) => one.error)}
  class:block--readonly={!line.editable}
  data-block={block.id}
  aria-label={heading()}
  onpointerdown={pointer}
  onfocusin={() => onfocus(true)}
  onfocusout={(event) => { if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node)) onfocus(false); }}
>
  <input
    type="checkbox"
    class="block__select"
    checked={selected}
    aria-label="Select for an agent"
    onchange={() => onselect(true)}
  />

  <div class="block__head">
    <span class="block__lead">
      {#if line.editable}
        <span
          class="block__grip"
          role="button"
          tabindex="-1"
          aria-label="Drag to move"
          data-tip="Drag to move"
          onpointerdown={ongrab}
        ><Glyph name="grip" /></span>
      {/if}
    </span>

    {#if statement}
      <span class="block__type">
        {#if naming}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="block__label block__label--{statement.kind} block__label-input"
            value={statement.environment}
            aria-label="Environment"
            autofocus
            onchange={(event) => setEnvironment(event.currentTarget.value)}
            onblur={(event) => setEnvironment(event.currentTarget.value)}
            onkeydown={(event) => { if (event.key === "Escape") naming = false; blurOnEnter(event); }}
          />
        {:else}
          <button
            type="button"
            class="block__label block__label--{statement.kind}"
            aria-haspopup="menu"
            aria-expanded={typing}
            disabled={!line.editable}
            data-tip={line.editable ? "Change type" : null}
            onclick={() => (typing = !typing)}
          >{heading()}</button>
        {/if}
        <Bubble bind:open={typing} label="Change type" menu>
          <p class="menu__heading">Change type</p>
          {#each environments as environment (environment)}
            <button type="button" class="menu__item" role="menuitemcheckbox" aria-checked={environment === statement.environment}
              onclick={() => setEnvironment(environment)}>
              {environment}
              {#if environment === statement.environment}<span class="menu__hint"><Glyph name="check" small /></span>{/if}
            </button>
          {/each}
          <hr class="menu__separator" />
          <button type="button" class="menu__item" role="menuitem" onclick={() => { typing = false; naming = true; }}>Another name</button>
        </Bubble>
      </span>

      <input
        class="block__title"
        placeholder={line.editable ? "Untitled" : ""}
        bind:value={titleDraft}
        disabled={!line.editable}
        aria-label="Title"
        onchange={(event) => setTitle(event.currentTarget.value)}
        onkeydown={blurOnEnter}
      />

      <input
        class="block__siglum"
        placeholder={`${siglumPrefix(statement)}:name`}
        bind:value={siglumDraft}
        disabled={!line.editable}
        aria-label="Identifier"
        aria-invalid={siglumProblem !== null}
        aria-describedby={siglumProblem ? siglumError : undefined}
        onchange={(event) => setSiglum(event.currentTarget.value)}
        onkeydown={blurOnEnter}
      />

      <Uses {line} {statement} />
    {:else}
      <span class="block__label block__label--{block.kind}">{heading()}</span>
      <span class="spacer"></span>
    {/if}

    <span class="block__actions">
      <span class="block__menu">
        <button type="button" class="button button--icon button--small" aria-label="Ask an agent about this block"
          data-tip="Ask an agent" aria-haspopup="menu" aria-expanded={asking}
          disabled={!offered.length || line.launching} onclick={() => (asking = !asking)}><Glyph name="sparkles" /></button>
        <Bubble bind:open={asking} label="Ask an agent" align="end" menu>
          <p class="menu__heading">Ask an agent to</p>
          {#each offered as task (task.task)}
            <button type="button" class="menu__item" role="menuitem" onclick={() => { asking = false; onask([task.task]); }}>
              <Glyph name="sparkle" />{task.label}
            </button>
          {/each}
        </Bubble>
      </span>

      {#if line.editable}
        <span class="block__menu">
          <button type="button" class="button button--icon button--small" aria-label="More actions" data-tip="More actions"
            aria-haspopup="menu" aria-expanded={acting} onclick={() => (acting = !acting)}><Glyph name="more" /></button>
          <Bubble bind:open={acting} label="Block actions" align="end" menu>
            {#if statement}
              <button type="button" class="menu__item" role="menuitem" onclick={() => toggleProof(statement)}>
                {statement.proof ? "Remove proof" : "Add proof"}
              </button>
              <hr class="menu__separator" />
            {/if}
            <button type="button" class="menu__item" role="menuitem" disabled={at === 0} onclick={() => moveBy(-1)}>
              Move up<span class="menu__hint">⌥↑</span>
            </button>
            <button type="button" class="menu__item" role="menuitem" disabled={at === count - 1} onclick={() => moveBy(1)}>
              Move down<span class="menu__hint">⌥↓</span>
            </button>
            <hr class="menu__separator" />
            <button type="button" class="menu__item menu__item--danger" role="menuitem" onclick={() => { acting = false; ondelete(); }}>
              {block.kind === "proof" ? "Delete proof" : "Delete"}<span class="menu__hint">⌘⌫</span>
            </button>
          </Bubble>
        </span>
      {/if}
    </span>
  </div>

  {#if siglumProblem}
    <p class="block__error" id={siglumError}><Glyph name="alert" small />{siglumProblem}</p>
  {/if}

  {#if statement}
    <div class="block__state">
      <button
        type="button"
        class="chip block__central"
        class:chip--accent={statement.central}
        class:chip--dashed={!statement.central}
        aria-pressed={statement.central}
        disabled={!line.editable}
        data-tip="Central statements are frozen against agents"
        onclick={() => edit(() => (statement.central = !statement.central))}
      ><Glyph name="star" small />Central</button>
      {#if !statement.proof}
        <span class="chip chip--dashed">Not proved here</span>
      {:else if node?.proved}
        <span class="chip chip--success"><Glyph name="check" small />Proof written</span>
      {:else}
        <span class="chip">Proof to write</span>
      {/if}
      {#if backed}
        <span class="chip chip--success"><Glyph name="check" small />Backed in Lean</span>
      {:else if verified}
        <span class="chip"><Glyph name="warning" small />Verified with missing backing</span>
      {:else}
        <span class="chip chip--dashed">Missing formal statement</span>
      {/if}
    </div>
  {/if}

  <div class="block__content" class:block__content--proof={block.kind === "proof"} {@attach mount}></div>

  {#if statement && formal}
    <details class="disclosure block__formal">
      <summary>Formal statement · {formal.state}</summary>
      {#each formal.declarations as declaration (declaration.name)}
        <p class="identifier">{declaration.name} · {declaration.module}</p>
        <pre>{declaration.statement}</pre>
        {#if declaration.reaches.length}<p class="quiet">Reaches {declaration.reaches.join(", ")}</p>{/if}
      {:else}
        <p class="quiet">{formal.diagnostic || "No marked Lean declaration reaches this block."}</p>
      {/each}
    </details>
  {/if}

  {#if faults.length || diagnostics.length}
    <div class="block__problems">
      {#each faults as fault, index (index)}<Message kind="error" text={fault} />{/each}
      {#each diagnostics as diagnostic, index (index)}
        <Message kind={diagnostic.error ? "error" : "warning"}
          text={`${diagnostic.line === null ? "" : `Line ${diagnostic.line}: `}${diagnostic.message}`} />
      {/each}
    </div>
  {/if}
</article>
