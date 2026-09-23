"""From CrewChief's folders to a list of clips to make.

The text comes from the user's own ``subtitles.csv`` files at run time.
Where a folder has none — every ``numbers`` folder, some ``corners``, a few
dozen odd intents — the text is *derived* from the folder name and the
entry says so, so the review screen and the QC loop know to lean on the
transcript rather than on the text.

Nothing in this module touches audio or a model. It is plain string work
and is tested as such.
"""

from __future__ import annotations

import fnmatch
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

from . import crewchief
from .crewchief import IntentFolder, variant_file_name

# ---------------------------------------------------------------- work items


@dataclass
class ClipSpec:
    """One clip to generate."""

    rel_path: str  # output path relative to the pack's out/ folder, e.g. alt/Alex/voice/flags/yellow_flag/1.wav
    text: str  # what the engine is asked to say (after your_name substitution)
    subtitle: str  # what the source subtitle said
    intent: str  # e.g. "flags/yellow_flag" — the key the tone map matches on
    role: str  # "chief" | "spotter" | "radio_check" | "personalisation"
    derived: bool = False  # text came from a folder name, not a subtitle
    variant: int = 0
    priority: int = 100  # lower first; the preview set is everything below PREVIEW_PRIORITY

    @property
    def id(self) -> str:
        return self.rel_path


PREVIEW_PRIORITY = 10

