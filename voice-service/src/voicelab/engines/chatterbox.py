"""Chatterbox (Resemble AI, MIT) — the default engine — and Chatterbox Turbo.

Both come from the ``chatterbox-tts`` package. The original takes an
emotion knob (``exaggeration``) and a pacing knob (``cfg_weight``), which is
what the tone map drives; Turbo ignores both and is faster. Both apply
Resemble's inaudible Perth watermark to their output, which is kept: it
marks the audio as synthetic at no cost.

Conditionals for a reference clip are prepared once and reused for every
phrase in that tone, rather than re-embedding the reference per call.
"""

from __future__ import annotations

import logging
from pathlib import Path

import numpy as np

from .base import EngineInfo, SynthesisRequest, TTSEngine

log = logging.getLogger(__name__)

CHATTERBOX = EngineInfo(
    id="chatterbox",
    name="Chatterbox",
    vendor="Resemble AI",
    licence="MIT",
    commercial_ok=True,
    summary="Zero-shot voice cloning with an emotion control. The default.",
    downloads=(
        ("ResembleAI/chatterbox", "ve.safetensors"),
        ("ResembleAI/chatterbox", "t3_cfg.safetensors"),
        ("ResembleAI/chatterbox", "s3gen.safetensors"),
        ("ResembleAI/chatterbox", "tokenizer.json"),
        ("ResembleAI/chatterbox", "conds.pt"),
    ),
    sample_rate=24000,
)

CHATTERBOX_TURBO = EngineInfo(
    id="chatterbox-turbo",
    name="Chatterbox Turbo",
    vendor="Resemble AI",
    licence="MIT",
    commercial_ok=True,
    summary="The faster Chatterbox. No emotion control; the reference carries the tone.",
    downloads=(
        ("ResembleAI/chatterbox-turbo", "t3_turbo_v1.safetensors"),
        ("ResembleAI/chatterbox-turbo", "s3gen_meanflow.safetensors"),
        ("ResembleAI/chatterbox-turbo", "ve.safetensors"),
        ("ResembleAI/chatterbox-turbo", "conds.pt"),
    ),
    sample_rate=24000,
)


class ChatterboxEngine(TTSEngine):
    info = CHATTERBOX

    def __init__(self, device: str = "cuda") -> None:
        super().__init__(device)
        self._model = None
        self._prepared: tuple[str, float] | None = None

    def load(self) -> None:
        if self.loaded:
            return
        import torch
        from chatterbox.tts import ChatterboxTTS

        device = self.device if (self.device != "cuda" or torch.cuda.is_available()) else "cpu"
        self._model = ChatterboxTTS.from_pretrained(device=device)
        self.device = device
        self.loaded = True
        log.info("chatterbox loaded on %s", device)

    def _prepare(self, reference: Path | None, exaggeration: float) -> None:
        assert self._model is not None
        if reference is None:
            # The built-in voice shipped with the model (conds.pt).
            return
        key = (str(reference), round(exaggeration, 3))
        if self._prepared == key:
            return
        self._model.prepare_conditionals(str(reference), exaggeration=exaggeration)
        self._prepared = key

    def synthesize(self, request: SynthesisRequest) -> tuple[np.ndarray, int]:
        self.load()
        assert self._model is not None
        import torch

        tone = request.tone
        self._prepare(request.reference, tone.exaggeration)
        torch.manual_seed(request.seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(request.seed)
        with torch.inference_mode():
            wav = self._model.generate(
                request.text,
                exaggeration=tone.exaggeration,
                cfg_weight=tone.cfg_weight,
                temperature=tone.temperature,
            )
        audio = wav.squeeze(0).detach().cpu().numpy().astype(np.float32)
        return audio, int(self._model.sr)

    def unload(self) -> None:
        self._model = None
        self._prepared = None
        super().unload()
        try:
            import torch

            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        except Exception:
            pass

    def vram_estimate_gb(self) -> float:
        return 3.5


class ChatterboxTurboEngine(ChatterboxEngine):
    info = CHATTERBOX_TURBO

    def load(self) -> None:
        if self.loaded:
            return
        import torch
        from chatterbox.tts_turbo import ChatterboxTurboTTS

        device = self.device if (self.device != "cuda" or torch.cuda.is_available()) else "cpu"
        self._model = ChatterboxTurboTTS.from_pretrained(device=device)
        self.device = device
        self.loaded = True
        log.info("chatterbox turbo loaded on %s", device)

    def _prepare(self, reference: Path | None, exaggeration: float) -> None:
        assert self._model is not None
        if reference is None:
            return
        key = (str(reference), 0.0)
        if self._prepared == key:
            return
        self._model.prepare_conditionals(str(reference))
        self._prepared = key

    def synthesize(self, request: SynthesisRequest) -> tuple[np.ndarray, int]:
        self.load()
        assert self._model is not None
        import torch

        self._prepare(request.reference, 0.0)
        torch.manual_seed(request.seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(request.seed)
        with torch.inference_mode():
            wav = self._model.generate(request.text, temperature=request.tone.temperature)
        audio = wav.squeeze(0).detach().cpu().numpy().astype(np.float32)
        return audio, int(self._model.sr)

    def vram_estimate_gb(self) -> float:
        return 3.0
