// examples/burial.rhai — Nocturnal Subterranean 2-Step AV Sketch ("Night Platform")
// Run headless:
//   target/release/chromagrain --script examples/burial.rhai
//
// Key: F minor | Tempo: 133 BPM (40 bars = 160 beats = 72.2 s)
// Zero lyric sentences, zero TTS speech — built entirely from sampled archival
// footage, wordless breath/hum/vowel micro-chops (<0.65 s), shuffled 2-step
// percussion, varispeed sub smears, paulstretch washes, sidechain ducking, and tape.
//
// Structure:
//   0..32   (0:00-0:14.4) Intro: rain on night glass, vinyl dust, paulstretch Fm9 wash,
//                         distant pitched-down breath fragments; beat 30.5-32 hush
//   32..80  (0:14.4-0:36.1) Drop I: 60%-swung 2-step UKG pocket, sidechained F1 Reese
//                         highway smears, sparse downward-pitched vowel/hum micro-chops
//   80..112 (0:36.1-0:50.5) Breakdown: drums & sub drop away; bare wordless hums &
//                         archival subway air echo across wet sodium glass; 110.5-112 hush
//   112..144(0:50.5-1:05.0) Drop II: evolved 2-step pattern with 5-step polymeter
//                         vocal-slice interlock, mirrored ghost reflections & dub throws
//   144..160(1:05.0-1:12.2) Outro: drums strip to rain, vinyl dust, and a low fading hum

let s = session(133.0);
s.canvas(640, 360, 24.0);

// ---- Load curated AV sample sources (assets/demo/) ----
let rain_src    = s.load("assets/demo/rain_window.mp4");
let chords_src  = s.load("assets/demo/night_chords.mp4");
let sub_src     = s.load("assets/demo/sub_highway.mp4");
let kick_src    = s.load("assets/demo/kick_tunnel.mp4");
let rim_src     = s.load("assets/demo/rim_train.mp4");
let hat_src     = s.load("assets/demo/hat_glitter.mp4");
let vbench_src  = s.load("assets/demo/vocal_bench.mp4");
let vstreet_src = s.load("assets/demo/vocal_street.mp4");

s.set_base_hz(sub_src,    87.307);  // F2 (audio carries F1 43.65 Hz + warm F2 Reese)
s.set_base_hz(chords_src, 174.614); // F3

// Derive 6x paulstretched AV chord wash (smoothed upper partials, zero sine whistle)
let chord_wash = s.paulstretch(chords_src, 6.0);

// Apple Vision foreground people cutouts (cached in .chromagrain-cache/segment/)
let vbench_cut = if s.segment_available() {
    s.segment(vbench_src, "person")
} else {
    vbench_src
};
let vstreet_cut = if s.segment_available() {
    s.segment(vstreet_src, "person")
} else {
    vstreet_src
};

// Detect natural breath & vowel transient slices for sampler chop sequencing
let bench_slices  = s.slices(vbench_cut, 12);
let street_slices = s.slices(vstreet_cut, 12);

// ============================================================================
// TRACK 0: Rain & Vinyl Crackle Bed (bottom z-layer: continuous blended video)
// ============================================================================
let t_rain = s.track("rain_vinyl_bed");
s.track_opacity_link(t_rain, 0.22);
s.track_level_point(t_rain, 0.0, 0.0);
s.track_level_point(t_rain, 5.0, 0.88);
s.track_level_point(t_rain, 148.0, 0.88);
s.track_level_point(t_rain, 160.0, 0.0);

let b = 0.0;
while b < 160.0 {
    let seg = s.snippet(t_rain, rain_src, b, 16.0);
    s.set(seg, "gain", 0.24);
    s.set(seg, "gain_to_opacity", 0.12);
    s.set(seg, "speed", 0.65);
    s.set(seg, "envelope", 0.08);
    b += 16.0;
}
let rain_tint = s.track_effect(t_rain, "tint");
s.track_fx(t_rain, rain_tint, "hue", 28.0);     // warm sodium streetlight amber
s.track_fx(t_rain, rain_tint, "amount", 0.28);
s.track_fx(t_rain, rain_tint, "audio", 0.0);

// ============================================================================
// TRACK 1: Paulstretched Minor Chords (warm nocturnal pad, sidechained in drops)
// ============================================================================
let t_pad = s.track("paulstretch_pad");
s.track_blend(t_pad, 0.55);
s.track_opacity_link(t_pad, 0.50);
s.track_tape(t_pad, 0.28, 0.22, 0.25, 0.0);

s.track_level_point(t_pad, 0.0, 0.0);
s.track_level_point(t_pad, 10.0, 0.72);
s.track_level_point(t_pad, 29.5, 0.72);
s.track_level_point(t_pad, 31.2, 0.22); // pre-drop breath of silence!
s.track_level_point(t_pad, 32.5, 0.62);
s.track_level_point(t_pad, 80.0, 0.84);
s.track_level_point(t_pad, 109.5, 0.80);
s.track_level_point(t_pad, 111.3, 0.18); // second pre-drop hush before Drop II!
s.track_level_point(t_pad, 112.5, 0.66);
s.track_level_point(t_pad, 144.0, 0.58);
s.track_level_point(t_pad, 160.0, 0.0);

