"""Apuntador de entrevistas (prototipo EN-01).

Escucha al entrevistador a través de `/streams` de Hex (docs/streams.md),
detecta cuándo acaba una pregunta y muestra una tarjeta con 3 puntos clave:
los tuyos si la pregunta está en tu guion, o los que propone un modelo local
de Ollama a partir del guion, el CV, la oferta y las notas de la empresa.

    py apuntador.py --wav ejemplo/entrevista.wav --model qwen3.5:4b --open
    py apuntador.py --replay resultados/segmentos.jsonl --model gemma4:12b

Sólo la biblioteca estándar. Página: http://127.0.0.1:8766/
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import re
import socket
import statistics
import subprocess
import sys
import threading
import time
import unicodedata
import wave
import webbrowser
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
RATE = 16_000
CHUNK = RATE // 10  # 100 ms per audio request

# --------------------------------------------------------------------------
# The guion
# --------------------------------------------------------------------------

STOPWORDS = set(
    """a al algo como con cual cuales de del donde el ella en era es esa ese eso esta este esto
    fue ha has hay la las le les lo los me mi mis muy nos nosotros nuestra nuestro o os para pero
    por porque que quien se si sin sobre su sus te ti tu tus un una uno unos y ya yo vosotros
    hablame cuentame dime explicame describeme comentame quieres quiero querrias gustaria
    puedes podrias haces hace es son estas esta seria eres vale bueno pues entonces voy vas va van""".split()
)
SUFFIXES = ("aciones", "acion", "amente", "mente", "ando", "iendo", "ados", "adas", "ado", "ada",
            "ar", "er", "ir", "os", "as", "es", "o", "a", "e", "s")


def fold(text: str) -> str:
    """Lower case without accents (ñ becomes n)."""
    text = unicodedata.normalize("NFD", text.lower())
    return "".join(c for c in text if unicodedata.category(c) != "Mn")


def stems(text: str) -> set[str]:
    """Content words of `text`, crudely stemmed so "cobrar" meets "cobro"."""
    out = set()
    for word in re.findall(r"[a-z0-9]+", fold(text)):
        if word in STOPWORDS or len(word) < 3:
            continue
        for suffix in SUFFIXES:
            if word.endswith(suffix) and len(word) - len(suffix) >= 3:
                word = word[: -len(suffix)]
                break
        out.add(word[:5])
    return out


@dataclass
class Prepared:
    """One of "Preguntas previstas": its phrasings and the user's points."""

    name: str
    phrasings: list[str]
    points: list[str] = field(default_factory=list)


def parse_guion(text: str) -> dict:
    """The four sections of plantilla-guion.md. "Preguntas que haré" also
    answers the interviewer's closing "¿tienes alguna pregunta?"."""
    text = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    prepared: list[Prepared] = []
    ask: list[str] = []
    section = ""
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("## "):
            section = fold(line[3:]).strip()
            continue
        if section.startswith("preguntas previstas"):
            if line.startswith("### "):
                prepared.append(Prepared(line[4:].strip(), [line[4:].strip()]))
            elif prepared and fold(line).startswith("tambien:"):
                prepared[-1].phrasings += [p.strip() for p in line.split(":", 1)[1].split("/") if p.strip()]
            elif prepared and line.startswith("- ") and line[2:].strip():
                prepared[-1].points.append(line[2:].strip())
        elif section.startswith("preguntas que hare") and line.startswith("- ") and line[2:].strip():
            ask.append(line[2:].strip())
    if ask:
        prepared.append(Prepared("Preguntas que haré", [
            # Every phrasing needs "pregunta": "¿Tienes alguna experiencia…?" is no closing.
            "alguna pregunta para nosotros", "alguna pregunta que quieras hacernos"], ask[:3]))
    return {"prepared": prepared, "ask": ask}


