"""Audio plumbing: read, write, resample, trim, fade, loudness, the radio effect.

Everything works on mono float32 numpy arrays in [-1, 1]. The sample rate
travels beside the array; nothing here assumes one.
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
import soundfile as sf


def read(path: Path | str) -> tuple[np.ndarray, int]:
    """Mono float32 and the file's sample rate."""
    data, sr = sf.read(str(path), dtype="float32", always_2d=True)
    return data.mean(axis=1).astype(np.float32), int(sr)


def write(path: Path | str, audio: np.ndarray, sr: int, subtype: str = "PCM_16") -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    x = np.clip(np.asarray(audio, dtype=np.float32).reshape(-1), -1.0, 1.0)
    # Write-then-rename: a half-written clip in a folder CrewChief picks from
    # at random would be played.
    tmp = p.with_suffix(p.suffix + ".tmp")
    # The format is stated because soundfile would otherwise read it off the
    # extension, and ".tmp" is not one.
    sf.write(str(tmp), x, sr, format="WAV", subtype=subtype)
    tmp.replace(p)


def resample(audio: np.ndarray, sr_in: int, sr_out: int) -> np.ndarray:
    if sr_in == sr_out:
        return np.asarray(audio, dtype=np.float32)
    from scipy.signal import resample_poly

    g = math.gcd(sr_in, sr_out)
    out = resample_poly(np.asarray(audio, dtype=np.float64), sr_out // g, sr_in // g)
    return out.astype(np.float32)


def duration(audio: np.ndarray, sr: int) -> float:
    return len(audio) / sr if sr else 0.0


# --------------------------------------------------------------------- trim


def trim_silence(audio: np.ndarray, sr: int, threshold_db: float = -45.0, pad_ms: int = 60) -> np.ndarray:
    """Cut leading and trailing quiet, keeping ``pad_ms`` of it at each end."""
    x = np.asarray(audio, dtype=np.float32)
    if len(x) == 0:
        return x
    frame = max(1, int(sr * 0.01))
    n = len(x) // frame
    if n == 0:
        return x
    rms = np.sqrt(np.mean(x[: n * frame].reshape(n, frame) ** 2, axis=1))
    db = 20 * np.log10(np.maximum(rms, 1e-9))
    loud = np.where(db > threshold_db)[0]
    if len(loud) == 0:
        return x
    pad = int(sr * pad_ms / 1000)
    start = max(0, loud[0] * frame - pad)
    end = min(len(x), (loud[-1] + 1) * frame + pad)
    return x[start:end]


def fade(audio: np.ndarray, sr: int, ms: int = 8) -> np.ndarray:
    """Short linear fades so a cut never clicks."""
    x = np.array(audio, dtype=np.float32, copy=True)
    n = min(len(x) // 2, int(sr * ms / 1000))
    if n > 0:
        ramp = np.linspace(0.0, 1.0, n, dtype=np.float32)
        x[:n] *= ramp
        x[-n:] *= ramp[::-1]
    return x


def pad(audio: np.ndarray, sr: int, before_ms: int = 0, after_ms: int = 0) -> np.ndarray:
    a = np.zeros(int(sr * before_ms / 1000), dtype=np.float32)
    b = np.zeros(int(sr * after_ms / 1000), dtype=np.float32)
    return np.concatenate([a, np.asarray(audio, dtype=np.float32), b])


# ----------------------------------------------------------------- loudness


def integrated_lufs(audio: np.ndarray, sr: int) -> float | None:
    """ITU-R BS.1770 integrated loudness; None when the clip is too short or silent."""
    import pyloudnorm as pyln

    x = np.asarray(audio, dtype=np.float32)
    if len(x) < int(0.4 * sr) + 1:
        # The gating block is 400 ms; pad rather than refuse a short clip.
        x = np.concatenate([x, np.zeros(int(0.4 * sr) + 1 - len(x), dtype=np.float32)])
    meter = pyln.Meter(sr)
    value = float(meter.integrated_loudness(x.astype(np.float64)))
    if not np.isfinite(value) or value < -70:
        return None
    return value


def match_loudness(audio: np.ndarray, sr: int, target_lufs: float, ceiling_dbfs: float = -1.0) -> np.ndarray:
    """Gain to the target loudness, then a hard ceiling on the peak."""
    x = np.asarray(audio, dtype=np.float32)
    current = integrated_lufs(x, sr)
    if current is None:
        return x
    gain = 10 ** ((target_lufs - current) / 20)
    y = x * gain
    peak = float(np.max(np.abs(y))) if len(y) else 0.0
    ceiling = 10 ** (ceiling_dbfs / 20)
    if peak > ceiling:
        y = y * (ceiling / peak)
    return y.astype(np.float32)


def peak_dbfs(audio: np.ndarray) -> float:
    peak = float(np.max(np.abs(audio))) if len(audio) else 0.0
    return 20 * math.log10(peak) if peak > 0 else -120.0


def measure_reference_loudness(paths: list[Path], limit: int = 60) -> float | None:
    """The mean integrated loudness of a set of files — CrewChief's own spotter clips."""
    values: list[float] = []
    for p in paths[:limit]:
        try:
            audio, sr = read(p)
        except Exception:
            continue
        v = integrated_lufs(audio, sr)
        if v is not None:
            values.append(v)
    if not values:
        return None
    return float(np.median(values))


# ------------------------------------------------------------- radio effect


def radio_effect(audio: np.ndarray, sr: int, amount: float = 1.0) -> np.ndarray:
    """A pit-radio sound: band-pass, light compression, a touch of saturation.

    ``amount`` scales how far it goes; 1.0 is the intended effect.
    """
    from scipy.signal import butter, sosfilt

    x = np.asarray(audio, dtype=np.float32)
    if len(x) == 0:
        return x
    nyq = sr / 2
    low, high = 300.0, min(3400.0, nyq * 0.95)
    sos = butter(4, [low / nyq, high / nyq], btype="band", output="sos")
    y = sosfilt(sos, x).astype(np.float32)

    # Compression: simple feed-forward RMS compressor, 4:1 above -18 dBFS.
    frame = max(1, int(sr * 0.01))
    env = np.sqrt(np.convolve(y**2, np.ones(frame) / frame, mode="same") + 1e-9)
    env_db = 20 * np.log10(np.maximum(env, 1e-6))
    threshold, ratio = -18.0, 4.0
    over = np.maximum(env_db - threshold, 0.0)
    gain_db = -over * (1 - 1 / ratio)
    y = y * (10 ** (gain_db / 20)).astype(np.float32)

    # Saturation: soft clip.
    drive = 1.0 + 2.0 * amount
    y = np.tanh(y * drive) / np.tanh(drive)

    # Match the original peak so the effect is not also a volume change.
    src_peak = float(np.max(np.abs(x))) or 1.0
    dst_peak = float(np.max(np.abs(y))) or 1.0
    y = y * (src_peak / dst_peak)
    return (x * (1 - amount) + y * amount).astype(np.float32)


# ------------------------------------------------------------- waveform


def waveform_peaks(audio: np.ndarray, points: int = 400) -> list[float]:
    """Peak per bucket, for drawing. 0..1."""
    x = np.abs(np.asarray(audio, dtype=np.float32))
    if len(x) == 0:
        return []
    points = max(1, min(points, len(x)))
    edges = np.linspace(0, len(x), points + 1, dtype=int)
    return [float(x[a:b].max()) if b > a else 0.0 for a, b in zip(edges[:-1], edges[1:])]


def time_stretch(audio: np.ndarray, sr: int, rate: float) -> np.ndarray:
    """Change speed without changing pitch. Costs quality; only used when a tone asks."""
    if abs(rate - 1.0) < 1e-3:
        return np.asarray(audio, dtype=np.float32)
    import librosa

    return librosa.effects.time_stretch(np.asarray(audio, dtype=np.float32), rate=rate).astype(np.float32)
