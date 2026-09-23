"""The tone map: defaults, rules, and what a broken edit does."""

import json

from voicelab.tones import DEFAULT_TONE_MAP, load_tone_map, parse_tone_map, save_tone_map


def test_default_rules_route_the_obvious_intents():
    m = parse_tone_map(DEFAULT_TONE_MAP)
    assert m.tone_for("spotter/car_left").tone == "urgent"
    assert m.tone_for("flags/yellow_flag").tone == "urgent"
    assert m.tone_for("fuel/half_distance_good_fuel").tone == "calm"
    assert m.tone_for("timings/gap_in_front").tone == "calm"
    assert m.tone_for("lap_counter/has_taken_the_win").tone == "celebratory"
    assert m.tone_for("lap_times/personal_best").tone == "celebratory"
    assert m.tone_for("something/never_seen").tone == "calm"
    assert m.tone_for("urgent_only_by_leaf/last_lap").tone == "calm"  # leaf match needs the pattern


def test_the_knobs_stay_where_the_words_survive():
    # Measured on two-word spotter phrases: exaggeration above 0.5 or cfg
    # below 0.5 halves intelligibility. Tone comes from the reference clip.
    m = parse_tone_map(DEFAULT_TONE_MAP)
    for tone in m.tones.values():
        assert tone.exaggeration <= 0.55, tone
        assert tone.cfg_weight >= 0.5, tone
    assert m.tones["urgent"].exaggeration >= m.tones["calm"].exaggeration


def test_first_use_writes_the_default_and_a_bad_edit_falls_back(tmp_path):
    path = tmp_path / "tones.json"
    m = load_tone_map(path)
    assert path.exists() and m.problem is None and m.source == "default"

    path.write_text("{ not json", encoding="utf-8")
    m = load_tone_map(path)
    assert m.problem and "tones.json" in m.problem
    assert m.tone_for("spotter/car_left").tone == "urgent"  # default still works

    raw = json.loads(json.dumps(DEFAULT_TONE_MAP))
    raw["rules"].insert(0, {"tone": "celebratory", "match": ["fuel/*"]})
    saved = save_tone_map(path, raw)
    assert saved.tone_for("fuel/low").tone == "celebratory"
    assert load_tone_map(path).tone_for("fuel/low").tone == "celebratory"


def test_a_map_without_calm_is_refused(tmp_path):
    raw = {"tones": {"urgent": {}}, "rules": []}
    try:
        save_tone_map(tmp_path / "t.json", raw)
    except ValueError as e:
        assert "calm" in str(e)
    else:
        raise AssertionError("a map without a calm tone was accepted")
    assert not (tmp_path / "t.json").exists()
