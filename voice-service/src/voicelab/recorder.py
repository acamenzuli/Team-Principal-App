"""Guided recording: the script, the microphone, and cleaning up a take.

Recording happens here, in Python, rather than in the webview: lossless PCM
from ``sounddevice``, a device list the user can choose from, and no
dependence on the browser's microphone permission prompt. Live levels
stream to the tab while a take is open; when it stops, the take is
trimmed with a voice activity detector, split on long pauses, checked for
clipping, noise and length, loudness-normalised, and stored against its
tone.
"""

from __future__ import annotations

import logging
import threading
import time
from dataclasses import dataclass, field
from typing import Callable

import numpy as np

from . import audio as A

log = logging.getLogger(__name__)

RECORD_SAMPLE_RATE = 48000
TAKE_TARGET_LUFS = -20.0
MIN_TAKE_SECONDS = 1.2
MAX_TAKE_SECONDS = 25.0
SPLIT_PAUSE_MS = 500
MIN_SNR_DB = 20.0


@dataclass(frozen=True)
class ScriptLine:
    id: str
    tone: str
    text: str
    hint: str = ""


# About twenty lines, three tones, written to cover the sounds a race
# engineer's phrases use — numbers, positions, gaps, tyres, flags — at the
# pace each tone wants. Each is long enough for a three to eight second
# take; the engine only ever uses the first ten seconds of a reference, so
# variety matters more than length.
SCRIPT: list[ScriptLine] = [
    ScriptLine("calm-1", "calm", "Okay, you're P3. The gap to the car in front is one point four seconds, and it's holding steady.", "Level, unhurried. Radio voice."),
    ScriptLine("calm-2", "calm", "Fuel is fine, we're good to the end. Tyre pressures look normal, front left is running a touch warm.", "Same pace. Let the numbers land."),
    ScriptLine("calm-3", "calm", "That was a one thirty two point five, personal best. Keep doing what you're doing.", ""),
    ScriptLine("calm-4", "calm", "Box this lap, box this lap. We'll take four tyres and a splash of fuel, in and out, nice and clean.", "A plan, not an alarm."),
    ScriptLine("calm-5", "calm", "Twenty laps to go. The car behind is two point eight seconds back and not making much of it.", ""),
    ScriptLine("calm-6", "calm", "Track temperature is coming down, so expect a bit more grip through turn seven and eight.", ""),
    ScriptLine("calm-7", "calm", "Understood. No more time deltas. I'll give you the gaps at the end of each lap.", "Acknowledging a request."),
    ScriptLine("urgent-1", "urgent", "Car left! Car left! Still there. Hold your line. Clear left.", "Fast and sharp. Spotter on the wall."),
    ScriptLine("urgent-2", "urgent", "Yellow flag, yellow flag, sector two. Slow down, there's a car stopped on the racing line.", "Quick, but every word clear."),
    ScriptLine("urgent-3", "urgent", "Three wide, you're in the middle! Give them room. Car right, car right. Clear all round.", ""),
    ScriptLine("urgent-4", "urgent", "We're running on fumes, mate. Box now, box now. Do not stay out.", "Serious. Faster than calm, not shouting."),
    ScriptLine("urgent-5", "urgent", "Puncture, front right! Bring it in, bring it in this lap. Take it easy through the fast stuff.", ""),
    ScriptLine("urgent-6", "urgent", "Red flag, red flag. Session stopped. Slow right down and get back to the pits.", ""),
    ScriptLine("urgent-7", "urgent", "Last lap! He's right behind you, half a second. Defend the inside into the final corner.", ""),
    ScriptLine("cheer-1", "celebratory", "Yes! That's the win! Brilliant drive, absolutely brilliant. Bring it home!", "Big. Genuinely pleased."),
    ScriptLine("cheer-2", "celebratory", "Fastest lap of the race, that one! Where did you find that? Superb.", ""),
    ScriptLine("cheer-3", "celebratory", "P1, P1! You did it. Great start, great race, great result. Well done mate.", ""),
    ScriptLine("cheer-4", "celebratory", "Nice one! You're up to second and the gap's coming down. Keep pushing, this is on.", "Excited but still talking to a driver."),
    ScriptLine("cheer-5", "celebratory", "Podium finish! Good job, that's a proper drive. Cool the brakes on the way in.", ""),
    ScriptLine("cheer-6", "celebratory", "Fantastic. Best lap of the weekend. The whole team's on their feet back here.", ""),
]


def script() -> list[dict]:
    return [line.__dict__ for line in SCRIPT]


