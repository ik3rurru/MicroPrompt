"""Who is speaking: live speaker labels for MicroPrompt captions.

Sortformer v2.1 (up to 4 voices at once) runs locally through transcribe.cpp's
Python bindings: the same model and runtime as upstream Hex's Labs branch
"Speakers in meetings" (docs/research/meeting-speakers.md there), on the GPU
when there is one (Vulkan on Windows, Metal on a Mac).

transcribe.cpp diarizes a recording at once; it has no incremental API. So
each time Hex closes a window (a `segment`, cut at a pause), the tracker
diarizes the audio of the current block, from the block's fixed start to now,
and the segment takes the voice heard most in it. Segments that pile up while
a run is busy are labelled together by the next run.

A block grows to BLOCK seconds; then a new one starts OVERLAP seconds back,
as upstream Hex stitches a meeting's 10-minute chunks. Sortformer numbers
voices by arrival, so within a block every run agrees; each run's voices are
matched to the previous run's by who talks in the audio both heard. A voice
that was silent through the overlap could not be matched that way, so a new
block starts with a few seconds of each such voice (its "reference", taken
from its own lines) as a fixed prefix, and the voice the model hears there is
that speaker. Voices enrolled with a name start the first block the same way.

Measured with real voices (README): runs over continuous audio from a fixed
start got every turn right; a window that slid with every run, or samples of
each voice pasted in front of a short window, confused voices.
"""

from __future__ import annotations

import hashlib
import os
import queue
import statistics
import sys
import threading
import time
import urllib.request
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Protocol

import numpy as np

RATE = 16_000

# Upstream Hex's pinned artifact (meeting-speakers-lab, SPEAKER_MODEL_ARTIFACT).
MODEL_FILE = "diar_streaming_sortformer_4spk-v2.1-Q8_0.gguf"
MODEL_REPOSITORY = "handy-computer/diar_streaming_sortformer_4spk-v2.1-gguf"
MODEL_REVISION = "60ebab3ef7cae9869be0abcde13360347e96ba03"
MODEL_BYTES = 139_310_336
MODEL_SHA256 = "a5dacdc650790266c7a362e54e6bf51952015487edaa606c4e11632bc32442a9"

BLOCK_S = 120.0  # most audio diarized per run
OVERLAP_S = 60.0  # audio a new block shares with the previous one
MIN_VOICE_S = 1.0  # speech a new voice needs before it gets a number
MIN_MATCH_S = 0.5  # talking together this long matches two runs' voices
REFERENCE_S = 6.0  # a voice's own speech kept to start later blocks
PREFIX_GAP_S = 1.0  # silence after each reference in a block's prefix
MAX_VOICES = 4  # Sortformer v2.1 tells this many apart in one run
NEAREST_S = 1.0  # a segment no voice covers takes the nearest one this close
PROVISIONAL_EVERY_S = 1.5  # how often the line being spoken is labelled


# --------------------------------------------------------------------------
# The model file
# --------------------------------------------------------------------------


def default_model_dir() -> Path:
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
        return base / "MicroPrompt" / "models"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Caches" / "MicroPrompt" / "models"
    return Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "microprompt" / "models"


def model_url() -> str:
    return f"https://huggingface.co/{MODEL_REPOSITORY}/resolve/{MODEL_REVISION}/{MODEL_FILE}"


