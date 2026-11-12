# chromagrain · design v002: four ways to expose parameters

Arden, after v001 and the Rust build: *"I'm not sure I like the sidebar with all the hidden
params. Can we make those more exposed (not swept under the rug), more integrated, but keeping
the intuitive design?"*

v002 is one interactive prototype with **four switchable directions** (`A · Pages`, `B · Clip`,
`C · Object`, and **`D · Hybrid ("touch it · open it")`**). The Deep drawer is gone
in all four. Switch with **A / B / C / D** on the keyboard or with the dashed (prototype-only)
control in the top bar. Every direction includes the spec-v003 features: **Chop, ADSR with
picture cut/fade, stack | tile, and the Add sheet (Files / Browse / Link / Dig)**.

```
python3 -m http.server -d design 8000   →  http://localhost:8000/v002/index.html
```
It also opens directly over file://, but grain sound then falls back to the key-snapped synth.
v002 reuses `../v001/media` (7 archival clips) and adds three "dug" phone-style clips in
`media/`: IMG_4821 (a dog), MVI_0032 (a campfire), and DSC_1177 (a subway platform). It also
adds `media/peaks.js`, which holds real audio peaks plus a per-clip "picture energy" curve used
when a clip has no sound.

Demo URLs (used for the screenshots): `?demo&dir=A|B|C|D&view=play|seq|perform` (case-insensitive `?dir=a|b|c|d`), plus
`&sel=emitter:N|pad:N|lane:N|project`, `&page=SHAPE`, `&clip`, `&proj`, `&layout=tile`,
`&sheet=add|dig|chop|tapchop`, `&mini=1|shape|fx|grain|pitch|link`, `&topop=tempo|key|project`,
`&slices=<clip>`.

## The three directions

| | **A · Pages** | **B · Clip page** | **C · On-object** |
|---|---|---|---|
| Model | Elektron / Tempera parameter pages | Koala sample-edit, but AV | Parameters live on the thing |
| Where params are | A strip at the bottom of every view, for the selection | One calm full-screen page per clip, scoped to an owner | Drawn on emitters, pads and lanes; gestures change them; contextual mini-strip |
| How you get there | Select anything (emitter, pad, lane, clip header, Project) | Tap ›, a lane header or a clip header; long-press an emitter tile or pad. One tap back | Touch / hold the object |
| Fixed knob positions | Yes: 8 slots, page tabs `MACRO · SOURCE · GRAIN · SHAPE · PITCH · FX · LINK` | Yes, as cards: `Shape · Grain · Pitch · Effects · Links` | No; 4 macros are gestures, the rest are popovers |
| Screen cost | ~190 px strip, always (150 at 960×600) | 0 while playing; the whole screen while editing | 0 |

### Where each v003 feature lives

| feature | A · Pages | B · Clip page | C · On-object |
|---|---|---|---|
| **Chop** | SOURCE page: mini waveform + filmstrip (tap a slice to audition), a `Chop` tile opens the full sheet, and a `Slices → pads` tile. Long-press a clip header or lane header also opens the chop sheet | The middle band of the clip page *is* the chopper: filmstrip, waveform, Auto · 4 · 8 · 16 · Tap, then Slices → pads | `chop` in the mini-strip, or long-press a clip header or lane header; the sheet grows out of the object with an arrow |
| **ADSR + picture cut/fade** | MACRO page has an ADSR preview (tap → SHAPE); SHAPE page has a 4-slot envelope over the clip's own thumbnail plus `cut / fade`, `cut at`, `layout` and `tile → pan` | Shape card: envelope over the thumbnail + cut/fade + cut-at | A thin envelope line is drawn on every emitter card, pad and lane header; `shape` in the mini-strip opens the full envelope popover |
| **stack \| tile** | On every stage, plus the SHAPE and project MAIN pages | On every stage, the clip-page picture, and the Project page | On every stage, plus the end of every mini-strip |
| **Add sheet** | `+` column header, `+` pad, `+` lane, the `clip` slot's Replace, and LIBRARY page `+ / Dig` | Same entry points, plus `Replace` / `Dig` on the clip page head | Same entry points, plus Library in the Project popover |
| **Dig** | One sheet everywhere: a big **Dig** button (hold = dig 4) shows the camera-filename queries. Results stream in as shimmer cards ("searching · < 500 views" → "downloading 480p · cutting a window"), then land with provenance (title · views · date · uploader) and a `Chop` button. The empty 8th grid column shimmers while it downloads, then becomes the found clip | ← | ← |

