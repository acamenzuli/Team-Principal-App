"""The HTTP face of the service.

Started by Team Principal with a port it chose and a token in the
environment; bound to 127.0.0.1 only; every request must carry the token.
The frontend calls these routes directly and listens on ``/ws`` for
progress. ``/health`` is what the app polls to know the service is up, and
what tells it how long the service has been idle.
"""

# No `from __future__ import annotations` here: FastAPI resolves the type
# hints of route functions and dependencies at runtime, and a deferred
# `Request` annotation it cannot resolve turns into a required query
# parameter called "request".
import argparse
import asyncio
import json
import logging
import os
import sys
import threading
import time
from pathlib import Path
from typing import Any

from . import __version__
from .config import ENV_TOKEN, Paths, configure_environment, default_home

log = logging.getLogger("voicelab")

WEBVIEW_ORIGINS = [
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
]


class ServiceState:
    """Everything the routes share. Built once in ``run``."""

    def __init__(self, paths: Paths, token: str, sounds: str | None) -> None:
        from . import qc
        from .jobs import Runner
        from .models import Downloader
        from .packs import PackStore
        from .recorder import Recorder
        from .tones import load_tone_map
        from .voices import VoiceStore

        self.paths = paths.ensure()
        self.token = token
        self.started_at = time.time()
        self.last_activity = time.time()
        self.explicit_sounds = sounds
        self.loop: asyncio.AbstractEventLoop | None = None
        self.subscribers: set[Any] = set()
        self.transcriber = qc.Transcriber()
        self.packs = PackStore(self.paths.packs)
        self.voices = VoiceStore(self.paths.voices)
        self.downloader = Downloader()
        self.recorder = Recorder(self.publish)
        self.tone_map = lambda: load_tone_map(self.paths.tones_file)
        self.runner = Runner(self.packs, self.voices, self.tone_map, self.publish, self.transcriber)
        self.record_target: dict | None = None

    # ------------------------------------------------------------ events

    def publish(self, event: dict) -> None:
        """Called from any thread; fans out to every websocket."""
        loop = self.loop
        if loop is None or not self.subscribers:
            return
        message = json.dumps(event)

        def _send() -> None:
            for ws in list(self.subscribers):
                asyncio.ensure_future(_safe_send(ws, message, self))

        loop.call_soon_threadsafe(_send)

    def touch(self) -> None:
        self.last_activity = time.time()

    @property
    def idle_seconds(self) -> float:
        busy = self.runner.busy or self.recorder.recording or self.downloader.busy
        if busy:
            return 0.0
        return time.time() - max(self.last_activity, self.runner.last_activity)

    def sounds_folder(self) -> Path | None:
        from .crewchief import find_sounds_folder

        return find_sounds_folder(self.explicit_sounds)


async def _safe_send(ws, message: str, state: ServiceState) -> None:
    try:
        await ws.send_text(message)
    except Exception:
        state.subscribers.discard(ws)


# ------------------------------------------------------------------- app


