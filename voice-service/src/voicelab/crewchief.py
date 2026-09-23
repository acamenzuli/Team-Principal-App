"""CrewChief's sounds folder: finding it, reading it, writing into it.

Everything known about CrewChief's layout is written down here and nowhere
else. It was read from the installed pack and from CrewChief's own source
(``Audio/Sounds.cs``, ``Audio/AudioPlayer.cs``, MIT):

* ``voice/<category>/<intent>/*.wav`` is the chief. An intent folder may
  carry ``subtitles.csv`` (``N.wav,"text"``).
* File names are matched by *substring*: ``op_prefix``/``op_suffix`` mark a
  clip that may take a personalisation before/after it, ``rq_prefix``/
  ``rq_suffix`` one that must, ``sweary`` a clip only played when swearing is
  on, and the stub name (``ok``, ``please``…) says which personalisation
  folder pairs with it. Numbers in file names mean nothing; any ``.wav`` in
  the folder is a candidate and CrewChief picks one at random.
* An alternative chief lives in ``alt/<Name>/`` and mirrors the whole tree —
  ``voice/``, ``personalisations/``, ``driver_names/``. CrewChief sets its
  sound path to that folder when ``chief_name`` is the folder's name.
* Spotter and radio check are *shared* folders that stay under the top-level
  ``voice/``: ``voice/spotter_<Name>/`` and ``voice/radio_check_<Name>/test``.
  Anything whose path contains ``\\spotter`` is treated as spotter audio.
* ``personalisations/<Name>/prefixes_and_suffixes/<stub>/*.wav`` are the
  clips that say the driver's name.

The install writes only into folders named after the voice. Nothing that
was there before is modified; anything in the way is backed up first and
listed in a manifest so an uninstall removes exactly what was written.
"""

from __future__ import annotations

import csv
import io
import json
import os
import shutil
import struct
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Iterator

DEFAULT_SOUNDS_SUBPATH = Path("CrewChiefV4") / "sounds"
USER_CONFIG_VENDOR = "Britton_IT_Ltd"
PERSONALISATION_STUBS = ("ok", "please", "come_on", "well_done", "bad_luck", "oh_dear")
_FILENAME_MARKERS = ("op_prefix", "op_suffix", "rq_prefix", "rq_suffix", "sweary", "bleep", "_male")


# ----------------------------------------------------------------- discovery


def candidate_sounds_folders(explicit: str | None = None) -> list[Path]:
    """Where the sounds folder might be, most authoritative first."""
    out: list[Path] = []
    if explicit:
        out.append(Path(explicit))
    override = read_user_config_override()
    if override:
        out.append(Path(override))
    local = os.environ.get("LOCALAPPDATA")
    if local:
        out.append(Path(local) / DEFAULT_SOUNDS_SUBPATH)
    return out


def find_sounds_folder(explicit: str | None = None) -> Path | None:
    for p in candidate_sounds_folders(explicit):
        if looks_like_sounds_folder(p):
            return p
    return None


def looks_like_sounds_folder(p: Path) -> bool:
    return (p / "voice").is_dir()


def is_shared_category(name: str) -> bool:
    """Folders under ``voice/`` that are not the chief's own phrases.

    ``spotter`` and ``radio_check`` are the default voice's shared folders,
    generated separately into ``spotter_<Name>`` and ``radio_check_<Name>``;
    the ``_<Name>`` variants belong to other voices; ``codriver*`` is the
    rally co-driver, out of scope.
    """
    return name in ("spotter", "radio_check") or name.startswith(("spotter_", "radio_check_", "codriver"))


def user_config_files() -> list[Path]:
    """CrewChief's per-version ``user.config`` files, newest version first."""
    local = os.environ.get("LOCALAPPDATA")
    if not local:
        return []
    vendor = Path(local) / USER_CONFIG_VENDOR
    if not vendor.is_dir():
        return []
    found: list[tuple[tuple[int, ...], Path]] = []
    for app_dir in vendor.iterdir():
        if not app_dir.is_dir():
            continue
        for version_dir in app_dir.iterdir():
            cfg = version_dir / "user.config"
            if cfg.is_file():
                found.append((_version_key(version_dir.name), cfg))
    found.sort(key=lambda t: t[0], reverse=True)
    return [p for _, p in found]


