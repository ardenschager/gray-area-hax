# 261111 plugin editor — v001 … v004

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

v002 → v003 (`overhaul-plugin-v2`): **Perform** joins (the instrument's 4×4 pad
bank; here clips 1–7 on pads 1–7, pads 1, 3 and 6 playing, launched on the beat
while the host plays). New shots: `perform_{960x600,1180x820,1600x1000}` and
`perform-fullscreen_1180x820` (Fullscreen fills the editor with the canvas). The
Output routing control is hidden (`Caps::output = false`: no second window in a
DAW). In v003 the pads' grains were drawn as small grain tiles (the default live
layer).

v003 → v004: pad loops draw full-frame on their own layer (GRAIN's sample-end
style + the shared SMEAR/LOOK picture effects), like the app's snippet pads;
held MIDI notes are drawn too (echoed from the audio thread, on emitter 1's
layer). Play / Sequence only gain the Perform tab in the top bar.