def match_prepared(question: str, prepared: list[Prepared], threshold: float = 0.6) -> tuple[Prepared | None, float]:
    """The prepared question whose phrasing's content words best appear in
    `question`: the share of a phrasing's stems found, at least two of them
    (a phrasing with a single content word never matches on its own). Below
    `threshold`, no match."""
    words = stems(question)
    best, best_score = None, 0.0
    for entry in prepared:
        for phrasing in entry.phrasings:
            wanted = stems(phrasing)
            if not wanted:
                continue
            common = len(wanted & words)
            score = common / len(wanted)
            if common >= 2 and score > best_score:
                best, best_score = entry, score
    return (best, best_score) if best_score >= threshold else (None, best_score)


# --------------------------------------------------------------------------
# When a question has ended
# --------------------------------------------------------------------------

# How a clause opens once filler ("y", "bueno", "me") is skipped. Nemotron
# often drops the question marks, so these matter.
REQUESTS = set("""cuentame cuentanos hablame hablanos explicame describeme dime comentame imagina ponme
    gustaria queria quisiera""".split())
# Second-person verbs: a question when the clause has some substance
# ("Tienes alguna pregunta…", not "Tienes razón").
YOU_VERBS = set("""haces cuentas dices explicas describes puedes podrias sabes tienes has conoces quieres
    crees piensas opinas ves consideras prefieres gestionas manejas valoras dirias harias""".split())
# Interrogatives: only in a sentence's last clause ("Como te decía, somos
# seis" is no question) and with substance ("Que vaya bien" neither).
# "cuando" is left out: without "?" it is nearly always "when".
ASKING = set("""que como cual cuales donde cuanto cuanta cuantos cuantas quien quienes""".split())
NOT_CONDITIONAL = {"varias", "diarias", "necesarias", "historias", "memorias", "primarias"}
FILLER = {"y", "e", "bueno", "vale", "pues", "entonces", "ahora", "me", "nos", "te", "a", "ver"}


def is_question(text: str) -> bool:
    """A question mark, or a clause that opens like a question or a request
    ("Cuéntame…", "Para empezar, me haces…", "Cómo diseñarías…")."""
    if "?" in text or "¿" in text:
        return True
    for sentence in re.split(r"[.!:;]\s*", fold(text)):
        clauses = sentence.split(",")
        for index, clause in enumerate(clauses):
            words = re.findall(r"[a-z]+", clause)
            while words and words[0] in FILLER:
                words.pop(0)
            if not words:
                continue
            first, last = words[0], index == len(clauses) - 1
            # Conditionals: diseñarías, harías, podrías, sabrías; not historias.
            conditional = re.search(r"[aeidb]rias$", first) and first not in NOT_CONDITIONAL
            if first in REQUESTS or ((first in YOU_VERBS or conditional) and len(words) >= 3):
                return True
            if last and len(words) >= 4 and (first in ASKING or words[:2] == ["por", "que"]):
                return True
    return False


class Turns:
    """Groups the interviewer's segments into turns and says when a turn
    has become a question. Hex's windows tile the audio, so a turn is told
    by when its segments arrive: one more than `gap_ms` after the last starts
    a new turn (the candidate answered meanwhile). A turn that keeps going
    after a question fires again with the longer text."""

    def __init__(self, gap_ms: int = 5000):
        self.gap_ms = gap_ms
        self.texts: list[str] = []
        self.last_ms = -10**9

    def add(self, text: str, at_ms: int) -> str | None:
        text = text.strip()
        if not text:
            return None
        if at_ms - self.last_ms > self.gap_ms:
            self.texts = []
        self.texts.append(text)
        self.last_ms = at_ms
        return " ".join(self.texts) if any(is_question(t) for t in self.texts) else None


# --------------------------------------------------------------------------
# Hex /streams (docs/streams.md)
# --------------------------------------------------------------------------


