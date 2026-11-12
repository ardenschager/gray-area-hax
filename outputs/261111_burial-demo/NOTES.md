# Burial AV Demo & Holdout Benchmark — Iteration Notes (`261111_burial-demo`)

- **Benchmark piece**: [examples/burial.rhai](../../../examples/burial.rhai) — *"Night Platform"*, 72.2 s (160 beats @ 133 BPM, F minor)
- **Held-out sketch**: [examples/holdout_ambient.rhai](../../../examples/holdout_ambient.rhai) — 32.0 s (32 beats @ 60 BPM, D dorian)
- **System log**: [SYSTEM_LOG.md](./SYSTEM_LOG.md)
- **Media attributions**: [assets/demo/ATTRIBUTIONS.md](../../../assets/demo/ATTRIBUTIONS.md)

---

## v001 — Stock Engine Baseline (`commit f684c60`)
- **Burial output**: `outputs/261111_burial-demo/v001/261111_chromagrain_burial-demo_v001.mp4` (`71.6 s`, `-9.3 LUFS`, `LRA 8.4 LU`, `+1.0 dBTP`)
- **Holdout output**: `outputs/261111_burial-demo/holdout/v001/261111_chromagrain_holdout-ambient_v001.mp4` (`32.0 s`, `-8.6 LUFS`, `LRA 13.4 LU`, `+0.6 dBTP`)
- **Multi-model critic consensus (`@critic`, `@critic-argon`, `@critic-opus`) + Arden (`#12`, `#14`)**:
  1. **AI Cheese (`FAIL` on `v001`)**: Full Lyria R&B lyric hooks (*"half past three"*, *"in the dark"*, *"memory of your touch"*) and theatrical TTS lines (*"I thought I saw your face..."*) sounded unmistakably AI-generated and overly literal Burial pastiche.
  2. **Squeaky & High-Pitched**: `+3/+5/+12 st` upward vocal shifts + narrow paulstretch Rhodes tine whistles (`1242–1869 Hz`) + 5-sine `hat_glitter` oscillators + `soft_clip` step discontinuity (`hi/lo = -11.1 dB`, worst 2s `-1.4 dB`).
  3. **Strobing (`496` full-frame / `380` center-ROI 1-frame reversals)**: 13.3 Hz Porter-Duff alpha ripple on stretched snippets + 0 ms percussive visual flashes over blacked-out track layers.
  4. **Groove, Mix & Visual Pacing**: Straight 16th step grid lacked UKG shuffle; sub & kick collided without sidechain ducking; Drop II duplicated Drop I's pattern; white 1950s car intruded in `vocal_street`.

---

## v002 — Engine Overhaul + Wordless Micro-Chop & Swung 2-Step Rewrite
- **Burial output**: `outputs/261111_burial-demo/v002/261111_chromagrain_burial-demo_v002.mp4` (`72.2 s`, 1733 frames, `-14.2 LUFS`, `LRA 10.9 LU`, `-1.1 dBTP`)
- **Holdout output**: `outputs/261111_burial-demo/holdout/v002/261111_chromagrain_holdout-ambient_v002.mp4` (`32.0 s`, 768 frames, `-13.6 LUFS`, `LRA 13.4 LU`, `-1.0 dBTP`)
- **What changed (`v001 -> v002`)**:
  - **SYSTEM (`crates/core`, 181 tests green)**: Continuous blended video for stretched snippets + WSOLA audio stretch; `VisualGrainStyle::{flicker, feather}` + fixed straight-alpha additive glow + `s.track_blend` + cutout `alpha_focus`; 5-tap pitch anti-aliasing + overlap/upward-pitch gain warmth + paulstretch bin smoothing + 8-comb/4-allpass HPF Freeverb + C1 `-1.0 dBFS` master ceiling; `swing`/`offset`/`humanize`, z-independent `track_duck`, streaming `TapeSettings`, `detect_slices` (`s.slices`), and Vision mask cache.
  - **SOURCES & PIECE (`assets/demo/` 6.99 MB, `examples/burial.rhai`, `examples/holdout_ambient.rhai`)**:
    - Removed **100% of sung lyric sentences and TTS speech**. Rebuilt `vocal_bench.mp4` and `vocal_street.mp4` strictly from wordless closed-lip hums, breaths, `"ooh/oh"` vowels, and 1941 archival NYC subway station pneumatic air; rebuilt `hat_glitter.mp4` from archival subway rail/turnstile foley + band-shaped noise (zero sine oscillators).
    - **Whisper (`mlx-community/whisper-small-mlx`) legibility check on `burial v002`**: **0 segments transcribed** across the entire 72.2 s render (down from 14 lyric sentences in `v001`).
    - Sequenced sparse `0.32–0.63 s` downward-pitched (`0, -2, -3, -5, -7, -12 st` varispeed, F minor) vocal micro-chops via `s.slices` + `s.step_slice`; added 60–62% UKG shuffle, laid-back backbeat rim offsets, sidechain ducking (`t_drums -> t_sub` & `t_pad`), pre-drop breaths of silence (`0:14`, `0:50`), and a distinct evolved `p_drop2` pattern with mirrored dual-tile reflections.
