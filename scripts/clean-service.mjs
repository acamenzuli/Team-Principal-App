/**
 * Remove generated Python caches from `voice-service/` before a build.
 *
 * The installer bundles that folder wholesale, so that a module added to the
 * service is carried without anybody having to remember to list it. The cost
 * of a whole-folder rule is that it also carries whatever the folder has
 * collected — running the tests once leaves `__pycache__` and
 * `.pytest_cache`, and those went into a real installer before this existed.
 *
 * So the folder is swept first. Only generated caches are removed; nothing
 * here touches source, and everything it deletes is already gitignored.
 */
import { readdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = "voice-service";
const GENERATED = new Set(["__pycache__", ".pytest_cache", ".ruff_cache", ".mypy_cache"]);

let removed = 0;

function sweep(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return; // the folder is not there; nothing to sweep
  }
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const path = join(dir, entry.name);
    if (GENERATED.has(entry.name)) {
      rmSync(path, { recursive: true, force: true });
      removed += 1;
      continue;
    }
    // A virtualenv inside the project would be gigabytes; never descend into
    // one, and never bundle one.
    if (entry.name === ".venv" || entry.name === "node_modules") {
      rmSync(path, { recursive: true, force: true });
      removed += 1;
      continue;
    }
    sweep(path);
  }
}

try {
  statSync(ROOT);
} catch {
  console.log(`${ROOT} is not here; nothing to clean.`);
  process.exit(0);
}

sweep(ROOT);
console.log(
  removed === 0
    ? "voice-service is clean; nothing to remove."
    : `Removed ${removed} generated folder${removed === 1 ? "" : "s"} from voice-service.`,
);
