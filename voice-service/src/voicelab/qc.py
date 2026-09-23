"""Quality control: did the engine say what it was asked to say?

Every clip is transcribed by a second model and the two texts are compared
after both have been normalised the same way — numbers as words, "P3" as
"p three", contractions expanded, punctuation gone — so a mismatch is a
mismatch in *words*, not in spelling. Duration, silence and spectral shape
are checked on the audio itself. A clip that fails is regenerated with a
new seed; one that keeps failing is listed for a human, never dropped in
silence and never kept in silence either.

The text half of this module has no heavy imports and is tested on any
machine. The transcriber loads faster-whisper lazily.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Iterable

import numpy as np

from .phrases import number_words

# ------------------------------------------------------------ normalisation

_CONTRACTIONS = {
    "i'm": "i am",
    "i'll": "i will",
    "i've": "i have",
    "i'd": "i would",
    "you'll": "you will",
    "you've": "you have",
    "you'd": "you would",
    "we'll": "we will",
    "we've": "we have",
    "we'd": "we would",
    "they'll": "they will",
    "they've": "they have",
    "he'll": "he will",
    "she'll": "she will",
    "it'll": "it will",
    "that'll": "that will",
    "there'll": "there will",
    "that's": "that is",
    "there's": "there is",
    "here's": "here is",
    "he's": "he is",
    "she's": "she is",
    "what's": "what is",
    "who's": "who is",
    "how's": "how is",
    "where's": "where is",
    "let's": "lets",
    "don't": "do not",
    "doesn't": "does not",
    "didn't": "did not",
    "can't": "cannot",
    "couldn't": "could not",
    "won't": "will not",
    "wouldn't": "would not",
    "shouldn't": "should not",
    "isn't": "is not",
    "aren't": "are not",
    "wasn't": "was not",
    "weren't": "were not",
    "haven't": "have not",
    "hasn't": "has not",
    "hadn't": "had not",
    "ain't": "is not",
    "c'mon": "come on",
    "gonna": "going to",
    "wanna": "want to",
    "gotta": "got to",
    "'em": "them",
    "em": "them",
    "'til": "until",
    "til": "until",
    "till": "until",
}

# Groups that sound alike, or that one side spells one way and the other
# side the other. Each group collapses to its first member on both sides.
_EQUIVALENTS: list[list[str]] = [
    ["your", "you are", "youre", "you're"],
    ["its", "it is", "it's"],
    ["their", "there", "they are", "theyre"],
    ["were", "we are", "where"],
    ["okay", "ok", "o k", "kay"],
    ["yeah", "yes", "yep", "ya", "yea"],
    ["tyre", "tire"],
    ["tyres", "tires"],
    ["kerb", "curb"],
    ["kerbs", "curbs"],
    ["metres", "meters"],
    ["metre", "meter"],
    ["litres", "liters"],
    ["litre", "liter"],
    ["colour", "color"],
    ["centre", "center"],
    ["practise", "practice"],
    ["programme", "program"],
    ["favour", "favor"],
    ["kph", "k p h", "kilometres per hour", "kilometers per hour"],
    ["mph", "m p h", "miles per hour"],
    ["pit lane", "pitlane"],
    ["pit stop", "pitstop"],
    ["pit stops", "pitstops"],
    ["in lap", "inlap"],
    ["out lap", "outlap"],
    ["p", "pee"],
    ["through", "thru"],
    ["to", "two", "too"],
    ["for", "four"],
    ["one", "won"],
    ["hour", "our"],
    ["hours", "ours"],
    ["whole", "hole"],
    ["mate", "mite"],
    ["all right", "alright"],
    ["pole", "paul", "poll"],
    ["lets", "let us"],
    ["oh", "o", "zero", "nought"],
    ["and", "n"],
    ["vsc", "v s c"],
    ["drs", "d r s"],
    ["kers", "k e r s"],
    ["ers", "e r s"],
    ["gt", "g t"],
    ["lmp", "l m p"],
    ["mr", "mister"],
]

_FILLERS = {"uh", "um", "umm", "hmm", "mm", "ah", "er", "erm", "mmm"}

# One table, shared with `phrases.pronounce`: the engine is asked for the
# same words this comparison expects back, so they cannot drift apart.
from .phrases import _ordinal_words as ordinal_words  # noqa: E402


def _decimal_words(m: re.Match) -> str:
    whole, frac = m.group(1), m.group(2)
    parts = []
    if whole:
        parts.append(number_words(int(whole)))
    parts.append("point")
    parts.append(" ".join("oh" if c == "0" else number_words(int(c)) for c in frac))
    return " ".join(parts)


def _time_words(m: re.Match) -> str:
    minutes, seconds = int(m.group(1)), m.group(2)
    secs = " ".join("oh" if c == "0" else number_words(int(c)) for c in seconds) if seconds.startswith("0") else number_words(int(seconds))
    if seconds == "00":
        secs = "zero zero"
    return f"{number_words(minutes)} {secs}"


def _int_words(m: re.Match) -> str:
    s = m.group(0)
    if len(s) > 1 and s.startswith("0"):
        return " ".join("oh" if c == "0" else number_words(int(c)) for c in s)
    n = int(s)
    # Four digits and up are read out digit by digit — a car number or a
    # year, not "one thousand and twelve". `phrases.pronounce` draws the
    # line in the same place, so both sides of the comparison agree.
    return number_words(n) if n < 1000 else " ".join(number_words(int(c)) for c in s)


def normalise(text: str) -> str:
    """Lower-case words only, numbers spelled out, one canonical spelling per group."""
    t = text.lower().strip()
    t = t.replace("&", " and ").replace("%", " percent ").replace("+", " plus ")
    t = re.sub(r"[‘’´`]", "'", t)
    t = re.sub(r"[-–—/]", " ", t)
    # Every numeric rewrite is padded with spaces so "1:05.3" cannot glue
    # "five" onto "point"; the padding collapses below.
    t = re.sub(r"\bp\.?\s?(\d+)\b", lambda m: " p " + number_words(int(m.group(1))) + " ", t)  # P3
    t = re.sub(r"\b(\d+):(\d\d)\b", lambda m: " " + _time_words(m) + " ", t)  # 1:05 → one oh five
    t = re.sub(r"\b(\d+)(?:st|nd|rd|th)\b", lambda m: " " + ordinal_words(int(m.group(1))) + " ", t)
    t = re.sub(r"(?<=\d),(?=\d{3}\b)", "", t)  # 1,000 → 1000
    t = re.sub(r"(\d+)?\.(\d+)\b", lambda m: " " + _decimal_words(m) + " ", t)  # 1.4, and a bare .3
    t = re.sub(r"\d+", lambda m: " " + _int_words(m) + " ", t)
    for k, v in _CONTRACTIONS.items():
        t = re.sub(rf"\b{re.escape(k)}\b", v, t)
    t = re.sub(r"[^a-z' ]+", " ", t)
    # Apostrophes vanish rather than split: "inside's clear" and "insides
    # clear" are the same sounds, and whisper writes whichever it likes.
    t = t.replace("'", "")
    t = re.sub(r"\s+", " ", t).strip()
    for group in _EQUIVALENTS:
        canonical = group[0]
        for alt in group[1:]:
            t = re.sub(rf"\b{re.escape(alt)}\b", canonical, t)
    t = re.sub(r"\s+", " ", t).strip()
    return t


def tokens(text: str, drop_fillers: bool = False) -> list[str]:
    out = normalise(text).split()
    if drop_fillers:
        out = [w for w in out if w not in _FILLERS]
    return out


# ------------------------------------------------------------- phonetics
#
# Whisper hears "car low" as "Carlo" and "clear right" as "Claire Wright":
# the same sounds, cut into words differently. Those clips are fine to
# play, so a transcript mismatch is forgiven when the two texts reduce to
# the same string of consonant sounds. "right side" heard as "light side"
# does not, and "car high" heard as "cog high" does not — those are the
# errors this check exists to catch. The key is a small metaphone-style
# reduction: enough to ignore word boundaries and vowels, not enough to
# call two different consonants the same.

_PHONETIC_RULES = [
    ("ough", "o"),
    ("augh", "a"),
    ("tch", "ch"),
    ("sch", "sk"),
    ("ph", "f"),
    ("gh", ""),
    ("ck", "k"),
    ("dg", "j"),
    ("sh", "x"),
    ("ch", "x"),
    ("th", "0"),
    ("qu", "kw"),
    ("wh", "w"),
    ("ce", "se"),
    ("ci", "si"),
    ("cy", "sy"),
    ("c", "k"),
    ("q", "k"),
    ("x", "ks"),
    ("z", "s"),
    ("v", "f"),
    ("b", "p"),
    ("d", "t"),
    ("g", "k"),
]


def _phonetic_word(word: str) -> str:
    w = word
    if w.startswith("kn") or w.startswith("gn") or w.startswith("pn") or w.startswith("wr"):
        w = w[1:]
    for old, new in _PHONETIC_RULES:
        w = w.replace(old, new)
    # Drop vowels (and the vowel-like w, h, y) except a leading one, which
    # says the word starts with a vowel sound.
    out = []
    for i, c in enumerate(w):
        if c in "aeiouwhy":
            if i == 0:
                out.append("a")
            continue
        out.append(c)
    key = "".join(out)
    # Collapse repeats: "ll" and "l" sound the same.
    collapsed = []
    for c in key:
        if not collapsed or collapsed[-1] != c:
            collapsed.append(c)
    return "".join(collapsed)


def phonetic_key(text: str) -> str:
    """The consonant skeleton of a phrase, ignoring where the word breaks fall."""
    joined = "".join(_phonetic_word(w) for w in tokens(text, drop_fillers=True))
    collapsed = []
    for c in joined:
        if not collapsed or collapsed[-1] != c:
            collapsed.append(c)
    return "".join(collapsed)


# -------------------------------------------------------------------- WER


@dataclass
class Alignment:
    substitutions: int
    deletions: int
    insertions: int
    ref_len: int
    ops: list[tuple[str, str | None, str | None]] = field(default_factory=list)  # (op, ref, hyp)

    @property
    def errors(self) -> int:
        return self.substitutions + self.deletions + self.insertions

    @property
    def wer(self) -> float:
        if self.ref_len == 0:
            return 0.0 if self.errors == 0 else 1.0
        return self.errors / self.ref_len


def align(ref: list[str], hyp: list[str]) -> Alignment:
    """Levenshtein alignment with the edit script, for showing *where* it went wrong."""
    n, m = len(ref), len(hyp)
    d = np.zeros((n + 1, m + 1), dtype=np.int32)
    d[:, 0] = np.arange(n + 1)
    d[0, :] = np.arange(m + 1)
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            cost = 0 if ref[i - 1] == hyp[j - 1] else 1
            d[i, j] = min(d[i - 1, j] + 1, d[i, j - 1] + 1, d[i - 1, j - 1] + cost)
    ops: list[tuple[str, str | None, str | None]] = []
    i, j = n, m
    subs = dels = ins = 0
    while i > 0 or j > 0:
        if i > 0 and j > 0 and ref[i - 1] == hyp[j - 1] and d[i, j] == d[i - 1, j - 1]:
            ops.append(("ok", ref[i - 1], hyp[j - 1]))
            i, j = i - 1, j - 1
        elif i > 0 and j > 0 and d[i, j] == d[i - 1, j - 1] + 1:
            ops.append(("sub", ref[i - 1], hyp[j - 1]))
            subs += 1
            i, j = i - 1, j - 1
        elif i > 0 and d[i, j] == d[i - 1, j] + 1:
            ops.append(("del", ref[i - 1], None))
            dels += 1
            i -= 1
        else:
            ops.append(("ins", None, hyp[j - 1]))
            ins += 1
            j -= 1
    ops.reverse()
    return Alignment(substitutions=subs, deletions=dels, insertions=ins, ref_len=n, ops=ops)


def word_error_rate(target: str, transcript: str) -> Alignment:
    return align(tokens(target), tokens(transcript, drop_fillers=True))


# ----------------------------------------------------------- audio checks


@dataclass
class AudioChecks:
    duration_s: float
    expected_s: float
    peak_dbfs: float
    clipped_ratio: float
    leading_silence_s: float
    trailing_silence_s: float
    longest_pause_s: float
    hf_energy_ratio: float
    problems: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.problems

    def to_dict(self) -> dict:
        return self.__dict__.copy()


# Roughly how fast the stock chief talks, in characters of text per second
# of audio. Measured on the installed pack: a 3.4 s "you're P3, the gap
# behind is one point four seconds" is 50 characters → ~15 c/s.
CHARS_PER_SECOND = 15.0


def expected_duration(text: str, speed: float = 1.0) -> float:
    chars = len(re.sub(r"\s+", " ", text.strip()))
    return max(0.35, chars / (CHARS_PER_SECOND * speed))


def check_audio(audio: np.ndarray, sr: int, text: str, speed: float = 1.0, silence_dbfs: float = -45.0) -> AudioChecks:
    """Duration against the expected speaking rate, clipping, silence, spectral shape."""
    x = np.asarray(audio, dtype=np.float32).reshape(-1)
    n = len(x)
    duration = n / sr if sr else 0.0
    expected = expected_duration(text, speed)
    problems: list[str] = []

    if n == 0 or not np.isfinite(x).all():
        return AudioChecks(duration, expected, -120.0, 0.0, 0.0, 0.0, 0.0, 0.0, ["empty_or_invalid"])

    peak = float(np.max(np.abs(x)))
    peak_dbfs = 20 * np.log10(peak) if peak > 0 else -120.0
    clipped = float(np.mean(np.abs(x) >= 0.985))

    frame = max(1, int(sr * 0.02))
    frames = n // frame
    if frames == 0:
        rms = np.array([np.sqrt(np.mean(x**2))])
    else:
        rms = np.sqrt(np.mean(x[: frames * frame].reshape(frames, frame) ** 2, axis=1))
    rms_db = 20 * np.log10(np.maximum(rms, 1e-9))
    loud = rms_db > silence_dbfs
    if loud.any():
        first = int(np.argmax(loud))
        last = int(len(loud) - 1 - np.argmax(loud[::-1]))
        leading = first * frame / sr
        trailing = (len(loud) - 1 - last) * frame / sr
        # longest run of quiet frames strictly inside the speech
        longest = 0
        run = 0
        for v in loud[first : last + 1]:
            if v:
                longest = max(longest, run)
                run = 0
            else:
                run += 1
        longest_pause = longest * frame / sr
    else:
        leading = trailing = duration
        longest_pause = 0.0
        problems.append("silent")

    # Spectral shape: the share of energy above 6 kHz. Speech has little;
    # hiss, babble and vocoder failures have a lot.
    spectrum = np.abs(np.fft.rfft(x * np.hanning(n))) ** 2
    freqs = np.fft.rfftfreq(n, 1.0 / sr)
    total = float(spectrum.sum()) or 1.0
    hf_ratio = float(spectrum[freqs >= 6000].sum() / total)

    if duration < 0.45 * expected or duration < 0.25:
        problems.append("too_short")
    if duration > 2.2 * expected + 0.6:
        problems.append("too_long")
    if clipped > 0.001:
        problems.append("clipping")
    if peak_dbfs < -30:
        problems.append("too_quiet")
    if longest_pause > 1.2:
        problems.append("long_pause")
    if hf_ratio > 0.35:
        problems.append("spectral_noise")

    return AudioChecks(
        duration_s=round(duration, 3),
        expected_s=round(expected, 3),
        peak_dbfs=round(float(peak_dbfs), 2),
        clipped_ratio=round(clipped, 5),
        leading_silence_s=round(leading, 3),
        trailing_silence_s=round(trailing, 3),
        longest_pause_s=round(longest_pause, 3),
        hf_energy_ratio=round(hf_ratio, 4),
        problems=problems,
    )


# ------------------------------------------------------------ the verdict


@dataclass
class Verdict:
    passed: bool
    wer: float
    errors: int
    ref_words: int
    transcript: str
    confidence: float | None
    audio: AudioChecks
    reasons: list[str]
    alignment: list[tuple[str, str | None, str | None]] = field(default_factory=list)
    # The words differed but the sounds did not, and the teacher-forced
    # score agreed; passed on that basis.
    phonetic_match: bool = False
    target_score: "TargetScore | None" = None

    def to_dict(self) -> dict:
        d = {
            "passed": self.passed,
            "phonetic_match": self.phonetic_match,
            "target_score": self.target_score.to_dict() if self.target_score else None,
            "wer": round(self.wer, 4),
            "errors": self.errors,
            "ref_words": self.ref_words,
            "transcript": self.transcript,
            "confidence": self.confidence,
            "audio": self.audio.to_dict(),
            "reasons": self.reasons,
            "alignment": [list(op) for op in self.alignment],
        }
        return d


# The word error rate a clip may have and still pass. One wrong word in five
# passes; one in four does not, so anything a spotter says — two or three
# words — has to come back exactly. A false failure costs a two-second
# regeneration; a false pass costs the driver hearing "car lift".
MAX_WER = 0.2


def letters(text: str) -> str:
    """The normalised text with the spaces taken out.

    Where the words break is not something the audio decides: "half-way
    home" and "halfway home" are the same sounds, and whisper writes
    whichever it prefers. Comparing the letters settles those with
    certainty — the same letters in the same order cannot be a different
    phrase — which is worth more than any probability.
    """
    return normalise(text).replace(" ", "")


def _expand_s(text: str) -> str:
    """``rear's punctured`` → ``rear is punctured``.

    Only for the comparison, and only as a second opinion: "your right
    rear's punctured" and "your right rear is punctured" are one utterance
    written two ways. A true possessive ("the driver's door") expands to
    nonsense and simply fails this path, which the plain letter comparison
    has already handled.
    """
    return re.sub(r"(\w)'s\b", r"\1 is", text, flags=re.IGNORECASE)


def same_words_either_way(target: str, transcript: str) -> bool:
    """Whether two texts are the same words, however they are spaced or spelled."""
    if letters(target) == letters(transcript):
        return True
    return letters(_expand_s(target)) == letters(transcript)


# The phonetic rescue — same sounds, different word boundaries, as in "car
# low" heard as "Carlo" — is backed by reading the target against the audio
# teacher-forced. Measured twice on the built-in voice:
#
# * On two-word calls it separates cleanly: correct clips score a mean of
#   0.4–0.9 with a minimum around 0.2, garbled ones 0.00–0.05.
# * On longer phrases it does not. A correct "we're half-way home, we're
#   okay on fuel" scores mean 0.77 but minimum 0.01 — a function word in
#   the middle is always uncertain — and a deliberately *wrong* text
#   against the same audio still scores 0.60.
#
# So the rescue is limited to phrases short enough for the score to mean
# something. Longer ones are settled by the letters above, or they fail.
RESCUE_MIN_WORD = 0.2
RESCUE_MEAN = 0.45
RESCUE_MAX_WORDS = 4


def judge(
    target: str,
    transcript: str,
    audio_checks: AudioChecks,
    confidence: float | None = None,
    target_score: "TargetScore | None" = None,
) -> Verdict:
    al = word_error_rate(target, transcript)
    reasons = list(audio_checks.problems)
    phonetic = False
    if al.ref_len == 0:
        text_ok = al.errors == 0
    elif al.errors == 0:
        text_ok = True
    elif al.wer <= MAX_WER:
        text_ok = True
    elif same_words_either_way(target, transcript):
        # The same letters in the same order: only the spacing or an
        # apostrophe differs. Certain, so no score is consulted.
        text_ok = True
    else:
        same_sounds = phonetic_key(target) != "" and phonetic_key(target) == phonetic_key(transcript)
        supported = (
            al.ref_len <= RESCUE_MAX_WORDS
            and target_score is not None
            and target_score.min >= RESCUE_MIN_WORD
            and target_score.mean >= RESCUE_MEAN
        )
        phonetic = same_sounds and supported
        text_ok = phonetic
    if not text_ok:
        reasons.append("transcript_mismatch")
    if confidence is not None and confidence < -1.2:
        reasons.append("low_confidence")
    return Verdict(
        passed=not reasons,
        wer=al.wer,
        errors=al.errors,
        ref_words=al.ref_len,
        transcript=transcript,
        confidence=confidence,
        audio=audio_checks,
        reasons=reasons,
        alignment=al.ops,
        phonetic_match=phonetic,
        target_score=target_score,
    )


# ------------------------------------------------------------ transcriber


class Transcriber:
    """faster-whisper, loaded on first use, shared by every worker.

    ``import torch`` happens first on purpose: the CUDA and cuDNN DLLs that
    CTranslate2 needs on Windows are the ones inside torch's own package,
    and importing torch is what puts them on the loader path.
    """

    def __init__(self, model_name: str = "large-v3-turbo", device: str = "auto") -> None:
        self.model_name = model_name
        self.device = device
        self._model = None
        self.loaded_on: str | None = None
        import threading

        self._lock = threading.Lock()

    def load(self) -> None:
        with self._lock:
            if self._model is not None:
                return
            import torch  # noqa: F401  (DLL path side effect, see class docstring)
            from faster_whisper import WhisperModel

            wanted = self.device
            if wanted == "auto":
                wanted = "cuda" if torch.cuda.is_available() else "cpu"
            try:
                self._model = WhisperModel(self.model_name, device=wanted, compute_type="float16" if wanted == "cuda" else "int8")
                self.loaded_on = wanted
            except Exception:
                if wanted == "cuda":
                    self._model = WhisperModel(self.model_name, device="cpu", compute_type="int8")
                    self.loaded_on = "cpu"
                else:
                    raise

    def transcribe(self, audio_16k: np.ndarray) -> tuple[str, float | None]:
        """Text and mean log-probability for 16 kHz mono float32 audio."""
        self.load()
        assert self._model is not None
        with self._lock:
            segments, _info = self._model.transcribe(
                np.asarray(audio_16k, dtype=np.float32),
                language="en",
                beam_size=5,
                best_of=5,
                condition_on_previous_text=False,
                without_timestamps=True,
                vad_filter=False,
                temperature=0.0,
            )
            texts = []
            logprobs = []
            for s in segments:
                texts.append(s.text.strip())
                if s.avg_logprob is not None:
                    logprobs.append(float(s.avg_logprob))
        text = " ".join(t for t in texts if t)
        conf = float(np.mean(logprobs)) if logprobs else None
        return text, conf

    def score_target(self, audio_16k: np.ndarray, target: str) -> "TargetScore | None":
        """How well the audio supports the *target* text, teacher-forced.

        The free transcription answers "what did whisper hear"; this answers
        "if it had to read the target, how sure would each word be". It is
        the same alignment call faster-whisper uses for word timestamps,
        which returns a probability per text token. A clip that says the
        words scores high on every word even when the free decode chose
        different spellings; a clip that says a different word scores low on
        that word. Returns None if the model cannot do it.
        """
        self.load()
        assert self._model is not None
        try:
            from faster_whisper.audio import pad_or_trim
            from faster_whisper.tokenizer import Tokenizer
        except Exception:
            return None
        text = target.strip()
        if not text:
            return None
        with self._lock:
            model = self._model
            features = model.feature_extractor(np.asarray(audio_16k, dtype=np.float32))
            content_frames = features.shape[-1] - 1
            segment_size = int(min(model.feature_extractor.nb_max_frames, max(1, content_frames)))
            segment = pad_or_trim(features[:, :segment_size])
            encoder_output = model.encode(segment)
            tok = Tokenizer(model.hf_tokenizer, model.model.is_multilingual, task="transcribe", language="en")
            text_tokens = tok.encode(" " + text)
            if not text_tokens:
                return None
            results = model.model.align(
                encoder_output, tok.sot_sequence, [text_tokens], segment_size, median_filter_width=7
            )
            probs = [float(p) for p in results[0].text_token_probs]
            words, word_tokens = tok.split_to_word_tokens(text_tokens + [tok.eot])
        word_probs: list[tuple[str, float]] = []
        i = 0
        for word, toks in zip(words, word_tokens):
            n = len(toks)
            chunk = probs[i : i + n]
            i += n
            if word.strip() and chunk:
                word_probs.append((word.strip(), float(np.mean(chunk))))
        if not word_probs:
            return None
        return TargetScore(
            mean=float(np.mean([p for _, p in word_probs])),
            min=float(min(p for _, p in word_probs)),
            words=word_probs,
        )

    def unload(self) -> None:
        with self._lock:
            self._model = None
            self.loaded_on = None


@dataclass
class TargetScore:
    """Per-word probabilities of the target text given the audio."""

    mean: float
    min: float
    words: list[tuple[str, float]]

    def to_dict(self) -> dict:
        return {"mean": round(self.mean, 4), "min": round(self.min, 4), "words": [[w, round(p, 4)] for w, p in self.words]}


def resample_to_16k(audio: np.ndarray, sr: int) -> np.ndarray:
    if sr == 16000:
        return np.asarray(audio, dtype=np.float32)
    from .audio import resample

    return resample(audio, sr, 16000)


def summarise_reasons(reasons: Iterable[str]) -> str:
    names = {
        "transcript_mismatch": "said something else",
        "too_short": "too short",
        "too_long": "too long",
        "clipping": "clipping",
        "too_quiet": "too quiet",
        "long_pause": "a long pause",
        "spectral_noise": "noisy",
        "silent": "silent",
        "empty_or_invalid": "no audio",
        "low_confidence": "hard to make out",
    }
    return ", ".join(names.get(r, r) for r in reasons)
