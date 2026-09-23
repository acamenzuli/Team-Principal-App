/**
 * The Voice Lab's client for the local Python service.
 *
 * This is the one place in the frontend that talks to something other than
 * Rust, and it is deliberately narrow: one base URL and token, obtained from
 * Rust, and a `call` that adds the bearer token and turns a failure into a
 * sentence. Everything about voices, packs and generation is here rather
 * than routed through Tauri commands — it is all local, there is a lot of
 * it, and a second set of types in the middle would earn nothing.
 */
import type { ServiceInfo } from "../ipc";

export type Connection = { baseUrl: string; token: string };

export function connectionOf(info: ServiceInfo | null): Connection | null {
  if (!info || !info.baseUrl || !info.token) return null;
  return { baseUrl: info.baseUrl, token: info.token };
}

/** What the service said went wrong, or what went wrong reaching it. */
export class ServiceError extends Error {
  constructor(
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "ServiceError";
  }
}

async function call<T>(
  conn: Connection,
  path: string,
  init?: { method?: string; body?: unknown },
): Promise<T> {
  const { body, method } = init ?? {};
  let response: Response;
  try {
    response = await fetch(`${conn.baseUrl}${path}`, {
      method: method ?? "GET",
      headers: {
        Authorization: `Bearer ${conn.token}`,
        ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      },
      ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
    });
  } catch (e) {
    throw new ServiceError(
      `The voice service did not answer. It may have stopped — try opening the tab again. (${String(e)})`,
    );
  }
  if (!response.ok) {
    let detail = `${response.status} ${response.statusText}`;
    try {
      const body = await response.json();
      if (typeof body?.detail === "string") detail = body.detail;
    } catch {
      // A non-JSON error body is not worth a second failure.
    }
    throw new ServiceError(detail, response.status);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

const get = <T>(c: Connection, path: string) => call<T>(c, path);
const post = <T>(c: Connection, path: string, body?: unknown) =>
  call<T>(c, path, { method: "POST", body: body ?? {} });
const put = <T>(c: Connection, path: string, body: unknown) =>
  call<T>(c, path, { method: "PUT", body });
const del = <T>(c: Connection, path: string) => call<T>(c, path, { method: "DELETE" });

// ------------------------------------------------------------------- shapes
//
// These describe the service's JSON. They are hand-written because the
// service is Python: the contract is the OpenAPI the routes declare, and the
// review screen is the thing that would notice a drift immediately.

export type Health = {
  ok: boolean;
  version: string;
  home: string;
  idle_seconds: number;
  busy: boolean;
  gpu: { available: boolean; name?: string; vram_total_gb?: number; vram_free_gb?: number };
  sounds_folder: string | null;
};

export type WavFormat = { sample_rate: number; channels: number; bits: number };

export type CrewChiefInfo = {
  sounds_folder: string | null;
  candidates: string[];
  config: Record<string, string>;
  probe?: {
    pack_language: string | null;
    pack_version: string | null;
    wav_format: WavFormat | null;
    format_uniform: boolean;
    chief_intents: number;
    chief_lines: number;
    chief_derived_folders: number;
    alt_voices: string[];
    spotter_voices: string[];
    personalisations: string[];
    spotter_intents: number;
  };
  installed?: { voice_name: string; pack_id: string; installed_at: string; files: number }[];
};

export type ScriptLine = { id: string; tone: string; text: string; hint: string };

export type InputDevice = {
  index: number;
  name: string;
  channels: number;
  host_api: string;
  default: boolean;
};

export type TakeAnalysis = {
  duration_s: number;
  speech_s: number;
  snr_db: number | null;
  clipped: boolean;
  peak_dbfs: number;
  lufs: number | null;
  problems: string[];
  waveform: number[];
  ok: boolean;
};

export type Voice = {
  id: string;
  name: string;
  created_at: string;
  source: string;
  tones: Record<string, string>;
  takes: Record<string, string[]>;
  attestation: { text: string; at: string } | null;
};

export type PackSettings = {
  voice_name: string;
  voice_id: string;
  engine: string;
  variants: number;
  your_name: string | null;
  radio_effect: boolean;
  sample_rate: number;
  max_attempts: number;
};

export type Pack = {
  id: string;
  created_at: string;
  settings: PackSettings;
  counts: Record<string, number>;
  installed_at: string | null;
  sounds_folder: string | null;
};

export type Clip = {
  rel_path: string;
  intent: string;
  role: string;
  text: string;
  subtitle: string;
  derived: boolean;
  variant: number;
  preview: boolean;
  state: "passed" | "failed" | "pending";
  attempts: number;
  wer: number | null;
  transcript: string | null;
  reasons: string[];
  confidence: number | null;
  duration_s: number | null;
  audio: "out" | "failed" | null;
  review: { marked?: boolean; note?: string; text_override?: string };
};

export type JobProgress = {
  state: "idle" | "starting" | "running" | "paused" | "finishing" | "done" | "cancelled" | "failed";
  pack_id: string | null;
  scope: string | null;
  total: number;
  done: number;
  passed: number;
  failed: number;
  skipped: number;
  attempts: number;
  workers: number;
  in_flight: Record<string, string>;
  eta_s: number | null;
  elapsed_s: number;
  seconds_per_clip: number | null;
  paused: boolean;
  pause_reason: string | null;
  error: string | null;
  recent: { rel_path: string; text: string; transcript: string; passed: boolean; reasons: string[] }[];
};

export type ModelsSnapshot = {
  busy: boolean;
  current: string | null;
  error: string | null;
  all_present: boolean;
  bytes_total: number;
  bytes_present: number;
  models: {
    id: string;
    name: string;
    licence: string;
    purpose: string;
    present: boolean;
    bytes_total: number | null;
    bytes_present: number;
  }[];
};

export type EngineInfo = {
  id: string;
  name: string;
  vendor: string;
  licence: string;
  commercial_ok: boolean;
  summary: string;
  sample_rate: number;
};

export type InstallPlan = {
  sounds: string;
  voice_name: string;
  targets: string[];
  files: number;
  bytes: number;
  in_the_way: string[];
  previously_installed: boolean;
};

// ------------------------------------------------------------------ the API

export const api = {
  health: (c: Connection) => get<Health>(c, "/health"),
  crewchief: (c: Connection) => get<CrewChiefInfo>(c, "/crewchief"),
  setSoundsFolder: (c: Connection, path: string | null) =>
    post<CrewChiefInfo>(c, "/crewchief/folder", { path }),
  inventory: (c: Connection, voiceName: string, variants: number, yourName: string | null) =>
    get<{ total: number; preview: number; derived: number; by_role: Record<string, number>; by_category: Record<string, number> }>(
      c,
      `/inventory?voice_name=${encodeURIComponent(voiceName)}&variants=${variants}` +
        (yourName ? `&your_name=${encodeURIComponent(yourName)}` : ""),
    ),

  engines: (c: Connection) => get<EngineInfo[]>(c, "/engines"),
  models: (c: Connection) => get<ModelsSnapshot>(c, "/models"),
  downloadModels: (c: Connection, engines?: string[]) =>
    post<ModelsSnapshot>(c, "/models/download", { engines: engines ?? null }),
  cancelModels: (c: Connection) => post<ModelsSnapshot>(c, "/models/cancel"),

  tones: (c: Connection) => get<{ raw: unknown; path: string; problem: string | null }>(c, "/tones"),
  saveTones: (c: Connection, raw: unknown) => put<{ raw: unknown }>(c, "/tones", { raw }),
  resetTones: (c: Connection) => post<{ raw: unknown }>(c, "/tones/reset"),

  devices: (c: Connection) => get<InputDevice[]>(c, "/devices"),
  script: (c: Connection) => get<ScriptLine[]>(c, "/script"),
  startRecording: (c: Connection, body: { voice_id: string; tone: string; line_id: string; device: number | null }) =>
    post<{ recording: boolean; sample_rate: number }>(c, "/record/start", body),
  stopRecording: (c: Connection) =>
    post<{ analysis: TakeAnalysis; message: string; saved: string | null; voice: Voice | null; tone: string; line_id: string }>(
      c,
      "/record/stop",
    ),
  cancelRecording: (c: Connection) => post<{ recording: boolean }>(c, "/record/cancel"),

  voices: (c: Connection) => get<Voice[]>(c, "/voices"),
  createVoice: (c: Connection, name: string) => post<Voice>(c, "/voices", { name }),
  deleteVoice: (c: Connection, id: string) => del<{ deleted: boolean }>(c, `/voices/${id}`),
  attest: (c: Connection, id: string, text: string) => post<Voice>(c, `/voices/${id}/attest`, { text }),
  deleteTake: (c: Connection, id: string, tone: string, lineId: string) =>
    del<Voice>(c, `/voices/${id}/takes/${tone}/${lineId}`),
  takeUrl: (c: Connection, id: string, tone: string, lineId: string) =>
    `${c.baseUrl}/voices/${id}/takes/${tone}/${slug(lineId)}.wav?token=${encodeURIComponent(c.token)}`,
  referenceUrl: (c: Connection, id: string, tone: string) =>
    `${c.baseUrl}/voices/${id}/ref/${tone}.wav?token=${encodeURIComponent(c.token)}`,

  packs: (c: Connection) => get<Pack[]>(c, "/packs"),
  createPack: (
    c: Connection,
    body: {
      voice_id: string;
      voice_name: string;
      engine: string;
      variants: number;
      your_name: string | null;
      radio_effect: boolean;
      max_attempts: number;
      attestation: string | null;
    },
  ) => post<Pack>(c, "/packs", body),
  pack: (c: Connection, id: string) => get<Pack>(c, `/packs/${id}`),
  deletePack: (c: Connection, id: string) => del<{ deleted: boolean }>(c, `/packs/${id}`),
  clips: (c: Connection, id: string, query: Record<string, string | number | boolean | undefined>) => {
    const search = Object.entries(query)
      .filter(([, v]) => v !== undefined && v !== "")
      .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
      .join("&");
    return get<{ total: number; clips: Clip[] }>(c, `/packs/${id}/clips${search ? `?${search}` : ""}`);
  },
  clipUrl: (c: Connection, id: string, relPath: string, which: "out" | "failed") =>
    `${c.baseUrl}/packs/${id}/audio?rel_path=${encodeURIComponent(relPath)}&which=${which}&token=${encodeURIComponent(c.token)}`,
  mark: (c: Connection, id: string, relPath: string, marked: boolean | null, note?: string | null) =>
    post<unknown>(c, `/packs/${id}/mark`, { rel_path: relPath, marked, note }),
  setText: (c: Connection, id: string, relPath: string, text: string | null) =>
    post<{ ok: boolean }>(c, `/packs/${id}/text`, { rel_path: relPath, text }),
  generate: (
    c: Connection,
    id: string,
    body: { scope: "preview" | "full" | "selection"; rel_paths?: string[]; workers?: number | null; regenerate?: boolean },
  ) => post<JobProgress>(c, `/packs/${id}/generate`, body),
  installPlan: (c: Connection, id: string) => get<InstallPlan>(c, `/packs/${id}/install/plan`),
  install: (c: Connection, id: string) =>
    post<{ manifest: string; files_written: number; bytes_written: number; backup: string | null }>(
      c,
      `/packs/${id}/install`,
    ),
  uninstall: (c: Connection, id: string) =>
    post<{ removed: number; restored: boolean; reason?: string }>(c, `/packs/${id}/uninstall`),
  exportPack: (c: Connection, id: string, dest?: string) =>
    post<{ path: string }>(c, `/packs/${id}/export`, { dest: dest ?? null }),

  job: (c: Connection) => get<JobProgress>(c, "/jobs"),
  pause: (c: Connection) => post<JobProgress>(c, "/jobs/pause", { reason: "user" }),
  resume: (c: Connection) => post<JobProgress>(c, "/jobs/resume"),
  cancel: (c: Connection) => post<JobProgress>(c, "/jobs/cancel"),
};

/** Matches the service's own `slug`, so a take's URL finds its file. */
export function slug(name: string): string {
  const s = name
    .trim()
    .replace(/[^A-Za-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
  return s || "voice";
}

/**
 * Subscribe to the service's progress stream.
 *
 * Reconnects on its own: the service is stopped when idle and restarted when
 * the tab needs it, and a socket that stayed dead after that would leave a
 * progress bar frozen with no explanation.
 */
export function watchJob(
  conn: Connection,
  onEvent: (event: { type: string } & Record<string, unknown>) => void,
): () => void {
  let socket: WebSocket | null = null;
  let timer: number | undefined;
  let closed = false;

  const open = () => {
    if (closed) return;
    const url = conn.baseUrl.replace(/^http/, "ws") + `/ws?token=${encodeURIComponent(conn.token)}`;
    socket = new WebSocket(url);
    socket.onmessage = (e) => {
      try {
        onEvent(JSON.parse(e.data));
      } catch {
        // A frame that is not JSON is not worth tearing the socket down for.
      }
    };
    socket.onclose = () => {
      if (!closed) timer = window.setTimeout(open, 2000);
    };
    socket.onerror = () => socket?.close();
    // A ping keeps the connection alive through anything that times idle
    // sockets out, and gives the service an "activity" signal while a long
    // job runs with nobody pressing anything.
    const ping = window.setInterval(() => {
      if (socket?.readyState === WebSocket.OPEN) socket.send("ping");
      else window.clearInterval(ping);
    }, 20000);
  };

  open();
  return () => {
    closed = true;
    window.clearTimeout(timer);
    socket?.close();
  };
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} bytes`;
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !isFinite(seconds)) return "—";
  const total = Math.round(seconds);
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${total % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
