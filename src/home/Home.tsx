import { useCallback, useEffect, useMemo, useState } from "react";

import {
  asIpcError,
  currentRig,
  desktopLayout,
  gameLibrary,
  listDevices,
  listMonitors,
  onDevicesChanged,
  panicHotkey,
  solveRig,
  type DesktopLayoutInfo,
  type DetectedDevice,
  type HotkeyInfo,
  type MonitorInfo,
  type ProfileCard,
  type RigModel,
  type RigSolutionInfo,
} from "../ipc";
import "./home.css";

/**
 * The landing page.
 *
 * Answers the three questions somebody has on opening this app, in the order
 * they have them: **is my rig as I left it, what can I race, and is anything
 * wrong.** Everything else is a tab.
 *
 * It is deliberately not a dashboard of tiles. A tile that reports a number
 * nobody acts on is decoration, so each block here is either something you
 * press or something that would stop you racing.
 */
export function Home({ onGo }: { onGo: (view: "games" | "screen" | "devices") => void }) {
  const [games, setGames] = useState<ProfileCard[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [devices, setDevices] = useState<DetectedDevice[]>([]);
  const [rig, setRig] = useState<RigModel | null>(null);
  const [solution, setSolution] = useState<RigSolutionInfo | null>(null);
  const [layout, setLayout] = useState<DesktopLayoutInfo | null>(null);
  const [hotkey, setHotkey] = useState<HotkeyInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [library, screens, hardware, r, desktop, keys] = await Promise.all([
        gameLibrary(),
        listMonitors(),
        listDevices(),
        currentRig(),
        desktopLayout(),
        panicHotkey(),
      ]);
      setGames(library);
      setMonitors(screens);
      setDevices(hardware);
      setRig(r);
      setLayout(desktop);
      setHotkey(keys);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Peripherals change while you are looking at this page more than anything
  // else does — that is the whole point of the watcher.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onDevicesChanged(setDevices).then((f) => (unlisten = f));
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!rig) return;
    let live = true;
    solveRig(rig, { kind: "full_span", fit: "letterbox" })
      .then((s) => live && setSolution(s))
      .catch(() => live && setSolution(null));
    return () => {
      live = false;
    };
  }, [rig]);

  const installed = useMemo(() => games.filter((g) => g.installed), [games]);
  const ready = useMemo(() => installed.filter((g) => g.profile.windowPlan.autoApply), [installed]);
  const connected = devices.filter((d) => d.status === "connected");

  // Only things that would actually stop or spoil a session. A list that
  // includes everything is a list nobody reads.
  const problems: { text: string; go?: () => void; label?: string }[] = [];
  if (rig && rig.screens.length === 0) {
    problems.push({
      text: "No screens described yet, so nothing can be worked out from your rig.",
      go: () => onGo("screen"),
      label: "Set up screens",
    });
  }
  const unmeasured = (rig?.screens ?? []).filter((s) => s.panel.visibleWidth.source !== "edid");
  if (rig && rig.screens.length > 0 && unmeasured.length > 0) {
    problems.push({
      text: `${unmeasured.length === 1 ? "One screen" : `${unmeasured.length} screens`} did not report a physical size, so ${unmeasured.length === 1 ? "it needs" : "they need"} measuring by hand — every angle depends on it.`,
      go: () => onGo("screen"),
      label: "Measure",
    });
  }
  if (devices.length > 0 && connected.length === 0) {
    problems.push({
      text: "Nothing is connected. Every peripheral this app can see is currently absent.",
      go: () => onGo("devices"),
      label: "Peripherals",
    });
  }
  for (const d of devices.filter((d) => d.bindingDrift !== null)) {
    problems.push({
      text: `${d.device.displayName} works, but its DirectInput slot has moved since it was last seen — which is what silently breaks game bindings.`,
      go: () => onGo("devices"),
      label: "Look",
    });
  }
  if (hotkey && !hotkey.registered) {
    problems.push({
      text: `The panic hotkey (${hotkey.combination}) could not be registered — something else on this machine owns it. Display changes still revert on their own.`,
    });
  }

  return (
    <div className="home">
      {error && <p className="warn warn--hard">{error}</p>}

      <section className="home__hero">
        <div className="home__lede">
          <h1 className="home__title">
            {ready.length > 0
              ? "Ready when you are."
              : installed.length > 0
                ? "Pick a game to set up."
                : "Let's get your rig described."}
          </h1>
          <p className="home__sub">
            {rig && solution && rig.screens.length > 0 ? (
              <>
                {rig.screens.length} {rig.screens.length === 1 ? "screen" : "screens"},{" "}
                <strong className="num">{solution.totalCoverageDeg.toFixed(0)}°</strong> of view
                from <strong className="num">{Math.round(rig.seating.eyeToCenter)} mm</strong>.
              </>
            ) : (
              "Describe your rig once and every game's settings follow from it."
            )}
          </p>
        </div>

        <button className="btn btn--go" onClick={() => onGo("games")}>
          {installed.length > 0 ? "Let's race" : "Find my games"}
        </button>
      </section>

      <div className="home__grid">
        <Panel
          title="Games"
          value={`${installed.length}`}
          unit={installed.length === 1 ? "installed" : "installed"}
          note={
            ready.length > 0
              ? `${ready.length} set up to place their window automatically`
              : installed.length > 0
                ? "None set up yet — run one, then Copy current layout"
                : "Nothing found yet"
          }
          onClick={() => onGo("games")}
        />

        <Panel
          title="Screens"
          value={`${monitors.length}`}
          unit={monitors.length === 1 ? "attached" : "attached"}
          note={
            layout && layout.deadRegions.length > 0
              ? `${layout.deadRegions.length} dead ${layout.deadRegions.length === 1 ? "region" : "regions"} — desktop space that maps to no glass`
              : monitors.length > 0
                ? "Tiling exactly, no dead space"
                : "None detected"
          }
          onClick={() => onGo("screen")}
        />

        <Panel
          title="Peripherals"
          value={`${connected.length}`}
          unit={`of ${devices.length} connected`}
          note={
            devices.length === 0
              ? "Plug a wheel in — the list fills itself"
              : `${devices.filter((d) => d.dinputPresent).length} visible to games`
          }
          onClick={() => onGo("devices")}
        />
      </div>

      {ready.length > 0 && (
        <section className="home__block">
          <h2 className="home__heading">Set up and ready</h2>
          <ul className="home__games">
            {ready.slice(0, 6).map((g) => (
              <li key={g.profile.id}>
                {g.art ? <img src={g.art} alt="" /> : <span className="home__mono">{g.profile.name[0]}</span>}
                <span>{g.profile.name}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {problems.length > 0 && (
        <section className="home__block">
          <h2 className="home__heading">Worth knowing</h2>
          <ul className="home__problems">
            {problems.map((p) => (
              <li key={p.text}>
                <span aria-hidden="true">▲</span>
                <span>{p.text}</span>
                {p.go && (
                  <button className="btn btn--quiet btn--tiny" onClick={p.go}>
                    {p.label}
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/** One readable fact, and the tab it belongs to. */
function Panel({
  title,
  value,
  unit,
  note,
  onClick,
}: {
  title: string;
  value: string;
  unit: string;
  note: string;
  onClick: () => void;
}) {
  return (
    <button className="hp" onClick={onClick}>
      <span className="hp__title">{title}</span>
      <span className="hp__value num">
        {value}
        <span className="hp__unit">{unit}</span>
      </span>
      <span className="hp__note">{note}</span>
    </button>
  );
}
