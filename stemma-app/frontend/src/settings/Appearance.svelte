<script lang="ts">
  import { appearance, type Appearance } from "../lib/appearance.svelte";

  const choices: [Appearance, string][] = [["system", "System"], ["light", "Light"], ["dark", "Dark"]];
</script>

<h3 class="settings__title">Appearance</h3>
<div class="setting">
  <div class="segmented" role="group" aria-label="Appearance">
    {#each choices as [choice, label] (choice)}
      <button type="button" aria-pressed={appearance.choice === choice} onclick={() => appearance.choose(choice)}>{label}</button>
    {/each}
  </div>
  <p class="setting__note">System follows the computer's appearance as it changes. The typeset page stays white, as it will print.</p>
</div>
