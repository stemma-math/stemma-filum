<script lang="ts">
  import { nameOf } from "../lib/blocks";
  import type { Statement } from "../lib/types";

  let { statement }: { statement: Statement } = $props();
</script>

<span class="statement-name">
  <span>{nameOf(statement)}</span>
  {#if statement.siglum}<code class="statement-name__siglum">{statement.siglum}</code>{/if}
</span>
