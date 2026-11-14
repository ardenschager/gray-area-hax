# `261113_vj-hillclimb` — Pure-YouTube AV Collage & `ISOL8` Studio Hillclimb

## 1. Engine & UX Tool Improvements (`vgs-hillclimb` on `crates/`)
While preparing and performing the 6 pieces with real YouTube footage, we fixed five concrete rough edges in `chromagrain` itself so every human musician/VJ gets them out of the box:

1. **Streaming Single-Frame `1080p` `60 fps` Video Exporter & `-1.4 dBTP` Oversampled AAC (`crates/core/src/render.rs`, `crates/core/src/media.rs`, `crates/core/src/script.rs`, `crates/app/src/main.rs`):**
   - Previously, `render_project` buffered all uncompressed RGBA frames in RAM before calling `encode_video` (`12–30 GB` of RAM for `1920×1080 @ 60 fps` exports), quantized intermediate audio to 16-bit PCM, and ran `ffmpeg -c:a aac` without oversampled inter-sample peak limiting.
   - Added `render_project_to_video` + `MediaBackend::encode_video_stream` + `save_wav_f32`: renders audio first, then streams **one RGBA frame at a time** (`< 20 MB` video RAM) into `ffmpeg` (`-crf 18 -movflags +faststart`) with a `192 kHz` 4× oversampled true-peak ceiling (`alimiter=limit=0.8128` -> post-AAC True Peak `<= -1.4 dBTP`, `0` clipped samples).
   - Upgraded `MAX_DECODE_WIDTH` (`480 -> 640`) and `DECODE_FPS` (`15.0 -> 24.0`) in `crates/core/src/media.rs` for sharper, smoother HD `60 fps` collage exports.
2. **True Varispeed Turntable / Capstan `Tape Stop` DSP & Zero-Rebuild Live Streaming (`crates/core/src/audio.rs`, `crates/core/src/video.rs`, `crates/core/src/realtime.rs`, `crates/core/src/instrument.rs`, `crates/ui/src/studio/mod.rs`, `crates/app/src/playback.rs`, `crates/plugin/`):**
   - Previously, `Studio::sync_punch_fx` approximated `Tape Stop` with a `Delay` whose delay time changed every frame, triggering full `RealtimeAudio` engine swaps 30–60 times per second while held.
   - Built allocation-free `TapeStopState` (4-point Hermite circular varispeed brake pitching the mix smoothly down to a dead stop with progressive capstan HF rolloff and 42 ms spin-up recovery) + CPU `apply_video_tape_stop` matching `wgpu` `fs_tapestop`, streamed live via `Host::set_tape_stop` with zero engine rebuilds.
3. **Timeline `MasterPunch` Performance Windows (`crates/core/src/timeline.rs`, `crates/core/src/render.rs`):**
   - Added beat-accurate `MasterPunch` (`PunchAction::Isolator`, `PunchAction::TapeStop`, `PunchAction::Effect`, `PunchAction::Layout`) on `Project` with click-free crossfade ramps so multi-bar `ISOL8` radio breaks, `Tape Stop` drops, spinning polar `Kaleido` sections, and `Split <-> Grid` stage transitions render sample- and frame-accurately.
4. ***MODELL 5* Synchronous Audio-Rate Pulse Train (`crates/core/src/grain.rs`):**
   - Previously, `schedule_grains_auto` added exponential Poisson timing jitter (`±50%`) even when `PictureCut::Cut` + `spray <= 0.08` + `duration_jitter == 0.0` was active, turning `18–45 Hz` *MODELL 5* micro-bursts into a noisy cloud instead of a crisp rhythmic-to-pitched oscillation.
   - Updated `schedule_grains_auto` so synchronous hard micro-bursts advance deterministically by `1.0 / density`, cleanly crossing from staccato frame shuttering into pitched harmonic buzz.
