"""Cleaning up a take.

The microphone cannot be tested here and neither can the voice activity
detector's opinion of what speech sounds like — it is a trained model. What
can be, and is, is everything this module decides *about* a take once the
speech has been located: the verdicts, the trimming, the noise measurement,
and the rule that a take which fails is not stored.

So the segments are handed in. One test at the end runs the real detector
over a real generated clip when the development folder has one.
"""

from pathlib import Path

import numpy as np
import pytest

from voicelab import audio as A
from voicelab.recorder import (
    MIN_TAKE_SECONDS,
    SCRIPT,
    analyse_take,
    describe_problems,
    script,
    speech_segments,
)

SR = 48000


def tone(seconds: float, amp: float = 0.25, sr: int = SR) -> np.ndarray:
    t = np.arange(int(sr * seconds)) / sr
    f0 = 120 + 8 * np.sin(2 * np.pi * 1.7 * t)
    phase = 2 * np.pi * np.cumsum(f0) / sr
    wave = sum(np.sin(phase * h) / h for h in (1, 2, 3, 4, 5))
    return (amp * wave).astype(np.float32)


def silence(seconds: float, sr: int = SR) -> np.ndarray:
    return np.zeros(int(sr * seconds), dtype=np.float32)


def test_the_script_covers_every_tone_and_reads_like_a_race_engineer():
    lines = script()
    assert len(lines) >= 18, "about twenty lines, as the brief asks"
    tones = {line["tone"] for line in lines}
    assert tones == {"calm", "urgent", "celebratory"}
    for t in tones:
        assert sum(1 for line in lines if line["tone"] == t) >= 5
    for line in SCRIPT:
        # Long enough to be a usable reference, short enough for one breath.
        assert 40 <= len(line.text) <= 200, line.id
    assert len({line.id for line in SCRIPT}) == len(SCRIPT), "ids are unique"


def test_a_clean_take_is_kept_and_trimmed():
    audio = np.concatenate([silence(0.8), tone(4.0), silence(0.9)])
    cleaned, analysis = analyse_take(audio, SR, segments=[(0.8, 4.8)])
    assert analysis.ok, analysis.problems
    assert analysis.speech_s >= MIN_TAKE_SECONDS
    # The 1.7 seconds of silence at the ends is gone.
    assert len(cleaned) / SR < len(audio) / SR - 1.0
    assert analysis.waveform, "the tab draws this"
    assert analysis.lufs is not None


def test_a_long_pause_in_the_middle_is_cut_out():
    audio = np.concatenate([tone(2.0), silence(2.5), tone(2.0)])
    cleaned, analysis = analyse_take(audio, SR, segments=[(0.0, 2.0), (4.5, 6.5)])
    assert len(analysis.segments) == 2
    assert analysis.speech_s == pytest.approx(4.0, abs=0.1)
    # Kept: the speech plus a short natural gap, not the two and a half
    # seconds of nothing.
    assert len(cleaned) / SR < len(audio) / SR - 1.5


def test_no_speech_clipping_too_short_and_too_quiet_are_each_refused():
    _, quiet = analyse_take(silence(3.0), SR, segments=[])
    assert "no_speech" in quiet.problems and not quiet.ok

    loud = np.clip(tone(3.0, amp=4.0), -1.0, 1.0)
    _, clipped = analyse_take(loud, SR, segments=[(0.0, 3.0)])
    assert "clipping" in clipped.problems and not clipped.ok

    _, short = analyse_take(tone(0.5), SR, segments=[(0.0, 0.5)])
    assert "too_short" in short.problems and not short.ok

    _, faint = analyse_take(tone(3.0, amp=0.005), SR, segments=[(0.0, 3.0)])
    assert "too_quiet" in faint.problems

    _, long = analyse_take(tone(30.0), SR, segments=[(0.0, 30.0)])
    assert "too_long" in long.problems


def test_background_noise_is_measured_and_reported():
    rng = np.random.default_rng(7)
    loud_room = (rng.standard_normal(int(SR * 5.0)) * 0.06).astype(np.float32)
    loud_room[int(SR * 1.0) : int(SR * 4.0)] += tone(3.0)
    _, noisy = analyse_take(loud_room, SR, segments=[(1.0, 4.0)])
    assert noisy.snr_db is not None and noisy.snr_db < 20
    assert "noisy" in noisy.problems
    # Reported, but not refused: a room with some noise in it is still a
    # usable reference, and somebody recording in a garage may have no
    # quieter option. Clipping and silence are refused; this is advice.
    assert noisy.ok

    quiet_room = (rng.standard_normal(int(SR * 5.0)) * 0.0005).astype(np.float32)
    quiet_room[int(SR * 1.0) : int(SR * 4.0)] += tone(3.0)
    _, clean = analyse_take(quiet_room, SR, segments=[(1.0, 4.0)])
    assert clean.snr_db is not None and clean.snr_db > 20
    assert "noisy" not in clean.problems


def test_a_take_that_fails_is_still_returned_so_it_can_be_shown():
    # The tab draws the waveform of a refused take so the person can see
    # what went wrong rather than being told it in the abstract.
    cleaned, analysis = analyse_take(np.clip(tone(3.0, amp=4.0), -1, 1), SR, segments=[(0.0, 3.0)])
    assert not analysis.ok
    assert len(cleaned) > 0
    assert analysis.peak_dbfs > -3


def test_every_problem_has_something_to_do_about_it():
    for problem in ["no_speech", "clipping", "too_short", "too_long", "noisy", "too_quiet"]:
        text = describe_problems([problem])
        assert text and text != problem, problem
    assert "mic" in describe_problems(["clipping"])


def test_the_real_detector_finds_speech_in_real_speech():
    """The one test that runs the voice activity detector.

    Uses a clip the Voice Lab generated earlier, if the development folder
    has one; there is nothing to assert about a detector with no speech to
    give it, so this skips rather than pretending.
    """
    packs = Path(__file__).resolve().parents[2] / ".voicelab-dev" / "packs"
    clips = sorted(packs.glob("*/out/**/*.wav"))[:1] if packs.is_dir() else []
    if not clips:
        pytest.skip("no generated clips in .voicelab-dev to test the detector with")
    audio, sr = A.read(clips[0])
    padded = np.concatenate([np.zeros(sr // 2, dtype=np.float32), audio, np.zeros(sr // 2, dtype=np.float32)])
    segments = speech_segments(padded, sr)
    assert segments, f"no speech found in {clips[0].name}"
    start, end = segments[0]
    assert end > start
    # The speech starts after the half second of silence that was prepended.
    assert start >= 0.2
