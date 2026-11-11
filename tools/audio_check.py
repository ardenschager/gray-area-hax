#!/usr/bin/env python3
"""Measurable sanity checks for rendered instrument phrases (no numpy).

    python3 tools/audio_check.py DIR [DIR ...]   # prints a table, writes DIR/stats.json

Per file: peak / RMS (dBFS), DC offset, clipped samples, silent fraction
(10 ms windows below -60 dBFS), clicks (second-difference outliers against
their local neighbourhood, outside note attacks), the 64-sample block-edge
step ratio (zipper from control-rate jumps), spectral centroid and L/R
correlation. Flags anything suspicious.
"""
import json, math, os, sys, wave, array, cmath

GRID = float(os.environ.get("GRID", "0.125"))


def read(path):
    with wave.open(path) as w:
        ch, sr, n = w.getnchannels(), w.getframerate(), w.getnframes()
        a = array.array("h", w.readframes(n))
    l = [a[i] / 32768.0 for i in range(0, len(a), ch)]
    r = [a[i] / 32768.0 for i in range(ch - 1, len(a), ch)]
    return sr, l, r


def db(x):
    return 20 * math.log10(max(x, 1e-9))


def centroid(m, sr):
    # Average magnitude spectrum of 2048-sample frames (radix-2 FFT).
    N = 2048

    def fft(x):
        n = len(x)
        if n == 1:
            return x
        e, o = fft(x[0::2]), fft(x[1::2])
        t = [cmath.exp(-2j * math.pi * k / n) * o[k] for k in range(n // 2)]
        return [e[k] + t[k] for k in range(n // 2)] + [e[k] - t[k] for k in range(n // 2)]

    win = [0.5 - 0.5 * math.cos(2 * math.pi * i / N) for i in range(N)]
    acc = [0.0] * (N // 2)
    frames = 0
    for s in range(0, len(m) - N, len(m) // 24 or N):
        X = fft([m[s + i] * win[i] for i in range(N)])
        for k in range(N // 2):
            acc[k] += abs(X[k])
        frames += 1
    tot = sum(acc) or 1
    return sum(k * sr / N * a for k, a in enumerate(acc)) / tot


def check(path):
    sr, l, r = read(path)
    n = len(l)
    m = [(a + b) * 0.5 for a, b in zip(l, r)]
    peak = max(max(abs(v) for v in l), max(abs(v) for v in r))
    rms = math.sqrt(sum(v * v for v in l + r) / (2 * n))
    dc = max(abs(sum(l) / n), abs(sum(r) / n))
    clipped = sum(1 for v in l + r if abs(v) >= 0.999)
    W = sr // 100
    wins = [math.sqrt(sum(v * v for v in m[i:i + W]) / W) for i in range(0, n - W, W)]
    silent = sum(1 for x in wins if x < 10 ** (-60 / 20)) / max(len(wins), 1)
    # Clicks: |d2| far above the local d2 level, and not at an attack
    # (envelope rising sharply over the next 5 ms).
    d2 = [0.0, 0.0] + [abs(m[i] - 2 * m[i - 1] + m[i - 2]) for i in range(2, n)]
    B = 256
    blk = [math.sqrt(sum(v * v for v in d2[i:i + B]) / B) + 1e-6 for i in range(0, n, B)]
    clicks = []
    for i in range(2, n):
        if d2[i] < 0.04:
            continue
        b = i // B
        local = sorted(blk[max(0, b - 8):b] + blk[b + 1:b + 9])
        ref = local[len(local) // 2] if local else 1e-6
        if d2[i] > 25 * ref:
            # Notes start on a 16th-note grid (0.125 s at 120 bpm); a
            # sharp edge just after a grid point is an attack, not a click.
            # Anything else (note-offs, mid-note) is suspicious.
            ph = (i / sr) % GRID
            if ph < 0.025 or ph > GRID - 0.001:
                continue
            if not clicks or i - clicks[-1] > sr // 50:
                clicks.append(i)
    edge = inner = 0.0
    ne = ni = 0
    for i in range(1, n):
        d = abs(m[i] - m[i - 1])
        if i % 64 == 0:
            edge += d; ne += 1
        else:
            inner += d; ni += 1
    zipper = (edge / ne) / max(inner / ni, 1e-9)
    dot = sum(a * b for a, b in zip(l, r))
    el = sum(a * a for a in l); er = sum(b * b for b in r)
    corr = dot / max(math.sqrt(el * er), 1e-12)
    st = dict(file=os.path.basename(path), secs=round(n / sr, 2), peak_db=round(db(peak), 2), rms_db=round(db(rms), 2),
              crest_db=round(db(peak) - db(rms), 1), dc=round(dc, 5), clipped=clipped, silent=round(silent, 3),
              clicks=[round(c / sr, 3) for c in clicks[:8]], n_clicks=len(clicks), zipper=round(zipper, 2),
              centroid_hz=round(centroid(m, sr)), lr_corr=round(corr, 3))
    flags = []
    if peak > 0.995: flags.append("near-clip")
    if clipped: flags.append(f"{clipped} clipped")
    if dc > 0.01: flags.append("DC")
    if rms < 0.01: flags.append("quiet")
    if silent > 0.5: flags.append("mostly silent")
    if clicks: flags.append(f"{len(clicks)} click(s)")
    if zipper > 1.6: flags.append("zipper")
    st["flags"] = flags
    return st


def main():
    for d in sys.argv[1:]:
        rows = [check(os.path.join(d, f)) for f in sorted(os.listdir(d)) if f.endswith(".wav")]
        print(f"\n== {d}")
        print(f"{'file':<16}{'peak':>7}{'rms':>7}{'crest':>6}{'dc':>8}{'sil':>6}{'zip':>6}{'cent':>7}{'corr':>6}  flags")
        for s in rows:
            print(f"{s['file'][:-4]:<16}{s['peak_db']:>7}{s['rms_db']:>7}{s['crest_db']:>6}{s['dc']:>8}{s['silent']:>6}"
                  f"{s['zipper']:>6}{s['centroid_hz']:>7}{s['lr_corr']:>6}  {', '.join(s['flags'])} {s['clicks'] if s['clicks'] else ''}")
        with open(os.path.join(d, "stats.json"), "w") as f:
            json.dump(rows, f, indent=1)


if __name__ == "__main__":
    main()