# The intents a driver hears in the first few minutes of a race, in rough
# order of how soon. Chosen by name from the installed pack so the preview
# is about 550 clips at one variant — ten minutes of generation on a 4090,
# not an evening. Shell globs, matched against "category/intent".
PREVIEW_PATTERNS: list[tuple[int, str]] = [
    (1, "radio_check/*"),
    (1, "spotter/*"),
    (2, "position/p[1-9]"),
    (2, "position/p1[0-9]"),
    (2, "position/p20"),
    (2, "position/leading"),
    (2, "position/last"),
    (2, "position/pole"),
    (2, "position/good_start"),
    (2, "position/ok_start"),
    (2, "position/bad_start"),
    (2, "lap_counter/get_ready"),
    (2, "lap_counter/green_green_green"),
    (2, "lap_counter/race_starts_in"),
    (2, "lap_counter/last_lap"),
    (2, "lap_counter/last_lap_leading"),
    (2, "lap_counter/two_to_go"),
    (2, "lap_counter/laps_make_them_count"),
    (2, "lap_counter/finished_race"),
    (2, "lap_counter/finished_race_good_finish"),
    (2, "lap_counter/has_taken_the_win"),
    (2, "lap_counter/won_race"),
    (2, "lap_counter/podium_finish"),
    (3, "flags/yellow_flag"),
    (3, "flags/yellow_flag_sector_*"),
    (3, "flags/green_flag_sector_*"),
    (3, "flags/double_yellow_flag"),
    (3, "flags/blue_flag"),
    (3, "flags/black_flag"),
    (3, "flags/white_flag"),
    (3, "flags/local_yellow_*"),
    (3, "flags/fc_yellow_start"),
    (3, "flags/fc_yellow_prepare_for_green"),
    (3, "flags/fc_yellow_green_flag"),
    (3, "flags/no_overtaking"),
    (3, "flags/clear_to_overtake"),
    (3, "flags/virtual_safety_car"),
    (3, "flags/virtual_safety_car_phase_over"),
    (3, "timings/gap_*"),
    (3, "timings/ahead_is_now"),
    (3, "timings/behind_is_now"),
    (3, "timings/seconds"),
    (3, "timings/the_gap_to"),
    (3, "timings/being_pressured"),
    (3, "timings/is_reeling_you_in"),
    (4, "fuel/half_distance_*"),
    (4, "fuel/fuel_should_be_ok"),
    (4, "fuel/fuel_will_be_tight"),
    (4, "fuel/plenty_of_fuel"),
    (4, "fuel/two_laps_fuel"),
    (4, "fuel/one_lap_fuel"),
    (4, "fuel/about_to_run_out"),
    (4, "fuel/laps_remaining"),
    (4, "fuel/we_estimate"),
    (4, "fuel/litres"),
    (4, "fuel/litres_remaining"),
    (4, "fuel/minutes_remaining"),
    (4, "fuel/ten_minutes_fuel"),
    (4, "fuel/five_minutes_fuel"),
    (4, "fuel/we_will_need_to_pit_for_fuel"),
    (4, "numbers/[0-9]"),
    (4, "numbers/[12][0-9]"),
    (4, "numbers/30"),
    (4, "numbers/point[1-9]"),
    (4, "numbers/[1-5]point[0-9]"),
    (4, "numbers/second"),
    (4, "numbers/seconds"),
    (4, "numbers/tenths"),
    (4, "numbers/minute"),
    (4, "numbers/minutes"),
    (4, "numbers/point"),
    (5, "lap_times/personal_best"),
    (5, "lap_times/best_lap_in_race"),
    (5, "lap_times/good_lap"),
    (5, "lap_times/consistent"),
    (5, "lap_times/improving"),
    (5, "lap_times/worsening"),
    (5, "lap_times/pace_*"),
    (5, "lap_times/time_intro"),
    (5, "lap_times/fastest_in_your_class"),
    (5, "lap_times/quickest_overall"),
    (5, "acknowledge/OK"),
    (5, "acknowledge/yes"),
    (5, "acknowledge/no"),
    (5, "acknowledge/stand_by"),
    (5, "acknowledge/didnt_understand"),
    (5, "acknowledge/radio_check"),
    (5, "acknowledge/no_data"),
    (6, "race_time/last_lap"),
    (6, "race_time/this_is_the_last_lap"),
    (6, "race_time/*_minutes_left"),
    (6, "race_time/one_minute_remaining"),
    (6, "race_time/half_way"),
    (6, "race_time/laps_remaining"),
    (6, "race_time/remaining"),
    (6, "penalties/cut_track_in_race"),
    (6, "penalties/cut_track_race_1"),
    (6, "penalties/lap_deleted"),
    (6, "penalties/possible_track_limits_warning"),
    (6, "penalties/new_penalty_drivethrough"),
    (6, "penalties/new_penalty_stopgo"),
    (6, "penalties/penalty_served"),
    (6, "penalties/you_have_a_penalty"),
    (6, "damage_reporting/are_you_ok_first_try"),
    (6, "damage_reporting/no_damage"),
    (6, "damage_reporting/minor_aero_damage"),
    (6, "damage_reporting/severe_aero_damage"),
    (6, "damage_reporting/*_puncture"),
    (7, "mandatory_pit_stops/box_now"),
    (7, "mandatory_pit_stops/box_in"),
    (7, "mandatory_pit_stops/pit_now"),
    (7, "mandatory_pit_stops/pit_this_lap"),
    (7, "mandatory_pit_stops/pit_window_open"),
    (7, "mandatory_pit_stops/pit_window_closed"),
    (7, "mandatory_pit_stops/pit_crew_ready"),
    (7, "mandatory_pit_stops/stop_complete_go"),
    (7, "mandatory_pit_stops/engage_limiter"),
    (7, "mandatory_pit_stops/disengage_limiter"),
    (7, "mandatory_pit_stops/watch_your_pit_speed"),
    (7, "tyre_monitor/good_tyre_temps"),
    (7, "tyre_monitor/hot_tyres_all_round"),
    (7, "tyre_monitor/cold_tyres_all_round"),
    (7, "tyre_monitor/worn_all_round"),
    (7, "tyre_monitor/knackered_all_round"),
    (7, "tyre_monitor/good_wear"),
]


@dataclass
class BuildOptions:
    voice_name: str
    variants: int = 2  # clips per distinct text
    your_name: str | None = None
    include_spotter: bool = True
    include_radio_check: bool = True
    include_personalisation: bool = True
    # Words CrewChief uses to address the driver, replaced by your_name.
    vocatives: tuple[str, ...] = ("mate",)