let pad_intro = s.clip(t_pad, chord_wash, 1.5, 78.5);
s.key(pad_intro, "F", "minor");
s.set(pad_intro, "quantize_amount", 0.75);
s.set(pad_intro, "detune_cents", -9.0);
s.set(pad_intro, "density", 7.5);
s.set(pad_intro, "duration", 0.72);
s.set(pad_intro, "spray", 2.0);
s.set(pad_intro, "scan_speed", 0.40);
s.set(pad_intro, "gain", 0.30);
s.set(pad_intro, "additive", 0.50);
s.set(pad_intro, "feather", 0.35);
s.set(pad_intro, "flicker", 0.10);
s.set(pad_intro, "size_scale", 1.7);
s.audio_filter(pad_intro, "lowpass", 1550.0, 0.72);
s.audio_filter(pad_intro, "highpass", 135.0, 0.70);

// Breakdown pad: pitched down with warm lower-octave voice (never upward squeak!)
let pad_brk = s.clip(t_pad, chord_wash, 80.0, 31.5);
s.key(pad_brk, "F", "minor");
s.set(pad_brk, "quantize_amount", 0.80);
s.set(pad_brk, "detune_cents", 8.0);
s.set(pad_brk, "density", 8.5);
s.set(pad_brk, "duration", 0.80);
s.set(pad_brk, "position", 0.24);
s.set(pad_brk, "spray", 2.4);
s.set(pad_brk, "scan_speed", 0.32);
s.set(pad_brk, "gain", 0.34);
s.set(pad_brk, "voices", 2.0);
s.set(pad_brk, "voice_interval", -12.0);
s.set(pad_brk, "voice_detune", 0.10);
s.set(pad_brk, "additive", 0.55);
s.set(pad_brk, "feather", 0.40);
s.set(pad_brk, "flicker", 0.10);
s.audio_filter(pad_brk, "lowpass", 1750.0, 0.72);
s.audio_filter(pad_brk, "highpass", 115.0, 0.70);

let pad_re = s.clip(t_pad, chord_wash, 112.0, 46.0);
s.key(pad_re, "F", "minor");
s.set(pad_re, "detune_cents", -7.0);
s.set(pad_re, "density", 8.0);
s.set(pad_re, "duration", 0.68);
s.set(pad_re, "position", 0.48);
s.set(pad_re, "spray", 1.8);
s.set(pad_re, "gain", 0.30);
s.set(pad_re, "additive", 0.50);
s.set(pad_re, "feather", 0.35);
s.set(pad_re, "flicker", 0.12);
s.audio_filter(pad_re, "lowpass", 1650.0, 0.72);
s.audio_filter(pad_re, "highpass", 135.0, 0.70);

let pad_verb = s.track_effect(t_pad, "reverb");
s.track_fx(t_pad, pad_verb, "size", 0.80);
s.track_fx(t_pad, pad_verb, "damp", 0.68);
s.track_fx(t_pad, pad_verb, "mix", 0.38);

// ============================================================================
// TRACK 2: Deep Varispeed Sub / Reese Bass (additive wet-highway lower smears)
// ============================================================================
let t_sub = s.track("sub_bass");
s.track_blend(t_sub, 0.85); // additive glow so night rain is NEVER blacked out!
s.track_opacity_link(t_sub, 0.55);

// 4-bar F-minor bass movement (varispeed = 1 -> single pristine Hermite grain, zero OLA warble/strobe!):
//   Beats 0..6:   F  (0 st)
//   Beats 6..8:   Eb (-2 st)
//   Beats 8..13:  Db (-4 st)
//   Beats 13..16: C  (-5 st) or Ab (+3 st on Drop II second half)
for bar_start in [32.0, 48.0, 64.0, 112.0, 128.0] {
    let n1 = s.snippet(t_sub, sub_src, bar_start + 0.0, 5.6);
    s.set(n1, "pitch", 0.0);
    s.set(n1, "varispeed", 1.0);
    s.set(n1, "gain", 0.62);
    s.set(n1, "envelope", 0.16);
    s.set(n1, "additive", 0.80);
    s.transform(n1, 0.0, 0.36, 1.08, 0.0);

    let step2_st = if bar_start >= 112.0 { 3.0 } else { -2.0 };
    let n2 = s.snippet(t_sub, sub_src, bar_start + 6.0, 1.8);
    s.set(n2, "pitch", step2_st);
    s.set(n2, "varispeed", 1.0);
    s.set(n2, "gain", 0.56);
    s.set(n2, "envelope", 0.20);
    s.set(n2, "additive", 0.80);
    s.transform(n2, -0.06, 0.36, 1.04, 0.0);

    let n3 = s.snippet(t_sub, sub_src, bar_start + 8.0, 4.65);
    s.set(n3, "pitch", -4.0);
    s.set(n3, "varispeed", 1.0);
    s.set(n3, "gain", 0.64);
    s.set(n3, "envelope", 0.16);
    s.set(n3, "additive", 0.80);
    s.transform(n3, 0.05, 0.38, 1.12, 0.0);

    let turn_st = if bar_start == 48.0 { -2.0 } else { -5.0 };
    let n4 = s.snippet(t_sub, sub_src, bar_start + 13.0, 2.65);
    s.set(n4, "pitch", turn_st);
    s.set(n4, "varispeed", 1.0);
    s.set(n4, "gain", 0.58);
    s.set(n4, "envelope", 0.20);
    s.set(n4, "additive", 0.80);
    s.transform(n4, 0.0, 0.37, 1.06, 0.0);
}

