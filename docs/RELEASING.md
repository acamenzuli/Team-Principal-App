# Releasing

Two different signatures are involved and they solve different problems. Mixing
them up is the expensive mistake here, so this document separates them first and
does everything else afterwards.

| | Updater signing key | Code-signing certificate |
| --- | --- | --- |
| **Stops** | somebody pushing a fake update to your customers | Windows SmartScreen warning on install |
| **Costs** | nothing | about €120/year |
| **Comes from** | `node scripts/updater-key.mjs` | a certificate authority |
| **Lives** | GitHub Actions secret | Azure, or a USB token |
| **Needed for** | the update button to work at all | a clean first install |

You can have either without the other. Do the updater key first: it is free, it
takes two minutes, and **it has to exist before the first release you give
anybody**, because the key is compiled into the app and a build made without one
can never be updated by a build made with one.

---

## 1. The updater key (free, do this first)

One command, from the repository root:

```powershell
node scripts/updater-key.mjs
```

It generates the keypair, writes the **public** half into
`src-tauri/tauri.conf.json`, and prints the **private** half once.

- **Commit the config change.** The public key is meant to be public, and
  committing it is what lets every build check for updates — including the
  installer CI produces on every push, which is the one you test with.
- **Add the private half as a secret.** Repository **Settings → Secrets and
  variables → Actions → New repository secret**, named
  `TAURI_SIGNING_PRIVATE_KEY`. That is the only secret needed.
- **Back the private key up** somewhere that is not this machine and not this
  repository. It cannot be regenerated: lose it and no existing install can
  ever be updated again, and the only fix is asking every customer to reinstall
  by hand. The script refuses to replace an existing key for that reason.

The script sets no password. The secret store is what protects the key; a
password in a second secret beside it adds a step without adding a keeper. To
use one anyway, set `TP_KEY_PASSWORD` before running the script and add it as
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

---

## Cutting a release

Everything is driven by the tag. There is no version to edit by hand — the
workflow takes it from the tag and writes it into both `tauri.conf.json` and
`Cargo.toml`, which is what stops the classic mistake of releasing the same
version twice and watching every app report "up to date" forever.

```powershell
git tag v0.1.1
git push origin v0.1.1
```

Then, on GitHub:

1. Watch **Actions → Release** finish. It builds, signs, and creates a release.
2. The release is a **draft**. Install the `.exe` from it and check it starts.
3. Press **Publish release**.

**Step 3 is not optional.** A draft is not `releases/latest`, so the updater
cannot see it — every installed copy will go on reporting *Up to date* until
the release is published. The workflow prints this as its run summary too,
because it is the one step that fails silently.

---

## Testing that updates actually work

The full loop, once the key exists:

1. Install the current build — Actions → newest CI run → Artifacts →
   `team-principal-installer`. Check **Settings → Updates** shows its version
   and that **Check now** says *Up to date* rather than "no signing key".
2. Tag the next version and publish the release as above.
3. Back in the installed app, press **Check now**. It should find the new
   version. Press **Install and restart**: the strip reports downloading,
   verifying, then restarting, and the app comes back on the new version with
   every rig, profile and backup exactly as it was.

If step 1 says *this build has no update signing key*, the installer predates
the key — rebuild by pushing the committed config change and use the new
artifact.

Two things that look like bugs and are not:

- **Windows asks for permission during the update.** The app installs for all
  users, so replacing it needs an administrator, exactly as the first install
  did. Answering the prompt is part of the flow.
- **SmartScreen warns again on the new version.** Until the code-signing
  certificate below exists, every build is unsigned as far as Windows is
  concerned, including updates. The updater signature is a different mechanism
  and Windows knows nothing about it.

---

## 2. The code-signing certificate

**Yes, it can be in your own name.** A certificate authority validates the
identity of a person just as it does a company, and the name on the certificate
is the name Windows shows in the "Verified publisher" line of the install
prompt. For a one-person product that is the right answer — it says who wrote
this, which is exactly what the prompt is asking.

### Azure Artifact Signing (formerly Trusted Signing) — the recommended route

- **About €9/month**, on an Azure subscription.
- **Open to self-employed individuals.** Microsoft dropped the earlier rule
  requiring three years of trading history, and the service covers the US,
  Canada, the EU and the UK — Malta qualifies.
- Microsoft validates your identity once, then issues short-lived certificates
  on demand from a certificate authority Windows already trusts. There is no
  USB token to keep in a drawer and nothing to renew by hand.

Two things to know before signing up:

1. **Identity validation takes a few business days.** Start it before you need
   it, not the week you want to ship.
2. At least one developer has reported the portal wanting a Microsoft Entra ID
   P2 licence to assign the signing role, which is an extra cost the €9 plan
   does not mention. Check that on the free trial before committing.

### The alternative

A traditional OV certificate from Sectigo or DigiCert, about €200–400/year. Since
2023 the private key must live on FIPS-certified hardware, so it arrives as a
USB token you physically hold — which also means CI cannot sign without it
plugged into a machine you control. Workable, and more friction than the above.

### What signing does not do on day one

An OV certificate does not give instant SmartScreen trust. Reputation builds
over installs, so the first few users may still see a warning. An EV certificate
does grant it immediately and costs roughly double. For a product selling in
small numbers to people who were told about it, OV is the sensible trade.

### Wiring it up

Azure Artifact Signing plugs into the release workflow as an extra step after
the build. Add it once the account exists — the workflow is already structured
so that signing is a step rather than a rewrite.

---

## 3. Cutting a release

```powershell
# 1. Bump the version. These two must match, and CI does not check it for you.
#    src-tauri/tauri.conf.json  ->  "version"
#    src-tauri/Cargo.toml       ->  package.version

git commit -am "release: v0.2.0"
git tag v0.2.0
git push origin main --tags
```

The `Release` workflow builds on Windows, signs with the updater key, and
publishes a **draft** release with the installer, the signature and
`latest.json`. Draft on purpose: it is the last point at which a release can be
reconsidered without anybody having downloaded it.

Two settings are switched on by that workflow rather than committed —
`createUpdaterArtifacts`, and the public key itself. Both make a build *require*
the private key, and the ordinary CI build has no key and must not have one: it
produces the installer people download for testing on every push. Committing
either would break that build, which is exactly what happened the first time.

Check the draft, then publish it. The moment you do, every running copy of the
app finds it on its next start.

---

## 4. What an update does to a customer's machine

It replaces the program files and **nothing else**.

Everything the app owns lives in `%APPDATA%\Team Principal` — rigs, game
profiles, display snapshots, config backups, preferences, the machine id, logs.
The installer does not touch that directory, which is why an update keeps
every setting, and why a reinstall does too.

The app checks on every start unless told not to. What happens next is the
customer's choice, in Settings:

- **Tell me** — a strip at the top of the window, and nothing happens until they
  press the button. The default.
- **Install it** — downloaded and installed on its own, then a restart.
- **Do nothing** — no check at all.

**Install it** still refuses while a session is in flight. Restarting the
launcher out from under a running race would tear down everything it started,
and the marker that says a session is live is already on disk for exactly this
kind of question.