class Hex:
    def __init__(self, discovery: Path):
        doc = json.loads(discovery.read_text(encoding="utf-8"))
        self.port, self.token = int(doc["port"]), str(doc["token"])
        self.id = self.owner = None

    def call(self, method: str, path: str, body: bytes = b"", ctype: str | None = None, timeout: float = 10):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        headers = {"Authorization": f"Bearer {self.token}"}
        if self.owner:
            headers["X-Hex-Stream-Token"] = self.owner
        if ctype:
            headers["Content-Type"] = ctype
        try:
            conn.request(method, path, body=body, headers=headers)
            response = conn.getresponse()
            data = response.read()
        finally:
            conn.close()
        if response.status >= 400:
            raise SystemExit(f"Hex respondió {response.status} a {path}: {data.decode('utf-8', 'replace')}")
        return json.loads(data) if data else None

    def create(self, language: str | None) -> dict:
        request = {"sampleFormat": "s16le", "sampleRate": RATE}
        if language:
            request["language"] = language
        created = self.call("POST", "/streams", json.dumps(request).encode(), "application/json")
        self.id, self.owner = created["id"], created["ownerToken"]
        return created

    def events(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=120)
        conn.request("GET", f"/streams/{self.id}/events",
                     headers={"Authorization": f"Bearer {self.token}", "X-Hex-Stream-Token": self.owner})
        response = conn.getresponse()
        if response.status != 200:
            raise SystemExit(f"Hex respondió {response.status} a los eventos: {response.read().decode('utf-8', 'replace')}")
        for raw in response:
            line = raw.decode("utf-8").strip()
            if line.startswith("data:"):
                event = json.loads(line[5:])
                yield event
                if event["type"] in ("final", "error", "cancelled"):
                    return


def read_wav(path: Path) -> bytes:
    with wave.open(str(path), "rb") as w:
        if (w.getframerate(), w.getnchannels(), w.getsampwidth()) != (RATE, 1, 2):
            raise SystemExit(f"{path}: hace falta un WAV de 16 kHz, mono, 16 bits (make_wav.py lo genera así)")
        return w.readframes(w.getnframes())


# --------------------------------------------------------------------------
# Ollama
# --------------------------------------------------------------------------

INSTRUCTIONS = """Eres el apuntador del candidato en una entrevista de trabajo en español.
Cuando el entrevistador pregunta algo, das EXACTAMENTE 3 puntos clave para que el candidato los diga con sus palabras.

Reglas:
- Una línea por punto, empezando por "- ". Máximo 8 palabras por punto: ideas, no frases.
- Nada más: sin título, sin introducción, sin cierre.
- Usa los datos del guion, el CV, la oferta y las notas: cifras, proyectos, nombres.
- Si la pregunta corresponde a una de "Preguntas previstas" del guion, usa esos puntos del candidato.
- No inventes experiencia que el candidato no tiene.
- Si añades algo que no está en estos documentos (conocimiento general), termina ese punto con "(sugerencia)".
"""


def build_prefix(files: dict[str, str]) -> str:
    """The fixed system prompt: same bytes for every question, so Ollama
    keeps its evaluation cached and only reads the new question."""
    parts = [INSTRUCTIONS]
    for name, text in files.items():
        parts.append(f"=== {name.upper()} ===\n{text.strip()}\n")
    return "\n".join(parts)