// Heavy visual persistence smear on sub headlights; audio stays dry & tight
let sub_smear = s.track_effect(t_sub, "reverb");
s.track_fx(t_sub, sub_smear, "size", 0.88);
s.track_fx(t_sub, sub_smear, "damp", 0.72);
s.track_fx(t_sub, sub_smear, "mix", 0.65);
s.track_fx(t_sub, sub_smear, "audio", 0.0);
s.track_fx(t_sub, sub_smear, "video", 1.0);

// ============================================================================
// TRACK 3: Swung 2-Step UK Garage Drums (60% Shuffle + Micro-Nudged Pocket)
// ============================================================================
let t_drums = s.track("ukg_2step_drums");
s.track_blend(t_drums, 0.85); // glowing bokeh/train flashes over night window, zero blackout!
s.track_tape(t_drums, 0.06, 0.22, 0.14, 0.0);

// Wire real sidechain ducking from drums -> sub & pad!
s.track_duck(t_sub, t_drums, 0.68, 0.005, 0.19);
s.track_duck_opacity(t_sub, 0.25);
s.track_duck(t_pad, t_drums, 0.40, 0.010, 0.24);
s.track_duck_opacity(t_pad, 0.20);

// Pattern A: Intro sparse ghost foley (8 beats = 32 steps, 60% shuffle)
let p_intro = s.pattern("intro_ghosts");
s.pattern_grid(p_intro, 8.0, 4);
s.pattern_swing(p_intro, 0.60);

let ig_rim = s.row(p_intro, rim_src);
s.row_set(p_intro, ig_rim, "duration", 0.34);
s.row_set(p_intro, ig_rim, "gain", 0.32);
s.row_set(p_intro, ig_rim, "envelope", 0.14);
s.row_set(p_intro, ig_rim, "env_skew", -0.80);
s.step(p_intro, ig_rim, 12, true);
s.step_offset(p_intro, ig_rim, 12, 0.06);
s.step(p_intro, ig_rim, 28, true);
s.step_gain(p_intro, ig_rim, 28, 0.55);

let ig_shk = s.row(p_intro, hat_src);
s.row_set(p_intro, ig_shk, "position", 0.50);
s.row_set(p_intro, ig_shk, "duration", 0.15);
s.row_set(p_intro, ig_shk, "gain", 0.24);
s.row_set(p_intro, ig_shk, "pan_spread", 0.65);
s.row_set(p_intro, ig_shk, "env_skew", -0.65);
s.row_humanize(p_intro, ig_shk, 0.006);
for st in [2, 6, 11, 14, 18, 22, 27] {
    s.step(p_intro, ig_shk, st, true);
}

// Pattern B: Drop I 2-step UKG groove (8 beats = 32 steps, 60% shuffle + laid-back rim)
let p_drop1 = s.pattern("ukg_drop1");
s.pattern_grid(p_drop1, 8.0, 4);
s.pattern_swing(p_drop1, 0.60);

let r_kick = s.row(p_drop1, kick_src);
s.row_set(p_drop1, r_kick, "duration", 0.34);
s.row_set(p_drop1, r_kick, "gain", 0.74);
s.row_set(p_drop1, r_kick, "pan_spread", 0.0);
s.row_set(p_drop1, r_kick, "envelope", 0.08);
s.row_set(p_drop1, r_kick, "env_skew", -0.88);
for st in [0, 10, 16, 19, 26] {
    s.step(p_drop1, r_kick, st, true);
}
s.step_gain(p_drop1, r_kick, 10, 0.86);
s.step_gain(p_drop1, r_kick, 19, 0.46); // swung ghost kick!
s.step_offset(p_drop1, r_kick, 19, 0.05);
s.step_gain(p_drop1, r_kick, 26, 0.84);

let r_rim = s.row(p_drop1, rim_src);
s.row_set(p_drop1, r_rim, "duration", 0.32);
s.row_set(p_drop1, r_rim, "gain", 0.64);
s.row_set(p_drop1, r_rim, "pan_spread", 0.16);
s.row_set(p_drop1, r_rim, "envelope", 0.08);
s.row_set(p_drop1, r_rim, "env_skew", -0.85);
s.row_humanize(p_drop1, r_rim, 0.004);
for st in [4, 7, 12, 15, 20, 28, 31] {
    s.step(p_drop1, r_rim, st, true);
}
// Slightly laid-back snare/rim pocket on beats 2 & 4
for backbeat in [4, 12, 20, 28] {
    s.step_offset(p_drop1, r_rim, backbeat, 0.05);
}
s.step_gain(p_drop1, r_rim, 7, 0.30);  s.step_pitch(p_drop1, r_rim, 7, -2.0);
s.step_gain(p_drop1, r_rim, 15, 0.25); s.step_pitch(p_drop1, r_rim, 15, -1.0);
s.step_gain(p_drop1, r_rim, 31, 0.34); s.step_chance(p_drop1, r_rim, 31, 0.75);

