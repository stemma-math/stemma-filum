<script lang="ts">
  // A message of one of the four kinds: its glyph, its words (addresses in
  // them are links, opened in the browser) and, when given, an action and
  // a close button. Toasts are messages too, raised over everything.
  import type { Snippet } from "svelte";
  import { api } from "../lib/api";
  import { pieces } from "../lib/links";
  import { fail, type Kind } from "../lib/status.svelte";
  import Glyph from "./Glyph.svelte";

  let {
    kind,
    text,
    children,
    actions,
    onclose,
    life = null,
    class: extra = "",
  }: {
    kind: Kind;
    /** Plain words, whose addresses become links. */
    text?: string;
    children?: Snippet;
    actions?: Snippet;
    onclose?: () => void;
    /** For a toast that goes away by itself: how long it stays, in
     * milliseconds, drawn as the time it has left. */
    life?: number | null;
    class?: string;
  } = $props();

  const GLYPHS: Record<Kind, string> = { error: "alert", warning: "warning", success: "done", info: "info" };

  function follow(event: MouseEvent, href: string) {
    event.preventDefault();
    api.openLink(href).catch(fail);
  }
</script>

<div class="message message--{kind} {extra}" role={kind === "error" || kind === "warning" ? "alert" : "status"}>
  <Glyph name={GLYPHS[kind]} />
  <p class="message__text">
    {#if text !== undefined}
      {#each pieces(text) as piece, index (index)}
        {#if piece.href}<a href={piece.href} onclick={(event) => follow(event, piece.href!)}>{piece.text}</a>{:else}{piece.text}{/if}
      {/each}
    {/if}
    {@render children?.()}
  </p>
  {@render actions?.()}
  {#if life !== null}
    <span class="toast__time" style="--toast-life: {life}ms" aria-hidden="true" onanimationend={onclose}></span>
  {/if}
  {#if onclose}
    <button type="button" class="message__close" aria-label="Close" onclick={onclose}><Glyph name="close" small /></button>
  {/if}
</div>
