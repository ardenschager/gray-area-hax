# Chromagrain VGS Studio UX & Stage Shaders — `v001` (`261113_ui-vgs`)

## Overview
Version `v001` upgrades Chromagrain's `D · Hybrid` ("touch it · open it") Studio UI and `wgpu` real-time stage compositor across `crates/ui/`, `crates/app/`, and `crates/plugin/` for both hyper-articulate rhythmic collages (*The Avalanches*, *The Books*, *Aphex Twin*, *Burial*, *Granular-Synthesis — MODELL 5*) and disintegrating ambient tape loops (*William Basinski*, *The Caretaker*), with **zero AI / LLM calls**.

## What Changed vs. `ui-hybrid` (`261112`)
1. **Add Sheet (`YouTube | Dig | Drive | Files`):**
   - **YouTube Search + Paste Link:** Direct `yt-dlp` metadata search bar with `<500 views` filter chip, starter query chips, clickable result rows (`title`, `uploader`, `views`, `duration`), and direct URL paste/fetch.
   - **Dig:** Added 1-click **`Dig 4 → Kit`** alongside 1-tap / hold-to-dig-4 unseen camera-roll roulette.
   - **Drive:** Local + Google Drive folder discovery (`~/Library/CloudStorage/GoogleDrive-*`, `/Volumes/GoogleDrive*`) with `All | Drive | Local` scope pills; removed all cloud embedding / Gemini API calls.
2. **Stage Modes, Aspect Framing & `wgpu` Compositor (`gpu.rs`):**
   - Upgraded stage pill to **`stack | split | grid | bleed`** + **`16:9 · 1:1 · 9:16`** aspect framing pill.
   - **`Split` (`Split + Bleed`):** Dynamic `1-up / 2-up / 3-up / 4-up` panels (`4×1 / 3×1 / 2×1` side-by-side portrait monoliths on `16:9` widescreen per *MODELL 5*; `1×2` on `9:16`; `2×2` quad on `1:1`/`9:16`), `+6.5%` feathered screen-blended interior seams, and full-canvas `~30%` sustained wash bleed.
   - **`Grid` (`Tile`):** Crisp architectural panels with hairline obsidian dividers and smart `(cx, cy)` cover crop.
   - **`Bleed`:** Full-screen optical luma-key + screen/exclusion multi-exposure melt.
   - **Optical Polish & *MODELL 5* Micro-Bursts:** `52 ms` optical carry across rapid 16th cuts, transient `zoom_punch`, Basinski `oxide_dropout` wear, polar radial `Kaleido` (`fs_kaleido`), Perform `Tape Stop` VHS roll (`fs_tapestop`), kinetic spoken-hook **Caption** pill, and 100% full-cell hard-frame micro-bursts when `PictureCut::Cut` + tight `spray <= 0.08`.
3. **Clip Sheet & Multi-Clip Harvester (`clip.rs`):**
   - Subtle acoustic slice badges (`sub`, `snap`, `air`, `C#3`, `phrase`), **`Kit → pads`** (16-pad cross-clip kit harvester), and **`Loops → pads`** (unsynced free-running tape loops).
   - **`SCULPT & TAPE` Card:** 1-tap stamps (`Raw · Kick · Snare · Hat · Pluck · Chord · Phrase · Tape Loop · Disintegrate`) populating 7 continuous dials (`Drop`, `Sub`, `Snap`, `Drive`, `Comb`, `Wow`, `Wear`) + `Chord`, `Choke`, `Sync | Free Loop (s)` selectors, `Caption` field, and draggable `(cx, cy)` focus crosshair on preview.
4. **VJ Set Arranger & Perform Live Bar (`sequence.rs`, `perform.rs`, `deep.rs`):**
   - **Sequence Step Hold-Sheet:** Added `Ratchet` (`×1..×8` + pitch slope), `Offset` (microtiming pocket), and `Chord` alongside `Slice`, `Pitch`, `Length`, `Chance`.
   - **Shared Set / Cue Arranger Bar:** Up to 12 inline-editable named scenes (`intro`, `drop`, `dialogue`, `wash`…), per-cue repeat counts (`×1..×16` or `∞`), `+ cue`, `dup`, and `∞ hold` live improv lock across `Sequence` and `Perform`.
   - **Perform VJ Punch-In FX:** 6 momentary/latching pills (`Stutter`, `Tape Stop`, `Comb`, `Dub`, `Shimmer`, `Wear`).
   - **60 fps Export:** `30 fps | 60 fps` selector right beside `Export` in `Sheet::Project`.

## Rendered Snapshots (`v001`)
- `261113_chromagrain_vgs_play_1600x1000_v001.png` & `261113_chromagrain_vgs_play_960x600_v001.png`
- `261113_chromagrain_vgs_add-youtube-search_1600x1000_v001.png`
- `261113_chromagrain_vgs_add-dig-kit_1600x1000_v001.png`
- `261113_chromagrain_vgs_add-drive_1600x1000_v001.png`
- `261113_chromagrain_vgs_clip-sculpt-tape_1600x1000_v001.png`, `1180x820`, `960x600`
- `261113_chromagrain_vgs_sequence-ratchet-set_1600x1000_v001.png`
- `261113_chromagrain_vgs_perform-split-bleed-9x16_1600x1000_v001.png`
- `261113_chromagrain_vgs_perform-bleed-16x9_1600x1000_v001.png`
- `261113_chromagrain_vgs_plugin-play_960x600_v001.png`, `plugin-clip_960x600_v001.png`, `plugin-perform_960x600_v001.png`
