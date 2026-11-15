# 261114_videograin — `v007` Notes

Feedback addressed: `#fb-701052396` ("this sounds pretty bad" on `hardware-console_ukg-showcase_v006`),
keeping what earned `#fb-701021625` ("this is a banger", `disco-pump`) and `#fb-701041057`
("this sounds pretty good", `bid-stutter`). `v006/` is untouched.

## Root cause (confirmed with per-lane solo renders + an ear audit of `v006`)
1. **Lanes were bound to raw load order, not sound.** In the v006 console pattern lane 4 was a spoken
   Flowbee "da" (pitched up, ratcheted, every off-16th = the "duh-uh" spam), the kick was a Turbo
   Encabulator syllable, and a "hat" lane played 2-second Rhodes chunks. The product's own 1-click
   `Disco` auto-kit put an Encabulator "duh" on the snare and tap shoes on the bass, because the kit
   builder scored slices with no idea what kind of clip they came from.
2. **Engine bug: choke group 5 clamped into group 4.** Every hat hit (private group 5) choked the
   group-4 chord stabs, which left the whole mix silent around beat 3 of every 2-step bar (and buried
   the Disco chords).
3. **Engine bug: `seq_current` ignored `∞ hold`.** A preset applied while holding a scene went to
   whichever chain scene the playhead happened to fall in, not the one playing.

## Product fixes (`crates/core`, `crates/ui`)
- `chop::profile_source` / `SourceProfile`: a whole-clip acoustic profile (held tempered pitch,
  tuning coherence, low-band share, ZCR, punch, speech glide) giving `harmonic / bassline / drums /
  speech` suitability. The UKG vocal kit, the chord-wash kit and the new lane kit all weight their
  slice candidates by it.
- `chop::build_lane_kit_across_sources_opt` + `LaneRole`: 8 lanes
  `Kick · Snare · Hat · VoxPerc · Chord · Bass · Hook A · Hook B`, each bound to the best source **and**
  its exact curated slice, whatever order the clips were loaded in.
- `seq::assign_curated_kit_to_pattern` (used by every groove-preset click),
  `seq::rebind_pattern_to_lane_kit` (used by `Vocal Kit → pads` to re-sync every Sequence scene with
  steps left as programmed), and `StepPattern::apply_preset_keyed`.
- VoxPerc pocket rule: HPF `>= 340 Hz`, step gains `0.42..0.52`, crisp ticks only (`<= 160 ms`, never
  voiced vowels, snare/kick bodies or chunks of tonal instruments).
- Hat lane grooves for every rhythmic preset. Hook B stays armed but silent and gets the spotlight in
  the 4-section `Break`. A real bass instrument is no longer dropped an extra octave under a 155 Hz LPF.
  Captioned user hooks bypass the outro guard. `∞ hold` loops the whole held pattern.
- Tests: synthetic Rhodes/bass/tap/beatbox/speech crate across 4 load orders × Disco/2-Step, a
  real-crate test across 3 load orders, a rebind/break/climax test, and a choke-group regression test.

## Deliverable (`out/`)
- `261114_videograin_hardware-console_disco-ukg-showcase_v007.mp4`: banger crate (Rhodes, fingerstyle
  bass, beatbox, tap legs "subject" cutout, Encabulator "subject" cutout, Soul Stutter pulled live and
  cut out "person"), zero title/end cards. Disco on frame 0 → YouTube pull → Cutout Person +
  Vocal Kit → Sequence → beatbox snare roll into 2-Step (132 BPM, the preset's own tempo) → Build Set
  → hold drop → Perform Grid × + one-bar telephone ISOL8 break on "It's not cheap" → slam back on "the
  government will buy it" → Rhodes/bass chops paulstretched ×8 swell under the groove → Chord Wash →
  Fullscreen.
- Telemetry: `1920x1080 @ 30fps`, `41.00 s`, **-15.6 LUFS**, **-3.4 dBTP** (post-AAC), sample peak
  `-3.45 dBFS`, `0` clipped samples, `0` black frames (`blackdetect d=0.03 pix_th=0.08`).
- Ear audit (Gemini audio model as a proxy ear, three 14 s 540p chunks; I can't play audio directly):
  the 2-step section has **0** repeated spoken syllables (v006: about 3.8 "duh" hits/s over the same
  window). Disco groove, soul hook, telephone break and slam-back are all heard as intended. The
  outro wash is heard as calm in some runs and dark/detuned in others.
- Process: take 1 → take 6. Each take re-listened to and re-tuned (choke bug, hold-scene bug,
  2-step tempo 132, snare roll, wash on stretched chops, master `loudnorm I=-13.4 → alimiter 0.60`).

## Known limits / next take
- Chord Wash still fills Perform pads only. Applying the `Wash` sequencer preset to busy sources (a
  Rhodes improv run, speech) turned into chatter, so the showcase closing uses paulstretched curated
  chops instead. A stretch-aware wash preset is the right follow-up.
- An ear model still calls the 124→132 BPM switch abrupt, even with the snare roll and pickup swell.
