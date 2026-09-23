"""The generation queue: GPU workers, the QC loop, pause and resume.

One job runs at a time. It walks a pack's clip list, skips anything that
already passed, and hands the rest to N workers, each holding its own copy
of the engine and sharing one transcriber. Every attempt is written to the
pack's ``results.jsonl`` before the next one starts, so a job that dies —
power cut, Task Manager, the app closing — resumes from the last clip it
finished rather than from the beginning.

Pausing is cooperative: a worker finishes the clip in hand (a couple of
seconds) and then waits. That is what the launcher asks for when a race
starts, and what the user gets from the Pause button.
"""

from __future__ import annotations

import logging
import queue
import threading
import time
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import numpy as np

from . import audio as A
from . import qc
from .engines.base import SynthesisRequest, TTSEngine
from .engines.registry import create_engine
from .packs import ClipResult, Pack, PackStore
from .phrases import ClipSpec, preview_only
from .tones import ToneMap, ToneParams
from .voices import VoiceStore, now_iso

log = logging.getLogger(__name__)

DEFAULT_TARGET_LUFS = -18.0
MAX_WORKERS = 4


@dataclass
class Progress:
    state: str = "idle"  # idle | starting | running | paused | finishing | done | cancelled | failed
    pack_id: str | None = None
    scope: str | None = None
    total: int = 0
    done: int = 0
    passed: int = 0
    failed: int = 0
    skipped: int = 0
    attempts: int = 0
    workers: int = 0
    in_flight: dict[str, str] = field(default_factory=dict)  # worker name -> rel_path
    started_at: float | None = None
    finished_at: float | None = None
    seconds_per_clip: float | None = None
    eta_s: float | None = None
    paused: bool = False
    pause_reason: str | None = None
    error: str | None = None
    recent: list[dict] = field(default_factory=list)  # last few verdicts, newest first

    def to_dict(self) -> dict:
        d = self.__dict__.copy()
        d["elapsed_s"] = (time.time() - self.started_at) if self.started_at else 0.0
        if self.finished_at and self.started_at:
            d["elapsed_s"] = self.finished_at - self.started_at
        return d


def seed_for(rel_path: str, attempt: int, generation: int = 0) -> int:
    """Deterministic per clip and attempt, so a rerun reproduces a result exactly."""
    return zlib.crc32(f"{rel_path}#{generation}#{attempt}".encode("utf-8")) & 0x7FFFFFFF