### Where global things go

| global | A · Pages | B · Clip page | C · On-object |
|---|---|---|---|
| tempo / key | Top bar (drag tempo, tap key) **and** the project MAIN page (`Project` button, or tap the tempo chip) | Top bar, and the Project page's "Time & key" card | Top bar; tapping a chip opens its own popover (tempo + swing + tap; 12 roots + scales) |
| song chain | Sequence sidebar, plus the project SONG page (tap a link to cycle ×1/2/4/8, `+ scene`) | Sequence sidebar, plus the Project page "Song · scenes" card | Sequence sidebar (it is already on-object) |
| library | Project LIBRARY page (thumbs, + add, Dig) | Project page Library card | `Project` popover |
| script | Project SCRIPT page | Project page Script card | `Project` popover → Script |
| master FX | Project FX page (8 dual ♪/▣ knobs) | Project page Master effects card | `Project` popover → Master fx |

## Pros and cons

**A · Pages**
- ✅ Nothing is hidden: the strip is always there, and the tabs say exactly what exists.
- ✅ Fixed 8 positions map 1:1 to **plugin host parameters, MIDI controller pages and automation
  lanes** (VST3/AU/CLAP want stable parameter IDs). Muscle memory carries across emitters,
  pads and lanes.
- ✅ Same surface in Play, Sequence and Perform, and the cheapest to build in egui (one strip
  widget plus a page table).
- ❌ It is still a panel. ~190 px of permanent chrome takes height away from the video, and the
  960×600 plugin window gets tight: Play loses the Rec row and Sequence loses lane height.
- ❌ Knobs are abstract ("which emitter am I editing?"); selection has to be very clear.

**B · Clip page**
- ✅ The calmest way to see *everything* about one clip at once: picture, chop, shape, grain,
  pitch, FX and links. This is the "params page" Arden mentioned, and it fits sample-minded
  people (Koala, SP-404).
- ✅ Zero cost to the play surfaces, which stay exactly as clean as v001.
- ✅ It makes the v003 flow obvious: Add → Chop → Slices → pads all happen on one page.
- ❌ It is modal: you leave Play to edit, so it is poor for tweaking mid-performance.
- ❌ It needs an owner selector (Clip default · Emitter 2 · Lane 5 · Pad 3) because the same
  clip is used in many places. That is powerful, but one more concept.

**C · On-object**
- ✅ The most instrument-like and integrated: the parameters *are* the picture. Ring = grain
  size (cloud dots inside), dot on ring = pitch, trail = smear, line = ADSR, bar = level.
  Performing and editing are the same act. It is iPad-native.
- ✅ No panels at all; the mini-strip and popovers appear only next to what you touched.
- ❌ Discoverability: gestures (twist / pinch / swipe) must be learned. Every gesture has a
  visible handle (drag the ring, drag the pitch dot, wheel / ⇧wheel) so mouse users aren't
  stuck, but it is still the steepest learning curve.
- ❌ Precision and breadth: FX, links and grain detail still end up in popovers. It is also the
  hardest to build robustly in egui plugin windows, where many hosts deliver no multi-touch.

## Recommendation: a hybrid, "touch it · open it"

**C for playing, B for editing, and A's page table as the shared data model.**

1. **Touch it (C):** the 4 macros and the envelope are drawn on every emitter, pad and lane
   and are playable there. This is what you use while performing, and there is no strip.
