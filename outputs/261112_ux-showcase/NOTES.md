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

Status: features landed on `overhaul`; `tour_film2()` and `showcase_film2` implemented in `crates/app/src/showcase.rs`.

### Creative Commons Provenance (`assets/dig-cc/`)

All Dig and Link finds in Film 2 are genuine low-view (`< 500` views) YouTube videos published under the **Creative Commons Attribution license (reuse allowed)** (`CC BY 3.0`), verified via `yt-dlp --dump-json` (`"license": "Creative Commons Attribution license (reuse allowed)"`), trimmed to ≤480p (`480×270`) and served deterministically offline via `tools/ytdlp-stub`:

| File | Title | Uploader | Upload Date | Views | YouTube URL | License |
|---|---|---|---|---:|---|---|
| `CA9rIqpteD8.mp4` | `Playing Vinyl` | `VisionMontage` | `2019-02-12` | 40 | `https://www.youtube.com/watch?v=CA9rIqpteD8` | Creative Commons Attribution license (reuse allowed) |
| `0E94_PMkJIM.mp4` | `IMG_9201.MOV` | `ipadjunkie101 Reed` | `2019-03-17` | 10 | `https://www.youtube.com/watch?v=0E94_PMkJIM` | Creative Commons Attribution license (reuse allowed) |
| `2ZN6r74smZA.mp4` | `MVI_2064.MOV` | `Vitaliy Volia` | `2012-03-11` | 119 | `https://www.youtube.com/watch?v=2ZN6r74smZA` | Creative Commons Attribution license (reuse allowed) |
| `V14hPPDvwMA.mp4` | `MVI_5245.mp4` | `Raleigh Littles` | `2026-01-02` | 29 | `https://www.youtube.com/watch?v=V14hPPDvwMA` | Creative Commons Attribution license (reuse allowed) |
| `WMN5oi3qzdw.mp4` | `Campfire at Night – Free 4K Stock Footage for Creators` | `PJAssets` | `2025-11-01` | 10 | `https://www.youtube.com/watch?v=WMN5oi3qzdw` | Creative Commons Attribution license (reuse allowed) |
| `U5VWxgYe-HQ.mp4` | `rhodes and piano` | `soundvoid1` | `2011-07-03` | 281 | `https://www.youtube.com/watch?v=U5VWxgYe-HQ` | Creative Commons Attribution license (reuse allowed) |
| `KzZkYH0lq1Y.mp4` | `000000000000000000-0000 112.MOV` | `Will Reed` | `2018-01-14` | 9 | `https://www.youtube.com/watch?v=KzZkYH0lq1Y` | Creative Commons Attribution license (reuse allowed) |

### Film 2 Versions (`outputs/261112_ux-showcase/film2/`)

#### `film2/v001`
- Output: `outputs/261112_ux-showcase/film2/v001/261112_chromagrain_ux-showcase-film2_v001.mp4` (82.0 s, 2460 frames at 30 fps, 1920×1080, 48 kHz stereo AAC).
- App & UI improvements for Film 2:
  - `Host::audition` and `Host::stop_audition` (`crates/ui/src/host.rs`) + `App::release_voice` (`crates/app/src/studio_host.rs`): auditions now carry a voice tag (`0xA0D1_7100_0000`) with a held `VoiceEnv` (`PictureCut::Cut`, 30 ms release) and `release_voice` updates both the realtime audio engine and `live_events`, so finishing tap-chop or starting a new slice audition cleanly stops the previous audition in both sound and picture.
  - `Studio::place_added` and `Studio::next_add_target` (`crates/ui/src/studio/add.rs`): adding multiple clips while the Add sheet stays open automatically spills onto the next free grid column (`AddTarget::Any`) instead of overwriting Column 1.
  - `play_grid` (`crates/ui/src/studio/play.rs`): tapping an empty grid column (`self.columns[c].is_none()`) opens the Add sheet for that column, matching the `+` affordance drawn in row 0.
  - `seed_now()` (`crates/core/src/dig.rs`) & `Adds::sim_clock` (`crates/app/src/adds.rs`): deterministic seed progression and simulated-clock placeholder pacing so `Files`, `Browse`, `Link`, `Dig (1)`, and `Hold to dig 4` animate their `Searching` / `Downloading` progress bars smoothly on the 30 fps film clock and resolve without wall-clock races.
