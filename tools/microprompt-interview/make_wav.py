"""Graba la entrevista simulada con una voz de Windows (SAPI) en un WAV de
16 kHz mono, y escribe al lado sus tiempos: dónde acaba de hablar el
entrevistador en cada turno (`entrevista.wav` + `entrevista.timeline.json`).

    py make_wav.py ejemplo/entrevista.json --out ejemplo/entrevista.wav

Necesita una voz en español instalada (Helena viene con Windows en español).
Sólo la biblioteca estándar; la voz se usa a través de PowerShell.
"""

from __future__ import annotations

import argparse
import array
import json
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

RATE = 16_000

SPEAK = r"""
Add-Type -AssemblyName System.Speech
$items = Get-Content -Raw -Encoding UTF8 $args[0] | ConvertFrom-Json
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voice = $s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like 'es-*' } | Select-Object -First 1
if (-not $voice) { Write-Error 'No hay ninguna voz de Windows en español'; exit 3 }
$s.SelectVoice($voice.VoiceInfo.Name)
$s.Rate = [int]$args[1]
$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
foreach ($item in $items) {
  $s.SetOutputToWaveFile($item.path, $format)
  $s.Speak($item.text)
}
$s.SetOutputToNull()
Write-Output $voice.VoiceInfo.Name
"""


def speech_bounds(samples: array.array, threshold: int = 400) -> tuple[int, int]:
    """First and last sample louder than `threshold` (SAPI pads both ends)."""
    loud = [i for i in range(0, len(samples), 80) if abs(samples[i]) > threshold]
    if not loud:
        return 0, len(samples)
    return loud[0], min(len(samples), loud[-1] + 80)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("interview", help="JSON con los turnos (ejemplo/entrevista.json)")
    parser.add_argument("--out", required=True, help="WAV de salida")
    parser.add_argument("--rate", type=int, default=0, help="velocidad de la voz, -10 a 10")
    parser.add_argument("--lead", type=float, default=1.0, help="silencio inicial en segundos")
    args = parser.parse_args(argv)

    turns = json.loads(Path(args.interview).read_text(encoding="utf-8"))["turnos"]
    with tempfile.TemporaryDirectory() as tmp:
        items = [{"text": t["texto"], "path": str(Path(tmp) / f"t{i}.wav")} for i, t in enumerate(turns)]
        listing = Path(tmp) / "items.json"
        listing.write_text(json.dumps(items), encoding="utf-8")
        script = Path(tmp) / "speak.ps1"
        script.write_text(SPEAK, encoding="utf-8-sig")
        done = subprocess.run(
            ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script), str(listing), str(args.rate)],
            capture_output=True, text=True,
        )
        if done.returncode != 0:
            sys.exit(f"La voz de Windows falló:\n{done.stderr.strip()}")
        voice = done.stdout.strip()

        out = array.array("h", [0] * int(args.lead * RATE))
        timeline = []
        for turn, item in zip(turns, items):
            with wave.open(item["path"], "rb") as w:
                if (w.getframerate(), w.getnchannels(), w.getsampwidth()) != (RATE, 1, 2):
                    sys.exit("SAPI no devolvió 16 kHz mono de 16 bits")
                clip = array.array("h", w.readframes(w.getnframes()))
            first, last = speech_bounds(clip)
            clip = clip[first:last]
            start = len(out)
            out.extend(clip)
            timeline.append({**turn, "startMs": round(start * 1000 / RATE), "endMs": round(len(out) * 1000 / RATE)})
            out.extend([0] * int(turn.get("pausa_s", 3) * RATE))

    with wave.open(args.out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(out.tobytes())
    meta = {"wav": Path(args.out).name, "voice": voice, "durationMs": round(len(out) * 1000 / RATE), "turnos": timeline}
    Path(args.out).with_suffix(".timeline.json").write_text(json.dumps(meta, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{args.out}: {len(out) / RATE:.1f} s, {len(turns)} turnos, voz {voice}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
