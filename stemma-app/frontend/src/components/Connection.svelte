<script lang="ts">
  // Whether this machine reaches the server, and whether anything is
  // waiting to leave it; opened, the lines that are not sent yet or need
  // a decision. Nothing here asks for synchronising: it happens by itself.
  import { app } from "../lib/app.svelte";
  import { sameLine, type Named } from "../lib/lines";
  import Bubble from "./Bubble.svelte";

  let open = $state(false);

  const lines = $derived(app.mirrored?.lines ?? []);
  const deciding = $derived(lines.filter((line) => line.state === "needs_decision"));
  const waiting = $derived(lines.filter((line) => line.state === "not_sent"));

  function count(many: number, one: string, more: string) {
    return many === 1 ? `1 ${one}` : `${many} ${more}`;
  }

  const shown = $derived.by(() => {
    const mirrored = app.mirrored;
    if (!mirrored) return { tone: "idle", headline: "Connecting…", detail: "Reaching the server" };
    const left = waiting.length ? `${count(waiting.length, "change", "changes")} waiting` : "Everything is sent";
    if (!mirrored.connected) return { tone: "failed", headline: "Offline", detail: left };
    if (deciding.length) {
      return { tone: "attention", headline: "Connected", detail: `${count(deciding.length, "line needs", "lines need")} a decision` };
    }
    return { tone: waiting.length ? "attention" : "good", headline: "Connected", detail: left };
  });

  const where = (line: Named) => app.lines.find((known) => sameLine(known, line));
</script>

<div class="sync">
  <button
    type="button"
    class="sync__button sync__button--{shown.tone}"
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="sync__dot" aria-hidden="true"></span>
    <span class="sync__words">
      <span class="sync__headline">{shown.headline}</span>
      <span class="sync__detail">{shown.detail}</span>
    </span>
  </button>
  <Bubble bind:open label="Connection" place="above">
    <div class="sync-pop">
      {#if app.mirrored && !app.mirrored.connected && app.mirrored.reason}
        <p class="sync-pop__note sync-pop__note--failed">{app.mirrored.reason}</p>
        <p class="sync-pop__note">What you write is kept on this machine and leaves once the server can be reached.</p>
      {/if}
      {#if lines.length}
        <p class="sync-pop__title">Lines</p>
        <ul class="sync-pop__list">
          {#each lines as line (line.id)}
            {@const known = where(line)}
            <li>
              {#if known}
                <button type="button" class="button button--link" onclick={() => { open = false; void app.open(known.path); }}>
                  {known.title ?? line.name}
                </button>
              {:else}
                <span class="identifier">{line.name}</span>
              {/if}
              <span class="sync-pop__outcome sync-pop__outcome--attention" title={line.trouble ?? undefined}>
                {line.state === "needs_decision" ? "needs a decision" : "not sent yet"}
              </span>
            </li>
          {/each}
        </ul>
      {:else if app.mirrored?.connected}
        <p class="sync-pop__note">Everything written on this machine is on the server.</p>
      {/if}
    </div>
  </Bubble>
</div>