def create_app(state: ServiceState):
    from fastapi import Depends, FastAPI, HTTPException, Request, WebSocket, WebSocketDisconnect
    from fastapi.middleware.cors import CORSMiddleware
    from fastapi.responses import FileResponse, JSONResponse
    from pydantic import BaseModel

    app = FastAPI(title="Team Principal Voice Lab", version=__version__, docs_url=None, redoc_url=None)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=WEBVIEW_ORIGINS,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    def require_token(request: Request) -> None:
        auth = request.headers.get("authorization", "")
        supplied = auth[7:] if auth.lower().startswith("bearer ") else request.query_params.get("token", "")
        if not supplied or supplied != state.token:
            raise HTTPException(status_code=401, detail="missing or wrong token")
        state.touch()

    auth = [Depends(require_token)]

    def error(status: int, message: str) -> JSONResponse:
        return JSONResponse(status_code=status, content={"detail": message})

    # ------------------------------------------------------------ health

    @app.get("/health", dependencies=auth)
    def health() -> dict:
        gpu = gpu_summary()
        return {
            "ok": True,
            "version": __version__,
            "pid": os.getpid(),
            "home": str(state.paths.home),
            "uptime_s": round(time.time() - state.started_at, 1),
            "idle_seconds": round(state.idle_seconds, 1),
            "busy": state.runner.busy or state.recorder.recording or state.downloader.busy,
            "job": state.runner.status(),
            "gpu": gpu,
            "sounds_folder": str(state.sounds_folder()) if state.sounds_folder() else None,
        }

    @app.post("/shutdown", dependencies=auth)
    def shutdown() -> dict:
        def _exit() -> None:
            time.sleep(0.3)
            os._exit(0)

        state.runner.cancel()
        state.recorder.cancel()
        threading.Thread(target=_exit, daemon=True).start()
        return {"ok": True}

    # ----------------------------------------------------------- crewchief

    @app.get("/crewchief", dependencies=auth)
    def crewchief_info() -> dict:
        from . import crewchief

        sounds = state.sounds_folder()
        info: dict[str, Any] = {
            "sounds_folder": str(sounds) if sounds else None,
            "candidates": [str(p) for p in crewchief.candidate_sounds_folders(state.explicit_sounds)],
            "config": crewchief.read_user_config_values(
                ("chief_name", "spotter_name", "PERSONALISATION_NAME", "override_default_sound_pack_location")
            ),
            "config_files": [str(p) for p in crewchief.user_config_files()[:1]],
        }
        if sounds:
            info["probe"] = crewchief.probe(sounds).to_dict()
            info["installed"] = crewchief.installed_packs(sounds)
        return info

    class FolderBody(BaseModel):
        path: str | None = None

    @app.post("/crewchief/folder", dependencies=auth)
    def set_folder(body: FolderBody):
        from . import crewchief

        if body.path:
            p = Path(body.path)
            if not crewchief.looks_like_sounds_folder(p):
                return error(400, f"{p} does not contain a voice folder, so it is not a CrewChief sounds folder")
        state.explicit_sounds = body.path or None
        return crewchief_info()

    @app.get("/inventory", dependencies=auth)
    def inventory(voice_name: str = "Voice", variants: int = 2, your_name: str | None = None):
        from .phrases import BuildOptions, build_clip_specs, summarise

        sounds = state.sounds_folder()
        if not sounds:
            return error(404, "no CrewChief sounds folder found")
        specs = build_clip_specs(sounds, BuildOptions(voice_name=voice_name, variants=variants, your_name=your_name))
        summary = summarise(specs).to_dict()
        by_category: dict[str, int] = {}
        for s in specs:
            cat = s.intent.split("/", 1)[0]
            by_category[cat] = by_category.get(cat, 0) + 1
        summary["by_category"] = by_category
        summary["sample"] = [s.__dict__ for s in specs[:30]]
        return summary

    # --------------------------------------------------------------- tones

    @app.get("/tones", dependencies=auth)
    def tones() -> dict:
        m = state.tone_map()
        return {**m.to_dict(), "raw": m.raw, "path": str(state.paths.tones_file)}

    class TonesBody(BaseModel):
        raw: dict

    @app.put("/tones", dependencies=auth)
    def put_tones(body: TonesBody):
        from .tones import save_tone_map

        try:
            m = save_tone_map(state.paths.tones_file, body.raw)
        except ValueError as e:
            return error(400, str(e))
        return {**m.to_dict(), "raw": m.raw, "path": str(state.paths.tones_file)}

    @app.post("/tones/reset", dependencies=auth)
    def reset_tones() -> dict:
        from .tones import DEFAULT_TONE_MAP, save_tone_map

        m = save_tone_map(state.paths.tones_file, DEFAULT_TONE_MAP)
        return {**m.to_dict(), "raw": m.raw, "path": str(state.paths.tones_file)}

    # ------------------------------------------------------------- engines

    @app.get("/engines", dependencies=auth)
    def engines() -> list[dict]:
        from .engines.registry import list_engines

        return [e.to_dict() for e in list_engines()]

    # -------------------------------------------------------------- models

    @app.get("/models", dependencies=auth)
    def models_status(engines: str | None = None) -> dict:
        ids = [e for e in (engines or "").split(",") if e] or None
        return state.downloader.snapshot(ids)

    class DownloadBody(BaseModel):
        engines: list[str] | None = None

    @app.post("/models/download", dependencies=auth)
    def models_download(body: DownloadBody) -> dict:
        ids = body.engines or None
        state.downloader.start(ids, lambda snap: state.publish({"type": "models", **snap}))
        return state.downloader.snapshot(ids)

    @app.post("/models/cancel", dependencies=auth)
    def models_cancel() -> dict:
        state.downloader.cancel()
        return state.downloader.snapshot(None)

    # ------------------------------------------------------------ recording

    @app.get("/devices", dependencies=auth)
    def devices() -> list[dict]:
        from .recorder import list_input_devices

        try:
            return list_input_devices()
        except Exception as e:
            raise HTTPException(status_code=500, detail=f"could not list audio devices: {e}")

    @app.get("/script", dependencies=auth)
    def script() -> list[dict]:
        from .recorder import script as lines

        return lines()

    class RecordStart(BaseModel):
        voice_id: str
        tone: str
        line_id: str
        device: int | None = None

    @app.post("/record/start", dependencies=auth)
    def record_start(body: RecordStart):
        if state.voices.load(body.voice_id) is None:
            return error(404, "no such voice")
        if body.voice_id == "builtin":
            return error(400, "the built-in voice cannot be recorded over")
        try:
            info = state.recorder.start(body.device)
        except Exception as e:
            return error(409, f"could not start recording: {e}")
        state.record_target = body.model_dump()
        return {"recording": True, **info}

    @app.post("/record/stop", dependencies=auth)
    def record_stop():
        from .recorder import analyse_take, describe_problems

        target = state.record_target
        try:
            audio, sr = state.recorder.stop()
        except Exception as e:
            return error(409, str(e))
        state.record_target = None
        if target is None:
            return error(409, "no take was open")
        cleaned, analysis = analyse_take(audio, sr)
        saved: str | None = None
        if analysis.ok:
            path = state.voices.add_take(target["voice_id"], target["tone"], target["line_id"], cleaned, sr)
            saved = str(path)
        voice = state.voices.load(target["voice_id"])
        return {
            "analysis": analysis.to_dict(),
            "message": describe_problems(analysis.problems),
            "saved": saved,
            "voice": voice.to_dict() if voice else None,
            **target,
        }

    @app.post("/record/cancel", dependencies=auth)
    def record_cancel() -> dict:
        state.recorder.cancel()
        state.record_target = None
        return {"recording": False}

    # --------------------------------------------------------------- voices

    @app.get("/voices", dependencies=auth)
    def voices() -> list[dict]:
        return [v.to_dict() for v in state.voices.list()]

    class VoiceBody(BaseModel):
        name: str

    @app.post("/voices", dependencies=auth)
    def create_voice(body: VoiceBody) -> dict:
        return state.voices.create(body.name).to_dict()

    @app.get("/voices/{voice_id}", dependencies=auth)
    def get_voice(voice_id: str):
        v = state.voices.load(voice_id)
        if v is None:
            return error(404, "no such voice")
        return v.to_dict()

    @app.delete("/voices/{voice_id}", dependencies=auth)
    def delete_voice(voice_id: str) -> dict:
        return {"deleted": state.voices.delete(voice_id)}

    class AttestBody(BaseModel):
        text: str

    @app.post("/voices/{voice_id}/attest", dependencies=auth)
    def attest(voice_id: str, body: AttestBody):
        v = state.voices.attest(voice_id, body.text)
        if v is None:
            return error(404, "no such voice")
        return v.to_dict()

    @app.get("/voices/{voice_id}/takes/{tone}/{line_id}.wav", dependencies=auth)
    def take_audio(voice_id: str, tone: str, line_id: str):
        from .voices import slug

        p = state.voices.dir(voice_id) / "takes" / tone / f"{slug(line_id)}.wav"
        if not p.is_file():
            return error(404, "no such take")
        return FileResponse(str(p), media_type="audio/wav")

    @app.delete("/voices/{voice_id}/takes/{tone}/{line_id}", dependencies=auth)
    def delete_take(voice_id: str, tone: str, line_id: str):
        state.voices.remove_take(voice_id, tone, line_id)
        v = state.voices.load(voice_id)
        return v.to_dict() if v else error(404, "no such voice")

    @app.get("/voices/{voice_id}/ref/{tone}.wav", dependencies=auth)
    def ref_audio(voice_id: str, tone: str):
        p = state.voices.dir(voice_id) / f"ref_{tone}.wav"
        if not p.is_file():
            return error(404, "no reference for that tone yet")
        return FileResponse(str(p), media_type="audio/wav")

    class ImportBody(BaseModel):
        name: str
        folder: str

    @app.post("/voices/import-dev", dependencies=auth)
    def import_dev(body: ImportBody):
        """Development only; refused unless VOICELAB_DEV_IMPORT=1."""
        try:
            return state.voices.create_from_folder(body.name, Path(body.folder)).to_dict()
        except PermissionError as e:
            return error(403, str(e))
        except FileNotFoundError as e:
            return error(404, str(e))

    # ---------------------------------------------------------------- packs

    @app.get("/packs", dependencies=auth)
    def packs() -> list[dict]:
        return [p.to_dict() for p in state.packs.list()]

    class PackBody(BaseModel):
        voice_id: str
        voice_name: str
        engine: str = "chatterbox"
        variants: int = 2
        your_name: str | None = None
        radio_effect: bool = False
        radio_amount: float = 1.0
        max_attempts: int = 4
        include_spotter: bool = True
        include_radio_check: bool = True
        include_personalisation: bool = True
        attestation: str | None = None

    @app.post("/packs", dependencies=auth)
    def create_pack(body: PackBody):
        from . import crewchief
        from .audio import measure_reference_loudness
        from .packs import PackSettings, safe_voice_name
        from .voices import now_iso

        sounds = state.sounds_folder()
        if not sounds:
            return error(404, "no CrewChief sounds folder found")
        voice = state.voices.load(body.voice_id)
        if voice is None:
            return error(404, "no such voice")
        name = safe_voice_name(body.voice_name)
        if not name:
            return error(400, "the voice needs a name CrewChief can use as a folder")
        attestation = None
        if body.voice_id != "builtin":
            text = (body.attestation or "").strip()
            if not text:
                return error(400, "an attestation is required before generating from a recorded voice")
            attestation = {"text": text, "at": now_iso()}
            state.voices.attest(body.voice_id, text)
        else:
            attestation = {"text": voice.attestation.text if voice.attestation else "", "at": now_iso()}
        fmt = crewchief.probe(sounds, sample_files=12).wav_format
        spotter_dir = sounds / "voice" / "spotter"
        refs = sorted(spotter_dir.rglob("*.wav"))[:60] if spotter_dir.is_dir() else []
        target = measure_reference_loudness(refs)
        settings = PackSettings(
            voice_name=name,
            voice_id=body.voice_id,
            engine=body.engine,
            variants=max(1, min(4, body.variants)),
            your_name=(body.your_name or "").strip() or None,
            radio_effect=body.radio_effect,
            radio_amount=max(0.0, min(1.0, body.radio_amount)),
            target_lufs=target,
            sample_rate=fmt.sample_rate if fmt else 22050,
            subtype=fmt.subtype if fmt else "PCM_16",
            max_attempts=max(1, min(8, body.max_attempts)),
            include_spotter=body.include_spotter,
            include_radio_check=body.include_radio_check,
            include_personalisation=body.include_personalisation,
        )
        pack, specs = state.packs.create(settings, sounds, attestation)
        return pack.to_dict()

    @app.get("/packs/{pack_id}", dependencies=auth)
    def get_pack(pack_id: str):
        p = state.packs.load(pack_id)
        if p is None:
            return error(404, "no such pack")
        return state.packs.refresh_counts(p).to_dict()

    @app.delete("/packs/{pack_id}", dependencies=auth)
    def delete_pack(pack_id: str):
        if state.runner.busy and state.runner.progress.pack_id == pack_id:
            return error(409, "that pack is being generated; cancel the job first")
        return {"deleted": state.packs.delete(pack_id)}

    @app.get("/packs/{pack_id}/clips", dependencies=auth)
    def clips(pack_id: str, state_filter: str | None = None, role: str | None = None, q: str | None = None,
              marked: bool | None = None, derived: bool | None = None, preview: bool | None = None,
              low_confidence: bool | None = None, recent_minutes: int | None = None, limit: int = 500, offset: int = 0):
        p = state.packs.load(pack_id)
        if p is None:
            return error(404, "no such pack")
        rows = state.packs.clip_view(pack_id)
        if state_filter:
            rows = [r for r in rows if r["state"] == state_filter]
        if role:
            rows = [r for r in rows if r["role"] == role]
        if marked is not None:
            rows = [r for r in rows if bool(r["review"].get("marked")) == marked]
        if derived is not None:
            rows = [r for r in rows if r["derived"] == derived]
        if preview is not None:
            rows = [r for r in rows if r["preview"] == preview]
        if low_confidence:
            rows = [r for r in rows if r["confidence"] is not None and r["confidence"] < -0.8]
        if recent_minutes:
            cutoff = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - recent_minutes * 60))
            rows = [r for r in rows if r["at"] and r["at"] >= cutoff]
        if q:
            needle = q.lower()
            rows = [r for r in rows if needle in r["rel_path"].lower() or needle in r["text"].lower()
                    or (r["transcript"] and needle in r["transcript"].lower())]
        total = len(rows)
        return {"total": total, "offset": offset, "clips": rows[offset : offset + limit]}

    @app.get("/packs/{pack_id}/audio", dependencies=auth)
    def clip_audio(pack_id: str, rel_path: str, which: str = "out"):
        base = state.packs.out_dir(pack_id) if which == "out" else state.packs.failed_dir(pack_id)
        p = (base / rel_path).resolve()
        if not str(p).startswith(str(base.resolve())) or not p.is_file():
            return error(404, "no audio for that clip")
        return FileResponse(str(p), media_type="audio/wav")

    class MarkBody(BaseModel):
        rel_path: str
        marked: bool | None = None
        note: str | None = None

    @app.post("/packs/{pack_id}/mark", dependencies=auth)
    def mark(pack_id: str, body: MarkBody):
        if state.packs.load(pack_id) is None:
            return error(404, "no such pack")
        fields: dict = {}
        if body.marked is not None:
            fields["marked"] = True if body.marked else None
        if body.note is not None:
            fields["note"] = body.note.strip() or None
        return state.packs.update_review(pack_id, body.rel_path, **fields)

    class TextBody(BaseModel):
        rel_path: str
        text: str | None = None

    @app.post("/packs/{pack_id}/text", dependencies=auth)
    def set_text(pack_id: str, body: TextBody):
        if state.packs.load(pack_id) is None:
            return error(404, "no such pack")
        state.packs.set_text_override(pack_id, body.rel_path, body.text)
        return {"ok": True}

    class GenerateBody(BaseModel):
        scope: str = "preview"  # preview | full | selection
        rel_paths: list[str] | None = None
        workers: int | None = None
        regenerate: bool = False

    @app.post("/packs/{pack_id}/generate", dependencies=auth)
    def generate(pack_id: str, body: GenerateBody):
        p = state.packs.load(pack_id)
        if p is None:
            return error(404, "no such pack")
        try:
            progress = state.runner.start(p, body.scope, body.rel_paths, body.workers, body.regenerate)
        except RuntimeError as e:
            return error(409, str(e))
        return progress.to_dict()

    @app.get("/packs/{pack_id}/install/plan", dependencies=auth)
    def install_plan(pack_id: str):
        from . import crewchief

        p = state.packs.load(pack_id)
        sounds = state.sounds_folder()
        if p is None:
            return error(404, "no such pack")
        if not sounds:
            return error(404, "no CrewChief sounds folder found")
        return crewchief.plan_install(state.packs.out_dir(pack_id), sounds, p.settings.voice_name).__dict__

    @app.post("/packs/{pack_id}/install", dependencies=auth)
    def install(pack_id: str):
        from . import crewchief
        from .voices import now_iso

        p = state.packs.load(pack_id)
        sounds = state.sounds_folder()
        if p is None:
            return error(404, "no such pack")
        if not sounds:
            return error(404, "no CrewChief sounds folder found")
        if state.runner.busy and state.runner.progress.pack_id == pack_id:
            return error(409, "wait for generation to finish, or pause it, before installing")
        result = crewchief.install(
            state.packs.out_dir(pack_id), sounds, p.settings.voice_name, state.paths.home / "backups", pack_id
        )
        p.installed_at = now_iso()
        state.packs.save(p)
        return result.__dict__

    @app.post("/packs/{pack_id}/uninstall", dependencies=auth)
    def uninstall(pack_id: str):
        from . import crewchief

        p = state.packs.load(pack_id)
        sounds = state.sounds_folder()
        if p is None:
            return error(404, "no such pack")
        if not sounds:
            return error(404, "no CrewChief sounds folder found")
        result = crewchief.uninstall(sounds, p.settings.voice_name)
        p.installed_at = None
        state.packs.save(p)
        return result

    class ExportBody(BaseModel):
        dest: str | None = None

    @app.post("/packs/{pack_id}/export", dependencies=auth)
    def export(pack_id: str, body: ExportBody):
        p = state.packs.load(pack_id)
        if p is None:
            return error(404, "no such pack")
        dest = Path(body.dest) if body.dest else state.paths.home / "exports" / f"{pack_id}.zip"
        return {"path": str(state.packs.export_zip(p, dest))}

    # ----------------------------------------------------------------- jobs

    @app.get("/jobs", dependencies=auth)
    def jobs() -> dict:
        return state.runner.status()

    class PauseBody(BaseModel):
        reason: str = "user"

    @app.post("/jobs/pause", dependencies=auth)
    def pause(body: PauseBody) -> dict:
        return state.runner.pause(body.reason).to_dict()

    @app.post("/jobs/resume", dependencies=auth)
    def resume() -> dict:
        return state.runner.resume().to_dict()

    @app.post("/jobs/cancel", dependencies=auth)
    def cancel() -> dict:
        return state.runner.cancel().to_dict()

    # ------------------------------------------------------------ websocket

    @app.websocket("/ws")
    async def ws(websocket: WebSocket):
        token = websocket.query_params.get("token", "")
        if token != state.token:
            await websocket.close(code=4401)
            return
        await websocket.accept()
        state.subscribers.add(websocket)
        try:
            await websocket.send_text(json.dumps({"type": "job", **state.runner.status()}))
            while True:
                # Clients send pings; anything received keeps the socket alive.
                await websocket.receive_text()
                state.touch()
        except WebSocketDisconnect:
            pass
        except Exception:
            pass
        finally:
            state.subscribers.discard(websocket)

    @app.on_event("startup")
    async def _startup() -> None:
        state.loop = asyncio.get_running_loop()

    return app