def ensure_model(path: str | None, log: Callable[[str], None]) -> Path:
    """The Sortformer GGUF: `path` when given, else the pinned artifact,
    downloaded once (139 MB) and checked against its SHA-256."""
    if path and path != "auto":
        model = Path(path)
        if not model.is_file():
            raise SystemExit(f"No encuentro el modelo de hablantes {model}.")
        return model
    model = default_model_dir() / MODEL_FILE
    if model.is_file() and model.stat().st_size == MODEL_BYTES:
        return model
    model.parent.mkdir(parents=True, exist_ok=True)
    partial = model.with_suffix(".part")
    log(f"Descargando el modelo de hablantes (139 MB) en {model.parent} …")
    digest = hashlib.sha256()
    try:
        with urllib.request.urlopen(model_url(), timeout=60) as response, open(partial, "wb") as out:
            done, shown = 0, -1
            while chunk := response.read(1 << 20):
                out.write(chunk)
                digest.update(chunk)
                done += len(chunk)
                if done * 10 // MODEL_BYTES != shown:
                    shown = done * 10 // MODEL_BYTES
                    log(f"  {done * 100 // MODEL_BYTES} %")
    except OSError as error:
        raise SystemExit(f"No pude descargar el modelo de hablantes: {error}\n"
                         f"Descárgalo a mano desde {model_url()} y pásalo con --speakers <ruta>.")
    if digest.hexdigest() != MODEL_SHA256:
        partial.unlink(missing_ok=True)
        raise SystemExit("El modelo de hablantes descargado no coincide con su SHA-256; vuelve a intentarlo.")
    partial.replace(model)
    return model


# --------------------------------------------------------------------------
# Diarizers
# --------------------------------------------------------------------------


class Diarizer(Protocol):
    def run(self, pcm: np.ndarray) -> list[tuple[float, float, int]]:
        """Speech stretches as (start s, end s, voice), voices numbered by
        arrival in this run."""

    def describe(self) -> str: ...


class SortformerDiarizer:
    """Sortformer through transcribe.cpp (`pip install transcribe-cpp`)."""

    def __init__(self, model: Path, backend: str = "auto", preset: str | None = None):
        try:
            import transcribe_cpp
        except ImportError:
            raise SystemExit("Para distinguir hablantes instala transcribe.cpp: py -m pip install transcribe-cpp")

        def native_log(level: int, message: str) -> None:
            if level in (2, 3):  # warnings and errors
                sys.stderr.write(f"transcribe.cpp: {message.rstrip()}\n")

        transcribe_cpp.set_log_callback(native_log)
        self.model = transcribe_cpp.Model(str(model), backend=backend)
        self.session = self.model.diarize_session()
        self.options = transcribe_cpp.SortformerDiarizeOptions(preset=preset) if preset else None

    def run(self, pcm: np.ndarray) -> list[tuple[float, float, int]]:
        pcm = np.ascontiguousarray(pcm, dtype=np.float32)
        segments = self.session.run(pcm, family=self.options) if self.options else self.session.run(pcm)
        return [
            (s.t0_ms / 1000, s.t1_ms / 1000, int(s.speaker_id))
            for s in segments
            if s.speaker_id > 0 and s.t1_ms > s.t0_ms
        ]

    def describe(self) -> str:
        device = self.model.device
        return f"Sortformer v2.1 en {device.description or device.name} ({self.model.backend})"


# --------------------------------------------------------------------------
# The audio the stream heard
# --------------------------------------------------------------------------


class Timeline:
    """The audio sent to Hex, addressed by sample number from the start of
    the stream (the same clock as a segment's startMs/endMs), keeping the
    last `keep_s` seconds."""

    def __init__(self, keep_s: float = 300.0):
        self.keep = int(keep_s * RATE)
        self.blocks: deque[tuple[int, np.ndarray]] = deque()
        self.end = 0
        self.lock = threading.Lock()

    def append(self, samples: np.ndarray) -> None:
        with self.lock:
            self.blocks.append((self.end, np.asarray(samples, np.float32)))
            self.end += samples.size
            while self.blocks and self.blocks[0][0] + self.blocks[0][1].size < self.end - self.keep:
                self.blocks.popleft()

    @property
    def start(self) -> int:
        with self.lock:
            return self.blocks[0][0] if self.blocks else self.end

    def slice(self, start: int, end: int) -> np.ndarray:
        with self.lock:
            parts = []
            for offset, block in self.blocks:
                lo, hi = max(start, offset), min(end, offset + block.size)
                if lo < hi:
                    parts.append(block[lo - offset : hi - offset])
        return np.concatenate(parts) if parts else np.zeros(0, np.float32)


