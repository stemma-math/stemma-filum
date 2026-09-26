<script lang="ts">
  // A line glyph from the identity's sprite, which `Glyphs` puts in the page
  // once.
  let { name, small = false }: { name: string; small?: boolean } = $props();
</script>

<svg class="glyph" class:glyph--small={small} aria-hidden="true"><use href="#glyph-{name}" /></svg>