def _version_key(name: str) -> tuple[int, ...]:
    parts = []
    for piece in name.split("."):
        try:
            parts.append(int(piece))
        except ValueError:
            parts.append(-1)
    return tuple(parts)


def read_user_config_override() -> str | None:
    """The ``override_default_sound_pack_location`` setting, if set."""
    values = read_user_config_values(("override_default_sound_pack_location",))
    value = values.get("override_default_sound_pack_location")
    return value or None


def read_user_config_values(keys: Iterable[str]) -> dict[str, str]:
    """Read named settings from the newest ``user.config``."""
    wanted = set(keys)
    for cfg in user_config_files():
        try:
            text = cfg.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        return parse_user_config(text, wanted)
    return {}


def parse_user_config(text: str, wanted: set[str]) -> dict[str, str]:
    """Pull ``<setting name="k"><value>v</value></setting>`` pairs out of a .NET user.config."""
    import re

    out: dict[str, str] = {}
    pattern = re.compile(
        r'<setting\s+name="([^"]+)"[^>]*>\s*<value\s*/>|<setting\s+name="([^"]+)"[^>]*>\s*<value>(.*?)</value>',
        re.S,
    )
    for m in pattern.finditer(text):
        name = m.group(1) or m.group(2)
        if name in wanted:
            out[name] = "" if m.group(1) else _xml_unescape(m.group(3).strip())
    return out


def _xml_unescape(s: str) -> str:
    return (
        s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", '"')
        .replace("&apos;", "'")
        .replace("&amp;", "&")
    )


# --------------------------------------------------------------------- probe


@dataclass
class WavFormat:
    sample_rate: int
    channels: int
    bits: int

    @property
    def subtype(self) -> str:
        return {8: "PCM_U8", 16: "PCM_16", 24: "PCM_24", 32: "PCM_32"}.get(self.bits, "PCM_16")


@dataclass
class SoundsProbe:
    path: str
    pack_language: str | None
    pack_version: str | None
    wav_format: WavFormat | None
    format_uniform: bool
    chief_intents: int
    chief_lines: int
    chief_derived_folders: int
    alt_voices: list[str]
    spotter_voices: list[str]
    radio_check_voices: list[str]
    personalisations: list[str]
    spotter_intents: int

    def to_dict(self) -> dict:
        d = self.__dict__.copy()
        d["wav_format"] = self.wav_format.__dict__ if self.wav_format else None
        return d


def read_wav_format(path: Path) -> WavFormat | None:
    """The fmt chunk, read by hand: no decoder, no dependency, 44 bytes."""
    try:
        with path.open("rb") as f:
            head = f.read(12)
            if len(head) < 12 or head[:4] != b"RIFF" or head[8:12] != b"WAVE":
                return None
            while True:
                chunk = f.read(8)
                if len(chunk) < 8:
                    return None
                cid, size = chunk[:4], struct.unpack("<I", chunk[4:])[0]
                if cid == b"fmt ":
                    body = f.read(size)
                    if len(body) < 16:
                        return None
                    _tag, ch, sr, _br, _align, bits = struct.unpack("<HHIIHH", body[:16])
                    return WavFormat(sample_rate=sr, channels=ch, bits=bits)
                f.seek(size + (size & 1), io.SEEK_CUR)
    except OSError:
        return None


