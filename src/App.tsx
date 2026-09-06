import { useEffect, useState } from "react";

import { Dashboard } from "./dashboard/Dashboard";
import { appInfo, asIpcError, type AppInfo } from "./ipc";
import "./app.css";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    appInfo()
      .then(setInfo)
      .catch((e) => setError(asIpcError(e).message));
  }, []);

  return (
    <div className="app">
      <header className="app__bar">
        <span className="app__mark">TEAM PRINCIPAL</span>
        <span className="app__milestone num">
          {info ? `v${info.version} · milestone ${info.milestone}` : "starting"}
        </span>
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
      </header>

      <main className="app__body">
        {error ? (
          <p className="app__error">
            <strong>Team Principal could not start.</strong> {error}
          </p>
        ) : (
          <Dashboard />
        )}
      </main>
    </div>
  );
}
