#!/usr/bin/env python3
"""Build outputs/261111_sound-revoice/v001/index.html from the renders + stats.json."""
import json, os, sys, html

root = sys.argv[1]
ver = os.path.basename(root.rstrip("/"))

ITEMS = [
    ("kit", "drum kit", "all three drums together in the starter groove"),
    ("cg_kick", "kick", "sine that dives from two octaves up and sags under the note, beater tick and mid knock, overdriven with punch"),
    ("cg_snare", "snare", "five struck drum-head modes with a pitch drop, stick crack, and wires that rattle with the head (noise ring-modded by the membrane + a comb tuned to the shell)"),
    ("cg_hat", "hats", "six squares at the TR-808 cymbal ratios, ring-modulated in pairs for a clangier metal, band-passed for closed/open"),
    ("cg_bass", "fold bass", "west-coast wavefolded sine through a Moog ladder; velocity and an envelope drive the folder so accents bark and ghost notes stay round; analog drift"),
    ("cg_acid", "acid fuzz", "303 squelch with real accents (loud notes open the filter further and tighten the resonance), steeper filter, then a fuzz pedal"),
    ("cg_drone", "tape drone", "partials drifting in level and tune on their own clocks, a resonant filter and an overtone-singer band wandering through them, on a worn tape loop with hiss and dust; stays below ~1 kHz so it sits under dialogue"),
    ("cg_pad", "grain cloud", "each note is a swarm of tiny FM grains scattered across octaves and the stereo field, over a dark drifting bed, smeared by allpass diffusion and a little tape wobble"),
    ("cg_keys", "cassette keys", "the FM tine piano bounced to a worn cassette: wow and flutter (the wobble knob), tube-ish bark on hard hits, rolled-off top, hiss; also fixes the old DC offset"),
    ("cg_pluck", "prepared string", "Karplus-Strong string with a bolt wedged in it: inharmonic free-bar modes ring with each pluck and a sitar-like jawari buzz rides the peaks"),
    ("cg_bell", "gamelan metal", "struck modal bar (free-bar ratios, warped by the metal knob), every partial doubled a few Hz apart between L and R so it beats like a gamelan pair, gong hum underneath"),
    ("cg_lead", "formant sync lead", "band-limited formant oscillators whose formant tears down on every note (the sync-lead 'yow' without aliasing), over a feedback-FM sine an octave down"),
    ("cg_choir", "speak & spell choir", "three jittery glottal buzzes per note through drifting vowel formants, so the chord talks; part of it crushed to an 8 kHz / 7-bit toy speech chip (new grit knob)"),
]


def load(d):
    try:
        return {s["file"][:-4]: s for s in json.load(open(os.path.join(root, d, "stats.json")))}
    except FileNotFoundError:
        return {}


before, after = load("before"), load("after")


def stat(s):
    if not s:
        return ""
    fl = ", ".join(s["flags"]) or "clean"
    return (f"peak {s['peak_db']} dBFS · rms {s['rms_db']} dBFS · crest {s['crest_db']} dB · "
            f"centroid {s['centroid_hz']} Hz · L/R corr {s['lr_corr']} · {html.escape(fl)}")


rows = []
for key, name, desc in ITEMS:
    rows.append(f"""
<section>
  <h2>{html.escape(name)} <code>{key}</code></h2>
  <p class="desc">{html.escape(desc)}</p>
  <div class="pair">
    <div><h3>before</h3><audio controls preload="none" src="before/{key}.wav"></audio>
      <img src="before/{key}.png" alt="before spectrogram" loading="lazy"><p class="stat">{stat(before.get(key))}</p></div>
    <div><h3>after</h3><audio controls preload="none" src="after/{key}.wav"></audio>
      <img src="after/{key}.png" alt="after spectrogram" loading="lazy"><p class="stat">{stat(after.get(key))}</p></div>
  </div>
</section>""")

page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>chromagrain sound re-voice {ver}</title>
<style>
  body {{ background:#8a8a8a; color:#111; font:15px/1.45 -apple-system, Helvetica, sans-serif; margin:0; padding:24px 32px; }}
  h1 {{ font-weight:600; margin:0 0 4px; }}
  .lede {{ max-width:70em; margin:0 0 24px; }}
  section {{ background:#9a9a9a; border-radius:6px; padding:14px 18px; margin:0 0 18px; }}
  h2 {{ margin:0; font-size:18px; }} h2 code {{ font-size:12px; color:#333; margin-left:6px; }}
  h3 {{ margin:6px 0; font-size:12px; text-transform:uppercase; letter-spacing:.08em; color:#333; }}
  .desc {{ margin:4px 0 10px; max-width:70em; }}
  .pair {{ display:grid; grid-template-columns:1fr 1fr; gap:18px; }}
  audio {{ width:100%; }} img {{ width:100%; display:block; margin-top:6px; border-radius:3px; }}
  .stat {{ font-size:11px; color:#222; margin:4px 0 0; }}
</style></head><body>
<h1>chromagrain: instrument re-voice ({ver})</h1>
<p class="lede">Every built-in instrument playing its starter phrase at 120 bpm (repeated to at least 6 s), rendered by plyphon
exactly like the app does (loop-folded, loudness-normalised to about -16 dBFS RMS), so before and after play at matching loudness.
Drums are also soloed. Spectrograms are log-frequency. Stats come from <code>tools/audio_check.py</code>.
"clicks" on acid are saw-wave edges and on the drone are the intentional tape-dust specks; hat "near-clip" is the soft-knee
limiter catching a sparse solo hat (no hard clipping; the kit render peaks at -1.5 dBFS). See NOTES.md for what changed and why.</p>
{''.join(rows)}
</body></html>
"""
open(os.path.join(root, "index.html"), "w").write(page)
print("wrote", os.path.join(root, "index.html"))
