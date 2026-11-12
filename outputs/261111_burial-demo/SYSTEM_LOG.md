# Chromagrain Engine Hill-Climb Log (`SYSTEM_LOG.md`)

Tracks general engine (`crates/core`) improvements motivated by multi-model critiques (`@critic`, `@critic-argon`, `@critic-opus`), Arden's direct notes (`#12`, `#14`), and self-critique on `examples/burial.rhai`, guarded against overfitting via `examples/holdout_ambient.rhai`.

---

## Baseline (`v001`)
- Stock `chromagrain-core` prior to system changes (130 tests passing; 170 after merging `overhaul`).
- Measured baseline issues exposed by `v001` (`burial` & `holdout`):
  - **Strobe A (13.3 Hz OLA hop opacity ripple on stretched snippets)**: `496` full-frame 1-frame luma reversals (`>3` luma) in `burial v001`; `294` in `holdout v001`.
  - **Strobe B (0 ms percussive audio-envelope camera-flash strobe + broken straight-alpha additive blending)**: `380` center-ROI 1-frame reversals (`max |dY| = 54.13`, `p99 |dY| = 35.94`) in `burial v001`; `206` in `holdout v001`.
  - **Squeakiness / upper-mid glare**: `hi/lo (1.5–6k / 150–800 Hz) = -11.1 dB` (worst 2s `-1.4 dB`) in `burial v001`; `-13.1 dB` + `+28.9 dB` narrow tonal whistle in `holdout v001`; `+1.0 dBTP` / `+0.6 dBTP` inter-sample clipping and `soft_clip` step discontinuity (`0.952 -> 0.762` at `|s|=1.0`).

---

## v002 — Anti-Strobe, Anti-Squeak, Groove, Sidechain, Tape & Slicing Upgrade (`commit c350ce5`)
**Test suite**: `181` tests passing (`155` `chromagrain-core` unit + `4` integration + `22` `chromagrain` app/wgpu parity tests, `0` failures).

### 1. Continuous Blended Snippet Video & WSOLA Audio Stretch (`render.rs`, `video.rs`)
- **Motivating critique**: Arden (`#14` *"too much strobing"*), `@critic-opus` (`#16` forensic proof of `13.3 Hz` Porter-Duff `1.0 <-> 0.75` ripple), `@critic-argon` (`#9`, `#15`), `@critic` (`#11`, `#17`).
- **System change**:
  - Untextured `ClipKind::Snippet` (`texture == 0`, `speed != pitch_ratio`) now renders video via a single unbroken pass (`ClipPlan::video_parts`) advancing smoothly at `speed` with temporal neighbor-frame crossfading (`VideoClip::frame_blended_at`), never stacking 50%-overlap Hann audio grains in straight-alpha space.
  - Added `GrainSettings::varispeed` (`varispeed` / `tape_pitch`) coupling `speed` to `pitch_ratio` as a single pristine Hermite grain with zero OLA windowing.
  - Upgraded non-varispeed snippet audio time-stretch to 200 ms Hann windows with `±8 ms` WSOLA waveform cross-correlation alignment (`wsola_align`), eliminating 13 Hz comb warble.
- **Before -> After evidence**:
  - `burial` full-frame 1-frame luma reversals (`>3`): **`496 -> 0`** (`mean |dY|: 3.82 -> 1.34`).
  - `holdout` full-frame 1-frame luma reversals (`>3`): **`294 -> 0`** (`mean |dY|: 4.47 -> 0.99`).

### 2. Visual Transient Smoothing (`flicker`), Optional `feather`, True Straight-Alpha Additive Glow & Cutout Focus (`video.rs`, `render.rs`, `timeline.rs`)
- **Motivating critique**: Arden (`#14` keep lo-fi visual vibe, kill strobing), `@critic` (`#17` Strobe B center-ROI `+54` jumps), `@critic-argon` (`#15`), `@critic-opus` (`#10`, `#16`).
- **System change**:
  - Added `VisualGrainStyle::flicker` (`0..1`, default `0.30` on clouds/patterns, `1.0` on `full_frame` snippets): softens percussive `env_skew` visual attack and gives sub-220 ms audio hits a smooth multi-frame visual decay tail while preserving crisp 1 ms audio transients.
  - Added `VisualGrainStyle::feather` (`0..1`, default `0.0` preserving crisp lo-fi grain blocks unless requested).
  - Fixed double-premultiplied darkening bug in `composite_grains_frame` and `blend_layer_over` (`((dd*da + s*a) / out_a).min(255.0)`) and wired `Track::additive` (`s.track_blend`) into `render_project` (defaulting `Clip::new_snippet` to `0.0` so z-order & wgpu parity tests hold while granular/pattern tracks genuinely glow over beds).
  - Added `Frame::alpha_focus` biasing sub-frame grain patches toward segmented foreground cutouts (`0.5%..80%` opaque).
