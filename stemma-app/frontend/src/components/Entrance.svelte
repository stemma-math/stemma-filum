<script lang="ts">
  // The pale page with one card: signing this machine in, or waiting for
  // the application to start.
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<main class="entrance" data-tauri-drag-region>
  <div class="entrance__card">{@render children()}</div>
</main>
