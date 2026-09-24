import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// `@stemma-math/identity` is linked from the repository's `identity/`
// directory, which sits outside this project's root.
const identity = fileURLToPath(new URL("../../identity", import.meta.url));

export default defineConfig({
  plugins: [svelte()],
  // The bundle is loaded from disk by the desktop shell, never over a network.
  build: { chunkSizeWarningLimit: 1024 },
  server: { fs: { allow: [".", identity] }, port: 1420, strictPort: true },
});
