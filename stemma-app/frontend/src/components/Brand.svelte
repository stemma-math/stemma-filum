<script lang="ts">
  import Mark from "./Mark.svelte";

  let { on = "light" }: { on?: "dark" | "light" } = $props();
</script>

<span class="brand"><Mark {on} /><span class="wordmark">Stemma</span></span>
