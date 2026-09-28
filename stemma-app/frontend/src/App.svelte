<script lang="ts">
  import { onMount } from "svelte";
  import Brand from "./components/Brand.svelte";
  import Removal from "./components/Removal.svelte";
  import SetUp from "./components/SetUp.svelte";
  import Entrance from "./components/Entrance.svelte";
  import Glyphs from "./components/Glyphs.svelte";
  import Shell from "./components/Shell.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Tooltips from "./components/Tooltips.svelte";
  import { api, onMachineChosen, onRepaired, onViewChosen, said } from "./lib/api";
  import { app } from "./lib/app.svelte";
  import { live } from "./lib/live.svelte";
  import { setup } from "./lib/setup.svelte";
  import { fail, setRemedies, tell } from "./lib/status.svelte";
  import SignIn from "./views/SignIn.svelte";

  // What each remedy an error can carry does, instead of the command the
  // command line would name.
  setRemedies({
    "sign-in": { label: "Sign In Again", run: () => void app.signOut() },
    "set-up": { label: "Set Up This Machine…", run: () => (setup.open = true) },
    sync: { label: "Send Now", run: () => void api.sendNow().then(() => tell("Sending now.")).catch(fail) },
  });

  // Once signed in: look at this machine, and open the checklist when
  // this was a sign-in rather than a launch.
  $effect(() => {
    if (!app.signed) return;
    void setup.load();
    if (setup.afterSignIn) {
      setup.afterSignIn = false;
      setup.open = true;
    }
  });

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

    let unchoose: (() => void) | undefined;
    void onMachineChosen((action) => {
      if (action === "setup") setup.open = true;
      if (action === "remove") setup.removing = true;
    }).then((stop) => (unchoose = stop));

    let unrepair: (() => void) | undefined;
    void onRepaired(() => void setup.load()).then((stop) => (unrepair = stop));

    let unlisten: (() => void) | undefined;
    void live.start().then((stop) => (unlisten = stop));

    let stopReviews: (() => void) | undefined;
    void window.__TAURI__?.event?.listen<string>("stemma:open-review", (event) => {
      if (app.ready && app.signed) void app.openReview(event.payload);
    }).then((stop) => (stopReviews = stop));

    void app.boot().then(async () => {
      if (!app.signed) return;
      const proposal = await api.takeOpenReview().catch(() => null);
      if (proposal) await app.openReview(proposal);
    });
    return () => {
      stop?.();
      unhear?.();
      unchoose?.();
      unrepair?.();
      unlisten?.();
      stopReviews?.();
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

<SetUp />
<Removal />
<Toasts />
<Tooltips />