- **Before -> After evidence**:
  - `burial` center-ROI 1-frame reversals (`>3`): **`380 -> 2`** (`max |dY|: 54.13 -> 27.69`, `p99 |dY|: 35.94 -> 17.89`).
  - `holdout` center-ROI 1-frame reversals (`>3`): **`206 -> 1`** (`max |dY|: 20.57 -> 8.55`, `p99 |dY|: 12.57 -> 6.57`).
  - Lowered `AvLink::default().pitch_to_hue` from `12.0` to `2.0`, restoring natural forest/cyan hues in `holdout v002`.

### 3. Pitch Anti-Aliasing, Overlap/Pitch Gain Compensation, Paulstretch Smoothing, 8-Comb Freeverb & C1 Master Limiter (`audio.rs`, `grain.rs`, `stretch.rs`, `fx.rs`)
- **Motivating critique**: Arden (`#14` *"sound is a bit squeaky and high-pitched"*), `@critic-opus` (`#16` `soft_clip` step discontinuity + paulstretch sine spikes + upward pitch aliasing), `@critic` (`#17` Freeverb 4-comb ringing in `holdout`).
- **System change**:
  - `sample_pitched` (`audio.rs`): stateless 5-tap symmetric FIR anti-aliasing & formant-warming kernel when `pitch_ratio > 1.05`.
  - `schedule_grains_auto` (`grain.rs`): mild overlap normalization (`overlap > 2.5`) + upward-pitch equal-loudness warmth (`ratio^-0.35` for `ratio > 1.0`).
  - `paulstretch_channel` (`stretch.rs`): zeroed `<20 Hz` DC/subsonic bins, added 5-bin triangular spectral smoothing above `750 Hz` + gentle HF warmth rolloff above `900 Hz`.
  - `ReverbChannel` (`fx.rs`): upgraded from 4 combs + 2 allpasses to canonical 8 combs + 4 allpasses with 160 Hz tank HPF and natural HF damping floor.
  - `MasterConditioner` / `soft_limit_sample` (`audio.rs`, `realtime.rs`): replaced discontinuous `soft_clip` with C1-continuous soft-knee ceiling at `-1.0 dBFS` (`0.891`), exact-identity below `0.65` and sample-exact between offline and `RealtimeAudio`.
- **Before -> After evidence**:
  - `burial`: `I = -9.3 -> -14.2 LUFS`, `TruePeak = +1.0 -> -1.1 dBTP`, `LRA = 8.4 -> 10.9 LU`, `hi/lo = -11.1 -> -22.0 dB` (worst 2s `-1.4 -> -13.5 dB`).
  - `holdout`: `I = -8.6 -> -13.6 LUFS`, `TruePeak = +0.6 -> -1.0 dBTP`, `hi/lo = -13.1 -> -23.4 dB`, max tonal prominence `>1.2 kHz = +28.9 -> +20.2 dB` (mean `+20.0 -> +13.0 dB`).

### 4. Groove (`swing`, `offset`, `humanize`), Z-Independent Sidechain Ducking, Analog Tape/Vinyl Stage, Transient Slicer & Segment Cache (`seq.rs`, `timeline.rs`, `render.rs`, `realtime.rs`, `media.rs`, `script.rs`)
- **Motivating critique**: Consensus `#9`, `#10`, `#11` (rigid 16th drums, sub/kick masking, lack of wow/flutter & slice chopping).
- **System change**:
  - `StepPattern::swing`, `SeqRow::{swing, humanize}`, `Step::offset`, `GrainSettings::swing` (exposed in Rhai + automatable).
  - Two-pass z-order-independent `Track::duck` (`DuckSpec` + `DuckState`) ducking both audio and video opacity (`duck_to_opacity`), sample-exact in `RealtimeAudio`.
  - Streaming-safe `TapeSettings` / `TapeState` (`wow`, `warmth`, `age`, `dust`) per-track and on master (`track_tape`, `master_tape`).
  - `AudioClip::detect_slices` (`s.slices`), `s.slice`, `s.pair`, `s.synth_source`, `256k` AAC encode, and FNV-1a mask caching in `media::segment_video`.

