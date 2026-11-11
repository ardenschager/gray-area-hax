# 261111 studio UI — v001 (first full pass)

Headless egui_kittest + wgpu renders (no window), 1× pixels, design media from
`design/v001/media` decoded with ffmpeg. Regenerate:

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261111_ui/v001 CHROMAGRAIN_UI_VERSION=v001 \
  cargo test -p chromagrain studio::tests::snapshots_every_view_at_every_size
```

| file | compare with (design/v001/shots) |
|---|---|
| `261111_ui_play_{1600x1000,1180x820,960x600}_v001.png` | `..._play_v001`, `..._play-ipad_v001`, `..._play-plugin-min_v001` |
| `261111_ui_sequence_{…}_v001.png` | `..._sequence_v001`, `..._sequence-ipad_v001`, `..._sequence-plugin-min_v001` |
| `261111_ui_perform_{…}_v001.png` | `..._perform_v001`, `..._perform-ipad_v001` |
| `261111_ui_deep-emitter_1600x1000_v001.png` | `..._deep-emitter_v001` |
| `261111_ui_sequence-hold_1600x1000_v001.png` | `..._sequence-hold_v001` |

State is the scripted "showcase" in `crates/app/src/studio/tests.rs` (emitter 1
held and spraying, the design's lanes, scenes 1–4, chain 1×2 2×2 3×4 4×2 3×2,
pads 1/5/10 playing). The canvases are the real GPU pipeline.

Known gaps vs the design (next passes): Play canvas is brighter than the
design's (emitter grains over the backdrop; v002 makes the sample end
alpha-over), single-pointer only (no multi-touch yet), emitter-tile labels
truncate at small sizes.
