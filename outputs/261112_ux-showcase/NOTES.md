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
  - After a successful mux it deletes `work/frames` (about 7 GB per version);
    `CHROMAGRAIN_SHOWCASE_KEEP_FRAMES=1` keeps them.
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
- Reviewer verdict (#43): not signed off yet, but called a "huge leap forward".
  - Remaining audio: clicks at bar boundaries; 2-step and sub buried under the keys;
    the same "oh" chop for 14 bars.
  - Fullscreen: the double HUD summon is redundant; the caption jumps when entering
    fullscreen.
  - UI-BUGs: an artifact on the emitter badge under the Deep scrim; a duplicate "tunnel"
    pad name.

### v003 (internal draft, not sent)
- Audio, in the engine and the app:
  - **Loop-wrap de-click**: a 4 ms raised-cosine ramp on the arrangement either side of
    a loop wrap, so sounds running past the loop end fade instead of being cut.
  - **Engine-swap crossfade** in the playback driver: a project rebuild (any edit), a
    seek while playing (scene queue, chain tap) or a stop now crossfades over 21 ms from
    the old engine to the new one. The old one keeps rendering its arrangement and
    effect tails. Previously each of these was a hard cut.
- Demo:
  - Root cause of the per-bar clicks: drum hits used per-step slices, so they read into
    the tails of the hat/rim samples and past the end of the hat source. Drum hits now
    play from the hit's start. The keys slice at 0.86 also read past the clip end and
    is now 0.80.
  - Scene 3 voices alternate by bar with pass conditions (`every`), so no chop repeats
    back to back.
  - Mix: keys −2 dB, keys bed −3 dB; 2-step and sub up.
  - The tunnel cloud pad is named "tunnel haze".
- Film:
  - Fullscreen: pure picture first ("fullscreen · just the picture"), then one tap for
    the HUD and a launch from it, then "tap again · it stops on the bar". 2-step, sub
    and voices stop on the bar, so only keys and the cloud ring into the end card.
  - The caption keeps the same baseline and type in fullscreen, with a soft shadow
    instead of a pill.
  - The Rec → playback gap is tighter.
  - Touch rings lose their centre dot (that dot was the "badge artifact").
  - The emitter-tile scrim is stronger.
- Not sent: the glitter (10 kHz hat) source made the hats and the HUD cloud too bright.

### v004 (internal draft, not sent)
- Hats −4 dB and shorter. No hats in scene 1. The HUD launches the bench cloud instead
  of glitter.
- Not sent: Sequence scene 1 still sat about 15 dB under Perform.

### v005
- Scene-1 keys +3.5 dB; voices +1.6 dB.
- Glue compressor a little firmer (−27 dB, 2.5:1), so the build from Sequence into
  Perform is a build, not a jump.
- Reviewer verdict (#45): **SIGN-OFF**.
  - Click-free throughout.
  - The 2-step and sub have weight, and the voice chops vary.
  - Fullscreen tells the story: picture, then HUD, then a strip-back on the bar.
  - Every UI-BUG is verified fixed.
  - Captions keep a steady baseline.
- `cargo test --workspace`: 221 passed, 0 failed, 5 ignored (including `showcase_film`),
  0 warnings.
- Current pick: `v005/261112_chromagrain_ux-showcase_v005.mp4`.
