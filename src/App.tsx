import { useCallback, useEffect, useState } from "react";

import { Adapters } from "./adapters/Adapters";
import { Dashboard } from "./dashboard/Dashboard";
import { Home } from "./home/Home";
import { applyTheme } from "./design/theme";
import { Peripherals } from "./devices/Peripherals";
import { Games } from "./games/Games";
import { FirstRun } from "./onboarding/FirstRun";
import { Recovery } from "./onboarding/Recovery";
import { ScreenSetup } from "./screen/ScreenSetup";
import { Settings } from "./settings/Settings";
import { WindowControl } from "./windowctl/WindowControl";
import {
  appInfo,
  asIpcError,
  getPreferences,
  ready,
  savePreferences,
  type AppInfo,
  type Preferences,
} from "./ipc";
import "./app.css";

type View = "home" | "rig" | "screen" | "devices" | "games" | "adapters" | "windows" | "settings";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [prefs, setPrefs] = useState<Preferences | null>(null);
  const [prefsProblem, setPrefsProblem] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("home");

  useEffect(() => {
    // Both before the window is shown, so the first frame is the themed app
    // rather than a flash of default colours. The splash is covering this.
    Promise.all([appInfo(), getPreferences()])
      .then(([app, loaded]) => {
        setInfo(app);
        setPrefs(loaded.preferences);
        setPrefsProblem(loaded.problem);
        applyTheme(loaded.preferences, loaded.accentForeground);
      })
      .catch((e) => setError(asIpcError(e).message))
      // Whatever happened, show the window. A splash that never goes away
      // because something failed to load is the worst outcome available: the
      // error is invisible and the app looks hung.
      .finally(() => void ready());
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
        {/* Before anything else, including the wizard: an unfinished session has
          left this machine in a state the user did not choose, and that is more
          urgent than an introduction. */}
      <Recovery />

      {/* Shown over the app rather than instead of it, so it reads as a guide
          through this product rather than a wall in front of it. */}
      {prefs && !prefs.onboarded && (
        <FirstRun
          unit={prefs.units}
          onDone={() => void updatePrefs({ ...prefs, onboarded: true })}
        />
      )}

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
          <Tab id="home" current={view} onSelect={setView}>
            Home
          </Tab>
          <Tab id="rig" current={view} onSelect={setView}>
            Displays
          </Tab>
          <Tab id="screen" current={view} onSelect={setView}>
            Screen Setup
          </Tab>
          <Tab id="devices" current={view} onSelect={setView}>
            Peripherals
          </Tab>
          <Tab id="games" current={view} onSelect={setView}>
            Games
          </Tab>
          <Tab id="adapters" current={view} onSelect={setView}>
            Game Settings
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

      {/* Before anything else, including the wizard: an unfinished session has
          left this machine in a state the user did not choose, and that is more
          urgent than an introduction. */}
      <Recovery />

      {/* Shown over the app rather than instead of it, so it reads as a guide
          through this product rather than a wall in front of it. */}
      {prefs && !prefs.onboarded && (
        <FirstRun
          unit={prefs.units}
          onDone={() => void updatePrefs({ ...prefs, onboarded: true })}
        />
      )}

      <main className="app__body">
        {prefsProblem && (
          <p className="app__notice">
            <span aria-hidden="true">▲</span> {prefsProblem}
          </p>
        )}
        {view === "home" && <Home onGo={setView} />}
        {view === "rig" && <Dashboard />}
        {view === "devices" && <Peripherals />}
        {view === "games" && <Games />}
        {view === "adapters" && <Adapters />}
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
