# 261111 studio UI — v004

Headless egui_kittest + wgpu renders (no window), 1× pixels, design media from
`design/v004/media` decoded with ffmpeg. Regenerate:

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261111_ui/v004 CHROMAGRAIN_UI_VERSION=v004 \
  cargo test -p chromagrain studio::tests::snapshots_every_view_at_every_size
```

| file | compare with (design/v004/shots) |
|---|---|
| `261111_ui_play_{1600x1000,1180x820,960x600}_v004.png` | `..._play_v004`, `..._play-ipad_v004`, `..._play-plugin-min_v004` |
| `261111_ui_sequence_{…}_v004.png` | `..._sequence_v004`, `..._sequence-ipad_v004`, `..._sequence-plugin-min_v004` |
| `261111_ui_perform_{…}_v004.png` | `..._perform_v004`, `..._perform-ipad_v004` |
| `261111_ui_deep-emitter_1600x1000_v004.png` | `..._deep-emitter_v004` |
| `261111_ui_sequence-hold_1600x1000_v004.png` | `..._sequence-hold_v004` |

State is the scripted "showcase" in `crates/app/src/studio/tests.rs` (emitter 1
held and spraying, the design's lanes, scenes 1–4, chain 1×2 2×2 3×4 4×2 3×2,
pads 1/5/10 playing). The canvases are the real GPU pipeline.

Changed vs v003: Stop-all icon is a crisp outlined square; pad names truncate
to the room left of the mode glyph (no overlap at 960 / 1180).

Known gaps: emitter-tile and pad labels truncate with an ellipsis at the
smallest size; Look detents follow core LOOKS (shared with the plugin), not
the design list.
