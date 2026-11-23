import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// The API is a separate process. In development Vite forwards `/api` to it, so the page and
// the API share an origin exactly as they will behind one host in production — which is what
// lets the session cookie stay `SameSite=Lax` and scoped to `/api`, and what the CSRF
// endpoint's same-origin argument rests on. There is no CORS configuration anywhere, and
// there should never need to be.
const api = process.env.MT_API ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/api": { target: api, changeOrigin: false } },
    // The design tokens live in `design/tokens/` at the repository root and are imported
    // from there, not copied: one source for colour, type and space across every client.
    fs: { allow: [".."] },
  },
  preview: {
    proxy: { "/api": { target: api, changeOrigin: false } },
  },
  build: { sourcemap: true },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
});
