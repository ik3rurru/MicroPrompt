#!/usr/bin/env python3
"""A stand-in for the running Hex's `/streams` API (docs/streams.md), so
captions.py and its page can be tried and tested without Hex or a model.

It speaks the same HTTP contract (discovery file, bearer token, stream
token, `Connection: close`, SSE with keep-alives, the error codes, the
10 s lease, one live stream) but "transcribes" by energy: every 0.4 s of
sound becomes the next word of a canned Spanish text, and a pause of
0.6 s (or 25 s of audio) closes the window as a segment.

    python mock_hex.py --discovery mock-local-api.json
    python captions.py --discovery mock-local-api.json --open
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16_000
FRAME = SAMPLE_RATE // 50
LEASE_S = 10.0
MAX_AUDIO_BYTES = 256 * 1024
MAX_BACKLOG = 120 * SAMPLE_RATE
WORD_SAMPLES = int(0.4 * SAMPLE_RATE)
PAUSE_SAMPLES = int(0.6 * SAMPLE_RATE)
MAX_WINDOW = 25 * SAMPLE_RATE
SPEECH_DBFS = -40.0

TEXT = (
    "hola a todos y bienvenidos a esta prueba de subtítulos en vivo "
    "hoy vamos a ver cómo el teleprompter escucha lo que digo y lo escribe "
    "en pantalla casi al instante mientras sigo hablando con normalidad "
    "si hago una pausa la frase se cierra y empieza una línea nueva"
).split()


class MockStream:
    def __init__(self, stream_id: int, owner: str, sample_format: str, partials: bool, model: str, language: str):
        self.id = stream_id
        self.owner = owner
        self.format = sample_format
        self.partials = partials
        self.model = model
        self.language = language
        self.cond = threading.Condition()
        self.events: list[dict] = []
        self.ended = False
        self.finishing = False
        self.received = 0
        self.lease_deadline = time.monotonic() + LEASE_S
        self.subscriber = 0
        # The window being "transcribed".
        self.segment = 0
        self.window_start = 0
        self.words: list[str] = []
        self.speech = 0
        self.quiet = 0
        self.next_word = 0
        self.segments: list[dict] = []

    # Called with the lock held.
    def _emit(self, event: dict) -> None:
        self.events.append(event)
        self.cond.notify_all()

    def _close_window(self, end: int) -> None:
        if self.words:
            text = " ".join(self.words)
            text = text[0].upper() + text[1:] + "."
            segment = {"segment": self.segment, "startMs": self.window_start * 1000 // SAMPLE_RATE,
                       "endMs": end * 1000 // SAMPLE_RATE, "text": text}
            self.segments.append(segment)
            self._emit({"type": "segment", **segment})
            self.segment += 1
        self.words = []
        self.speech = 0
        self.window_start = end

    def push(self, samples: np.ndarray) -> str | None:
        with self.cond:
            if self.ended:
                return "stream-ended"
            if self.finishing:
                return "stream-finishing"
            self.lease_deadline = time.monotonic() + LEASE_S
            for offset in range(0, samples.size, FRAME):
                frame = samples[offset : offset + FRAME]
                rms = float(np.sqrt(np.mean(np.square(frame)))) if frame.size else 0.0
                loud = 20 * np.log10(max(rms, 1e-9)) > SPEECH_DBFS
                self.received += frame.size
                if loud:
                    self.quiet = 0
                    self.speech += frame.size
                    if self.speech >= WORD_SAMPLES * (len(self.words) + 1):
                        self.words.append(TEXT[self.next_word % len(TEXT)])
                        self.next_word += 1
                        if self.partials:
                            self._emit({"type": "partial", "segment": self.segment,
                                        "stable": " ".join(self.words[:-1]),
                                        "tentative": (" " if len(self.words) > 1 else "") + self.words[-1]})
                else:
                    self.quiet += frame.size
                    if self.words and self.quiet >= PAUSE_SAMPLES:
                        self._close_window(self.received - self.quiet // 2)
                    elif not self.words:
                        self.window_start = self.received
                if self.received - self.window_start >= MAX_WINDOW:
                    self._close_window(self.received)
        return None

    def finish(self) -> dict:
        with self.cond:
            if not self.ended:
                self.finishing = True
                self._close_window(self.received)
                final = {"text": " ".join(s["text"] for s in self.segments), "segments": list(self.segments),
                         "durationMs": self.received * 1000 // SAMPLE_RATE}
                self._emit({"type": "final", **final})
                self.ended = True
            for event in reversed(self.events):
                if event["type"] in ("final", "error", "cancelled"):
                    return event
        return {}

    def end(self, event: dict) -> None:
        with self.cond:
            if not self.ended:
                self._emit(event)
                self.ended = True


class MockHex:
    def __init__(self, partials: bool, model: str, language: str, loaded: bool = True):
        self.token = "hex_" + secrets.token_hex(16)
        self.partials = partials
        self.model = model
        self.language = language
        self.loaded = loaded
        self.lock = threading.Lock()
        self.current: MockStream | None = None
        self.next_id = 0
        threading.Thread(target=self._leases, daemon=True).start()

    def _leases(self) -> None:
        while True:
            time.sleep(0.25)
            stream = self.current
            if stream and not stream.ended and not stream.finishing and time.monotonic() > stream.lease_deadline:
                stream.end({"type": "error", "code": "lease-expired", "message": "no audio or heartbeat for 10 s"})

    def create(self, body: dict) -> tuple[int, dict]:
        if body.get("sampleRate") != SAMPLE_RATE:
            return 400, {"code": "unsupported-sample-rate"}
        if body.get("sampleFormat") not in ("s16le", "f32le"):
            return 400, {"code": "unsupported-sample-format"}
        if not self.loaded:
            return 503, {"code": "model-not-loaded"}
        if (body.get("model") and body["model"] != self.model) or (
            body.get("language") and body["language"] != self.language
        ):
            return 409, {"code": "model-mismatch", "model": self.model, "language": self.language}
        with self.lock:
            if self.current and not self.current.ended:
                return 409, {"code": "stream-busy"}
            self.next_id += 1
            owner = "hex_" + secrets.token_hex(16)
            self.current = MockStream(self.next_id, owner, body["sampleFormat"], self.partials, self.model, self.language)
        return 201, {"id": self.next_id, "ownerToken": owner, "model": self.model, "language": self.language,
                     "sampleRate": SAMPLE_RATE, "sampleFormat": body["sampleFormat"], "partials": self.partials,
                     "leaseMs": int(LEASE_S * 1000)}

    def stream(self, stream_id: int, owner: str | None) -> MockStream | None:
        stream = self.current
        if stream and stream.id == stream_id and owner and secrets.compare_digest(owner, stream.owner):
            return stream
        return None


def handler(hex_: MockHex):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def _reply(self, status: int, body: dict | None = None) -> None:
            data = json.dumps(body).encode() if body is not None else b""
            self.send_response(status)
            if data:
                self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(data)
            self.close_connection = True

        def _authorized(self) -> bool:
            if self.headers.get("Authorization") != f"Bearer {hex_.token}":
                self._reply(401)
                return False
            if self.headers.get("Transfer-Encoding"):
                self._reply(400, {"code": "invalid-request"})
                return False
            return True

        def _body(self, limit: int) -> bytes | None:
            length = self.headers.get("Content-Length")
            if length is None:
                self._reply(411, {"code": "length-required"})
                return None
            if int(length) > limit:
                self._reply(413, {"code": "audio-too-large" if limit == MAX_AUDIO_BYTES else "request-too-large"})
                return None
            return self.rfile.read(int(length))

        def _route(self) -> tuple[MockStream, str] | None:
            parts = self.path.strip("/").split("/")
            if len(parts) != 3 or parts[0] != "streams" or not parts[1].isdigit():
                self._reply(404, {"code": "not_found"})
                return None
            stream = hex_.stream(int(parts[1]), self.headers.get("X-Hex-Stream-Token"))
            if stream is None:
                self._reply(404, {"code": "stream-not-found"})
                return None
            return stream, parts[2]

        def do_POST(self):
            if not self._authorized():
                return
            if self.path == "/streams":
                body = self._body(1024)
                if body is None:
                    return
                try:
                    request = json.loads(body)
                except ValueError:
                    return self._reply(400, {"code": "invalid-request"})
                return self._reply(*hex_.create(request))
            routed = self._route()
            if routed is None:
                return
            stream, action = routed
            if action == "audio":
                if self.headers.get("Content-Type") != "application/octet-stream":
                    return self._reply(415, {"code": "unsupported-content-type"})
                body = self._body(MAX_AUDIO_BYTES)
                if body is None:
                    return
                width = 2 if stream.format == "s16le" else 4
                if len(body) % width:
                    return self._reply(400, {"code": "invalid-audio"})
                if stream.format == "s16le":
                    samples = np.frombuffer(body, "<i2").astype(np.float32) / 32768.0
                else:
                    samples = np.frombuffer(body, "<f4")
                    if not np.all(np.isfinite(samples)):
                        return self._reply(400, {"code": "invalid-audio"})
                refusal = stream.push(samples)
                return self._reply(409, {"code": refusal}) if refusal else self._reply(204)
            if action == "finish":
                self._body(1024)
                end = stream.finish()
                if end.get("type") == "final":
                    return self._reply(200, {k: end[k] for k in ("text", "segments", "durationMs")})
                if end.get("type") == "cancelled":
                    return self._reply(409, {"code": "stream-cancelled"})
                return self._reply(409, {"code": end.get("code"), "message": end.get("message")})
            if action == "cancel":
                self._body(1024)
                stream.end({"type": "cancelled"})
                return self._reply(204)
            if action == "heartbeat":
                self._body(1024)
                with stream.cond:
                    if stream.ended:
                        return self._reply(409, {"code": "stream-ended"})
                    stream.lease_deadline = time.monotonic() + LEASE_S
                return self._reply(204)
            return self._reply(404, {"code": "not_found"})

        def do_GET(self):
            if not self._authorized():
                return
            if self.path in ("/health", "/capabilities"):
                return self._reply(200, {"streams": True})
            routed = self._route()
            if routed is None:
                return
            stream, action = routed
            if action != "events":
                return self._reply(404, {"code": "not_found"})
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True

            def send(event: dict) -> None:
                self.wfile.write(b"data: " + json.dumps(event).encode() + b"\n\n")
                self.wfile.flush()

            try:
                send({"type": "ready", "id": stream.id, "model": stream.model, "language": stream.language,
                      "sampleFormat": stream.format, "partials": stream.partials})
                with stream.cond:
                    stream.subscriber += 1
                    me = stream.subscriber
                    stream.cond.notify_all()
                sent = 0
                while True:
                    with stream.cond:
                        if stream.subscriber != me:
                            return
                        if sent == len(stream.events):
                            stream.cond.wait(15)
                        pending = stream.events[sent:]
                        sent = len(stream.events)
                        # Each subscriber gets every segment from the start, but
                        # only the latest partial.
                    if not pending:
                        self.wfile.write(b": keep-alive\n\n")
                        self.wfile.flush()
                        continue
                    for index, event in enumerate(pending):
                        later = pending[index + 1 :]
                        if event["type"] == "partial" and any(
                            e["type"] in ("partial", "segment") and e["segment"] == event["segment"] for e in later
                        ):
                            continue  # superseded within this batch
                        send(event)
                        if event["type"] in ("final", "error", "cancelled"):
                            return
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError, OSError):
                return

    return Handler


def start(discovery: Path, port: int = 0, partials: bool = True, model: str = "nemotron_3_5_asr",
          language: str = "es", loaded: bool = True) -> tuple[ThreadingHTTPServer, MockHex]:
    hex_ = MockHex(partials, model, language, loaded)
    server = ThreadingHTTPServer(("127.0.0.1", port), handler(hex_))
    server.daemon_threads = True
    discovery.write_text(json.dumps({"port": server.server_address[1], "token": hex_.token,
                                     "apiVersion": "1", "pid": os.getpid()}), encoding="utf-8")
    threading.Thread(target=server.serve_forever, name="mock-hex", daemon=True).start()
    return server, hex_


def main() -> None:
    parser = argparse.ArgumentParser(description="Un Hex falso con /streams, para probar captions.py.")
    parser.add_argument("--discovery", default="mock-local-api.json", help="dónde escribir local-api.json")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--no-partials", action="store_true", help="como un modelo que no hace streaming")
    parser.add_argument("--model", default="nemotron_3_5_asr")
    parser.add_argument("--language", default="es")
    args = parser.parse_args()
    server, _ = start(Path(args.discovery), args.port, not args.no_partials, args.model, args.language)
    print(f"Hex falso en 127.0.0.1:{server.server_address[1]}; discovery en {args.discovery}. Ctrl+C para salir.")
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
