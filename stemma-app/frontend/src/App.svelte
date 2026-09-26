<script lang="ts">
  import { onMount } from "svelte";
  import Brand from "./components/Brand.svelte";
  import Entrance from "./components/Entrance.svelte";
  import Glyphs from "./components/Glyphs.svelte";
  import Shell from "./components/Shell.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Tooltips from "./components/Tooltips.svelte";
  import { onViewChosen, said } from "./lib/api";
  import { app } from "./lib/app.svelte";
  import { fail } from "./lib/status.svelte";
  import SignIn from "./views/SignIn.svelte";

  onMount(() => {
    let closing = false;
    let stop: (() => void) | undefined;

    // Closing flushes and records the open line; if that fails the window
    // stays open so nothing is lost.
    window.__TAURI__?.window
      ?.getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (closing) {
          event.preventDefault();
          return;
        }
        closing = true;
        try {
          await app.beforeClose();
        } catch (error) {
          event.preventDefault();
          fail(`The window remains open because your changes could not be saved: ${said(error)}`);
        } finally {
          closing = false;
        }
      })
      .then((unlisten) => (stop = unlisten));

    let unhear: (() => void) | undefined;
    void onViewChosen((view) => {
      if (view === "blocks" || view === "pdf") app.view = view;
    }).then((stop) => (unhear = stop));

    void app.boot();
    return () => {
      stop?.();
      unhear?.();
    };
  });
</script>

<Glyphs />

{#if !app.ready}
  <Entrance>
    <Brand />
    <p class="entrance__quiet">Loading…</p>
  </Entrance>
{:else if !app.signed}
  <SignIn />
{:else}
  <Shell />
{/if}

<Toasts />
<Tooltips />
