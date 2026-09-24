<script lang="ts">
  import { api, said } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { latestRequest } from "../lib/lifecycle";
  import { tell } from "../lib/status.svelte";
  import type { AuditAction, AuditTarget, Vault } from "../lib/types";

  const loadRequest = latestRequest();
  let vault = $state<Vault | null>(null);
  let problem = $state<string | null>(null);

  let admit = $state({ siglum: "", name: "", prose: "", lean: "" });
  let bridge = $state({ left: "", right: "", kind: "predicate", lean: "" });
  let auditing = $state<{ target: AuditTarget; id: string; action: AuditAction; note: string } | null>(null);

  const names = $derived(new Map(vault?.entries.map((entry) => [entry.id, entry.name]) ?? []));

  async function load(group: string) {
    const active = loadRequest();
    try {
      const next = await api.vault(group);
      if (!active()) return;
      vault = next;
      problem = null;
    } catch (error) {
      if (active()) problem = said(error);
    }
  }

  $effect(() => {
    if (app.group) void load(app.group);
  });

  async function submit(event: SubmitEvent, work: (group: string) => Promise<string>) {
    event.preventDefault();
    const group = app.group!;
    try {
      tell(await work(group));
      await load(group);
    } catch (error) {
      tell(said(error));
    }
  }

  const admitIt = (event: SubmitEvent) =>
    submit(event, (group) => api.vaultAdmit(group, admit.siglum, admit.name, admit.prose, admit.lean));

  const bridgeIt = (event: SubmitEvent) =>
    submit(event, (group) => api.vaultBridge(group, bridge.left, bridge.right, bridge.kind, bridge.lean));

  const auditIt = (event: SubmitEvent) => {
    const { target, id, action, note } = auditing!;
    return submit(event, async (group) => {
      const said = await api.vaultAudit(group, target, id, action, note);
      auditing = null;
      return said;
    });
  };

  const isAuditing = (target: AuditTarget, id: string) => auditing?.target === target && auditing.id === id;
</script>

{#snippet actions(target: AuditTarget, id: string)}
  <div class="row__actions">
    <button onclick={() => (auditing = { target, id, action: "attest", note: "" })}>Attest</button>
    <button onclick={() => (auditing = { target, id, action: "withdraw", note: "" })}>Withdraw</button>
  </div>
  {#if auditing && isAuditing(target, id)}
    <form class="auditing stack" onsubmit={auditIt}>
      <label>{auditing.action === "attest" ? "What did you check?" : "Why is the guarantee withdrawn?"}
        <textarea rows="2" bind:value={auditing.note} required></textarea></label>
      <div class="row__actions">
        <button type="submit">{auditing.action === "attest" ? "Attest it" : "Withdraw it"}</button>
        <button type="button" class="quiet-button" onclick={() => (auditing = null)}>Not now</button>
      </div>
    </form>
  {/if}
{/snippet}

{#if app.group}
  <div class="pane">
    <h1>Definitions</h1>
    <p class="quiet">What {app.group} has admitted as settled, and the bridges between those statements.</p>
    <section>
      <p class="label">Admitted</p>
      {#if problem}
        {problem}
      {:else if !vault}
        Loading...
      {:else}
        {#each vault.entries as entry (entry.id)}
          <div class="row">
            <div>
              <p class="row__name">{entry.name}</p>
              <p class="quiet identifier">{entry.siglum} · {entry.correspondence}</p>
            </div>
            {@render actions("entry", entry.id)}
          </div>
        {:else}
          <p class="quiet">Nothing has been admitted yet.</p>
        {/each}
        {#if vault.bridges.length}
          <p class="label">Bridges</p>
          {#each vault.bridges as one (one.id)}
            <div class="row">
              <div>
                <p class="row__name">{names.get(one.left) ?? one.left} ↔ {names.get(one.right) ?? one.right}</p>
                <p class="quiet identifier">{one.kind}{one.derived ? " · derived" : ""} · {one.correspondence}</p>
              </div>
              {@render actions("bridge", one.id)}
            </div>
          {/each}
        {/if}
      {/if}
    </section>
    <section>
      <p class="label">Admit a definition</p>
      <form class="stack" onsubmit={admitIt}>
        <label>Siglum<input type="text" bind:value={admit.siglum} placeholder="def:group" required /></label>
        <label>Distinguishing name<input type="text" bind:value={admit.name} required /></label>
        <label>The prose admitted<textarea bind:value={admit.prose} rows="3" required></textarea></label>
        <label>The complete Lean module<textarea bind:value={admit.lean} rows="5" required></textarea></label>
        <button type="submit">Admit it</button>
        <p class="quiet">The module is compiled locally against the group's pinned version before submission.</p>
      </form>
    </section>
    <section>
      <p class="label">Admit a bridge</p>
      <form class="stack" onsubmit={bridgeIt}>
        <label>From<input type="text" bind:value={bridge.left} required /></label>
        <label>To<input type="text" bind:value={bridge.right} required /></label>
        <label>Kind<select bind:value={bridge.kind}><option value="predicate">predicate</option><option value="structure">structure</option></select></label>
        <label>The complete Lean module<textarea bind:value={bridge.lean} rows="5" required></textarea></label>
        <button type="submit">Admit the bridge</button>
        <p class="quiet">Bridges must be unconditional. Equivalences with hypotheses should be added as regular theorems.</p>
      </form>
    </section>
  </div>
{:else}
  <div class="pane"><p class="quiet">No groups are available on this machine.</p></div>
{/if}
