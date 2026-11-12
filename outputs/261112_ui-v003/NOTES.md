# 261112 ui v003: dig, chop, ADSR, tile (UI side)

These are headless renders (egui_kittest + wgpu, 1× pixels) of the v003 studio features on branch
`feat-ui-v003`. The canvas is the real GPU pipeline. The media is the design's 7 archival clips,
decoded with ffmpeg.

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261112_ui-v003/v00N CHROMAGRAIN_UI_VERSION=v00N \
  cargo test -p chromagrain v003_snapshots                      # app
CHROMAGRAIN_UI_SHOTS=... cargo test -p chromagrain-plugin --test editor plugin_chop   # plugin
CHROMAGRAIN_PARITY_DIR=$PWD/outputs/.../parity cargo test -p chromagrain-ui --lib tiles_and_envelopes
```

## Files (each one at 1600×1000, 1180×820 and 960×600 unless noted)

| file | what |
|---|---|
| `261112_ui_add-{files,browse,link,dig}_*` | the Add sheet, one per tab. **Browse** reads a fixture library (`CHROMAGRAIN_BROWSE_ROOT` = design/v001/media). **Dig** shows two placeholders arriving (a dig still searching, a download at 42 %) and a dug card with provenance (uploader · date · views · CC) plus Chop |
| `261112_ui_play-arriving_*` | the grid while a dig lands in column 8: a full-height placeholder with a spade glyph and pulsing dots |
| `261112_ui_chop_*` | the clip sheet for a grid column: waveform + filmstrip, 8 slices as alternating bands, edge grips, Auto/4/8/16, Tap, Clear, Slices → pads |
| `261112_ui_pad-sheet_*` | a pad's sheet: chop, plus the A/D/S/R envelope over the clip's thumbnail (Fade: the picture follows the curve) |
| `261112_ui_play-tile_*`, `261112_ui_perform-tile_*` | the `stack | tile` switch on the stage, set to tile |
| `261112_ui_deep-emitter-adsr_1600x1000_*` | an emitter's envelope on its Deep page (the panel stacks when narrow) |
| `261112_ui_lane-sheet_1600x1000_*` | a lane's sheet (chop + envelope + "Lane settings") |
| `261112_plugin_editor_{perform-slices,pad-sheet}_1180x820_*` | the plugin editor: slices → the instrument's pads (named "sine · N"), and a pad's envelope |
| `parity/parity_{tile2,tile3,adsr-fade,adsr-cut}_{gpu,cpu}.png` | GPU vs CPU-export frames for the tile layout and the ADSR picture weight |

## Versions

- **v001**: first full pass.
- **v002**:
  - The envelope panel stacks vertically when narrow, so the Deep page no longer clips.
  - Placeholders get a loading glyph and pulsing dots.
  - Browse snapshots read a real fixture library.
  - Adds the plugin editor shots.
- **v004**: merged the core's tile fix (`fix-tile-mute` 59d66e2). Muted pad tracks no longer take tile slots, so 3 playing pads give a clean 2×2 grid; `perform-tile` was 3×3 with holes in v003. No UI code changed.
- **v003**:
  - The GPU tile pass lands, mirroring the core compositor. Tile shots now show real tiles.
  - Slice pads in the plugin are named.
  - Adds the GPU/CPU parity dumps.

## GPU ↔ CPU parity (mean |Δ| on 0..255)

tile2 0.43 · tile3 0.74 · adsr-fade 1.49 · adsr-cut 1.09. These are asserted < 3 in
`gpu::tests::gpu_matches_the_cpu_for_tiles_and_envelopes`. The existing effect parity test
is unchanged.
