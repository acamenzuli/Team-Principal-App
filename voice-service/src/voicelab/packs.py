"""Packs: a voice, an engine, a list of clips, and everything that happened making them.

```
packs/<id>/
  pack.json         who, what, settings, attestation, progress counts
  specs.json        the clip list, frozen when the pack was created
  results.jsonl     one line per attempt — engine, seed, attempt, WER, verdict
  review.json       marks and text overrides from the review screen
  out/              only clips that passed QC, in CrewChief's layout
  failed/           the last attempt of anything that never passed, for listening
  ABOUT_THIS_VOICE.txt
```
"""

from __future__ import annotations

import json
import re
import shutil
import threading
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path

from .phrases import PREVIEW_PRIORITY, BuildOptions, ClipSpec, build_clip_specs, summarise
from .voices import now_iso, slug

ABOUT_FILE = "ABOUT_THIS_VOICE.txt"


@dataclass
class PackSettings:
    voice_name: str  # the CrewChief folder name: alt/<voice_name>
    voice_id: str
    engine: str = "chatterbox"
    variants: int = 2
    your_name: str | None = None
    radio_effect: bool = False
    radio_amount: float = 1.0
    target_lufs: float | None = None  # measured from CrewChief's spotter clips at creation
    sample_rate: int = 22050
    subtype: str = "PCM_16"
    max_attempts: int = 4
    include_spotter: bool = True
    include_radio_check: bool = True
    include_personalisation: bool = True


@dataclass
class Pack:
    id: str
    created_at: str
    settings: PackSettings
    attestation: dict | None = None
    sounds_folder: str | None = None
    engine_version: str | None = None
    counts: dict = field(default_factory=dict)  # total, preview, passed, failed, ...
    installed_at: str | None = None
    exported: str | None = None

    def to_dict(self) -> dict:
        d = asdict(self)
        return d

    @staticmethod
    def from_dict(d: dict) -> "Pack":
        s = d.get("settings") or {}
        known = {k: v for k, v in s.items() if k in PackSettings.__dataclass_fields__}
        return Pack(
            id=d["id"],
            created_at=d.get("created_at", ""),
            settings=PackSettings(**known),
            attestation=d.get("attestation"),
            sounds_folder=d.get("sounds_folder"),
            engine_version=d.get("engine_version"),
            counts=dict(d.get("counts") or {}),
            installed_at=d.get("installed_at"),
            exported=d.get("exported"),
        )


@dataclass
class ClipResult:
    """One attempt at one clip. Appended to results.jsonl, never rewritten."""

    rel_path: str
    attempt: int
    seed: int
    engine: str
    tone: str
    passed: bool
    wer: float
    errors: int
    transcript: str
    reasons: list[str]
    duration_s: float
    confidence: float | None
    elapsed_s: float
    at: str
    text: str

    def to_dict(self) -> dict:
        return asdict(self)


