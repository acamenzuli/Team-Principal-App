"""Which engines exist, and which may ship.

``VOICELAB_DEV_ENGINES=1`` in the environment reveals engines whose licence
is not commercial. The app never sets it; it exists so a licence-restricted
model can be compared during development without any path by which a
shipped build could load it.
"""

from __future__ import annotations

import os
from typing import Callable

from .base import EngineInfo, TTSEngine
from .chatterbox import CHATTERBOX, CHATTERBOX_TURBO, ChatterboxEngine, ChatterboxTurboEngine

DEFAULT_ENGINE = "chatterbox"

ENGINES: dict[str, tuple[EngineInfo, Callable[[str], TTSEngine]]] = {
    CHATTERBOX.id: (CHATTERBOX, ChatterboxEngine),
    CHATTERBOX_TURBO.id: (CHATTERBOX_TURBO, ChatterboxTurboEngine),
}


def dev_engines_enabled() -> bool:
    return os.environ.get("VOICELAB_DEV_ENGINES") == "1"


def list_engines() -> list[EngineInfo]:
    out = []
    for info, _ in ENGINES.values():
        if info.commercial_ok or dev_engines_enabled():
            out.append(info)
    return out


def create_engine(engine_id: str, device: str = "cuda") -> TTSEngine:
    entry = ENGINES.get(engine_id)
    if entry is None:
        raise KeyError(f"no engine called {engine_id!r}")
    info, factory = entry
    if not info.commercial_ok and not dev_engines_enabled():
        raise PermissionError(f"{info.name} is not licensed for a commercial product and is not available")
    return factory(device)
