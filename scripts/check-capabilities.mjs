/**
 * Fails if the webview cannot listen for events.
 *
 * Tauri v2 denies every core-plugin call from a webview unless a capability
 * grants it, and `listen` is one of those calls. The app shipped for eleven
 * milestones with no capability file at all: every `#[tauri::command]` worked,
 * because app commands are not governed by the ACL, and every event was
 * silently dropped. Device arrivals, live input, launch progress — all of it
 * dead, with nothing on screen or in the log to say so.
 *
 * That is the failure this guards: not a missing file, but a permission that
 * something later removes or narrows while the app still compiles, still
 * passes every test, and still looks like it works.
 */
import { readFileSync } from "node:fs";

const PATH = "src-tauri/gen/schemas/capabilities.json";

let capabilities;
try {
  capabilities = JSON.parse(readFileSync(PATH, "utf8"));
} catch (e) {
  console.error(`Could not read ${PATH}: ${e.message}`);
  console.error("It is written by tauri-build; run a cargo check first.");
  process.exit(1);
}

const entries = Object.values(capabilities);
if (entries.length === 0) {
  console.error(
    "No capabilities at all. Every event in the app is dead: the webview cannot\n" +
      "call `listen`, so device changes, live input and launch progress never\n" +
      "arrive. Add src-tauri/capabilities/default.json.",
  );
  process.exit(1);
}

// `core:default` contains `core:event:default`, which is what grants listen.
const grants = (permission) =>
  permission === "core:default" ||
  permission === "core:event:default" ||
  permission === "core:event:allow-listen";

const listening = entries.filter((c) =>
  (c.permissions ?? []).some((p) => grants(typeof p === "string" ? p : p.identifier)),
);

if (listening.length === 0) {
  console.error(
    "No capability grants `core:event`, so the webview cannot listen for events.\n" +
      "Everything event-driven in this app will fail silently.",
  );
  process.exit(1);
}

const windows = new Set(listening.flatMap((c) => c.windows ?? []));
for (const required of ["main", "splash"]) {
  if (!windows.has(required)) {
    console.error(`The "${required}" window is not covered by a capability that can listen.`);
    process.exit(1);
  }
}

console.log(
  `Capabilities OK — ${entries.length} capability, events allowed on ${[...windows].join(", ")}.`,
);