def probe(sounds: Path, sample_files: int = 40) -> SoundsProbe:
    voice = sounds / "voice"
    formats: list[WavFormat] = []
    for wav in _sample_wavs(voice, sample_files):
        fmt = read_wav_format(wav)
        if fmt:
            formats.append(fmt)
    uniform = len({(f.sample_rate, f.channels, f.bits) for f in formats}) <= 1
    fmt = formats[0] if formats else None

    inventory = list(iter_chief_folders(sounds))
    lines = sum(len(f.lines) for f in inventory)
    derived = sum(1 for f in inventory if not f.has_subtitles)

    alt = sorted(p.name for p in (sounds / "alt").iterdir() if p.is_dir()) if (sounds / "alt").is_dir() else []
    spotters = sorted(p.name[len("spotter_"):] for p in voice.glob("spotter_*") if p.is_dir())
    radios = sorted(p.name[len("radio_check_"):] for p in voice.glob("radio_check_*") if p.is_dir())
    pers_dir = sounds / "personalisations"
    personalisations = sorted(p.name for p in pers_dir.iterdir() if p.is_dir()) if pers_dir.is_dir() else []
    spotter_intents = sum(1 for p in (voice / "spotter").iterdir() if p.is_dir()) if (voice / "spotter").is_dir() else 0

    return SoundsProbe(
        path=str(sounds),
        pack_language=_read_first_line(sounds / "sound_pack_language.txt"),
        pack_version=_read_first_line(sounds / "sound_pack_version_info.txt"),
        wav_format=fmt,
        format_uniform=uniform,
        chief_intents=len(inventory),
        chief_lines=lines,
        chief_derived_folders=derived,
        alt_voices=alt,
        spotter_voices=spotters,
        radio_check_voices=radios,
        personalisations=personalisations,
        spotter_intents=spotter_intents,
    )


def _sample_wavs(voice: Path, n: int) -> Iterator[Path]:
    """A spread of files from different categories, not the first n in one folder."""
    count = 0
    if not voice.is_dir():
        return
    for category in sorted(p for p in voice.iterdir() if p.is_dir()):
        if is_shared_category(category.name):
            continue
        for intent in sorted(p for p in category.iterdir() if p.is_dir())[:2]:
            for wav in sorted(intent.glob("*.wav"))[:1]:
                yield wav
                count += 1
                if count >= n:
                    return


def _read_first_line(p: Path) -> str | None:
    try:
        return p.read_text(encoding="utf-8", errors="replace").splitlines()[0].strip() or None
    except (OSError, IndexError):
        return None


# ----------------------------------------------------------------- inventory


@dataclass
class SubtitleLine:
    file_name: str
    text: str


@dataclass
class IntentFolder:
    """One intent: a folder of wavs and what they say."""

    rel: str  # e.g. "flags/yellow_flag", relative to voice/
    path: Path
    lines: list[SubtitleLine] = field(default_factory=list)
    wav_names: list[str] = field(default_factory=list)
    has_subtitles: bool = True

    @property
    def category(self) -> str:
        return self.rel.split("/", 1)[0]

    @property
    def intent(self) -> str:
        return self.rel.split("/", 1)[1] if "/" in self.rel else self.rel


def parse_subtitles(text: str) -> list[SubtitleLine]:
    """``N.wav,"text"`` lines. Tolerates unquoted text and commas inside quotes."""
    out: list[SubtitleLine] = []
    for row in csv.reader(io.StringIO(text)):
        if not row:
            continue
        name = row[0].strip()
        if not name.lower().endswith(".wav"):
            continue
        phrase = ",".join(row[1:]).strip() if len(row) > 1 else ""
        phrase = phrase.strip().strip('"').strip()
        if phrase:
            out.append(SubtitleLine(file_name=name, text=phrase))
    return out


def read_intent_folder(voice: Path, folder: Path) -> IntentFolder | None:
    wavs = sorted(p.name for p in folder.glob("*.wav"))
    if not wavs:
        return None
    rel = folder.relative_to(voice).as_posix()
    subs = folder / "subtitles.csv"
    if subs.is_file():
        try:
            lines = parse_subtitles(subs.read_text(encoding="utf-8-sig", errors="replace"))
        except OSError:
            lines = []
        if lines:
            return IntentFolder(rel=rel, path=folder, lines=lines, wav_names=wavs, has_subtitles=True)
    return IntentFolder(rel=rel, path=folder, lines=[], wav_names=wavs, has_subtitles=False)


