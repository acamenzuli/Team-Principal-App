/**
 * Fails if the Rust command surface and the TypeScript client have drifted.
 *
 * ts-rs generates the payload *types* but knows nothing about command names, so
 * a renamed command would otherwise compile cleanly on both sides and fail only
 * when a user clicks the thing. This closes that gap.
 *
 * `pub async fn` is matched as well as `pub fn`. It was not, once, and the
 * first async command added was invisible to this check — a guard with a blind
 * spot is worse than no guard, because it is trusted.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

function walk(dir) {
  return readdirSync(dir).flatMap((e) => {
    const p = join(dir, e);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

const rustCommands = new Set();
for (const file of walk("src-tauri/src").filter((f) => f.endsWith(".rs"))) {
  const src = readFileSync(file, "utf8");
  for (const m of src.matchAll(/#\[tauri::command\]\s*(?:#\[[^\]]*\]\s*)*pub (?:async )?fn (\w+)/g)) {
    rustCommands.add(m[1]);
  }
}

const client = readFileSync("src/ipc.ts", "utf8");
// `[^(]*` rather than `[^>]*`: a generic argument can contain its own angle
// brackets — `invoke<Record<string, string>>` — and stopping at the first `>`
// made those calls invisible to this check. A guard with a blind spot is worse
// than no guard, because it is trusted; this is the second one found here.
const tsCommands = new Set([...client.matchAll(/invoke<[^(]*>\("(\w+)"/g)].map((m) => m[1]));

const missingInTs = [...rustCommands].filter((c) => !tsCommands.has(c));
const missingInRust = [...tsCommands].filter((c) => !rustCommands.has(c));

if (missingInTs.length || missingInRust.length) {
  if (missingInTs.length) {
    console.error(`Rust commands with no wrapper in src/ipc.ts: ${missingInTs.join(", ")}`);
  }
  if (missingInRust.length) {
    console.error(`src/ipc.ts calls commands that do not exist in Rust: ${missingInRust.join(", ")}`);
  }
  process.exit(1);
}

console.log(`IPC contract OK — ${rustCommands.size} commands, matched on both sides.`);
