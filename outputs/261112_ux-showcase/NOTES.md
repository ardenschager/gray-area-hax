# 261112 · chromagrain UX showcase

A 60–90 s product film of the new studio UI (Play · Sequence · Perform · Deep),
made from the **real app**, driven by a scripted tour and rendered headlessly.
No window, no screen capture, and no audio device are used.

## How it's made

```
CHROMAGRAIN_SHOWCASE=v00N cargo test -p chromagrain showcase_film -- --ignored --nocapture
```

- `crates/app/src/showcase.rs`: the tour (a timeline of gestures and captions), the
  overlay (touch rings and the caption band), and the recorder.
  - The recorder drives `App` through `egui_kittest` + wgpu offscreen at a fixed 30 fps
    (1600×900 pt at 1.2× = 1920×1080).
  - It writes every frame to `v00N/work/frames/*.png`.
  - For audio it steps the same engine offline: exactly 1600 samples (48 kHz / 30) after
    every frame (`Playback::new_offline` + `render_offline`). Picture and sound come
    from one clock (`Clock::Sim`).
  - It muxes with ffmpeg: H.264 CRF 15 + AAC 256k. Audio goes through a gentle glue
    compressor, then a two-pass loudnorm to −16 LUFS / −1.5 dBTP, with a 3 s fade.
  - It refuses to overwrite an existing version.
- `crates/app/src/demo.rs`: the demo-project builder. `chromagrain --demo` now opens
  **Night Platform** (assets/demo) in the studio at 133 bpm, F minor:
  - 7 clips on the Play grid;
  - 7 lanes (kick, rim, hats, sub, keys, two voices) with 3 scenes and a song chain;
  - 15 pads, with the keys loop already playing as a bed.
- Engine changes that also help the real app:
  - `RealtimeAudio::adopt_live` keeps sounding live grains and bus tails when an edit
    rebuilds the engine. Previously an edit cut them.
  - Live hits can be delayed by a fraction of a block.
  - Studio-built sequence clips draw soft-edged hits.

Captions sit in a 58 pt band under the UI, so they never cover the interface.
Touch rings are soft white circles: press-in, a short trail while dragging, and a
ripple on lift.

## Versions

### v001
- First complete tour, about 82 s:
  - title card;
  - Play: touch and drift, two fingers, a held finger while GRAIN / PITCH / SMEAR / LOOK
    turn, a Rec take;
  - Sequence: transport, tap 4 steps, drag up for pitch ×2, hold sheet (slice + length),
    queue scene 3, extend the song with an empty scene;
  - Perform: launch the groove and a voice cloud on the bar, fullscreen with the HUD,
    launch from the HUD;
  - Deep drawer;
  - end card.
- Mix: −16 LUFS integrated.
- Reviewer verdict: pending.
