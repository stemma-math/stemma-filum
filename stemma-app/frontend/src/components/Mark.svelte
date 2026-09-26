<script lang="ts">
  // The mark as an image: its gradients carry ids, which two inline copies
  // on one page would share.
  import onDark from "@stemma-math/identity/mark-on-dark.svg?url";
  import onLight from "@stemma-math/identity/mark-on-light.svg?url";

  let { on = "dark" }: { on?: "dark" | "light" } = $props();
</script>

<img class="mark" src={on === "dark" ? onDark : onLight} alt="" />
