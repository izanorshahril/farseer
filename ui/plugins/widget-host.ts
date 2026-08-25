import { execFile } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { promisify } from "node:util";
import path from "node:path";
import { build } from "esbuild";
import type { Connect, Plugin } from "vite";

const run = promisify(execFile);

/**
 * The widget host: compile, gate, and answer for agent-authored widgets.
 *
 * `28 operator surface` section 3 put widget code in `widgets/` in git, written
 * by cell zero, and farseer never stores or serves it. This plugin is the
 * **client-side loader** that ruling permits - `01 cell primitive` ruled out a
 * plugin ABI in the *runtime*, and the runtime here still knows nothing about
 * widgets.
 *
 * Two of `28`'s three gates live in this file:
 *
 * - **Gate 2, the import allowlist**, enforced at compile by refusing to resolve
 *   anything outside it. It is the only gate that constrains what code *can do*
 *   at author time rather than hoping about runtime.
 * - **Gate 1, keep or undo**, which is `12 autonomy and deny list`'s `reversible`
 *   level expressed in the UI. Git is what makes it true, so these endpoints are
 *   thin wrappers over git scoped strictly to `widgets/`.
 *
 * Gate 3, the sandboxed render, is in `src/SandboxWidget.tsx` - it is a property
 * of how the bundle is *run*, not of how it is built.
 */

/** `28 operator surface` gate 2. Everything else fails to build. */
const ALLOWED_IMPORTS = new Set([
  "react",
  "react/jsx-runtime",
  "react/jsx-dev-runtime",
  "react-dom/client",
]);

export type WidgetManifest = {
  id: string;
  title: string;
  subtitle: string;
  /** What the widget claims it fronts. `28`: a widget displays a cell. */
  cell?: string;
};

async function git(repo: string, args: string[]) {
  const { stdout } = await run("git", ["-C", repo, ...args], { windowsHide: true });
  return stdout;
}

async function manifests(widgetsDir: string): Promise<WidgetManifest[]> {
  let entries: string[];
  try {
    entries = (await readdir(widgetsDir, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name);
  } catch {
    return [];
  }
  const found: WidgetManifest[] = [];
  for (const id of entries) {
    try {
      const raw = await readFile(path.join(widgetsDir, id, "widget.json"), "utf8");
      const parsed = JSON.parse(raw) as Partial<WidgetManifest>;
      found.push({
        id,
        title: parsed.title ?? id,
        subtitle: parsed.subtitle ?? "",
        ...(parsed.cell ? { cell: parsed.cell } : {}),
      });
    } catch {
      // A directory without a readable manifest is not a widget. Skipping it
      // beats failing the whole canvas over one bad folder.
    }
  }
  return found;
}

/**
 * Compile one widget, refusing any import outside the allowlist.
 *
 * React is bundled **into** the widget rather than shared with the host, because
 * gate 3 runs it in an opaque-origin frame that cannot fetch anything at all.
 * The bundle has to be self-contained or it cannot run.
 */
async function bundle(widgetsDir: string, id: string, hostDir: string): Promise<string> {
  const root = path.join(widgetsDir, id);
  const result = await build({
    entryPoints: [path.join(root, "widget.tsx")],
    bundle: true,
    write: false,
    format: "esm",
    target: "es2022",
    jsx: "automatic",
    // Not minified on purpose: the operator is about to be asked to keep or
    // undo this code, and unreadable code is a worse thing to be asked about.
    minify: false,
    absWorkingDir: path.resolve(widgetsDir, ".."),
    plugins: [
      {
        name: "farseer-import-allowlist",
        setup(build) {
          build.onResolve({ filter: /.*/ }, (args) => {
            if (args.kind === "entry-point") return null;
            // `build.resolve` re-runs the whole plugin chain, so without this
            // the allowlist below calls itself forever. The marker says "this
            // one already passed the gate".
            if ((args.pluginData as { gated?: boolean } | undefined)?.gated) return null;
            // The gate governs what the **widget** reaches. Once `react` is
            // allowed, react's own internals are downstream of that decision -
            // policing them would only mean the allowlist cannot allow anything
            // real.
            if (!args.importer.startsWith(root + path.sep) && args.importer !== root) return null;
            const specifier = args.path;
            // An allowed bare import resolves from the **host's** dependencies,
            // not the widget's: `widgets/` holds source and nothing else - no
            // package.json, no node_modules, nothing for a widget to install.
            // The cost is that a widget shares the host's React version, which
            // is the same bargain baby-menu makes and is worth stating.
            if (ALLOWED_IMPORTS.has(specifier)) {
              return build.resolve(specifier, {
                resolveDir: hostDir,
                kind: "import-statement",
                pluginData: { gated: true },
              });
            }
            // A widget's own files, and nothing above its own directory.
            if (specifier.startsWith(".")) {
              const resolved = path.resolve(args.resolveDir, specifier);
              if (resolved.startsWith(root + path.sep) || resolved === root) return null;
              return {
                errors: [
                  {
                    text: `\`${specifier}\` reaches outside the widget's own directory`,
                  },
                ],
              };
            }
            return {
              errors: [
                {
                  text:
                    `\`${specifier}\` is not on the import allowlist. A widget may import ` +
                    `${[...ALLOWED_IMPORTS].join(", ")} and its own local files.`,
                },
              ],
            };
          });
        },
      },
    ],
  });
  return result.outputFiles?.[0]?.text ?? "";
}

function json(response: Parameters<Connect.NextHandleFunction>[1], status: number, body: unknown) {
  response.statusCode = status;
  response.setHeader("content-type", "application/json");
  response.end(JSON.stringify(body));
}

export function widgetHost(options: { widgetsDir: string; repo: string; hostDir: string }): Plugin {
  const { widgetsDir, repo, hostDir } = options;
  return {
    name: "farseer-widget-host",
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const url = request.url ?? "";
        if (!url.startsWith("/__widgets")) return next();

        try {
          if (url === "/__widgets") {
            return json(response, 200, await manifests(widgetsDir));
          }

          // Gate 1: what has changed under `widgets/` since the last keep.
          if (url === "/__widgets/changes") {
            const status = await git(repo, ["status", "--porcelain", "--", "widgets"]);
            const changes = status
              .split("\n")
              .filter(Boolean)
              .map((line) => ({ state: line.slice(0, 2).trim(), file: line.slice(3) }));
            return json(response, 200, changes);
          }

          if (url === "/__widgets/keep" && request.method === "POST") {
            await git(repo, ["add", "--", "widgets"]);
            return json(response, 200, { kept: true });
          }

          // Scoped hard to `widgets/`: this throws away work, and the blast
          // radius is the only thing standing between "undo" and "disaster".
          if (url === "/__widgets/undo" && request.method === "POST") {
            await git(repo, ["restore", "--worktree", "--staged", "--", "widgets"]);
            await git(repo, ["clean", "-fd", "--", "widgets"]);
            return json(response, 200, { undone: true });
          }

          const bundleMatch = /^\/__widgets\/([a-z0-9-]+)\/bundle$/.exec(url);
          if (bundleMatch?.[1]) {
            const code = await bundle(widgetsDir, bundleMatch[1], hostDir);
            response.statusCode = 200;
            response.setHeader("content-type", "text/plain; charset=utf-8");
            return response.end(code);
          }
        } catch (error) {
          // A widget that will not compile is a widget that does not mount, and
          // the operator gets the compiler's own words rather than "failed".
          return json(response, 422, { error: (error as Error).message });
        }
        return next();
      });
    },
  };
}