### Candidate SYSTEM Improvement Queued for `v003` (Discovered Profiling `v002` Render)
- **8x8 DCT `Compress` allocation bottleneck + single-threaded video render (`fx.rs`, `render.rs`)**: Stack-sampling `v002` showed >80% of wall-clock render time spent heap-allocating & sorting 64-element `Vec`s per 8x8 block in `fx::video_compress` (`6.2M` heap allocations per 72 s video!). Replacing `Vec` sort with a stack `[f32; 64]` `select_nth_unstable_by` makes `Compress` ~8–10× faster for every project.


---

## v004 — Chromatic Vibrato Pitch Lock, Vision Cutout Precision & `alpha_bounds`, Seamless Video Loop Crossfade, Peaking EQ, Audio-Reactive Opacity/Pulse & `step_every` (`commit 6049d26`)
**Test suite**: `189` tests passing (`163` `chromagrain-core` unit + `4` integration + `22` `chromagrain` app/wgpu parity tests, `0` failures).

### 1. Multi-Window Chromatic `detect_pitch` + `0.60 s` `Source::base_hz_at` + `Source::slice` Base Inheritance (`dsp.rs`, `timeline.rs`)
- **Motivating critique**: `@critic-opus` (`#23` Item 1) & `@critic-argon` (`#25` Item 1.2): `base_hz_at` used a `0.26 s` window (`< 0.27 s` required by `detect_pitch`, making it a no-op), while single-window `detect_pitch` on vibrato/melismatic vocals landed ~45 cents off A=440, causing `Key` quantization (plus `quantize_amount < 1.0` partial-snap) to detune sustained vocals by `-45c / +23c`.
- **System change**: Upgraded `detect_pitch` to evaluate up to 13 hopped `0.18 s` windows, preserving exact sub-cent frequency on stationary tones while stabilizing vibrato/melismatic sources (`spread > 12 cents`) to their nearest A=440 chromatic semitone; widened `Source::base_hz_at` to `0.60 s` locked to parent chromatic tuning; inherited `base_hz_at` in `Source::slice`.
- **Before -> After evidence**:
  - Verified by unit test `vibrato_pitch_detection_stabilizes_to_a440_semitone` (`< 3 cents` error on asymmetric vibrato/glide).
  - 65k-point Welch intonation scan on `burial`: `v003` `Bb4 (-45c), Bb4 (+23c), F4 (+26c)` -> **`v004` `F5 (-1c), G5 (-2c), G4 (+9c), Ab5 (-3c)`**; `holdout`: `v003` `F4 (+26c)` -> **`v004` `F5 (+2c), Eb4 (+1c), Eb5 (-1c)`**.

### 2. `PersonSegmenter` Background-Slab Rejection + `Frame::alpha_bounds` + Nocturnal `shadow` Crush (`tools/segment_vision.swift`, `media.rs`, `video.rs`)
- **Motivating critique**: `@critic-argon` (`#25` Bug A) & `@critic` (`#24`): `VNGenerateForegroundInstanceMaskRequest` (`keepThreshold = 0.15`) unioned the `58.8%`-area subway bench/wall slab into `vbench_cut`, while `video.rs` sub-patch cropping guillotined cutouts into square boxes and bright daytime luminance looked like pasted stickers.
- **System change**: Added instance precision (`>= 0.55`) and area (`<= 1.4x personPixels`, `< 35%` frame) guards in `PersonSegmenter.instancesOverlappingEvidence` (`v2` cache key); added `Frame::alpha_bounds` so cutout grains frame the entire padded silhouette intact; added alpha-preserving `VisualGrainStyle::shadow` (`0..1`).
- **Before -> After evidence**: Verified by `alpha_bounds_frames_entire_segmented_cutout` and visual inspection of `burial v004` (`0:28, 0:37, 0:41, 0:54`): rectangular bench/wall halo completely eliminated, silhouettes grounded cleanly at bottom canvas.

