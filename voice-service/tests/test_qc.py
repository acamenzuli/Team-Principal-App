"""Normalisation, WER and the audio checks — no models involved."""

import numpy as np

from voicelab import qc
from voicelab.qc import AudioChecks, check_audio, judge, normalise, word_error_rate


def test_numbers_and_positions_normalise_the_same_both_ways():
    assert normalise("You're P3, the gap behind is 1.4 seconds.") == normalise(
        "your p three the gap behind is one point four seconds"
    )
    assert normalise("P 3") == normalise("P3") == "p three"
    assert normalise("1:05.3") == normalise("one oh five point three")
    assert normalise("box in 2 laps") == normalise("box in two laps")
    assert normalise("that's a 1:30.5") == "that is a one thirty point five"
    assert normalise("3rd place") == "third place"
    assert normalise("0.5 seconds") == normalise("zero point five seconds")


def test_spelling_and_homophones_collapse():
    assert normalise("tire pressures") == normalise("tyre pressures")
    assert normalise("OK, pit lane is open") == normalise("okay pitlane is open")
    assert normalise("we're running on fumes, mate") == normalise("were running on fumes mate")
    assert normalise("inside's clear") == normalise("insides clear")
    assert normalise("the driver's door") == normalise("the drivers door")
    assert normalise("cut-track warnings") == "cut track warnings"
    assert normalise("Yeah, I can hear you") == normalise("yes i can hear you")


def test_wer_alignment():
    al = word_error_rate("car left", "car left")
    assert al.errors == 0 and al.wer == 0
    al = word_error_rate("car left", "car right")
    assert al.substitutions == 1 and al.wer == 0.5
    al = word_error_rate("box this lap", "box this lap now")
    assert al.insertions == 1
    al = word_error_rate("box this lap", "box lap")
    assert al.deletions == 1
    # Fillers in the transcript do not count.
    al = word_error_rate("box this lap", "uh, box this lap")
    assert al.errors == 0


def _tone(freq: float, seconds: float, sr: int = 24000, amp: float = 0.3) -> np.ndarray:
    t = np.arange(int(sr * seconds)) / sr
    return (amp * np.sin(2 * np.pi * freq * t)).astype(np.float32)


def test_audio_checks_flag_the_obvious():
    sr = 24000
    speech_like = _tone(220, 1.6, sr)  # ~ "car left" sized
    ok = check_audio(speech_like, sr, "yellow flag in sector one")
    assert "too_short" not in ok.problems and "clipping" not in ok.problems

    short = check_audio(_tone(220, 0.15, sr), sr, "yellow flag in sector one")
    assert "too_short" in short.problems

    long = check_audio(_tone(220, 12.0, sr), sr, "car left")
    assert "too_long" in long.problems

    clipped = check_audio(np.clip(_tone(220, 1.0, sr, amp=3.0), -1, 1), sr, "car left")
    assert "clipping" in clipped.problems

    silent = check_audio(np.zeros(sr, dtype=np.float32), sr, "car left")
    assert "silent" in silent.problems

    gap = np.concatenate([_tone(220, 0.5, sr), np.zeros(int(sr * 1.6), dtype=np.float32), _tone(220, 0.5, sr)])
    paused = check_audio(gap, sr, "car left, car left")
    assert "long_pause" in paused.problems

    hiss = (np.random.default_rng(1).standard_normal(sr) * 0.2).astype(np.float32)
    noisy = check_audio(hiss, sr, "car left")
    assert "spectral_noise" in noisy.problems


def _clean() -> AudioChecks:
    return AudioChecks(1.0, 1.0, -6.0, 0.0, 0.05, 0.05, 0.1, 0.05, [])


