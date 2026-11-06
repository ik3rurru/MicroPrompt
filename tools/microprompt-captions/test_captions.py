"""Tests for captions.py, against mock_hex.py (no Hex, no microphone).

    python -m unittest -v test_captions.py
"""

from __future__ import annotations

import contextlib
import io
import json
import tempfile
import threading
import unittest
import urllib.request
import wave
from pathlib import Path

import numpy as np

import captions
import mock_hex
import speakers


def tone(hz: float, seconds: float, rate: int = 16_000) -> np.ndarray:
    """A loud modulated tone: one "voice" for FakeDiarizer."""
    t = np.arange(int(seconds * rate)) / rate
    return (0.3 * np.sin(2 * np.pi * hz * t) * (0.6 + 0.4 * np.sin(2 * np.pi * 3 * t))).astype(np.float32)


def speech_like_wav(path: Path, rate: int = 48_000, channels: int = 2, phrases: int = 3,
                    phrase_s: float = 2.4, pause_s: float = 1.2, voices: list[float] | None = None) -> None:
    """Loud modulated tones separated by near-silence, as 16-bit PCM; with
    `voices`, phrase i is a tone of voices[i] Hz."""
    rng = np.random.default_rng(7)
    parts = [np.zeros(int(0.5 * rate))]
    for index in range(phrases):
        hz = voices[index] if voices else 220
        tone = globals()["tone"](hz, phrase_s, rate)
        parts += [tone, 0.0005 * rng.standard_normal(int(pause_s * rate))]
    mono = np.concatenate(parts)
    frames = np.repeat(mono[:, None], channels, axis=1)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((frames * 32767).astype("<i2").tobytes())