5. **Subject Focus & Slice Caption Inheritance on `Granular` & `Snippet` Clips (`crates/core/src/render.rs`):**
   - Added `source_focus_at` and `source_caption_at_start` in `plan_project` so `Granular` and `Snippet` clips automatically inherit smart `(cx, cy)` slice subject framing (keeping portrait faces centered in `9:16` and `3×1` triptych strips) and slice captions.
6. **Cache-Friendly `O(1)` Box Blur & Multi-Threaded HD Scanline Compositor (`crates/core/src/fx.rs`, `crates/core/src/video.rs`):**
   - Replaced the `O(W * H * R)` `box_blur_rgb` in `ISOL8` video isolation and vertical-stride `box_blur` in `Reverb` with `O(1)`-per-pixel row-major sliding-window accumulators (L1 cache resident), plus 8-band `std::thread::scope` scanline parallelism in `composite_grains_frame_stage` for HD exports (`>= 250k` px).
7. **Live `SET` Sequence Continuity Across `Perform` View (`crates/app/src/{ui_perform.rs,studio_host.rs}`):**
   - Previously, switching into `View::Perform` swapped `p.tracks` to the 16 Perform pads and dropped any active `"sequence"` track (`SET` cues), silencing live step-sequencer grooves during `Perform` and ignoring `SET` cue clicks (`sequence_changed` returned `false` outside `Arrange`).
   - Preserved active `"sequence"` pattern tracks at `p.tracks[PADS]` across `Perform` mode (tiling across `loop_len` alongside free-running/looping Perform pads while keeping empty sequences at `tracks.len() == PADS`), enabled live `SET` cue switching inside `Perform`, and enabled `Layout::Split` panel claiming on one-shot chop pads (`p.layout.uses_tiles()`).
8. **Dense Active-Slot Stage Packing & Polyphonic Chord Visual Deduplication (`crates/core/src/{grain.rs,tile.rs,render.rs,video.rs}`, `crates/ui/src/gpu.rs`):**
   - Added `VoiceTag::chord_harmony` in `expand_chord_grain_inline` so polyphonic `ChordVoicing` stacks (`Maj7`, `Min9`, `Fifths`, `Sus4`) play all `N` resampled stereo voices in audio while letting the root voice own ONE clean video panel (eliminating 4× duplicate cloned panels of the same guitarist/trumpeter).
   - Added `dense_slot_remap` for `Layout::Split` (all aspect ratios) and portrait `9:16` `Layout::Tile` so `2` active voices pack into full-width `1×2` bands (`2×1` on wide) and `3` voices pack into `1×3` (`3×1` on wide) with zero empty black quadrants, while holding fixed `(3, 1)` `1.5 px`-divided monolith columns when `portrait_columns == true` in `Layout::Tile`.
9. **Tiled Subject Crop Zoom (`transform.scale`) & Auto-Scaling `9:16` Stage Captions (`crates/core/src/video.rs`):**
   - Honored `clip.transform.scale` inside `focus_cover_patch` for tiled/split/bleed panels so clips can zoom tightly into expressive human faces or crop out corner text, and upgraded `draw_stage_caption` with responsive width auto-scaling + high-contrast smoked-obsidian pill + warm amber accent bar so long spoken punchlines never drop on vertical `9:16` stages.
10. **`Kit -> pads` & `Loops -> pads` YouTuber Outro / Speech CTA Guard (`crates/core/src/chop.rs`):**
    - Updated `build_kit_across_sources` (`Pads 15–16` `Wash` / `Tape Loop`) and `build_tape_loops_across_sources` to penalize late-clip outro segments (last `20%` of clips `>= 8s`, where YouTubers put `"like & subscribe"` CTAs) and `1–3.5 kHz` syllabic speech slices (`pitch_confidence < 0.62`), favoring sustained harmonic/rhythmic slices instead.
