#!/usr/bin/env python3
"""MicroPrompt captions: live subtitles of your voice on Hex's `/streams`.

Phase 1 prototype of docs/plans/teleprompter.md. It records the microphone
(or plays a WAV file in real time), pushes 16 kHz s16le audio to the running
Hex app's `/streams` API (docs/streams.md), and shows the partial and final
text as subtitles:

- in a local web page (http://127.0.0.1:8765/), for a browser window, a
  second display, or an OBS Browser Source;
- in the terminal with --console;
- and, once you stop, as a transcript (--txt) and timed subtitles (--srt).

Requirements: Python 3.10+, numpy, and sounddevice for the microphone.
Hex must be running, enabled, and holding a model (Nemotron 3.5 ASR streams
Spanish on a GPU; see README.md).
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import queue
import socket
import statistics
import sys
import threading
import time
import wave
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16_000
# Hex accepts 256 KiB per audio request; 100 ms of s16le is 3.2 KB.
SEND_SAMPLES = SAMPLE_RATE // 10
FINISH_TIMEOUT_S = 130
HERE = Path(__file__).resolve().parent


# --------------------------------------------------------------------------
# Discovery and the Hex client
# --------------------------------------------------------------------------


def default_discovery_path() -> Path:
    """Where the running Hex publishes `local-api.json` (docs/streams.md)."""
    if sys.platform == "win32":
        return Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming")) / "hex" / "local-api.json"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "hex" / "local-api.json"
    data = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(data) / "hex" / "local-api.json"


class HexError(Exception):
    """A refusal from Hex, with its API code."""

    def __init__(self, status: int, code: str, body: dict | None = None):
        super().__init__(f"{status} {code}")
        self.status = status
        self.code = code
        self.body = body or {}


class HexClient:
    """The `/streams` routes over loopback HTTP. Hex answers every request
    with `Connection: close`, so each call opens its own connection."""

    def __init__(self, port: int, token: str, timeout: float = 10.0):
        self.port = port
        self.token = token
        self.timeout = timeout
        self.stream_id: int | None = None
        self.stream_token: str | None = None

    @classmethod
    def discover(cls, path: Path) -> "HexClient":
        try:
            document = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            raise SystemExit(
                f"No encuentro {path}.\n"
                "¿Está Hex abierto? Hex publica ese fichero al arrancar (o usa --discovery)."
            )
        return cls(int(document["port"]), str(document["token"]))

    def _headers(self, content_type: str | None = None, stream: bool = False) -> dict:
        headers = {"Authorization": f"Bearer {self.token}"}
        if stream and self.stream_token:
            headers["X-Hex-Stream-Token"] = self.stream_token
        if content_type:
            headers["Content-Type"] = content_type
        return headers

    def _connection(self, timeout: float | None = None) -> http.client.HTTPConnection:
        return http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout or self.timeout)

    def request(
        self,
        method: str,
        path: str,
        body: bytes = b"",
        content_type: str | None = None,
        stream: bool = False,
        timeout: float | None = None,
    ) -> tuple[int, dict | None]:
        connection = self._connection(timeout)
        try:
            # http.client always sends Content-Length for a bytes body (even
            # an empty one); Hex refuses chunked bodies.
            connection.request(method, path, body=body, headers=self._headers(content_type, stream))
            response = connection.getresponse()
            data = response.read()
        finally:
            connection.close()
        parsed = None
        if data:
            try:
                parsed = json.loads(data)
            except ValueError:
                parsed = {"code": data.decode("utf-8", "replace")}
        if response.status >= 400:
            code = (parsed or {}).get("code", "unauthorized" if response.status == 401 else "error")
            raise HexError(response.status, code, parsed)
        return response.status, parsed

    def create(self, language: str | None, model: str | None) -> dict:
        request = {"sampleFormat": "s16le", "sampleRate": SAMPLE_RATE}
        if language:
            request["language"] = language
        if model:
            request["model"] = model
        _, created = self.request("POST", "/streams", json.dumps(request).encode(), "application/json")
        self.stream_id = created["id"]
        self.stream_token = created["ownerToken"]
        return created

    def push(self, pcm: bytes) -> None:
        self.request(
            "POST", f"/streams/{self.stream_id}/audio", pcm, "application/octet-stream", stream=True
        )

    def finish(self) -> dict:
        _, final = self.request(
            "POST", f"/streams/{self.stream_id}/finish", stream=True, timeout=FINISH_TIMEOUT_S
        )
        return final or {}

    def cancel(self) -> None:
        self.request("POST", f"/streams/{self.stream_id}/cancel", stream=True)

    def events(self, stop: threading.Event):
        """Yields each event of `GET /streams/{id}/events` until the stream
        ends or the connection closes. Keep-alives arrive every 15 s."""
        connection = self._connection(timeout=60)
        try:
            connection.request("GET", f"/streams/{self.stream_id}/events", headers=self._headers(stream=True))
            response = connection.getresponse()
            if response.status != 200:
                data = response.read()
                raise HexError(response.status, data.decode("utf-8", "replace") or "error")
            yield from parse_sse(response, stop)
        finally:
            connection.close()


def parse_sse(lines, stop: threading.Event | None = None):
    """Server-sent events as Hex writes them: one JSON object per `data:`
    line, an empty line after each, and `:` comments as keep-alives."""
    data: list[str] = []
    while not (stop and stop.is_set()):
        raw = lines.readline()
        if not raw:
            return
        line = raw.decode("utf-8").rstrip("\r\n")
        if line.startswith(":"):
            continue
        if line.startswith("data:"):
            data.append(line[5:].lstrip(" "))
            continue
        if line == "" and data:
            yield json.loads("\n".join(data))
            data = []


# --------------------------------------------------------------------------
# Audio: resampling and sources
# --------------------------------------------------------------------------


class Resampler:
    """Streaming conversion of mono float audio to 16 kHz: a windowed-sinc
    low-pass below the new Nyquist, then linear interpolation, keeping the
    filter history and the fractional position across blocks."""

    def __init__(self, rate_in: int, rate_out: int = SAMPLE_RATE, taps: int = 63):
        self.rate_in = rate_in
        self.ratio = rate_in / rate_out
        self.passthrough = rate_in == rate_out
        self.filter = None
        if rate_in > rate_out:
            cutoff = 0.45 * rate_out / rate_in
            n = np.arange(taps) - (taps - 1) / 2
            kernel = 2 * cutoff * np.sinc(2 * cutoff * n) * np.hamming(taps)
            self.filter = (kernel / kernel.sum()).astype(np.float64)
            self.history = np.zeros(taps - 1)
        self.position = 0.0
        self.previous = 0.0

    def process(self, samples: np.ndarray) -> np.ndarray:
        samples = np.asarray(samples, dtype=np.float64)
        if self.passthrough or samples.size == 0:
            return samples.astype(np.float32)
        if self.filter is not None:
            padded = np.concatenate([self.history, samples])
            self.history = padded[-(self.filter.size - 1) :]
            samples = np.convolve(padded, self.filter, mode="valid")
        last = samples.size - 1
        if last < self.position:
            count = 0
        else:
            count = int((last - self.position) // self.ratio) + 1
        times = self.position + self.ratio * np.arange(count)
        # Index 0 of `buffer` is the previous block's last sample (time -1).
        buffer = np.concatenate([[self.previous], samples])
        out = np.interp(times + 1, np.arange(buffer.size), buffer)
        self.position = self.position + self.ratio * count - samples.size
        self.previous = samples[-1]
        return out.astype(np.float32)


def to_s16le(samples: np.ndarray) -> bytes:
    return (np.clip(samples, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()


class MicSource:
    """The microphone through sounddevice (PortAudio). It asks for 16 kHz
    mono and, when the device refuses, records at the device's own rate and
    resamples."""

    def __init__(self, device: str | None, out: "queue.Queue[np.ndarray]"):
        try:
            import sounddevice as sd
        except (ImportError, OSError) as error:
            raise SystemExit(
                f"No puedo usar el micrófono: {error}\n"
                "Instala sounddevice (pip install sounddevice) o usa --wav <fichero>."
            )
        self.sd = sd
        self.out = out
        self.device = int(device) if device and device.isdigit() else device
        self.dropped = 0
        self.stream = None
        self.resampler: Resampler | None = None

    def _callback(self, indata, frames, time_info, status):  # PortAudio's thread
        mono = indata.mean(axis=1) if indata.shape[1] > 1 else indata[:, 0]
        samples = self.resampler.process(mono) if self.resampler else mono.copy()
        try:
            self.out.put_nowait(samples)
        except queue.Full:
            self.dropped += samples.size

    def start(self) -> str:
        info = self.sd.query_devices(self.device, "input")
        attempts = [(SAMPLE_RATE, 1), (int(info["default_samplerate"]), 1)]
        if info["max_input_channels"] > 1:
            attempts.append((int(info["default_samplerate"]), min(2, info["max_input_channels"])))
        last_error = None
        for rate, channels in attempts:
            try:
                self.resampler = None if rate == SAMPLE_RATE else Resampler(rate)
                self.stream = self.sd.InputStream(
                    samplerate=rate,
                    channels=channels,
                    dtype="float32",
                    blocksize=rate // 50,
                    device=self.device,
                    callback=self._callback,
                )
                self.stream.start()
                return f"{info['name']} ({rate} Hz, {channels} canal{'es' if channels > 1 else ''})"
            except Exception as error:  # PortAudio raises its own error types
                last_error = error
        raise SystemExit(f"No pude abrir el micrófono «{info['name']}»: {last_error}")

    def stop(self) -> None:
        if self.stream is not None:
            self.stream.stop()
            self.stream.close()
            self.stream = None


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    """A PCM WAV file as mono float samples and its rate."""
    with wave.open(str(path), "rb") as wav:
        rate, channels, width = wav.getframerate(), wav.getnchannels(), wav.getsampwidth()
        frames = wav.readframes(wav.getnframes())
    if width == 1:
        samples = (np.frombuffer(frames, np.uint8).astype(np.float32) - 128.0) / 128.0
    elif width == 2:
        samples = np.frombuffer(frames, "<i2").astype(np.float32) / 32768.0
    elif width == 3:
        raw = np.frombuffer(frames, np.uint8).reshape(-1, 3)
        ints = (raw[:, 0].astype(np.int32) | (raw[:, 1].astype(np.int32) << 8) | (raw[:, 2].astype(np.int32) << 16))
        ints = np.where(ints & 0x800000, ints - 0x1000000, ints)
        samples = ints.astype(np.float32) / 8388608.0
    elif width == 4:
        samples = np.frombuffer(frames, "<i4").astype(np.float32) / 2147483648.0
    else:
        raise SystemExit(f"{path}: muestras de {width} bytes no soportadas")
    if channels > 1:
        samples = samples.reshape(-1, channels).mean(axis=1)
    return samples, rate


class WavSource:
    """A WAV file played into the stream at real-time pace (or `speed`
    times faster), as Hex's own HEX_AUDIO_FIXTURE does, then a second of
    silence so the last words close their window."""

    def __init__(self, path: Path, out: "queue.Queue[np.ndarray]", speed: float = 1.0):
        samples, rate = read_wav(path)
        self.samples = Resampler(rate).process(samples) if rate != SAMPLE_RATE else samples
        self.samples = np.concatenate([self.samples, np.zeros(SAMPLE_RATE, np.float32)])
        self.out = out
        self.speed = speed
        self.done = threading.Event()
        self.stopped = threading.Event()
        self.dropped = 0
        self.path = path

    def start(self) -> str:
        threading.Thread(target=self._run, name="wav-source", daemon=True).start()
        seconds = self.samples.size / SAMPLE_RATE
        return f"{self.path.name} ({seconds:.1f} s a {self.speed:g}x)"

    def _run(self) -> None:
        block = SAMPLE_RATE // 50
        started = time.monotonic()
        for offset in range(0, self.samples.size, block):
            if self.stopped.is_set():
                break
            due = started + offset / SAMPLE_RATE / self.speed
            delay = due - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            self.out.put(self.samples[offset : offset + block])
        self.done.set()

    def stop(self) -> None:
        self.stopped.set()


# --------------------------------------------------------------------------
# Captions: what the page and the console show
# --------------------------------------------------------------------------


class Captions:
    """Completed segments plus the window being transcribed, and every
    change for the page's subscribers."""

    def __init__(self, keep: int = 12):
        self.keep = keep
        self.lock = threading.Condition()
        self.version = 0
        self.segments: dict[int, dict] = {}
        self.partial: dict | None = None
        self.info: dict = {}
        self.status = "conectando"
        self.error: str | None = None

    def _changed(self) -> None:
        self.version += 1
        self.lock.notify_all()

    def set_status(self, status: str, error: str | None = None) -> None:
        with self.lock:
            self.status = status
            self.error = error
            self._changed()

    def set_info(self, info: dict) -> None:
        with self.lock:
            self.info = info
            self._changed()

    def apply(self, event: dict) -> None:
        kind = event.get("type")
        with self.lock:
            if kind == "partial":
                self.partial = event
            elif kind == "segment":
                self.segments[event["segment"]] = event
                if self.partial and self.partial.get("segment") == event["segment"]:
                    self.partial = None
            elif kind == "final":
                for segment in event.get("segments", []):
                    self.segments[segment["segment"]] = segment
                self.partial = None
                self.status = "terminado"
            elif kind == "error":
                self.status = "error"
                self.error = f"{event.get('code')}: {event.get('message', '')}"
            elif kind == "cancelled":
                self.status = "cancelado"
            else:
                return
            self._changed()

    def snapshot(self) -> dict:
        with self.lock:
            done = [self.segments[i]["text"] for i in sorted(self.segments) if self.segments[i]["text"].strip()]
            partial = self.partial
            if partial and partial.get("segment") in self.segments:
                partial = None
            return {
                "version": self.version,
                "lines": done[-self.keep :],
                "stable": (partial or {}).get("stable", ""),
                "tentative": (partial or {}).get("tentative", ""),
                "status": self.status,
                "error": self.error,
                "info": self.info,
            }

    def wait(self, version: int, timeout: float) -> dict:
        with self.lock:
            self.lock.wait_for(lambda: self.version != version, timeout)
        return self.snapshot()

    def ordered_segments(self) -> list[dict]:
        with self.lock:
            return [self.segments[i] for i in sorted(self.segments)]