- Tour (`tour_film2()`):
  - `0–2 s`: Title card (`chromagrain` / `dig · chop · play`) over an empty project in `Play` view.
  - `2–20 s`: Open Add sheet from empty Column 1; add via `Files` (`fountain keys`), `Browse` (`vocal bench`), `Link` (`Playing Vinyl`, CC BY), `Dig` (`IMG_9201.MOV`, CC BY), and `Hold to dig 4` (closing the sheet onto the Play grid as Columns 5–8 shimmer with spade placeholders and resolve into 4 Creative Commons contact sheets one by one).
  - `20–26 s`: Multi-touch two emitters across the newly added & dug columns on the Play grid.
  - `26–40 s`: Long-press Column 3 (`Playing Vinyl`) to open the Chop sheet; tap `Tap` and drop 4 live Koala-style chop points on the drum transients; drag `slice 1 end`; audition two slices (sound + picture); tap `Slices → pads`.
  - `40–53 s`: Switch to `Sequence`; tap steps on the chopped vinyl lane and vocal lane; hold a step to pick one of the clip's own numbered chops (`slice 3`); switch the stage from `stack` to `tile` (`layout tile`).
  - `53–82 s`: Switch to `Perform`; long-press `pad 1` to shape the 4-handle ADSR envelope over the thumbnail and toggle `Fade` / `Cut` (`Envelope — sound and picture`); launch pads to build a `2×2` multi-sample `tile` grid with layout-driven stereo pan (`left tile, left ear`); enter `Fullscreen` in `tile` mode, summon the HUD to launch/strip back on the bar, and fade into the end card.