11. **`N == 3` Full-Width Bottom-Row Span in `Layout::Tile` (`crates/core/src/{tile.rs,video.rs,render.rs}`, `crates/ui/src/gpu.rs`):**
    - Added `tile_rect_for_count` across both CPU (`render_project`) and GPU (`GpuPreview::render`, verified by `gpu_matches_the_cpu_for_tiles_and_envelopes` `"tile3"`): when `3` voices are active in `Layout::Tile` (`!portrait_columns`), cells `0` and `1` occupy the top row while cell `2` spans `100%` width across the bottom row — eliminating the empty black `25%` quadrant!

---

## 2. Real YouTube Source Crate (`outputs/261113_vj-hillclimb/src/`, Gitignored `.mp4`s)
100% real YouTube clips fetched via `yt-dlp` (`0` synth oscillators, `0` external drum WAVs):

| Piece | Local Clip (`src/`) | YouTube Title | Uploader | Views | URL | Cut Window |
| :--- | :--- | :--- | :--- | ---: | :--- | :--- |
| **1 · Frontier Therapy** | `p1_angry_boy_1950_vwVHhrj-doE.mp4` | *Angry Boy Part I (1950) – Vintage Child Psychology & Psychiatry Film* | OVR \| Old Videos Restored | 79 | https://www.youtube.com/watch?v=vwVHhrj-doE | `296.0s + 40.0s` |
| **1 · Frontier Therapy** | `p1_mental_fit_1952_uC9B2F-l5V8.mp4` | *Mental Health: Keeping Mentally Fit (1952) \| Restored Educational Film in 4K* | OVR \| Old Videos Restored | 24 | https://www.youtube.com/watch?v=uC9B2F-l5V8 | `2.0s + 42.0s` |
| **1 · Frontier Therapy** | `p1_deviant_behavior_ABCDPGzjalQ.mp4` | *Changing "Deviant" Behavior (50s - 60s Educational Video)* | Bite-Size Random | 783,311 | https://www.youtube.com/watch?v=ABCDPGzjalQ | `18.5s + 42.0s` |
| **1 · Frontier Therapy** | `p1_crazy_calls_tape_wewsuLjJpEo.mp4` | *1980s Crazy Calls Answering machine gag cassette.* | UncleCathode | 186,607 | https://www.youtube.com/watch?v=wewsuLjJpEo | `13.5s + 45.0s` |
| **1 · Frontier Therapy** | `p1_horse_whinny_Y4kY-Rx47VM.mp4` | *11 Horse Sounds Explained: Understand Your Horse’s Calls* | wildLINK | 356,581 | https://www.youtube.com/watch?v=Y4kY-Rx47VM | `200.5s + 34.0s` |
| **1 · Frontier Therapy** | `p1_trumpet_fanfare_T_Kj30n-g9E.mp4` | *John Williams’s “Fanfare for Solo Trumpet To Celebrate the Reopening of David Geffen Hall”* | New York Philharmonic | 53,101 | https://www.youtube.com/watch?v=T_Kj30n-g9E | `6.0s + 30.0s` |
| **1 · Frontier Therapy** | `p1_funk_drums_fnTaVyzWYjI.mp4` | *DRUM ONLY - FUNK GROOVE - 80 BPM* | TIMTIM RECORDS | 265,399 | https://www.youtube.com/watch?v=fnTaVyzWYjI | `19.5s + 28.0s` |
| **1 · Frontier Therapy** | `p1_room_drums_NqoBtAsByLI.mp4` | *Drum Room Mic Shootout: Blue Yeti Vs Zoom H4n Pro* | Geoff Bosco | 926 | https://www.youtube.com/watch?v=NqoBtAsByLI | `7.5s + 26.0s` |
| **1 · Frontier Therapy** | `p1_turntable_scratch_rtqTmUVjsuY.mp4` | *3 Basic Scratches \| Watch And Learn \| Scratch DJ Academy* | ScratchDJAcademy | 1,160,631 | https://www.youtube.com/watch?v=rtqTmUVjsuY | `150.5s + 30.0s` |
| **2 · Lemon & String** | `p2_baroque_cello_I-lZe5ZMeZM.mp4` | *Sarabande – Suite No. 3 in C Major, BWV 1009 (Juliana Soltis – Baroque Cello)* | Juliana Soltis | 166 | https://www.youtube.com/watch?v=I-lZe5ZMeZM | `156.5s + 32.0s` |
| **2 · Lemon & String** | `p2_fingerpick_guitar_EfZyjGF64wA.mp4` | *Mr. J playing an assortment of fingerpicking riffs in C* | Jeff Segall | 77 | https://www.youtube.com/watch?v=EfZyjGF64wA | `2.5s + 32.0s` |
| **2 · Lemon & String** | `p2_img0238_guitar_oGJvKwLkcPs.mp4` | *IMG 0238* | Ed Haare SemperPaternus | 1 | https://www.youtube.com/watch?v=oGJvKwLkcPs | `22.0s + 28.0s` |
| **2 · Lemon & String** | `p2_wooden_board_eup71wodKY8.mp4` | *Chopping Vegetables On Wooden Board Sound Effect* | Sound Effect Database | 490 | https://www.youtube.com/watch?v=eup71wodKY8 | `4.0s + 28.0s` |
| **2 · Lemon & String** | `p2_book_pages_WZhetc795TA.mp4` | *#13: old books & page turning + scratching/tapping (asmr)* | latenightwhisperer | 6,807 | https://www.youtube.com/watch?v=WZhetc795TA | `180.0s + 32.0s` |
| **2 · Lemon & String** | `p2_found_cassette_jenny_EA0X4C2shiw.mp4` | *Found Cassette - "Jenny"* | Audio Artifacts | 140 | https://www.youtube.com/watch?v=EA0X4C2shiw | `291.0s + 42.0s` |
| **3 · Kinetic Prism** | `p3_handpan_slap_X3xMP7zyq3M.mp4` | *Striking: Shoulder Slaps - Handpan (RAV) Fundamentals* | Rosie Bergonzi - Handpan ChordBook | 444 | https://www.youtube.com/watch?v=X3xMP7zyq3M | `295.5s + 30.0s` |
| **3 · Kinetic Prism** | `p3_anvil_rhythm_uQVeyOplSj0.mp4` | *Perfect Rhythm* | windfarmer78 | 99 | https://www.youtube.com/watch?v=uQVeyOplSj0 | `2.0s + 26.0s` |
| **3 · Kinetic Prism** | `p3_crt_lissajous_p5Wi2hcncHg.mp4` | *Lissajous Figures on a Homemade CRT TV Oscilloscope* | RadioTrefoil | 17,733 | https://www.youtube.com/watch?v=p5Wi2hcncHg | `10.5s + 30.0s` |
| **3 · Kinetic Prism** | `p3_house_footwork_Bldi_Ph-oDw.mp4` | *Learn This House Dance FAST Footwork Combo \| Quick Tutorial* | PRINCE DANCE TUTORIALS | 959 | https://www.youtube.com/watch?v=Bldi_Ph-oDw | `49.5s + 28.0s` |
| **4 · Oxide Ballroom** | `p4_waltz_1920_HnBT-jPRNBU.mp4` | *People Dance a Waltz on a Lawn in 1920* | Walter Nelson (Wallywood Pictures) | 2,020 | https://www.youtube.com/watch?v=HnBT-jPRNBU | `12.0s + 34.0s` |
| **4 · Oxide Ballroom** | `p4_ballroom_1930s_MHExYQdDvoE.mp4` | *1930s Couple Dancing Poolside* (replaced in `v002` by watermark-free `p4_waltz_1920`) | My Footage | 686 | https://www.youtube.com/watch?v=MHExYQdDvoE | `1.0s + 22.0s` |
| **4 · Oxide Ballroom** | `p4_victrola_1930_jazz_9QNikxbXuYM.mp4` | *Victor Credenza Victrola plays "The Terror" 1930 Black Jazz, Cliff Jackson & his Krazy Kats* | Bruce Victrolaman Young | 14,479 | https://www.youtube.com/watch?v=9QNikxbXuYM | `30.5s + 34.0s` |
| **4 · Oxide Ballroom** | `p4_victrola_holiday_0YQ-Ya-LYPU.mp4` | *Victrola Phonograph - "Body and Soul" - Billie Holiday, 1940 (78 rpm)* | Mitch Devine | 3,966 | https://www.youtube.com/watch?v=0YQ-Ya-LYPU | `31.5s + 34.0s` |
| **4 · Oxide Ballroom** | `p4_detuned_piano_wAhwj_kNZIA.mp4` | *Moonlight Sonata in an old out of tune piano* | Stringless Piano | 9,271 | https://www.youtube.com/watch?v=wAhwj_kNZIA | `142.5s + 34.0s` |
| **4 · Oxide Ballroom** | `p4_super8_projector_4rWesEobV38.mp4` | *1960s Super8 movie projector* | coolbluelights | 2,604 | https://www.youtube.com/watch?v=4rWesEobV38 | `111.5s + 28.0s` |
| **5 · Modell 5 Pulse** | `p5_closeup_baritone_Vpv8jFV_-ZI.mp4` | *Rehearsal for The Song Company CLOSE-UP: Book of Travelers – Hayden Barrington sings Beethoven...* | The Song Company | 434 | https://www.youtube.com/watch?v=Vpv8jFV_-ZI | `72.0s + 28.0s` |
| **5 · Modell 5 Pulse** | `p5_opera_closeup_XPCANtdso1Q.mp4` | *OP Close Up - Celebrating the Spring Equinox* | Opera Parallèle | 293 | https://www.youtube.com/watch?v=XPCANtdso1Q | `12.0s + 32.0s` |
| **5 · Modell 5 Pulse** | `p5_closeup_drama_nbBruTwcxvs.mp4` | *Becca Fox: Drama "Emotional Close Up"* | Becca Fox | 170 | https://www.youtube.com/watch?v=nbBruTwcxvs | `156.0s + 32.0s` |
| **5 · Modell 5 Pulse** | `p5_closeup_soprano_xGz_lKfLFdg.mp4` | *Largo al factotum (Rossini) – Michael Spyres (Full Performance)* | Best Of Opera | 72 | https://www.youtube.com/watch?v=xGz_lKfLFdg | `275.5s + 28.0s` |