def iter_chief_folders(sounds: Path) -> Iterator[IntentFolder]:
    """Every intent folder of the chief voice, excluding other voices' shared folders."""
    voice = sounds / "voice"
    if not voice.is_dir():
        return
    for category in sorted(p for p in voice.iterdir() if p.is_dir()):
        if is_shared_category(category.name):
            continue
        yield from _iter_intents(voice, category)


def iter_spotter_folders(sounds: Path) -> Iterator[IntentFolder]:
    """The default spotter's intents (``voice/spotter/``)."""
    voice = sounds / "voice"
    spotter = voice / "spotter"
    if spotter.is_dir():
        yield from _iter_intents(voice, spotter)


def _iter_intents(voice: Path, category: Path) -> Iterator[IntentFolder]:
    # Some categories nest one level deeper (voice/corners/<track>/<corner>);
    # a folder that contains wavs is an intent wherever it sits.
    for root, dirs, files in os.walk(category):
        dirs.sort()
        if any(f.lower().endswith(".wav") for f in files):
            folder = read_intent_folder(voice, Path(root))
            if folder:
                yield folder


def split_file_name(name: str) -> tuple[str, str]:
    """``"2_op_prefix_ok.wav"`` → ``("2", "_op_prefix_ok")``: the base and the markers.

    The markers are what CrewChief matches by substring, so a generated
    variant keeps them verbatim and only changes the base.
    """
    stem = name[:-4] if name.lower().endswith(".wav") else name
    cut = len(stem)
    for marker in _FILENAME_MARKERS:
        i = stem.find(marker)
        if i >= 0:
            # Keep the underscore that introduces the marker with the marker.
            start = i - 1 if i > 0 and stem[i - 1] == "_" else i
            cut = min(cut, start)
    base, markers = stem[:cut], stem[cut:]
    if not base:
        base, markers = stem, ""
    return base, markers


def variant_file_name(original: str, variant: int) -> str:
    """Variant 0 keeps the original name; variant 1 is ``-a``, 2 is ``-b``…"""
    if variant <= 0:
        return original
    base, markers = split_file_name(original)
    letter = chr(ord("a") + variant - 1) if variant <= 26 else str(variant)
    return f"{base}-{letter}{markers}.wav"


# -------------------------------------------------------------------- install


@dataclass
class InstallPlan:
    """What an install would do, for the preview."""

    sounds: str
    voice_name: str
    targets: list[str]
    files: int
    bytes: int
    in_the_way: list[str]  # existing folders that would be backed up first
    previously_installed: bool


@dataclass
class InstallResult:
    manifest: str
    files_written: int
    bytes_written: int
    backup: str | None


def voice_targets(voice_name: str) -> list[str]:
    """The folders (relative to the sounds folder) a voice pack owns."""
    return [
        f"alt/{voice_name}",
        f"voice/spotter_{voice_name}",
        f"voice/radio_check_{voice_name}",
    ]


def plan_install(pack_out: Path, sounds: Path, voice_name: str) -> InstallPlan:
    files = 0
    size = 0
    for p in pack_out.rglob("*"):
        if p.is_file():
            files += 1
            size += p.stat().st_size
    targets = voice_targets(voice_name)
    in_the_way = [t for t in targets if (sounds / t).exists() and not _manifest_for(sounds, voice_name).exists()]
    return InstallPlan(
        sounds=str(sounds),
        voice_name=voice_name,
        targets=targets,
        files=files,
        bytes=size,
        in_the_way=in_the_way,
        previously_installed=_manifest_for(sounds, voice_name).exists(),
    )


def _manifest_for(sounds: Path, voice_name: str) -> Path:
    return sounds / "alt" / voice_name / ".team-principal-voicelab.json"


