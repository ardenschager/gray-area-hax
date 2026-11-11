# 261111 studio UI — v003

Headless egui_kittest + wgpu renders (no window), 1× pixels, design media from
`design/v003/media` decoded with ffmpeg. Regenerate:

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261111_ui/v003 CHROMAGRAIN_UI_VERSION=v003 \
  cargo test -p chromagrain studio::tests::snapshots_every_view_at_every_size
```

| file | compare with (design/v003/shots) |
|---|---|
| `261111_ui_play_{1600x1000,1180x820,960x600}_v003.png` | `..._play_v003`, `..._play-ipad_v003`, `..._play-plugin-min_v003` |
| `261111_ui_sequence_{…}_v003.png` | `..._sequence_v003`, `..._sequence-ipad_v003`, `..._sequence-plugin-min_v003` |
| `261111_ui_perform_{…}_v003.png` | `..._perform_v003`, `..._perform-ipad_v003` |
| `261111_ui_deep-emitter_1600x1000_v003.png` | `..._deep-emitter_v003` |
| `261111_ui_sequence-hold_1600x1000_v003.png` | `..._sequence-hold_v003` |

State is the scripted "showcase" in `crates/app/src/studio/tests.rs` (emitter 1
held and spraying, the design's lanes, scenes 1–4, chain 1×2 2×2 3×4 4×2 3×2,
pads 1/5/10 playing). The canvases are the real GPU pipeline.

Changed vs v002: multi-touch on the Play grid (each finger plays its own
emitter; test two_fingers_play_two_emitters), the Sequence monitor holds the
latest hit between steps (backdrop), song-chain chips narrower so all 5 fit.

Known gaps: emitter-tile and pad labels truncate with an ellipsis at the
smallest size; Look detents follow core LOOKS (shared with the plugin), not
the design list.