def gpu_summary() -> dict:
    try:
        import torch

        if not torch.cuda.is_available():
            return {"available": False}
        free, total = torch.cuda.mem_get_info()
        return {
            "available": True,
            "name": torch.cuda.get_device_name(0),
            "vram_total_gb": round(total / 2**30, 2),
            "vram_free_gb": round(free / 2**30, 2),
            "cuda": torch.version.cuda,
            "torch": torch.__version__,
        }
    except Exception as e:
        return {"available": False, "error": str(e)}


# ------------------------------------------------------------------- main


def _setup_logging(paths: Paths) -> None:
    from logging.handlers import RotatingFileHandler

    paths.logs.mkdir(parents=True, exist_ok=True)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    fh = RotatingFileHandler(paths.logs / "voice-service.log", maxBytes=2_000_000, backupCount=3, encoding="utf-8")
    fh.setFormatter(fmt)
    root.addHandler(fh)
    sh = logging.StreamHandler(sys.stderr)
    sh.setFormatter(fmt)
    root.addHandler(sh)
    for noisy in ("httpx", "httpcore", "urllib3", "huggingface_hub", "filelock"):
        logging.getLogger(noisy).setLevel(logging.WARNING)


def run(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="voicelab", description="Team Principal Voice Lab service")
    parser.add_argument("--port", type=int, default=0, help="port on 127.0.0.1 (0 = pick one and print it)")
    parser.add_argument("--home", type=str, default=None, help="the Voice Lab home folder")
    parser.add_argument("--sounds", type=str, default=None, help="CrewChief sounds folder, if the app knows it")
    parser.add_argument("--token", type=str, default=None, help=f"session token (or ${ENV_TOKEN})")
    args = parser.parse_args(argv)

    paths = Paths(Path(args.home) if args.home else default_home()).ensure()
    configure_environment(paths)
    _setup_logging(paths)

    token = args.token or os.environ.get(ENV_TOKEN)
    if not token:
        import secrets

        token = secrets.token_urlsafe(24)
        log.warning("no token supplied; generated one for this session")

    port = args.port
    if port == 0:
        import socket

        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]

    state = ServiceState(paths, token, args.sounds)
    app = create_app(state)

    import uvicorn

    # Announce the port on stdout in one parseable line so a parent that
    # asked for port 0 can find us; nothing else is written to stdout.
    print(json.dumps({"voicelab": {"port": port, "pid": os.getpid(), "version": __version__}}), flush=True)
    log.info("voice lab service %s on 127.0.0.1:%d, home %s", __version__, port, paths.home)
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning", access_log=False)