### 3. Seamless Cosine RGB Video Loop Crossfade + Snippet Entry Ramp (`video.rs`, `render.rs`)
- **Motivating critique**: Unanimous `#23, #24, #25`: hard video wrap jump-cuts (`-51.7, -46.1` luma) and `8.79 s` (`+70.3 / -98.9` luma) doorway cut flash.
- **System change**: Added `VideoClip::frame_looped_at` overlapping `[D - X .. D]` with `[0 .. X]` in RGB at full `alpha = 255`, wired `wraps_video` in `snippet_video_events`, and enforced `>= 3.5`-frame visual fade ramps on full-frame snippets.
- **Before -> After evidence**: Verified by `video_loop_crossfade_wraps_without_jump_or_alpha_dip`; `burial` full-frame `max |dY|` dropped **`70.31 -> 10.63`** (center-ROI **`98.92 -> 14.77`**); `holdout v004` achieved **`0` full-frame AND `0` center-ROI reversals** (`max |dY| = 4.17`).

### 4. Audio-Reactive `Track::rms_to_opacity` & Kick-Pulse `duck_to_opacity`, Parametric Peaking `EqBand`, and `Step::every_n` (`dsp.rs`, `seq.rs`, `timeline.rs`, `render.rs`, `realtime.rs`, `script.rs`)
- **Motivating critique**: Unanimous `#23, #24, #25`: low onset<->visual sync (`0.08–0.14`), `220–500 Hz` boxiness tying `<90 Hz` sub, and mechanical 2-bar downbeat vocal repetition.
- **System change**: Added per-frame smoothed audio-envelope opacity coupling (`s.track_rms_opacity`), signed `duck_to_opacity` / `s.track_visual_pulse` (`-1..1`), RBJ peaking bell EQ (`s.clip_eq`, `s.track_eq` with `RealtimeAudio` streaming parity), and pass-modulo `s.step_every(pat, row, step, every_n, offset)`.
- **Before -> After evidence**:
  - Verified by `peaking_eq_cuts_and_boosts_target_band`, `step_every_fires_only_on_matching_passes`, and `realtime_stream_matches_offline_render`.
  - `burial` `Sub(<90 Hz)` vs `Box(220–500 Hz)`: `v003` `-22.9 vs -22.9 dB` (`0.0 dB` separation) -> **`v004` `-21.6 vs -29.6 dB` (`+8.0 dB` clean sub authority)**.
  - AV correspondence: `burial` `onset <-> |dY|` **`+0.078 -> +0.261`** (`3.3x`), `onset <-> motion` **`+0.157 -> +0.246`**; `holdout` `onset <-> motion` **`+0.294 -> +0.415`**, `onset <-> |dY|` **`+0.077 -> +0.284`**, `rms <-> luma` **`+0.871`**.


---

## v005 / v006 — Symmetric Scatter `paulstretch` Tuning Fix, Rim-Lit Cutout Silhouettes, `master_gain` & Slew-Limited `master_av_bloom` (`commit 41204de`)
**Test suite**: `191` tests passing (`165` `chromagrain-core` unit + `4` integration + `22` `chromagrain` app/wgpu parity tests, `0` failures).

### 1. Symmetric `O(N)` Difference-Array Scatter in `paulstretch` (`stretch.rs`)
- **Motivating critique**: `@critic-opus` (`#28` Item 1: `641.6 Hz = E5 -48c` in Breakdown `36–50 s`) & `@critic` (`#29` Item 1: pinpointed variable-width gather box `blur_power_pass` in `stretch.rs:115-128` asymmetrically pulling `Eb5 = 622.25 Hz` upward by `+51 cents` where `dk(k)` grows).
- **System change**: Replaced variable-width gather averaging with symmetric difference-array scattering (`scatter_power_pass`), distributing each bin `j`'s power `src[j]` symmetrically across `[j - dj, j + dj]` with `dj = 0` below `950 Hz` and ramping across `950..1550 Hz`.
- **Before -> After evidence**:
  - Locked by unit test `stretch_preserves_eb5_harmonic_tuning_within_3_cents` (`< 3 cents` error on `622.254 Hz` `6x` stretch) alongside `stretch_smears_upper_whistle_while_preserving_hf_air`.
  - Breakdown (`36–50 s`) 65k Welch scan: `v004` `641.6 Hz (E5 -48c)` -> **`v006` `G5 (-1c), G4 (+3c), F4 (+4c), Eb4 (-3c), Eb5 (621.1 Hz -3c)`**.

