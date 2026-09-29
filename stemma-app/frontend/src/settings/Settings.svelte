<script lang="ts">
  import Dialog from "../components/Dialog.svelte";
  import { app, type Panel } from "../lib/app.svelte";
  import Account from "./Account.svelte";
  import Appearance from "./Appearance.svelte";
  import Machine from "./Machine.svelte";

  const pages: [Panel, string][] = [
    ["machine", "This machine"],
    ["account", "Account"],
    ["appearance", "Appearance"],
  ];

  /** Open while a page is chosen; closing it chooses none. */
  function close() {
    app.panel = null;
  }
</script>

<Dialog bind:open={() => app.panel !== null, (open) => { if (!open) close(); }} title="Settings" wide>
  <div class="settings">
    <nav class="settings__nav" aria-label="Settings">
      {#each pages as [page, label] (page)}
        <button type="button" class="settings__page" aria-current={app.panel === page ? "page" : undefined} onclick={() => (app.panel = page)}>
          {label}
        </button>
      {/each}
    </nav>
    <div class="settings__pane">
      {#if app.panel === "machine"}
        <Machine />
      {:else if app.panel === "account"}
        <Account />
      {:else if app.panel === "appearance"}
        <Appearance />
      {/if}
    </div>
  </div>
  {#snippet actions()}
    <button type="button" class="button" onclick={close}>Done</button>
  {/snippet}
</Dialog>
