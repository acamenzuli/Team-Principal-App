import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  asIpcError,
  onVoicelabModule,
  onVoicelabService,
  voicelabInstallModule,
  voicelabModule,
  voicelabRemoveModule,
  voicelabRequirements,
  voicelabService,
  voicelabStartService,
  voicelabStopService,
  type ModuleStatus,
  type Preferences,
  type ServiceInfo,
  type VoiceLabRequirements,
} from "../ipc";
import { api, connectionOf, formatBytes, watchJob, type Connection, type CrewChiefInfo, type JobProgress } from "./client";
import { Requirements } from "./Requirements";
import { Recorder } from "./Recorder";
import { PackScreen } from "./PackScreen";
import { Review } from "./Review";
import "./voicelab.css";

type Screen = "voice" | "pack" | "review";

/**
 * The Voice Lab.
 *
 * Your own voice, turned into the crew chief. The work happens in a Python
 * service that this tab starts when it opens and the app stops when it has
 * been idle — none of it ships in the installer, and the tab's first job is
 * to say whether this machine can run it at all.
 *
 * Nothing here can break the rest of the app: if the service is missing,
 * refuses to start or dies mid-job, this tab says so and every other tab
 * carries on.
 */
export function VoiceLab({
  prefs,
  onChange,
}: {
  prefs: Preferences;
  onChange: (next: Preferences) => Promise<void>;
}) {
  const [requirements, setRequirements] = useState<VoiceLabRequirements | null>(null);
  const [module, setModule] = useState<ModuleStatus | null>(null);
  const [service, setService] = useState<ServiceInfo | null>(null);
  const [crewchief, setCrewChief] = useState<CrewChiefInfo | null>(null);
  const [job, setJob] = useState<JobProgress | null>(null);
  const [screen, setScreen] = useState<Screen>("voice");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [packId, setPackId] = useState<string | null>(null);
  const started = useRef(false);

  const conn = useMemo(() => connectionOf(service), [service]);

  // What the machine has and what is installed. Read every time the tab
  // opens: a driver update or a removed folder between two visits is exactly
  // where a remembered answer would be wrong.
  useEffect(() => {
    void Promise.all([voicelabRequirements(), voicelabModule(), voicelabService()])
      .then(([r, m, s]) => {
        setRequirements(r);
        setModule(m);
        setService(s);
      })
      .catch((e) => setError(asIpcError(e).message));
  }, []);

  useEffect(() => {
    let unlisten: (() => void)[] = [];
    void onVoicelabModule(setModule).then((f) => unlisten.push(f));
    void onVoicelabService(setService).then((f) => unlisten.push(f));
    return () => unlisten.forEach((f) => f());
  }, []);

  // Start the service once, when the tab is opened and everything it needs
  // is there. Never on app start: this is gigabytes of CUDA for a tab most
  // people will not open today.
  const start = useCallback(async () => {
    if (starting) return;
    setStarting(true);
    try {
      setService(await voicelabStartService());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setStarting(false);
    }
  }, [starting]);

  useEffect(() => {
    if (started.current) return;
    if (!module || module.stage !== "ready") return;
    if (service && service.state !== "stopped") return;
    started.current = true;
    void start();
  }, [module, service, start]);

  // Everything the service knows, once it is up.
  const refresh = useCallback(
    async (c: Connection) => {
      try {
        const [cc, j] = await Promise.all([api.crewchief(c), api.job(c)]);
        setCrewChief(cc);
        setJob(j);
        if (j.pack_id) setPackId((current) => current ?? j.pack_id);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [],
  );

  useEffect(() => {
    if (!conn) return;
    void refresh(conn);
    const stop = watchJob(conn, (event) => {
      if (event.type === "job") setJob(event as unknown as JobProgress);
    });
    return stop;
  }, [conn, refresh]);

  const install = async () => {
    try {
      setModule(await voicelabInstallModule());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  };

  const remove = async () => {
    try {
      started.current = false;
      setModule(await voicelabRemoveModule(true));
    } catch (e) {
      setError(asIpcError(e).message);
    }
  };

  const setVoiceLabPrefs = (next: Partial<Preferences["voiceLab"]>) =>
    onChange({ ...prefs, voiceLab: { ...prefs.voiceLab, ...next } });

  // ---------------------------------------------------------------- gates

  if (error && !requirements) {
    return (
      <section className="voicelab">
        <p className="note note--fail">
          <span aria-hidden="true">■</span> {error}
        </p>
      </section>
    );
  }

  if (!requirements || !module) {
    return (
      <section className="voicelab">
        <p className="note">Looking at what this machine has…</p>
      </section>
    );
  }

  if (!requirements.ok || module.stage !== "ready") {
    return (
      <Requirements
        requirements={requirements}
        module={module}
        error={error}
        onInstall={install}
        onRemove={remove}
      />
    );
  }

  if (!conn) {
    return (
      <section className="voicelab">
        <header className="voicelab__head">
          <h1 className="voicelab__title">Voice Lab</h1>
        </header>
        <p className="note">
          {starting || service?.state === "starting"
            ? "Starting the voice service — the first start loads the models, which takes a few seconds."
            : "The voice service is not running."}
        </p>
        {error && (
          <p className="note note--fail">
            <span aria-hidden="true">■</span> {error}
          </p>
        )}
        {!starting && service?.state !== "starting" && (
          <button className="btn" onClick={() => void start()}>
            Start the voice service
          </button>
        )}
      </section>
    );
  }

  // ---------------------------------------------------------------- the tab

  return (
    <section className="voicelab">
      <header className="voicelab__head">
        <h1 className="voicelab__title">Voice Lab</h1>
        <nav className="voicelab__screens" aria-label="Voice Lab sections">
          <ScreenTab id="voice" current={screen} onSelect={setScreen}>
            1 · Your voice
          </ScreenTab>
          <ScreenTab id="pack" current={screen} onSelect={setScreen}>
            2 · The pack
          </ScreenTab>
          <ScreenTab id="review" current={screen} onSelect={setScreen} disabled={!packId}>
            3 · Listen
          </ScreenTab>
        </nav>
        <span className="voicelab__spacer" />
        <ServiceBadge service={service} job={job} onStop={() => void voicelabStopService()} />
      </header>

      {error && (
        <p className="note note--fail">
          <span aria-hidden="true">■</span> {error}
        </p>
      )}

      {job && job.paused && job.pause_reason !== "user" && (
        <p className="app__notice">
          <span aria-hidden="true">▲</span> Generation is paused while a session is running — the
          game gets the graphics card. It starts again on its own when you finish.
        </p>
      )}

      {screen === "voice" && (
        <Recorder conn={conn} onError={setError} />
      )}
      {screen === "pack" && (
        <PackScreen
          conn={conn}
          crewchief={crewchief}
          job={job}
          prefs={prefs}
          packId={packId}
          onPack={(id) => {
            setPackId(id);
          }}
          onReview={(id) => {
            setPackId(id);
            setScreen("review");
          }}
          onPrefs={setVoiceLabPrefs}
          onError={setError}
          onCrewChief={setCrewChief}
        />
      )}
      {screen === "review" && packId && (
        <Review conn={conn} packId={packId} job={job} onError={setError} />
      )}

      <footer className="voicelab__foot">
        <span className="note num">
          Module at {module.home} · {formatBytes(module.bytesOnDisk)}
        </span>
        <button className="btn btn--quiet" onClick={() => void remove()}>
          Remove Voice Lab
        </button>
      </footer>
    </section>
  );
}

function ScreenTab(props: {
  id: Screen;
  current: Screen;
  onSelect: (s: Screen) => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  const on = props.id === props.current;
  return (
    <button
      className={`tab${on ? " tab--on" : ""}`}
      aria-current={on ? "page" : undefined}
      disabled={props.disabled}
      onClick={() => props.onSelect(props.id)}
    >
      {props.children}
    </button>
  );
}

/** What the service is doing, in the corner, always. */
function ServiceBadge({
  service,
  job,
  onStop,
}: {
  service: ServiceInfo | null;
  job: JobProgress | null;
  onStop: () => void;
}) {
  const busy = job && ["starting", "running", "paused", "finishing"].includes(job.state);
  const text = !service || service.state !== "ready"
    ? "service stopped"
    : busy
      ? job!.paused
        ? "paused"
        : `working · ${job!.done}/${job!.total}`
      : "service ready";
  const tone = !service || service.state !== "ready" ? "idle" : busy && !job!.paused ? "run" : "pass";
  return (
    <span className="voicelab__service">
      <span className={`dot dot--${tone}`} aria-hidden="true" />
      <span className="note">{text}</span>
      {service?.state === "ready" && !busy && (
        <button className="btn btn--quiet btn--small" onClick={onStop} title="Free the graphics card">
          Stop
        </button>
      )}
    </span>
  );
}