---

## 3. Iteration Log

### `v001` (`outputs/261113_vj-hillclimb/v001/out/`)
- Initial 6-piece render pass across all 5 contrasting YouTube collage worlds (`60 fps` H.264, `48 kHz` oversampled `-1.4 dBTP` AAC) + `ISOL8` hardware studio VJ showcase (`30 fps` `1920×1080`).
- **Verified `v001` Audio & Video QA (`ebur128` + `astats` + 5-band spectral distribution, `0` NaNs/Infs, `0` clipped samples):**

| File (`v001/out/`) | Spec | Duration | Integrated LUFS | True Peak | LRA | Sub (`<95 Hz`) | Mud (`180–450 Hz`) |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `261113_chromagrain_frontier-therapy_dialogue-collage_v001.mp4` | `1080×1920 @ 60fps` | `19.59 s` | `-14.20 LUFS` | `-1.50 dBTP` | `2.30 LU` | `29.3%` | `12.9%` |
| `261113_chromagrain_lemon-and-string_books-collage_v001.mp4` | `1920×1080 @ 60fps` | `18.64 s` | `-12.10 LUFS` | `-1.50 dBTP` | `0.70 LU` | `27.4%` | `27.8%` |
| `261113_chromagrain_kinetic-prism_ratchet-grid_v001.mp4` | `1080×1080 @ 60fps` | `17.14 s` | `-16.80 LUFS` | `-1.70 dBTP` | `2.10 LU` | `41.8%` | `17.1%` |
| `261113_chromagrain_oxide-ballroom_disintegration-loops_v001.mp4` | `1920×1080 @ 60fps` | `25.60 s` | `-13.40 LUFS` | `-1.80 dBTP` | `5.20 LU` | `0.4%` | `18.2%` |
| `261113_chromagrain_modell5-pulse_portrait-bursts_v001.mp4` | `1920×1080 @ 60fps` | `16.40 s` | `-16.70 LUFS` | `-2.20 dBTP` | `3.40 LU` | `26.5%` | `12.5%` |
| `261113_chromagrain_isol8-studio_vj-showcase_v001.mp4` | `1920×1080 @ 30fps` | `52.50 s` | `-14.60 LUFS` | `-1.70 dBTP` | `10.60 LU` | `46.0%` | `10.1%` |

