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
- Reviewer verdict (#41): not signed off. Main notes:
  - Cause and effect don't land: two fingers on two grey clips, no payoff for Rec, a
    hold sheet with identical slices, the HUD hidden in fullscreen, a weak Deep page.
  - Dead air at 0:00 and 0:29.
  - The arc is flat.
  - Audio: a pop on LOOK through crush, and loop clicks.
  - UI bugs:
    - visual grains are hard rectangles (the GPU ignored `feather`);
    - fullscreen is pillarboxed by the band;
    - the HUD is missing pad names and thumbnails;
    - Perform pads all say "kick";
    - the Deep global "Links" tab is really song settings and shows a `--classic` CLI banner;
    - the LOOK tag lags the knob by one frame;
    - emitter tile labels are unreadable on bright footage.

### v002
- Fixes in the app:
  - **GPU feather**: `fs_grain` tapers patch borders exactly like the CPU compositor,
    with a new parity case. Emitter clouds now use feather 0.7.
  - **Live-bus crossfade**: a LOOK change crossfades the old effect chain into the new
    one over 43 ms instead of rebuilding with a click. Has a test.
  - **Named pads**: a named pad track names its pad (`Pad::name`).
  - **Fullscreen HUD to spec**: tempo, beat dots and the names of playing pads; the
    mini pads are thumbnails with an active border.
  - **Deep**: the global first tab is labelled "Song" and the CLI banner is gone.
  - **Play**: knobs update before the emitter tiles and the stage tag, so all three
    show the same LOOK. Emitter tiles get a bottom scrim.
- Demo:
  - The pad bank has one role per row, with unique names: beds (keys, rain, tunnel,
    highway); step layers (2-step, sub, voices, chords); clouds; whole scenes (drop,
    intro, ghost rim).
  - Drums and sub are louder.
  - The keys loop is the Perform bed.
- Tour:
  - The first finger lands under the title, so there's sound from 0.3 s.
  - Two fingers play keys (teal) + bench (voice), and each finger grabs its own
    emitter ring.
  - Knob rings stay on the knob, with a drag-up chevron.
  - LOOK slips quickly past crush and settles on tint, then kaleido.
  - Deep opens on Emitter 1 (⋯); pitch → hue is dragged while a finger keeps playing.
  - After Rec, "hands off · it plays back": the transport plays the take, which also
    bridges into Sequence with no silence.
  - The hold sheet is on the keys lane: tap slice 12 (a different chord and frame),
    then drag chance down.
  - Scene 3 is queued and the drums drop in on the bar.
  - Perform layers 2-step → sub → voices on the bar.
  - Fullscreen is edge to edge, with the HUD summoned after 1 s, pure picture, then a
    launch from the HUD.
  - The film ends on the picture; audio fades with the end card.
- Encode: CRF 19, 30.9 MB. Mix: −15.9 LUFS, LRA 6.5.
