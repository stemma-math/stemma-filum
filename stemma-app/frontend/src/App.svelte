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
  import { api, onCloseRequested, onMachineChosen, onOpenReview, onRepaired, onViewChosen, said } from "./lib/api";
  import { app } from "./lib/app.svelte";
  import { appearance } from "./lib/appearance.svelte";
  import { subscriptions } from "./lib/lifecycle";
  import { live } from "./lib/live.svelte";
  import { setup } from "./lib/setup.svelte";
  import { fail, setRemedies, tell } from "./lib/status.svelte";
  import SignIn from "./views/SignIn.svelte";

  // What each remedy an error can carry does, instead of the command the
  // command line would name.
  setRemedies({
    "sign-in": { label: "Sign In Again", run: () => void app.signOut() },
    "set-up": { label: "Set Up This Machine", run: () => (setup.open = true) },
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
    const listeners = subscriptions();
    let closing = false;

    // Closing flushes and records the open line; if that fails the window
    // stays open so nothing is lost.
    listeners.add(onCloseRequested(async (event) => {
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
    }));

    listeners.add(onViewChosen((view) => {
      if (view === "blocks" || view === "pdf") app.view = view;
    }));
    listeners.add(onMachineChosen((action) => {
      if (action === "setup") setup.open = true;
      if (action === "remove") setup.removing = true;
    }));
    listeners.add(onRepaired(() => void setup.load()));
    listeners.add(onOpenReview(() => void app.openPendingReview()));
    listeners.add(live.start());
    const refreshAppearance = () => { if (app.signed) void appearance.refresh().catch(() => {}); };
    window.addEventListener("focus", refreshAppearance);
    listeners.add(() => window.removeEventListener("focus", refreshAppearance));

    void app.boot();
    return () => listeners.stop();
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
