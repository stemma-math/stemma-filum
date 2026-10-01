<script lang="ts">
  // The pale page with one card: signing this machine in, or waiting for
  // the application to start.
  import type { Snippet } from "svelte";
  import { onMount } from "svelte";
  import { light } from "@stemma-math/identity/light.js";

  let { children }: { children: Snippet } = $props();
  let canvas: HTMLCanvasElement;

  onMount(() => {
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const drawing = light(canvas, {
      int: 0.7, size: 1.18, cy: 0.75, haze: 0.32,
      grain: 0.36, speed: reduced ? 0 : 1, wob: 1, rim: 1,
    });
    if (!drawing) return;

    let frame = 0;
    let last = performance.now();
    const draw = (now: number) => {
      drawing.advance(Math.min((now - last) / 1000, 0.1), false);
      drawing.draw(0);
      last = now;
      if (!reduced) frame = requestAnimationFrame(draw);
    };
    const resize = new ResizeObserver(() => drawing.draw(0));
    resize.observe(canvas);
    frame = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
    };
  });
</script>

<main class="entrance" data-tauri-drag-region>
  <canvas class="entrance__light" bind:this={canvas} aria-hidden="true"></canvas>
  <div class="entrance__card">{@render children()}</div>
</main>
