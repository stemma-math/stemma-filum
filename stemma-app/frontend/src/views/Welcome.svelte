<script lang="ts">
  import Glyph from "../components/Glyph.svelte";
  import { app } from "../lib/app.svelte";
  import { initials } from "../lib/names";
</script>

<div class="pane pane--center" data-tauri-drag-region>
  {#if app.group}
    <div class="welcome">
      <span class="badge badge--large" aria-hidden="true">{initials(app.group)}</span>
      <h1 class="welcome__title identifier">{app.group}</h1>
      <p class="welcome__text">Choose a line on the left, or start a new one.</p>
      <div class="welcome__actions">
        <button type="button" class="button button--primary" onclick={() => app.goTo("new-line")}><Glyph name="add" small />New line</button>
        {#if app.managed.includes(app.group)}
          <button
            type="button"
            class="button"
            data-tip="Opens in your browser"
            onclick={() => app.openOnWeb(`/g/${encodeURIComponent(app.group!)}/edit`)}
          ><Glyph name="edit" small />Edit group</button>
        {/if}
      </div>
    </div>
  {:else}
    <div class="welcome welcome--empty">
      <h1 class="welcome__title">No groups yet</h1>
      <p class="welcome__text">Your groups arrive here as soon as this machine reaches the server.</p>
    </div>
  {/if}
</div>