class Ollama:
    def __init__(self, model: str, prefix: str, host: str = "127.0.0.1", port: int = 11434):
        self.model, self.prefix, self.host, self.port = model, prefix, host, port
        # Spanish runs ~3 characters per token here; room for the question and answer.
        self.num_ctx = max(2048, (len(prefix) // 3 + 600 + 1023) // 1024 * 1024)
        self.think: bool | None = False

    def _post(self, path: str, body: dict, timeout: float = 300):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=timeout)
        conn.request("POST", path, json.dumps(body).encode(), {"Content-Type": "application/json"})
        return conn, conn.getresponse()

    def request(self, question: str, num_predict: int = 120) -> dict:
        body = {
            "model": self.model, "stream": True, "keep_alive": "30m",
            "messages": [{"role": "system", "content": self.prefix},
                         {"role": "user", "content": f"Pregunta del entrevistador: «{question}»"}],
            "options": {"num_ctx": self.num_ctx, "temperature": 0.2, "num_predict": num_predict},
        }
        if self.think is not None:
            body["think"] = self.think
        return body

    def unload_all(self) -> None:
        """Frees the GPU of every Ollama model, so a cold load is measured alone."""
        conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
        conn.request("GET", "/api/ps")
        models = json.loads(conn.getresponse().read()).get("models", [])
        conn.close()
        for model in models:
            conn, response = self._post("/api/generate", {"model": model["name"], "keep_alive": 0})
            response.read()
            conn.close()

    def stream(self, question: str, num_predict: int = 120, on_connect=None):
        """Yields (piece_of_text, final_stats_or_None). `on_connect` gets the
        connection, so another thread can close it: Ollama then stops
        generating instead of finishing a stale answer before the next one."""
        conn, response = self._post("/api/chat", self.request(question, num_predict))
        if on_connect:
            on_connect(conn)
        try:
            if response.status != 200:
                error = response.read().decode("utf-8", "replace")
                if "think" in error and self.think is not None:
                    self.think = None  # the model has no thinking switch
                    yield from self.stream(question, num_predict, on_connect)
                    return
                raise SystemExit(f"Ollama respondió {response.status}: {error}")
            for raw in response:
                message = json.loads(raw)
                yield message.get("message", {}).get("content", ""), (message if message.get("done") else None)
        finally:
            conn.close()

    def loaded(self) -> dict:
        conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
        conn.request("GET", "/api/ps")
        models = json.loads(conn.getresponse().read()).get("models", [])
        conn.close()
        return next((m for m in models if m["name"] == self.model or m["model"] == self.model), {})


def clean_point(line: str) -> str:
    return re.sub(r"^\s*(?:[-*•]|\d+[.)])\s*", "", line).strip().strip("*").strip()


# --------------------------------------------------------------------------
# The card the page shows
# --------------------------------------------------------------------------


class Card:
    def __init__(self):
        self.lock = threading.Condition()
        self.state = {"version": 0, "live": "", "question": "", "source": "", "points": [],
                      "typing": "", "latency": "", "status": "esperando"}

    def update(self, **changes) -> None:
        with self.lock:
            self.state.update(changes)
            self.state["version"] += 1
            self.lock.notify_all()

    def snapshot(self) -> dict:
        with self.lock:
            return dict(self.state, points=list(self.state["points"]))

    def wait(self, version: int, timeout: float) -> dict:
        with self.lock:
            self.lock.wait_for(lambda: self.state["version"] != version, timeout)
            return dict(self.state, points=list(self.state["points"]))


def serve(card: Card, port: int) -> None:
    page = (HERE / "tarjeta.html").read_text(encoding="utf-8")

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            # Only this computer's own pages (no DNS rebinding from a website).
            if self.headers.get("Host") not in (f"127.0.0.1:{port}", f"localhost:{port}"):
                self.send_error(403)
                return
            if self.path.split("?")[0] == "/events":
                return self.events()
            # "<" escaped: ASR or model text can't close the <script>.
            state = json.dumps(card.snapshot()).replace("<", "\\u003c")
            body = page.replace("/*INITIAL*/null", state).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def events(self):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            state = card.snapshot()
            try:
                while True:
                    self.wfile.write(b"data: " + json.dumps(state).encode() + b"\n\n")
                    self.wfile.flush()
                    state = card.wait(state["version"], 15)
            except (OSError, socket.timeout):
                return

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()


# --------------------------------------------------------------------------
# The session
# --------------------------------------------------------------------------


class VramWatch:
    """Peak `nvidia-smi` memory while the session runs (MiB; None without NVIDIA)."""

    def __init__(self):
        self.peak = self.now()
        self.stop = threading.Event()
        threading.Thread(target=self._run, daemon=True).start()

    @staticmethod
    def now() -> int | None:
        try:
            out = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                                 capture_output=True, text=True, timeout=5).stdout
            return int(out.split()[0])
        except (OSError, ValueError, IndexError, subprocess.TimeoutExpired):
            return None

    def _run(self):
        while not self.stop.wait(0.5):
            used = self.now()
            if used is not None and (self.peak is None or used > self.peak):
                self.peak = used