let r_ch = s.row(p_drop1, hat_src);
s.row_swing(p_drop1, r_ch, 0.62); // extra garage shuffle on hats!
s.row_humanize(p_drop1, r_ch, 0.005);
s.row_set(p_drop1, r_ch, "position", 0.0);
s.row_set(p_drop1, r_ch, "duration", 0.10);
s.row_set(p_drop1, r_ch, "gain", 0.34);
s.row_set(p_drop1, r_ch, "pan_spread", 0.72);
s.row_set(p_drop1, r_ch, "envelope", 0.14);
s.row_set(p_drop1, r_ch, "env_skew", -0.75);
for st in [0, 2, 3, 6, 8, 11, 14, 16, 18, 22, 23, 26, 27, 30] {
    s.step(p_drop1, r_ch, st, true);
}
for ghost_st in [3, 11, 23, 27] {
    s.step_gain(p_drop1, r_ch, ghost_st, 0.48);
    s.step_pitch(p_drop1, r_ch, ghost_st, -2.0);
}

let r_oh = s.row(p_drop1, hat_src);
s.row_set(p_drop1, r_oh, "position", 0.25);
s.row_set(p_drop1, r_oh, "duration", 0.22);
s.row_set(p_drop1, r_oh, "gain", 0.28);
s.row_set(p_drop1, r_oh, "pan", 0.22);
s.row_set(p_drop1, r_oh, "pan_spread", 0.40);
s.row_set(p_drop1, r_oh, "envelope", 0.16);
s.row_set(p_drop1, r_oh, "env_skew", -0.70);
for st in [2, 10, 18, 26] {
    s.step(p_drop1, r_oh, st, true);
}

let r_wb = s.row(p_drop1, hat_src);
s.row_set(p_drop1, r_wb, "position", 0.75);
s.row_set(p_drop1, r_wb, "duration", 0.17);
s.row_set(p_drop1, r_wb, "gain", 0.34);
s.row_set(p_drop1, r_wb, "pan", -0.26);
s.row_set(p_drop1, r_wb, "pan_spread", 0.38);
s.row_set(p_drop1, r_wb, "env_skew", -0.80);
for st in [6, 15, 21, 29] {
    s.step(p_drop1, r_wb, st, true);
}
s.step_pitch(p_drop1, r_wb, 15, -3.0);
s.step_pitch(p_drop1, r_wb, 29, -5.0);

// Pattern C: Evolved Drop II groove (adds syncopated rim stutter, 7-step polymeter
// shaker, AND a 12-step rhythmic micro-vocal breath chop row interlocked with the beat!)
let p_drop2 = s.pattern("ukg_drop2_evolved");
s.pattern_grid(p_drop2, 8.0, 4);
s.pattern_swing(p_drop2, 0.61);

let r2_kick = s.row(p_drop2, kick_src);
s.row_set(p_drop2, r2_kick, "duration", 0.34);
s.row_set(p_drop2, r2_kick, "gain", 0.76);
s.row_set(p_drop2, r2_kick, "envelope", 0.08);
s.row_set(p_drop2, r2_kick, "env_skew", -0.88);
for st in [0, 7, 10, 16, 19, 26, 29] {
    s.step(p_drop2, r2_kick, st, true);
}
s.step_gain(p_drop2, r2_kick, 7, 0.42);
s.step_gain(p_drop2, r2_kick, 19, 0.50);
s.step_gain(p_drop2, r2_kick, 29, 0.44);

let r2_rim = s.row(p_drop2, rim_src);
s.row_set(p_drop2, r2_rim, "duration", 0.32);
s.row_set(p_drop2, r2_rim, "gain", 0.66);
s.row_set(p_drop2, r2_rim, "pan_spread", 0.20);
s.row_set(p_drop2, r2_rim, "envelope", 0.08);
s.row_set(p_drop2, r2_rim, "env_skew", -0.85);
s.row_humanize(p_drop2, r2_rim, 0.004);
for st in [4, 9, 12, 15, 20, 25, 28, 30, 31] {
    s.step(p_drop2, r2_rim, st, true);
}
for backbeat in [4, 12, 20, 28] {
    s.step_offset(p_drop2, r2_rim, backbeat, 0.05);
}
s.step_gain(p_drop2, r2_rim, 9, 0.32);  s.step_pitch(p_drop2, r2_rim, 9, -2.0);
s.step_gain(p_drop2, r2_rim, 15, 0.28);
s.step_gain(p_drop2, r2_rim, 25, 0.34); s.step_pitch(p_drop2, r2_rim, 25, -4.0);
s.step_gain(p_drop2, r2_rim, 30, 0.38); s.step_pitch(p_drop2, r2_rim, 30, -2.0);
s.step_gain(p_drop2, r2_rim, 31, 0.44);

let r2_ch = s.row(p_drop2, hat_src);
s.row_swing(p_drop2, r2_ch, 0.62);
s.row_humanize(p_drop2, r2_ch, 0.005);
s.row_set(p_drop2, r2_ch, "position", 0.0);
s.row_set(p_drop2, r2_ch, "duration", 0.10);
s.row_set(p_drop2, r2_ch, "gain", 0.36);
s.row_set(p_drop2, r2_ch, "pan_spread", 0.78);
s.row_set(p_drop2, r2_ch, "env_skew", -0.75);
for st in [0, 1, 2, 3, 6, 8, 10, 11, 14, 16, 18, 21, 22, 23, 26, 27, 30] {
    s.step(p_drop2, r2_ch, st, true);
}
for gs in [1, 3, 11, 21, 23, 27] {
    s.step_gain(p_drop2, r2_ch, gs, 0.45);
    s.step_pitch(p_drop2, r2_ch, gs, -2.5);
}

