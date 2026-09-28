<script lang="ts">
  // Asks before something that cannot be taken back lightly, naming what
  // will happen and the exact verb.
  import Dialog from "../components/Dialog.svelte";

  let {
    open,
    title,
    text,
    verb,
    danger = true,
    onconfirm,
    oncancel,
  }: {
    open: boolean;
    title: string;
    text: string;
    verb: string;
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

</script>

<!-- Closing the dialog any other way than confirming cancels. -->
<Dialog bind:open={() => open, (shown) => { if (!shown && open) oncancel(); }} {title}>
  <p class="dialog__text">{text}</p>
  {#snippet actions()}
    <button type="button" class="button" onclick={oncancel}>Cancel</button>
    <button type="button" class="button button--primary" class:button--danger={danger} onclick={onconfirm}>{verb}</button>
  {/snippet}
</Dialog>