class Session:
    def __init__(self, args, guion: dict, ollama: Ollama | None, card: Card, timeline: list[dict]):
        self.args, self.guion, self.ollama, self.card, self.timeline = args, guion, ollama, card, timeline
        self.turns = Turns()
        self.rows: dict = {}
        self.t0 = 0.0  # monotonic time of audio position 0
        self.lock = threading.Lock()
        self.generation = 0  # the card being built; older generations are stale
        self.connection: http.client.HTTPConnection | None = None
        self.active_row: dict | None = None
        self.worker: threading.Thread | None = None
        self.segments_log: list[dict] = []

    def now_ms(self) -> int:
        return round((time.monotonic() - self.t0) * 1000)

    def turn_for(self, start_ms: int, end_ms: int) -> dict | None:
        """The script's turn that overlaps this segment most (true speech end)."""
        best, overlap = None, 0
        for turn in self.timeline:
            o = min(end_ms, turn["endMs"]) - max(start_ms, turn["startMs"])
            if o > overlap:
                best, overlap = turn, o
        return best

    def on_partial(self, event: dict) -> None:
        self.card.update(live=(event.get("stable", "") + event.get("tentative", "")).strip())

    def on_segment(self, event: dict, at_ms: int | None = None) -> None:
        arrived = self.now_ms() if at_ms is None else at_ms
        self.segments_log.append({"atMs": arrived, **{k: event[k] for k in ("startMs", "endMs", "text")}})
        print(f"[{arrived / 1000:6.1f} s] entrevistador: {event['text']}")
        self.card.update(live="")
        question = self.turns.add(event["text"], arrived)
        turn = self.turn_for(event["startMs"], event["endMs"])
        # One row per scripted turn (or per segment without a script).
        key = self.timeline.index(turn) if turn else f"s{len(self.segments_log)}"
        row = self.rows.setdefault(key, {"turno": key, "tipo": turn and turn.get("tipo"),
                                         "esperada": turn and turn.get("esperada"), "asr": "", "tarjeta": False})
        row["asr"] = f"{row['asr']} {event['text'].strip()}".strip()
        if question is None:
            return
        spoken_end = turn["endMs"] if turn else event["endMs"]
        gen = self.supersede(row)
        # A turn that fires again replaces its earlier card.
        for stale in ("coincide", "puntuacion", "fuente", "puntos", "primerPuntoMs", "tresPuntosMs", "primerTokenMs",
                      "promptTokens", "error", "cancelada"):
            row.pop(stale, None)
        row.update(pregunta=question, tarjeta=True, finPreguntaMs=spoken_end, segmentoMs=arrived - spoken_end)
        entry, score = match_prepared(question, self.guion["prepared"])
        row["coincide"], row["puntuacion"] = (entry.name if entry else None), round(score, 2)
        if self.args.solo_modelo:
            entry = None  # measure the model on every question
        if entry is not None:
            row.update(fuente="guion", puntos=entry.points[:3], primerPuntoMs=arrived - spoken_end,
                       tresPuntosMs=arrived - spoken_end)
            self.card.update(question=question, source=f"Tu guion · {entry.name}", points=entry.points[:3],
                             typing="", latency=f"{(arrived - spoken_end) / 1000:.1f} s", status="")
            print(f"           → guion: {entry.name} ({score:.2f})")
            return
        row["fuente"] = "modelo"
        self.card.update(question=question, source=f"Sugerido · {self.ollama.model}", points=[], typing="",
                         latency="", status="pensando…")
        self.active_row = row
        self.worker = threading.Thread(target=self.generate, args=(question, row, spoken_end, gen), daemon=True)
        self.worker.start()

    def supersede(self, row: dict) -> int:
        """Starts a new card: the generation still running is stale. Its
        connection is closed, so Ollama stops it at once instead of making the
        new question wait (it answers one request at a time), and the event
        thread never waits for it. A stale card that never finished counts as
        cancelled, not as fast."""
        with self.lock:
            self.generation += 1
            old, self.active_row = self.active_row, None
            if old is not None and old is not row and "tresPuntosMs" not in old:
                old["cancelada"] = True
            if self.connection is not None:
                try:
                    self.connection.sock.shutdown(socket.SHUT_RDWR)
                except (AttributeError, OSError):
                    pass
                self.connection = None
            return self.generation

    def generate(self, question: str, row: dict, spoken_end: int, gen: int) -> None:
        def current() -> bool:
            return gen == self.generation

        def connected(conn) -> None:
            with self.lock:
                if current():
                    self.connection = conn
                else:
                    conn.close()

        def show(**changes) -> None:
            with self.lock:  # never over a newer card
                if current():
                    self.card.update(**changes)

        points: list[str] = []
        buffer = ""
        first_token = None
        stats = None
        try:
            for piece, done in self.ollama.stream(question, num_predict=120, on_connect=connected):
                if not current():
                    return
                if first_token is None and piece.strip():
                    first_token = self.now_ms()
                buffer += piece
                while "\n" in buffer and len(points) < 3:
                    line, buffer = buffer.split("\n", 1)
                    if clean_point(line):
                        points.append(clean_point(line))
                        if len(points) == 1:
                            row["primerPuntoMs"] = self.now_ms() - spoken_end
                show(points=list(points), typing=clean_point(buffer) if len(points) < 3 else "")
                stats = done or stats
                if len(points) == 3:
                    break  # closing the response stops the generation
        except (OSError, ValueError, http.client.HTTPException, SystemExit) as error:
            if current():
                row["error"] = str(error)
                show(typing="", status=f"Ollama falló: {error}")
                print(f"           → error de Ollama: {error}", file=sys.stderr)
            return
        if not current():
            return
        if len(points) < 3 and clean_point(buffer):
            points.append(clean_point(buffer))  # the last line has no newline
            row.setdefault("primerPuntoMs", self.now_ms() - spoken_end)
        row["tresPuntosMs"] = self.now_ms() - spoken_end
        row["puntos"] = points
        row["primerTokenMs"] = (first_token - spoken_end) if first_token else None
        if stats:
            row["promptTokens"] = stats.get("prompt_eval_count")
        show(points=points, typing="", status="", latency=f"{row['tresPuntosMs'] / 1000:.1f} s")
        print(f"           → {self.ollama.model}: {' | '.join(points)}  ({row.get('tresPuntosMs')} ms)")

    def feed_hex(self) -> None:
        hex_ = Hex(Path(self.args.discovery))
        created = hex_.create(self.args.language)
        print(f"Hex: modelo {created['model']} ({created['language']}), parciales {'sí' if created['partials'] else 'no'}")
        pcm = read_wav(Path(self.args.wav))
        events = hex_.events()
        ready = next(events)
        assert ready["type"] == "ready", ready

        def push():
            self.t0 = time.monotonic()
            step = CHUNK * 2
            try:
                for offset in range(0, len(pcm), step):
                    chunk = pcm[offset: offset + step]
                    due = self.t0 + (offset + len(chunk)) / 2 / RATE  # after the audio has "happened"
                    time.sleep(max(0.0, due - time.monotonic()))
                    hex_.call("POST", f"/streams/{hex_.id}/audio", chunk, "application/octet-stream")
                silence = b"\0" * step
                for _ in range(15):  # 1.5 s of silence closes the last window
                    time.sleep(0.1)
                    hex_.call("POST", f"/streams/{hex_.id}/audio", silence, "application/octet-stream")
                hex_.call("POST", f"/streams/{hex_.id}/finish", timeout=130)
            except (SystemExit, OSError) as error:  # a thread's SystemExit would vanish silently
                self.card.update(status=f"Hex: {error}")
                print(f"Error enviando audio a Hex: {error}", file=sys.stderr)

        threading.Thread(target=push, daemon=True).start()
        for event in events:
            if event["type"] == "partial":
                self.on_partial(event)
            elif event["type"] == "segment":
                self.on_segment(event)
            elif event["type"] == "error":
                raise SystemExit(f"Hex: {event}")

    def replay(self) -> None:
        lines = [json.loads(l) for l in Path(self.args.replay).read_text(encoding="utf-8").splitlines() if l.strip()]
        self.t0 = time.monotonic()
        for segment in lines:
            time.sleep(max(0.0, self.t0 + segment["atMs"] / 1000 - time.monotonic()))
            self.on_segment(segment, at_ms=segment["atMs"])

    def finish(self) -> None:
        if self.worker:
            self.worker.join()