class FakeDiarizer:
    """Tells "voices" apart by their tone and, like Sortformer, numbers them
    by arrival within each run."""

    def __init__(self):
        self.runs = 0

    def run(self, pcm: np.ndarray) -> list[tuple[float, float, int]]:
        self.runs += 1
        frame = 1600
        order: dict[int, int] = {}
        found: list[list] = []
        for i in range(pcm.size // frame):
            x = pcm[i * frame : (i + 1) * frame]
            if np.sqrt(np.mean(x ** 2)) < 0.02:
                continue
            hz = int(round(dominant_hz(x, 16_000) / 50.0)) * 50
            local = order.setdefault(hz, len(order) + 1)
            s, e = i * 0.1, (i + 1) * 0.1
            if found and found[-1][2] == local and abs(found[-1][1] - s) < 1e-6:
                found[-1][1] = e
            else:
                found.append([s, e, local])
        return [tuple(f) for f in found]

    def describe(self) -> str:
        return "diarizador de prueba"


def dominant_hz(samples: np.ndarray, rate: int) -> float:
    spectrum = np.abs(np.fft.rfft(samples * np.hanning(samples.size)))
    return float(np.fft.rfftfreq(samples.size, 1 / rate)[np.argmax(spectrum)])


class ResamplerTests(unittest.TestCase):
    def check(self, rate_in: int, block: int) -> None:
        t = np.arange(rate_in * 2) / rate_in
        signal = 0.5 * np.sin(2 * np.pi * 440 * t)
        resampler = captions.Resampler(rate_in)
        out = np.concatenate([resampler.process(signal[i : i + block]) for i in range(0, signal.size, block)])
        self.assertLessEqual(abs(out.size - 32_000), 2, f"{rate_in} Hz -> {out.size} samples")
        self.assertAlmostEqual(dominant_hz(out[2000:], 16_000), 440, delta=2)
        self.assertLess(np.max(np.abs(out)), 0.55)

    def test_common_device_rates_become_16_khz_in_blocks(self):
        for rate, block in [(48_000, 960), (44_100, 882), (32_000, 333), (16_000, 320), (8_000, 160)]:
            with self.subTest(rate=rate):
                self.check(rate, block)

    def test_energy_above_the_new_nyquist_is_filtered(self):
        rate = 48_000
        t = np.arange(rate) / rate
        out = captions.Resampler(rate).process(0.5 * np.sin(2 * np.pi * 12_000 * t))
        self.assertLess(np.sqrt(np.mean(out[200:] ** 2)), 0.02)

    def test_s16le_clips_and_scales(self):
        pcm = captions.to_s16le(np.array([0.0, 1.0, -1.0, 2.0], np.float32))
        self.assertEqual(np.frombuffer(pcm, "<i2").tolist(), [0, 32767, -32767, 32767])


class SseTests(unittest.TestCase):
    def test_events_are_parsed_and_keep_alives_skipped(self):
        stream = io.BytesIO(
            b'data: {"type":"ready","partials":true}\n\n'
            b": keep-alive\n\n"
            b'data: {"type":"partial","segment":0,"stable":"hola","tentative":" a"}\r\n\r\n'
            b'data: {"type":"final","text":"Hola a todos.","segments":[]}\n\n'
        )
        events = list(captions.parse_sse(stream))
        self.assertEqual([e["type"] for e in events], ["ready", "partial", "final"])
        self.assertEqual(events[1]["tentative"], " a")


class SpeakerTrackerTests(unittest.TestCase):
    A, B, C, D = 300.0, 500.0, 700.0, 900.0

    def conversation(self, tracker, timeline, turns, seconds=1.5):
        numbers = []
        for hz in turns:
            start = timeline.end
            timeline.append(tone(hz, seconds))
            end = timeline.end
            timeline.append(np.zeros(8000, np.float32))
            voice = tracker.label(start, end)
            numbers.append(voice.number if voice else None)
        return numbers

    def test_voices_keep_their_numbers_and_names_across_runs_and_blocks(self):
        timeline = speakers.Timeline()
        # 6 s blocks: a 40 s conversation hands over to a new block many times.
        tracker = speakers.SpeakerTracker(FakeDiarizer(), timeline, names=["Iker", "Ana"], block_s=6, overlap_s=3)
        A, B, C = self.A, self.B, self.C
        numbers = self.conversation(tracker, timeline, [A, B, A, C, B, A, C, A, B, A, C, B, A, B, C, A, B, C, A, B])
        self.assertEqual(numbers, [1, 2, 1, 3, 2, 1, 3, 1, 2, 1, 3, 2, 1, 2, 3, 1, 2, 3, 1, 2])
        self.assertEqual([v.name for v in tracker.voices], ["Iker", "Ana", "Hablante 3"])

    def test_an_enrolled_voice_is_named_even_after_a_long_absence(self):
        timeline = speakers.Timeline()
        tracker = speakers.SpeakerTracker(FakeDiarizer(), timeline, block_s=6, overlap_s=3)
        tracker.enroll("Iker", tone(self.A, 3))
        numbers = self.conversation(tracker, timeline, [self.B] * 8 + [self.A, self.B])
        self.assertEqual(numbers, [2] * 8 + [1, 2])
        self.assertEqual([v.name for v in tracker.voices], ["Iker", "Hablante 2"])

    def test_a_blip_never_gets_a_number_and_provisional_labels_add_no_voice(self):
        timeline = speakers.Timeline()
        tracker = speakers.SpeakerTracker(FakeDiarizer(), timeline)
        self.assertEqual(self.conversation(tracker, timeline, [self.A]), [1])
        self.assertEqual(self.conversation(tracker, timeline, [self.B], seconds=0.5), [None])
        start = timeline.end
        timeline.append(tone(self.C, 2))
        self.assertIsNone(tracker.label(start, timeline.end, provisional=True))
        self.assertEqual(tracker.label(start, timeline.end, provisional=False).number, 2)
        self.assertEqual(len(tracker.voices), 2)

    def test_one_run_labels_every_waiting_segment(self):
        timeline = speakers.Timeline()
        diarizer = FakeDiarizer()
        tracker = speakers.SpeakerTracker(diarizer, timeline)
        spans = []
        for hz in (self.A, self.B, self.A):
            start = timeline.end
            timeline.append(tone(hz, 1.5))
            spans.append((start, timeline.end))
            timeline.append(np.zeros(8000, np.float32))
        voices = tracker.label_many(spans)
        self.assertEqual([v.number for v in voices], [1, 2, 1])
        self.assertEqual(diarizer.runs, 1)

    def test_the_timeline_keeps_the_newest_audio_on_the_stream_clock(self):
        timeline = speakers.Timeline(keep_s=1)
        for value in range(5):
            timeline.append(np.full(8000, value, np.float32))  # 0.5 s each
        self.assertEqual(timeline.end, 40_000)
        self.assertLessEqual(timeline.start, 40_000 - 16_000)
        self.assertEqual(timeline.slice(32_000, 40_000).tolist(), [4.0] * 8000)


class CaptionsTests(unittest.TestCase):
    def test_a_segment_replaces_its_partial_and_keeps_the_newest_lines(self):
        state = captions.Captions(keep=2)
        state.apply({"type": "partial", "segment": 0, "stable": "hola a", "tentative": " todos"})
        snapshot = state.snapshot()
        self.assertEqual((snapshot["stable"], snapshot["tentative"]), ("hola a", " todos"))
        for index, text in enumerate(["Uno.", "Dos.", "Tres."]):
            state.apply({"type": "segment", "segment": index, "startMs": 0, "endMs": 1, "text": text})
        state.apply({"type": "partial", "segment": 3, "stable": "cuatro", "tentative": ""})
        snapshot = state.snapshot()
        self.assertEqual([line["text"] for line in snapshot["lines"]], ["Dos.", "Tres."])
        self.assertEqual(snapshot["stable"], "cuatro")

    def test_speakers_label_lines_and_the_line_being_spoken(self):
        state = captions.Captions()
        state.apply({"type": "segment", "segment": 0, "startMs": 0, "endMs": 1, "text": "Hola."})
        state.apply({"type": "partial", "segment": 1, "stable": "qué", "tentative": " tal"})
        state.set_speaker(0, 1, "Iker", provisional=False)
        state.set_speaker(1, 2, "Ana", provisional=True)
        snapshot = state.snapshot()
        self.assertEqual(snapshot["lines"], [{"text": "Hola.", "speaker": 1, "name": "Iker"}])
        self.assertEqual((snapshot["speaker"], snapshot["name"]), (2, "Ana"))

    def test_a_transcript_has_one_paragraph_per_turn(self):
        segments = [{"segment": i, "text": t} for i, t in enumerate(["Hola.", "¿Qué tal?", "Bien.", "  "])]
        self.assertEqual(
            captions.transcript(segments, {0: "Iker", 1: "Iker", 2: "Ana"}),
            "Iker: Hola. ¿Qué tal?\n\nAna: Bien.",
        )
        self.assertEqual(captions.transcript(segments), "Hola. ¿Qué tal? Bien.")

    def test_srt_times_and_numbering(self):
        self.assertEqual(captions.srt_time(3_723_456), "01:02:03,456")
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "out.srt"
            count = captions.write_srt(path, [
                {"segment": 0, "startMs": 500, "endMs": 2900, "text": "Hola."},
                {"segment": 1, "startMs": 3000, "endMs": 3100, "text": "  "},
                {"segment": 2, "startMs": 4000, "endMs": 6000, "text": "Adiós."},
            ])
            self.assertEqual(count, 2)
            self.assertEqual(
                path.read_text(encoding="utf-8"),
                "1\n00:00:00,500 --> 00:00:02,900\nHola.\n\n2\n00:00:04,000 --> 00:00:06,000\nAdiós.\n",
            )


class EndToEndTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.dir = Path(self.folder.name)
        self.discovery = self.dir / "local-api.json"
        self.wav = self.dir / "speech.wav"
        speech_like_wav(self.wav)

    def tearDown(self):
        self.folder.cleanup()

    def serve(self, **options):
        server, hex_ = mock_hex.start(self.discovery, **options)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return hex_

    def run_captions(self, *extra: str) -> tuple[int, str]:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = captions.main([
                "--discovery", str(self.discovery), "--wav", str(self.wav), "--speed", "8", *extra,
            ])
        return code, output.getvalue()

    def test_a_wav_becomes_segments_a_transcript_and_subtitles(self):
        self.serve()
        srt, txt = self.dir / "out.srt", self.dir / "out.txt"
        code, output = self.run_captions("--no-page", "--console", "--srt", str(srt), "--txt", str(txt))
        self.assertEqual(code, 0, output)
        self.assertIn("parciales sí", output)
        self.assertIn("primeras palabras", output)
        blocks = [b for b in srt.read_text(encoding="utf-8").split("\n\n") if b.strip()]
        self.assertEqual(len(blocks), 3, srt.read_text(encoding="utf-8"))
        self.assertTrue(txt.read_text(encoding="utf-8").startswith("Hola a todos"))

    def test_speakers_name_who_said_each_subtitle(self):
        self.serve()
        speech_like_wav(self.wav, phrases=4, voices=[300, 500, 300, 700])
        srt, txt = self.dir / "out.srt", self.dir / "out.txt"
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = captions.main([
                "--discovery", str(self.discovery), "--wav", str(self.wav), "--speed", "8", "--no-page",
                "--console", "--speakers", "--names", "Iker,Ana", "--srt", str(srt), "--txt", str(txt),
            ], diarizer=FakeDiarizer())
        self.assertEqual(code, 0, output.getvalue())
        subtitles = [b.split("\n")[2] for b in srt.read_text(encoding="utf-8").split("\n\n") if b.strip()]
        self.assertEqual([line.split(":")[0] for line in subtitles], ["Iker", "Ana", "Iker", "Hablante 3"])
        self.assertEqual(txt.read_text(encoding="utf-8").count("\n\n"), 3)
        self.assertIn("3 voces", output.getvalue())

    def test_without_partials_phrases_still_arrive_whole(self):
        self.serve(partials=False)
        srt = self.dir / "out.srt"
        code, output = self.run_captions("--no-page", "--srt", str(srt))
        self.assertEqual(code, 0, output)
        self.assertIn("parciales NO", output)
        self.assertIn("no llegó ningún parcial", output)
        self.assertEqual(srt.read_text(encoding="utf-8").count("-->"), 3)

    def test_refusals_are_explained(self):
        cases = [
            (dict(loaded=False), [], "no tiene un modelo cargado"),
            (dict(language="en"), ["--language", "es"], "distinto de lo pedido"),
        ]
        for options, extra, message in cases:
            with self.subTest(options=options):
                server, _ = mock_hex.start(self.discovery, **options)
                try:
                    code, output = self.run_captions("--no-page", *extra)
                finally:
                    server.shutdown()
                    server.server_close()
                self.assertEqual(code, 2, output)
                self.assertIn(message, output)

    def test_a_second_client_is_told_the_stream_is_busy(self):
        hex_ = self.serve()
        hex_.create({"sampleFormat": "s16le", "sampleRate": 16000})
        code, output = self.run_captions("--no-page")
        self.assertEqual(code, 2)
        self.assertIn("Ya hay otro stream", output)

    def test_the_page_serves_its_html_and_live_state(self):
        self.serve()
        page = captions.serve_page(captions.Captions(), "127.0.0.1", 0)
        self.addCleanup(page.server_close)
        self.addCleanup(page.shutdown)
        base = f"http://127.0.0.1:{page.server_address[1]}"
        html = urllib.request.urlopen(base + "/").read().decode()
        self.assertIn('EventSource("/captions")', html)
        state = json.loads(urllib.request.urlopen(base + "/state").read())
        self.assertEqual(state["status"], "conectando")
        with urllib.request.urlopen(base + "/captions", timeout=5) as events:
            first = events.readline().decode()
        self.assertTrue(first.startswith("data: {"), first)

    def test_a_silent_stream_lives_past_the_lease_while_audio_flows(self):
        # 12 s of near-silence at 4x is 3 s of wall time; the lease only
        # needs audio, so the stream must not expire. The mock's lease is
        # wall-clock, so shorten it to prove pushes renew it.
        self.serve()
        old = mock_hex.LEASE_S
        mock_hex.LEASE_S = 1.0
        self.addCleanup(setattr, mock_hex, "LEASE_S", old)
        with wave.open(str(self.wav), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(16_000)
            wav.writeframes(b"\x00\x00" * 16_000 * 12)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = captions.main(["--discovery", str(self.discovery), "--wav", str(self.wav),
                                  "--speed", "4", "--no-page"])
        self.assertEqual(code, 0, output.getvalue())
        self.assertNotIn("lease-expired", output.getvalue())


if __name__ == "__main__":
    unittest.main()
