<script lang="ts">
  // The checklist in a dialog of its own: after the first sign-in, from
  // the application menu, and wherever else settings are not open.
  import { setup } from "../lib/setup.svelte";
  import Checklist from "./Checklist.svelte";
  import Dialog from "./Dialog.svelte";
</script>

<Dialog bind:open={setup.open} title="Set up this machine" wide>
  <div class="checklist">
    <p class="setting__note">Everything here is optional, and can be done later from Settings &gt; This machine.</p>
    <Checklist />
  </div>
  {#snippet actions()}
    <button type="button" class="button" onclick={() => (setup.open = false)}>Done</button>
  {/snippet}
</Dialog>
