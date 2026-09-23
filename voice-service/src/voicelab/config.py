"""Where things are.

Nothing here is hard-coded to a machine. The home folder comes from the app
(``--home`` or ``TP_VOICELAB_HOME``); every other path is derived from it.
The Hugging Face cache is pointed inside the home *before* any library that
reads ``HF_HOME`` is imported, which is why this module must stay free of
heavy imports.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path

ENV_HOME = "TP_VOICELAB_HOME"
ENV_TOKEN = "VOICELAB_TOKEN"


@dataclass(frozen=True)
class Paths:
    """The Voice Lab home and its subfolders."""

    home: Path

    @property
    def models(self) -> Path:
        return self.home / "models"

    @property
    def voices(self) -> Path:
        return self.home / "voices"

    @property
    def packs(self) -> Path:
        return self.home / "packs"

    @property
    def logs(self) -> Path:
        return self.home / "logs"

    @property
    def tones_file(self) -> Path:
        """The user-editable tone map. Copied from the packaged default on first use."""
        return self.home / "tones.json"

    @property
    def sandbox_sounds(self) -> Path:
        """A copy of a CrewChief sounds folder for development. Not used by the product."""
        return self.home / "sandbox" / "CrewChiefV4" / "sounds"

    def ensure(self) -> "Paths":
        for p in (self.home, self.models, self.voices, self.packs, self.logs):
            p.mkdir(parents=True, exist_ok=True)
        return self


def default_home() -> Path:
    r"""``TP_VOICELAB_HOME`` if set, else ``%LOCALAPPDATA%\Team Principal\voicelab``."""
    env = os.environ.get(ENV_HOME)
    if env:
        return Path(env)
    local = os.environ.get("LOCALAPPDATA")
    if local:
        return Path(local) / "Team Principal" / "voicelab"
    return Path.home() / ".team-principal" / "voicelab"


def configure_environment(paths: Paths) -> None:
    """Point every model cache inside the home. Call before importing torch/HF."""
    os.environ.setdefault("HF_HOME", str(paths.models))
    os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
    os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
    os.environ.setdefault("TORCH_HOME", str(paths.models / "torch"))
    # silero-vad's pip package ships its weights; nothing to point at.
    os.environ.setdefault("PYTHONWARNINGS", "ignore")
