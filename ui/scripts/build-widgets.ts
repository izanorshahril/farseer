/**
 * Compile every agent-authored widget into the canvas build.
 *
 * **Because the desktop app had none of them.** `28 operator surface`'s three
 * gates were built as a Vite dev-server plugin, so `/__widgets` existed only
 * under `bun run dev`. The shell serves the built canvas itself and answered
 * that path with `index.html`, which `App.tsx` failed to parse and swallowed -
 * so the whole feature was absent from the shipped application, silently, and
 * looked exactly like an operator who had written no widgets.
 *
 * Compiling here rather than in the shell keeps **gate 2 at compile time**,
 * which is what `28` asked for: the import allowlist is enforced by the build
 * that produces the bundle, not by something the app does at runtime with a
 * toolchain it would have to ship.
 *
 * The cost is stated rather than hidden: in the packaged app a *new* widget
 * needs a rebuild. Authoring stays a `bun run dev` loop, which is also where
 * gate 1's keep-or-undo lives.
 */
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { bundle, manifests } from "../plugins/widget-host";

const ui = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repo = path.dirname(ui);
const widgetsDir = path.join(repo, "widgets");
const out = path.join(ui, "dist", "widgets");

const found = await manifests(widgetsDir);
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });

const built = [];
for (const widget of found) {
  try {
    const code = await bundle(widgetsDir, widget.id, ui);
    await writeFile(path.join(out, `${widget.id}.js`), code, "utf8");
    built.push(widget);
  } catch (error) {
    // One widget that will not compile must not fail the canvas build, for the
    // same reason a directory without a manifest does not: the operator's other
    // widgets are not implicated in this one's mistake. Loud on the way past
    // rather than silent.
    console.error(`widget \`${widget.id}\` did not compile and was left out:`);
    console.error(`  ${(error as Error).message.split("\n")[0]}`);
  }
}

await writeFile(path.join(out, "index.json"), JSON.stringify(built, null, 2), "utf8");
console.log(`widgets: ${built.length} of ${found.length} compiled into dist/widgets`);