# -------------------------------------------------------------------- build


def build_clip_specs(sounds: Path, options: BuildOptions, overrides: dict[str, str] | None = None) -> list[ClipSpec]:
    """Every clip a full pack contains, preview items first."""
    overrides = overrides or {}
    specs: list[ClipSpec] = []

    alt_root = f"alt/{options.voice_name}/voice"
    for folder in crewchief.iter_chief_folders(sounds):
        specs.extend(_specs_for_folder(folder, alt_root, "chief", options, overrides))

    if options.include_spotter:
        spotter_root = f"voice/spotter_{options.voice_name}"
        for folder in crewchief.iter_spotter_folders(sounds):
            # Spotter intents are rel "spotter/car_left"; drop the category so the
            # output path is voice/spotter_<Name>/car_left/.
            specs.extend(
                _specs_for_folder(folder, spotter_root, "spotter", options, overrides, strip_category=True)
            )

    if options.include_radio_check:
        specs.extend(radio_check_specs(sounds, options, overrides))

    if options.include_personalisation and options.your_name:
        specs.extend(personalisation_specs(options))

    specs.sort(key=lambda s: (s.priority, s.rel_path))
    return specs


def _specs_for_folder(
    folder: IntentFolder,
    out_root: str,
    role: str,
    options: BuildOptions,
    overrides: dict[str, str],
    strip_category: bool = False,
) -> list[ClipSpec]:
    rel_out = folder.intent if strip_category else folder.rel
    priority = preview_priority(folder.rel)
    entries: list[tuple[str, str, bool]] = []  # (file_name, text, derived)

    if folder.has_subtitles:
        seen: set[str] = set()
        for line in folder.lines:
            key = line.text.strip().lower()
            if key in seen:
                # The same words under two file names: one recording, then
                # variants, rather than four takes of "P3".
                continue
            seen.add(key)
            entries.append((line.file_name, line.text.strip(), False))
    else:
        text = derived_text(folder.rel)
        if text:
            entries.append((folder.wav_names[0], text, True))

    out: list[ClipSpec] = []
    for file_name, subtitle, derived in entries:
        for v in range(max(1, options.variants)):
            name = variant_file_name(file_name, v)
            rel_path = f"{out_root}/{rel_out}/{name}"
            override = overrides.get(rel_path) or overrides.get(f"{folder.rel}")
            # An override is what a person typed to fix a pronunciation, so
            # it is sent as written; everything else goes through the
            # speakable rewrite.
            text = override or pronounce(subtitle)
            text = personalise(text, options.your_name, options.vocatives)
            out.append(
                ClipSpec(
                    rel_path=rel_path,
                    text=text,
                    subtitle=subtitle,
                    intent=folder.rel,
                    role=role,
                    derived=derived,
                    variant=v,
                    priority=priority,
                )
            )
    return out


def radio_check_specs(sounds: Path, options: BuildOptions, overrides: dict[str, str]) -> list[ClipSpec]:
    """``voice/radio_check_<Name>/test/``: what the chief says when you ask for a radio check.

    Taken from the default voice's ``voice/radio_check/test`` subtitles when
    present, else a small built-in set — CrewChief needs the folder to exist
    for the voice to pass its own radio check.
    """
    folder = crewchief.read_intent_folder(sounds / "voice", sounds / "voice" / "radio_check" / "test")
    texts: list[tuple[str, str]] = []
    if folder and folder.has_subtitles:
        seen: set[str] = set()
        for line in folder.lines:
            k = line.text.lower()
            if k not in seen:
                seen.add(k)
                texts.append((line.file_name, line.text))
    if not texts:
        texts = [
            ("1.wav", "Radio check, loud and clear."),
            ("2.wav", "Yeah, I can hear you."),
            ("3.wav", "Copy, reading you fine."),
        ]
    root = f"voice/radio_check_{options.voice_name}/test"
    out: list[ClipSpec] = []
    for file_name, text in texts:
        for v in range(max(1, options.variants)):
            rel_path = f"{root}/{variant_file_name(file_name, v)}"
            out.append(
                ClipSpec(
                    rel_path=rel_path,
                    text=overrides.get(rel_path) or personalise(pronounce(text), options.your_name, options.vocatives),
                    subtitle=text,
                    intent="radio_check/test",
                    role="radio_check",
                    variant=v,
                    priority=1,
                )
            )
    return out


