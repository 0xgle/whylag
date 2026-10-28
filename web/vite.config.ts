import { defineConfig } from "vite";

// Relative base so the pages work on GitHub Pages or any static host.
export default defineConfig({
  base: "./",
  build: {
    target: "es2022",
    rollupOptions: { input: { main: "index.html", ranking: "ranking.html" } },
  },
});
