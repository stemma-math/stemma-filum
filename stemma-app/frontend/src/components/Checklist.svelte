<script lang="ts">
  // Setting this machine up: five items, each with where it stands and
  // the one button that does it. Every item can be left as it is.
  import Message from "./Message.svelte";
  import { onMount, type Snippet } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { setup } from "../lib/setup.svelte";
  import { fail, succeed, warn } from "../lib/status.svelte";
  import type { SetupAgent, SetupItem, SetupState } from "../lib/types";

  const words: Record<SetupState, string> = {
    done: "Done",
    attention: "Needs attention",
    not_set_up: "Not set up",
    unknown: "Cannot check",
  };

  /** Which button is working, so it can say so and not be pressed twice. */
  let busy = $state<string | null>(null);
  /** The groups ticked, seeded from the survey once it arrives. */
  let offered = $state<string[] | null>(null);

  $effect(() => {
    const groups = setup.survey?.groups;
    if (offered === null && groups) offered = groups.state === "done" ? [...groups.serving] : [...app.groups];
  });

  async function act(key: string, work: () => Promise<string>) {
    busy = key;
    try { await setup.act(work); } finally { busy = null; }
  }

  async function test(agent: SetupAgent) {
    busy = `agent:${agent.name}`;
    try {
      const tried = await api.tryAgent(agent.name);
      const said = `${agent.called}: ${tried.said}`;
      if (tried.availability === "ready") succeed(said);
      else warn(said);
    } catch (error) {
      fail(error);
    } finally {
      busy = null;
    }
    await setup.load();
  }

  function saveGroups(event: SubmitEvent) {
    event.preventDefault();
    void act("groups", () => api.nodeInit(offered ?? []));
  }

  function instructions(url: string) {
    api.openLink(url).catch(fail);
  }

  onMount(() => void setup.load());
</script>

{#snippet chip(state: SetupState)}
  <span class="chip" class:chip--success={state === "done"} class:chip--warning={state === "attention"} class:chip--dashed={state === "not_set_up"}>{words[state]}</span>
{/snippet}

{#snippet item(title: string, standing: SetupItem, about: string, body: Snippet)}
  <section class="setting checklist__item">
    <h4 class="setting__title">{title}{@render chip(standing.state)}</h4>
    <p class="setting__note">{about}</p>
    {#if standing.detail}
      <p class="setting__note" class:setting__note--attention={standing.state === "attention"}>{standing.detail}</p>
    {/if}
    {@render body()}
  </section>
{/snippet}

{#snippet working(key: string, label: string, run: () => void)}
  <button type="button" class="button button--small" disabled={busy !== null} onclick={run}>
    {#if busy === key}<span class="spinner" aria-hidden="true"></span>{/if}{label}
  </button>
{/snippet}

{#if setup.problem && !setup.survey}
  <Message kind="error" text={setup.problem} />
{:else if !setup.survey}
  <div class="skeleton setting__skeleton"></div>
  <div class="skeleton setting__skeleton"></div>
{:else}
  {@const survey = setup.survey}

  {#snippet service()}
    {#if survey.service.state !== "done"}
      <div class="row__actions">
        {@render working("service", survey.service.state === "attention" ? "Repair" : "Install", () => act("service", api.setupService))}
      </div>
    {/if}
  {/snippet}
  {@render item("Background service", survey.service, "Keeps lines in step and takes jobs while the application is closed.", service)}

  {#snippet commandLine()}
    {@const path = survey.command_line.path_line}
    {#if path}
      <p class="setting__note">Stemma can add this line to <span class="identifier">{path.profile}</span>:</p>
      <code class="checklist__line">{path.line}</code>
      <div class="row__actions">
        {@render working("path", `Add to ${path.profile.split("/").pop()}`, () => act("path", api.setupPath))}
      </div>
    {:else if survey.command_line.state !== "done"}
      <div class="row__actions">
        {@render working("link", survey.command_line.state === "attention" ? "Repair" : "Install", () => act("link", api.setupLink))}
      </div>
    {/if}
  {/snippet}
  {@render item("Command line", survey.command_line, "Puts the stemma command in ~/.local/bin, for use in a terminal.", commandLine)}

  {#snippet groups()}
    <form class="stack" onsubmit={saveGroups}>
      <div class="stack stack--tight">
        {#each app.groups as group (group)}
          <label class="choice"><input type="checkbox" bind:group={offered} value={group} /><span class="identifier">{group}</span></label>
        {:else}
          <p class="setting__note">No groups are available on this machine.</p>
        {/each}
      </div>
      {#if app.groups.length}
        <div class="row__actions">
          <button class="button button--small" type="submit" disabled={busy !== null}>
            {#if busy === "groups"}<span class="spinner" aria-hidden="true"></span>{/if}Save
          </button>
        </div>
      {/if}
    </form>
  {/snippet}
  {@render item("Work for groups", survey.groups, "Which of your groups may send jobs to this machine.", groups)}

  {#snippet agents()}
    <ul class="checklist__agents">
      {#each survey.agents as agent (agent.name)}
        <li>
          <span class="checklist__agent">
            <span>{agent.called}</span>
            <span class="setting__note">{agent.detail ?? agent.version ?? words[agent.state]}</span>
          </span>
          {@render chip(agent.state)}
          {#if agent.state === "not_set_up"}
            <button type="button" class="button button--small" onclick={() => instructions(agent.instructions)}>How to Install</button>
          {:else}
            {@render working(`agent:${agent.name}`, "Test", () => test(agent))}
          {/if}
        </li>
      {/each}
    </ul>
  {/snippet}
  {@render item(
    "Agents",
    { state: survey.agents.some((agent) => agent.state === "done") ? "done" : survey.agents.some((agent) => agent.state === "attention") ? "attention" : "not_set_up", detail: null },
    "Only needed for agent work. Stemma finds the agents installed and signed in here; it does not install them.",
    agents,
  )}

  {#snippet lean()}
    {#if survey.lean.state !== "done"}
      <div class="row__actions">
        <button type="button" class="button button--small" onclick={() => instructions(survey.lean.instructions)}>How to Install</button>
      </div>
    {/if}
  {/snippet}
  {@render item("Lean", survey.lean, "Only needed for formalization. Lean is installed with elan, which Stemma does not install.", lean)}
{/if}
