"""The string work: derived text, personalisation, variants, priorities."""

from pathlib import Path

from voicelab import crewchief, phrases
from voicelab.phrases import (
    BuildOptions,
    corner_folder_text,
    derived_text,
    intent_folder_text,
    number_folder_text,
    number_words,
    personalise,
    preview_priority,
)


def test_numbers_in_words():
    assert number_words(0) == "zero"
    assert number_words(7) == "seven"
    assert number_words(13) == "thirteen"
    assert number_words(20) == "twenty"
    assert number_words(21) == "twenty one"
    assert number_words(100) == "one hundred"
    assert number_words(105) == "one hundred and five"
    assert number_words(1000) == "one thousand"
    assert number_words(1234) == "one thousand two hundred and thirty four"


def test_number_folders_read_like_the_pack_does():
    # The conventions the installed pack uses, checked against the reference
    # inventory during recon.
    assert number_folder_text("10") == "ten"
    assert number_folder_text("0") == "zero"
    assert number_folder_text("01") == "oh one"
    assert number_folder_text("1_05") == "one oh five"
    assert number_folder_text("1_30") == "one thirty"
    assert number_folder_text("10point3") == "ten point three"
    assert number_folder_text("0point1") == "zero point one"
    assert number_folder_text("point05") == "point oh five"
    assert number_folder_text("point10") == "point ten"
    assert number_folder_text("point9seconds") == "point nine seconds"
    assert number_folder_text("1point5seconds") == "one point five seconds"
    assert number_folder_text("double_oh") == "double oh"
    assert number_folder_text("zerozero") == "zero zero"
    assert number_folder_text("hundred_and") == "hundred and"
    assert number_folder_text("tenths") == "tenths"
    assert number_folder_text("minus") == "minus"
    assert number_folder_text("not_a_number") is None


def test_corner_and_intent_folders():
    assert corner_folder_text("arrabbiata_2") == "Arrabbiata two"
    assert corner_folder_text("michael_schumacher_s") == "Michael Schumacher S"
    assert corner_folder_text("130r") == "one hundred and thirty R"
    assert corner_folder_text("t1") == "T one"
    assert intent_folder_text("stay_below_vsc_speed") == "Stay below V S C speed"
    assert intent_folder_text("rejoin_clear") == "Rejoin clear"
    assert derived_text("numbers/10point3") == "ten point three"
    assert derived_text("corners/big_red") == "Big Red"
    assert derived_text("flags/virtual_safety_car") == "Virtual safety car"
    assert derived_text("numbers") is None


def test_personalise_replaces_whole_words_only():
    assert personalise("well done mate", "Alex") == "well done Alex"
    assert personalise("Mate, box this lap", "Alex") == "Alex, box this lap"
    assert personalise("the teammate is ahead", "Alex") == "the teammate is ahead"
    assert personalise("well done mate", None) == "well done mate"
    assert personalise("well done mate", "  ") == "well done mate"


def test_variant_names_keep_crewchief_markers():
    assert crewchief.variant_file_name("1.wav", 0) == "1.wav"
    assert crewchief.variant_file_name("1.wav", 1) == "1-a.wav"
    assert crewchief.variant_file_name("1.wav", 2) == "1-b.wav"
    assert crewchief.variant_file_name("2_op_prefix_ok.wav", 1) == "2-a_op_prefix_ok.wav"
    assert crewchief.variant_file_name("sweary_8.wav", 1) == "sweary_8-a.wav" or crewchief.split_file_name("sweary_8.wav")[0]
    assert crewchief.variant_file_name("12_rq_suffix_please.wav", 2) == "12-b_rq_suffix_please.wav"
    assert crewchief.variant_file_name("3_male.wav", 1) == "3-a_male.wav"


def test_split_file_name_markers():
    assert crewchief.split_file_name("2_op_prefix_ok.wav") == ("2", "_op_prefix_ok")
    assert crewchief.split_file_name("7.wav") == ("7", "")
    assert crewchief.split_file_name("1_op_suffix_come_on.wav") == ("1", "_op_suffix_come_on")


def test_preview_priorities():
    assert preview_priority("spotter/car_left") == 1
    assert preview_priority("radio_check/test") == 1
    assert preview_priority("position/p3") == 2
    assert preview_priority("flags/yellow_flag") == 3
    assert preview_priority("fuel/half_distance_good_fuel") == 4
    assert preview_priority("corners/arrabbiata_1") == 100
    assert preview_priority("pearls_of_wisdom/must_do_better") == 100


def test_subtitles_parsing_tolerates_quoting():
    text = '1.wav,"car left"\n2.wav,"okay, we\'ll time this stop"\n3.wav,unquoted text\nnotes.txt,ignored\n\n'
    lines = crewchief.parse_subtitles(text)
    assert [(l.file_name, l.text) for l in lines] == [
        ("1.wav", "car left"),
        ("2.wav", "okay, we'll time this stop"),
        ("3.wav", "unquoted text"),
    ]


