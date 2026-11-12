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

// Cents-proportional critical-band paulstretch keeps upper air while melting whistles
let forest_wash = s.paulstretch(forest, 4.0);

// Track 0: Slow sunlit pine snippet bed, 0.75x drift, breathing visually with waterfall swells
let t_bed = s.track("forest_bed");
s.track_opacity_link(t_bed, 0.45);
s.track_eq(t_bed, 320.0, 1.1, -2.8);
s.track_level_point(t_bed, 0.0, 0.0);
s.track_level_point(t_bed, 5.0, 0.72);
s.track_level_point(t_bed, 26.5, 0.72);
s.track_level_point(t_bed, 32.0, 0.0);

let bed = s.snippet(t_bed, forest_wash, 0.0, 32.0);
s.set(bed, "gain", 0.32);
s.set(bed, "gain_to_opacity", 0.20);
s.set(bed, "speed", 0.75);
s.set(bed, "envelope", 0.25);

// Track 1: Evolving misty waterfall granular cloud in pure A=440 D dorian
let t_cloud = s.track("waterfall_cloud");
s.track_blend(t_cloud, 0.55);
s.track_rms_opacity(t_cloud, 0.78);
s.track_eq(t_cloud, 310.0, 1.2, -2.5);
// Forest sunlight pulses gently with the waterfall & cello swells
s.track_visual_pulse(t_bed, t_cloud, 0.44, 0.028, 0.24, 6000.0);

let cl = s.clip(t_cloud, falls, 2.0, 28.0);
s.key(cl, "D", "dorian");
s.set(cl, "quantize_amount", 1.0); // 100% scale snap -> zero quarter-tone drift!
s.set(cl, "detune_cents", 2.5);
s.set(cl, "density", 10.0);
s.set(cl, "duration", 0.62);
s.set(cl, "spray", 1.8);
s.set(cl, "scan_speed", 0.35);
s.set(cl, "voices", 2.0);
s.set(cl, "voice_interval", -12.0);
s.set(cl, "voice_detune", 0.04);
s.set(cl, "pan_spread", 0.8);
s.set(cl, "gain", 0.35);
s.set(cl, "additive", 0.55);
s.set(cl, "feather", 0.55);
s.set(cl, "flicker", 0.24);
s.audio_filter(cl, "highpass", 115.0, 0.70);
s.automate(cl, "density", 0.0, 4.5);
s.automate(cl, "density", 14.0, 15.0);
s.automate(cl, "density", 28.0, 3.5);
s.automate(cl, "gain", 0.0, 0.10);
s.automate(cl, "gain", 13.0, 0.40);
s.automate(cl, "gain", 28.0, 0.05);

let cl_rv = s.track_effect(t_cloud, "reverb");
s.track_fx(t_cloud, cl_rv, "size", 0.82);
s.track_fx(t_cloud, cl_rv, "damp", 0.34);
s.track_fx(t_cloud, cl_rv, "mix", 0.38);

// Track 2: Ocean surf shimmer grains with deep cosine feathering (zero horizon seam!)
let t_shimmer = s.track("ocean_shimmer");
s.track_blend(t_shimmer, 0.68);
s.track_rms_opacity(t_shimmer, 0.72);
let sh = s.clip(t_shimmer, ocean, 6.0, 22.0);
s.key(sh, "D", "dorian");
s.set(sh, "quantize_amount", 1.0);
s.set(sh, "density", 5.5);
s.set(sh, "duration", 0.48);
s.set(sh, "pitch", 0.0);
s.set(sh, "pitch_jitter", 4.0);
s.set(sh, "spray", 1.4);
s.set(sh, "reverse_prob", 0.35);
s.set(sh, "pan_spread", 0.88);
s.set(sh, "gain", 0.28);
s.set(sh, "additive", 0.68);
s.set(sh, "feather", 0.72); // deep soft feathering melts surf horizon into forest mist
s.set(sh, "flicker", 0.28);
s.audio_filter(sh, "highpass", 170.0, 0.70);
s.color_keep(sh, 195.0, 130.0);

let sh_dl = s.track_effect(t_shimmer, "delay");
s.track_fx(t_shimmer, sh_dl, "time", 0.65);
s.track_fx(t_shimmer, sh_dl, "feedback", 0.44);
s.track_fx(t_shimmer, sh_dl, "mix", 0.34);
s.track_fx(t_shimmer, sh_dl, "shift_x", 0.03);

s.master_tape(0.08, 0.12, 0.03, 0.04);

let m_rv = s.master_effect("reverb");
s.master_fx(m_rv, "size", 0.68);
s.master_fx(m_rv, "damp", 0.36);
s.master_fx(m_rv, "mix", 0.16);

s.render(0.0, 32.0, "outputs/261111_burial-demo/holdout/v004/261111_chromagrain_holdout-ambient_v004.mp4");