# ---------------------------------------------------------------- devices


def list_input_devices() -> list[dict]:
    import sounddevice as sd

    out = []
    try:
        default_in = sd.default.device[0]
    except Exception:
        default_in = -1
    hostapis = sd.query_hostapis()
    for i, d in enumerate(sd.query_devices()):
        if d.get("max_input_channels", 0) <= 0:
            continue
        api = hostapis[d["hostapi"]]["name"] if 0 <= d["hostapi"] < len(hostapis) else ""
        out.append(
            {
                "index": i,
                "name": d["name"],
                "channels": int(d["max_input_channels"]),
                "sample_rate": int(d.get("default_samplerate") or 0),
                "host_api": api,
                "default": i == default_in,
            }
        )
    return out


# --------------------------------------------------------------- recorder


class Recorder:
    """One open take at a time."""

    def __init__(self, publish: Callable[[dict], None]) -> None:
        self.publish = publish
        self._stream = None
        self._blocks: list[np.ndarray] = []
        self._lock = threading.Lock()
        self._last_level = 0.0
        self.sample_rate = RECORD_SAMPLE_RATE
        self.device: int | None = None
        self.started_at: float | None = None
        self.peak = 0.0

    @property
    def recording(self) -> bool:
        return self._stream is not None

    def start(self, device: int | None = None) -> dict:
        import sounddevice as sd

        with self._lock:
            if self._stream is not None:
                raise RuntimeError("already recording")
            self._blocks = []
            self.peak = 0.0
            self.device = device
            try:
                info = sd.query_devices(device, "input") if device is not None else sd.query_devices(kind="input")
                sr = int(info.get("default_samplerate") or RECORD_SAMPLE_RATE)
            except Exception:
                sr = RECORD_SAMPLE_RATE
            self.sample_rate = sr if sr in (44100, 48000, 96000) else RECORD_SAMPLE_RATE

            def callback(indata, frames, t, status):  # noqa: ANN001
                if status:
                    log.debug("input status: %s", status)
                mono = np.asarray(indata, dtype=np.float32).mean(axis=1) if indata.ndim > 1 else np.asarray(indata, dtype=np.float32)
                self._blocks.append(mono.copy())
                peak = float(np.max(np.abs(mono))) if len(mono) else 0.0
                rms = float(np.sqrt(np.mean(mono**2))) if len(mono) else 0.0
                self.peak = max(self.peak, peak)
                now = time.time()
                if now - self._last_level >= 1 / 15:
                    self._last_level = now
                    self.publish(
                        {
                            "type": "level",
                            "peak": peak,
                            "rms": rms,
                            "peak_dbfs": A.peak_dbfs(mono),
                            "clipped": peak >= 0.99,
                            "seconds": (now - self.started_at) if self.started_at else 0.0,
                        }
                    )

            self._stream = sd.InputStream(
                device=device,
                channels=1,
                samplerate=self.sample_rate,
                dtype="float32",
                blocksize=int(self.sample_rate * 0.05),
                callback=callback,
            )
            self._stream.start()
            self.started_at = time.time()
            return {"sample_rate": self.sample_rate, "device": device}

    def stop(self) -> tuple[np.ndarray, int]:
        with self._lock:
            if self._stream is None:
                raise RuntimeError("not recording")
            try:
                self._stream.stop()
                self._stream.close()
            finally:
                self._stream = None
            audio = np.concatenate(self._blocks) if self._blocks else np.zeros(0, dtype=np.float32)
            self._blocks = []
            self.started_at = None
            return audio, self.sample_rate

    def cancel(self) -> None:
        with self._lock:
            if self._stream is not None:
                try:
                    self._stream.stop()
                    self._stream.close()
                finally:
                    self._stream = None
            self._blocks = []
            self.started_at = None


# ---------------------------------------------------------------- analysis


@dataclass
class TakeAnalysis:
    duration_s: float
    speech_s: float
    segments: list[tuple[float, float]]
    snr_db: float | None
    clipped: bool
    clipped_ratio: float
    peak_dbfs: float
    lufs: float | None
    problems: list[str] = field(default_factory=list)
    waveform: list[float] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not [p for p in self.problems if p in ("too_short", "clipping", "no_speech")]

    def to_dict(self) -> dict:
        d = self.__dict__.copy()
        d["ok"] = self.ok
        d["segments"] = [list(s) for s in self.segments]
        return d


_vad_model = None
_vad_lock = threading.Lock()


