import { useCallback, useEffect, useState } from "react";

import { Dashboard } from "./dashboard/Dashboard";
import { applyTheme } from "./design/theme";
import { Peripherals } from "./devices/Peripherals";
import { ScreenSetup } from "./screen/ScreenSetup";
import { Settings } from "./settings/Settings";
import { WindowControl } from "./windowctl/WindowControl";
import {
  appInfo,
  asIpcError,
  getPreferences,
  savePreferences,
  type AppInfo,
  type Preferences,
} from "./ipc";
import "./app.css";

type View = "rig" | "screen" | "devices" | "windows" | "settings";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [prefs, setPrefs] = useState<Preferences | null>(null);
  const [prefsProblem, setPrefsProblem] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("rig");

  useEffect(() => {
    appInfo()
      .then(setInfo)
      .catch((e) => setError(asIpcError(e).message));

    getPreferences()
      .then((loaded) => {
        setPrefs(loaded.preferences);
        setPrefsProblem(loaded.problem);
        applyTheme(loaded.preferences, loaded.accentForeground);
      })
      .catch((e) => setError(asIpcError(e).message));
  }, []);

  // Every settings change writes through to disk and re-themes immediately, so
  // the control and its effect are never out of step and there is no Save
  // button to forget.
  const updatePrefs = useCallback(async (next: Preferences) => {
    const saved = await savePreferences(next);
    setPrefs(saved.preferences);
    applyTheme(saved.preferences, saved.accentForeground);
  }, []);

  if (error) {
    return (
      <div className="app">
        <main className="app__body">
          <p className="app__error">
            <strong>Team Principal could not start.</strong> {error}
          </p>
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="app__bar glass">
        <div className="brand">
          {prefs?.team.logo ? (
            <img className="brand__logo" src={prefs.team.logo.dataUri} alt="" />
          ) : (
            <span className="brand__mark" aria-hidden="true" />
          )}
          <span className="brand__names">
            <span className="brand__product">TEAM PRINCIPAL</span>
            {prefs?.team.name && <span className="brand__team">{prefs.team.name}</span>}
          </span>
        </div>

        <nav className="tabs" aria-label="Sections">
          <Tab id="rig" current={view} onSelect={setView}>
            Displays
          </Tab>
          <Tab id="screen" current={view} onSelect={setView}>
            Screen Setup
          </Tab>
          <Tab id="devices" current={view} onSelect={setView}>
            Peripherals
          </Tab>
          <Tab id="windows" current={view} onSelect={setView}>
            Windows
          </Tab>
          <Tab id="settings" current={view} onSelect={setView}>
            Settings
          </Tab>
        </nav>

        <span className="app__spacer" />

        {info?.simulated && (
          /* Never let a fixture screenshot pass for real hardware. */
          <span className="badge badge--warn">▲ SIMULATED — fixture data, not your rig</span>
        )}
        {info && !info.dpiAwarenessOk && (
          <span className="badge badge--fail" title={info.dpiAwareness}>
            ■ DPI awareness is {info.dpiAwareness} — geometry values are unreliable
          </span>
        )}
        <span className="app__version num">
          {info ? `v${info.version} · M${info.milestone}` : "…"}
        </span>
      </header>

      <main className="app__body">
        {prefsProblem && (
          <p className="app__notice">
            <span aria-hidden="true">▲</span> {prefsProblem}
          </p>
        )}
        {view === "rig" && <Dashboard />}
        {view === "devices" && <Peripherals />}
        {view === "windows" && <WindowControl />}
        {view === "screen" &&
          (prefs ? <ScreenSetup unit={prefs.units} /> : <p className="note">Loading…</p>)}
        {view === "settings" &&
          (prefs ? (
            <Settings prefs={prefs} onChange={updatePrefs} />
          ) : (
            <p className="note">Loading preferences…</p>
          ))}
      </main>
    </div>
  );
}

function Tab(props: {
  id: View;
  current: View;
  onSelect: (v: View) => void;
  children: React.ReactNode;
}) {
  const on = props.id === props.current;
  return (
    <button
      className={`tab${on ? " tab--on" : ""}`}
      aria-current={on ? "page" : undefined}
      onClick={() => props.onSelect(props.id)}
    >
      {props.children}
    </button>
  );
}
