import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import os from "node:os";
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
 * Port and token come from **farseer's own runtime file**, which `farseer serve`
 * writes on startup and `farseer where` prints. Asking the operator to paste
 * them into environment variables was a step that existed only because this file
 * did not read the file farseer already writes, and getting it wrong produced
 * `ECONNREFUSED 127.0.0.1:9000` - a message that names the default rather than
 * the mistake.
 */
function runtimeFilePath(): string {
  // The same order `security.rs` uses, so the two cannot disagree about where
  // the file lives.
  const base =
    process.env.LOCALAPPDATA ??
    process.env.XDG_RUNTIME_DIR ??
    (process.env.HOME ? path.join(process.env.HOME, ".local/state") : os.tmpdir());
  return path.join(base, "farseer", "runtime.json");
}

function runtime(): { port: number; token: string } {
  // An explicit environment wins: two daemons on one machine is a real case,
  // and this is how the operator says which one.
  if (process.env.FARSEER_PORT) {
    return { port: Number(process.env.FARSEER_PORT), token: process.env.FARSEER_TOKEN ?? "" };
  }
  try {
    return JSON.parse(readFileSync(runtimeFilePath(), "utf8")) as { port: number; token: string };
  } catch {
    console.warn(
      `[farseer] no runtime file at ${runtimeFilePath()} - start \`farseer serve\`, then restart this dev server`,
    );
    return { port: 9000, token: "" };
  }
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const { port, token } = runtime();

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