# --------------------------------------------------------------------------
# Voices and the tracker
# --------------------------------------------------------------------------


@dataclass(eq=False)
class Voice:
    """One voice across runs. It gets a number and a name only when it first
    labels a finished segment, so a stray detection never shows."""

    number: int | None = None
    name: str | None = None
    reference: np.ndarray = field(default_factory=lambda: np.zeros(0, np.float32))
    enrolled: bool = False
    last_heard: float = -1.0  # stream seconds; -1 = not yet

    @property
    def shown(self) -> bool:
        return self.number is not None


@dataclass
class Stats:
    runs: list[float] = field(default_factory=list)  # seconds per diarization
    audio: list[float] = field(default_factory=list)  # seconds diarized per run
    lags: list[float] = field(default_factory=list)  # segment event -> label

    def summary(self) -> list[str]:
        lines = []
        if self.runs:
            speed = sum(self.audio) / max(sum(self.runs), 1e-9)
            lines.append(f"diarización: {statistics.median(self.runs):.2f} s por pasada "
                         f"(mediana; {len(self.runs)} pasadas, {speed:.0f}x tiempo real)")
        if self.lags:
            lines.append(f"etiqueta de cada frase: {statistics.median(self.lags):.2f} s tras su texto (mediana)")
        return lines


class SpeakerTracker:
    def __init__(self, diarizer: Diarizer, timeline: Timeline, names: list[str] | None = None,
                 block_s: float = BLOCK_S, overlap_s: float = OVERLAP_S):
        self.diarizer = diarizer
        self.timeline = timeline
        self.names = list(names or [])
        self.block = int(block_s * RATE)
        self.overlap = int(min(overlap_s, block_s / 2) * RATE)
        self.block_start = 0
        self.voices: list[Voice] = []  # shown voices, by number
        self.prefix: list[tuple[Voice, np.ndarray]] = []  # this block's references
        self.previous: list[tuple[float, float, Voice]] = []  # last run, in stream seconds
        self.lock = threading.Lock()
        self.stats = Stats()

    def enroll(self, name: str, samples: np.ndarray) -> Voice:
        """A known voice from a recording of it alone (16 kHz mono)."""
        samples = np.asarray(samples, np.float32)
        if samples.size < MIN_VOICE_S * RATE:
            raise ValueError(f"la muestra de {name} dura menos de {MIN_VOICE_S:g} s")
        with self.lock:
            reference = samples[: int(REFERENCE_S * RATE)].copy()
            voice = Voice(len(self.voices) + 1, name, reference, enrolled=True)
            self.voices.append(voice)
            if len(self.prefix) < MAX_VOICES:
                self.prefix.append((voice, reference))
        return voice

    @property
    def enrolled(self) -> list[Voice]:
        return [v for v in self.voices if v.enrolled]

    def _start_block(self, start: int) -> None:
        """A new block from sample `start`: the voices that spoke in the
        audio it shares with the last run match by overlap; the others that
        have a reference go first, the most recently heard ones, while the
        block can still tell them apart."""
        self.block_start = start
        shared = start / RATE
        active = {id(v) for s, e, v in self.previous if e > shared and v.shown}
        absent = [v for v in self.voices
                  if id(v) not in active and v.reference.size >= MIN_VOICE_S * RATE]
        absent.sort(key=lambda v: v.last_heard if v.last_heard >= 0 else float("inf"), reverse=True)
        # Every slot the active voices leave: a known voice coming back is
        # likelier than a fifth one (measured: reserving a slot for a new
        # voice cost the returning one its number).
        room = max(0, MAX_VOICES - len(active))
        self.prefix = [(v, v.reference.copy()) for v in absent[:room]]

    def label(self, start: int, end: int, provisional: bool = False) -> Voice | None:
        """The voice heard most between samples `start` and `end`. A final
        label may number a new voice; a provisional one only names voices
        already shown."""
        return self.label_many([(start, end)], provisional)[0]

    def label_many(self, spans: list[tuple[int, int]], provisional: bool = False) -> list[Voice | None]:
        """`label` for several segments, from one run."""
        with self.lock:
            return self._label(spans, provisional)

    def _label(self, spans: list[tuple[int, int]], provisional: bool) -> list[Voice | None]:
        stop = self.timeline.end
        spans = [(start, min(end, stop)) for start, end in spans]
        first = min(start for start, _ in spans)
        if stop - self.block_start > self.block:
            self._start_block(stop - self.overlap)
        self.block_start = max(self.timeline.start, min(self.block_start, first))
        window_start = self.block_start
        live = self.timeline.slice(window_start, stop)

        gap = np.zeros(int(PREFIX_GAP_S * RATE), np.float32)
        parts, prefix, offset = [], [], 0.0
        for voice, reference in self.prefix:
            prefix.append((voice, offset, offset + reference.size / RATE))
            parts += [reference, gap]
            offset += (reference.size + gap.size) / RATE
        pcm = np.concatenate(parts + [live]) if parts else live

        started = time.perf_counter()
        found = self.diarizer.run(pcm)
        self.stats.runs.append(time.perf_counter() - started)
        self.stats.audio.append(pcm.size / RATE)

        # This run's stretches of live audio, in stream seconds.
        base = window_start / RATE - offset
        stretches = [(s + base, e + base, l) for s, e, l in found if e > offset]
        stretches = [(max(s, window_start / RATE), e, l) for s, e, l in stretches]

        # Match this run's voices: enrolled ones by their prefix, the others
        # by who they talk with in the audio the previous run also heard.
        claims = []
        locals_ = {l for _, _, l in found}
        for voice, a0, a1 in prefix:
            for local in locals_:
                heard = sum(max(0.0, min(a1, e) - max(a0, s)) for s, e, l in found if l == local)
                if heard >= min(MIN_MATCH_S, 0.3 * (a1 - a0)):
                    claims.append((heard + 1e6, local, voice))  # prefixes win
        for local in locals_:
            together: dict[int, tuple[float, Voice]] = {}
            for s, e, l in stretches:
                if l != local:
                    continue
                for ps, pe, voice in self.previous:
                    overlap = min(e, pe) - max(s, ps)
                    if overlap > 0:
                        heard, _ = together.get(id(voice), (0.0, voice))
                        together[id(voice)] = (heard + overlap, voice)
            for heard, voice in together.values():
                if heard >= MIN_MATCH_S:
                    claims.append((heard, local, voice))
        voice_of: dict[int, Voice] = {}
        for _, local, voice in sorted(claims, key=lambda c: -c[0]):
            if local not in voice_of and voice not in voice_of.values():
                voice_of[local] = voice
        for local in locals_:
            if local not in voice_of:
                voice_of[local] = Voice()  # unseen; numbered when it labels a segment
        self.previous = [(s, e, voice_of[l]) for s, e, l in stretches]

        voices = [self._heard_most(start, end, stretches, voice_of, provisional) for start, end in spans]
        if not provisional:
            for (start, end), voice in zip(spans, voices):
                if voice is not None:
                    self._heard(voice, start, end, stretches, voice_of, live, window_start)
        return voices

    def _heard(self, voice, start, end, stretches, voice_of, live, window_start) -> None:
        """Notes when `voice` last spoke and grows its reference with its own
        speech in this segment."""
        voice.last_heard = max(voice.last_heard, end / RATE)
        if voice.enrolled or voice.reference.size >= REFERENCE_S * RATE:
            return
        t0, t1, origin = start / RATE, end / RATE, window_start / RATE
        pieces = [
            live[int((max(s, t0) - origin) * RATE) : int((min(e, t1) - origin) * RATE)]
            for s, e, l in stretches
            if voice_of.get(l) is voice and min(e, t1) > max(s, t0)
        ]
        voice.reference = np.concatenate([voice.reference, *pieces])[: int(REFERENCE_S * RATE)]

    def _heard_most(self, start, end, stretches, voice_of, provisional) -> Voice | None:
        if end - start < RATE // 4:
            return None
        t0, t1 = start / RATE, end / RATE
        heard: dict[int, float] = {}
        for s, e, l in stretches:
            overlap = min(t1, e) - max(t0, s)
            if overlap > 0:
                heard[l] = heard.get(l, 0.0) + overlap
        if heard:
            local = max(heard, key=heard.get)
        else:
            near = [(min(abs(s - t1), abs(e - t0)), l) for s, e, l in stretches
                    if s < t1 + NEAREST_S and e > t0 - NEAREST_S]
            if not near:
                return None
            local = min(near)[1]
        voice = voice_of[local]
        if not voice.shown:
            if provisional or heard.get(local, 0.0) < MIN_VOICE_S:
                return None
            voice.number = len(self.voices) + 1
            unnamed = sum(1 for v in self.voices if not v.enrolled)
            voice.name = self.names[unnamed] if unnamed < len(self.names) else f"Hablante {voice.number}"
            self.voices.append(voice)
        return voice


