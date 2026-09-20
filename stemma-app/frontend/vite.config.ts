import { defineConfig } from "vite";

// The drawing of the descent is shared with the instance's own pages,
// so it lives above this directory and Vite is told it may read it.
export default defineConfig({
  server: { fs: { allow: [".."] }, port: 1420, strictPort: true },
});