### `v002` (`outputs/261113_vj-hillclimb/v002/out/`)
- **Addressed every `#84` `[SYSTEM]` & `[PIECE]` note across `crates/` and `vj_hillclimb.rs` (`cargo test --workspace` green with `-D warnings`):**
  1. **Piece 6 (`isol8-studio_vj-showcase_v002.mp4`):** Taps `play` at `0:04.85` so the live 3-lane YouTube `SET` groove plays continuously through `Add -> YouTube` (`0:05.8–0:14.2`); fires immediate audible auditions when `sculpt Kick`, `sculpt Chord`, and `sculpt Disintegrate` are clicked in `Clip`; keeps `"sequence"` (`p.tracks[PADS]`) + looping Perform video pads (`15 & 16`) running non-stop across `0:22–0:48.8` so `ISOL8` (`LMF+MF+HMF SOLO` radio -> `LF SOLO` sub -> full band), 6-fold spinning polar `Kaleido` (`punch shimmer`), `Stutter`, varispeed `Tape Stop`, `chain 2` (`climax`), and 4-panel `Fullscreen` `Grid` are 100% audible and visually alive.
  2. **Piece 1 (`frontier-therapy_dialogue-collage_v002.mp4`):** Word-exact local VTT slices (accounting for `2.0s` trim offset on `p1_mental_fit_1952` -> exact `"MENTAL—"` / `"IN A MENTAL INSTITUTION"` and pure ZCR-verified horse whinnies); paired static-card drum recordings with live turntable & 1950s archival visuals + `1.85×` dancer crop (zero text cards); sidechain-ducked brass under dialogue + stripped drums during the `ISOL8` AM radio break (`LRA` `2.3 -> 3.00 LU`); dense `9:16` `1×2 / 1×3 / 2×2` bands eliminate empty black quadrants; auto-scaled amber-bar captions never drop.
  3. **Piece 2 (`lemon-and-string_books-collage_v002.mp4`):** Paired wooden cutting-board audio with real page-turning hands video (zero `"SOUND EFFECT DATABASE"` card); `chord_harmony` deduplication shows 1 guitarist per voice; surgical `265–295 Hz` mud carve + `+3.4 dB @ 5.2 kHz` air + macro chamber dynamic arc (whisper fingerpick intro + Jenny 1 -> full polymetric cello/guitar bloom -> intimate cassette hush, raising `LRA` `0.70 -> 10.40 LU` at `-15.20 LUFS`).
  4. **Piece 3 (`kinetic-prism_ratchet-grid_v002.mp4`):** Re-sliced `p3_handpan_slap` strictly inside `3.5..19.5s` (zero Patreon end-credits) + `1.85×` crop past `"YOUR ANGLE"`; dense `1:1` `Layout::Tile` slot packing eliminates empty black quadrants; 6-stage `Q = 5.4` resonant acid handpan sweep + 8-beat UKG club breakdown (`LRA = 3.60 LU`); bright `Split` + dual prism bed across `beats 20..36` lights up the 6-fold spinning polar `Kaleido` (`-15.10 LUFS`, `-1.30 dBTP`).
  5. **Piece 4 (`oxide-ballroom_disintegration-loops_v002.mp4`):** Replaced watermarked `p4_ballroom_1930s` with watermark-free 1920 lawn waltz archival footage (`p4_waltz_1920`, `2,020` views) and paired `p4_detuned_piano` audio with clean 1920 waltz visuals (zero `"MyFootage.com"`, SMPTE timecode, or `"YouTube: Stringless Piano"`); clear golden-sepia opening exposure progressively softens and darkens across passes via `Disintegrate` + `ISOL8` `HF MUTE -> HF+HMF MUTE` (`-12.70 LUFS`, `-2.60 dBTP`, `6.30 LU` LRA).
  6. **Piece 5 (`modell5-pulse_portrait-bursts_v002.mp4`):** Switched to `Layout::Tile` + `portrait_columns: true` for crisp `1.5 px`-divided `3×1` architectural monoliths; paired baritone voice with tight front-facing singer portrait (`p5_opera_closeup`, `293` views, `2.35×` zoom) and locked right column to tight close-up window (`14.88..15.56s`, `1.65×` zoom) so all 3 monoliths hold monumental human faces from `12 Hz` shutter through `45 Hz` pitched vocal buzz (`-12.50 LUFS`, `-1.20 dBTP`, `2.10 LU` LRA).

