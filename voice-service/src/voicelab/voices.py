"""Voices: a set of reference recordings, one per tone, and the consent that goes with them.

A voice lives in ``voices/<id>/``:

```
voice.json            name, tones present, attestation, source
takes/<tone>/<line>.wav    cleaned takes from the recorder
ref_<tone>.wav        the takes for a tone joined into one reference clip
```

The product makes voices only through the recorder. ``create_from_folder``
exists for development (``VOICELAB_DEV_IMPORT=1``) so the pipeline can be
exercised with a stand-in voice before anyone has recorded anything; a
shipped build never enables it.
"""

from __future__ import annotations

import json
import os
import re
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path

import numpy as np

from . import audio as A
from .tones import TONES

BUILTIN_VOICE_ID = "builtin"
REF_SAMPLE_RATE = 24000
# Chatterbox reads up to ten seconds of the reference for the acoustic
# prompt; a little more than that gives the speaker embedding more to go on
# without making the file large.
MAX_REFERENCE_SECONDS = 14.0
GAP_SECONDS = 0.25


@dataclass
class Attestation:
    text: str
    at: str  # ISO-8601 UTC


@dataclass
class Voice:
    id: str
    name: str
    created_at: str
    source: str  # "recorded" | "imported-dev" | "builtin"
    tones: dict[str, str] = field(default_factory=dict)  # tone -> ref file name
    attestation: Attestation | None = None
    takes: dict[str, list[str]] = field(default_factory=dict)  # tone -> take file names

    def to_dict(self) -> dict:
        d = asdict(self)
        return d

    @staticmethod
    def from_dict(d: dict) -> "Voice":
        att = d.get("attestation")
        return Voice(
            id=d["id"],
            name=d.get("name", d["id"]),
            created_at=d.get("created_at", ""),
            source=d.get("source", "recorded"),
            tones=dict(d.get("tones") or {}),
            attestation=Attestation(**att) if att else None,
            takes={k: list(v) for k, v in (d.get("takes") or {}).items()},
        )


def now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def slug(name: str) -> str:
    s = re.sub(r"[^A-Za-z0-9]+", "-", name.strip()).strip("-").lower()
    return s or "voice"