def test_judge_passes_exact_and_near_misses_only():
    assert judge("car left", "Car left.", _clean()).passed
    assert judge("you're P3", "Your P3.", _clean()).passed  # equivalence group
    # A short phrase has to come back word for word: "car lift" is not "car left".
    assert not judge("box this lap", "box these lap", _clean()).passed
    assert not judge("car left", "car lift", _clean()).passed
    assert judge("box now box now please", "box now box now blease", _clean()).passed
    # A long phrase tolerates a small rate.
    long = "okay we will use this stop as a benchmark for the rest of the race today"
    assert judge(long, long.replace("benchmark", "bench mark"), _clean()).passed
    assert not judge("car left", "clear", _clean()).passed
    v = judge("car left", "car left", AudioChecks(0.1, 1.0, -6.0, 0.0, 0, 0, 0, 0.05, ["too_short"]))
    assert not v.passed and v.reasons == ["too_short"]
    assert qc.summarise_reasons(["too_short", "transcript_mismatch"]) == "too short, said something else"


def test_phonetic_key_forgives_word_boundaries_but_not_wrong_sounds():
    from voicelab.qc import phonetic_key

    # Same sounds, cut differently: fine to play.
    assert phonetic_key("car low") == phonetic_key("Carlo")
    assert phonetic_key("clear right") == phonetic_key("Claire Wright")
    assert phonetic_key("inside's clear") == phonetic_key("Insights clear")
    # Different sounds: the errors QC exists to catch.
    assert phonetic_key("right side") != phonetic_key("light side")
    assert phonetic_key("car high") != phonetic_key("cog high")
    assert phonetic_key("got a car outside") != phonetic_key("and a car outside")
    # Vowels and fillers are ignored by design; the teacher-forced score is
    # what separates "car left" from "car lift".
    assert phonetic_key("car left") == phonetic_key("car lift")
    assert phonetic_key("are low") != phonetic_key("car low")

    from voicelab.qc import TargetScore

    # Same sounds alone are not enough; the teacher-forced score must agree.
    v = judge("car low", "Carlo.", _clean())
    assert not v.passed and not v.phonetic_match
    sure = TargetScore(mean=0.6, min=0.3, words=[("car", 0.6), ("low", 0.6)])
    v = judge("car low", "Carlo.", _clean(), target_score=sure)
    assert v.passed and v.phonetic_match
    unsure = TargetScore(mean=0.24, min=0.19, words=[("car", 0.29), ("low", 0.19)])
    v = judge("car low", "Carlo.", _clean(), target_score=unsure)
    assert not v.passed
    # Different sounds are never rescued, however sure the score.
    v = judge("right side", "Light side.", _clean(), target_score=sure)
    assert not v.passed and not v.phonetic_match


def test_spacing_and_apostrophes_are_not_content():
    from voicelab.qc import same_words_either_way

    # Twelve of sixteen failures in the first full preview run were this.
    assert same_words_either_way("we're half-way home", "We're halfway home.")
    assert same_words_either_way("standby", "Stand by.")
    assert same_words_either_way("that's half-way", "That's halfway.")
    # An apostrophe-s the speaker said as "is".
    assert same_words_either_way("your right rear's punctured", "Your right rear is punctured.")
    # A real possessive still matches, by the plain letter path.
    assert same_words_either_way("the driver's door", "the drivers door")
    # Different words are still different words.
    assert not same_words_either_way("half-way home, fuel looks good", "Halfway home, fuel looks bad")
    assert not same_words_either_way("car left", "car right")
    assert not same_words_either_way("box this lap", "box next lap")

    assert judge("we're half-way home, we're okay on fuel", "We're halfway home, we're okay on fuel.", _clean()).passed
    assert judge("standby", "Stand by.", _clean()).passed
    assert judge("yea", "Yeah.", _clean()).passed
    assert not judge("half-way home, fuel looks good", "Halfway home, fuel looks bad.", _clean()).passed


def test_the_phonetic_rescue_is_limited_to_phrases_it_was_calibrated_on():
    from voicelab.qc import TargetScore

    sure = TargetScore(mean=0.8, min=0.4, words=[("x", 0.8)])
    # Two words: the score separates there, so the rescue applies.
    assert judge("car low", "Carlo.", _clean(), target_score=sure).passed
    # Five words, same sounds, high error rate: measured, the score does
    # not separate correct from wrong at this length, so being sure is not
    # enough — this one goes back for another try.
    assert not judge("car low car low car", "Carlo Carlo car", _clean(), target_score=sure).passed
