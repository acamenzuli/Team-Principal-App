"""Model weights: what is needed, what is present, fetching the rest.

Every model comes from its official Hugging Face repository into the home's
``models/`` folder (``HF_HOME``). Downloads resume: the hub client keeps
``.incomplete`` files and continues them, so an interrupted 2 GB fetch does
not start over. Progress is read from disk rather than trusted from a
callback, which also makes it correct across restarts.
"""

from __future__ import annotations

import logging
import os
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

from .engines.registry import ENGINES, list_engines

log = logging.getLogger(__name__)

WHISPER_REPO = "mobiuslabsgmbh/faster-whisper-large-v3-turbo"
WHISPER_MODEL_NAME = "large-v3-turbo"
WHISPER_FILES = ("model.bin", "config.json", "tokenizer.json", "vocabulary.json", "preprocessor_config.json")


@dataclass
class ModelFile:
    repo: str
    filename: str
    present: bool = False
    bytes_total: int | None = None
    bytes_present: int = 0


@dataclass
class ModelStatus:
    id: str
    name: str
    licence: str
    purpose: str
    files: list[ModelFile] = field(default_factory=list)

    @property
    def present(self) -> bool:
        return all(f.present for f in self.files)

    @property
    def bytes_total(self) -> int | None:
        sizes = [f.bytes_total for f in self.files]
        return None if any(s is None for s in sizes) else int(sum(sizes))  # type: ignore[arg-type]

    @property
    def bytes_present(self) -> int:
        return sum(f.bytes_present for f in self.files)

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "name": self.name,
            "licence": self.licence,
            "purpose": self.purpose,
            "present": self.present,
            "bytes_total": self.bytes_total,
            "bytes_present": self.bytes_present,
            "files": [f.__dict__ for f in self.files],
        }


def required_models(engine_ids: list[str] | None = None) -> list[ModelStatus]:
    """The engines (default, or the ones named) plus the QC transcriber."""
    out: list[ModelStatus] = []
    ids = engine_ids or ["chatterbox"]
    for info in list_engines():
        if info.id not in ids:
            continue
        out.append(
            ModelStatus(
                id=info.id,
                name=info.name,
                licence=info.licence,
                purpose="speech",
                files=[ModelFile(repo=r, filename=f) for r, f in info.downloads],
            )
        )
    out.append(
        ModelStatus(
            id="whisper",
            name="Whisper large-v3-turbo (faster-whisper)",
            licence="MIT",
            purpose="quality control",
            files=[ModelFile(repo=WHISPER_REPO, filename=f) for f in WHISPER_FILES],
        )
    )
    return out


def _cache_dir() -> Path:
    from huggingface_hub.constants import HF_HUB_CACHE

    return Path(HF_HUB_CACHE)


def _repo_dir(repo: str) -> Path:
    return _cache_dir() / ("models--" + repo.replace("/", "--"))


def _present_path(repo: str, filename: str) -> Path | None:
    from huggingface_hub import try_to_load_from_cache

    result = try_to_load_from_cache(repo_id=repo, filename=filename)
    if isinstance(result, str) and os.path.isfile(result):
        return Path(result)
    return None


def _incomplete_bytes(repo: str) -> int:
    blobs = _repo_dir(repo) / "blobs"
    if not blobs.is_dir():
        return 0
    total = 0
    for p in blobs.glob("*.incomplete"):
        try:
            total += p.stat().st_size
        except OSError:
            pass
    return total


_size_cache: dict[str, dict[str, int]] = {}


def remote_sizes(repo: str) -> dict[str, int]:
    """File sizes from the hub, cached; empty when offline."""
    if repo in _size_cache:
        return _size_cache[repo]
    try:
        from huggingface_hub import HfApi

        info = HfApi().model_info(repo, files_metadata=True)
        sizes = {s.rfilename: int(s.size or 0) for s in info.siblings or []}
    except Exception as e:  # offline, rate-limited, gone
        log.info("could not read sizes for %s: %s", repo, e)
        sizes = {}
    _size_cache[repo] = sizes
    return sizes


def status(engine_ids: list[str] | None = None, with_sizes: bool = True) -> list[ModelStatus]:
    models = required_models(engine_ids)
    for m in models:
        repos = {f.repo for f in m.files}
        sizes = {r: (remote_sizes(r) if with_sizes else {}) for r in repos}
        partial = {r: _incomplete_bytes(r) for r in repos}
        for f in m.files:
            path = _present_path(f.repo, f.filename)
            f.present = path is not None
            f.bytes_total = sizes[f.repo].get(f.filename) or (path.stat().st_size if path else None)
            f.bytes_present = path.stat().st_size if path else 0
        # Attribute in-flight bytes to the first missing file so progress moves.
        for r, n in partial.items():
            for f in m.files:
                if f.repo == r and not f.present:
                    f.bytes_present += n
                    break
    return models


class Downloader:
    """One download at a time, in a thread, progress by re-reading the disk."""

    def __init__(self) -> None:
        self._thread: threading.Thread | None = None
        self.error: str | None = None
        self.current: str | None = None
        self.done = False
        self.cancelled = False

    @property
    def busy(self) -> bool:
        return self._thread is not None and self._thread.is_alive()

    def start(self, engine_ids: list[str] | None, on_progress: Callable[[dict], None]) -> None:
        if self.busy:
            return
        self.error = None
        self.done = False
        self.cancelled = False
        self._thread = threading.Thread(target=self._run, args=(engine_ids, on_progress), name="model-download", daemon=True)
        self._thread.start()

    def cancel(self) -> None:
        self.cancelled = True

    def _run(self, engine_ids: list[str] | None, on_progress: Callable[[dict], None]) -> None:
        from huggingface_hub import hf_hub_download

        try:
            models = required_models(engine_ids)
            for m in models:
                for f in m.files:
                    if self.cancelled:
                        return
                    if _present_path(f.repo, f.filename):
                        continue
                    self.current = f"{m.name}: {f.filename}"
                    stop = threading.Event()
                    reporter = threading.Thread(target=self._report, args=(stop, engine_ids, on_progress), daemon=True)
                    reporter.start()
                    try:
                        hf_hub_download(repo_id=f.repo, filename=f.filename)
                    finally:
                        stop.set()
                        reporter.join(timeout=2)
            self.done = True
            on_progress(self.snapshot(engine_ids))
        except Exception as e:
            self.error = f"{type(e).__name__}: {e}"
            log.warning("model download failed: %s", self.error)
            on_progress(self.snapshot(engine_ids))
        finally:
            self.current = None

    def _report(self, stop: threading.Event, engine_ids: list[str] | None, on_progress: Callable[[dict], None]) -> None:
        while not stop.wait(1.0):
            try:
                on_progress(self.snapshot(engine_ids))
            except Exception:
                pass

    def snapshot(self, engine_ids: list[str] | None = None) -> dict:
        models = status(engine_ids)
        return {
            "busy": self.busy,
            "current": self.current,
            "error": self.error,
            "done": self.done,
            "models": [m.to_dict() for m in models],
            "bytes_total": sum((m.bytes_total or 0) for m in models),
            "bytes_present": sum(m.bytes_present for m in models),
            "all_present": all(m.present for m in models),
            "at": time.time(),
        }