class VoiceStore:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)

    def dir(self, voice_id: str) -> Path:
        return self.root / voice_id

    def list(self) -> list[Voice]:
        out = [builtin_voice()]
        for d in sorted(p for p in self.root.iterdir() if p.is_dir()):
            v = self.load(d.name)
            if v:
                out.append(v)
        return out

    def load(self, voice_id: str) -> Voice | None:
        if voice_id == BUILTIN_VOICE_ID:
            return builtin_voice()
        p = self.dir(voice_id) / "voice.json"
        try:
            return Voice.from_dict(json.loads(p.read_text(encoding="utf-8")))
        except (OSError, ValueError, KeyError):
            return None

    def save(self, voice: Voice) -> Voice:
        d = self.dir(voice.id)
        d.mkdir(parents=True, exist_ok=True)
        tmp = d / "voice.json.tmp"
        tmp.write_text(json.dumps(voice.to_dict(), indent=2), encoding="utf-8")
        tmp.replace(d / "voice.json")
        return voice

    def create(self, name: str, source: str = "recorded") -> Voice:
        base = slug(name)
        voice_id = base
        n = 2
        while self.dir(voice_id).exists():
            voice_id = f"{base}-{n}"
            n += 1
        v = Voice(id=voice_id, name=name.strip() or voice_id, created_at=now_iso(), source=source)
        return self.save(v)

    def delete(self, voice_id: str) -> bool:
        import shutil

        d = self.dir(voice_id)
        if voice_id == BUILTIN_VOICE_ID or not d.is_dir():
            return False
        shutil.rmtree(d)
        return True

    def attest(self, voice_id: str, text: str) -> Voice | None:
        v = self.load(voice_id)
        if v is None or voice_id == BUILTIN_VOICE_ID:
            return v
        v.attestation = Attestation(text=text.strip(), at=now_iso())
        return self.save(v)

    # ----------------------------------------------------------- references

    def reference_for(self, voice: Voice, tone: str) -> Path | None:
        """The tone's reference clip, falling back to calm, then to any tone."""
        if voice.id == BUILTIN_VOICE_ID:
            return None
        for candidate in (tone, "calm", *TONES):
            name = voice.tones.get(candidate)
            if name and (self.dir(voice.id) / name).is_file():
                return self.dir(voice.id) / name
        return None

    def add_take(self, voice_id: str, tone: str, line_id: str, audio: np.ndarray, sr: int) -> Path:
        """Store a cleaned take and rebuild the tone's reference clip."""
        v = self.load(voice_id)
        if v is None:
            raise KeyError(voice_id)
        takes_dir = self.dir(voice_id) / "takes" / tone
        takes_dir.mkdir(parents=True, exist_ok=True)
        path = takes_dir / f"{slug(line_id)}.wav"
        A.write(path, A.resample(audio, sr, REF_SAMPLE_RATE), REF_SAMPLE_RATE)
        names = [n for n in v.takes.get(tone, []) if n != path.name]
        names.append(path.name)
        v.takes[tone] = names
        self.save(v)
        self.rebuild_reference(v, tone)
        return path

    def remove_take(self, voice_id: str, tone: str, line_id: str) -> None:
        v = self.load(voice_id)
        if v is None:
            return
        name = f"{slug(line_id)}.wav"
        path = self.dir(voice_id) / "takes" / tone / name
        try:
            path.unlink()
        except OSError:
            pass
        v.takes[tone] = [n for n in v.takes.get(tone, []) if n != name]
        self.save(v)
        self.rebuild_reference(v, tone)

    def rebuild_reference(self, voice: Voice, tone: str) -> Path | None:
        """Join the tone's takes into one clip, best-sounding first, up to the cap."""
        d = self.dir(voice.id)
        pieces: list[np.ndarray] = []
        total = 0.0
        gap = np.zeros(int(REF_SAMPLE_RATE * GAP_SECONDS), dtype=np.float32)
        for name in voice.takes.get(tone, []):
            p = d / "takes" / tone / name
            if not p.is_file():
                continue
            audio, sr = A.read(p)
            audio = A.resample(audio, sr, REF_SAMPLE_RATE)
            if total + len(audio) / REF_SAMPLE_RATE > MAX_REFERENCE_SECONDS and pieces:
                break
            pieces.append(A.fade(audio, REF_SAMPLE_RATE, 5))
            pieces.append(gap)
            total += len(audio) / REF_SAMPLE_RATE + GAP_SECONDS
        ref_name = f"ref_{tone}.wav"
        ref_path = d / ref_name
        if not pieces:
            try:
                ref_path.unlink()
            except OSError:
                pass
            voice.tones.pop(tone, None)
            self.save(voice)
            return None
        joined = np.concatenate(pieces[:-1]) if len(pieces) > 1 else pieces[0]
        A.write(ref_path, A.match_loudness(joined, REF_SAMPLE_RATE, -20.0), REF_SAMPLE_RATE)
        voice.tones[tone] = ref_name
        self.save(voice)
        return ref_path

    # ------------------------------------------------------------ dev only

    def create_from_folder(self, name: str, folder: Path) -> Voice:
        """Development: build a voice from WAVs in a folder. Never available in the product."""
        if os.environ.get("VOICELAB_DEV_IMPORT") != "1":
            raise PermissionError("importing audio files is not available; record with the microphone")
        v = self.create(name, source="imported-dev")
        wavs = sorted(folder.glob("*.wav"))
        if not wavs:
            raise FileNotFoundError(f"no .wav files in {folder}")
        for tone in TONES:
            for i, w in enumerate(wavs):
                audio, sr = A.read(w)
                audio = A.trim_silence(audio, sr)
                self.add_take(v.id, tone, f"import-{i}", audio, sr)
        v = self.load(v.id)
        assert v is not None
        return v


def builtin_voice() -> Voice:
    return Voice(
        id=BUILTIN_VOICE_ID,
        name="Built-in sample voice",
        created_at="",
        source="builtin",
        tones={},
        attestation=Attestation(text="Sample voice shipped with the engine; no one's real voice.", at=""),
    )