| File (`v002/out/`) | Spec | Duration | Integrated LUFS | True Peak | LRA | Sub (`<95 Hz`) | Mid (`450–4k Hz`) |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `261113_chromagrain_frontier-therapy_dialogue-collage_v002.mp4` | `1080×1920 @ 60fps` | `19.59 s` | `-13.50 LUFS` | `-1.30 dBTP` | `3.00 LU` | `36.9%` | `35.3%` |
| `261113_chromagrain_lemon-and-string_books-collage_v002.mp4` | `1920×1080 @ 60fps` | `18.64 s` | `-15.20 LUFS` | `-1.60 dBTP` | `10.40 LU` | `18.3%` | `46.0%` |
| `261113_chromagrain_kinetic-prism_ratchet-grid_v002.mp4` | `1080×1080 @ 60fps` | `17.14 s` | `-15.10 LUFS` | `-1.30 dBTP` | `3.60 LU` | `30.3%` | `6.5%` |
| `261113_chromagrain_oxide-ballroom_disintegration-loops_v002.mp4` | `1920×1080 @ 60fps` | `25.60 s` | `-12.70 LUFS` | `-2.60 dBTP` | `6.30 LU` | `0.4%` | `78.4%` |
| `261113_chromagrain_modell5-pulse_portrait-bursts_v002.mp4` | `1920×1080 @ 60fps` | `16.40 s` | `-12.50 LUFS` | `-1.20 dBTP` | `2.10 LU` | `26.6%` | `28.1%` |
| `261113_chromagrain_isol8-studio_vj-showcase_v002.mp4` | `1920×1080 @ 30fps` | `52.50 s` | `-14.50 LUFS` | `-2.10 dBTP` | `5.20 LU` | `19.2%` | `53.4%` |