class PackStore:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)
        self._locks: dict[str, threading.Lock] = {}
        self._locks_guard = threading.Lock()

    def dir(self, pack_id: str) -> Path:
        return self.root / pack_id

    def out_dir(self, pack_id: str) -> Path:
        return self.dir(pack_id) / "out"

    def failed_dir(self, pack_id: str) -> Path:
        return self.dir(pack_id) / "failed"

    def _lock(self, pack_id: str) -> threading.Lock:
        with self._locks_guard:
            return self._locks.setdefault(pack_id, threading.Lock())

    # ----------------------------------------------------------------- crud

    def list(self) -> list[Pack]:
        out = []
        for d in sorted(p for p in self.root.iterdir() if p.is_dir()):
            p = self.load(d.name)
            if p:
                out.append(p)
        out.sort(key=lambda p: p.created_at, reverse=True)
        return out

    def load(self, pack_id: str) -> Pack | None:
        try:
            return Pack.from_dict(json.loads((self.dir(pack_id) / "pack.json").read_text(encoding="utf-8")))
        except (OSError, ValueError, KeyError, TypeError):
            return None

    def save(self, pack: Pack) -> Pack:
        d = self.dir(pack.id)
        d.mkdir(parents=True, exist_ok=True)
        tmp = d / "pack.json.tmp"
        tmp.write_text(json.dumps(pack.to_dict(), indent=2), encoding="utf-8")
        tmp.replace(d / "pack.json")
        return pack

    def create(
        self,
        settings: PackSettings,
        sounds: Path,
        attestation: dict | None,
        overrides: dict[str, str] | None = None,
    ) -> tuple[Pack, list[ClipSpec]]:
        base = slug(settings.voice_name) + "-" + time.strftime("%Y%m%d-%H%M%S")
        pack_id = base
        n = 2
        while self.dir(pack_id).exists():
            pack_id = f"{base}-{n}"
            n += 1
        options = BuildOptions(
            voice_name=settings.voice_name,
            variants=settings.variants,
            your_name=settings.your_name,
            include_spotter=settings.include_spotter,
            include_radio_check=settings.include_radio_check,
            include_personalisation=settings.include_personalisation,
        )
        specs = build_clip_specs(sounds, options, overrides)
        pack = Pack(
            id=pack_id,
            created_at=now_iso(),
            settings=settings,
            attestation=attestation,
            sounds_folder=str(sounds),
            counts=summarise(specs).to_dict(),
        )
        self.save(pack)
        self.save_specs(pack_id, specs)
        (self.dir(pack_id) / "results.jsonl").touch()
        self.write_about(pack)
        return pack, specs

    def delete(self, pack_id: str) -> bool:
        d = self.dir(pack_id)
        if not d.is_dir():
            return False
        shutil.rmtree(d)
        return True

    # ---------------------------------------------------------------- specs

    def save_specs(self, pack_id: str, specs: list[ClipSpec]) -> None:
        data = [asdict(s) for s in specs]
        tmp = self.dir(pack_id) / "specs.json.tmp"
        tmp.write_text(json.dumps(data), encoding="utf-8")
        tmp.replace(self.dir(pack_id) / "specs.json")

    def load_specs(self, pack_id: str) -> list[ClipSpec]:
        try:
            data = json.loads((self.dir(pack_id) / "specs.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return []
        return [ClipSpec(**d) for d in data]

    # -------------------------------------------------------------- results

    def append_result(self, pack_id: str, result: ClipResult) -> None:
        with self._lock(pack_id):
            with (self.dir(pack_id) / "results.jsonl").open("a", encoding="utf-8") as f:
                f.write(json.dumps(result.to_dict()) + "\n")

    def load_results(self, pack_id: str) -> list[ClipResult]:
        out: list[ClipResult] = []
        p = self.dir(pack_id) / "results.jsonl"
        try:
            for line in p.read_text(encoding="utf-8").splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    d = json.loads(line)
                    known = {k: v for k, v in d.items() if k in ClipResult.__dataclass_fields__}
                    out.append(ClipResult(**known))
                except (ValueError, TypeError):
                    continue
        except OSError:
            pass
        return out

    def latest_results(self, pack_id: str) -> dict[str, ClipResult]:
        """The last attempt recorded for each clip."""
        latest: dict[str, ClipResult] = {}
        for r in self.load_results(pack_id):
            latest[r.rel_path] = r
        return latest

    def passed_set(self, pack_id: str) -> set[str]:
        """Clips whose *latest* attempt passed and whose file exists."""
        out = set()
        out_dir = self.out_dir(pack_id)
        for rel, r in self.latest_results(pack_id).items():
            if r.passed and (out_dir / rel).is_file():
                out.add(rel)
        return out

    # --------------------------------------------------------------- review

    def load_review(self, pack_id: str) -> dict[str, dict]:
        try:
            return json.loads((self.dir(pack_id) / "review.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}

    def save_review(self, pack_id: str, review: dict[str, dict]) -> None:
        with self._lock(pack_id):
            tmp = self.dir(pack_id) / "review.json.tmp"
            tmp.write_text(json.dumps(review, indent=2), encoding="utf-8")
            tmp.replace(self.dir(pack_id) / "review.json")

    def update_review(self, pack_id: str, rel_path: str, **fields) -> dict:
        review = self.load_review(pack_id)
        entry = review.get(rel_path, {})
        for k, v in fields.items():
            if v is None:
                entry.pop(k, None)
            else:
                entry[k] = v
        if entry:
            review[rel_path] = entry
        else:
            review.pop(rel_path, None)
        self.save_review(pack_id, review)
        return entry

    def set_text_override(self, pack_id: str, rel_path: str, text: str | None) -> None:
        """Change what the engine is asked to say for one clip. The subtitle stays."""
        specs = self.load_specs(pack_id)
        for s in specs:
            if s.rel_path == rel_path:
                s.text = text.strip() if text and text.strip() else s.subtitle
        self.save_specs(pack_id, specs)
        self.update_review(pack_id, rel_path, text_override=(text.strip() if text and text.strip() else None))

    # ------------------------------------------------------------- listing

    def clip_view(self, pack_id: str, specs: list[ClipSpec] | None = None) -> list[dict]:
        """Every clip with its latest verdict and review state, for the review screen."""
        specs = specs if specs is not None else self.load_specs(pack_id)
        latest = self.latest_results(pack_id)
        review = self.load_review(pack_id)
        out_dir = self.out_dir(pack_id)
        failed_dir = self.failed_dir(pack_id)
        counts = {r: 0 for r in ("passed", "failed", "pending")}
        rows = []
        for s in specs:
            r = latest.get(s.rel_path)
            has_out = (out_dir / s.rel_path).is_file()
            has_failed = (failed_dir / s.rel_path).is_file()
            if r is None:
                state = "pending"
            elif r.passed and has_out:
                state = "passed"
            else:
                state = "failed"
            counts[state] += 1
            rows.append(
                {
                    "rel_path": s.rel_path,
                    "intent": s.intent,
                    "role": s.role,
                    "text": s.text,
                    "subtitle": s.subtitle,
                    "derived": s.derived,
                    "variant": s.variant,
                    "preview": s.priority < PREVIEW_PRIORITY and s.variant == 0,
                    "state": state,
                    "attempts": r.attempt if r else 0,
                    "wer": r.wer if r else None,
                    "transcript": r.transcript if r else None,
                    "reasons": r.reasons if r else [],
                    "confidence": r.confidence if r else None,
                    "seed": r.seed if r else None,
                    "engine": r.engine if r else None,
                    "duration_s": r.duration_s if r else None,
                    "at": r.at if r else None,
                    "audio": ("out" if has_out else "failed" if has_failed else None),
                    "review": review.get(s.rel_path, {}),
                }
            )
        return rows

    # ------------------------------------------------------------ metadata

    def write_about(self, pack: Pack) -> None:
        engine = pack.settings.engine
        text = ABOUT_TEMPLATE.format(
            voice=pack.settings.voice_name,
            engine=engine_display_name(engine),
            created=pack.created_at,
            attested=(pack.attestation or {}).get("text", "(no attestation recorded)"),
            attested_at=(pack.attestation or {}).get("at", ""),
        )
        d = self.dir(pack.id)
        (d / ABOUT_FILE).write_text(text, encoding="utf-8")
        # Inside the alt folder too, so it travels with an installed pack.
        alt = self.out_dir(pack.id) / "alt" / pack.settings.voice_name
        alt.mkdir(parents=True, exist_ok=True)
        (alt / ABOUT_FILE).write_text(text, encoding="utf-8")

    def refresh_counts(self, pack: Pack) -> Pack:
        rows = self.clip_view(pack.id)
        pack.counts.update(
            {
                "total": len(rows),
                "preview": sum(1 for r in rows if r["preview"]),
                "passed": sum(1 for r in rows if r["state"] == "passed"),
                "failed": sum(1 for r in rows if r["state"] == "failed"),
                "pending": sum(1 for r in rows if r["state"] == "pending"),
                "marked": sum(1 for r in rows if r["review"].get("marked")),
            }
        )
        return self.save(pack)

    def export_zip(self, pack: Pack, dest: Path) -> Path:
        """The out/ tree as a zip, laid out to unzip over a sounds folder."""
        import zipfile

        dest.parent.mkdir(parents=True, exist_ok=True)
        out_dir = self.out_dir(pack.id)
        with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as z:
            for p in sorted(x for x in out_dir.rglob("*") if x.is_file()):
                z.write(p, p.relative_to(out_dir).as_posix())
            z.write(self.dir(pack.id) / ABOUT_FILE, ABOUT_FILE)
        pack.exported = str(dest)
        self.save(pack)
        return dest


def engine_display_name(engine_id: str) -> str:
    try:
        from .engines.registry import ENGINES

        return ENGINES[engine_id][0].name + " (" + ENGINES[engine_id][0].vendor + ")"
    except KeyError:
        return engine_id


def safe_voice_name(name: str) -> str:
    """A CrewChief folder name: letters, digits, spaces, a few safe marks."""
    s = re.sub(r"[^A-Za-z0-9 _\-]+", "", name).strip()
    s = re.sub(r"\s+", " ", s)
    return s[:40]


ABOUT_TEMPLATE = """This voice pack was generated with Team Principal's Voice Lab.

Voice name:   {voice}
Engine:       {engine}
Created:      {created}

The audio in this pack is AI-generated speech: a text-to-speech model was
given a few minutes of a person's own recorded voice as a reference and
asked to say each phrase. It is not a recording of that person saying
these words.

Consent:      {attested}
Recorded on:  {attested_at}

The phrase list was read from the user's own installation of CrewChief.
CrewChief is a separate project by Britton IT Ltd and its contributors.
This pack is not made, endorsed or supported by the CrewChief project.

Team Principal — https://github.com/acamenzuli/Team-Principal-App
"""