def summarize(rows: list[dict]) -> dict:
    model_rows = [r for r in rows if r.get("fuente") == "modelo" and r.get("tresPuntosMs") is not None]
    times = [r["tresPuntosMs"] for r in model_rows]
    firsts = [r["primerPuntoMs"] for r in model_rows if r.get("primerPuntoMs") is not None]
    questions = [r for r in rows if r.get("tipo") in ("prevista", "imprevista")]
    return {
        "preguntasConTarjeta": sum(1 for r in questions if r["tarjeta"]),
        "preguntas": len(questions),
        "noPreguntasConTarjeta": sum(1 for r in rows if r.get("tipo") == "no-pregunta" and r["tarjeta"]),
        "previstasAlGuion": sum(1 for r in rows if r.get("tipo") == "prevista" and r.get("coincide") == r.get("esperada")),
        "previstas": sum(1 for r in rows if r.get("tipo") == "prevista"),
        "imprevistasAlGuion": sum(1 for r in rows if r.get("tipo") == "imprevista" and r.get("coincide")),
        # Sent to the model but never shown whole (cancelled by a newer question, or failed).
        "modeloSinTerminar": sum(1 for r in rows if r.get("fuente") == "modelo" and r.get("tresPuntosMs") is None),
        "modeloTresPuntosMs": {"mediana": statistics.median(times), "peor": max(times)} if times else None,
        "modeloPrimerPuntoMs": {"mediana": statistics.median(firsts), "peor": max(firsts)} if firsts else None,
        "segmentoMs": statistics.median([r["segmentoMs"] for r in rows if r.get("segmentoMs") is not None] or [0]),
    }


