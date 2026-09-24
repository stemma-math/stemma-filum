<script lang="ts">
  import { onMount } from "svelte";
  import Brand from "./components/Brand.svelte";
  import Entrance from "./components/Entrance.svelte";
  import Shell from "./components/Shell.svelte";
  import { said } from "./lib/api";
  import { app } from "./lib/app.svelte";
  import { status, tell } from "./lib/status.svelte";
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
          tell(`The window remains open because your changes could not be saved: ${said(error)}`);
        } finally {
          closing = false;
        }
      })
      .then((unlisten) => (stop = unlisten));

    void app.boot();
    return () => stop?.();
  });
</script>

{#if !app.ready}
  <Entrance>
    <Brand />
    <p class="quiet">Starting your workspace...</p>
  </Entrance>
{:else if !app.signed}
  <SignIn />
{:else}
  <Shell />
{/if}

<div id="said" hidden={!status.message}>{status.message}</div>
