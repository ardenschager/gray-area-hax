# chromagrain · design v001

An interactive design prototype for the new chromagrain interface. Open `index.html`
directly in a browser (file:// works). For real granular audio from the clips' soundtracks,
serve the folder over http; over file:// the grains fall back to a key-snapped synth.

```
python3 -m http.server -d design/v001 8000   # then http://localhost:8000
```

`?demo&view=play|seq|perform` starts a scripted performance, used for the screenshots.
Extra flags: `&pop` (step hold sheet), `&d2` (Display 2 state), `&deep=emitter|lane`.

## What changed vs. the current UI (docs/screenshot.png, docs/perform.png)

| current | v001 |
|---|---|
| Inspector with about 40 sliders, 6 collapsible sections, and modes | 4 knobs per emitter. Everything else is behind one drawer |
| Library, timeline, pattern grid, and pads all fighting for space | 3 views, one at a time: **Play · Sequence · Perform** |
| Salmon, green, blue, and purple UI colours | Neutral graphite with one off-white highlight. Colour comes only from the footage |
| Explanatory text everywhere ("drag clips to move…") | No instructional copy. Where a screen seemed to need it, the screen was simplified |

## Principles

1. **The video is the interface.** Every primary surface (the Play grid, the step cells, the
   pads) is made of frames from your clips. The chrome is grey so the footage can be the only colour.
2. **Every audio grain is a visual grain.** Touching the grid sprays patches of picture with the
   same life as their sound: envelope → opacity, pitch → hue, pan → x, position → frame.
3. **Three views and one drawer.** Play (Tempera), Sequence (nanoloop), and Perform (pads + output).
   Effects, filters, automation, correspondence links, polymeter, pattern slots, and script all
   live in the **Deep** drawer. It opens from the top bar, from the emitter's `⋯`, and from a
   long-press on an emitter or lane header. It is never open by default.
4. **Touch-first.** Every target is at least 44 px. Nothing depends on hover or right-click: hold
   opens detail, tap toggles, and vertical drag changes a value. Keyboard shortcuts are extras
   (space, Tab / 1-2-3, F, R, Esc).
5. **Resizable plugin window.** The layout holds from 960×600 (plugin minimum) through iPad
   landscape (1180×820) to 1600×1000 and up. Below 1100 px wide, the Sequence arrangement column
   moves into a top strip.
6. **Sampling first.** No synth or instrument panel anywhere. If a synthesized source exists, it
   appears as one entry in the add/source picker. The 8th Play column and 8th Sequence lane are
   the empty "drop a video" slots.

## The three views

### 1 · Play: the Tempera model, translated to video
- **8×8 video grid.** Each column is one clip, laid out top → bottom as 8 frame tiles: a
  contact sheet you can play. Column 8 is empty and accepts a dropped clip.
- **4 emitters** (numbered rings) are brushes that each hold their own grain settings. Touch
  anywhere on the grid and the selected emitter jumps there and sprays grains while held.
  Grab an emitter ring to play that one instead. Multi-touch plays several emitters at once.
  The cells under a spray light up.
- **Canvas** (top right) shows the result: the active column's video as the base layer, plus
  grain patches lifted from the same spot in a *different moment* of the clip.
- **Brush panel**: 4 emitter tiles (their background is the frame they're sitting on), the
  selected emitter's 4 macros (**Grain** sample↔cloud, **Pitch** snapped to the global key,
  **Smear** persistence + reverb/echo, **Look** detented: clean, tint, crush, kaleido, cutout,
  negative), **Rec**, and `⋯` (emitter page in the drawer).

### 2 · Sequence: nanoloop-simple
- One lane per clip. The lane header is the clip itself. Tap it to mute; hold it to open the
  lane page (length, swing, patterns A–H).
- **16 steps. Tap toggles a step.** An active step *is* a thumbnail of the slice it triggers
  (chop sequencing for video). **Drag up/down** on a step to set pitch (in key degrees).
  **Hold** a step to open a 3-parameter sheet: slice picker (16 frames), pitch, length, and chance.
- **Polymeter** stays but is quiet. A lane shorter than 16 simply ends with `↺`, and each lane's
  playhead ring runs at its own length.
- **Right column = arrangement + monitor**: a live canvas of the hits, **scenes 1–8** (a tiny
  bar per lane shows which pattern it plays; tap to queue at the next bar; tap an empty scene
  to copy the current one), and the **song chain** (`1×2 2×2 3×4 …`) with a progress fill.
  Lanes that are silent in the current scene are drawn ghosted.

### 3 · Perform: pads and the output
- **4×4 pads**, each a clip with a mode (`∞` loop, `⁘` grain cloud, `⫶` chop) and a look.
  Playing pads animate their thumbnail and draw a bar-progress line. Queued pads blink a small
  triangle. **Launch quantize**: Now / Beat / Bar. Up to 4 layers composite at once.
- **Fullscreen (F or the button)** fullscreens just the output: no chrome, and the cursor hides
  after 1.5 s. Tap or move to summon a minimal HUD (tempo, beat dots, what's playing, and a
  tappable 4×4 mini-pad grid). It auto-hides after 2.6 s.
- **Output → Display 2** opens a chromeless output window (driven over a BroadcastChannel) that
  you drag to the projector and fullscreen. The laptop keeps the controls, and the stage turns
  into a dimmed monitor labelled "Live on Display 2". In the Rust app this is a second
  `egui::ViewportBuilder` window (or Syphon/Spout out), placed with the OS screen list.

## Tokens

| token | value | use |
|---|---|---|
| `g0` | `#0b0b0c` | app bg, letterbox |
| `g1` | `#121213` | panels, empty step |
| `g2` | `#18181a` | raised: chips, cells, buttons |
| `g3` | `#202022` | pressed |
| `g4` | `#2a2a2d` | knob body top, selected segment |
| `g5` | `#3a3a3e` | idle dots and strokes |
| `line` / `line-2` / `line-3` | white @ 6% / 10% / 16% | 1 px hairlines |
| `ink` / `ink-2` / `ink-3` / `ink-4` | `#ecebe8` / `#9a9894` / `#5f5d5a` / `#3e3d3b` | text ramp |
| `hi` | `#f3f2ee` | **the only highlight**: selection, playhead, knob arc, active view |
| `rec` | `#e5484d` | only while recording |
| per-source swatch | sampled from the clip (`SOURCES[].sw`) | 3 px lane-header edge only |

Radii 4 / 6 / 10 / 14 / 18. Spacing 4-pt grid; page padding 20 (16 / 14 when small).
Touch minimum 44 px. Motion: `cubic-bezier(.2,.7,.1,1)`, 150–320 ms. Grain flashes are 90–320 ms.

**Type**: Geist (UI, 13 px base, −0.005em) + Geist Mono (all numbers: tempo, position, knob
values, pitch, scene numbers). Labels are 10.5 px uppercase at +0.08em tracking, `ink-3`.
Both fonts are OFL and can be embedded in egui via `FontDefinitions`.

**Texture**: a 2.8% fractal-noise film grain over the whole window (one tiled texture), plus a
1 px top inner highlight on raised surfaces. There is no blur anywhere.

## Components → egui custom painting

| component | egui implementation |
|---|---|
| Top bar segmented view switch | `Painter::rect_filled` track + an animated thumb (`ctx.animate_value_with_time`). Labels via `painter.text` |
| Transport / chips | `Button` with custom `Visuals` (g2 fill, 1 px `line` stroke, 10 px rounding); the playing state inverts to `hi` |
| Video grid cell / step cell / pad | `Image` UV sub-rect into a per-clip **frame atlas** (8×6 frames at 240×135), `Rounding` 6–12. The live video texture goes on the canvas only. Brightness/saturation dimming = tint colour multiply |
| Emitter ring | `circle_stroke` 1.5 px `hi` + `circle_filled` when selected; ripple rings = 3 animated circles with fading alpha |
| Knob | `Shape::Path` arc (270°, 5 px, round caps) + filled disc with a vertical gradient (`Mesh` with 2 vertex colours) + indicator line. Look knob = detent dots instead of an arc. Drag-y via `Sense::drag`; double-tap resets |
| Step hold sheet | `egui::Area` popup anchored to the step rect; 8×2 slice atlas images + 3 drag-value tiles |
| Scenes / song chain | rounded rects + mono text; the chain progress is a filled sub-rect |
| Canvas / output | the existing wgpu grain-quad pipeline (`egui_wgpu::Callback`). Each grain = a textured quad with the envelope as alpha, pitch → hue uniform; Looks = existing effect passes |
| Fullscreen HUD | separate `egui::Area` layers at 50% black fill; visibility on pointer activity with a timer |
| Deep drawer | right `SidePanel` animated with `animate_bool`, plus a scrim rect. Rows = label + custom 2 px slider + mono value |
| Film grain | one 160 px noise texture tiled with `Painter::image` at 3% alpha, drawn last |

Avoided on purpose: backdrop blur, CSS 3D, blend-mode stacks in the chrome. The canvas uses
blend modes (lighten, color, difference) only inside the video compositor, where the wgpu
effect passes already handle them.

## Media

`media/` holds 7 archival clips (6 s, 640×360, h264, ≤ 600 KB each) cut from
`vibecut/videos/`, plus one frame atlas and one poster per clip. Total is about 4.6 MB.

| id | name | source | in |
|---|---|---|---|
| oregon | horses | 098566_1938_state_of_oregon_promotional_reel.mp4 (center-cropped from pillarbox) | 313 s |
| betty | beanstalk | betty_boop_beanstalk.mp4 | 210 s |
| aurora | aurora | aurora_borealis.mp4 | 1 s |
| bees | comb | bees_and_spiders_macro.mp4 (+25% contrast) | 92 s |
| nyc | concourse | arteries_of_new_york_city.mp4 | 297 s |
| hummingbird | hummingbird | hummingbirds_at_play.mp4 | 4 s |
| map | rivers | arteries_of_new_york_city.mp4 | 278 s |

## Versions

- **v001**: first take. Neutral palette, Tempera-style Play grid with 4 emitters,
  nanoloop-style Sequence with hold sheet, scenes + song chain, Perform pads with real
  fullscreen, HUD, and a Display 2 output window. Deep drawer with Links / Effects /
  Automation / Script and lane/emitter pages. An earlier colourful single-canvas Play layout
  was replaced before the first commit, following Arden's "neutral, Tempera, nanoloop" note.

## Open questions for Arden

1. Play grid: should touching an empty spot *move* the selected emitter (current), or *place
   a new* one, as on Tempera (4 max)?
2. Should Sequence lanes be tied to the Play grid's columns (lane N = column N, as now), or be
   free so one clip can appear on several lanes?
3. Should Perform pads be one global bank, or one bank per scene?
