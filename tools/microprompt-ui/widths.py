"""MicroPrompt's panes at several window widths, light and dark (IN-01).

Draws each scene with `hex app -- --snap` (a fixture: no runtime, no data
folder touched) and writes target/widths/index.html, one row per scene and
theme, each image at the window's own size so overflow shows.

    cargo build --bin hex
    python tools/microprompt-ui/widths.py [--exe target/debug/hex.exe] [scene ...]
"""

import argparse
import html
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# 640: below today's minimum, if we lower it; 860: today's minimum;
# 980: the default size; 1300: a wide window.
WIDTHS = [640, 860, 980, 1300]
HEIGHT = 760
SCENES = [
    "teleprompter-idle",
    "teleprompter",
    "teleprompter-speakers",
    "teleprompter-assistant-idle",
    "teleprompter-assistant",
    "teleprompter-thread",
    "teleprompter-script",
    "teleprompter-interview",
    "history-teleprompter",
    "windows-labs",
    "windows-settings",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=str(ROOT / "target/debug/hex.exe"))
    parser.add_argument("--out", default=str(ROOT / "target/widths"))
    parser.add_argument("scenes", nargs="*", default=SCENES)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    failed = []
    for scene in args.scenes:
        for theme in ("light", "dark"):
            for width in WIDTHS:
                png = out / f"{scene}-{theme}-{width}.png"
                run = subprocess.run(
                    [args.exe, "app", "--", "--snap", str(png), "--scene", scene,
                     "--theme", theme, "--size", f"{width}x{HEIGHT}", "--scale", "1",
                     # Past the first frame: panes that measure their width
                     # (History, Labs) lay out again once they know it.
                     "--at", "0.5"],
                    capture_output=True, text=True,
                )
                if run.returncode != 0 or not png.exists():
                    failed.append(f"{scene} {theme} {width}: {run.stderr.strip()[-200:]}")
                print(f"{scene} {theme} {width}", "ok" if png.exists() else "FAILED", flush=True)
    rows = []
    for scene in args.scenes:
        for theme in ("light", "dark"):
            images = "".join(
                f'<figure><img src="{html.escape(f"{scene}-{theme}-{w}.png")}" width="{w}">'
                f"<figcaption>{w} px</figcaption></figure>"
                for w in WIDTHS
            )
            rows.append(f'<h2 id="{scene}-{theme}">{scene} · {theme}</h2><div class="row">{images}</div>')
    (out / "index.html").write_text(
        "<!doctype html><meta charset=utf-8><title>MicroPrompt widths</title>"
        "<style>body{font:14px system-ui;margin:16px;background:#888}"
        ".row{display:flex;gap:16px;align-items:flex-start;overflow-x:auto}"
        "figure{margin:0}img{display:block;outline:1px solid #000}"
        "figcaption{padding:4px 0}h2{margin:24px 0 8px}</style>"
        + "".join(rows),
        encoding="utf-8",
    )
    print(out / "index.html")
    for line in failed:
        print("FAILED", line, file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
