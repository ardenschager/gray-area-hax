# 261111 sound re-voice, v001

Branch `overhaul-sound`. Open `index.html` to hear before and after side by side.

## Why
Arden said the built-in instruments sounded "very generic". They were textbook SuperCollider
patches: a supersaw lead, a seven-saw pad, an FM EP, a DX bell. The instruments are secondary to
sampling, so this is a voicing pass. The engine is unchanged (plyphon, in-process), and so are
the def ids, the arg interface and the sc.rs API. Most care went to what layers with footage:
the drums, the bass, and the two beds (drone, grain cloud). Every voice also gets some
instability (drift, wow, flutter, grit) so nothing sounds like a preset.

## How the renders were made
- `cargo test --release -p chromagrain-core sc::tests::dump_phrases -- --ignored` with `SC_WAV_DIR`
  renders each instrument's starter phrase at 120 bpm, repeated to at least 6 s, plus each drum
  soloed. This is the app's own render path: loop-folded and loudness-normalised to about -16 dBFS
  RMS, so before and after play at matching loudness.
- `tools/audio_check.py DIR` reports peak, RMS, crest, DC, clipped samples, silent fraction,
  clicks (second-difference outliers off the note grid), 64-sample block-edge (zipper) ratio,
  spectral centroid and L/R correlation, and writes `stats.json`.
- `tools/spectrograms.sh DIR` writes log-frequency spectrograms (ffmpeg `showspectrumpic`).
- `tools/listening_page.py v001/` builds `index.html`.
- The "before" is commit 4d6e7e0 (`overhaul`). The "after" is this branch.

## Per instrument (label changes in sc.rs; ids unchanged)

| id | before | after | what changed |
|---|---|---|---|
| cg_kick | sine + pitch env | **kick** | Pitch dives from 3–10× the note and then sags to 0.9× as it dies. Beater = HPF noise tick + a 4.2 kHz ring. Short mid "knock" at 3.3×. tanh drive scales with `punch`. The kit gains a `kick click` knob. |
| cg_snare | 2 sines + noise | **snare** | Five membrane modes (1, 1.59, 2.14, 2.30, 2.65) with a 1.4→1 pitch drop and a stick crack. The wires are noise ring-modulated by the head's fundamental, so they rattle *with* the drum, plus a comb tuned to the shell. |
| cg_hat | 6 pulses + BPF | **hats** | Six squares at the real TR-808 ratios, ring-modulated in pairs (denser, clangier sidebands), two BPFs around `freq`, and noise sizzle. `tone` retunes the metal. |
| cg_bass | saw + sub, Moog | **fold bass** | West-coast: sine core into a wavefolder whose depth rides an envelope and velocity (accents bark, ghost notes stay round), then MoogFF and asymmetric tanh for even harmonics. `sub` is now a clean sine at pitch (plus a little octave-down), so it reads on small speakers. ±7 cents of drift. LeakDC moved to 3.8 Hz so the sub isn't thinned. `drive` (label "fold") now defaults to 0.5. |
| cg_acid | RLPF saw/square | **acid fuzz** | Accent above velocity 0.75 opens the filter further, tightens resonance, shortens decay and adds level. Two-stage filter (steeper). Fuzz pedal: asymmetric hard clip blended with tanh, with a 5.5 kHz tone. The square side gets a PWM sweep. |
| cg_drone | resonant noise | **tape drone** | Nine partials, each drifting in level and tune on its own clock, crossfading sine→VarSaw with `color`. A wandering resonant LPF, plus an "overtone singer" band sweeping 4–20× the root. Tape saturation, hiss and dust specks, wow and flutter (deeper with `motion`), decorrelated per channel. Deliberately stays below ~1 kHz so it sits under dialogue. |
| cg_pad | 7 detuned saws | **grain cloud** | Dust-and-impulse-triggered GrainFM swarm: each grain picks note, ×2, ×3 or ×4, with `detune` as scatter and `width` as pan scatter. `air` is density plus hiss. Under it, a dark drifting VarSaw bed. Allpass diffusion and light tape wobble. A granular pad for a granular sampler. |
| cg_keys | FM EP + tremolo | **cassette keys** | Same FM tine core, bounced to a worn cassette. Wow and flutter replace the tremolo (the knob is now "wobble"), with separate transports for L and R so the wobble also gives width. Tube-ish asymmetric bark on hard hits, 6.5 kHz roll-off, hiss, per-key detune. Fixes a DC offset in the old def (0.036 → 0). |
| cg_pluck | Karplus-Strong | **prepared string** | Exciter morphs from a soft thumb to a hard metal pick (`color`, label "pick"). A "bolt" of free-bar modes (2.756, 5.404, 8.933×). Jawari buzz: only the peaks above a threshold pass, high-passed. `body` (label "prepared", default 0.55) sets how prepared it is. Slightly widened. |
| cg_bell | 2-op FM bell | **gamelan metal** | Modal bar (Ringz bank, free-bar ratios warped by `ratio`, label "metal"). Mallet hardness comes from `index` ("strike"). Each partial is doubled 0.6–4.6 Hz apart between L and R, so it beats like a gamelan pair (ombak); `shimmer` ("beating") sets the rate. Low gong hum. |
| cg_lead | supersaw | **formant sync lead** | Band-limited `Formant` oscillators (hard-sync tone without aliasing). The formant tears down on every note; the new `sync` knob replaces `detune` and sets how far. Feedback-FM sine an octave down, capped below the noise threshold. Stereo via detune and a 9 ms offset. |
| cg_choir | saw + 3 BPF vowels | **speak & spell choir** | Three jittery Blip "glottal" sources per note (low-passed for a natural roll-off), with vowels interpolated (no longer stepped) and drifting per singer so the chord talks. New `grit` knob: mixes in an 8 kHz / 7-bit sample-and-hold crush. |

## Measured (after)
- Every file: DC < 0.004, no hard clipping, no NaN, block-edge ratio 0.94–1.01. The solo kick
  reads 1.31 because its hits start exactly on block edges; excluding 256 samples after each
  onset it is 1.009, so it's not zipper.
- The "clicks" the detector flags are intentional: saw edges on acid (same as before) and dust
  specks on the tape drone (d2 ≈ 0.1 on a 0.4 signal). The solo hat's "near-clip" is the
  normaliser's soft knee on a sparse signal. The kit render peaks at -1.5 dBFS.
- Sub-60 Hz energy on lead, keys, choir and bell is at -52 dB or lower. Keys went from -26 dB of
  DC/sub junk to -52 dB.

## Tests
- New `sc::tests::every_synthdef_renders_alone`: every instrument at 3 pitches and velocities,
  and every kit drum soft and hard, compiles in plyphon and renders finite, audible audio with
  headroom and no DC.
- New ignored `sc::tests::dump_phrases` (the render tool above).
- `cargo test`: 137 passed, 4 ignored. `cargo build --release` OK.

## plyphon coverage
No limits hit. The registry has about 250 UGens: Ringz, Klank, Pluck, Formant, Formlet, GrainFM/Sin,
SinOscFB, VarSaw, Latch, Gendy1, fold2, excess, CombC, AllpassC, PitchShift, FreeVerb and GVerb,
among others. Not available (and not used): Linen, Vibrato, IRand, and unary random ops
(`rand`, `rrand` on signals). Use Rand, TRand or TIRand instead.

## Next
- Arden: "no SuperCollider dependency at all". The re-voiced defs are being ported to a small
  Rust graph builder in crates/core that feeds plyphon `SynthDef`s directly (no sclang, no
  .scsyndef). Before build.scd and the binaries are deleted, a render-equality test will check
  the port against these defs.
