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


def speech_like_wav(path: Path, rate: int = 48_000, channels: int = 2, phrases: int = 3,
                    phrase_s: float = 2.4, pause_s: float = 1.2) -> None:
    """Loud modulated tones separated by near-silence, as 16-bit PCM."""
    rng = np.random.default_rng(7)
    parts = [np.zeros(int(0.5 * rate))]
    for _ in range(phrases):
        t = np.arange(int(phrase_s * rate)) / rate
        tone = 0.3 * np.sin(2 * np.pi * 220 * t) * (0.6 + 0.4 * np.sin(2 * np.pi * 3 * t))
        parts += [tone, 0.0005 * rng.standard_normal(int(pause_s * rate))]
    mono = np.concatenate(parts)
    frames = np.repeat(mono[:, None], channels, axis=1)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((frames * 32767).astype("<i2").tobytes())


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
        self.assertEqual(snapshot["lines"], ["Dos.", "Tres."])
        self.assertEqual(snapshot["stable"], "cuatro")

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
