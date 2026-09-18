import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev server proxies the API to the read-only backend on :7777. The production
// path (`npm start`) serves the built bundle from the same backend, so the UI
// lives on 127.0.0.1:7777 there.
export default defineConfig({
  plugins: [react()],
  // Collapse the several packages that each depend on three (react-force-graph-3d
  // / 3d-force-graph and three-spritetext) onto ONE three instance. Two copies
  // crash the renderer with "intersectsFrustum is not a function".
  resolve: {
    dedupe: ["three"],
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:7777",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
