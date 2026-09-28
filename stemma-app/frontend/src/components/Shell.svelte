<script lang="ts">
  import { app } from "../lib/app.svelte";
  import LineEditor from "../line/LineEditor.svelte";
  import Settings from "../settings/Settings.svelte";
  import NewLine from "../views/NewLine.svelte";
  import Reviews from "../views/Reviews.svelte";
  import Vault from "../views/Vault.svelte";
  import Welcome from "../views/Welcome.svelte";
  import Sidebar from "./Sidebar.svelte";
</script>

<div class="app-shell">
  <Sidebar />
  <section class="stage">
    {#if app.stage === "editor" && app.line}
      {#key app.line}
        <LineEditor line={app.line} />
      {/key}
    {:else if app.stage === "new-line"}
      <NewLine />
    {:else if app.stage === "vault"}
      <Vault />
    {:else if app.stage === "reviews"}
      <Reviews />
    {:else}
      <Welcome />
    {/if}
  </section>
  <Settings />
</div>
