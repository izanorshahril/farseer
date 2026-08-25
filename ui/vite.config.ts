import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { widgetHost } from "./plugins/widget-host";

/**
 * The canvas talks to farseer at `/v1`, same-origin, and the dev server proxies
 * that to the running daemon.
 *
 * The proxy attaches the operator token, so **the browser never holds it**.
 * `28 operator surface` gate 3 requires that of a widget; holding it in the host
 * page instead would only move the problem into `localStorage`, where clearing
 * site data or any script on the page reaches it. Whatever packages this later -
 * served by farseer itself, Tauri, Electron - has to keep that property.
 *
 * `FARSEER_PORT` and `FARSEER_TOKEN` come from `farseer serve`'s own startup
 * output.
 */
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const port = process.env.FARSEER_PORT ?? "9000";
const token = process.env.FARSEER_TOKEN ?? "";

export default defineConfig({
  plugins: [
    react(),
    // `28 operator surface` section 3: widget code lives in `widgets/` in git,
    // beside the cell definitions, and farseer never stores or serves it.
    widgetHost({
      widgetsDir: path.join(repoRoot, "widgets"),
      repo: repoRoot,
      hostDir: path.join(repoRoot, "ui"),
    }),
  ],
  server: {
    port: 5173,
    proxy: {
      "/v1": {
        target: `http://127.0.0.1:${port}`,
        changeOrigin: true,
        headers: token ? { authorization: `Bearer ${token}` } : undefined,
      },
    },
  },
});