def _vad():
    global _vad_model
    with _vad_lock:
        if _vad_model is None:
            from silero_vad import load_silero_vad

            _vad_model = load_silero_vad()
        return _vad_model


def speech_segments(audio: np.ndarray, sr: int, min_silence_ms: int = SPLIT_PAUSE_MS) -> list[tuple[float, float]]:
    """(start, end) seconds of speech, split where a pause exceeds ``min_silence_ms``."""
    import torch
    from silero_vad import get_speech_timestamps

    x16 = A.resample(audio, sr, 16000)
    if len(x16) == 0:
        return []
    stamps = get_speech_timestamps(
        torch.from_numpy(np.ascontiguousarray(x16)),
        _vad(),
        sampling_rate=16000,
        min_silence_duration_ms=min_silence_ms,
        speech_pad_ms=120,
        threshold=0.5,
    )
    return [(s["start"] / 16000, s["end"] / 16000) for s in stamps]


def analyse_take(audio: np.ndarray, sr: int) -> tuple[np.ndarray, TakeAnalysis]:
    """Clean a raw take and say what is wrong with it. Returns (cleaned, analysis)."""
    x = np.asarray(audio, dtype=np.float32)
    duration = len(x) / sr if sr else 0.0
    problems: list[str] = []
    clipped_ratio = float(np.mean(np.abs(x) >= 0.99)) if len(x) else 0.0
    peak = A.peak_dbfs(x) if len(x) else -120.0

    segments = speech_segments(x, sr) if len(x) else []
    speech_s = sum(e - s for s, e in segments)
    if not segments:
        problems.append("no_speech")
        return x, TakeAnalysis(duration, 0.0, [], None, clipped_ratio > 0, clipped_ratio, peak, None, problems, A.waveform_peaks(x))

    # Noise floor from what the VAD called silence; speech level from the rest.
    mask = np.zeros(len(x), dtype=bool)
    for s, e in segments:
        mask[int(s * sr) : int(e * sr)] = True
    speech_rms = float(np.sqrt(np.mean(x[mask] ** 2))) if mask.any() else 0.0
    noise = x[~mask]
    noise_rms = float(np.sqrt(np.mean(noise**2))) if len(noise) > sr * 0.2 else None
    snr = 20 * np.log10(speech_rms / noise_rms) if noise_rms and noise_rms > 0 and speech_rms > 0 else None

    # Reassemble: speech segments with a short natural gap, long pauses gone.
    gap = np.zeros(int(sr * 0.25), dtype=np.float32)
    pieces = []
    for s, e in segments:
        piece = x[max(0, int(s * sr) - int(sr * 0.05)) : min(len(x), int(e * sr) + int(sr * 0.08))]
        pieces.append(A.fade(piece, sr, 6))
        pieces.append(gap)
    cleaned = np.concatenate(pieces[:-1]) if len(pieces) > 1 else pieces[0]
    cleaned = A.match_loudness(cleaned, sr, TAKE_TARGET_LUFS)
    lufs = A.integrated_lufs(cleaned, sr)

    if clipped_ratio > 0.0005:
        problems.append("clipping")
    if speech_s < MIN_TAKE_SECONDS:
        problems.append("too_short")
    if speech_s > MAX_TAKE_SECONDS:
        problems.append("too_long")
    if snr is not None and snr < MIN_SNR_DB:
        problems.append("noisy")
    if peak < -35:
        problems.append("too_quiet")

    analysis = TakeAnalysis(
        duration_s=round(duration, 3),
        speech_s=round(speech_s, 3),
        segments=[(round(s, 3), round(e, 3)) for s, e in segments],
        snr_db=round(float(snr), 1) if snr is not None else None,
        clipped=clipped_ratio > 0.0005,
        clipped_ratio=round(clipped_ratio, 5),
        peak_dbfs=round(peak, 2),
        lufs=round(lufs, 2) if lufs is not None else None,
        problems=problems,
        waveform=A.waveform_peaks(cleaned),
    )
    return cleaned, analysis


def describe_problems(problems: list[str]) -> str:
    names = {
        "no_speech": "no speech was detected",
        "clipping": "the signal clipped — move back from the mic or turn the gain down",
        "too_short": "too short — a take needs at least a couple of seconds of speech",
        "too_long": "very long — one line at a time works best",
        "noisy": "background noise is high — a quieter room or a closer mic helps",
        "too_quiet": "very quiet — turn the gain up or come closer",
    }
    return "; ".join(names.get(p, p) for p in problems)
