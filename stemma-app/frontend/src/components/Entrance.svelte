<script lang="ts">
  import type { Snippet } from "svelte";
  import Mark from "./Mark.svelte";

  let { children }: { children: Snippet } = $props();
</script>

<main class="entrance-surface">
  <div class="entrance-brand"><Mark /></div>
  <div class="entrance">{@render children()}</div>
</main>