# The clips that say the driver's name. CrewChief plays one before or after
# a phrase whose file name carries the matching stub (see crewchief.py).
PERSONALISATION_TEMPLATES: dict[str, list[str]] = {
    "ok": ["Okay {name}.", "{name}.", "Right {name}.", "Okay, {name}."],
    "please": ["please {name}", "{name}, please", "please, {name}", "{name}, if you can"],
    "come_on": ["Come on {name}!", "Come on, {name}!", "Let's go {name}!", "Push, {name}!"],
    "well_done": ["Well done {name}.", "Nice one {name}.", "Good job {name}.", "Great work, {name}."],
    "bad_luck": ["Bad luck {name}.", "Unlucky {name}.", "Ah, bad luck, {name}.", "Never mind {name}."],
    "oh_dear": ["Oh dear, {name}.", "Oh no, {name}.", "Ah, {name}.", "Hmm, {name}."],
}


def personalisation_specs(options: BuildOptions) -> list[ClipSpec]:
    """``alt/<Voice>/personalisations/<your_name>/prefixes_and_suffixes/<stub>/N.wav``."""
    name = (options.your_name or "").strip()
    if not name:
        return []
    root = f"alt/{options.voice_name}/personalisations/{name}/prefixes_and_suffixes"
    out: list[ClipSpec] = []
    n = 1
    for stub in crewchief.PERSONALISATION_STUBS:
        for template in PERSONALISATION_TEMPLATES.get(stub, []):
            text = template.format(name=name)
            out.append(
                ClipSpec(
                    rel_path=f"{root}/{stub}/{n}.wav",
                    text=text,
                    subtitle=text,
                    intent=f"personalisation/{stub}",
                    role="personalisation",
                    priority=8,
                )
            )
            n += 1
    return out


# ---------------------------------------------------------------- priority


def preview_priority(intent_rel: str) -> int:
    for priority, pattern in PREVIEW_PATTERNS:
        if fnmatch.fnmatchcase(intent_rel, pattern):
            return priority
    return 100


def preview_only(specs: Iterable[ClipSpec]) -> list[ClipSpec]:
    """The preview set: the most-heard intents, one clip per text."""
    return [s for s in specs if s.priority < PREVIEW_PRIORITY and s.variant == 0]


# --------------------------------------------------------- pronunciation


_ORDINALS = {1: "first", 2: "second", 3: "third", 5: "fifth", 8: "eighth", 9: "ninth", 12: "twelfth"}


def _ordinal_words(n: int) -> str:
    """``3`` → "third". The QC normaliser has the same table, and must."""
    if n in _ORDINALS:
        return _ORDINALS[n]
    if n < 20:
        return number_words(n) + "th"
    if n % 10 == 0:
        return number_words(n)[:-1] + "ieth"
    tens, ones = divmod(n, 10)
    return number_words(tens * 10) + " " + _ordinal_words(ones)