let r2_shk = s.row(p_drop2, hat_src);
s.row_steps(p_drop2, r2_shk, 7); // 7-step polymeter shaker drifting across bars!
s.row_set(p_drop2, r2_shk, "position", 0.50);
s.row_set(p_drop2, r2_shk, "duration", 0.13);
s.row_set(p_drop2, r2_shk, "gain", 0.28);
s.row_set(p_drop2, r2_shk, "pan_spread", 0.85);
s.row_set(p_drop2, r2_shk, "env_skew", -0.65);
s.step(p_drop2, r2_shk, 0, true);
s.step(p_drop2, r2_shk, 3, true); s.step_gain(p_drop2, r2_shk, 3, 0.62);
s.step(p_drop2, r2_shk, 5, true); s.step_gain(p_drop2, r2_shk, 5, 0.52);

// Place drum clips (calm visual `flicker` = 0.18 so hits bloom without camera-flash strobe!)
let c_d_intro = s.pattern_clip(t_drums, p_intro, 16.0, 14.0); // stops at beat 30 for pre-drop hush!
s.set(c_d_intro, "additive", 0.70);
s.set(c_d_intro, "flicker", 0.16);
s.set(c_d_intro, "feather", 0.25);
s.set(c_d_intro, "size_scale", 1.25);
s.automate(c_d_intro, "gain", 0.0, 0.40);
s.automate(c_d_intro, "gain", 14.0, 0.88);

let c_d_drop1 = s.pattern_clip(t_drums, p_drop1, 32.0, 48.0);
s.set(c_d_drop1, "additive", 0.78);
s.set(c_d_drop1, "flicker", 0.18);
s.set(c_d_drop1, "feather", 0.20);
s.set(c_d_drop1, "size_scale", 1.15);
s.set(c_d_drop1, "min_size", 0.09);
s.set(c_d_drop1, "max_size", 0.38);

let c_d_riser = s.pattern_clip(t_drums, p_intro, 104.0, 6.0); // stops at 110 for Drop II hush!
s.set(c_d_riser, "additive", 0.70);
s.set(c_d_riser, "flicker", 0.16);
s.automate(c_d_riser, "gain", 0.0, 0.35);
s.automate(c_d_riser, "gain", 6.0, 0.90);

let c_d_drop2 = s.pattern_clip(t_drums, p_drop2, 112.0, 32.0);
s.set(c_d_drop2, "additive", 0.82);
s.set(c_d_drop2, "flicker", 0.18);
s.set(c_d_drop2, "feather", 0.20);
s.set(c_d_drop2, "size_scale", 1.22);
s.set(c_d_drop2, "min_size", 0.09);
s.set(c_d_drop2, "max_size", 0.42);

// ============================================================================
// TRACK 4: Dub Rimshot Throws (drifting silver subway window echoes)
// ============================================================================
let t_dub = s.track("dub_rim_throws");
s.track_blend(t_dub, 0.80);
for hit_b in [28.0, 44.0, 60.0, 76.0, 108.0, 124.0, 140.0, 148.0] {
    let th = s.snippet(t_dub, rim_src, hit_b, 0.75);
    s.set(th, "pitch", -4.0);
    s.set(th, "varispeed", 1.0);
    s.set(th, "gain", 0.36);
    s.set(th, "additive", 0.75);
    s.set(th, "feather", 0.30);
    s.transform(th, -0.16, -0.06, 0.58, -2.5);
}
let dub_dl = s.track_effect(t_dub, "delay");
s.track_fx(t_dub, dub_dl, "time", 0.338); // dotted-8th at 133 BPM
s.track_fx(t_dub, dub_dl, "feedback", 0.58);
s.track_fx(t_dub, dub_dl, "mix", 0.62);
s.track_fx(t_dub, dub_dl, "shift_x", 0.065);
s.track_fx(t_dub, dub_dl, "shift_y", -0.015);

// ============================================================================
// TRACK 5: Low Pitched-Down Vocal Ghost Halo (warm -5 / -12 st, zero squeak!)
// ============================================================================
let t_vghost = s.track("vocal_ghosts");
s.track_blend(t_vghost, 0.68);

// Soft reversed breath swells leading into bar lines (pitched DOWN -5 st, never up!)
for sw_b in [29.0, 61.5, 77.5, 109.0, 141.5] {
    let rev = s.snippet(t_vghost, vbench_cut, sw_b, 2.0);
    s.set(rev, "position", 0.36);
    s.set(rev, "pitch", -5.0);
    s.set(rev, "varispeed", 1.0);
    s.set(rev, "reverse_prob", 1.0);
    s.set(rev, "envelope", 0.85);
    s.set(rev, "env_skew", 0.82);
    s.set(rev, "gain", 0.36);
    s.set(rev, "additive", 0.65);
    s.set(rev, "feather", 0.35);
    s.audio_filter(rev, "lowpass", 1800.0, 0.75);
    s.transform(rev, 0.14, -0.04, 0.88, 0.0);
}

