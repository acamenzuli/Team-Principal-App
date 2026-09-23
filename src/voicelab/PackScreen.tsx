import { useCallback, useEffect, useMemo, useState } from "react";

import type { Preferences } from "../ipc";
import {
  api,
  formatBytes,
  formatDuration,
  type Connection,
  type CrewChiefInfo,
  type EngineInfo,
  type InstallPlan,
  type JobProgress,
  type ModelsSnapshot,
  type Pack,
  type Voice,
} from "./client";

const ATTESTATION =
  "This is my voice, or I have the speaker's permission to use it.";

/**
 * Making the pack, watching it being made, and putting it where CrewChief
 * looks.
 *
 * The preview is the point of this screen: about seven hundred clips — the
 * spotter, positions, gaps, flags, fuel, the start and the finish — which is
 * enough to go racing with in a quarter of an hour. The rest can generate
 * while you sleep.
 */
export function PackScreen({
  conn,
  crewchief,
  job,
  prefs,
  packId,
  onPack,
  onReview,
  onPrefs,
  onError,
  onCrewChief,
}: {
  conn: Connection;
  crewchief: CrewChiefInfo | null;
  job: JobProgress | null;
  prefs: Preferences;
  packId: string | null;
  onPack: (id: string) => void;
  onReview: (id: string) => void;
  onPrefs: (next: Partial<Preferences["voiceLab"]>) => Promise<void>;
  onError: (message: string | null) => void;
  onCrewChief: (info: CrewChiefInfo) => void;
}) {
  const [voices, setVoices] = useState<Voice[]>([]);
  const [packs, setPacks] = useState<Pack[]>([]);
  const [engines, setEngines] = useState<EngineInfo[]>([]);
  const [models, setModels] = useState<ModelsSnapshot | null>(null);
  const [voiceId, setVoiceId] = useState<string | null>(null);
  const [voiceName, setVoiceName] = useState("");
  const [attested, setAttested] = useState(false);
  const [busy, setBusy] = useState(false);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [installed, setInstalled] = useState<string | null>(null);

  const pack = useMemo(() => packs.find((p) => p.id === packId) ?? null, [packs, packId]);
  const voice = useMemo(() => voices.find((v) => v.id === voiceId) ?? null, [voices, voiceId]);
  const running = job && ["starting", "running", "paused", "finishing"].includes(job.state);

  const load = useCallback(async () => {
    try {
      const [v, p, e, m] = await Promise.all([
        api.voices(conn),
        api.packs(conn),
        api.engines(conn),
        api.models(conn),
      ]);
      setVoices(v);
      setPacks(p);
      setEngines(e);
      setModels(m);
      setVoiceId((current) => current ?? v.find((x) => x.source === "recorded")?.id ?? v[0]?.id ?? null);
      onError(null);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [conn, onError]);

  useEffect(() => {
    void load();
  }, [load]);

  // The counts change as clips land, so refresh the pack list when a job
  // finishes rather than leaving a stale "0 of 765".
  useEffect(() => {
    if (job && ["done", "cancelled", "failed"].includes(job.state)) void load();
  }, [job?.state, job, load]);

  useEffect(() => {
    if (voice && !voiceName) setVoiceName(voice.name);
  }, [voice, voiceName]);

  const create = async () => {
    if (!voiceId || !voiceName.trim()) return;
    setBusy(true);
    try {
      const created = await api.createPack(conn, {
        voice_id: voiceId,
        voice_name: voiceName.trim(),
        engine: prefs.voiceLab.engine,
        variants: prefs.voiceLab.variants,
        your_name: prefs.voiceLab.yourName,
        radio_effect: prefs.voiceLab.radioEffect,
        max_attempts: 4,
        attestation: attested ? ATTESTATION : null,
      });
      setPacks(await api.packs(conn));
      onPack(created.id);
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const generate = async (scope: "preview" | "full") => {
    if (!packId) return;
    setBusy(true);
    try {
      await api.generate(conn, packId, { scope, workers: prefs.voiceLab.workers ?? null });
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const showPlan = async () => {
    if (!packId) return;
    try {
      setPlan(await api.installPlan(conn, packId));
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const install = async () => {
    if (!packId) return;
    setBusy(true);
    try {
      const result = await api.install(conn, packId);
      setInstalled(
        `Installed ${result.files_written} files${result.backup ? `; what was there is backed up in ${result.backup}` : ""}.`,
      );
      setPlan(null);
      onCrewChief(await api.crewchief(conn));
      setPacks(await api.packs(conn));
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const uninstall = async (id: string) => {
    setBusy(true);
    try {
      const result = await api.uninstall(conn, id);
      setInstalled(result.reason ?? `Removed ${result.removed} files.`);
      onCrewChief(await api.crewchief(conn));
      setPacks(await api.packs(conn));
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const modelsMissing = models && !models.all_present;

  return (
    <div className="voicelab__screen">
      {crewchief && (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">Your CrewChief</h2>
            <p className="group__note">
              {crewchief.sounds_folder ? "found" : "not found — pick the folder"}
            </p>
          </header>
          <div className="group__body">
            {crewchief.sounds_folder ? (
              <>
                <p className="note num">{crewchief.sounds_folder}</p>
                {crewchief.probe && (
                  <p className="note">
                    {crewchief.probe.chief_intents.toLocaleString()} phrase folders ·{" "}
                    {crewchief.probe.chief_lines.toLocaleString()} lines of subtitles ·{" "}
                    {crewchief.probe.spotter_intents} spotter calls ·{" "}
                    {crewchief.probe.wav_format
                      ? `${crewchief.probe.wav_format.sample_rate} Hz, ${crewchief.probe.wav_format.bits}-bit${
                          crewchief.probe.wav_format.channels === 1 ? " mono" : ""
                        }`
                      : "format unknown"}
                    . Your pack is written to match.
                  </p>
                )}
                {crewchief.probe && crewchief.probe.chief_derived_folders > 0 && (
                  <p className="note">
                    {crewchief.probe.chief_derived_folders.toLocaleString()} folders have no subtitle
                    file — mostly numbers. Their words come from the folder names, and the check
                    listens to those especially carefully.
                  </p>
                )}
              </>
            ) : (
              <p className="note note--fail">
                <span aria-hidden="true">■</span> CrewChief's sounds folder was not where it usually
                is. Looked in: {crewchief.candidates.join(", ")}.
              </p>
            )}
            {crewchief.installed && crewchief.installed.length > 0 && (
              <div className="voicelab__installed">
                <p className="note">Voice Lab packs installed:</p>
                {crewchief.installed.map((p) => (
                  <div key={p.voice_name} className="voicelab__installed-row">
                    <span>{p.voice_name}</span>
                    <span className="note num">{p.files} files</span>
                    <button className="btn btn--quiet btn--small" onClick={() => void uninstall(p.pack_id)}>
                      Uninstall
                    </button>
                  </div>
                ))}
                <p className="note">
                  To use one: open CrewChief, then Properties → search for <em>chief name</em> and
                  choose it. The spotter is a separate setting in the same place.
                </p>
              </div>
            )}
          </div>
        </div>
      )}

      {modelsMissing && (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">Models</h2>
            <p className="group__note">downloaded once, on first use</p>
          </header>
          <div className="group__body">
            {models!.busy ? (
              <>
                <p className="voicelab__stage">{models!.current ?? "Downloading…"}</p>
                <progress
                  className="voicelab__progress"
                  value={models!.bytes_present}
                  max={Math.max(models!.bytes_total, 1)}
                />
                <p className="note num">
                  {formatBytes(models!.bytes_present)} of {formatBytes(models!.bytes_total)}
                </p>
              </>
            ) : (
              <>
                <p className="note">
                  The speech and checking models are not here yet — about{" "}
                  {formatBytes(models!.bytes_total)}. They download once.
                </p>
                <button
                  className="btn"
                  onClick={() => void api.downloadModels(conn).then(setModels).catch((e) => onError(String(e)))}
                >
                  Download the models
                </button>
              </>
            )}
            {models!.error && (
              <p className="note note--fail">
                <span aria-hidden="true">■</span> {models!.error}
              </p>
            )}
          </div>
        </div>
      )}

      <div className="group glass">
        <header className="group__head">
          <h2 className="group__title">The pack</h2>
          <p className="group__note">what CrewChief will call this voice</p>
        </header>
        <div className="group__body">
          <div className="row">
            <div className="row__label">
              <span>Voice</span>
            </div>
            <div className="row__control voicelab__voices">
              {voices.map((v) => (
                <button
                  key={v.id}
                  className={`chip${v.id === voiceId ? " chip--on" : ""}`}
                  onClick={() => setVoiceId(v.id)}
                  title={v.source === "builtin" ? "The engine's own sample voice, for trying this out" : undefined}
                >
                  {v.name}
                  {v.source === "builtin" && <span className="note"> · sample</span>}
                </button>
              ))}
            </div>
          </div>

          <div className="row">
            <div className="row__label">
              <span>Name in CrewChief</span>
              <p className="hint">The name you will pick from CrewChief's own list.</p>
            </div>
            <div className="row__control">
              <input
                className="text-input"
                value={voiceName}
                onChange={(e) => setVoiceName(e.target.value)}
                placeholder="Alex"
              />
            </div>
          </div>

          <div className="row">
            <div className="row__label">
              <span>What the chief calls you</span>
              <p className="hint">
                Baked into the phrases that address you, in place of "mate". Leave it empty to keep
                what CrewChief says.
              </p>
            </div>
            <div className="row__control">
              <input
                className="text-input"
                value={prefs.voiceLab.yourName ?? ""}
                onChange={(e) => void onPrefs({ yourName: e.target.value || null })}
                placeholder="mate"
              />
            </div>
          </div>

          <div className="row">
            <div className="row__label">
              <span>Engine</span>
            </div>
            <div className="row__control voicelab__voices">
              {engines.map((e) => (
                <button
                  key={e.id}
                  className={`chip${e.id === prefs.voiceLab.engine ? " chip--on" : ""}`}
                  onClick={() => void onPrefs({ engine: e.id })}
                  title={`${e.summary} · ${e.licence}`}
                >
                  {e.name}
                </button>
              ))}
            </div>
          </div>

          <div className="row">
            <div className="row__label">
              <span>Radio effect</span>
              <p className="hint">Band-passed and compressed, like a pit radio. Try both.</p>
            </div>
            <div className="row__control">
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={prefs.voiceLab.radioEffect}
                  onChange={(e) => void onPrefs({ radioEffect: e.target.checked })}
                />
                <span className="toggle__track" aria-hidden="true">
                  <span className="toggle__knob" />
                </span>
                <span>Pit radio</span>
              </label>
            </div>
          </div>

          {voice && voice.source !== "builtin" && (
            <div className="row">
              <div className="row__label">
                <span>Consent</span>
              </div>
              <div className="row__control">
                <label className="toggle">
                  <input type="checkbox" checked={attested} onChange={(e) => setAttested(e.target.checked)} />
                  <span className="toggle__track" aria-hidden="true">
                    <span className="toggle__knob" />
                  </span>
                  <span>{ATTESTATION}</span>
                </label>
                <p className="hint">
                  Recorded with the pack, with today's date. Every pack also carries a note saying
                  the audio is AI-generated and which engine made it.
                </p>
              </div>
            </div>
          )}

          <div className="settings__actions">
            <button
              className="btn"
              onClick={() => void create()}
              disabled={busy || !voiceId || !voiceName.trim() || (voice?.source !== "builtin" && !attested)}
            >
              Make a pack
            </button>
            {packs.length > 0 && (
              <select
                className="text-input"
                value={packId ?? ""}
                onChange={(e) => e.target.value && onPack(e.target.value)}
              >
                <option value="">Or pick one you started…</option>
                {packs.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.settings.voice_name} · {p.created_at.slice(0, 10)} · {p.counts.passed ?? 0}/
                    {p.counts.total ?? 0} done
                  </option>
                ))}
              </select>
            )}
          </div>
        </div>
      </div>

      {pack && (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">{pack.settings.voice_name}</h2>
            <p className="group__note">
              {(pack.counts.passed ?? 0).toLocaleString()} of {(pack.counts.total ?? 0).toLocaleString()} clips made
            </p>
          </header>
          <div className="group__body">
            {running ? (
              <>
                <div className="voicelab__job">
                  <progress className="voicelab__progress" value={job!.done} max={Math.max(job!.total, 1)} />
                  <p className="voicelab__stage">
                    {job!.paused
                      ? job!.pause_reason === "user"
                        ? "Paused."
                        : "Paused while you race."
                      : `${job!.done.toLocaleString()} of ${job!.total.toLocaleString()} · ${job!.passed} kept · ${job!.failed} being retried`}
                  </p>
                  <p className="note num">
                    {job!.workers} worker{job!.workers === 1 ? "" : "s"} ·{" "}
                    {job!.eta_s ? `about ${formatDuration(job!.eta_s)} left` : "estimating…"} · running for{" "}
                    {formatDuration(job!.elapsed_s)}
                  </p>
                </div>
                <div className="settings__actions">
                  {job!.paused ? (
                    <button className="btn" onClick={() => void api.resume(conn)} disabled={job!.pause_reason !== "user"}>
                      Carry on
                    </button>
                  ) : (
                    <button className="btn btn--quiet" onClick={() => void api.pause(conn)}>
                      Pause
                    </button>
                  )}
                  <button className="btn btn--quiet" onClick={() => void api.cancel(conn)}>
                    Stop
                  </button>
                  <button className="btn btn--quiet" onClick={() => onReview(pack.id)}>
                    Listen to what is done
                  </button>
                </div>
                {job!.recent.length > 0 && (
                  <ul className="voicelab__recent">
                    {job!.recent.slice(0, 5).map((r, i) => (
                      <li key={`${r.rel_path}-${i}`}>
                        <span className={r.passed ? "status status--pass" : "status status--warn"}>
                          <span aria-hidden="true">{r.passed ? "●" : "▲"}</span>
                        </span>
                        <span className="voicelab__recent-text">{r.text}</span>
                        {!r.passed && <span className="note">heard “{r.transcript}” — trying again</span>}
                      </li>
                    ))}
                  </ul>
                )}
              </>
            ) : (
              <>
                <p className="note">
                  The preview makes the {(pack.counts.preview ?? 0).toLocaleString()} phrases you hear
                  in the first minutes of a race — the spotter, positions, gaps, flags, fuel, the
                  start and the finish. About fifteen minutes on this machine, and enough to go
                  racing with.
                </p>
                <div className="settings__actions">
                  <button className="btn" onClick={() => void generate("preview")} disabled={busy}>
                    Make the preview
                  </button>
                  <button className="btn btn--quiet" onClick={() => void generate("full")} disabled={busy}>
                    Make everything ({(pack.counts.total ?? 0).toLocaleString()})
                  </button>
                  {(pack.counts.passed ?? 0) > 0 && (
                    <button className="btn btn--quiet" onClick={() => onReview(pack.id)}>
                      Listen
                    </button>
                  )}
                </div>
                {job?.state === "failed" && job.error && (
                  <p className="note note--fail">
                    <span aria-hidden="true">■</span> {job.error}
                  </p>
                )}
              </>
            )}

            {(pack.counts.passed ?? 0) > 0 && !running && (
              <div className="voicelab__install">
                <h3 className="voicelab__subtitle">Install it</h3>
                {plan ? (
                  <>
                    <p className="note">
                      {plan.files.toLocaleString()} files ({formatBytes(plan.bytes)}) into:
                    </p>
                    <ul className="voicelab__targets">
                      {plan.targets.map((t) => (
                        <li key={t} className="num">
                          {plan.sounds}\{t.replace(/\//g, "\\")}
                        </li>
                      ))}
                    </ul>
                    {plan.in_the_way.length > 0 && (
                      <p className="note">
                        <span aria-hidden="true">▲</span> {plan.in_the_way.join(", ")} already exists
                        and will be moved to a backup folder first, not overwritten.
                      </p>
                    )}
                    {plan.previously_installed && (
                      <p className="note">Replaces the copy already installed under that name.</p>
                    )}
                    <div className="settings__actions">
                      <button className="btn" onClick={() => void install()} disabled={busy}>
                        Install
                      </button>
                      <button className="btn btn--quiet" onClick={() => setPlan(null)}>
                        Not now
                      </button>
                    </div>
                  </>
                ) : (
                  <div className="settings__actions">
                    <button className="btn" onClick={() => void showPlan()}>
                      Install into CrewChief…
                    </button>
                    <button
                      className="btn btn--quiet"
                      onClick={() =>
                        void api
                          .exportPack(conn, pack.id)
                          .then((r) => setInstalled(`Saved to ${r.path}`))
                          .catch((e) => onError(String(e)))
                      }
                    >
                      Save as a zip
                    </button>
                  </div>
                )}
                {installed && <p className="note">{installed}</p>}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