### 2. Alpha-Contour Rim Highlight & Exposure Floor in `VisualGrainStyle::shadow` (`video.rs`)
- **Motivating critique**: `@critic-opus` (`#28` Item 2 & Top System #3) & `@critic` (`#29` Item 2): uniform shadow darkening made cutouts disappear into dark Breakdown beds.
- **System change**: `VisualGrainStyle::shadow` now computes a 2-px alpha-edge contour gradient (`ta - min_neighbor_alpha`) + warm rim boost and midtone exposure floor while preserving cutout alpha.
- **Before -> After evidence**: Breakdown human portraits (`0:37–0:48` in `burial v006`) read with clear, luminous figure-ground separation against glowing nocturnal water.

### 3. `Project::master_gain_db` (`s.master_gain`) + Slew-Limited `Project::master_bloom` (`s.master_av_bloom`) & Reproducible AV-Sync Metrics (`timeline.rs`, `render.rs`, `realtime.rs`, `script.rs`)
- **Motivating critique**: `@critic-opus` (`#28` Item 3 & AV-sync metric note) & `@critic` (`#29` Item 3).
- **System change**:
  - `master_gain_db`: post-tape, pre-`MasterConditioner` dB trim with exact offline/realtime streaming parity (`burial`: `-17.3 -> -14.6 LUFS / -0.8 dBTP`; `holdout`: `-15.3 -> -14.3 LUFS / -1.3 dBTP`).
  - `master_bloom` + 4-frame `track_duck_shaped` release smoothing: couples positive frame log-RMS onset steps (`(log_rms[t] - log_rms[t-1]).max(0)`) into whole-frame luminance lift with downward slew capped at `<= 1.40 * amount` luma/frame, locked by regression test `master_av_bloom_correlates_audio_onsets_with_frame_changes` (`corr > 0.38`, `0` 1-frame reversals).
  - **Documented AV-Sync Metrics in `eval_render.py`**:
    1. *3-frame (~125 ms) positive RMS-onset envelope vs whole-frame `|I_t - I_{t-1}|` (`motion`) and `|dY_t|`*: `burial v006` `r = +0.251` (`motion`), `+0.323` (`|dY|`); `holdout v006` `r = +0.449` (`motion`), `+0.367` (`|dY|`), `rms <-> luma r = +0.871`.
    2. *Unfiltered 1-frame band log-flux `max(0, diff(log(band_rms)))` vs whole-frame `|I_t - I_{t-1}|` (`@critic-opus` `scratch/avsync.py`, lags `-3..+3`)*: `burial v006` `low<120 = +0.098`, `mid300-3k = +0.118 (@+3)`, `high>5k = +0.107 (@+1)` (moved positive from near-zero/negative while avoiding whole-screen camera-flash strobing over continuous motion footage).


### Open Opportunities Logged After `v006` Sign-Off (`#32`)
- **Deeper per-pixel AV motion coupling**: While 3-frame RMS-onset <-> `|dY|` / `motion` reaches `r = +0.32..+0.45` (`rms <-> luma = +0.87`), unfiltered 1-frame band log-flux vs whole-frame `tblend` over moving camera beds stays modest (`~0.10`) because exposure modulation is capped below strobe thresholds (`<= 1.4` luma/frame release). Future engine work could couple transients to spatial displacement / optical-flow warp (`|I_t - I_{t-1}|`) rather than luminance alone.
- **Per-chord harmonic tracking inside `paulstretch`**: Multi-chord source clips (`night_chords.mp4`) carry subtle natural electric-piano inharmonicity on individual chord voicings (`~20c` on `F4` in bars 1–3).
