import { useEffect, useMemo, useState } from "react";

import {
  asIpcError,
  deleteProfile,
  listDevices,
  listRunningApps,
  saveProfile,
  type DetectedDevice,
  type Necessity,
  type PeripheralRequirement,
  type Profile,
  type ReadinessGate,
  type RunningApp,
  type UtilitySpec,
} from "../ipc";

/**
 * What a profile says about a session, in the two terms that matter: which
 * peripherals have to be there, and what has to be running first.
 *
 * Peripherals are *picked from what is plugged in*, never typed. A VID/PID
 * entered by hand is a VID/PID entered wrongly, and the device list already
 * knows the answer — including the instance path, which is the only thing that
 * separates two identical un-serialled pedal sets.
 *
 * Everything else on a profile — window plan, rig binding, session mode — is
 * derived from the rig or arrives with the adapters. Nothing here asks the user
 * for a number the app can work out for itself.
 */
export function ProfileEditor({
  profile,
  onSaved,
  onDeleted,
  onClose,
}: {
  profile: Profile;
  onSaved: (profile: Profile) => void;
  onDeleted: (id: string) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<Profile>(profile);
  const [devices, setDevices] = useState<DetectedDevice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [picking, setPicking] = useState(false);

  useEffect(() => {
    listDevices()
      .then(setDevices)
      .catch(() => setDevices([]));
  }, []);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(profile),
    [draft, profile],
  );

  async function save() {
    setSaving(true);
    try {
      onSaved(await saveProfile(draft));
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    try {
      await deleteProfile(draft.id);
      onDeleted(draft.id);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }

  function requirementFor(device: DetectedDevice): PeripheralRequirement | undefined {
    return draft.peripherals.find((p) => sameDevice(p, device));
  }

  function setNecessity(device: DetectedDevice, necessity: Necessity | null) {
    setDraft((d) => {
      const rest = d.peripherals.filter((p) => !sameDevice(p, device));
      if (necessity === null) return { ...d, peripherals: rest };
      const existing = d.peripherals.find((p) => sameDevice(p, device));
      const updated: PeripheralRequirement = existing
        ? { ...existing, necessity }
        : {
            device: device.device,
            necessity,
            // Slot and GUID are recorded when a session actually runs, not
            // guessed here — a wrong expectation would report drift that
            // never happened.
            expectDinputSlot: null,
            expectInstanceGuid: null,
            vendorProcess: null,
            vjoy: null,
          };
      return { ...d, peripherals: [...rest, updated] };
    });
  }

  return (
    <div className="pe">
      <header className="pe__head">
        <div>
          <h3 className="pe__title">{draft.name}</h3>
          <p className="pe__sub num">{describeLaunch(draft)}</p>
        </div>
        <div className="pe__head-actions">
          <button className="btn" disabled={!dirty || saving} onClick={() => void save()}>
            {saving ? "Saving…" : dirty ? "Save profile" : "Saved"}
          </button>
          <button className="btn btn--quiet" onClick={onClose}>
            Close
          </button>
        </div>
      </header>

      {error && <p className="warn warn--hard">{error}</p>}

      <section className="pe__block">
        <h4 className="pe__block-title">Peripherals</h4>
        <p className="note">
          Required blocks the launch. Optional warns and lets you race. Anything left off is not
          checked at all — a preflight that tests things nobody asked for is one people learn to
          scroll past.
        </p>

        {devices.length === 0 ? (
          <p className="note note--pending">
            Nothing detected yet. Plug a wheel or pedals in and this list fills itself.
          </p>
        ) : (
          <ul className="pe__devices">
            {devices.map((d) => {
              const requirement = requirementFor(d);
              const necessity = requirement?.necessity ?? null;
              return (
                <li className="pe__device" key={d.device.instancePath ?? d.device.displayName}>
                  <span className="pe__device-name">
                    {d.device.displayName}
                    {d.isVirtual && <span className="tag">virtual</span>}
                  </span>
                  <span className="pe__necessity" role="radiogroup" aria-label={d.device.displayName}>
                    {NECESSITY.map(([value, label]) => (
                      <button
                        key={label}
                        role="radio"
                        aria-checked={necessity === value}
                        className={`pe__choice${necessity === value ? " pe__choice--on" : ""}`}
                        onClick={() => setNecessity(d, value)}
                      >
                        {label}
                      </button>
                    ))}
                  </span>
                </li>
              );
            })}
          </ul>
        )}

        {orphaned(draft, devices).map((p) => (
          <p className="note note--fail" key={p.device.displayName}>
            {p.device.displayName} is required by this profile but is not plugged in now.{" "}
            <button className="btn btn--quiet" onClick={() => dropRequirement(setDraft, p)}>
              Remove it
            </button>
          </p>
        ))}
      </section>

      <section className="pe__block">
        <h4 className="pe__block-title">Utilities</h4>
        <p className="note">
          Started before the game, in the order their dependencies allow. Each one says how to know
          it is <em>ready</em> rather than merely running — that condition is what replaces the
          fixed wait every other launcher relies on.
        </p>

        {draft.utilities.map((utility, i) => (
          <UtilityRow
            key={i}
            utility={utility}
            others={draft.utilities.filter((_, j) => j !== i).map((u) => u.label)}
            onChange={(next) =>
              setDraft((d) => ({
                ...d,
                utilities: d.utilities.map((u, j) => (j === i ? next : u)),
              }))
            }
            onRemove={() =>
              setDraft((d) => ({ ...d, utilities: d.utilities.filter((_, j) => j !== i) }))
            }
          />
        ))}

        <div className="pe__actions">
          {/* Start it, then point at it. Typing a path from memory is how you
              get a profile that fails its own check, and a catalog of known
              utilities would guess at paths and go stale. */}
          <button className="btn" onClick={() => setPicking(true)}>
            Add software that's running
          </button>
          <button
            className="btn btn--quiet"
            onClick={() => setDraft((d) => ({ ...d, utilities: [...d.utilities, blankUtility()] }))}
          >
            Add one by hand
          </button>
        </div>
      </section>

      {picking && (
        <RunningPicker
          already={draft.utilities.map((u) => u.exePath)}
          onClose={() => setPicking(false)}
          onPick={(app) => {
            setDraft((d) => ({ ...d, utilities: [...d.utilities, utilityFor(app)] }));
            setPicking(false);
          }}
        />
      )}

      <footer className="pe__foot">
        <button className="btn btn--quiet" onClick={() => void remove()}>
          Delete this profile
        </button>
      </footer>
    </div>
  );
}

/**
 * Pick a running program to require before a race.
 *
 * Everything on this machine, right now — not a list of utilities somebody
 * thought of in advance. SimHub, a dashboard, a telemetry tool, a script
 * written last weekend: if it is running, it can be required.
 */
function RunningPicker({
  already,
  onPick,
  onClose,
}: {
  already: string[];
  onPick: (app: RunningApp) => void;
  onClose: () => void;
}) {
  const [apps, setApps] = useState<RunningApp[] | null>(null);
  const [filter, setFilter] = useState("");
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    listRunningApps()
      .then(setApps)
      .catch((e) => setProblem(asIpcError(e).message));
  }, []);

  const shown = (apps ?? []).filter((a) =>
    `${a.label} ${a.exePath}`.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <div className="picker" role="dialog" aria-label="Software running now">
      <header className="picker__head">
        <h4 className="picker__title">Software running now</h4>
        <button className="btn btn--quiet btn--tiny" onClick={onClose}>
          Close
        </button>
      </header>

      <input
        className="picker__filter"
        placeholder="Type to narrow the list — simhub, crew, dash…"
        value={filter}
        autoFocus
        onChange={(e) => setFilter(e.target.value)}
      />

      {problem && <p className="warn warn--hard">{problem}</p>}
      {apps === null && !problem && <p className="note">Looking…</p>}
      {apps !== null && shown.length === 0 && (
        <p className="note">
          Nothing matches. Start the program first — this lists what is running now, so the path
          it records is one that has actually worked.
        </p>
      )}

      <ul className="picker__list">
        {shown.map((app) => {
          const have = already.some((p) => p.toLowerCase() === app.exePath.toLowerCase());
          return (
            <li key={app.exePath}>
              <button className="picker__row" disabled={have} onClick={() => onPick(app)}>
                <span className="picker__name">{app.label}</span>
                <span className="picker__path num">{app.exePath}</span>
                {have && <span className="tag">already required</span>}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function UtilityRow({
  utility,
  others,
  onChange,
  onRemove,
}: {
  utility: UtilitySpec;
  others: string[];
  onChange: (next: UtilitySpec) => void;
  onRemove: () => void;
}) {
  const exe = fileName(utility.exePath);

  return (
    <div className="pe__utility">
      <div className="field">
        <label className="field__label">Name</label>
        <input
          className="text-input"
          value={utility.label}
          placeholder="SimHub"
          onChange={(e) => onChange({ ...utility, label: e.target.value })}
        />
      </div>

      <div className="field field--wide">
        <label className="field__label">Program</label>
        <input
          className="text-input num"
          value={utility.exePath}
          placeholder="C:\Program Files (x86)\SimHub\SimHubWPF.exe"
          onChange={(e) => onChange({ ...utility, exePath: e.target.value })}
        />
      </div>

      <div className="field field--wide">
        <label className="field__label">Ready when</label>
        <GateChooser
          gate={utility.readyWhen}
          exe={exe}
          onChange={(readyWhen) => onChange({ ...utility, readyWhen })}
        />
      </div>

      <div className="field">
        <label className="field__label">Give up after</label>
        <span className="field__row">
          <input
            className="text-input num"
            type="number"
            min={1}
            value={Math.round(utility.timeoutMs / 1000)}
            onChange={(e) =>
              onChange({ ...utility, timeoutMs: Math.max(1, Number(e.target.value)) * 1000 })
            }
          />
          <span className="field__suffix">seconds</span>
        </span>
      </div>

      <div className="pe__utility-flags">
        <label className="field__check">
          <input
            type="checkbox"
            checked={utility.required}
            onChange={(e) => onChange({ ...utility, required: e.target.checked })}
          />
          Blocks the launch if it fails
        </label>

        {others.length > 0 && (
          <label className="field__check">
            <span>Start after</span>
            <select
              className="text-input"
              value={utility.after[0] ?? ""}
              onChange={(e) =>
                onChange({ ...utility, after: e.target.value ? [e.target.value] : [] })
              }
            >
              <option value="">nothing in particular</option>
              {others.map((label) => (
                <option key={label} value={label}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}

        <button className="btn btn--quiet" onClick={onRemove}>
          Remove
        </button>
      </div>
    </div>
  );
}

/**
 * The readiness conditions worth offering by hand.
 *
 * The model expresses far more — composed gates, named mutexes and events —
 * and a profile written by an adapter will use them. These four are the ones a
 * person can answer about their own utility without reading its source.
 */
function GateChooser({
  gate,
  exe,
  onChange,
}: {
  gate: ReadinessGate;
  exe: string;
  onChange: (gate: ReadinessGate) => void;
}) {
  return (
    <div className="pe__gate">
      <select
        className="text-input"
        value={gate.kind}
        onChange={(e) => onChange(gateOfKind(e.target.value, gate, exe))}
      >
        <option value="process_exists">its program is running</option>
        <option value="tcp_port">it answers on a port</option>
        <option value="file_appears">a file appears</option>
        <option value="delay">a fixed wait</option>
        <option value="immediate">straight away</option>
      </select>

      {gate.kind === "process_exists" && (
        <input
          className="text-input num"
          value={gate.exe}
          placeholder="SimHubWPF.exe"
          onChange={(e) => onChange({ kind: "process_exists", exe: e.target.value })}
        />
      )}

      {gate.kind === "tcp_port" && (
        <>
          <input
            className="text-input num"
            value={gate.host}
            onChange={(e) => onChange({ ...gate, host: e.target.value })}
          />
          <input
            className="text-input num"
            type="number"
            value={gate.port}
            onChange={(e) => onChange({ ...gate, port: Number(e.target.value) })}
          />
        </>
      )}

      {gate.kind === "file_appears" && (
        <input
          className="text-input num"
          value={gate.path}
          placeholder="C:\ProgramData\...\ready.lock"
          onChange={(e) => onChange({ kind: "file_appears", path: e.target.value })}
        />
      )}

      {gate.kind === "delay" && (
        <span className="field__row">
          <input
            className="text-input num"
            type="number"
            min={0}
            value={Math.round(gate.ms / 1000)}
            onChange={(e) => onChange({ kind: "delay", ms: Number(e.target.value) * 1000 })}
          />
          <span className="field__suffix">seconds</span>
        </span>
      )}

      {gate.kind === "delay" && (
        <p className="note note--fail">
          A fixed wait is a guess. It is here because some utilities signal nothing at all — but if
          yours writes a file or opens a port, say so instead and the preflight gets both faster and
          more honest.
        </p>
      )}
    </div>
  );
}

function gateOfKind(kind: string, previous: ReadinessGate, exe: string): ReadinessGate {
  switch (kind) {
    case "process_exists":
      return { kind, exe: previous.kind === "process_exists" ? previous.exe : exe };
    case "tcp_port":
      return { kind, host: "127.0.0.1", port: 8888 };
    case "file_appears":
      return { kind, path: "" };
    case "delay":
      return { kind, ms: 5000 };
    default:
      return { kind: "immediate" };
  }
}

/**
 * A utility built from a program that is running.
 *
 * Required by default, and checked by its executable name: that is the whole
 * point of adding it — "do not let me on track without this". Optional is a
 * deliberate downgrade rather than the starting position.
 */
function utilityFor(app: RunningApp): UtilitySpec {
  return {
    label: app.label,
    exePath: app.exePath,
    args: [],
    readyWhen: { kind: "process_exists", exe: app.exe },
    timeoutMs: 60_000,
    required: true,
    after: [],
  };
}

function blankUtility(): UtilitySpec {
  return {
    label: "",
    exePath: "",
    args: [],
    readyWhen: { kind: "process_exists", exe: "" },
    timeoutMs: 60_000,
    required: false,
    after: [],
  };
}

/** Requirements whose device is not plugged in, so the list cannot show them. */
function orphaned(profile: Profile, devices: DetectedDevice[]): PeripheralRequirement[] {
  return profile.peripherals.filter((p) => !devices.some((d) => sameDevice(p, d)));
}

function dropRequirement(
  setDraft: (f: (d: Profile) => Profile) => void,
  requirement: PeripheralRequirement,
) {
  setDraft((d) => ({
    ...d,
    peripherals: d.peripherals.filter((p) => p !== requirement),
  }));
}

/**
 * Instance path first, VID/PID second — the same rule the Rust gate uses, and
 * for the same reason: two identical un-serialled pedal sets differ only by
 * which port they are in.
 */
function sameDevice(requirement: PeripheralRequirement, device: DetectedDevice): boolean {
  const want = requirement.device;
  const have = device.device;
  if (want.instancePath && have.instancePath) return want.instancePath === have.instancePath;
  return want.vid === have.vid && want.pid === have.pid;
}

function describeLaunch(profile: Profile): string {
  const launch = profile.game.launch;
  switch (launch.kind) {
    case "executable":
      return launch.path;
    case "steam":
      return `steam://rungameid/${launch.appId}`;
    case "epic":
      return launch.appName;
    case "uwp":
      return launch.packageFamilyName;
    case "uri":
      return launch.uri;
  }
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

const NECESSITY: [Necessity | null, string][] = [
  [null, "Not checked"],
  ["optional", "Optional"],
  ["required", "Required"],
];
