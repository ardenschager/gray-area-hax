# Chromagrain Engine Hill-Climb Log (`SYSTEM_LOG.md`)

Tracks general engine (`crates/core`) improvements motivated by multi-model critiques (`@critic`, `@critic-argon`, `@critic-opus`) and self-critique on `examples/burial.rhai`, guarded against overfitting via `examples/holdout_ambient.rhai`.

---

## Baseline (`v001`)
- Stock `chromagrain-core` prior to system changes (130 tests passing).
- Candidate SYSTEM gaps already observed while authoring `v001` (to validate against `v001` render & critic consensus):
  1. **Step-sequencer & sync-cloud groove (`seq.rs`, `grain.rs`)**: `StepPattern` has rigid 16th quantization (`slot * step_secs`) with zero swing/shuffle or per-step micro-timing offset (`offset`), making UKG 2-step rhythms stiff out of the box.
  2. **Segmented alpha patch framing (`video.rs`)**: When a source is segmented (`s.segment(src, "person")`) and played with sub-frame visual patches (`patch_size < 1.0`, e.g. grain clouds, textured snippets, or pattern chops), `grain_draws_opts` picks `patch` coordinates from `src_frac` rather than biasing toward the frame's foreground alpha centroid — causing many grains of cut-out people/subjects to sample empty `alpha = 0` background and vanish.
  3. **Track sidechain / ducking (`timeline.rs`, `render.rs`, `realtime.rs`, `script.rs`)**: No general way for a rhythmic track (e.g. kick) to duck a pad/wash/bed track in audio (and optionally video opacity via AV correspondence), forcing static muddiness between sub/kick and lush washes.
  4. **Tape / analog wow-flutter & warmth (`fx.rs` / track & master character)**: No pitch wobble / tape flutter or warm high-shelf saturation, and Freeverb's 4 parallel combs + 2 allpasses can sound metallic on impulsive rimshots/vocals without pre-delay / diffusion / high-cut damping.
  5. **Script & Source ergonomics (`timeline.rs`, `script.rs`, `media.rs`)**: `Source::slice` exists in Rust (`timeline.rs:42`) but is unexposed in Rhai; pairing separate audio + video sources (`Source::pair`) is missing; `media::segment_video` re-runs Apple Vision from scratch on every run without disk caching in `.chromagrain-cache/segment/`.
