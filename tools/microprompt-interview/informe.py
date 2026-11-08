"""Tablas en Markdown de los resultados de apuntador.py (--out), para el README.

    py informe.py resultados/solo-modelo/*.json
"""

from __future__ import annotations

import json
import statistics
import sys
from pathlib import Path


def seconds(ms) -> str:
    return "—" if ms is None else f"{ms / 1000:.1f}".replace(".", ",")


def main(paths: list[str]) -> int:
    runs = [json.loads(Path(p).read_text(encoding="utf-8")) for p in paths]
    print("| Modelo | VRAM del modelo | GPU con todo (pico) | Carga en frío | 1.er punto (mediana / peor) | 3 puntos (mediana / peor) | ≤ 4 s |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    for run in runs:
        rows = [r for r in run["filas"] if r.get("fuente") == "modelo" and r.get("tresPuntosMs") is not None]
        first = sorted(r["primerPuntoMs"] for r in rows if r.get("primerPuntoMs") is not None)
        three = sorted(r["tresPuntosMs"] for r in rows)
        # nvidia-smi before and after loading: Ollama's own size_vram undercounts gemma4.
        vram = (run["gpuMiB"]["cargado"] - run["gpuMiB"]["antes"]) / 1024
        print(f"| {run['modelo']} | {seconds(vram * 1000)} GB | {seconds(run['gpuMiB']['pico'] / 1.024)} GB"
              f" | {seconds(run['carga']['cargaMs'])} s"
              f" | {seconds(statistics.median(first))} / {seconds(first[-1])} s | {seconds(statistics.median(three))} / {seconds(three[-1])} s"
              f" | {sum(t <= 4000 for t in three)} de {len(three)} |")
    print()
    questions = [r for r in runs[0]["filas"] if r.get("tipo") in ("prevista", "imprevista")]
    for number, question in enumerate(questions, 1):
        kind = question["tipo"] + (f" (guion: {question['esperada']})" if question.get("esperada") else "")
        print(f"#### {number}. «{question.get('pregunta') or question['asr']}»\n")
        print(f"{kind}. Coincidencia con el guion: {question.get('coincide') or 'ninguna'}.\n")
        print("| Modelo | Puntos | 3 puntos a | Tu nota (1–5) |")
        print("| --- | --- | ---: | --- |")
        for run in runs:
            row = next((r for r in run["filas"] if r["turno"] == question["turno"]), {})
            points = " · ".join(row.get("puntos") or []) or ("sin tarjeta" if not row.get("tarjeta") else "—")
            print(f"| {run['modelo']} | {points} | {seconds(row.get('tresPuntosMs'))} s |  |")
        print()
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main(sys.argv[1:]))