- Reviewer verdict (#55): not signed off yet.
  - Opening `Add a clip` flow (`Files` → `Browse` → `Link` → `Dig` → `Hold to dig 4` with spade placeholders) praised as fast and tactile.
  - Notes to address:
    1. Remove phantom `00:00.3` chords before `fountain keys` is imported (`00:03.6`).
    2. Eliminate dead-air holes in the Chop section and make `Slices → pads` open `Perform` immediately so the slice pads are finger-drummed right away.
    3. Trigger one-shot slice pads while shaping ADSR (`Fade` vs `Cut`) so the viewer sees and hears `Fade` vs `Cut` live; demo `"left tile, left ear"` with isolated Left/Right tile ping-pong; drive `Sequence`, `Perform`, and `Fullscreen` in `tile` mode from short-gate `Cut` pattern steps so tiles dynamically pop and hard-cut across `1×1 ↔ 2×1 ↔ 2×2` on every beat instead of holding static `∞` loops.
    4. Fix 5 `UI-BUG`s: hide emitter rings on empty columns (`play.rs`), format `ADDED` card provenance as `<views> · CC · <year> · <uploader>` over a darker scrim (`mod.rs`, `add.rs`), peak-normalize Chop waveforms (`wave.rs`) and show the auditioned slice on the stage behind the modal (`mod.rs`, `gpu.rs`), preserve `· 1`..`· 4` slice suffixes + `slice.start` thumbnails + `Icon::Chop` on `Slices → pads` and deduplicate Perform pads (`perform.rs`, `studio_host.rs`, `ui_perform.rs`), and keep the real playing engine in `Driver::swap` when two swaps arrive in the same block (`playback.rs`).

#### `film2/v002`
- Output: `outputs/261112_ux-showcase/film2/v002/261112_chromagrain_ux-showcase-film2_v002.mp4` (82.0 s, 2460 frames at 30 fps, 1920×1080, 48 kHz stereo AAC).
- All 5 `UI-BUG`s fixed in `crates/ui` and `crates/app`:
  - **Orphan emitter rings (`crates/ui/src/studio/play.rs`):** Emitter rings `(1)..(4)` only render when `self.columns[e.col]` holds a loaded clip.
  - **Provenance legibility (`crates/ui/src/studio/mod.rs`, `crates/ui/src/studio/add.rs`):** `provenance_line` now orders `<views> · CC · <year> · <uploader>` over a darker bottom scrim (`alpha 205`, `INK2`) so `40 views · CC` and `281 views · CC` never clip off the right edge.
  - **Chop waveform normalization & stage audition (`crates/ui/src/wave.rs`, `crates/ui/src/studio/mod.rs`, `crates/ui/src/gpu.rs`):** `Peaks::of` peak-normalizes clip waveforms to 92% of the waveform box height; `Studio::backdrop` and `bus == 0` display the live tap-chop / slice audition video frame on the stage behind the modal.
  - **One-shot `Slices → pads` with `slice.start` thumbnails, `· 1..4` suffix preservation, `Icon::Chop`, and immediate transition to `Perform` (`crates/ui/src/studio/clip.rs`, `crates/ui/src/studio/perform.rs`, `crates/app/src/studio_host.rs`, `crates/app/src/ui_perform.rs`):** Clicking `Slices → pads` closes the Chop sheet and switches straight to `View::Perform`; Pads 1–4 show `Playing V… · 1`..`4` with each slice's own `slice.start` turntable shot thumbnail and `Icon::Chop`, fire as ADSR-shaped one-shots (`fire_pad_slice`) that claim tiles in `Layout::Tile`, and auto-preview when toggling `Fade` / `Cut` in the Pad Sheet. Perform cloud pads are named `<source> cloud` so no two pads share duplicate labels, and `000000000000...` is removed from `search.jsonl`.
  - **Mode-switch de-click (`crates/app/src/playback.rs`):** `Driver::swap` preserves an active `self.prev` crossfade engine when `set_project` and `TransportCmd::Play` arrive in the same block.
- Story & audio updates (`tour_film2()`):
  - Honest empty opening (`0:00–0:03.6` quiet until `fountain keys` lands in Column 1).
  - Multi-touch Play (`0:20.8–0:25.8`) plays warm `fountain keys` + `rhodes and piano` + `vocal bench` (zero camcorder noise).
  - Continuous subtle pad bed during Chop (`0:26.4–0:35.0`, zero dead air); `Slices → pads` (`0:35.0`) opens `Perform`, where Pads 1–4 are immediately finger-drummed as one-shots (`0:35.9–0:38.5`), shaped in the Pad Sheet with live `Fade` (`0:44.0`) and `Cut` (`0:45.6`) one-shot previews, flipped to `tile` (`0:48.8`) popping `1×1 → 2×1 → 2×2`, and ping-ponged Left-tile/Left-ear vs Right-tile/Right-ear (`0:53.0–0:56.5`).
  - `Sequence` (`0:57.4–1:09.6`) builds a 2-scene groove of short-gate `Cut` chops, which then drives the `Perform` / `Fullscreen` climax (`1:09.7–1:22.0`) with beat-synced sharp cuts across `1×1 ↔ 2×1 ↔ 2×2` tiles and HUD strip-back on the bar.
- Reviewer verdict (#57): huge leap forward, not signed off yet.
  - Praised: empty start (`0:00–0:03.6`), un-truncated `<views> · CC` provenance cards, normalized Chop waveform + live stage backdrop (`0:27–0:35`), `Slices → pads` landing directly in `Perform` with finger-drumming (`0:35–0:38.5`), `1×1 → 2×1 → 2×2` vinyl slice expansion (`0:49–0:52`), tiled `Sequence` monitor (`0:58–1:09`), and dynamic Fullscreen tile cuts (`1:10–1:16`).
  - Notes to address for `v003`:
    1. **`UI-BUG` (`00:52.8–00:56.5` — `"left tile, left ear"` visual/pad mismatch):** Remove fake silent anchor grains that showed `fountain keys` + `vocal bench` on both hits; instead hold `Pad 5` (`fountain keys`) in `slot 0` (Left tile, Left ear) and tap `Pad 1` (`Playing Vinyl · 1`) and `Pad 2` (`Playing Vinyl · 2`) into `slot 1` (Right tile pops in on the right with each vinyl turntable shot, Right ear, and cuts out cleanly on release).
    2. **`UI-BUG` (`00:36.5`, `01:10.2` — `Perform` pad thumbnails & `.mp4`/`.MOV` `"… c…"` truncation):** Match `campfire` source by name (`"campfire"`) instead of hardcoded index `7`; give each scene/pattern pad in Row 2 (`keys + vocal`, `vinyl + rhodes`, `campfire air`, `full groove`) its own distinct thumbnail (`c.source` / first unmuted active row in `studio_host.rs`); strip `.mp4`/`.MOV` extensions (`strip_media_ext`), remove the redundant `" cloud"` suffix (`Icon::Cloud` already marks cloud pads), and widen pad text room in `perform.rs` so `"Playing Vinyl · 1"`..`"4"` never truncate.
    3. **ADSR `Fade` vs `Cut` legibility (`00:39–00:48`):** Eliminate the `3.7 s` silence when opening Pad 1's sheet by auto-previewing on sheet open and whenever an ADSR handle drag releases (`clip.rs`); drag `adsr R` right (`~680 ms`) and `adsr D` right+down (`~650 ms`, low sustain `~18%`) instead of dragging `R` to `0 ms`, so `Fade` visibly fades out gradually while `Cut` holds 100% brightness across the sloped tail and snaps hard to black (including an unobstructed full-stage hit right after closing the sheet at `47.5s`).
    4. **Musicality (`00:22` Rhodes touch & `00:58–01:16` Sequence/Perform groove):** Pitch-align `U5VWxgYe-HQ.mp4` (`rhodes and piano`, CC BY 3.0) to F minor with a warm 3.2 kHz lowpass; touch the warm sustained chord cell (`row 5.6..6.1`) at `00:21.5` instead of the high trill at `row 2.0`; syncopate the 4 Sequence/Perform lanes (`fountain keys` on `1, 9`, `rhodes` answering on off-beats `4, 12`, `Playing Vinyl` kick on `1, 7` and snare on `5, 13`, `vocal bench` on `3, 7, 11, 15`); and don't reset the playhead in `set_mode` while playing so switching `Sequence → Perform` (`01:09.7`) is completely click/dip-free.

#### `film2/v003`
- Output: `outputs/261112_ux-showcase/film2/v003/261112_chromagrain_ux-showcase-film2_v003.mp4` (82.0 s, 2460 frames at 30 fps, 1920×1080, 48 kHz stereo AAC).
- Fixes all items from #57:
  - Honest `"left tile, left ear"` (`0:52.5–0:56.8`): `Pad 5` (`fountain keys`) holds `slot 0` (Left tile, Left ear `-0.80`), while tapping `Pad 1` (`Playing Vinyl · 1`) at `53.55s` and `Pad 2` (`Playing Vinyl · 2`) at `55.05s` natively claims `slot 1` (Right tile pops in on the right showing each turntable slice, Right ear `+0.80`) via `fire_pad_slice` and cuts out cleanly on release.
  - Perform pad grid (`perform.rs`, `studio_host.rs`, `mod.rs`): `strip_media_ext` strips `.mp4`/`.MOV` extensions; `"Playing Vinyl · 1"`..`"4"` fit in full without ellipsis; `campfire` pad matches `Campfire at Night` by name; pattern pads in Row 2 (`keys + vocal`, `vinyl + rhodes`, `campfire air`, `full groove`) each display their own distinct source thumbnail; Rows 3–4 cloud pads drop the redundant `" cloud"` suffix.
  - ADSR `Fade` vs `Cut` (`clip.rs`, `ui_perform.rs`): Pad 1 previews on sheet open (`39.65s`), on releasing `adsr R` (`40.95s`, dragged right to `~680 ms`), and on releasing `adsr D` (`42.15s`, dragged right+down to `D ≈ 650 ms, S ≈ 18%`), eliminating dead air; `Fade` (`43.9s`) visibly fades both the editor thumbnail and live stage along the ~680 ms slope, while `Cut` (`45.5s` and `47.55s` full-stage hit) holds 100% brightness until the tail ends and snaps hard to black.
  - Sound & musicality: `U5VWxgYe-HQ.mp4` (`rhodes and piano`, CC BY 3.0) is pitch-aligned to F minor (`-2 st`, 3.2 kHz lowpass); Emitter 2 touches the warm sustained chord cell (`row 5.6..6.1`) at `0:21.5`; Sequence & Perform lanes are syncopated across off-beats (`fountain keys` on `1, 9`, `rhodes` on `4, 12`, `Playing Vinyl` kick/snare on `1, 5, 7, 13`, `vocal bench` on `3, 7, 11, 15`); `set_mode` preserves playhead while playing so `Sequence → Perform` (`1:09.7`) has zero audio dip.
- Reviewer verdict (#59): **SIGN-OFF**.
  - `"left tile, left ear · stereo follows the grid"` (`00:52.5–00:56.8`) verified 1:1 visually and sonically (`fountain keys` held in `slot 0` Left ear, `Playing Vinyl · 1` and `· 2` popping into `slot 1` Right ear and hard-cutting back to `1×1` on release).
  - ADSR `Fade` vs `Cut` (`00:39.5–00:48.2`) unmistakable (`Fade` dims along the sloped ~680 ms decay/release, `Cut` holds 100% brightness across the tail and snaps hard to black).
  - Sound, groove, and transitions (`00:00–01:22`) verified click- and dip-free (`-16.0 LUFS`, `-1.5 dBFS` peak).
  - All 1080p `UI-BUG`s verified fixed.
- `cargo test --workspace`: 281 passed, 0 failed, 7 ignored (including `showcase_film` and `showcase_film2`), 0 warnings.
- Current Film 2 pick: `film2/v003/261112_chromagrain_ux-showcase-film2_v003.mp4`.