def default_discovery() -> Path:
    root = os.environ.get("HEX_APPLICATION_SUPPORT_DIR") or str(Path(os.environ.get("APPDATA", "~")) / "hex")
    return Path(root) / "local-api.json"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Apuntador de entrevistas sobre /streams de Hex y Ollama.")
    parser.add_argument("--guion", default=str(HERE / "ejemplo" / "guion.md"), help="tu guion (plantilla-guion.md)")
    parser.add_argument("--contexto", nargs="*", help="CV, oferta, notas (por defecto: los .md junto al guion)")
    parser.add_argument("--model", default="qwen3.5:4b", help="modelo de Ollama")
    parser.add_argument("--wav", help="WAV del entrevistador (16 kHz mono) que se envía a Hex en tiempo real")
    parser.add_argument("--replay", help="segmentos grabados en una sesión anterior (segmentos.jsonl), sin Hex")
    parser.add_argument("--timeline", help="tiempos reales de cada turno (por defecto, el .timeline.json del WAV)")
    parser.add_argument("--discovery", default=str(default_discovery()), help="local-api.json del Hex a usar")
    parser.add_argument("--language", default="es", help="idioma que debe tener cargado Hex")
    parser.add_argument("--solo-modelo", action="store_true",
                        help="pregunta al modelo también las preguntas del guion (para comparar modelos)")
    parser.add_argument("--cold", action="store_true", help="descarga el modelo antes, para medir su carga")
    parser.add_argument("--out", help="carpeta donde guardar resultados (JSON) y segmentos")
    parser.add_argument("--port", type=int, default=8766, help="puerto de la página")
    parser.add_argument("--open", action="store_true", help="abre la página en el navegador")
    parser.add_argument("--linger", type=float, default=0, help="segundos que la página sigue viva al terminar")
    args = parser.parse_args(argv)
    if not (args.wav or args.replay):
        parser.error("hace falta --wav o --replay")

    guion_path = Path(args.guion)
    guion_text = guion_path.read_text(encoding="utf-8")
    skip = {guion_path.name.lower(), "readme.md", "plantilla-guion.md"}
    context = [Path(p) for p in args.contexto] if args.contexto is not None else sorted(
        p for p in guion_path.parent.iterdir()
        if p.suffix.lower() in (".md", ".txt") and p.name.lower() not in skip)
    files = {"guion": guion_text, **{p.stem: p.read_text(encoding="utf-8") for p in context}}
    print("Contexto:", ", ".join(p.name for p in context) or "ninguno")
    guion = parse_guion(guion_text)
    timeline_path = Path(args.timeline or Path(args.wav or HERE / "ejemplo" / "entrevista.wav").with_suffix(".timeline.json"))
    timeline = json.loads(timeline_path.read_text(encoding="utf-8"))["turnos"] if timeline_path.exists() else []
    if not timeline:
        print(f"Sin {timeline_path.name}: no mido la latencia desde el final real de cada pregunta "
              "(se usa el final del segmento de Hex).", file=sys.stderr)

    ollama = Ollama(args.model, build_prefix(files))
    vram_before = VramWatch.now()
    if args.cold:
        ollama.unload_all()
        time.sleep(2)
        vram_before = VramWatch.now()
    started = time.monotonic()
    stats = None
    # Loads the model and caches the prefix. A whole answer, not one token:
    # measured, the first real question after a bare load took 6 s on gemma4.
    for _, done in ollama.stream("¿Qué sabes de nuestra empresa?", num_predict=40):
        stats = done or stats
    load = {"cargaMs": round((stats or {}).get("load_duration", 0) / 1e6), "calentamientoMs": round((time.monotonic() - started) * 1000),
            "prefijoTokens": (stats or {}).get("prompt_eval_count"), "numCtx": ollama.num_ctx}
    ps = ollama.loaded()
    load["vramOllamaMiB"] = round(ps.get("size_vram", 0) / 2**20)
    vram_after = VramWatch.now()
    print(f"{args.model}: cargado en {load['cargaMs']} ms, prefijo {load['prefijoTokens']} tokens, "
          f"VRAM Ollama {load['vramOllamaMiB']} MiB, GPU {vram_before} → {vram_after} MiB")

    card = Card()
    serve(card, args.port)
    if args.open:
        webbrowser.open(f"http://127.0.0.1:{args.port}/")
    session = Session(args, guion, ollama, card, timeline)
    watch = VramWatch()
    try:
        session.replay() if args.replay else session.feed_hex()
        session.finish()
    except KeyboardInterrupt:
        pass
    watch.stop.set()
    rows = list(session.rows.values())
    summary = summarize(rows)
    print(json.dumps(summary, ensure_ascii=False))
    if args.out:
        out = Path(args.out)
        out.mkdir(parents=True, exist_ok=True)
        name = re.sub(r"[^\w.-]", "_", args.model)
        result = {"modelo": args.model, "fuente": "hex" if args.wav else "replay", "carga": load,
                  "gpuMiB": {"antes": vram_before, "cargado": vram_after, "pico": watch.peak},
                  "resumen": summary, "filas": rows}
        (out / f"{name}.json").write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
        if args.wav:
            (out / "segmentos.jsonl").write_text(
                "".join(json.dumps(s, ensure_ascii=False) + "\n" for s in session.segments_log), encoding="utf-8")
    if args.linger:
        time.sleep(args.linger)
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", line_buffering=True)
    raise SystemExit(main())
