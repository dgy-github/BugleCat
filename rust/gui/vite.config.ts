import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev port and serves the built `dist/` in production.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5179,
    strictPort: true,
  },
  build: {
    target: "esnext",
    outDir: "dist",
    emptyOutDir: true,
  },
});