// Warm chest-register ghost cloud in Breakdown & Drop II (-5 st & -12 st, LP 1250 Hz)
let v_halo = s.clip(t_vghost, vbench_cut, 84.0, 60.0);
s.key(v_halo, "F", "minor_pentatonic");
s.set(v_halo, "position", 0.35);
s.set(v_halo, "density", 5.5);
s.set(v_halo, "duration", 0.38);
s.set(v_halo, "spray", 1.4);
s.set(v_halo, "pitch", -5.0);
s.set(v_halo, "pitch_jitter", 3.0);
s.set(v_halo, "reverse_prob", 0.40);
s.set(v_halo, "pan_spread", 0.82);
s.set(v_halo, "gain", 0.22);
s.set(v_halo, "additive", 0.65);
s.set(v_halo, "feather", 0.40);
s.set(v_halo, "flicker", 0.12);
s.audio_filter(v_halo, "lowpass", 1250.0, 0.75);
s.audio_filter(v_halo, "highpass", 180.0, 0.70);
s.automate(v_halo, "gain", 0.0, 0.10);
s.automate(v_halo, "gain", 24.0, 0.24);
s.automate(v_halo, "gain", 60.0, 0.06);

let vg_verb = s.track_effect(t_vghost, "reverb");
s.track_fx(t_vghost, vg_verb, "size", 0.84);
s.track_fx(t_vghost, vg_verb, "damp", 0.65);
s.track_fx(t_vghost, vg_verb, "mix", 0.48);

// ============================================================================
// TRACK 6: Sampled Wordless Micro-Chops (segmented human cutouts, 0.25-0.65 s!)
// ============================================================================
let t_voc = s.track("vocal_micro_chops");
s.track_blend(t_voc, 0.50);
s.track_tape(t_voc, 0.18, 0.24, 0.22, 0.0);

let sl_b0 = if bench_slices.len() > 0 { bench_slices[0] } else { 0.02 };
let sl_b1 = if bench_slices.len() > 2 { bench_slices[2] } else { 0.35 };
let sl_b2 = if bench_slices.len() > 4 { bench_slices[4] } else { 0.68 };
let sl_s0 = if street_slices.len() > 0 { street_slices[0] } else { 0.02 };
let sl_s1 = if street_slices.len() > 2 { street_slices[2] } else { 0.36 };
let sl_s2 = if street_slices.len() > 4 { street_slices[4] } else { 0.70 };

// --- INTRO: Two sparse, haunting pitched-down breath/hum fragments with plenty of air ---
let vi_1 = s.snippet(t_voc, vstreet_cut, 8.5, 1.3); // ~0.58s breath
s.set(vi_1, "position", sl_s2);
s.set(vi_1, "pitch", -5.0);
s.set(vi_1, "varispeed", 1.0);
s.set(vi_1, "gain", 0.50);
s.set(vi_1, "envelope", 0.35);
s.set(vi_1, "feather", 0.25);
s.transform(vi_1, 0.18, 0.02, 0.90, 0.0);

let vi_2 = s.snippet(t_voc, vbench_cut, 21.0, 1.4); // ~0.63s closed-lip hum
s.set(vi_2, "position", sl_b0);
s.set(vi_2, "pitch", -3.0); // diatonic in F minor (G->E? hum is on F -> D/C; quantized to F minor!)
s.key(vi_2, "F", "minor");
s.set(vi_2, "varispeed", 1.0);
s.set(vi_2, "gain", 0.48);
s.set(vi_2, "envelope", 0.35);
s.set(vi_2, "feather", 0.25);
s.transform(vi_2, -0.14, 0.04, 0.95, 0.0);

// --- DROP I (32..80): Sequenced 2-bar micro-chop call & response (strictly 0, -2, -5, -7 st!) ---
let p_voc1 = s.pattern("drop1_vocal_chops");
s.pattern_grid(p_voc1, 16.0, 4); // 4-bar (64-step) sparse phrasing!
s.pattern_swing(p_voc1, 0.60);

let rv_fem = s.row(p_voc1, vbench_cut);
s.row_key(p_voc1, rv_fem, "F", "minor_pentatonic");
s.row_set(p_voc1, rv_fem, "duration", 0.42);
s.row_set(p_voc1, rv_fem, "gain", 0.56);
s.row_set(p_voc1, rv_fem, "envelope", 0.32);
s.row_set(p_voc1, rv_fem, "pan", -0.15);
s.row_set(p_voc1, rv_fem, "pan_spread", 0.25);

// Only 4 short micro-chops across 64 steps -> generous space & syncopation!
s.step(p_voc1, rv_fem, 2, true);  s.step_slice(p_voc1, rv_fem, 2, sl_b0);  s.step_pitch(p_voc1, rv_fem, 2, 0.0);
s.step(p_voc1, rv_fem, 11, true); s.step_slice(p_voc1, rv_fem, 11, sl_b1); s.step_pitch(p_voc1, rv_fem, 11, -5.0); s.step_length(p_voc1, rv_fem, 11, 0.85);
s.step(p_voc1, rv_fem, 34, true); s.step_slice(p_voc1, rv_fem, 34, sl_b1); s.step_pitch(p_voc1, rv_fem, 34, -2.0);
s.step(p_voc1, rv_fem, 46, true); s.step_slice(p_voc1, rv_fem, 46, sl_b2); s.step_pitch(p_voc1, rv_fem, 46, -7.0); s.step_length(p_voc1, rv_fem, 46, 1.15);

