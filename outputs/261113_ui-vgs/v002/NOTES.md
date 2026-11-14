# Chromagrain VGS Studio UX, `ISOL8` 5-Band Frequency Monitor & Hardware Reskin — `v002` (`261113_ui-vgs`)

## Overview
Version `v002` transforms Chromagrain's Studio UX and `wgpu` stage compositor into a tactile, anodized-metal broadcast sampler & frequency isolator inspired by Crimson Merry's **TBProAudio ISOL8**, integrates a live 5-band `24 dB/Oct` Linkwitz-Riley frequency monitor (`ISOL8`) across both audio and GPU video, and resolves all Phase 2 wiring items across `crates/ui/`, `crates/app/`, and `crates/plugin/`.

## What Changed vs. `v001`
1. **Anodized Hardware Industrial Design (`theme.rs`, `widgets.rs`, `envelope.rs`, `studio/*`):**
   - **Chassis & Putty Plates:** Warm anodized sand/olive-putty brushed metal chassis (`#B4AC8C → #948C6E`) with top catchlight (`#CEC7AC`), dark bottom bevel (`#5A5440`), subtle micro-grain texture, 4 slotted corner rack screws, lighter brushed-putty inset plates (`#B8B19A`), and dark silkscreened uppercase typography (`#231F16`).
   - **Recessed Bakelite / Broadcast Monitor Bezels:** Sunken `#16191A` CRT wells with inner shadow and bottom catchlight framing the Video Stage, 8×8 Clip Grid, Chopper Waveform, ADSR Oscilloscope, Emitter Cards, Sequence Step Wells, and 4×4 Perform Pads — keeping footage and waveforms crisp inside dark bezels.
   - **Machined Pointer Knobs & Sunken Silver LCDs:** Physical charcoal cylindrical pointer knobs (`#343535 → #202121` cap, `#181919` skirt, directional drop shadow, high-contrast white radial pointer `#F4F2EC`, silkscreened calibration ticks) paired with sunken brushed-silver LCD value readouts (`#A29F97`).
   - **Backlit Translucent Hardware Switches (GPU Radial Lens Mesh):**
     - **Incandescent Amber-Gold** (`#FFFDD4 → #FCE35D → #CB9D33` lit; smoked `#7D6A38` unlit) for `SOLO`, Sculpt Stamps, VJ Punch-In FX, and primary actions.
     - **Glowing Ruby** (`#FFD6C9 → #EC5647 → #A42C22` lit; smoked `#7B3B34` unlit) for `MUTE` and `REC`.
     - **Glowing Mint-Teal / Console Putty** (`#CCFFE8 → #63D9A7 → #388463` lit; raised `#AFAA9D` putty cap unlit) for `MONITOR` (`L · R · M · S`), segmented switches, and owner tabs.
     - **Charcoal Sunken Display Slots & Jewel LEDs:** `#27251F` slots (`STEREO`, `24 dB/Oct`), ruby `FILTER BYPASS` jewel button, and emerald `DIM` jewel LED.
2. **5-Band `24 dB/Oct` `ISOL8` Frequency Monitor & GPU Shader (`studio/mod.rs`, `gpu.rs`):**
   - Dedicated `ISOL8 · 5-BAND FREQUENCY MONITOR` rack bay in `Perform` (and `Sheet::Project`) wired live to `BandIsolator` (`X1..X4` `200 · 700 · 2500 · 8000 Hz` knobs + Hz LCDs, `FILTER BYPASS`, `5×2` `LF · LMF · MF · HMF · HF` Ruby `MUTE` / Amber `SOLO` matrix, `CH. MODE` `STEREO | MID | SIDE` + `24 dB/Oct`, `MONITOR` `L · R · M · S`, `OUTPUT` knob, and `ATTEN.` `DIM -18.0` putty plate) in both standalone `App` and VST3/CLAP `PluginHost`.
   - 1:1 WGSL spatial-frequency & Bernstein tonal partition-of-unity shaders (`fs_iso_blur_h`, `fs_iso_blur_v`, `fs_iso_combine`, `fs_isolator`) matching `core::fx::apply_video_isolator` within `< 1.15 / 255` mean absolute diff across `LF`, `LMF/MF/HMF Mid`, and `HF` edge solo modes.
3. **End-to-End Wiring Fixes (`ui_perform.rs`, `studio_host.rs`, `ui_host.rs`):**
   - **Free-Loop Pad Horizon:** Unsynced free-running tape-loop pads (`Loops → pads`) stay a single unbroken `Clip` across a `>= 64`-beat (`>= 6` loop passes) horizon in `Perform`, allowing `loop_pass` and Basinski `disintegrate` wear to advance continuously across passes.
   - **Per-Pad Sculpt & Captions:** Added `PerformHost::{pad_sculpt, set_pad_sculpt}` routed cleanly to pad tracks in both `Arrange` and `Perform` (and `Live::Pad` in `PluginHost`), wired `pitch_semitones` into live Emitter hits, and synced `Slice::caption` + smart `(cx, cy)` focus in `Kit → pads`.
   - **Plugin Stage Layout Persistence:** `PluginHost::sync` preserves `StageMode::Split` and `StageMode::Bleed` (`Layout::Split` / `Layout::Bleed`) across host parameter ticks.

## Rendered Snapshots (`v002`)
- `261113_chromagrain_vgs_play_1600x1000_v002.png` & `261113_chromagrain_vgs_play_960x600_v002.png`
- `261113_chromagrain_vgs_add-youtube-search_1600x1000_v002.png`
- `261113_chromagrain_vgs_add-dig-kit_1600x1000_v002.png`
- `261113_chromagrain_vgs_add-drive_1600x1000_v002.png`
- `261113_chromagrain_vgs_clip-sculpt-tape_1600x1000_v002.png`, `1180x820`, `960x600`
- `261113_chromagrain_vgs_sequence-ratchet-set_1600x1000_v002.png`
- `261113_chromagrain_vgs_perform-split-bleed-9x16_1600x1000_v002.png` (`ISOL8` Vocal `LMF/MF/HMF SOLO` + `M` Monitor)
- `261113_chromagrain_vgs_perform-bleed-16x9_1600x1000_v002.png` & `261113_chromagrain_vgs_perform-isol8_960x600_v002.png` (`ISOL8` `LF/LMF SOLO` + `HF MUTE` + `DIM`)
- `261113_chromagrain_vgs_plugin-play_960x600_v002.png`, `plugin-clip_960x600_v002.png`, `plugin-perform_960x600_v002.png`
