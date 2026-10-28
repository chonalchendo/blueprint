/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The build lands in web/dist, which bp-view embeds (32, design change 12). The dev server proxies the API to a
// running bp-view, so `npm run dev` reads a real vault.
export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", emptyOutDir: true, assetsInlineLimit: 0 },
  server: { proxy: { "/api": process.env.BP_VIEW_URL ?? "http://127.0.0.1:7777" } },
  test: { environment: "happy-dom", include: ["src/**/*.test.ts", "src/**/*.test.tsx"], setupFiles: ["src/test-setup.ts"] },
});
