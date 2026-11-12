# spec v003 — dig, chop, tile, ADSR (261112)

Arden, after watching the UX showcase v005: "showcase the UX of adding new videos, and maybe
try making a new track too. I want it to be able to do that thing it could do before of
downloading random and unseen YouTube videos. I want a sample chopping mechanism, like
Koala Sampler. And an optional tiling mechanism, and ADSR also affects visual cutoff too.
Layout maybe affects panning, or we can just have tiling too. I think tiling could be sweet,
with sharp cuts for multiple samples."

Design language is unchanged: neutral graphite, colour comes only from footage, touch-first
(44 px targets, no hover or right-click only), few controls, everything else in Deep.
The core is the source of truth, so the app, plugin and export all render the same.

## 1. Add media + Dig (random unseen YouTube)
- One **Add sheet**, reached from the grid's empty "+" column, a pad's "+" or Sequence's "+":
  `Files` · `Browse` (every video on this Mac, which exists in the classic UI) · `Link` (YouTube or any yt-dlp URL) · **`Dig`**.
- **Dig** fetches a random, rarely seen video, the way astronaut.io / petittube do:
  - The query is a camera-default filename (`IMG_####`, `MVI_####`, `DSC_####`, `MOV_####`,
    `VID_YYYYMMDD`, `DSCN####`, …) or a random word.
  - It runs `yt-dlp "ytsearchN:<q>"` with a match filter (low view count, e.g. < 500;
    duration 10 s–10 min; not live), picks one at random, and downloads it at 480p or less.
  - A random 20–60 s window is cut and loaded as a Source.
  - Provenance (title, uploader, upload date, views, url) is kept on the Source and shown on its card.
  - It retries with a new query when nothing passes the filter.
  - Desktop only, behind `MediaBackend`. Tests use a stub yt-dlp (env override).
  - Dig is one tap. Hold it to "dig 4" (fill 4 empty columns). Results stream in with a
    placeholder tile while they download.

## 2. Chop (Koala-style)
- Any Source can carry **slices** (`Vec<Slice{start,end}>`, persisted). Ways to make them:
  - **auto**: transients, via the existing `detect_slices`;
  - **equal**: 4 / 8 / 16;
  - **tap**: play the sample and tap to drop chop points live, like Koala's live chop;
  - **edit**: drag edges, double-tap a slice to delete it.
- **Chop sheet**: a full-width waveform plus a filmstrip of the same span, with slices as
  alternating subtle bands; tap a slice to audition it (sound and picture). Opened by
  long-pressing a grid column header, a lane header or a pad, or from the Add sheet right after import.
- Slices **map out**:
  - "slices → pads" fills Perform pads 1..N with one slice each;
  - MIDI keys C3 and up play slices;
  - Sequence steps pick from the source's slices (the step sheet's "slice" control).
  - The plugin gets the same thing via Instrument.

## 3. ADSR (sound and picture)
- Per pad, emitter and lane: an **A / D / S / R** envelope that shapes audio gain AND the picture.
- **Picture cutoff**: the picture follows the envelope.
  - **cut** (default): full opacity while the envelope is above a small threshold, then a
    hard cut, with no fades. A short release gives a sharp cut in sound and picture together.
  - **fade**: opacity is the envelope.
- UI: a compact 4-handle envelope drawn in the pad's or lane's hold sheet and the emitter's
  Deep page, previewed over the clip's own thumbnail. No numbers unless dragging.

## 4. Tiling
- Optional **layout mode**: `stack` (today: layered compositing) or **`tile`**.
- In tile mode every sounding voice (pad, slice, lane hit, held note) gets its own tile in an
  automatic grid (1 → 2 → 2×2 → 3×3, stable slot assignment so tiles don't jump). Tiles
  appear and disappear with **hard cuts**, governed by ADSR cut, with a hairline gap and
  no animation.
- **Layout → pan** (optional, on by default in tile mode): tile column maps to stereo pan
  and row to a subtle brightness/elevation cue, reusing the existing position-as-sound links.
- Lives in the core compositor (CPU export) with GPU parity in crates/ui, and is available
  in the plugin. Toggle: a small `stack | tile` switch on the stage (Play/Perform) plus a
  `layout` param in the plugin.

## 5. Showcase v006+
A second film. Start from an empty project, add media through every route (Files, Browse,
Link, Dig with a couple of random unseen finds), chop one by tapping, send slices to pads,
shape ADSR, switch to tile mode for sharp-cut multi-sample visuals, then build a short new
track in Sequence and perform it. Carbon reviews and iterates until sign-off.
