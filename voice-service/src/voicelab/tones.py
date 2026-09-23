"""Which tone a phrase is said in.

A tone is a named set of engine parameters — how much emotion, how fast,
how much the engine is allowed to wander from the reference — plus which
reference recording it uses. Rules map intent paths to tones with shell
globs, first match wins, and the last rule is a catch-all.

The default map ships inside the package. On first use it is copied to
``<home>/tones.json`` so a power user can edit it; a broken edit falls back
to the default and says so rather than silently generating a flat pack.
"""

from __future__ import annotations

import fnmatch
import json
from dataclasses import dataclass, field
from pathlib import Path

TONES = ("calm", "urgent", "celebratory")


@dataclass
class ToneParams:
    """Engine-facing knobs. Engines ignore what they cannot use."""

    tone: str = "calm"
    # Chatterbox: 0 is flat, 0.5 neutral, 1+ theatrical.
    exaggeration: float = 0.4
    # Chatterbox: lower = faster, more expressive pacing; higher = closer to the reference.
    cfg_weight: float = 0.5
    temperature: float = 0.7
    # Post-hoc time stretch; 1.0 = none. Chatterbox has no rate control, and
    # a phase-vocoder stretch costs quality, so this stays at 1.0 by default
    # and cfg_weight carries the pace.
    speed: float = 1.0
    # Trim target after generation, seconds of silence kept at each end.
    pad_ms: int = 60

    def to_dict(self) -> dict:
        return self.__dict__.copy()


DEFAULT_TONE_MAP: dict = {
    "version": 1,
    # Measured, not guessed: on two-word spotter phrases exaggeration 0.7
    # with cfg 0.3 came back intelligible 15 times in 30; 0.5 with 0.5, 26
    # in 30. The knob buys expressiveness at the price of the words, so it
    # stays near neutral and the *reference recording* for each tone — the
    # urgent takes are recorded fast and sharp — carries the tone instead.
    "tones": {
        "calm": {"exaggeration": 0.4, "cfg_weight": 0.5, "temperature": 0.7, "speed": 1.0},
        "urgent": {"exaggeration": 0.5, "cfg_weight": 0.5, "temperature": 0.7, "speed": 1.0},
        "celebratory": {"exaggeration": 0.55, "cfg_weight": 0.5, "temperature": 0.75, "speed": 1.0},
    },
    "rules": [
        {
            "tone": "urgent",
            "match": [
                "spotter/*",
                "flags/*",
                "penalties/*",
                "damage_reporting/*",
                "incidents/*",
                "push_now/*",
                "frozen_order/*",
                "rejoining/*",
                "overtaking_aids/*",
                "engine_monitor/*",
                "tyre_monitor/*puncture*",
                "tyre_monitor/*hot*",
                "fuel/*fumes*",
                "fuel/*no_fuel*",
                "fuel/*critical*",
                "lap_counter/last_lap*",
                "lap_counter/*white_flag*",
                "race_time/*last*",
                "personalisation/come_on",
            ],
        },
        {
            "tone": "celebratory",
            "match": [
                "lap_counter/has_taken_the_win*",
                "lap_counter/*win*",
                "lap_counter/*podium*",
                "position/leading*",
                "position/good_start*",
                "position/*gained*",
                "lap_times/*fastest*",
                "lap_times/*personal_best*",
                "lap_times/*best_lap*",
                "lap_times/*good*",
                "timings/*pulling_away*",
                "pearls_of_wisdom/*",
                "personalisation/well_done",
            ],
        },
        {"tone": "calm", "match": ["*"]},
    ],
}


@dataclass
class ToneMap:
    tones: dict[str, ToneParams]
    rules: list[tuple[str, list[str]]]  # (tone, patterns)
    problem: str | None = None
    source: str = "default"
    raw: dict = field(default_factory=dict)

    def tone_for(self, intent: str) -> ToneParams:
        for tone, patterns in self.rules:
            for pattern in patterns:
                if fnmatch.fnmatchcase(intent, pattern) or fnmatch.fnmatchcase(intent.rsplit("/", 1)[-1], pattern):
                    return self.tones.get(tone, self.tones["calm"])
        return self.tones["calm"]

    def to_dict(self) -> dict:
        return {
            "tones": {k: v.to_dict() for k, v in self.tones.items()},
            "rules": [{"tone": t, "match": p} for t, p in self.rules],
            "problem": self.problem,
            "source": self.source,
        }


def parse_tone_map(raw: dict) -> ToneMap:
    tones: dict[str, ToneParams] = {}
    for name, params in (raw.get("tones") or {}).items():
        if not isinstance(params, dict):
            raise ValueError(f"tone {name!r} is not an object")
        tones[name] = ToneParams(
            tone=name,
            exaggeration=float(params.get("exaggeration", 0.4)),
            cfg_weight=float(params.get("cfg_weight", 0.5)),
            temperature=float(params.get("temperature", 0.7)),
            speed=float(params.get("speed", 1.0)),
            pad_ms=int(params.get("pad_ms", 60)),
        )
    if "calm" not in tones:
        raise ValueError("a 'calm' tone is required; it is the fallback for everything")
    rules: list[tuple[str, list[str]]] = []
    for rule in raw.get("rules") or []:
        tone = rule.get("tone")
        patterns = rule.get("match")
        if tone not in tones:
            raise ValueError(f"rule refers to unknown tone {tone!r}")
        if not isinstance(patterns, list) or not all(isinstance(p, str) for p in patterns):
            raise ValueError(f"rule for {tone!r} needs a list of patterns")
        rules.append((tone, patterns))
    return ToneMap(tones=tones, rules=rules, raw=raw)


def load_tone_map(path: Path) -> ToneMap:
    """The user's map if it parses, else the default with the problem attached."""
    if not path.exists():
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(DEFAULT_TONE_MAP, indent=2), encoding="utf-8")
        except OSError:
            pass
        m = parse_tone_map(DEFAULT_TONE_MAP)
        m.source = "default"
        return m
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
        m = parse_tone_map(raw)
        m.source = str(path)
        return m
    except (OSError, ValueError) as e:
        m = parse_tone_map(DEFAULT_TONE_MAP)
        m.problem = f"{path.name} could not be used ({e}); the built-in map is in use"
        m.source = "default"
        return m


def save_tone_map(path: Path, raw: dict) -> ToneMap:
    """Validate, then write. A map that does not parse is never written."""
    m = parse_tone_map(raw)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(raw, indent=2), encoding="utf-8")
    m.source = str(path)
    return m
