"""Nemotron 3.5 streaming latency per lookahead (att_context_right), on the
same transcribe.cpp 0.4.1 and GGUF Hex uses. Usage: lookahead.py MODEL WAV [REFERENCE_TEXT]"""
import re
import statistics as st
import sys
import time
import wave

import numpy as np
import transcribe_cpp as tc

model_path, wav_path = sys.argv[1], sys.argv[2]
reference = sys.argv[3] if len(sys.argv) > 3 else None
with wave.open(wav_path) as w:
    assert w.getframerate() == 16000 and w.getnchannels() == 1
    pcm = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768

words = lambda s: re.findall(r"\w+", s.lower())


def wer(ref, hyp):
    r, h = words(ref), words(hyp)
    d = list(range(len(h) + 1))
    for i in range(1, len(r) + 1):
        prev, d[0] = d[0], i
        for j in range(1, len(h) + 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (r[i - 1] != h[j - 1]))
    return d[len(h)] / len(r)


model = tc.Model(model_path, backend="vulkan")
session = model.session()
offline = session.run(pcm, language="es-ES", timestamps="word")
ends = [w.t1_ms for w in offline.words if words(w.text)]
print(f"offline: {offline.text.strip()}")
reference = reference or offline.text
STEP = 1280  # 80 ms

for right in (13, 6, 3, 0):
    appear, stable, updates, costs = {}, {}, [], []
    with session.stream(language="es-ES", family=tc.ParakeetStreamOptions(att_context_right=right)) as s:
        for start in range(0, len(pcm), STEP):
            t = time.perf_counter()
            u = s.feed(pcm[start:start + STEP])
            cost = (time.perf_counter() - t) * 1000
            costs.append(cost)
            fed = (start + STEP) / 16
            if u.result_changed:
                text = s.text()
                updates.append(fed)
                # A word counts as shown the first time the shown text has that many words.
                for k in range(len(words(text.display))):
                    appear.setdefault(k, fed + cost)
                for k in range(len(words(text.committed))):
                    stable.setdefault(k, fed + cost)
        s.finalize()
        final = s.text().display
    lag = [appear[k] - ends[k] for k in range(min(len(ends), len(appear)))]
    lag_stable = [stable[k] - ends[k] for k in range(min(len(ends), len(stable)))]
    gaps = [b - a for a, b in zip(updates, updates[1:])]
    print(f"\n== right={right}")
    print(f"  final: {final.strip()}")
    print(f"  WER vs reference: {wer(reference, final):.1%}")
    print(f"  word shown after it was spoken: median {st.median(lag):.0f} ms, p90 {sorted(lag)[int(len(lag) * .9)]:.0f} ms")
    if lag_stable:
        print(f"  word committed after it was spoken: median {st.median(lag_stable):.0f} ms")
    print(f"  updates: {len(updates)}, every {st.median(gaps):.0f} ms (median)")
    print(f"  GPU time per 80 ms fed: median {st.median(costs):.1f} ms, max {max(costs):.0f} ms, total {sum(costs) / (len(pcm) / 16):.1%} of audio")
