import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  api,
  type Connection,
  type InputDevice,
  type ScriptLine,
  type TakeAnalysis,
  type Voice,
} from "./client";

const TONES = ["calm", "urgent", "celebratory"] as const;
const TONE_LABEL: Record<string, string> = {
  calm: "Calm",
  urgent: "Urgent",
  celebratory: "Celebratory",
};
const TONE_NOTE: Record<string, string> = {
  calm: "Gaps, fuel, strategy — most of what a chief says.",
  urgent: "Spotter calls, flags, damage. Fast and clear, not shouted.",
  celebratory: "Wins, podiums, a personal best.",
};

/**
 * Guided recording.
 *
 * Twenty-odd lines in three tones, one take at a time. The microphone is
 * read in Python rather than in the webview: lossless PCM, a device list you
 * can choose from, and the trimming has to happen there anyway. What comes
 * back is a cleaned take with a verdict on it — clipping, noise, length —
 * because a reference recorded too close to the microphone makes every one
 * of six thousand clips worse, and that is worth catching in the first
 * minute rather than the last.
 */
export function Recorder({
  conn,
  onError,
}: {
  conn: Connection;
  onError: (message: string | null) => void;
}) {
  const [script, setScript] = useState<ScriptLine[]>([]);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [device, setDevice] = useState<number | null>(null);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [voiceId, setVoiceId] = useState<string | null>(null);
  const [tone, setTone] = useState<string>("calm");
  const [recording, setRecording] = useState<string | null>(null);
  const [level, setLevel] = useState<{ peak: number; clipped: boolean; seconds: number } | null>(null);
  const [last, setLast] = useState<{ lineId: string; analysis: TakeAnalysis; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState("");
  const socket = useRef<WebSocket | null>(null);

  const voice = useMemo(() => voices.find((v) => v.id === voiceId) ?? null, [voices, voiceId]);

  const load = useCallback(async () => {
    try {
      const [s, d, v] = await Promise.all([api.script(conn), api.devices(conn), api.voices(conn)]);
      setScript(s);
      setDevices(d);
      setVoices(v);
      setDevice((current) => current ?? d.find((x) => x.default)?.index ?? null);
      setVoiceId((current) => current ?? v.find((x) => x.source === "recorded")?.id ?? null);
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }, [conn, onError]);

  useEffect(() => {
    void load();
  }, [load]);

  // Live level while a take is open. Its own socket: this is 15 messages a
  // second and has nothing to do with the job stream.
  useEffect(() => {
    if (!recording) return;
    const url = conn.baseUrl.replace(/^http/, "ws") + `/ws?token=${encodeURIComponent(conn.token)}`;
    const ws = new WebSocket(url);
    socket.current = ws;
    ws.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data);
        if (event.type === "level") {
          setLevel({ peak: event.peak, clipped: event.clipped, seconds: event.seconds });
        }
      } catch {
        /* a frame that is not JSON is not worth stopping for */
      }
    };
    return () => {
      ws.close();
      socket.current = null;
    };
  }, [recording, conn]);

  const createVoice = async () => {
    const name = newName.trim();
    if (!name) return;
    try {
      const created = await api.createVoice(conn, name);
      setVoices(await api.voices(conn));
      setVoiceId(created.id);
      setNewName("");
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const start = async (line: ScriptLine) => {
    if (!voiceId || busy) return;
    setBusy(true);
    try {
      await api.startRecording(conn, { voice_id: voiceId, tone: line.tone, line_id: line.id, device });
      setRecording(line.id);
      setLevel(null);
      setLast(null);
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const stop = async (line: ScriptLine) => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await api.stopRecording(conn);
      setRecording(null);
      setLevel(null);
      setLast({ lineId: line.id, analysis: result.analysis, message: result.message });
      if (result.voice) {
        setVoices((current) => current.map((v) => (v.id === result.voice!.id ? result.voice! : v)));
      }
      onError(null);
    } catch (e) {
      setRecording(null);
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const discard = async (line: ScriptLine) => {
    if (!voiceId) return;
    try {
      const updated = await api.deleteTake(conn, voiceId, line.tone, line.id);
      setVoices((current) => current.map((v) => (v.id === updated.id ? updated : v)));
      setLast(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const lines = script.filter((l) => l.tone === tone);
  const recorded = (line: ScriptLine) => (voice?.takes?.[line.tone] ?? []).includes(`${slugOf(line.id)}.wav`);
  const counts = TONES.map((t) => ({
    tone: t,
    done: (voice?.takes?.[t] ?? []).length,
    total: script.filter((l) => l.tone === t).length,
  }));

  return (
    <div className="voicelab__screen">
      <div className="group glass">
        <header className="group__head">
          <h2 className="group__title">Whose voice</h2>
          <p className="group__note">this is the voice the chief will have</p>
        </header>
        <div className="group__body">
          <div className="row">
            <div className="row__label">
              <span>Voice</span>
              <p className="hint">
                One set of recordings. You can keep more than one — your own, and a friend's who
                said yes.
              </p>
            </div>
            <div className="row__control voicelab__voices">
              {voices
                .filter((v) => v.source !== "builtin")
                .map((v) => (
                  <button
                    key={v.id}
                    className={`chip${v.id === voiceId ? " chip--on" : ""}`}
                    onClick={() => setVoiceId(v.id)}
                  >
                    {v.name}
                    <span className="note"> · {Object.values(v.takes ?? {}).flat().length} takes</span>
                  </button>
                ))}
              <span className="voicelab__new">
                <input
                  className="text-input"
                  placeholder="A name for a new voice"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void createVoice()}
                />
                <button className="btn btn--quiet" onClick={() => void createVoice()} disabled={!newName.trim()}>
                  Add
                </button>
              </span>
            </div>
          </div>

          <div className="row">
            <div className="row__label">
              <span>Microphone</span>
              <p className="hint">
                A headset boom or a desk mic both work. What matters is a quiet room and staying the
                same distance away for every take.
              </p>
            </div>
            <div className="row__control">
              <select
                className="text-input"
                value={device ?? ""}
                onChange={(e) => setDevice(e.target.value === "" ? null : Number(e.target.value))}
              >
                <option value="">System default</option>
                {devices.map((d) => (
                  <option key={d.index} value={d.index}>
                    {d.name} ({d.host_api})
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>
      </div>

      {!voiceId ? (
        <p className="note">Add a voice above to start recording.</p>
      ) : (
        <div className="group glass">
          <header className="group__head">
            <h2 className="group__title">Read these out</h2>
            <p className="group__note">
              {counts.map((c) => `${TONE_LABEL[c.tone]} ${c.done}/${c.total}`).join(" · ")}
            </p>
          </header>
          <div className="group__body">
            <div className="segmented" role="radiogroup">
              {TONES.map((t) => (
                <button
                  key={t}
                  role="radio"
                  aria-checked={tone === t}
                  className={`segmented__item${tone === t ? " segmented__item--on" : ""}`}
                  onClick={() => setTone(t)}
                >
                  <span className="segmented__label">{TONE_LABEL[t]}</span>
                  <span className="segmented__desc">{TONE_NOTE[t]}</span>
                </button>
              ))}
            </div>

            <ol className="voicelab__script">
              {lines.map((line) => {
                const open = recording === line.id;
                const done = recorded(line);
                const verdict = last?.lineId === line.id ? last : null;
                return (
                  <li key={line.id} className={`voicelab__line${open ? " voicelab__line--live" : ""}`}>
                    <div className="voicelab__line-head">
                      <span className={done ? "status status--pass" : "status status--idle"}>
                        <span aria-hidden="true">{done ? "●" : "○"}</span> {done ? "DONE" : "WAIT"}
                      </span>
                      {line.hint && <span className="note">{line.hint}</span>}
                    </div>
                    <p className="voicelab__line-text">{line.text}</p>

                    {open && (
                      <div className="voicelab__meter">
                        <div className="voicelab__meter-bar">
                          <span
                            className={`voicelab__meter-fill${level?.clipped ? " voicelab__meter-fill--hot" : ""}`}
                            style={{ width: `${Math.min(100, Math.round((level?.peak ?? 0) * 100))}%` }}
                          />
                        </div>
                        <span className="note num">
                          {(level?.seconds ?? 0).toFixed(1)}s{level?.clipped ? " · too loud" : ""}
                        </span>
                      </div>
                    )}

                    <div className="voicelab__line-actions">
                      {open ? (
                        <button className="btn" onClick={() => void stop(line)} disabled={busy}>
                          Stop
                        </button>
                      ) : (
                        <button className="btn" onClick={() => void start(line)} disabled={busy || !!recording}>
                          {done ? "Record again" : "Record"}
                        </button>
                      )}
                      {done && !open && (
                        <>
                          <audio
                            className="voicelab__audio"
                            controls
                            preload="none"
                            src={api.takeUrl(conn, voiceId, line.tone, line.id)}
                          />
                          <button className="btn btn--quiet" onClick={() => void discard(line)}>
                            Delete
                          </button>
                        </>
                      )}
                    </div>

                    {verdict && (
                      <>
                        <p className={verdict.analysis.ok ? "note" : "note note--fail"}>
                          <span aria-hidden="true">{verdict.analysis.ok ? "●" : "■"}</span>{" "}
                          {verdict.analysis.ok
                            ? `Kept — ${verdict.analysis.speech_s.toFixed(1)}s of speech${
                                verdict.analysis.snr_db
                                  ? `, ${Math.round(verdict.analysis.snr_db)} dB above the room`
                                  : ""
                              }.`
                            : `Not kept: ${verdict.message}.`}
                        </p>
                        {/* A take can be kept and still be worth improving —
                            a noisy room makes every generated clip worse, so
                            say so rather than burying it in a number. */}
                        {verdict.analysis.ok && verdict.analysis.problems.length > 0 && (
                          <p className="note">
                            <span aria-hidden="true">▲</span> {verdict.message}. Recording it again
                            in a quieter moment would improve the whole pack.
                          </p>
                        )}
                      </>
                    )}
                  </li>
                );
              })}
            </ol>
          </div>
        </div>
      )}
    </div>
  );
}

/** Matches the service's `slug`, so "is this line recorded" agrees with it. */
function slugOf(id: string): string {
  return (
    id
      .trim()
      .replace(/[^A-Za-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .toLowerCase() || "voice"
  );
}