def install(pack_out: Path, sounds: Path, voice_name: str, backups: Path, pack_id: str) -> InstallResult:
    """Copy a pack's ``out/`` tree into the sounds folder.

    Only the voice's own folders are touched. If one of them exists and was
    not written by us, it is moved to ``backups/<stamp>/`` first and the
    manifest records that, so an uninstall can put it back. A previous
    install of the same voice is replaced file by file; files it wrote that
    the new pack lacks are removed, because a stale clip in a folder
    CrewChief picks from at random would be played.
    """
    targets = voice_targets(voice_name)
    manifest_path = _manifest_for(sounds, voice_name)
    previous = _read_manifest(manifest_path)

    backup_dir: Path | None = None
    if previous is None:
        for rel in targets:
            existing = sounds / rel
            if existing.exists():
                if backup_dir is None:
                    backup_dir = backups / time.strftime("%Y%m%dT%H%M%S")
                dest = backup_dir / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(str(existing), str(dest))

    written: list[str] = []
    total = 0
    for src in sorted(p for p in pack_out.rglob("*") if p.is_file()):
        rel = src.relative_to(pack_out).as_posix()
        if not any(rel == t or rel.startswith(t + "/") for t in targets):
            # A pack must not be able to write outside its own folders,
            # whatever ended up in out/.
            continue
        dst = sounds / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        written.append(rel)
        total += src.stat().st_size

    if previous is not None:
        stale = set(previous.get("files", [])) - set(written)
        for rel in stale:
            try:
                (sounds / rel).unlink()
            except OSError:
                pass
        _prune_empty_dirs(sounds, [Path(r).parent for r in stale])

    manifest = {
        "voice_name": voice_name,
        "pack_id": pack_id,
        "installed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "targets": targets,
        "files": written,
        "backup": str(backup_dir) if backup_dir else (previous or {}).get("backup"),
    }
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return InstallResult(
        manifest=str(manifest_path),
        files_written=len(written),
        bytes_written=total,
        backup=str(backup_dir) if backup_dir else None,
    )


def uninstall(sounds: Path, voice_name: str) -> dict:
    """Remove exactly what the manifest lists, then restore any backup."""
    manifest_path = _manifest_for(sounds, voice_name)
    manifest = _read_manifest(manifest_path)
    if manifest is None:
        return {"removed": 0, "restored": False, "reason": "nothing installed by Team Principal under that name"}
    removed = 0
    for rel in manifest.get("files", []):
        try:
            (sounds / rel).unlink()
            removed += 1
        except OSError:
            pass
    try:
        manifest_path.unlink()
    except OSError:
        pass
    for rel in manifest.get("targets", []):
        target = sounds / rel
        if target.is_dir():
            _prune_empty_dirs(sounds, [Path(p).parent for p in manifest.get("files", []) if p.startswith(rel)])
            try:
                if not any(target.rglob("*")):
                    shutil.rmtree(target)
            except OSError:
                pass
    restored = False
    backup = manifest.get("backup")
    if backup and Path(backup).is_dir():
        for rel in manifest.get("targets", []):
            src = Path(backup) / rel
            if src.exists() and not (sounds / rel).exists():
                shutil.move(str(src), str(sounds / rel))
                restored = True
    return {"removed": removed, "restored": restored}


def installed_packs(sounds: Path) -> list[dict]:
    out = []
    alt = sounds / "alt"
    if not alt.is_dir():
        return out
    for voice_dir in sorted(p for p in alt.iterdir() if p.is_dir()):
        m = _read_manifest(_manifest_for(sounds, voice_dir.name))
        if m:
            out.append(
                {
                    "voice_name": m.get("voice_name", voice_dir.name),
                    "pack_id": m.get("pack_id"),
                    "installed_at": m.get("installed_at"),
                    "files": len(m.get("files", [])),
                }
            )
    return out


def _read_manifest(p: Path) -> dict | None:
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def _prune_empty_dirs(root: Path, dirs: Iterable[Path]) -> None:
    for d in sorted(set(dirs), key=lambda p: len(p.parts), reverse=True):
        p = root / d
        while p != root and p.is_dir():
            try:
                p.rmdir()  # fails unless empty, which is the point
            except OSError:
                break
            p = p.parent
