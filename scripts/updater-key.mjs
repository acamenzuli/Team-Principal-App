/**
 * Generate the updater signing key, once, and wire its public half in.
 *
 * Updates are verified against a minisign public key **compiled into the app**.
 * That has a consequence worth understanding before running this: a build made
 * with one key can only be updated by releases signed with the matching private
 * key. Rotating the key later means every existing install stops updating and
 * has to be reinstalled by hand. So this refuses to overwrite a key that is
 * already configured unless you insist.
 *
 * The public half is written into `src-tauri/tauri.conf.json` and committed —
 * public keys are meant to be public, and committing it is what lets *every*
 * build check for updates, including the installer CI produces on each push.
 * The private half is written to a gitignored folder and must be copied into a
 * GitHub secret; it is the one thing here that must never be committed.
 *
 * Usage:  node scripts/updater-key.mjs [--force]
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const CONFIG = "src-tauri/tauri.conf.json";
const KEY_DIR = ".updater-key";
const KEY_FILE = join(KEY_DIR, "team-principal.key");
const UNCONFIGURED = "UNCONFIGURED";

const force = process.argv.includes("--force");
const config = JSON.parse(readFileSync(CONFIG, "utf8"));
const current = config.plugins?.updater?.pubkey ?? "";

if (current && current !== UNCONFIGURED && !force) {
  console.error(
    [
      "This app already has an updater public key.",
      "",
      "Replacing it would strand every copy already installed: those builds can",
      "only accept updates signed with the key they were built with, so they",
      "would go quiet rather than fail loudly, and the only fix is a manual",
      "reinstall on each machine.",
      "",
      "If you have lost the private key and accept that, run it again with --force.",
    ].join("\n"),
  );
  process.exit(1);
}

mkdirSync(KEY_DIR, { recursive: true });

// No password. The private key lives in a GitHub secret, which is the thing
// actually protecting it; a password stored in a second secret next to it adds
// a step without adding a secret-keeper. Pass one here if you would rather.
const password = process.env.TP_KEY_PASSWORD ?? "";

console.log("Generating a signing keypair…");
execFileSync(
  process.platform === "win32" ? "npx.cmd" : "npx",
  ["tauri", "signer", "generate", "-w", KEY_FILE, "-p", password, "--force", "--ci"],
  { stdio: ["ignore", "ignore", "inherit"] },
);

const privateKey = readFileSync(KEY_FILE, "utf8").trim();
const publicKey = readFileSync(`${KEY_FILE}.pub`, "utf8").trim();

config.plugins ??= {};
config.plugins.updater ??= {};
config.plugins.updater.pubkey = publicKey;
writeFileSync(CONFIG, `${JSON.stringify(config, null, 2)}\n`);

console.log(
  [
    "",
    `Public key written into ${CONFIG}. Commit that — it is meant to be public,`,
    "and it is what lets every build, including the one CI puts on the Actions",
    "tab, check for and verify updates.",
    "",
    `Private key written to ${KEY_FILE}, which is gitignored. Do not commit it,`,
    "and keep a copy somewhere you will still have in a year: losing it means no",
    "existing install can ever be updated again.",
    "",
    "Now add one repository secret on GitHub —",
    "  Settings → Secrets and variables → Actions → New repository secret",
    "",
    "  Name:  TAURI_SIGNING_PRIVATE_KEY",
    "  Value: (the whole line below, exactly)",
    "",
    privateKey,
    "",
    password
      ? "Also add TAURI_SIGNING_PRIVATE_KEY_PASSWORD with the password you set."
      : "No password was set, so TAURI_SIGNING_PRIVATE_KEY_PASSWORD is not needed.",
    "",
    "Then: commit, push, and tag a version to publish a release. docs/RELEASING.md",
    "has the whole sequence.",
  ].join("\n"),
);
