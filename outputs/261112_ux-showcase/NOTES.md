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

---

## Film 2: dig, chop, tile, a new track (storyboard; spec_v003)

Goal: from an **empty project**, add media through every route, chop a find Koala-style,
shape ADSR (which cuts the picture too), switch to tile for sharp-cut multi-sample
visuals, then build and perform a short new track. Same film grammar as v005: a caption
band, soft touch rings, knob-anchored rings, fullscreen with no band.
- Target length: about 85 s.
- Output: `outputs/261112_ux-showcase/v006/` onward. A separate `tour2()` keeps `tour()`
  (film 1) reproducible.

| t (s) | view | gesture | caption | payoff |
|---|---|---|---|---|
| 0–3 | Play (empty) | — | title "chromagrain" / "dig · chop · play" | graphite studio, empty grid with "+" columns, soft room tone |
| 3–6 | Play | tap "+" on column 1 | one sheet adds anything | Add sheet rises: Files · Browse · Link · Dig |
| 6–9 | Add | tap Files | files | (stub picker) a placeholder tile streams into a contact sheet in column 1 |
| 9–13 | Add | tap Browse, tap a thumbnail | every video on this mac | Browse grid (fixture folder) → column 2 |
| 13–17 | Add | tap Link, paste URL, tap add | paste a link | "downloading…" placeholder → column 3, with provenance on the card |
| 17–21 | Add | tap Dig | dig · a video almost nobody has seen | "digging…" → a find, its card showing e.g. `IMG_4417 · 9 views · 2014` |
| 21–25 | Add | hold Dig | hold to dig four | 4 placeholders fill columns 4–7 one by one (staggered) |
| 25–28 | Play | touch a find (emitter) | (none) | the first sound of the found footage, grains in picture |
| 28–31 | Play | hold column 5 header | hold a clip to chop it | Chop sheet: waveform + filmstrip |
| 31–36 | Chop | tap "tap", then tap ×8 in time | tap along to chop | chop points drop live; alternating bands |
| 36–39 | Chop | tap two slices | each slice, heard and seen | audition, sound and picture |
| 39–41 | Chop | tap "slices → pads" | slices to pads | sheet closes into Perform: pads 1–8 are the slices |
| 41–44 | Perform | finger-drum 3 pads | (none) | the chopped find as a kit |
| 44–50 | Perform | hold a pad; drag R short, then A | the envelope cuts sound and picture | 4-handle ADSR over the pad thumbnail; tap → sharp cut in sound and picture together |
| 50–52 | Perform | tap stack → tile | tile · every voice gets its own frame | layout switch on the stage |
| 52–58 | Perform | two-finger drumming on pads | left tile, left ear | voices pop in and out of a 2×2 / 3×3 grid with hard cuts; pan follows the tile |
| 58–61 | Sequence | tap Sequence | build a track from what you found | lanes = the new clips, all empty |
| 61–66 | Sequence | tap steps on 3 lanes (kick-ish slice, texture, voice) | tap a step | the step thumbnail is the slice |
| 66–69 | Sequence | hold a step, pick slice 5 | steps choose from the slices | step sheet slice picker = the chop |
| 69–72 | Sequence | tap play; tap scene 2 (empty → copy), add 2 steps | a second scene | the groove builds on the bar |
| 72–78 | Perform | launch the sequence pad + 2 slice pads; Fullscreen (tile) | perform it | tiled fullscreen, sharp cuts on the beat |
| 78–80 | Fullscreen | tap for the HUD, stop 2 pads | (none) | strip back on the bar |
| 80–85 | — | — | end card "chromagrain" / "dig · chop · play" | audio fades with the card |

### What the recorder needs from the features
Headless, deterministic, and no network:
1. **File picking seam.** `Host::import_media` currently opens an `rfd` dialog. The film
   needs an injectable picker, e.g. an `App` field or `CHROMAGRAIN_PICK=<path>`. The
   film draws no OS dialog: the tap on Files goes straight to the placeholder tile, and
   the caption says "files".
2. **Browse root override**, pointing at a fixture folder (e.g. `assets/demo` + the dig
   cache), so the Browse grid is stable and has thumbnails.
3. **yt-dlp stub** (the spec's env override) for Link and Dig, returning cached clips plus
   provenance JSON. The dig finds are pre-dug once, **reviewed by a person for content
   and rights**, and cached under `assets/dig-cache/` (or `outputs/…/src/`), so the film
   never shows an unvetted random video.
4. **Background jobs complete deterministically.** The recorder already blocks on
   `App::busy` between frames, which also works for downloads and decoding. Placeholder
   tiles need a visible "loading" state so the stream-in reads on screen.
5. **Accessible labels** on new controls: `add Files` / `add Browse` / `add Link` /
   `add Dig`, `chop tap`, `slices to pads`, `adsr A/D/S/R`, `layout stack` / `layout tile`,
   and `browse item N`, so gestures resolve against the real widgets.
6. **Text input** for Link: kittest `Event::Text` / `Event::Paste` into the URL field.

Status: storyboard only. No rendering until the features land on `overhaul`.
