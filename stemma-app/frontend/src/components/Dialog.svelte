<script lang="ts">
  // A modal dialog: a title in a sentence, what will happen, and the
  // buttons at the right. Escape and a click on the backdrop close it, and
  // focus goes back to whatever opened it. Opening focuses the field marked
  // `autofocus`, or else the dialog itself, never whichever button is first.
  import type { Snippet } from "svelte";

  let {
    open = $bindable(false),
    title,
    wide = false,
    children,
    actions,
  }: { open: boolean; title: string; wide?: boolean; children: Snippet; actions?: Snippet } = $props();

  let dialog = $state<HTMLDialogElement>();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      if (!dialog.querySelector("[autofocus]")) dialog.focus();
    }
    else if (!open && dialog.open) dialog.close();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialog}
  class="dialog"
  class:dialog--wide={wide}
  tabindex="-1"
  aria-label={title}
  onclose={() => (open = false)}
  onclick={(event) => { if (event.target === dialog) open = false; }}
>
  {#if open}
    <div class="dialog__body">
      <h2 class="dialog__title">{title}</h2>
      {@render children()}
      {#if actions}<div class="dialog__actions">{@render actions()}</div>{/if}
    </div>
  {/if}
</dialog>
