<script lang="ts">
  // The navy sidebar: the brand, the group, its lines, the vault, whether
  // this machine reaches the server and who is signed in.
  import { app } from "../lib/app.svelte";
  import { initials } from "../lib/names";
  import type { Known } from "../lib/types";
  import Bubble from "./Bubble.svelte";
  import Glyph from "./Glyph.svelte";
  import Mark from "./Mark.svelte";
  import Connection from "./Connection.svelte";

  let choosing = $state(false);

  function choose(group: string) {
    choosing = false;
    void app.chooseGroup(group);
  }

  const here = $derived(app.stage === "editor" ? app.line?.path : undefined);
  const signed = $derived(app.signed!);
</script>

{#snippet list(lines: Known[], empty: string)}
  {#each lines as line (line.path)}
    <button
      type="button"
      class="sidebar__line"
      aria-current={line.path === here ? "page" : undefined}
      onclick={() => app.open(line.path)}
    >
      <span class="sidebar__line-title">{line.title ?? line.name}</span>
      {#if line.state !== "sent"}
        <span class="sidebar__line-state">{line.state === "needs_decision" ? "Needs a decision" : "Not sent yet"}</span>
      {/if}
      {#if line.description}<span class="sidebar__line-about">{line.description}</span>{/if}
    </button>
  {:else}
    <p class="sidebar__empty">{empty}</p>
  {/each}
{/snippet}

<aside class="sidebar">
  <div class="sidebar__head" data-tauri-drag-region>
    <span class="brand" data-tauri-drag-region><Mark /><span class="wordmark">Stemma</span></span>
  </div>

  <div class="sidebar__group">
    <button
      type="button"
      class="group-switch"
      aria-haspopup="menu"
      aria-expanded={choosing}
      aria-label="Switch group"
      onclick={() => (choosing = !choosing)}
    >
      {#if app.group}
        <span class="group-switch__badge" aria-hidden="true">{initials(app.group)}</span>
        <span class="group-switch__name">{app.group}</span>
      {:else}
        <span class="group-switch__name group-switch__name--none">No group</span>
      {/if}
      <Glyph name="chevron-up-down" small />
    </button>
    <Bubble bind:open={choosing} label="Groups" menu>
      {#each app.groups as group (group)}
        <button type="button" class="menu__item" role="menuitemcheckbox" aria-checked={group === app.group} onclick={() => choose(group)}>
          <span class="group-switch__badge group-switch__badge--small" aria-hidden="true">{initials(group)}</span>
          <span class="identifier">{group}</span>
          {#if group === app.group}<span class="menu__hint"><Glyph name="check" small /></span>{/if}
        </button>
      {:else}
        <p class="menu__empty">No groups on this machine.</p>
      {/each}
    </Bubble>
  </div>

  {#if app.group}
    <button
      type="button"
      class="sidebar__new"
      aria-current={app.stage === "new-line" ? "page" : undefined}
      onclick={() => app.goTo("new-line")}
    >
      <Glyph name="add" small />New line
    </button>

    <nav class="sidebar__lines" aria-label="Lines">
      <p class="sidebar__section">Private</p>
      {@render list(app.railLines.mine, "No private lines yet.")}
      <p class="sidebar__section">Shared with the group</p>
      {@render list(app.railLines.ours, "Nothing shared yet.")}
    </nav>
  {:else}
    <p class="sidebar__empty sidebar__lines">No groups are on this machine yet. They arrive as soon as it reaches the server.</p>
  {/if}

  <div class="sidebar__foot">
    <button
      type="button"
      class="sidebar__item"
      aria-current={app.stage === "reviews" ? "page" : undefined}
      onclick={() => app.goTo("reviews")}
    >
      <Glyph name="check" />Reviews
      {#if app.waiting.length}<span class="count">{app.waiting.length}</span>{/if}
    </button>
    {#if app.group}
      <button
        type="button"
        class="sidebar__item"
        aria-current={app.stage === "vault" ? "page" : undefined}
        onclick={() => app.goTo("vault")}
      >
        <Glyph name="book" />Group vault
      </button>
    {/if}

    <Connection />

    <div class="user-card">
      <button
        type="button"
        class="user-card__me"
        aria-label="Your profile, on the web"
        data-tip="Your profile, in your browser"
        onclick={() => app.openOnWeb("/profile")}
      >
        <span class="avatar" aria-hidden="true">{initials(signed.user)}</span>
        <span class="user-card__who">
          <span class="identifier user-card__id">{signed.user}</span>
          <span class="identifier user-card__server">{signed.server}</span>
        </span>
      </button>
      <button
        type="button"
        class="user-card__settings"
        aria-label="Settings"
        data-tip="Settings"
        onclick={() => (app.panel = "machine")}
      >
        <Glyph name="settings" />
      </button>
    </div>
  </div>
</aside>
