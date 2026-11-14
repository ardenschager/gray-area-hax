# 261114_videograin — `v006` Release Notes

## Summary of Changes vs `v005`
1. **Zero Mid-Groove Black Drop-Frames (`crates/core/src/render.rs`)**:
   - Sample-and-holds `last_lit_bg` and `last_lit_full` (`<= 1.25s`) across staccato `VoxPerc` and breakdown gaps during active tiled song spans (`Tile`, `Split`, `Collage`) — `0` pure-black or near-black frames across all 4 `60fps` `v006` MP4s.
2. **Zero Twin Same-Video Panels & Zero Empty Grid Cells (`crates/core/src/tile.rs`, `crates/core/src/render.rs`, `crates/core/src/video.rs`)**:
   - Added `TileAssigner::force_share_slot` + per-frame surjective `dedup_frame_grids_by_video`, propagated `collage_cutout` onto `Snippet` / `has_alpha()` clip plans, and routed alpha cutouts to foreground stickers across `Collage` and `Split` stages so every active cell is 100% populated and non-ratchet (`cell_salt == 0`) voices sharing a video clip never open duplicate panels.
3. **1080p Alpha Dust Island Despeckle + `"subject"` Foreground Mask + Disco Hat Air (`crates/core/src/video.rs`, `crates/core/src/seq.rs`, `crates/core/src/media.rs`, `crates/core/tests/ukg_v004.rs`)**:
   - Replaced `3x3` crumb filter with `< 28` alpha threshold + 4-connected BFS island despeckle (`min_keep = (max_area / 50).clamp(24, 950)`) before `3px` `#F2EDE0` Euclidean sticker border dilation.
   - Switched `encab_cut` to `"subject"` so the `$750,000,000` sign in the doctor's hands stays un-notched.
   - Upgraded `put_vox_steps` slice-pool selection and `PatternPreset::DiscoFilterPump` `Hat`/`VoxPerc`/`Snare` top-end presence (`hi(>3.5kHz)` lifted from `1.19%` to `3.75%`), plus tightened `encode_video_stream` 4x-oversampled true-peak ceiling (`0.640` -> `<= -1.9 dBTP` post-AAC).
4. **Optional Video-to-Chord & Cathedral Ambient Wash Engine (`crates/core/src/grain.rs`, `crates/core/src/chop.rs`, `crates/core/src/seq.rs`, `crates/core/src/timeline.rs`)**:
   - `VoiceSculpt::preset(Stamp::Wash)` + `GrainSettings::apply_stamp(Stamp::Wash)` (`ChordVoicing::Min9`, scale-locked Karplus-Strong `comb_mix = 0.52`, `+12st` `shimmer = 0.28`, warm `lpf_hz = 3400 Hz`, `PictureCut::Fade` overlapping Hann/Tukey swell).
   - `chop::build_chord_washes_across_sources(sources, key)` curating a 16-pad scale-locked ambient chord & free-running cathedral drone bank (`i min9 -> VI maj7 -> iv min9 -> v sus4 -> III maj7 -> VII fifths`) from any pitched or spoken YouTube clips (`envelope_to_opacity = 0.85`).
   - `PatternPreset::AmbientWash` (`"Wash"`, `68 BPM`, `swing = 0.50`, 8th preset in `PatternPreset::ALL`).

## `v006` Deliverables (`outputs/261114_videograin/v006/out/`)
- `261114_videograin_flowbee-zeon_ukg-sticker-collage_v006.mp4` (`1080x1920 @ 60fps`, `19.70s`, `-15.0 LUFS`, `-2.7 dBTP`, `0` clips, `0` black frames)
- `261114_videograin_bid-stutter_vox-percussion-grid_v006.mp4` (`1080x1080 @ 60fps`, `19.41s`, `-14.9 LUFS`, `-1.9 dBTP`, `0` clips, `0` black frames)
- `261114_videograin_disco-pump_auto-kit-four-section_v006.mp4` (`1920x1080 @ 60fps`, `19.36s`, `-15.9 LUFS`, `-1.9 dBTP`, `0` clips, `0` black frames)
- `261114_videograin_zeon-cathedral_ambient-chord-wash_v006.mp4` (`1920x1080 @ 60fps`, `21.18s`, `-14.9 LUFS`, `-3.8 dBTP`, `0` clips, `0` black frames)
