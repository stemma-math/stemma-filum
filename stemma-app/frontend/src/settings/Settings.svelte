<script lang="ts">
  import Dialog from "../components/Dialog.svelte";
  import { app, type Panel } from "../lib/app.svelte";
  import Account from "./Account.svelte";
  import Machine from "./Machine.svelte";
  import Reviews from "./Reviews.svelte";

  const pages: [Panel, string][] = [["machine", "This machine"], ["reviews", "Reviews"], ["account", "Account"]];

  let open = $state(false);
  $effect(() => { open = app.panel !== null; });
  $effect(() => { if (!open) app.panel = null; });
</script>

<Dialog bind:open title="Settings" wide>
  <div class="settings">
    <nav class="settings__nav" aria-label="Settings">
      {#each pages as [page, label] (page)}
        <button type="button" class="settings__page" aria-current={app.panel === page ? "page" : undefined} onclick={() => (app.panel = page)}>
          {label}{#if page === "reviews" && app.waiting.length}<span class="count">{app.waiting.length}</span>{/if}
        </button>
      {/each}
    </nav>
    <div class="settings__pane">
      {#if app.panel === "machine"}
        <Machine />
      {:else if app.panel === "reviews"}
        <Reviews />
      {:else if app.panel === "account"}
        <Account />
      {/if}
    </div>
  </div>
  {#snippet actions()}
    <button type="button" class="button" onclick={() => (open = false)}>Done</button>
  {/snippet}
</Dialog>
