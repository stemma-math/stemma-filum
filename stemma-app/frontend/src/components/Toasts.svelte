<script lang="ts">
  import { dismiss, status } from "../lib/status.svelte";
  import Glyph from "./Glyph.svelte";
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each status.toasts as toast (toast.id)}
    <div class="toast" class:toast--error={toast.error}>
      {#if toast.error}<Glyph name="alert" />{/if}
      <span class="toast__message">{toast.message}</span>
      {#if toast.action}
        {@const action = toast.action}
        <button type="button" class="button button--link" onclick={() => { action.run(); dismiss(toast.id); }}>{action.label}</button>
      {/if}
      <button type="button" class="toast__close" aria-label="Close" onclick={() => dismiss(toast.id)}><Glyph name="close" small /></button>
    </div>
  {/each}
</div>
