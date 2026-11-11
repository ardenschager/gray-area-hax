// examples/holdout_ambient.rhai — Held-out granular ambient benchmark (~32 s)
// Guards against overfitting chromagrain to 2-step garage: slow beatless granular
// chamber & nature piece in D dorian on sunlit forest, misty waterfall, and surf.
//
// Run headless:
//   target/release/chromagrain --script examples/holdout_ambient.rhai

let s = session(60.0); // 1 beat = 1.0 second -> 32 beats = 32.0 s
s.canvas(640, 360, 24.0);

let forest = s.load("assets/demo/holdout_forest.mp4");
let falls  = s.load("assets/demo/holdout_waterfall.mp4");
let ocean  = s.load("assets/demo/holdout_ocean.mp4");

s.set_base_hz(forest, 146.832); // D3
s.set_base_hz(falls,  146.832);
s.set_base_hz(ocean,  293.665); // D4

let forest_wash = s.paulstretch(forest, 4.0);

// Track 0: Slow sunlit pine snippet bed, half-speed drift
let t_bed = s.track("forest_bed");
s.track_opacity_link(t_bed, 0.45);
s.track_level_point(t_bed, 0.0, 0.0);
s.track_level_point(t_bed, 5.0, 0.85);
s.track_level_point(t_bed, 27.0, 0.85);
s.track_level_point(t_bed, 32.0, 0.0);

let bed = s.snippet(t_bed, forest_wash, 0.0, 32.0);
s.set(bed, "gain", 0.48);
s.set(bed, "gain_to_opacity", 0.25);
s.set(bed, "speed", 0.75);
s.set(bed, "envelope", 0.25);

// Track 1: Evolving misty waterfall granular cloud in D dorian with fifth harmonies
let t_cloud = s.track("waterfall_cloud");
let cl = s.clip(t_cloud, falls, 2.0, 28.0);
s.key(cl, "D", "dorian");
s.set(cl, "quantize_amount", 0.85);
s.set(cl, "detune_cents", 7.0);
s.set(cl, "density", 11.0);
s.set(cl, "duration", 0.55);
s.set(cl, "spray", 1.6);
s.set(cl, "scan_speed", 0.35);
s.set(cl, "voices", 2.0);
s.set(cl, "voice_interval", 7.0);
s.set(cl, "voice_detune", 0.08);
s.set(cl, "pan_spread", 0.8);
s.set(cl, "gain", 0.46);
s.set(cl, "additive", 0.5);
s.automate(cl, "density", 0.0, 5.0);
s.automate(cl, "density", 14.0, 18.0);
s.automate(cl, "density", 28.0, 4.0);
s.automate(cl, "gain", 0.0, 0.15);
s.automate(cl, "gain", 12.0, 0.52);
s.automate(cl, "gain", 28.0, 0.08);

let cl_rv = s.track_effect(t_cloud, "reverb");
s.track_fx(t_cloud, cl_rv, "size", 0.84);
s.track_fx(t_cloud, cl_rv, "damp", 0.45);
s.track_fx(t_cloud, cl_rv, "mix", 0.45);

// Track 2: High crystalline ocean surf shimmer grains (octave up, occasional reverse)
let t_shimmer = s.track("ocean_shimmer");
let sh = s.clip(t_shimmer, ocean, 6.0, 22.0);
s.key(sh, "D", "dorian");
s.set(sh, "density", 6.5);
s.set(sh, "duration", 0.38);
s.set(sh, "pitch", 12.0);
s.set(sh, "pitch_jitter", 7.0);
s.set(sh, "spray", 1.2);
s.set(sh, "reverse_prob", 0.35);
s.set(sh, "pan_spread", 0.9);
s.set(sh, "gain", 0.32);
s.set(sh, "additive", 0.65);
s.color_keep(sh, 195.0, 130.0); // keep turquoise/cyan surf hues

let sh_dl = s.track_effect(t_shimmer, "delay");
s.track_fx(t_shimmer, sh_dl, "time", 0.65);
s.track_fx(t_shimmer, sh_dl, "feedback", 0.52);
s.track_fx(t_shimmer, sh_dl, "mix", 0.42);
s.track_fx(t_shimmer, sh_dl, "shift_x", 0.03);

let m_rv = s.master_effect("reverb");
s.master_fx(m_rv, "size", 0.72);
s.master_fx(m_rv, "damp", 0.5);
s.master_fx(m_rv, "mix", 0.22);

s.render(0.0, 32.0, "outputs/261111_burial-demo/holdout/v001/261111_chromagrain_holdout-ambient_v001.mp4");
