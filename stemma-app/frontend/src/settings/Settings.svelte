<script lang="ts">
  import Count from "../components/Count.svelte";
  import { app, type Panel } from "../lib/app.svelte";
  import Account from "./Account.svelte";
  import Machine from "./Machine.svelte";
  import Reviews from "./Reviews.svelte";

  const pages: [Panel, string][] = [["machine", "This machine"], ["reviews", "Reviews"], ["account", "Account"]];

  const close = () => (app.panel = null);

  $effect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={(event) => { if (event.target === event.currentTarget) close(); }}>
  <div class="panel" role="dialog" aria-modal="true" aria-label="Settings">
    <header class="panel__head">
      <strong>Settings</strong>
      <button class="button button--quiet" aria-label="Close settings" onclick={close}>Done</button>
    </header>
    <div class="panel__body">
      <nav class="panel__nav" aria-label="Settings">
        {#each pages as [page, label] (page)}
          <button class:here={app.panel === page} onclick={() => (app.panel = page)}>
            {label}{#if page === "reviews"}<Count n={app.waiting.length} />{/if}
          </button>
        {/each}
      </nav>
      <div class="panel__pane">
        {#if app.panel === "machine"}
          <Machine />
        {:else if app.panel === "reviews"}
          <Reviews />
        {:else}
          <Account />
        {/if}
      </div>
    </div>
  </div>
</div>
