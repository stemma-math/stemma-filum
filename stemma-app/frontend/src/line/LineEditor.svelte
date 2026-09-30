<script lang="ts">
  import Message from "../components/Message.svelte";
  import { onMount } from "svelte";
  import Glyph from "../components/Glyph.svelte";
  import { app, type View } from "../lib/app.svelte";
  import { blurOnEnter } from "../lib/dom";
  import { jobs } from "../lib/jobs.svelte";
  import type { LineSession } from "../lib/line.svelte";
  import { fail } from "../lib/status.svelte";
  import type { CustomTask } from "../lib/types";
  import Agents from "./Agents.svelte";
  import Blocks from "./Blocks.svelte";
  import ClaimBox from "./ClaimBox.svelte";
  import FloatingBar from "./FloatingBar.svelte";
  import LineJobs from "./LineJobs.svelte";
  import Parted from "./Parted.svelte";
  import Preview from "./Preview.svelte";
  import Review from "./Review.svelte";
  import ReviewHistory from "./ReviewHistory.svelte";
  import Privacy from "./Privacy.svelte";

  let { line }: { line: LineSession } = $props();

  onMount(() => {
    void line.lookAtClaim();
    void line.render();
    void line.refreshLean();
    return () => line.detach();
  });

  async function ask(tasks: string[], custom?: CustomTask) {
    if (await line.ask(tasks, custom)) await jobs.refresh();
  }

  /** Undo and redo for the whole line, and the two views. A plain text
   * field keeps its own undo. */
  function keys(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === "1" || key === "2") {
      event.preventDefault();
      show(key === "1" ? "blocks" : "pdf");
      return;
    }
    if ((event.target as HTMLElement | null)?.closest("input, textarea, select")) return;
    if (key === "z") {
      event.preventDefault();
      if (event.shiftKey) line.redo();
      else line.undo();
    } else if (key === "y") {
      event.preventDefault();
      line.redo();
    }
  }

  function show(view: View) {
    app.view = view;
  }

  /** Commits the line's title or description when the field is left. */
  function retitle(value: string) {
    const title = value.trim();
    if (!line.document || !title || title === line.document.title) return;
    line.document.title = title;
    line.changed();
    app.updateLineSummary(line.path, line.document);
  }

  function redescribe(value: string) {
    if (!line.document) return;
    const description = value.trim();
    if ((line.document.description ?? "") === description) return;
    if (description) line.document.description = description;
    else delete line.document.description;
    line.changed();
    app.updateLineSummary(line.path, line.document);
  }

  let titleDraft = $state("");
  let aboutDraft = $state("");
  $effect(() => {
    titleDraft = line.document?.title || line.opened.entry.name;
    aboutDraft = line.document?.description ?? "";
  });
</script>

<svelte:window onkeydown={keys} />

<main class="editor">
  <header class="editor-head" data-tauri-drag-region>
    <div class="editor-head__text" data-tauri-drag-region>
      <span class="editor-head__path identifier" data-tauri-drag-region>
        {line.opened.entry.group} / {line.opened.entry.name}
      </span>
      <div class="editor-head__title">
        <label class="autosize editor-head__name" data-value={titleDraft}>
          <span class="visually-hidden">Title</span>
          <input
            size="1"
            bind:value={titleDraft}
            disabled={!line.editable || !line.document}
            onchange={(event) => retitle(event.currentTarget.value)}
            onkeydown={blurOnEnter}
          />
        </label>
        <label class="autosize editor-head__about" data-value={aboutDraft || "Add a description"}>
          <span class="visually-hidden">Description</span>
          <input
            size="1"
            bind:value={aboutDraft}
            placeholder={line.editable ? "Add a description" : ""}
            disabled={!line.editable || !line.document}
            onchange={(event) => redescribe(event.currentTarget.value)}
            onkeydown={blurOnEnter}
          />
        </label>
        <span class="chip">
          <Glyph name={line.mine ? "lock" : "group"} />{line.mine ? "Private" : "Shared"}
        </span>
      </div>
    </div>

    <div class="segmented" role="group" aria-label="View">
      <button type="button" aria-pressed={app.view === "blocks"} data-tip="Blocks" data-shortcut="⌘1" onclick={() => show("blocks")}>
        <Glyph name="blocks" />Blocks
      </button>
      <button type="button" aria-pressed={app.view === "pdf"} data-tip="PDF" data-shortcut="⌘2" onclick={() => show("pdf")}>
        <Glyph name="document" />PDF
      </button>
    </div>

    <div class="editor-head__history">
      <button type="button" class="button button--icon button--small" aria-label="Undo" data-tip="Undo" data-shortcut="⌘Z"
        disabled={!line.editable || !line.canUndo} onclick={() => line.undo()}><Glyph name="undo" /></button>
      <button type="button" class="button button--icon button--small" aria-label="Redo" data-tip="Redo" data-shortcut="⇧⌘Z"
        disabled={!line.editable || !line.canRedo} onclick={() => line.redo()}><Glyph name="redo" /></button>
      {#if line.saveState === "saving"}
        <span class="save-state"><span class="spinner" aria-hidden="true"></span>Saving…</span>
      {:else if line.saveState === "not saved"}
        <span class="save-state save-state--failed"><Glyph name="alert" small />Couldn't save
          <button type="button" class="button button--link" onclick={() => line.save().catch(fail)}>Retry</button></span>
      {:else if app.stateOf(line.path) === "needs_decision"}
        <span class="save-state save-state--failed"><Glyph name="alert" small />Needs a decision</span>
      {:else if app.stateOf(line.path) === "not_sent"}
        <span class="save-state"><Glyph name="check" small />Saved · not sent yet</span>
      {:else}
        <span class="save-state"><Glyph name="check" small />Saved</span>
      {/if}
    </div>

    <ClaimBox {line} />
  </header>

  <div class="editor-body">
    {#if app.review}
      {#key app.review}
        <Review {line} review={app.review} />
      {/key}
    {:else if app.view === "blocks"}
      <section class="writing">
        {#if line.document}
          <Blocks {line} document={line.document} onask={ask} />
        {:else}
          <div class="blocks">
            <Message kind="error" text="This file is not a valid line, so it cannot be edited here." />
            {#each line.opened.faults as fault, index (index)}<p class="quiet">{fault.message}</p>{/each}
            <pre class="source">{line.opened.source}</pre>
          </div>
        {/if}
        {#if line.document}<FloatingBar {line} document={line.document} onask={ask} />{/if}
      </section>
    {:else}
      <Preview {line} />
    {/if}

    {#if !app.review}<aside class="panel" aria-label="Line">
      <Parted {line} />
      <section class="panel__section">
        <h2 class="panel__title">Agent</h2>
        <Agents {line} onask={ask} />
      </section>
      <section class="panel__section">
        <h2 class="panel__title">Jobs</h2>
        <LineJobs {line} />
      </section>
      <ReviewHistory {line} />
      <section class="panel__section panel__section--foot">
        <h2 class="panel__title">Visibility</h2>
        <Privacy {line} />
      </section>
    </aside>{/if}
  </div>
</main>