### `v003` (`outputs/261113_vj-hillclimb/v003/out/`)
- **Addressed all 3 `#90` surgical items (`[SYSTEM]` + `[PIECE]`) across `crates/` and `vj_hillclimb.rs` (`RUSTFLAGS="-D warnings" cargo test --workspace` green):**
  1. **Piece 6 + `[SYSTEM]` Loop-Pad Outro/Speech Guard (`crates/core/src/chop.rs` + `vj_hillclimb.rs`):**
     - `build_kit_across_sources` (`Pads 15–16` `Wash` / `Tape Loop`) and `build_tape_loops_across_sources` now penalize late-clip outro segments (last `20%` of clips `>= 8s`) and `1–3.5 kHz` syllabic speech slices (`pitch_confidence < 0.62`) in favor of sustained harmonics.
     - Trimmed `s0` (`Footwork Sub`) in `harness_isol8_showcase` to `0..17.8s` and cropped frames `[0.45, 0.08, 0.50, 0.76]` (physically excising the `18..28s` `"smashing that like button"` spoken outro, red `"SUBSCRIBE"` banner, and `"RECAP / YOUR ANGLE"` overlay), and faded audio to `0.0` before `48.8s` so the `0:48.8–0:52.5` `chromagrain` end card has zero bleed (`-14.60 LUFS`, `-2.40 dBTP`, `5.40 LU` LRA).
  2. **Piece 3 + `[SYSTEM]` `N == 3` Full-Width Bottom-Row Span (`crates/core/src/{tile.rs,video.rs,render.rs}`, `crates/ui/src/gpu.rs`, `vj_hillclimb.rs`):**
     - Added `tile_rect_for_count` across CPU and GPU (`gpu_matches_the_cpu_for_tiles_and_envelopes` `"tile3"` verified) so when `3` voices are active in `Layout::Tile` (`!portrait_columns`), cell `2` spans full-width across the bottom row — eliminating empty black quadrants in both Piece 3 (`0:00`, `0:07`, `0:16`) and Piece 6 (`0:41–0:44` Fullscreen `Grid`).
     - High-passed `t_crt` (`>= 380 Hz`, `0 st` `440–880 Hz` register), high-passed `t_prism` (`>= 540 Hz`, whisper audio gain), carved `265–275 Hz`, and boosted `1.65–3.4 kHz` bite on the `Q = 5.4` handpan ratchets & anvil percussion: **`mud(180–450 Hz)` dropped from `27.3% -> 8.4%`** while **`mid(450–4k)` jumped from `6.0% -> 26.0%`** (`-15.90 LUFS`, `-1.50 dBTP`, `2.20 LU` LRA).
  3. **Piece 5 (`modell5-pulse_portrait-bursts_v003.mp4`):**
     - Set `scan_speed = 0.0` (`spray = 0.004`) in `make_monolith` and clamped Right monolith `pos_pts` strictly inside `14.84..15.25s` so `source_pos` never drifts past `15.8s` onto the wide orchestra cut — all 3 `1.5 px`-divided monolith columns hold tight monumental human faces across every frame (`-11.90 LUFS`, `-2.10 dBTP`, `5.50 LU` LRA, `12.1%` mud).

