import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => ({
  plugins: [react(), ...(mode === "web" ? [{
    name: "browser-isolation-bootstrap",
    transformIndexHtml() {
      return [{ tag: "script", attrs: { src: "./coi-serviceworker.js" }, injectTo: "head" as const }];
    },
  }] : [])],
  base: "./",
  clearScreen: false,
  preview: { headers: { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" } },
  server: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    strictPort: true,
    port: 1420,
  },
}));
