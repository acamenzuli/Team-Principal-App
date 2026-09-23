"""The engine interface.

An engine turns text plus a reference recording plus a tone into audio.
That is the whole contract; the queue, the QC loop, the post-processing and
the installer never see a model. ``load()`` is separate from construction
so an engine can be listed — name, licence, whether it may ship — without
pulling weights into memory.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from ..tones import ToneParams


@dataclass(frozen=True)
class EngineInfo:
    id: str
    name: str
    vendor: str
    licence: str
    # Whether the licence permits use in a product that is sold. Engines
    # with False may exist for development only and are never listed by a
    # shipped build.
    commercial_ok: bool
    # A one-line description for the bake-off screen.
    summary: str
    # Model files the engine needs, as (repo, filename) pairs on the Hugging
    # Face hub, so presence and size can be reported before anything loads.
    downloads: tuple[tuple[str, str], ...] = ()
    sample_rate: int = 24000
    supports_cloning: bool = True

    def to_dict(self) -> dict:
        d = self.__dict__.copy()
        d["downloads"] = [list(x) for x in self.downloads]
        return d


@dataclass
class SynthesisRequest:
    text: str
    reference: Path | None  # the tone's reference clip; None = the engine's built-in voice
    tone: ToneParams = field(default_factory=ToneParams)
    seed: int = 0


class TTSEngine(ABC):
    info: EngineInfo

    def __init__(self, device: str = "cuda") -> None:
        self.device = device
        self.loaded = False

    @abstractmethod
    def load(self) -> None:
        """Bring the weights into memory. Idempotent."""

    @abstractmethod
    def synthesize(self, request: SynthesisRequest) -> tuple[np.ndarray, int]:
        """Mono float32 audio and its sample rate."""

    def unload(self) -> None:
        self.loaded = False

    def vram_estimate_gb(self) -> float:
        """Roughly what one loaded instance needs, for sizing the worker pool."""
        return 3.5
