# Burial AV Demo & Holdout Benchmark — Iteration Notes (`261111_burial-demo`)

- **Benchmark piece**: [examples/burial.rhai](../../../examples/burial.rhai) — *"Half Past Three"*, 71.6 s (160 beats @ 134 BPM, F minor)
- **Held-out sketch**: [examples/holdout_ambient.rhai](../../../examples/holdout_ambient.rhai) — 32.0 s (32 beats @ 60 BPM, D dorian)
- **System log**: [SYSTEM_LOG.md](./SYSTEM_LOG.md)
- **Media attributions**: [assets/demo/ATTRIBUTIONS.md](../../../assets/demo/ATTRIBUTIONS.md)

---

## v001 — Stock Engine Baseline
- **Burial output**: `outputs/261111_burial-demo/v001/261111_chromagrain_burial-demo_v001.mp4` (71.6 s, 1720 frames, `-9.3 LUFS` integrated, `LRA 8.4 LU`, True Peak `+1.0 dBFS`)
- **Holdout output**: `outputs/261111_burial-demo/holdout/v001/261111_chromagrain_holdout-ambient_v001.mp4` (32.0 s, 768 frames, `-8.6 LUFS` integrated, `LRA 13.4 LU`, True Peak `+0.6 dBFS`)
- **Engine state**: Unmodified `overhaul` baseline prior to SYSTEM improvements (130 tests passing).
- **Curated AV sample palette (`assets/demo/`, 7.48 MB total)**:
  - `rain_window.mp4` (raindrops on dark glass w/ sodium bokeh + stereo rain & vinyl dust/pops)
  - `night_chords.mp4` (nocturnal amber/cyan reflections + `gm` Lyria drumless Fm9/DbMaj7 Rhodes & pad chords, paulstretched 6×)
  - `sub_highway.mp4` (headlights on wet night highway + F1 43.65 Hz sine sub & low-passed F2 stereo Reese)
  - `kick_tunnel.mp4`, `rim_train.mp4`, `hat_glitter.mp4` (4-slice perc strip: closed hat, open hat, shaker/casing, woodblock/turnstile)
  - `vocal_bench.mp4` (7th Ave solitary bench woman + `gm` Lyria solo female R&B acapella phrases, segmented via Apple Vision)
  - `vocal_street.mp4` (SF street profile + `gm` Lyria falsetto ad-libs & `gm` Gemini TTS late-night UK whispers, segmented via Apple Vision)
  - `holdout_forest.mp4`, `holdout_waterfall.mp4`, `holdout_ocean.mp4` (`gm` Lyria Nordic chamber cello/harp/choir + nature footage)
- **Composer self-critique after watching & listening to v001**:
  - *What works*: Intro atmosphere (`0:00–0:14`), Breakdown emotion (`0:36–0:49`, *"Watch the clock, it's half past three..."* over segmented bench silhouette & rain reflections), and Outro fade (`1:04–1:11`).
  - *SYSTEM issues exposed by v001*:
    1. **Hard rectangular visual patches (`video.rs`)**: Sub-frame grains (`0:14–0:35`, `0:50–1:03` in burial; `0:07–0:21` in holdout) draw sharp 90° rectangular boxes with zero edge feathering.
    2. **Track bus compositing ignores additive blend (`render.rs:484`)**: `blend_layer(frame, &tl, 0.0, opacity)` hardcodes `additive = 0.0` across tracks, so dark background pixels of `sub_highway` and `kick_tunnel` paint opaque black/grey rectangles over the rainy window & pad layers underneath.
    3. **Straight-quantized step sequencer (`seq.rs`)**: Zero swing/shuffle or per-step micro-timing offset makes the 2-step UKG drum groove stiff (`0:14–0:35`) rather than stumbled/shuffled.
    4. **Fixed-hop snippet stretch comb-filtering (`render.rs:snippet_events`)**: Setting `speed = 0.92` with `pitch = 0.0` triggers fixed 150 ms / 75 ms OLA grains without phase alignment, causing audible 13 Hz chorus/comb warble on bare vocals (`0:04–0:07`, `0:38–0:44`).
    5. **Hot grain-overlap summing, `+1.0 dBFS` true peak, & missing sidechain ducking (`grain.rs`, `audio.rs`, `render.rs`)**: Dense clouds lack overlap gain normalization, `soft_clip` allows `+0.6..+1.0 dBFS` AAC true peaks, and pads/sub cannot sidechain-duck under the kick.