let rv_mal = s.row(p_voc1, vstreet_cut);
s.row_key(p_voc1, rv_mal, "F", "minor_pentatonic");
s.row_set(p_voc1, rv_mal, "duration", 0.38);
s.row_set(p_voc1, rv_mal, "gain", 0.54);
s.row_set(p_voc1, rv_mal, "envelope", 0.35);
s.row_set(p_voc1, rv_mal, "pan", 0.18);
s.row_set(p_voc1, rv_mal, "pan_spread", 0.25);

s.step(p_voc1, rv_mal, 22, true); s.step_slice(p_voc1, rv_mal, 22, sl_s0); s.step_pitch(p_voc1, rv_mal, 22, -5.0);
s.step(p_voc1, rv_mal, 29, true); s.step_slice(p_voc1, rv_mal, 29, sl_s1); s.step_pitch(p_voc1, rv_mal, 29, -7.0); s.step_gain(p_voc1, rv_mal, 29, 0.72);
s.step(p_voc1, rv_mal, 58, true); s.step_slice(p_voc1, rv_mal, 58, sl_s2); s.step_pitch(p_voc1, rv_mal, 58, -5.0); s.step_length(p_voc1, rv_mal, 58, 1.25);

let c_voc_d1 = s.pattern_clip(t_voc, p_voc1, 32.0, 48.0);
s.set(c_voc_d1, "additive", 0.55);
s.set(c_voc_d1, "flicker", 0.20);
s.set(c_voc_d1, "feather", 0.22);
s.set(c_voc_d1, "min_size", 0.28);
s.set(c_voc_d1, "max_size", 0.62);

// --- BREAKDOWN (80..110): Bare intimate wordless hums & breaths with spacious silence ---
let vb_1 = s.snippet(t_voc, vbench_cut, 82.0, 3.2); // ~1.4s pure closed-lip hum, pitched -2 st
s.key(vb_1, "F", "minor");
s.set(vb_1, "position", 0.02);
s.set(vb_1, "pitch", -2.0);
s.set(vb_1, "varispeed", 1.0);
s.set(vb_1, "gain", 0.58);
s.set(vb_1, "envelope", 0.30);
s.set(vb_1, "feather", 0.28);
s.transform(vb_1, -0.06, 0.02, 1.08, 0.0);

let vb_2 = s.snippet(t_voc, vstreet_cut, 90.5, 2.8); // ~1.25s chest 'oh' sigh, pitched -5 st
s.key(vb_2, "F", "minor");
s.set(vb_2, "position", 0.35);
s.set(vb_2, "pitch", -5.0);
s.set(vb_2, "varispeed", 1.0);
s.set(vb_2, "gain", 0.56);
s.set(vb_2, "envelope", 0.32);
s.set(vb_2, "feather", 0.28);
s.transform(vb_2, 0.14, 0.02, 0.98, 0.0);

let vb_3 = s.snippet(t_voc, vbench_cut, 98.5, 3.6); // ~1.6s descending 'mmm-ooh' dissolving into grains
s.key(vb_3, "F", "minor");
s.set(vb_3, "position", 0.68);
s.set(vb_3, "pitch", -5.0);
s.set(vb_3, "speed", 0.80);
s.set(vb_3, "texture", 0.22);
s.set(vb_3, "density", 14.0);
s.set(vb_3, "duration", 0.28);
s.set(vb_3, "gain", 0.52);
s.set(vb_3, "feather", 0.32);
s.set(vb_3, "flicker", 0.15);
s.transform(vb_3, 0.0, 0.03, 1.12, 0.0);

// 1941 archival NYC subway pneumatic air + breath sigh at beat 106 before Drop II hush
let vb_air = s.snippet(t_voc, vstreet_cut, 105.5, 3.5);
s.set(vb_air, "position", 0.72);
s.set(vb_air, "pitch", -4.0);
s.set(vb_air, "varispeed", 1.0);
s.set(vb_air, "gain", 0.52);
s.set(vb_air, "envelope", 0.35);
s.transform(vb_air, 0.12, 0.0, 0.96, 0.0);

// --- DROP II (112..144): Syncopated 2-step micro-chop stutter + mirrored visual reflection ---
let p_voc2 = s.pattern("drop2_vocal_stutter");
s.pattern_grid(p_voc2, 16.0, 4);
s.pattern_swing(p_voc2, 0.61);

let rv2_a = s.row(p_voc2, vbench_cut);
s.row_key(p_voc2, rv2_a, "F", "minor_pentatonic");
s.row_set(p_voc2, rv2_a, "duration", 0.32);
s.row_set(p_voc2, rv2_a, "gain", 0.58);
s.row_set(p_voc2, rv2_a, "envelope", 0.28);
s.row_set(p_voc2, rv2_a, "pan", -0.18);
s.row_set(p_voc2, rv2_a, "pan_spread", 0.32);

