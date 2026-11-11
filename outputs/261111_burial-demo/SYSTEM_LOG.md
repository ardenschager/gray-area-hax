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
