<script lang="ts">
  import { appearance, type Appearance } from "../lib/appearance.svelte";
  import { fail } from "../lib/status.svelte";

  const choices: [Appearance, string][] = [["system", "System"], ["light", "Light"], ["dark", "Dark"]];
</script>

<h3 class="settings__title">Appearance</h3>
<div class="setting">
  <div class="segmented" role="group" aria-label="Appearance">
    {#each choices as [choice, label] (choice)}
      <button type="button" aria-pressed={appearance.choice === choice} onclick={() => appearance.choose(choice).catch(fail)}>{label}</button>
    {/each}
  </div>
  <p class="setting__note">Your choice also applies to the web. System follows each device's appearance. The typeset page stays white, as it will print.</p>
</div>