for st in [0, 3, 11, 26, 34, 37, 46] {
    s.step(p_voc2, rv2_a, st, true);
}
s.step_slice(p_voc2, rv2_a, 0, sl_b1);  s.step_pitch(p_voc2, rv2_a, 0, 0.0);
s.step_slice(p_voc2, rv2_a, 3, sl_b1);  s.step_pitch(p_voc2, rv2_a, 3, -2.0); s.step_gain(p_voc2, rv2_a, 3, 0.68); s.step_length(p_voc2, rv2_a, 3, 0.65);
s.step_slice(p_voc2, rv2_a, 11, sl_b0); s.step_pitch(p_voc2, rv2_a, 11, -5.0);
s.step_slice(p_voc2, rv2_a, 26, sl_b2); s.step_pitch(p_voc2, rv2_a, 26, -7.0);
s.step_slice(p_voc2, rv2_a, 34, sl_b1); s.step_pitch(p_voc2, rv2_a, 34, -2.0);
s.step_slice(p_voc2, rv2_a, 37, sl_b0); s.step_pitch(p_voc2, rv2_a, 37, -5.0); s.step_gain(p_voc2, rv2_a, 37, 0.65); s.step_length(p_voc2, rv2_a, 37, 0.65);
s.step_slice(p_voc2, rv2_a, 46, sl_b2); s.step_pitch(p_voc2, rv2_a, 46, -12.0); s.step_length(p_voc2, rv2_a, 46, 1.30);

let rv2_b = s.row(p_voc2, vstreet_cut);
s.row_key(p_voc2, rv2_b, "F", "minor_pentatonic");
s.row_set(p_voc2, rv2_b, "duration", 0.36);
s.row_set(p_voc2, rv2_b, "gain", 0.54);
s.row_set(p_voc2, rv2_b, "envelope", 0.32);
s.row_set(p_voc2, rv2_b, "pan", 0.20);
s.row_set(p_voc2, rv2_b, "pan_spread", 0.30);

s.step(p_voc2, rv2_b, 18, true); s.step_slice(p_voc2, rv2_b, 18, sl_s0); s.step_pitch(p_voc2, rv2_b, 18, -5.0);
s.step(p_voc2, rv2_b, 22, true); s.step_slice(p_voc2, rv2_b, 22, sl_s1); s.step_pitch(p_voc2, rv2_b, 22, -7.0);
s.step(p_voc2, rv2_b, 54, true); s.step_slice(p_voc2, rv2_b, 54, sl_s1); s.step_pitch(p_voc2, rv2_b, 54, -5.0);
s.step(p_voc2, rv2_b, 59, true); s.step_slice(p_voc2, rv2_b, 59, sl_s2); s.step_pitch(p_voc2, rv2_b, 59, -12.0); s.step_length(p_voc2, rv2_b, 59, 1.25);

let c_voc_d2 = s.pattern_clip(t_voc, p_voc2, 112.0, 32.0);
s.set(c_voc_d2, "additive", 0.60);
s.set(c_voc_d2, "flicker", 0.18);
s.set(c_voc_d2, "feather", 0.22);
s.set(c_voc_d2, "min_size", 0.28);
s.set(c_voc_d2, "max_size", 0.62);
// Mirrored dual-reflection visual treatment in Drop II so climax looks distinct from Drop I!
let d2_tile = s.effect(c_voc_d2, "tile");
s.fx(c_voc_d2, d2_tile, "count", 2.0);
s.fx(c_voc_d2, d2_tile, "mirror", 1.0);
s.fx(c_voc_d2, d2_tile, "audio", 0.0);
s.fx(c_voc_d2, d2_tile, "video", 0.45);

// --- OUTRO (144..160): Single low wordless hum fading into night rain & vinyl dust ---
let v_out = s.snippet(t_voc, vbench_cut, 146.0, 5.5);
s.key(v_out, "F", "minor");
s.set(v_out, "position", 0.02);
s.set(v_out, "pitch", -5.0);
s.set(v_out, "varispeed", 1.0);
s.set(v_out, "gain", 0.46);
s.set(v_out, "envelope", 0.45);
s.set(v_out, "feather", 0.35);
s.transform(v_out, 0.0, 0.04, 1.02, 0.0);

let voc_dl = s.track_effect(t_voc, "delay");
s.track_fx(t_voc, voc_dl, "time", 0.338);
s.track_fx(t_voc, voc_dl, "feedback", 0.42);
s.track_fx(t_voc, voc_dl, "mix", 0.28);
s.track_fx(t_voc, voc_dl, "shift_x", 0.04);
s.track_fx(t_voc, voc_dl, "shift_y", -0.01);

let voc_rv = s.track_effect(t_voc, "reverb");
s.track_fx(t_voc, voc_rv, "size", 0.74);
s.track_fx(t_voc, voc_rv, "damp", 0.64);
s.track_fx(t_voc, voc_rv, "mix", 0.30);

// ============================================================================
// MASTER BUS: Warm analog tape glue + lo-fi DCT film block character
// ============================================================================
s.master_tape(0.16, 0.22, 0.14, 0.08);

let m_comp = s.master_effect("compress");
s.master_fx(m_comp, "quality", 0.80);
s.master_fx(m_comp, "audio", 0.04);
s.master_fx(m_comp, "video", 0.30);

let m_verb = s.master_effect("reverb");
s.master_fx(m_verb, "size", 0.58);
s.master_fx(m_verb, "damp", 0.64);
s.master_fx(m_verb, "mix", 0.10);

s.render(0.0, 160.0, "outputs/261111_burial-demo/v002/261111_chromagrain_burial-demo_v002.mp4");
