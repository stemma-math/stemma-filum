<script lang="ts">
  let { n }: { n: number } = $props();
</script>

{#if n}<span class="count">{n}</span>{/if}