# --------------------------------------------------------------------------
# The worker the captions use
# --------------------------------------------------------------------------


class SpeakerWorker:
    """Labels segments off the event thread. Every closed segment is
    labelled in order; the line still being spoken is labelled now and then
    when nothing else is waiting."""

    def __init__(self, tracker: SpeakerTracker, on_label: Callable[[int, Voice | None, bool], None]):
        self.tracker = tracker
        self.on_label = on_label
        self.finals: queue.Queue = queue.Queue()
        self.pending_provisional: tuple[int, int] | None = None
        self.last_provisional = 0.0
        self.lock = threading.Lock()
        self.done = threading.Condition()
        self.outstanding = 0  # segments handed over and not yet labelled
        self.stopped = False
        self.wake = threading.Event()
        threading.Thread(target=self._run, name="speakers", daemon=True).start()

    def segment(self, index: int, start_ms: int, end_ms: int) -> None:
        with self.done:
            self.outstanding += 1
        self.finals.put((index, start_ms * RATE // 1000, end_ms * RATE // 1000, time.monotonic()))
        self.wake.set()

    def speaking(self, index: int, start_ms: int) -> None:
        """The line being spoken (segment `index`, from `start_ms`) changed."""
        if time.monotonic() - self.last_provisional < PROVISIONAL_EVERY_S:
            return
        with self.lock:
            self.pending_provisional = (index, start_ms * RATE // 1000)
        self.wake.set()

    def drain(self, timeout: float) -> bool:
        """Waits until every segment handed over is labelled."""
        with self.done:
            return self.done.wait_for(lambda: self.outstanding == 0, timeout)

    def stop(self) -> None:
        self.stopped = True
        self.wake.set()

    def _run(self) -> None:
        while not self.stopped:
            self.wake.wait(0.5)
            self.wake.clear()
            while not self.stopped:
                batch = []
                while True:
                    try:
                        batch.append(self.finals.get_nowait())
                    except queue.Empty:
                        break
                if not batch:
                    break
                voices = self._label([(start, end) for _, start, end, _ in batch], False)
                for (index, _, _, queued), voice in zip(batch, voices):
                    self.tracker.stats.lags.append(time.monotonic() - queued)
                    self.on_label(index, voice, False)
                with self.done:
                    self.outstanding -= len(batch)
                    self.done.notify_all()
            with self.lock:
                provisional, self.pending_provisional = self.pending_provisional, None
            if provisional and not self.stopped and self.finals.empty():
                index, start = provisional
                self.last_provisional = time.monotonic()
                voice = self._label([(start, self.tracker.timeline.end)], True)[0]
                if voice is not None:
                    self.on_label(index, voice, True)

    def _label(self, spans: list[tuple[int, int]], provisional: bool) -> list[Voice | None]:
        try:
            return self.tracker.label_many(spans, provisional)
        except Exception as error:  # a failed run leaves the lines unlabelled
            sys.stderr.write(f"No pude diarizar: {error}\n")
            return [None] * len(spans)
