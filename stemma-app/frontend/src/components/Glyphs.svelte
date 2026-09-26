<script lang="ts">
  // The glyph sprite, inlined once so every `Glyph` can point into it. Its
  // symbols are renamed `glyph-…` so they cannot meet another id.
  import sprite from "@stemma-math/identity/glyphs.svg?raw";

  const inlined = sprite.replaceAll('id="', 'id="glyph-');
</script>

<div class="glyphs" aria-hidden="true">{@html inlined}</div>