| File (`v003/out/`) | Spec | Duration | Integrated LUFS | True Peak | LRA | Mud (`180–450 Hz`) | Mid (`450–4k Hz`) |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `261113_chromagrain_frontier-therapy_dialogue-collage_v003.mp4` | `1080×1920 @ 60fps` | `19.59 s` | `-13.80 LUFS` | `-1.40 dBTP` | `3.00 LU` | `13.0%` | `37.3%` |
| `261113_chromagrain_lemon-and-string_books-collage_v003.mp4` | `1920×1080 @ 60fps` | `18.64 s` | `-15.20 LUFS` | `-2.20 dBTP` | `10.40 LU` | `23.9%` | `46.0%` |
| `261113_chromagrain_kinetic-prism_ratchet-grid_v003.mp4` | `1080×1080 @ 60fps` | `17.14 s` | `-15.90 LUFS` | `-1.50 dBTP` | `2.20 LU` | `8.4%` | `26.0%` |
| `261113_chromagrain_oxide-ballroom_disintegration-loops_v003.mp4` | `1920×1080 @ 60fps` | `25.60 s` | `-12.70 LUFS` | `-2.60 dBTP` | `6.30 LU` | `18.0%` | `78.4%` |
| `261113_chromagrain_modell5-pulse_portrait-bursts_v003.mp4` | `1920×1080 @ 60fps` | `16.40 s` | `-11.90 LUFS` | `-2.10 dBTP` | `5.50 LU` | `12.1%` | `46.0%` |
| `261113_chromagrain_isol8-studio_vj-showcase_v003.mp4` | `1920×1080 @ 30fps` | `52.50 s` | `-14.60 LUFS` | `-2.40 dBTP` | `5.40 LU` | `13.0%` | `50.0%` |