2. **Open it (B):** one tap (›, a clip or lane header) or a long-press on an emitter or pad opens
   that clip's page, with everything visible and grouped. This replaces the Deep drawer: same
   depth, but a calm page instead of a list, and one tap back to play. The global Project page
   uses the same layout.
3. **A as structure, not chrome:** the clip page's cards *are* A's pages (Shape · Grain ·
   Pitch · FX · Link), in the same order and with the same 8-slot groupings. Those pages are
   exactly what the plugin publishes to the host and to MIDI controllers. On desktop, the
   A-strip can be an optional "controller view" toggle for people with hardware knobs; it
   costs nothing to keep because it is the same table.

This keeps v001's play-first calm, makes every parameter one gesture or one tap away (nothing
hidden), and gives the plugin a clean parameter map. If only one direction ships first,
**B** gives the most for the effort: it lands Chop, ADSR and Dig in one obvious place.

## Implementation notes → egui

- **Page table** (`PAGES` in app.js): `kind → page → [slot items]`. It drives A's strip, B's
  cards and C's popovers. In Rust: `const PAGES: &[(Kind, Page, [Slot; 8])]`, with each `Slot`
  carrying a parameter ID (host-automatable).
- **ADSR widget**: an envelope polyline over the clip thumbnail; a horizontal gradient mask
  (hard stops for `cut`, envelope-shaped for `fade`) shows *when the picture is on*. Time axis
  is sqrt-scaled so short percussive envelopes stay editable. In egui: `Mesh` with
  per-vertex alpha for the lit band, plus 3 draggable handles (22 px hit radius).
- **Chop widget**: a 16-frame filmstrip plus a waveform canvas (audio peaks, or "picture
  energy" bars for silent clips), alternating slice bands, drag edges, double-tap to delete,
  and Tap mode (a looping playhead where each tap drops a cut).
- **Tile compositor** (`tileRects`): stable slot assignment (1 → 2 → 2×2 → 3×2 → 3×3 → 4×4),
  hairline gap, black between tiles, hard cuts via ADSR `cut`, and tile column → pan
  (`slotPan`).
- **On-object drawing** (`objSVG`): ring, dots, pitch dot, smear trail, envelope line and level
  bar are all plain shapes, so the same painter code draws the emitter cards, pads, lane
  headers and grid markers.
- **Dual FX amount** (♪ audio / ▣ picture): two concentric arcs in a knob, or two hairline bars
  in a 44 px pill.

## Screenshots (`shots/`)

All are `261112_chromagrain_design_<dir>_<view>_v002.png`, each at 1600×1000 and at
960×600 (the `-960x600` variants).

| dir | views |
|---|---|
| `a-pages` | `play` (emitter MACRO) · `play-source` (SOURCE page with mini-chop) · `sequence` (lane SHAPE page) · `perform` (pad MACRO, **tile** layout) · `project` (LIBRARY page) |
| `b-clip` | `play` (v001 surface + Clip ›) · `clip-page` (Emitter 2 owner) · `project-page` · `perform-slices` (beanstalk chopped → slice pads playing) |
| `c-on-object` | `play` (emitter cards with on-object rings, gesture bubble) · `perform` (pad mini-strip + Shape popover) · `sequence` (lane mini-strip) · `project` (Project popover) |
| `d-hybrid` | `play` (on-object emitter rings/cards + `Clip ›` buttons + object bar `Open clip ›`) · `clip-drawer` (calm Clip sheet opened over Play for `Emitter 1 · horses` with `← Play` and owner tabs) · `perform` (Tile layout with chopped slices, ADSR curves + `›` on pads) |
| `shared` | `add-dig` (two finds landed, column 8 filled) · `chop-tap` (tap-to-chop running) |

## Versions
- **v002**: three directions (A pages · B clip page · C on-object) plus a hybrid
  recommendation. Adds Chop, ADSR (sound + picture cut/fade), stack | tile, and the Add sheet with
  Dig. The Deep drawer is removed; the top-bar `Deep` button becomes `Project`.
- v001: first take (neutral, Tempera grid, nanoloop steps, pads + fullscreen, Deep drawer).