# --------------------------------------------------------------------------
# Timing: how far behind the voice the text runs
# --------------------------------------------------------------------------


class Timing:
    """Audio sent against text received, in stream time (ms of audio)."""

    def __init__(self):
        self.lock = threading.Lock()
        self.sent_samples = 0
        self.first_partial_ms: float | None = None
        self.partial_times: list[float] = []
        self.segment_lags_ms: list[float] = []

    def sent(self, samples: int) -> None:
        with self.lock:
            self.sent_samples += samples

    def sent_ms(self) -> float:
        with self.lock:
            return self.sent_samples * 1000 / SAMPLE_RATE

    def event(self, event: dict) -> None:
        now = time.monotonic()
        kind = event.get("type")
        if kind == "partial" and (event.get("stable", "").strip() or event.get("tentative", "").strip()):
            if self.first_partial_ms is None:
                self.first_partial_ms = self.sent_ms()
            self.partial_times.append(now)
        elif kind == "segment" and event.get("text", "").strip():
            # The window's last audio ended at endMs; the text arrived when
            # this much audio had been sent.
            self.segment_lags_ms.append(self.sent_ms() - event["endMs"])

    def summary(self) -> list[str]:
        lines = []
        if self.first_partial_ms is not None:
            lines.append(f"primeras palabras: a los {self.first_partial_ms / 1000:.2f} s de audio enviado")
        else:
            lines.append("primeras palabras: no llegó ningún parcial")
        if len(self.partial_times) > 1:
            gaps = [b - a for a, b in zip(self.partial_times, self.partial_times[1:])]
            lines.append(
                f"actualizaciones parciales: {len(self.partial_times)}, "
                f"cada {statistics.median(gaps):.2f} s (mediana)"
            )
        if self.segment_lags_ms:
            lags = sorted(self.segment_lags_ms)
            p90 = lags[min(len(lags) - 1, int(round(0.9 * (len(lags) - 1))))]
            lines.append(
                f"texto final de cada frase: {statistics.median(lags) / 1000:.2f} s tras acabar su audio "
                f"(mediana; p90 {p90 / 1000:.2f} s; {len(lags)} frases)"
            )
        return lines


