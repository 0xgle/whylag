import { defineConfig } from "vite";

// Relative base so the page works on GitHub Pages or any static host.
export default defineConfig({ base: "./", build: { target: "es2022" } });
