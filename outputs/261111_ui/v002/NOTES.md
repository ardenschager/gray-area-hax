# 261111 studio UI — v002

Headless egui_kittest + wgpu renders (no window), 1× pixels, design media from
`design/v002/media` decoded with ffmpeg. Regenerate:

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261111_ui/v002 CHROMAGRAIN_UI_VERSION=v002 \
  cargo test -p chromagrain studio::tests::snapshots_every_view_at_every_size
```

| file | compare with (design/v002/shots) |
|---|---|
| `261111_ui_play_{1600x1000,1180x820,960x600}_v002.png` | `..._play_v002`, `..._play-ipad_v002`, `..._play-plugin-min_v002` |
| `261111_ui_sequence_{…}_v002.png` | `..._sequence_v002`, `..._sequence-ipad_v002`, `..._sequence-plugin-min_v002` |
| `261111_ui_perform_{…}_v002.png` | `..._perform_v002`, `..._perform-ipad_v002` |
| `261111_ui_deep-emitter_1600x1000_v002.png` | `..._deep-emitter_v002` |
| `261111_ui_sequence-hold_1600x1000_v002.png` | `..._sequence-hold_v002` |

State is the scripted "showcase" in `crates/app/src/studio/tests.rs` (emitter 1
held and spraying, the design's lanes, scenes 1–4, chain 1×2 2×2 3×4 4×2 3×2,
pads 1/5/10 playing). The canvases are the real GPU pipeline.

Changed vs v001: emitter grains at the sample end of GRAIN draw alpha-over
(not additive glow) on the backdrop, so the Play canvas is the footage, not a
washed-out double exposure; Stop-all icon is an outlined square; emitter-tile
labels only take the room right of the ring.

Known gaps: single-pointer only (no multi-touch), song chain shows 4 of 5
entries at 1600 wide, Sequence canvas is black at bar edges when nothing plays.