- **Measurable improvements (`v001 -> v002`)**:
  - `burial` 1-frame luma reversals (`>3`): **Full-frame `496 -> 0`**, **Center-ROI `380 -> 2`** (`max |dY|: 54.1 -> 27.7`).
  - `holdout` 1-frame luma reversals (`>3`): **Full-frame `294 -> 0`**, **Center-ROI `206 -> 1`** (`max |dY|: 20.6 -> 8.6`).
  - `burial` spectral warmth: `hi/lo` **`-11.1 dB -> -22.0 dB`** (worst 2s `-1.4 -> -13.5 dB`); loudness **`-9.3 -> -14.2 LUFS`**, `LRA` **`8.4 -> 10.9 LU`**, True Peak **`+1.0 -> -1.1 dBTP`**.
- **Composer honest self-critique after watching & listening to `v002` (queued to verify with critics for `v003`)**:
  1. **White 1950s car fender still visible at `0:41`**: Even trimmed to `110.1–113.3s`, the white car enters behind the man's shoulder at `111.5s` and Apple Vision's saliency mask keeps its fender/wheel at `0:41`. Must replace or crop `vocal_street` video cleanly in `v003`.
  2. **Pre-drop visual dip to near-black (`0:14.0`, `0:50.2`) & sharp rectangular bokeh patch at `0:48.5`**: Dimming `t_pad` while `t_rain` has low gain causes a brief dark blink before each drop, and the dry `0:48.5` rim throw has a hard-edged white window box in the quiet breakdown.
  3. **Static background framing across 72 s**: Because `rain_window` now stays cleanly visible underneath additive upper tracks, the single central orange streetlight orb sits at the exact same pixel `(x=0.38, y=0.52)` for all 72 s; evolving camera scale/drift or cutting to tunnel/night-water scenes in the Breakdown will deepen visual storytelling.
  4. **Breakdown `1242 Hz` Rhodes fifth partial (`0:40–0:44`)**: Much warmer than `v001`, though `chord_wash` still holds a noticeable `1242 Hz` partial during the quiet breakdown.


---

