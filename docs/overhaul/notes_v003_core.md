# v003 core notes: dig, chop, ADSR, tile (261112)

Core side of [spec_v003](spec_v003_dig-chop-tile-adsr.md), branch `feat-core-v003`.

## Dig / Link (`crates/core/src/dig.rs`, desktop `MediaBackend`)
- `dig(&DigOptions) -> Job`, `dig_many(n, ..)`, `fetch_link(url, ..) -> Job`. `Job` has `status()`, `progress()` (0..1), `title()`, `take_source()`, `cancel()` and `wait()`. Jobs run on a background thread.
- The query is a camera-default filename (`IMG_####`, `MVI_####`, `DSC_####`, `DSCN####`, `VID_YYYYMMDD`, …) or a random word, run through `yt-dlp --flat-playlist --dump-json "ytsearchN:<q>"`.
- Match filter (`DigOptions` defaults):
  - fewer than 500 views
  - 10 s–10 min long
  - not live
  - optional `cc_only` (yt-dlp `license` contains "Creative Commons")
- One candidate is picked at random and downloaded at 480p or less (progress is streamed). A random 20–60 s window is cut out and loaded as a `Source`, with `Source.provenance` filled in: title, uploader, upload date, views, url, license, query and window.
- When nothing passes the filter, it retries with a new query (6 attempts).
- Env overrides:
  - `CHROMAGRAIN_YTDLP=<bin>` points at the yt-dlp binary.
  - `CHROMAGRAIN_DIG_SEED=<u64>` makes query and pick deterministic.
  - **Stub for tests and the showcase:** `CHROMAGRAIN_YTDLP=tools/ytdlp-stub CHROMAGRAIN_YTDLP_STUB_DIR=<finds>`. It serves `search.jsonl`, `<id>.json` and `<id>.<ext>`, so pre-dug, CC-licensed finds come back deterministically. An `empty_searches` file containing a number N makes the first N searches return nothing, which exercises the retry.
- Normal tests never touch the network. `dig::tests::real_dig` is `#[ignore]`d.
- A manual real dig succeeded headless in 15 s: "DSCN7462", 4 views, uploaded 2011-02-15, youtube.com/watch?v=E6Q741Pl1uU.

## Chop (`chop.rs`)
- `Slice{start,end}` is in source seconds. `Source.slices` is always normalized: sorted, non-overlapping, inside `0..len`, and at least 10 ms long.
- Ways to make and edit slices:
  - make: `auto_slices` (transients), `equal_slices(len, n)`, `slices_from_points`
  - tap: `add_chop_point` splits a slice, or starts one when there are none
  - edit: `move_boundary` (a shared edge moves both slices), `set_slice_edges`, `delete_slice` (the previous slice absorbs it)
  - lookup: `slice_for_note`, `slice_at`
- Mapping out:
  - **Pads:** `Instrument::slices_to_pads(source, first)` gives one-shot pads (`PadSlot::from_slice`). A pad hit plays now, not launch-quantized.
  - **MIDI:** in `Instrument`, MIDI note 48 (C3) and up plays slice `note-48`, at unity pitch, and stops at the slice end. Note precedence: an *assigned* pad note (36–51) wins over a slice.
  - **Steps:** `Step.slice_index` plays that slice, starting at its start, for `slice length / pitch ratio × step length`.

## ADSR (`envelope.rs`)
- `Adsr{attack, decay, sustain, release, picture: Cut|Fade, cut_threshold=0.05}`. A voice carries a `VoiceEnv{adsr, on, off}` in `GrainEvent::tag.env` on every grain.
- **Audio:** each output sample is multiplied by `env.level_at(sample_time)`, so the result is identical for any block size and realtime matches offline.
- **Picture:** `picture_at(t)`. With `Cut`, opacity is 1 while the level is above the threshold, then 0. With `Fade`, opacity equals the level.
- Where the envelope applies:
  - **Notes and slices:** `Instrument::set_note_adsr`. Note-off releases the voice. Slice pads use `PadSlot.adsr`, falling back to the note ADSR.
  - **Clip:** `GrainSettings.adsr` on a granular/snippet clip turns the whole clip into one voice, released so that the release ends at the clip end.
  - **Lane (pattern row):** `row.grains.adsr`. Each hit is a voice with gate = the hit length. Its grain grows by the release and its window goes flat.
  - **Live emitters:** `RealtimeAudio::release_voice(voice, off)` (live clock) patches the queued and sounding grains of a voice and drops any that start after the release.

## Tile (`tile.rs`)
- `Project.layout: Stack|Tile`, `Project.layout_pan` (default true); `Instrument::set_layout / set_layout_pan`.
- **Slots:** `TileAssigner` gives each voice a slot when it starts. A voice keeps its slot. A new voice takes the lowest free slot. When all 9 slots are full, voices share slot `voice % 9`. The table is fixed-size, so assignment doesn't allocate.
- **Grid:** `frame_grid(slots)` → 1×1, 2×1, 2×2 or 3×3. It is sized for both the number of tiles and the highest slot in use, so it never shrinks under a live tile.
- **Pan:** `layout_pan(slot, cols, rows)`. The column maps to pan across ±0.8, and each row down is −1 dB. The grid used is **the grid at each grain's onset**, so a voice's pan follows its tile as the grid grows or shrinks. A grain's final pan is `layout pan + 0.2 × its own pan`.
  - Instrument: `TileAssigner::grid_at(onset)` counts voices with `start <= t < end`.
  - Render plans: `render::assign_tiles` uses `tile::grid_timeline(spans)` + `grid_in`.
- **Offline per-frame grids:** `render::frame_tile_grids`.

### GPU parity (crates/ui tile pass), mirrors `video::grain_draws_layout(.., tile_grid)`
1. **Grid per frame:** `(cols, rows) = frame_grid(distinct tag.tile of grains with onset <= t < onset + duration and tag.picture_at(t) > 0)`.
2. **Destination:** `tile_rect(slot, cols, rows, aspect)`, normalized and y-down.
   - The gap is `TILE_GAP` = 0.004 of the shorter canvas side, between tiles only (none at the canvas edges).
   - An out-of-range slot clamps to the last cell.
3. **Source patch (centered cover crop):** `tile_ar = w·aspect / h`. If `tile_ar < aspect`, the patch is `(tile_ar/aspect, 1)`; otherwise it is `(1, aspect/tile_ar)`. Either way it is centered. This assumes the source aspect equals the canvas aspect.
4. **Look:** alpha = `picture_at(t)`, no rotation, normal blend, no feather, shadow or additive. A tile is drawn only while `t < onset + duration`, so there is no visual tail and the cuts are hard.
5. **Stack mode:** grain alpha is multiplied by `picture_at(t)`. That is the only change.

## Known approximations
- The row cue is a −1 dB gain per row, not the clip's spectral-tilt link.
- The cover crop assumes the source aspect equals the canvas aspect.
- **Clip ADSR:** the release ends at the clip end. It is not applied to pattern clips; their rows have their own ADSR.
- **Instrument lookahead:** grains already scheduled within the 'lookahead' window before a new voice starts keep the old grid's pan. This holds for every block size, so the output is still block-size independent.
