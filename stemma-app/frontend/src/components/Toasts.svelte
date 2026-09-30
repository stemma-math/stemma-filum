<script lang="ts">
  // The toasts, over everything. A modal dialog makes the rest of the
  // window inert and sits in the top layer, so the toasts are a popover
  // kept inside the topmost open dialog (or else the body) and shown again
  // after it, which puts them above it and leaves them within reach.
  import { onMount } from "svelte";
  import { dismiss, status } from "../lib/status.svelte";
  import Message from "./Message.svelte";

  let host = $state<HTMLDivElement>();

  /** Moves the toasts above whatever is on top now, if that changed. */
  function raise() {
    if (!host) return;
    const dialogs = [...document.querySelectorAll<HTMLDialogElement>("dialog[open]")];
    const place = dialogs.at(-1) ?? document.body;
    // Leaving the document hides a popover, so a move shows it again last.
    if (host.parentElement !== place) place.append(host);
    const shown = host.matches(":popover-open");
    if (status.toasts.length && !shown) host.showPopover();
    else if (!status.toasts.length && shown) host.hidePopover();
  }

  onMount(() => {
    // A dialog opens, closes or goes away with the view that held it.
    const watch = new MutationObserver(raise);
    watch.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open"] });
    return () => { watch.disconnect(); host?.remove(); };
  });

  $effect(() => {
    void status.toasts.length;
    raise();
  });
</script>

<div bind:this={host} class="toasts" popover="manual" aria-live="polite">
  {#each status.toasts as toast (toast.id)}
    <Message kind={toast.kind} text={toast.message} class="toast" life={toast.life} onclose={() => dismiss(toast.id)}>
      {#snippet actions()}
        {#if toast.action}
          {@const action = toast.action}
          <button type="button" class="button button--link" onclick={() => { action.run(); dismiss(toast.id); }}>{action.label}</button>
        {/if}
      {/snippet}
    </Message>
  {/each}
</div>