def pronounce(text: str) -> str:
    """Rewrite a subtitle into something the engine reliably says.

    CrewChief's subtitles are written to be *read*: "P15", "1.4", "1:32.5".
    A TTS model given those digits guesses, and it guesses wrong often
    enough to matter — measured on Chatterbox, "P15" came back as "P5",
    "Juan 5" or "15" in twelve of fifteen attempts, while "P fifteen" was
    right in fourteen. A position call that says the wrong number is the
    worst thing this pack could do, so the digits are spelled out before
    they reach the engine.

    The subtitle is untouched: CrewChief still shows "P15", and the QC
    comparison normalises both sides to the same words, so a clip that says
    "P fifteen" still matches the subtitle it came from.
    """

    def p_number(m: re.Match) -> str:
        return "P " + number_words(int(m.group(1)))

    def decimal(m: re.Match) -> str:
        whole, frac = m.group(1), m.group(2)
        head = number_words(int(whole)) if whole else ""
        tail = " ".join("oh" if c == "0" else _ONES[int(c)] for c in frac)
        return f"{head} point {tail}".strip()

    def lap_time(m: re.Match) -> str:
        minutes, seconds = int(m.group(1)), m.group(2)
        secs = (
            " ".join("oh" if c == "0" else _ONES[int(c)] for c in seconds)
            if seconds.startswith("0")
            else number_words(int(seconds))
        )
        return f"{number_words(minutes)} {secs}"

    def integer(m: re.Match) -> str:
        s = m.group(0)
        if len(s) > 1 and s.startswith("0"):
            return " ".join("oh" if c == "0" else _ONES[int(c)] for c in s)
        n = int(s)
        # Four digits and up are said digit by digit rather than as a
        # number: a car number or a year, not "one thousand and twelve".
        return number_words(n) if n < 1000 else " ".join(_ONES[int(c)] for c in s)

    # Each replacement is padded, for the same reason the QC normaliser pads:
    # "1:32.5" is two rewrites that would otherwise be glued into
    # "twopoint five". The padding is collapsed at the end.
    t = text
    t = re.sub(r"\bP\.?\s?(\d+)\b", lambda m: " " + p_number(m) + " ", t)
    t = re.sub(r"\b(\d+):(\d\d)\b", lambda m: " " + lap_time(m) + " ", t)
    # Before the plain-integer rule, or "10th" becomes "ten th".
    t = re.sub(r"\b(\d+)(?:st|nd|rd|th)\b", lambda m: " " + _ordinal_words(int(m.group(1))) + " ", t)
    t = re.sub(r"(?<=\d),(?=\d{3}\b)", "", t)
    t = re.sub(r"(\d+)?\.(\d+)\b", lambda m: " " + decimal(m) + " ", t)
    t = re.sub(r"\d+", lambda m: " " + integer(m) + " ", t)
    t = re.sub(r"\s+", " ", t).strip()
    return re.sub(r"\s+([,.!?;:])", r"\1", t)


# ------------------------------------------------------------ your_name


def personalise(text: str, your_name: str | None, vocatives: tuple[str, ...] = ("mate",)) -> str:
    """Replace how CrewChief addresses the driver with their name."""
    name = (your_name or "").strip()
    if not name:
        return text
    for word in vocatives:
        text = re.sub(rf"\b{re.escape(word)}\b", name, text, flags=re.IGNORECASE)
    return text


# ------------------------------------------------------ derived text


_ONES = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
_TEENS = [
    "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
]
_TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]

_WORDS = {
    "double_oh": "double oh",
    "zerozero": "zero zero",
    "oh": "oh",
    "point": "point",
    "hundred": "hundred",
    "hundred_and": "hundred and",
    "thousand": "thousand",
    "thousand_and": "thousand and",
    "minus": "minus",
    "minute": "minute",
    "minutes": "minutes",
    "hour": "hour",
    "hours": "hours",
    "second": "second",
    "seconds": "seconds",
    "tenth": "tenth",
    "tenths": "tenths",
}


def number_words(n: int) -> str:
    """0..9999 in words, the way a race engineer says them."""
    if n < 0:
        return "minus " + number_words(-n)
    if n < 10:
        return _ONES[n]
    if n < 20:
        return _TEENS[n - 10]
    if n < 100:
        tens, ones = divmod(n, 10)
        return _TENS[tens] + ("" if ones == 0 else " " + _ONES[ones])
    if n < 1000:
        hundreds, rest = divmod(n, 100)
        return _ONES[hundreds] + " hundred" + ("" if rest == 0 else " and " + number_words(rest))
    thousands, rest = divmod(n, 1000)
    return number_words(thousands) + " thousand" + ("" if rest == 0 else " " + number_words(rest))


