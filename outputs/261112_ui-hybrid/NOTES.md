# chromagrain · UI D · Hybrid ("touch it · open it")

## v001 (261112)

Implements the `D · Hybrid ("touch it · open it")` parameter-exposure design in `crates/ui`, `crates/app`, and `crates/plugin` (reference: `design/v002`):

- **Calm `Project` & `Clip` modal sheets replace the right-hand `Deep` sidebar drawer**:
  - Top bar replaces `Deep` with `Project` (`Icon::Grid`, `Sheet::Project`).
  - `Studio::deep` backwards-compatibility shim routes `DeepTarget::Global` to `Sheet::Project`, `DeepTarget::Emitter(i)` to `Sheet::Clip(ClipTarget::Emitter(i))`, and `DeepTarget::Lane(l)` to `Sheet::Clip(ClipTarget::Lane(l))`.
  - `Sheet::Project` presents `TIME & KEY`, `SONG · SCENES`, `LIBRARY` (`+ add` & `Dig`), `MASTER EFFECTS ♪ ▣` (dual audio/video wet bars), and `SCRIPT` (standalone host), scrolling cleanly at `960×600`.
- **Play view on-object emitter cards + grid pitch rings**:
  - Contextual object bar (`Emitter N · <clip>` + primary `Open clip ›` pill).
  - 2×2 grid of on-object emitter cards with footage backdrop, grain ring + golden-angle cloud dots, circumference pitch dot + semitone readout, smear trail lines, bottom-left mini ADSR curve, right-edge level bar, top-right `Look` chip, bottom-right `Clip ›` pill, direct drag/wheel gestures, and floating dark HUD readout pill.
  - Grid column headers show `<clip> ›` (one-tap `›` or long-press opens `ClipTarget::Column(c)`), and grid emitter rings draw their circumference pitch dot.
- **Perform & Sequence on-object overlays + one-tap `›`**:
  - Perform pads draw top-left mini ADSR curves, top-right `›` buttons (`ClipTarget::Pad(i)` without toggling the pad), and on-object grain/pitch ring on `sel_pad`, plus contextual `Pad N · <name>` + `Open clip ›` in the bottom bar on wide windows.
  - Sequence lane headers draw bottom-left mini ADSR curves and top-right `›` buttons (`ClipTarget::Lane(l)` without muting), plus contextual `Lane N · <name>` + `Open clip ›` below the song chain on wide windows.
- **Unified `Sheet::Clip` (`crates/ui/src/studio/clip.rs`)**:
  - Header with `< Play` / `< Sequence` / `< Perform` back pill, clip title + provenance/slices, in-place Owner Switcher (`Clip`, `Emitter 1..4`, `Lane 1..8`, `Pad 1..16`), `Replace`, and `Dig`.
  - Top band: live clip stage preview (`stack | tile`) + 6 owner macros (`GRAIN`, `PITCH`, `SMEAR`, `LOOK`, `LEVEL`, `PAN`).
  - Middle band: Koala-style `CHOP` card with 16-frame filmstrip, audio waveform (plus picture-energy bars on silent video clips), edge grips, tap-chop, and `Slices → pads`.
  - Bottom band: 5 parameter cards (`SHAPE · SOUND + PICTURE`, `GRAIN`, `PITCH`, `EFFECTS ♪ ▣`, `LINKS`) wired to the active owner.

### Snapshots (`v001/`)
Generated headlessly via `egui_kittest` + `wgpu`:
- `261112_chromagrain_ui-hybrid_play_1600x1000_v001.png` & `960x600`
- `261112_chromagrain_ui-hybrid_clip-sheet_1600x1000_v001.png` & `960x600`
- `261112_chromagrain_ui-hybrid_perform-tile_1600x1000_v001.png` & `960x600`
- `261112_chromagrain_ui-hybrid_sequence_1600x1000_v001.png`
- `261112_chromagrain_ui-hybrid_project-sheet_1600x1000_v001.png` & `960x600`
- `261112_chromagrain_ui-hybrid_plugin-play_1180x820_v001.png` & `960x600`
- `261112_chromagrain_ui-hybrid_plugin-clip-sheet_1180x820_v001.png` & `960x600`
