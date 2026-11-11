# 261111 plugin editor — v001 / v002

The real studio UI (crates/ui) inside the VST3/CLAP plugin, rendered headlessly
(egui_kittest + wgpu, 1× pixels; the canvas is the real GPU pipeline fed by the
plugin's document). Media: the design's 7 clips (ffmpeg) handed to the plugin's
loader exactly like a host session.

```
CHROMAGRAIN_UI_SHOTS=$PWD/outputs/261111_plugin_ui/v00N CHROMAGRAIN_UI_VERSION=v00N \
  cargo test -p chromagrain-plugin --test editor
```

| file | what |
|---|---|
| `261111_plugin_editor_play_{960x600,1180x820}_v00N.png` | Play: grid + emitters + canvas; knobs = the plugin's GRAIN/PITCH/SMEAR/LOOK params |
| `261111_plugin_editor_sequence_{960x600,1180x820}_v00N.png` | Sequence: one scene of step lanes, played by the instrument in sync with the host transport |

v001 → v002: plugin-only chrome follows `Host::caps()` — no scenes / song chain
(the DAW arranges), the brush panel's Rec becomes "Load clip" (no takes in a
plugin), no Export, the tempo chip and play button show the host's state
read-only, and only Play + Sequence are offered.