def worker_count(engine: TTSEngine, requested: int | None = None) -> int:
    """How many engine copies the GPU has room for, leaving space for the transcriber."""
    if requested and requested > 0:
        return max(1, min(MAX_WORKERS, requested))
    try:
        import torch

        if not torch.cuda.is_available():
            return 1
        free, _total = torch.cuda.mem_get_info()
        free_gb = free / 2**30
        reserve_gb = 2.5  # whisper plus headroom
        n = int((free_gb - reserve_gb) // engine.vram_estimate_gb())
        return max(1, min(MAX_WORKERS, n))
    except Exception:
        return 1


class Runner:
    def __init__(
        self,
        packs: PackStore,
        voices: VoiceStore,
        tone_map: Callable[[], ToneMap],
        publish: Callable[[dict], None],
        transcriber: qc.Transcriber,
    ) -> None:
        self.packs = packs
        self.voices = voices
        self.tone_map = tone_map
        self.publish = publish
        self.transcriber = transcriber
        self.progress = Progress()
        self._resume = threading.Event()
        self._resume.set()
        self._cancel = threading.Event()
        self._threads: list[threading.Thread] = []
        self._lock = threading.Lock()
        self._durations: list[float] = []
        self._last_publish = 0.0
        self._engines: list[TTSEngine] = []
        self.last_activity = time.time()

    # ------------------------------------------------------------ control

    @property
    def busy(self) -> bool:
        return self.progress.state in ("starting", "running", "paused", "finishing")

    def start(
        self,
        pack: Pack,
        scope: str = "preview",
        rel_paths: list[str] | None = None,
        workers: int | None = None,
        regenerate: bool = False,
    ) -> Progress:
        with self._lock:
            if self.busy:
                raise RuntimeError("a job is already running")
            specs = self.packs.load_specs(pack.id)
            if scope == "preview":
                todo = preview_only(specs)
            elif scope == "selection":
                wanted = set(rel_paths or [])
                todo = [s for s in specs if s.rel_path in wanted]
            else:
                todo = list(specs)
            already = set() if regenerate else self.packs.passed_set(pack.id)
            skipped = [s for s in todo if s.rel_path in already]
            todo = [s for s in todo if s.rel_path not in already]

            self._cancel.clear()
            self._resume.set()
            self._durations = []
            self.progress = Progress(
                state="starting",
                pack_id=pack.id,
                scope=scope,
                total=len(todo) + len(skipped),
                done=len(skipped),
                skipped=len(skipped),
                started_at=time.time(),
            )
            self.last_activity = time.time()
            self._publish(force=True)

            generation = 0
            if regenerate:
                generation = int(time.time())

            q: "queue.Queue[ClipSpec]" = queue.Queue()
            for s in todo:
                q.put(s)

            supervisor = threading.Thread(
                target=self._supervise,
                args=(pack, q, workers, generation),
                name="voicelab-job",
                daemon=True,
            )
            supervisor.start()
            self._threads = [supervisor]
            return self.progress

    def pause(self, reason: str = "user") -> Progress:
        if self.busy:
            self._resume.clear()
            self.progress.paused = True
            self.progress.pause_reason = reason
            if self.progress.state == "running":
                self.progress.state = "paused"
            self._publish(force=True)
        return self.progress

    def resume(self) -> Progress:
        if self.busy:
            self.progress.paused = False
            self.progress.pause_reason = None
            if self.progress.state == "paused":
                self.progress.state = "running"
            self._resume.set()
            self._publish(force=True)
        return self.progress

    def cancel(self) -> Progress:
        if self.busy:
            self._cancel.set()
            self._resume.set()
            self.progress.state = "finishing"
            self._publish(force=True)
        return self.progress

    def status(self) -> dict:
        return self.progress.to_dict()

    # ---------------------------------------------------------- the work

    def _supervise(self, pack: Pack, q: "queue.Queue[ClipSpec]", workers: int | None, generation: int) -> None:
        try:
            voice = self.voices.load(pack.settings.voice_id)
            if voice is None:
                raise RuntimeError(f"voice {pack.settings.voice_id!r} no longer exists")
            tone_map = self.tone_map()
            engine0 = create_engine(pack.settings.engine)
            n = worker_count(engine0, workers)
            self.progress.workers = n
            self.progress.state = "running" if self._resume.is_set() else "paused"
            self._publish(force=True)

            # Load the transcriber once, up front, rather than on the first
            # verdict of four workers at once.
            self.transcriber.load()

            engines = [engine0] + [create_engine(pack.settings.engine) for _ in range(n - 1)]
            self._engines = engines
            threads = []
            for i, engine in enumerate(engines):
                t = threading.Thread(
                    target=self._worker,
                    args=(f"worker-{i + 1}", engine, pack, voice, tone_map, q, generation),
                    name=f"voicelab-worker-{i + 1}",
                    daemon=True,
                )
                t.start()
                threads.append(t)
            for t in threads:
                t.join()
            self.progress.state = "cancelled" if self._cancel.is_set() else "done"
        except Exception as e:
            log.exception("job failed")
            self.progress.state = "failed"
            self.progress.error = f"{type(e).__name__}: {e}"
        finally:
            self.progress.finished_at = time.time()
            self.progress.in_flight = {}
            self.progress.paused = False
            for engine in self._engines:
                try:
                    engine.unload()
                except Exception:
                    pass
            self._engines = []
            try:
                self.packs.refresh_counts(pack)
            except Exception:
                pass
            self._publish(force=True)

    def _worker(
        self,
        name: str,
        engine: TTSEngine,
        pack: Pack,
        voice,
        tone_map: ToneMap,
        q: "queue.Queue[ClipSpec]",
        generation: int,
    ) -> None:
        try:
            engine.load()
        except Exception as e:
            log.exception("%s could not load the engine", name)
            self.progress.error = f"engine failed to load: {e}"
            self._cancel.set()
            return
        while not self._cancel.is_set():
            self._resume.wait()
            if self._cancel.is_set():
                break
            try:
                spec = q.get_nowait()
            except queue.Empty:
                break
            self.progress.in_flight[name] = spec.rel_path
            self.last_activity = time.time()
            t0 = time.time()
            try:
                passed = self._make_clip(engine, pack, voice, tone_map, spec, generation)
            except Exception as e:
                log.exception("%s failed on %s", name, spec.rel_path)
                passed = False
                self.packs.append_result(
                    pack.id,
                    ClipResult(
                        rel_path=spec.rel_path,
                        attempt=0,
                        seed=0,
                        engine=engine.info.id,
                        tone="",
                        passed=False,
                        wer=1.0,
                        errors=0,
                        transcript="",
                        reasons=[f"error: {type(e).__name__}: {e}"],
                        duration_s=0.0,
                        confidence=None,
                        elapsed_s=time.time() - t0,
                        at=now_iso(),
                        text=spec.text,
                    ),
                )
            elapsed = time.time() - t0
            with self._lock:
                self._durations.append(elapsed)
                self._durations = self._durations[-40:]
                self.progress.done += 1
                if passed:
                    self.progress.passed += 1
                else:
                    self.progress.failed += 1
                self.progress.in_flight.pop(name, None)
                self._update_eta()
            self._publish()
        self.progress.in_flight.pop(name, None)

    def _update_eta(self) -> None:
        if not self._durations:
            return
        per_clip = float(np.mean(self._durations))
        workers = max(1, self.progress.workers)
        self.progress.seconds_per_clip = per_clip
        remaining = max(0, self.progress.total - self.progress.done)
        self.progress.eta_s = remaining * per_clip / workers

    def _make_clip(self, engine: TTSEngine, pack: Pack, voice, tone_map: ToneMap, spec: ClipSpec, generation: int) -> bool:
        settings = pack.settings
        tone: ToneParams = tone_map.tone_for(spec.intent)
        reference = self.voices.reference_for(voice, tone.tone)
        target_lufs = settings.target_lufs if settings.target_lufs is not None else DEFAULT_TARGET_LUFS
        last_audio: np.ndarray | None = None
        last_sr = engine.info.sample_rate

        for attempt in range(1, max(1, settings.max_attempts) + 1):
            if self._cancel.is_set():
                return False
            seed = seed_for(spec.rel_path, attempt, generation)
            t0 = time.time()
            raw, sr = engine.synthesize(SynthesisRequest(text=spec.text, reference=reference, tone=tone, seed=seed))
            audio = A.trim_silence(raw, sr, pad_ms=tone.pad_ms)
            if abs(tone.speed - 1.0) > 1e-3:
                audio = A.time_stretch(audio, sr, tone.speed)
            checks = qc.check_audio(audio, sr, spec.text, speed=1.0)
            audio_16k = qc.resample_to_16k(audio, sr)
            transcript, confidence = self.transcriber.transcribe(audio_16k)
            verdict = qc.judge(spec.text, transcript, checks, confidence)
            if not verdict.passed and "transcript_mismatch" in verdict.reasons and not checks.problems:
                # Only now pay for the teacher-forced score: the transcript
                # disagreed, and this is the check that can still rescue it.
                score = self.transcriber.score_target(audio_16k, spec.text)
                verdict = qc.judge(spec.text, transcript, checks, confidence, score)
            self.progress.attempts += 1
            self.packs.append_result(
                pack.id,
                ClipResult(
                    rel_path=spec.rel_path,
                    attempt=attempt,
                    seed=seed,
                    engine=engine.info.id,
                    tone=tone.tone,
                    passed=verdict.passed,
                    wer=verdict.wer,
                    errors=verdict.errors,
                    transcript=transcript,
                    reasons=verdict.reasons,
                    duration_s=checks.duration_s,
                    confidence=confidence,
                    elapsed_s=time.time() - t0,
                    at=now_iso(),
                    text=spec.text,
                ),
            )
            self._note_recent(spec, verdict, attempt)
            last_audio, last_sr = audio, sr
            if verdict.passed:
                final = self._finish(audio, sr, settings, target_lufs)
                A.write(self.packs.out_dir(pack.id) / spec.rel_path, final, settings.sample_rate, settings.subtype)
                # A clip that used to be a persistent failure is one no longer.
                failed = self.packs.failed_dir(pack.id) / spec.rel_path
                if failed.exists():
                    try:
                        failed.unlink()
                    except OSError:
                        pass
                return True

        # Every attempt failed: keep the last one where a person can hear it.
        if last_audio is not None:
            final = self._finish(last_audio, last_sr, settings, target_lufs)
            A.write(self.packs.failed_dir(pack.id) / spec.rel_path, final, settings.sample_rate, settings.subtype)
        return False

    def _finish(self, audio: np.ndarray, sr: int, settings, target_lufs: float) -> np.ndarray:
        x = A.fade(audio, sr, 8)
        x = A.match_loudness(x, sr, target_lufs)
        if settings.radio_effect:
            x = A.radio_effect(x, sr, settings.radio_amount)
        return A.resample(x, sr, settings.sample_rate)

    def _note_recent(self, spec: ClipSpec, verdict: qc.Verdict, attempt: int) -> None:
        entry = {
            "rel_path": spec.rel_path,
            "text": spec.text,
            "transcript": verdict.transcript,
            "passed": verdict.passed,
            "attempt": attempt,
            "wer": round(verdict.wer, 3),
            "reasons": verdict.reasons,
        }
        with self._lock:
            self.progress.recent.insert(0, entry)
            self.progress.recent = self.progress.recent[:12]

    def _publish(self, force: bool = False) -> None:
        now = time.time()
        if not force and now - self._last_publish < 0.5:
            return
        self._last_publish = now
        try:
            self.publish({"type": "job", **self.progress.to_dict()})
        except Exception:
            pass
