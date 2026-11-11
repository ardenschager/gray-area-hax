# chromagrain overhaul — plan (v001)

**Goal:** make chromagrain something you *play*, not configure. Keep the core idea (sample videos → granulate → arrange video art) and get it running as a **standalone Mac app and a plugin inside Logic**.

## 1. Rust or C++?

**Recommendation: keep the Rust engine and wrap it as plugins. Don't rewrite in C++.**

| | Keep Rust engine + plugin wrappers | Rewrite in C++ (JUCE) |
|---|---|---|
| What carries over | All ~20k lines, 136 tests, deterministic renderer, plyphon SC instruments | Nothing, so every feature gets rebuilt and re-debugged |
| Formats | **VST3 + CLAP** (nih-plug), **AU for Logic** (clap-wrapper → AUv2), **AUv3** later through a small Swift app-extension shell that calls the Rust core over a C ABI | VST3 / AU / AUv3 / standalone straight from JUCE |
| "Native" | Already a native Metal/OpenGL Mac binary, with no Electron or web layer | Same |
| Licensing | GPL-3 (already the case because of plyphon) | JUCE is AGPL or paid; plyphon would need replacing |
| Risk to "everything still works" | Low, because the tests keep guarding the engine | High |

C++ only pays off if you want JUCE's ecosystem or plan to sell closed-source. Neither is needed to get it into Logic.

## 2. What the plugin is

The DAW becomes the arranger, and chromagrain becomes **a video-sampler instrument**:

- Load a video into the plugin. MIDI notes play it (sampler mapping or grains), and audio goes out to the DAW track.
- The picture shows in the plugin window. It also goes to a **fullscreen output window**, plus Syphon so Resolume/VDMX/OBS can pick it up. The picture is locked to the host transport.
- Several instances on several tracks get composited into one canvas through a shared output, which gives you video-art arrangement in Logic.
- The standalone app keeps its own simple arranger for when you're not in a DAW.

## 3. Interface: "play first"

Today's UI shows every parameter at once: modes, 20 sliders, and 6 collapsible sections per clip. The new one hides almost all of it.

```
┌──────────────────────────────────────────────┐
│                                              │
│            BIG CANVAS (the video)            │
│     touch / drag on it = play the grains     │
│   x = where in the video,  y = grain size    │
│                                              │
├──────────────────────────────────────────────┤
│ [vid][vid][vid][vid][+]   ← your sources     │
├──────────────────────────────────────────────┤
│  GRAIN   PITCH   SMEAR   LOOK ◐   ● rec      │  ← 4 macro knobs
├──────────────────────────────────────────────┤
│  ▭▭▭▭  ▭▭  ▭▭▭▭▭▭     takes, laid out in time  │  ← arrangement
└──────────────────────────────────────────────┘
```

- **Grab a video → it's already playing.** The canvas itself is the instrument (XY play, like a Kaossilator), and keys and MIDI play it too.
- **Four macros instead of ~40 parameters:**
  - *Grain*: sample ↔ cloud, using the existing morph
  - *Pitch*: snapped to one global key
  - *Smear*: paulstretch / delay / reverb
  - *Look*: cycles through the one-click looks
- **● rec captures what you play** as a take. Takes go onto a single arrangement lane, so arranging means moving takes around rather than building clips by hand.
- **One global key and tempo**, set once and out of the way.
- Everything else (automation, filters, polymeter, correspondence dials, scripting) moves into a **"deep" drawer**. It still exists and is still tested, but it's never on screen by default.
- Classic UI stays behind `--classic` until the new one reaches parity, so nothing is lost while we build.

## 4. Phases (each one a git commit or branch, each one ends with a `gx show`)

1. **Mockups:** 2–3 interactive HTML directions for the play-first UI. You pick one.
2. **Engine API:** extract `chromagrain-engine`, an instrument-style API: load source, note on/off, macros, `render_block(audio)` + `render_frame(video)`, host-tempo sync. It gets its own tests, and the existing 136 still pass.
3. **New app UI** on that API (egui + the existing GPU pipeline), with the classic UI kept behind a flag.
4. **Plugin:** nih-plug CLAP + VST3 and AUv2 for Logic. The plugin window has the canvas and the 4 macros. Add the fullscreen/Syphon output. Validate with `auval` and in Logic.
5. **AUv3 (optional):** Swift app-extension shell, which also opens the door to iPad.

> [!IMPORTANT]
> "Everything still works" means the full test suite stays green at every step, and new engine/macro tests get added. Removing features from the *screen* is not the same as removing them from the code.