def digits_words(s: str) -> str:
    """``"05"`` → ``"oh five"``; each digit spoken, leading zero as "oh"."""
    return " ".join("oh" if c == "0" else _ONES[int(c)] for c in s)


def number_folder_text(name: str) -> str | None:
    """The spoken form of a ``voice/numbers/<name>`` folder.

    The conventions, read off the installed pack:
    ``10`` ten · ``01`` oh one · ``1_05`` one oh five (a lap time) ·
    ``1_30`` one thirty · ``10point3`` ten point three · ``point05`` point
    oh five · ``point9seconds`` point nine seconds ·
    ``1point5seconds`` one point five seconds · and the words above.
    """
    if name in _WORDS:
        return _WORDS[name]
    m = re.fullmatch(r"(\d+)_(\d+)", name)
    if m:
        minutes, seconds = m.group(1), m.group(2)
        secs = digits_words(seconds) if seconds.startswith("0") else number_words(int(seconds))
        return f"{number_words(int(minutes))} {secs}"
    m = re.fullmatch(r"(\d*)point(\d+)(seconds?)?", name)
    if m:
        whole, frac, unit = m.group(1), m.group(2), m.group(3)
        parts = []
        if whole:
            parts.append(number_words(int(whole)))
        parts.append("point")
        parts.append(digits_words(frac) if len(frac) > 1 and frac.startswith("0") else
                     (number_words(int(frac)) if len(frac) <= 2 else digits_words(frac)))
        if unit:
            parts.append(unit)
        return " ".join(parts)
    m = re.fullmatch(r"0(\d)", name)
    if m:
        return "oh " + _ONES[int(m.group(1))]
    if name.isdigit():
        return number_words(int(name))
    return None


def corner_folder_text(name: str) -> str:
    """``arrabbiata_2`` → ``Arrabbiata two``; ``130r`` → ``one thirty R``."""
    words = []
    for token in name.split("_"):
        if token.isdigit():
            words.append(number_words(int(token)))
        elif re.fullmatch(r"\d+[a-z]", token):
            words.append(number_words(int(token[:-1])) + " " + token[-1].upper())
        elif re.fullmatch(r"[a-z]\d+", token):
            words.append(token[0].upper() + " " + number_words(int(token[1:])))
        else:
            words.append(token.capitalize())
    return " ".join(words)


_ABBREVIATIONS = {
    "vsc": "V S C",
    "drs": "D R S",
    "kers": "K E R S",
    "fcy": "full course yellow",
    "sc": "safety car",
    "usa": "U S A",
    "ers": "E R S",
    "p2p": "push to pass",
}


def intent_folder_text(intent: str) -> str:
    """``stay_below_vsc_speed`` → ``stay below V S C speed``. A guess, flagged as one."""
    words = []
    for token in intent.split("_"):
        if token in _ABBREVIATIONS:
            words.append(_ABBREVIATIONS[token])
        elif token.isdigit():
            words.append(number_words(int(token)))
        else:
            words.append(token)
    text = " ".join(w for w in words if w)
    return text[:1].upper() + text[1:] if text else text


def derived_text(rel: str) -> str | None:
    """Text for an intent folder that has no subtitles, from its path."""
    category, _, rest = rel.partition("/")
    leaf = rest.rsplit("/", 1)[-1] if rest else ""
    if not leaf:
        return None
    if category == "numbers":
        return number_folder_text(leaf)
    if category == "corners":
        return corner_folder_text(leaf)
    return intent_folder_text(leaf)


# ------------------------------------------------------------------- summary


@dataclass
class InventorySummary:
    total: int
    preview: int
    derived: int
    by_role: dict[str, int] = field(default_factory=dict)

    def to_dict(self) -> dict:
        return self.__dict__


def summarise(specs: list[ClipSpec]) -> InventorySummary:
    by_role: dict[str, int] = {}
    for s in specs:
        by_role[s.role] = by_role.get(s.role, 0) + 1
    return InventorySummary(
        total=len(specs),
        preview=len(preview_only(specs)),
        derived=sum(1 for s in specs if s.derived),
        by_role=by_role,
    )
