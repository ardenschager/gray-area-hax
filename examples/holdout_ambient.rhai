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

// Track 0: Slow sunlit pine snippet bed, 0.75x drift (continuous blended video)
let t_bed = s.track("forest_bed");
s.track_opacity_link(t_bed, 0.45);
s.track_level_point(t_bed, 0.0, 0.0);
s.track_level_point(t_bed, 5.5, 0.78);
s.track_level_point(t_bed, 26.5, 0.78);
s.track_level_point(t_bed, 32.0, 0.0);

let bed = s.snippet(t_bed, forest_wash, 0.0, 32.0);
s.set(bed, "gain", 0.36);
s.set(bed, "gain_to_opacity", 0.20);
s.set(bed, "speed", 0.75);
s.set(bed, "envelope", 0.25);

// Track 1: Evolving misty waterfall granular cloud in D dorian with warm lower-octave body
let t_cloud = s.track("waterfall_cloud");
s.track_blend(t_cloud, 0.45);
let cl = s.clip(t_cloud, falls, 2.0, 28.0);
s.key(cl, "D", "dorian");
s.set(cl, "quantize_amount", 0.85);
s.set(cl, "detune_cents", 6.0);
s.set(cl, "density", 10.0);
s.set(cl, "duration", 0.62);
s.set(cl, "spray", 1.8);
s.set(cl, "scan_speed", 0.35);
s.set(cl, "voices", 2.0);
s.set(cl, "voice_interval", -12.0);
s.set(cl, "voice_detune", 0.07);
s.set(cl, "pan_spread", 0.8);
s.set(cl, "gain", 0.34);
s.set(cl, "additive", 0.45);
s.set(cl, "feather", 0.40);
s.set(cl, "flicker", 0.12);
s.audio_filter(cl, "lowpass", 3200.0, 0.72);
s.automate(cl, "density", 0.0, 4.5);
s.automate(cl, "density", 14.0, 15.0);
s.automate(cl, "density", 28.0, 3.5);
s.automate(cl, "gain", 0.0, 0.10);
s.automate(cl, "gain", 13.0, 0.38);
s.automate(cl, "gain", 28.0, 0.05);

let cl_rv = s.track_effect(t_cloud, "reverb");
s.track_fx(t_cloud, cl_rv, "size", 0.82);
s.track_fx(t_cloud, cl_rv, "damp", 0.58);
s.track_fx(t_cloud, cl_rv, "mix", 0.42);

// Track 2: Gentle ocean surf shimmer grains in natural register (fifth / unison drift, occasional reverse)
let t_shimmer = s.track("ocean_shimmer");
s.track_blend(t_shimmer, 0.60);
let sh = s.clip(t_shimmer, ocean, 6.0, 22.0);
s.key(sh, "D", "dorian");
s.set(sh, "density", 5.5);
s.set(sh, "duration", 0.48);
s.set(sh, "pitch", 0.0);
s.set(sh, "pitch_jitter", 5.0);
s.set(sh, "spray", 1.4);
s.set(sh, "reverse_prob", 0.35);
s.set(sh, "pan_spread", 0.88);
s.set(sh, "gain", 0.24);
s.set(sh, "additive", 0.60);
s.set(sh, "feather", 0.45);
s.set(sh, "flicker", 0.14);
s.audio_filter(sh, "lowpass", 2800.0, 0.72);
s.color_keep(sh, 195.0, 130.0); // keep turquoise/cyan surf hues

let sh_dl = s.track_effect(t_shimmer, "delay");
s.track_fx(t_shimmer, sh_dl, "time", 0.65);
s.track_fx(t_shimmer, sh_dl, "feedback", 0.48);
s.track_fx(t_shimmer, sh_dl, "mix", 0.38);
s.track_fx(t_shimmer, sh_dl, "shift_x", 0.03);

s.master_tape(0.12, 0.16, 0.12, 0.0);

let m_rv = s.master_effect("reverb");
s.master_fx(m_rv, "size", 0.70);
s.master_fx(m_rv, "damp", 0.58);
s.master_fx(m_rv, "mix", 0.20);

s.render(0.0, 32.0, "outputs/261111_burial-demo/holdout/v002/261111_chromagrain_holdout-ambient_v002.mp4");