# --------------------------------------------------------------------------
# The local page
# --------------------------------------------------------------------------


def serve_page(captions: Captions, host: str, port: int) -> ThreadingHTTPServer:
    page = (HERE / "captions.html").read_bytes()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):  # quiet
            pass

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            if path in ("/", "/index.html"):
                self._send(200, "text/html; charset=utf-8", page)
            elif path == "/state":
                self._send(200, "application/json", json.dumps(captions.snapshot()).encode())
            elif path == "/captions":
                self._events()
            else:
                self._send(404, "text/plain", b"not found")

        def _send(self, status: int, content_type: str, body: bytes) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _events(self) -> None:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            snapshot = captions.snapshot()
            try:
                while True:
                    self.wfile.write(b"data: " + json.dumps(snapshot).encode() + b"\n\n")
                    self.wfile.flush()
                    version = snapshot["version"]
                    snapshot = captions.wait(version, timeout=15)
                    if snapshot["version"] == version:
                        self.wfile.write(b": keep-alive\n\n")
                        self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError, socket.timeout, OSError):
                return

    server = ThreadingHTTPServer((host, port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, name="page", daemon=True).start()
    return server


# --------------------------------------------------------------------------
# The console view
# --------------------------------------------------------------------------


class Console:
    """One live line that rewrites itself, and each finished phrase on its
    own line above it."""

    def __init__(self, enabled: bool):
        self.enabled = enabled
        self.color = sys.stdout.isatty()
        if self.color and sys.platform == "win32":
            os.system("")  # turns on ANSI escapes in the Windows console
        self.width = 100
        try:
            self.width = max(40, os.get_terminal_size().columns - 1)
        except OSError:
            pass
        self.lock = threading.Lock()

    def event(self, event: dict) -> None:
        if not self.enabled:
            return
        kind = event.get("type")
        with self.lock:
            if kind == "partial":
                if not self.color:
                    return  # no live line outside a terminal; whole phrases only
                stable, tentative = event.get("stable", ""), event.get("tentative", "")
                line = (stable + tentative)[-self.width :]
                cut = max(0, len(line) - len(tentative))
                if self.color:
                    line = line[:cut] + "\x1b[2m" + line[cut:] + "\x1b[0m"
                self._live(line)
            elif kind == "segment" and event.get("text", "").strip():
                self._live("")
                print(f"[{event['startMs'] / 1000:6.1f}s] {event['text']}", flush=True)

    def _live(self, line: str) -> None:
        if self.color:
            sys.stdout.write("\r\x1b[2K" + line)
            sys.stdout.flush()

    def note(self, text: str) -> None:
        with self.lock:
            if self.enabled:
                self._live("")
            print(text, flush=True)


# --------------------------------------------------------------------------
# Output files
# --------------------------------------------------------------------------


def srt_time(ms: float) -> str:
    ms = max(0, int(ms))
    hours, ms = divmod(ms, 3_600_000)
    minutes, ms = divmod(ms, 60_000)
    seconds, ms = divmod(ms, 1000)
    return f"{hours:02}:{minutes:02}:{seconds:02},{ms:03}"


def write_srt(path: Path, segments: list[dict]) -> int:
    blocks = []
    for segment in segments:
        text = segment.get("text", "").strip()
        if not text:
            continue
        blocks.append(
            f"{len(blocks) + 1}\n{srt_time(segment['startMs'])} --> {srt_time(segment['endMs'])}\n{text}\n"
        )
    path.write_text("\n".join(blocks), encoding="utf-8")
    return len(blocks)


# --------------------------------------------------------------------------
# The session
# --------------------------------------------------------------------------


EXPLANATIONS = {
    "model-not-loaded": "Hex no tiene un modelo cargado. Activa Hex (Enabled) y espera a que el modelo esté listo; "
    "si hace falta, haz un dictado corto.",
    "engine-unavailable": "El motor de Hex no está disponible todavía. Activa Hex (Enabled) y espera a que cargue.",
    "stream-busy": "Ya hay otro stream abierto en Hex. Cierra el otro cliente o espera 10 s a que caduque.",
    "unauthorized": "Hex rechazó el token: local-api.json es de otra ejecución. Reinicia este script.",
}


class Session:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.audio: "queue.Queue[np.ndarray]" = queue.Queue(maxsize=500)
        self.captions = Captions()
        self.timing = Timing()
        self.console = Console(args.console)
        self.stop_capture = threading.Event()
        self.stop_events = threading.Event()
        self.ended = threading.Event()
        self.final: dict | None = None
        self.failure: str | None = None
        self.client = HexClient.discover(Path(args.discovery) if args.discovery else default_discovery_path())

    def run(self) -> int:
        args = self.args
        if not args.no_page:
            server = serve_page(self.captions, args.host, args.port)
            url = f"http://{args.host}:{server.server_address[1]}/"
            self.console.note(f"Subtítulos en {url}  (opciones en README.md: ?size=72&lines=2 …)")
            if args.open:
                webbrowser.open(url)

        try:
            created = self.client.create(args.language, args.model)
        except ConnectionRefusedError:
            self.console.note("Hex no responde en el puerto de local-api.json. ¿Está abierto?")
            return 2
        except HexError as error:
            if error.code == "model-mismatch":
                self.console.note(
                    f"Hex tiene cargado {error.body.get('model')} en «{error.body.get('language')}», "
                    "distinto de lo pedido con --model/--language. Cámbialo en Hex o quita esas opciones."
                )
            else:
                self.console.note(f"Hex rechazó el stream ({error}). {EXPLANATIONS.get(error.code, '')}")
            return 2

        info = {
            "model": created.get("model"),
            "language": created.get("language"),
            "partials": created.get("partials"),
        }
        self.captions.set_info(info)
        self.console.note(
            f"Stream {created['id']}: modelo {info['model']}, idioma {info['language']}, "
            f"parciales {'sí' if info['partials'] else 'NO'}"
        )
        if not info["partials"]:
            self.console.note(
                "  Sin parciales: cada frase aparecerá entera tras la pausa. Para texto palabra a palabra "
                "elige Nemotron 3.5 ASR (o Parakeet Unified English) y comprueba que Hex usa la GPU."
            )

        events = threading.Thread(target=self._events, name="events", daemon=True)
        events.start()

        if args.wav:
            source = WavSource(Path(args.wav), self.audio, args.speed)
        else:
            source = MicSource(args.device, self.audio)
        self.console.note(f"Audio: {source.start()}. Habla; Ctrl+C para terminar.")
        self.captions.set_status("en vivo")

        sender = threading.Thread(target=self._send, name="sender", daemon=True)
        sender.start()
        try:
            while sender.is_alive():
                if isinstance(source, WavSource) and source.done.is_set() and self.audio.empty():
                    break
                sender.join(0.2)
        except KeyboardInterrupt:
            pass
        source.stop()
        self.stop_capture.set()
        sender.join(5)
        if source.dropped:
            self.console.note(f"Aviso: se descartaron {source.dropped / SAMPLE_RATE:.1f} s de audio (cola llena).")
        return self._finish(events)

    def _send(self) -> None:
        pending: list[np.ndarray] = []
        count = 0
        while True:
            try:
                block = self.audio.get(timeout=0.05)
                pending.append(block)
                count += block.size
            except queue.Empty:
                if self.stop_capture.is_set():
                    break
            if count >= SEND_SAMPLES or (self.stop_capture.is_set() and count):
                samples = np.concatenate(pending)
                pending, count = [], 0
                if not self._push(samples):
                    return
            if self.stop_capture.is_set() and self.audio.empty() and not count:
                break

    def _push(self, samples: np.ndarray) -> bool:
        pcm = to_s16le(samples)
        for attempt in range(20):
            try:
                self.client.push(pcm)
                self.timing.sent(samples.size)
                return True
            except HexError as error:
                if error.code == "stream-backlog-full" and attempt < 19:
                    time.sleep(0.25)  # Hex is behind (a long dictation); retry
                    continue
                if error.code in ("stream-ended", "stream-finishing", "stream-not-found"):
                    self.failure = self.failure or f"el stream terminó ({error.code})"
                    self.stop_capture.set()
                    return False
                self.console.note(f"Error enviando audio: {error}")
                return False
            except OSError as error:
                self.console.note(f"Hex dejó de responder: {error}")
                self.failure = str(error)
                self.stop_capture.set()
                return False
        return False

    def _events(self) -> None:
        try:
            for event in self.client.events(self.stop_events):
                self.timing.event(event)
                self.captions.apply(event)
                self.console.event(event)
                kind = event.get("type")
                if kind == "final":
                    self.final = event
                    break
                if kind in ("error", "cancelled"):
                    self.failure = f"{event.get('code', kind)} {event.get('message', '')}".strip()
                    self.stop_capture.set()
                    break
        except (HexError, OSError) as error:
            if not self.stop_events.is_set():
                self.failure = f"se cortaron los eventos: {error}"
                self.stop_capture.set()
        finally:
            self.ended.set()

    def _finish(self, events: threading.Thread) -> int:
        args = self.args
        if not self.ended.is_set():
            self.console.note("Terminando: Hex transcribe lo que queda…")
            try:
                final = self.client.finish()
                self.final = self.final or final
            except KeyboardInterrupt:
                self.client.cancel()
                self.failure = "cancelado"
            except (HexError, OSError) as error:
                self.failure = self.failure or f"finish falló: {error}"
            self.ended.wait(5)
        self.stop_events.set()
        self.captions.set_status("terminado" if self.final else "error", self.failure)

        segments = (self.final or {}).get("segments") or self.captions.ordered_segments()
        text = (self.final or {}).get("text") or " ".join(s["text"] for s in segments)
        self.console.note("")
        if self.failure and not self.final:
            self.console.note(f"El stream acabó con error: {self.failure}")
        self.console.note("— Transcripción —")
        self.console.note(text.strip() or "(vacía)")
        self.console.note("— Latencia —")
        for line in self.timing.summary():
            self.console.note("  " + line)
        if args.txt:
            Path(args.txt).write_text(text.strip() + "\n", encoding="utf-8")
            self.console.note(f"Guardado {args.txt}")
        if args.srt:
            written = write_srt(Path(args.srt), segments)
            self.console.note(f"Guardado {args.srt} ({written} subtítulos)")
        if args.linger and not args.no_page:
            self.console.note(f"La página sigue abierta {args.linger} s más.")
            time.sleep(args.linger)
        return 0 if self.final else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Subtítulos en vivo de tu voz sobre /streams de Hex.")
    parser.add_argument("--language", help="idioma que debe tener cargado Hex (p. ej. es); si no, el que tenga")
    parser.add_argument("--model", help="modelo que debe tener cargado Hex (p. ej. nemotron_3_5_asr)")
    parser.add_argument("--device", help="micrófono: índice o parte del nombre (ver --list-devices)")
    parser.add_argument("--list-devices", action="store_true", help="lista los micrófonos y sale")
    parser.add_argument("--wav", help="usa un WAV en lugar del micrófono (pruebas repetibles)")
    parser.add_argument("--speed", type=float, default=1.0, help="velocidad del WAV (1 = tiempo real)")
    parser.add_argument("--console", action="store_true", help="muestra también los subtítulos en la terminal")
    parser.add_argument("--no-page", action="store_true", help="no sirve la página de subtítulos")
    parser.add_argument("--host", default="127.0.0.1", help="dirección de la página (por defecto 127.0.0.1)")
    parser.add_argument("--port", type=int, default=8765, help="puerto de la página (por defecto 8765)")
    parser.add_argument("--open", action="store_true", help="abre la página en el navegador")
    parser.add_argument("--txt", help="guarda la transcripción final en este fichero")
    parser.add_argument("--srt", help="guarda los subtítulos con tiempos en este fichero .srt")
    parser.add_argument("--discovery", help="ruta de local-api.json (por defecto la de Hex)")
    parser.add_argument("--linger", type=float, default=0, help="segundos que la página sigue viva al terminar")
    args = parser.parse_args(argv)

    if args.list_devices:
        try:
            import sounddevice as sd
        except (ImportError, OSError) as error:
            raise SystemExit(f"sounddevice no está disponible: {error}")
        print(sd.query_devices())
        return 0
    return Session(args).run()


if __name__ == "__main__":
    sys.exit(main())
