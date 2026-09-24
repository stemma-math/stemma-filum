<script lang="ts">
  import { app } from "../lib/app.svelte";
  import LineEditor from "../line/LineEditor.svelte";
  import Settings from "../settings/Settings.svelte";
  import NewLine from "../views/NewLine.svelte";
  import Vault from "../views/Vault.svelte";
  import Welcome from "../views/Welcome.svelte";
  import Brand from "./Brand.svelte";
  import Count from "./Count.svelte";
  import Rail from "./Rail.svelte";

  /** `usr:mario` → `mario`. */
  const shortName = (id: string) => id.trim().split(":").pop() ?? "";

  const signed = $derived(app.signed!);
  const initial = $derived(shortName(signed.user).charAt(0).toUpperCase() || "S");
  const where = $derived(
    app.stage === "editor" ? app.lineTitle || "Editor"
      : app.stage === "vault" ? "Definitions"
      : app.stage === "new-line" ? "New line"
      : signed.server,
  );
</script>

<div class="app-shell">
  <header class="topbar">
    <Brand />
    <span class="topbar__where">{where}</span>
    <span class="spacer"></span>
    <button class="ghost" title="Bring this machine into step with the instance" onclick={() => app.sync()}>
      <span class="status-dot"></span>Sync
    </button>
    <button class="ghost" onclick={() => (app.panel = "machine")}>Settings<Count n={app.waiting.length} /></button>
    <button class="account" onclick={() => (app.panel = "account")}>
      <span class="avatar" aria-hidden="true">{initial}</span>
      <span class="identity">{signed.user}<small>{signed.server}</small></span>
    </button>
  </header>
  <div class="app-main">
    <Rail />
    <section class="stage">
      {#if app.stage === "editor" && app.line}
        {#key app.line}
          <LineEditor line={app.line} />
        {/key}
      {:else if app.stage === "new-line"}
        <NewLine />
      {:else if app.stage === "vault"}
        <Vault />
      {:else}
        <Welcome />
      {/if}
    </section>
  </div>
  {#if app.panel}
    <Settings />
  {/if}
</div>
