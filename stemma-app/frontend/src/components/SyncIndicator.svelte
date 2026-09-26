<script lang="ts">
  // How the last sync went, in one line; opened, each repository's outcome
  // and the way to sync again.
  import { app, syncReport } from "../lib/app.svelte";
  import Bubble from "./Bubble.svelte";
  import Glyph from "./Glyph.svelte";

  let open = $state(false);

  const report = $derived(app.synced.stepped ? syncReport(app.synced.stepped) : null);

  const shown = $derived(
    app.synced.running ? { tone: "running", headline: "Syncing…", detail: "Bringing every repository up to date" }
      : app.synced.failed ? { tone: "failed", headline: "Couldn't sync", detail: app.synced.failed }
      : report ?? { tone: "idle", headline: "Sync", detail: "Not synced since the window opened" },
  );
</script>

<div class="sync">
  <button
    type="button"
    class="sync__button sync__button--{shown.tone}"
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    {#if shown.tone === "running"}
      <span class="spinner" aria-hidden="true"></span>
    {:else}
      <span class="sync__dot" aria-hidden="true"></span>
    {/if}
    <span class="sync__words">
      <span class="sync__headline">{shown.headline}</span>
      <span class="sync__detail">{shown.detail}</span>
    </span>
  </button>
  <Bubble bind:open label="Sync" place="above">
    <div class="sync-pop">
      <p class="sync-pop__title">Repositories</p>
      {#if app.synced.stepped?.length}
        <ul class="sync-pop__list">
          {#each app.synced.stepped as one (`${one.group}/${one.side}`)}
            <li>
              <span class="identifier">{one.group}/{one.side}</span>
              <span class="sync-pop__outcome" class:sync-pop__outcome--attention={one.parted || one.dirty}>
                {one.parted ? "conflict" : one.dirty ? "uncommitted changes" : one.outcome}
              </span>
            </li>
          {/each}
        </ul>
        {#if report && report.tone === "attention"}<p class="sync-pop__note">{report.detail}</p>{/if}
      {:else if app.synced.failed}
        <p class="sync-pop__note sync-pop__note--failed">{app.synced.failed}</p>
      {:else}
        <p class="sync-pop__note">Sync to see where each repository stands.</p>
      {/if}
      <button type="button" class="button button--primary button--small" disabled={app.synced.running} onclick={() => app.sync()}>
        <Glyph name="sync" small />{app.synced.failed ? "Retry" : "Sync now"}
      </button>
    </div>
  </Bubble>
</div>