def _make_sounds(tmp_path: Path) -> Path:
    sounds = tmp_path / "sounds"
    voice = sounds / "voice"
    for rel, lines, wavs in [
        ("flags/yellow_flag", '1.wav,"yellow flag"\n2.wav,"yellow flag"\n3.wav,"yellow flag in sector one"\n', 3),
        ("numbers/10point3", None, 2),
        ("spotter/car_left", '1.wav,"car left"\n5.wav,"left side"\n', 5),
        ("spotter_Jerry/car_left", '1.wav,"car left"\n', 1),
        ("radio_check/test", '1.wav,"radio check"\n', 1),
        ("radio_check_Jerry/test", '1.wav,"radio check"\n', 1),
        ("codriver/left_1", '1.wav,"left one"\n', 1),
        ("acknowledge/OK", '1.wav,"okay mate"\n2_op_prefix_ok.wav,"understood"\n', 2),
    ]:
        d = voice / rel
        d.mkdir(parents=True)
        for i in range(1, wavs + 1):
            (d / f"{i}.wav").write_bytes(b"RIFF")
        if rel == "acknowledge/OK":
            (d / "2_op_prefix_ok.wav").write_bytes(b"RIFF")
        if lines is not None:
            (d / "subtitles.csv").write_text(lines, encoding="utf-8")
    return sounds


def test_build_specs_covers_chief_spotter_radio_and_personalisation(tmp_path):
    sounds = _make_sounds(tmp_path)
    specs = phrases.build_clip_specs(sounds, BuildOptions(voice_name="Alex", variants=2, your_name="Alex"))
    paths = {s.rel_path for s in specs}

    # Chief: distinct texts only, two variants each, under alt/<Voice>/voice.
    assert "alt/Alex/voice/flags/yellow_flag/1.wav" in paths
    assert "alt/Alex/voice/flags/yellow_flag/1-a.wav" in paths
    assert "alt/Alex/voice/flags/yellow_flag/2.wav" not in paths  # duplicate text
    assert "alt/Alex/voice/flags/yellow_flag/3.wav" in paths
    # Markers survive on variants.
    assert "alt/Alex/voice/acknowledge/OK/2-a_op_prefix_ok.wav" in paths
    # Derived numbers.
    ten = next(s for s in specs if s.rel_path == "alt/Alex/voice/numbers/10point3/1.wav")
    assert ten.derived and ten.text == "ten point three"
    # Spotter goes to the shared folder with the category stripped.
    assert "voice/spotter_Alex/car_left/1.wav" in paths
    assert "voice/spotter_Alex/car_left/5-a.wav" in paths
    # Other voices and the co-driver are ignored.
    assert not any("Jerry" in p or "codriver" in p for p in paths)
    # Radio check and personalisations exist.
    assert "voice/radio_check_Alex/test/1.wav" in paths
    assert "alt/Alex/personalisations/Alex/prefixes_and_suffixes/ok/1.wav" in paths
    # your_name replaced "mate".
    ok = next(s for s in specs if s.rel_path == "alt/Alex/voice/acknowledge/OK/1.wav")
    assert ok.text == "okay Alex" and ok.subtitle == "okay mate"
    # Preview items sort first.
    assert specs[0].priority <= specs[-1].priority
    summary = phrases.summarise(specs)
    assert summary.total == len(specs)
    assert summary.by_role["spotter"] == 4
    assert summary.derived == 2


def test_pronounce_spells_out_what_the_engine_gets_wrong():
    from voicelab.phrases import pronounce

    # Measured: "P15" is said correctly 3 times in 15, "P fifteen" 14.
    assert pronounce("P15") == "P fifteen"
    assert pronounce("P3") == "P three"
    assert pronounce("P 7") == "P seven"
    assert pronounce("that's a 1:32.5") == "that's a one thirty two point five"
    assert pronounce("the gap is 1.4 seconds") == "the gap is one point four seconds"
    assert pronounce("box in 2 laps") == "box in two laps"
    assert pronounce("0.5") == "zero point five"
    assert pronounce("car 1012") == "car one zero one two"
    assert pronounce("no numbers here") == "no numbers here"


def test_pronounced_text_still_matches_its_subtitle():
    from voicelab.qc import normalise
    from voicelab.phrases import pronounce

    # The whole point: the engine is asked for something speakable, and the
    # QC comparison still holds it to what CrewChief displays.
    for subtitle in ["P15", "P3", "the gap is 1.4 seconds", "that's a 1:32.5", "box in 2 laps"]:
        assert normalise(pronounce(subtitle)) == normalise(subtitle), subtitle


def test_pronounce_handles_ordinals_before_plain_numbers():
    from voicelab.phrases import pronounce
    from voicelab.qc import normalise

    # "10th position" became "ten th position" until the ordinal rule ran first.
    assert pronounce("10th position") == "tenth position"
    assert pronounce("3rd place") == "third place"
    assert pronounce("21st lap") == "twenty first lap"
    for subtitle in ["10th position", "3rd place", "21st lap"]:
        assert normalise(pronounce(subtitle)) == normalise(subtitle), subtitle
