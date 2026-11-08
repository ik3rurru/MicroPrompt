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


def spread(values: list) -> str:
    return f"{seconds(statistics.median(values))} / {seconds(max(values))} s" if values else "—"


def main(paths: list[str]) -> int:
    if not paths:
        raise SystemExit(__doc__)
    runs = [json.loads(Path(p).read_text(encoding="utf-8")) for p in paths]
    print("| Modelo | VRAM del modelo | GPU con todo (pico) | Carga en frío | 1.er punto (mediana / peor) | 3 puntos (mediana / peor) | ≤ 4 s |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    for run in runs:
        asked = [r for r in run["filas"] if r.get("fuente") == "modelo"]
        done = [r for r in asked if r.get("tresPuntosMs") is not None]
        first = [r["primerPuntoMs"] for r in done if r.get("primerPuntoMs") is not None]
        three = [r["tresPuntosMs"] for r in done]
        # nvidia-smi before and after loading: Ollama's own size_vram undercounts gemma4.
        gpu = run["gpuMiB"]
        vram = (gpu["cargado"] - gpu["antes"]) * 1000 / 1024 if None not in (gpu["cargado"], gpu["antes"]) else None
        peak = gpu["pico"] * 1000 / 1024 if gpu["pico"] is not None else None
        # A card cancelled or failed counts as over budget.
        print(f"| {run['modelo']} | {seconds(vram)} GB | {seconds(peak)} GB | {seconds(run['carga']['cargaMs'])} s"
              f" | {spread(first)} | {spread(three)} | {sum(t <= 4000 for t in three)} de {len(asked)} |")
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