## v003 — Open-Air Mix, Cents-Spread Paulstretch, Low-Band/Visual Ducking & 5-Act Subterranean Montage
- **Burial output**: `outputs/261111_burial-demo/v003/261111_chromagrain_burial-demo_v003.mp4` (`72.2 s`, 1733 frames, `-14.8 LUFS`, `LRA 11.0 LU`, `-1.1 dBTP`)
- **Holdout output**: `outputs/261111_burial-demo/holdout/v003/261111_chromagrain_holdout-ambient_v003.mp4` (`32.0 s`, 768 frames, `-14.3 LUFS`, `LRA 13.5 LU`, `-1.0 dBTP`)
- **Critiques folded in from `v002` (`#19` `@critic-argon`, `#20` `@critic-opus`, `#21` `@critic`)**:
  1. **SYSTEM (`crates/core`, commit `f0bb86b`, 184 tests green)**:
     - **Critical-band cents-proportional `paulstretch` (`stretch.rs`)**: Replaced fixed 5-bin blur + hardcoded `>900 Hz` HF rolloff with energy-preserving 2-pass cents-proportional power smearing (`85` cents), melting narrow `>1 kHz` whistles while keeping natural `>6 kHz` broadband air.
     - **Low-band sidechain detector + `track_visual_duck` (`timeline.rs`, `render.rs`, `realtime.rs`, `script.rs`)**: Added `DuckSpec::key_hz` (`165 Hz` stereo 2-pole LP) so only kicks pump sub/pads while hi-hats/shakers pass untouched, plus `s.track_visual_duck(...)` for audio-free background opacity ducking.
     - **Seamless crossfade-looping `Snippet`s (`render.rs`)**: Untextured snippets longer than source duration now tile complementary `160 ms` Tukey audio passes while keeping an unbroken visual span, eliminating `7.2 s` butt-join seam blackouts.
     - **Smooth `ColorFilter` HSV ramps (`video.rs`)**: Replaced binary `sat_min`/`val_min` thresholds with smooth Hermite ramps, eliminating 1-px foam stipple in `holdout_ambient` (`0:06–0:26`).
     - **Post-LP broadband vinyl `dust` + AV `apply_video_tape` (`audio.rs`, `video.rs`, `render.rs`)**: Crisp `1.65–7.6 kHz` needle crackle placed *after* a gentler `age` LP, plus film-gate weave, warm highlight/lifted-black grading, and sparse 16mm dust specks on video.
     - **Local `Source::base_hz_at` (`timeline.rs`, `seq.rs`, `render.rs`)** for sliced/positioned `Key` pitch quantization + **separable 2D DCT + stack quickselect (`fx.rs`)** cutting 72 s render time from `>10 min` to `2m39s`.
  2. **SOURCES & PIECE (`assets/demo/` 7.29 MB, `examples/burial.rhai`, `examples/holdout_ambient.rhai`)**:
     - **Restored `3–12.5 kHz` air**: Removed `2500–3000 Hz` blanket low-passes from `vocal_bench`, `vocal_street`, `night_chords`, and `holdout_ambient`, and opened `hat_glitter` to `12.6 kHz`; dropped `t_sub` ~`4.2 dB` and high-passed `t_pad` at `210 Hz`.
     - **Eliminated 1941 orchestral bleed & daytime white car**: Removed `arteries_of_new_york_city.mp4` orchestral audio bleed (which caused the `0:48` `vb_air` spike in `v002`), replaced `vocal_street.mp4` daytime car street video with continuous 16mm NYC 7th Ave subway platform silhouettes (`126–138 s`), and added `subway_tunnel.mp4` (forward-rushing 1941 NYC dark subway tunnel + 133 BPM steel bogie rail-joint `da-CLACK` rhythm).
     - **Recurring wordless open-vowel hook**: Replaced baritone `-12 st` closed-mouth grunts with a recurring 4-bar F-minor open-vowel call-and-response (`"Ooooooh"` / falsetto `"Hoooo"`, `0, -2, -4, -5, +3 st`), laid bare in the Breakdown (`0:36–0:50`) and stuttered in Drop II (`0:51–1:04`). **Whisper ASR = 0 segments**.
- **Composer honest self-critique on `v003` (ready to combine with critics' `v003` notes for `v004`)**:
  1. **`subway_tunnel.mp4` source cut flash (`8.79 s`, `51 s`, `56 s`, `62 s`)**: `subway_tunnel.mp4` (`202.2–208.2 s`) starts on a bright between-car doorway (`0:00–0:01.6`) before entering the dark tunnel (`0:01.7–0:04.7`) and ends on bright white map lines (`0:04.8–0:06.0`), causing `8` full-frame luma reversals (`max |dY| = 70.3 @ 8.79 s`) and un-crossfaded video wrap jumps when looped. Fix in `v004`: add video loop crossfading in `render.rs` (SYSTEM) and trim `subway_tunnel.mp4` strictly to the continuous dark tunnel POV (`203.8–206.9 s`).
  2. **Central orange orb from `rain_window` still visible in Drops/Breakdown**: Dimming `rain_window` to `0.22–0.36` helped, but its static orange circle at `(0.38, 0.52)` still peeks behind commuter cutouts; fully transitioning away from the rain-glass video in Drops/Breakdown (keeping subtle vinyl/rain audio) will give true visual scene changes.
