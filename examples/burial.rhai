// examples/burial.rhai — Nocturnal Subterranean 2-Step AV Piece ("Night Platform")
// Run headless:
//   target/release/chromagrain --script examples/burial.rhai
//
// Key: F minor (A=440 locked) | Tempo: 133 BPM (40 bars = 160 beats = 72.2 s)
// Zero lyric sentences, zero TTS speech, zero raw exposed AI vocal stems — built
// from public-domain NYC subway & night-highway archival footage, vinyl-resampled
// wordless vowel/breath fragments, 60%-swung 2-step percussion, 133 BPM bogie
// rail-joint rhythm, low-band sidechained F1 Reese bass, and audio-reactive visuals.
//
// 5-Act Audiovisual Montage (each act owns its own visual space):
//   0..32   (0:00-0:14.4) Act I - Rainy Night Window: sodium raindrops, vinyl dust,
//                         velvet Fm9 paulstretch wash, two distant resampled sighs
//   32..80  (0:14.4-0:36.1) Act II - Wet Night Highway: rain-lamp dissolves out;
//                         wet asphalt headlights & bokeh pulse with kick/2-step groove;
//                         sparse pass-conditional ('step_every') shadow silhouettes
//   80..112 (0:36.1-0:50.5) Act III - Nocturnal Water Breakdown: beatless glowing
//                         water reflections & grounded nocturnal shadow silhouettes
//                         echoing concise resampled dubplate fragments across silence
//   112..144(0:50.5-1:05.0) Act IV - Subterranean Tunnel Climax: pure dark rushing
//                         1941 NYC subway tunnel + sample-locked 133 BPM rail-joint
//                         'da-CLACK' groove, kick-pulsed tunnel glow & mirrored ghosts
//   144..160(0:65.0-1:12.2) Act V - Return to Rain Glass: tunnel dissolves back into
//                         quiet sodium raindrops, vinyl crackle, and one fading sigh

let s = session(133.0);
s.canvas(640, 360, 24.0);

// ---- Load curated AV sample sources (assets/demo/) ----
let rain_src    = s.load("assets/demo/rain_window.mp4");
let chords_src  = s.load("assets/demo/night_chords.mp4");
let sub_src     = s.load("assets/demo/sub_highway.mp4");
let kick_src    = s.load("assets/demo/kick_tunnel.mp4");
let rim_src     = s.load("assets/demo/rim_train.mp4");
let hat_src     = s.load("assets/demo/hat_glitter.mp4");
let tunnel_src  = s.load("assets/demo/subway_tunnel.mp4");
let vbench_src  = s.load("assets/demo/vocal_bench.mp4");
let vstreet_src = s.load("assets/demo/vocal_street.mp4");

// Lock exact A=440 fundamentals so Key quantization stays within ±3 cents across all layers!
s.set_base_hz(sub_src,     87.307);  // F2 (audio carries deep F1 43.65 Hz + warm F2 Reese)
s.set_base_hz(chords_src,  174.614); // F3
s.set_base_hz(vbench_src,  391.995); // G4
s.set_base_hz(vstreet_src, 466.164); // Bb4

// Derive 6x paulstretched AV chord wash (85-cent critical-band smear: silky air, zero whistle)
let chord_wash = s.paulstretch(chords_src, 6.0);

// Apple Vision foreground people cutouts (v2 precision-filtered: pure human silhouette,
// zero subway-bench/wall rectangle!)
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
s.set_base_hz(vbench_cut,  391.995);
s.set_base_hz(vstreet_cut, 466.164);

// ============================================================================
// TRACK 0: Rain & 12" Vinyl Surface Bed
//          (Audio runs continuously; rain-lamp VIDEO bookends Intro & Outro only
//           so Acts II, III & IV fully own the frame without a static lamp!)
// ============================================================================
let t_vinyl = s.track("vinyl_rain_audio");
s.track_opacity_link(t_vinyl, 1.0);
s.track_eq(t_vinyl, 320.0, 1.2, -3.5); // keep 220-500 Hz crystal clean
s.track_level_point(t_vinyl, 0.0, 0.0);
s.track_level_point(t_vinyl, 30.0, 0.0);
s.track_level_point(t_vinyl, 33.0, 0.68); // continuous vinyl/rain audio while rain video leaves
s.track_level_point(t_vinyl, 143.0, 0.68);
s.track_level_point(t_vinyl, 146.5, 0.0);

let vin_aud = s.snippet(t_vinyl, rain_src, 30.0, 117.0);
s.set(vin_aud, "gain", 0.24);
s.set(vin_aud, "speed", 0.75);
// Off-canvas audio-only bed during Acts II-IV so the orange rain lamp completely leaves!
s.transform(vin_aud, 0.0, 4.0, 0.02, 0.0);

let t_rain = s.track("rain_window_bookends");
s.track_opacity_link(t_rain, 0.85);
s.track_eq(t_rain, 320.0, 1.2, -3.0);
s.track_level_point(t_rain, 0.0, 0.0);
s.track_level_point(t_rain, 4.0, 0.90);   // Act I: rainy night sodium glass
s.track_level_point(t_rain, 28.5, 0.86);
s.track_level_point(t_rain, 33.5, 0.0);   // dissolves cleanly into wet highway!
s.track_level_point(t_rain, 142.5, 0.0);
s.track_level_point(t_rain, 146.5, 0.88); // Act V: quiet return to rain-streaked glass
s.track_level_point(t_rain, 152.5, 0.82);
s.track_level_point(t_rain, 160.0, 0.0);

let rain_intro = s.snippet(t_rain, rain_src, 0.0, 34.0);
s.set(rain_intro, "gain", 0.28);
s.set(rain_intro, "gain_to_opacity", 0.25);
s.set(rain_intro, "speed", 0.72);
s.set(rain_intro, "envelope", 0.12);

let rain_outro = s.snippet(t_rain, rain_src, 142.5, 17.5);
s.set(rain_outro, "gain", 0.28);
s.set(rain_outro, "gain_to_opacity", 0.25);
s.set(rain_outro, "speed", 0.70);
s.set(rain_outro, "envelope", 0.18);

let rain_tint = s.track_effect(t_rain, "tint");
s.track_fx(t_rain, rain_tint, "hue", 28.0);
s.track_fx(t_rain, rain_tint, "amount", 0.26);
s.track_fx(t_rain, rain_tint, "audio", 0.0);

// ============================================================================
// TRACK 1: Nocturnal Water Reflections & Paulstretched Fm9 Chords (A=440 locked)
// ============================================================================
let t_pad = s.track("paulstretch_pad");
s.track_blend(t_pad, 0.58);
s.track_opacity_link(t_pad, 0.50);
s.track_tape(t_pad, 0.18, 0.18, 0.08, 0.0);
s.track_eq(t_pad, 330.0, 1.15, -4.2); // carve 240-450 Hz boxiness cleanly!

s.track_level_point(t_pad, 0.0, 0.0);
s.track_level_point(t_pad, 9.0, 0.74);
s.track_level_point(t_pad, 29.5, 0.74);
s.track_level_point(t_pad, 31.3, 0.22); // pre-drop breath
s.track_level_point(t_pad, 32.5, 0.58);
s.track_level_point(t_pad, 80.0, 0.86); // Act III Breakdown: nocturnal water fills frame!
s.track_level_point(t_pad, 109.5, 0.82);
s.track_level_point(t_pad, 111.3, 0.20); // pre-drop II breath
s.track_level_point(t_pad, 112.5, 0.58);
s.track_level_point(t_pad, 144.0, 0.54);
s.track_level_point(t_pad, 160.0, 0.0);

let pad_intro = s.clip(t_pad, chord_wash, 1.0, 79.0);
s.key(pad_intro, "F", "minor");
s.set(pad_intro, "quantize_amount", 1.0); // 100% in-tune F minor!
s.set(pad_intro, "detune_cents", -3.0);
s.set(pad_intro, "density", 7.5);
s.set(pad_intro, "duration", 0.72);
s.set(pad_intro, "spray", 2.0);
s.set(pad_intro, "scan_speed", 0.42);
s.set(pad_intro, "gain", 0.28);
s.set(pad_intro, "additive", 0.55);
s.set(pad_intro, "feather", 0.42);
s.set(pad_intro, "flicker", 0.14);
s.set(pad_intro, "size_scale", 1.85);
s.audio_filter(pad_intro, "lowpass", 3800.0, 0.70);
s.audio_filter(pad_intro, "highpass", 220.0, 0.70);

let pad_brk = s.clip(t_pad, chord_wash, 80.0, 31.5);
s.key(pad_brk, "F", "minor");
s.set(pad_brk, "quantize_amount", 1.0);
s.set(pad_brk, "detune_cents", 3.0);
s.set(pad_brk, "density", 9.0);
s.set(pad_brk, "duration", 0.82);
s.set(pad_brk, "position", 0.25);
s.set(pad_brk, "spray", 2.4);
s.set(pad_brk, "scan_speed", 0.34);
s.set(pad_brk, "gain", 0.32);
s.set(pad_brk, "voices", 2.0);
s.set(pad_brk, "voice_interval", -12.0);
s.set(pad_brk, "voice_detune", 0.04);
s.set(pad_brk, "additive", 0.62);
s.set(pad_brk, "feather", 0.45);
s.set(pad_brk, "flicker", 0.14);
s.set(pad_brk, "size_scale", 2.05);
s.audio_filter(pad_brk, "lowpass", 4200.0, 0.70);
s.audio_filter(pad_brk, "highpass", 195.0, 0.70);

let pad_re = s.clip(t_pad, chord_wash, 112.0, 46.0);
s.key(pad_re, "F", "minor");
s.set(pad_re, "quantize_amount", 1.0);
s.set(pad_re, "detune_cents", -3.0);
s.set(pad_re, "density", 8.0);
s.set(pad_re, "duration", 0.68);
s.set(pad_re, "position", 0.48);
s.set(pad_re, "spray", 1.8);
s.set(pad_re, "gain", 0.27);
s.set(pad_re, "additive", 0.52);
s.set(pad_re, "feather", 0.42);
s.set(pad_re, "flicker", 0.14);
s.set(pad_re, "size_scale", 1.75);
s.audio_filter(pad_re, "lowpass", 3800.0, 0.70);
s.audio_filter(pad_re, "highpass", 220.0, 0.70);

let pad_verb = s.track_effect(t_pad, "reverb");
s.track_fx(t_pad, pad_verb, "size", 0.80);
s.track_fx(t_pad, pad_verb, "damp", 0.48);
s.track_fx(t_pad, pad_verb, "mix", 0.36);

// ============================================================================
// TRACK 2: Subterranean 1941 NYC Dark Subway Tunnel & Sample-Locked Rail Groove
//          (Pure dark tunnel rails — zero white doorway flash, zero tempo drift!)
// ============================================================================
let t_tunnel = s.track("subway_tunnel_rhythm");
s.track_blend(t_tunnel, 0.76);
s.track_opacity_link(t_tunnel, 0.65);
s.track_eq(t_tunnel, 340.0, 1.25, -4.0);

// Smooth dark tunnel foreshadow in Intro (bars 5..7)
let tun_pre = s.snippet(t_tunnel, tunnel_src, 16.0, 12.0);
s.set(tun_pre, "gain", 0.22);
s.set(tun_pre, "gain_to_opacity", 0.35);
s.set(tun_pre, "speed", 1.0);
s.set(tun_pre, "envelope", 0.28);
s.set(tun_pre, "feather", 0.32);
s.audio_filter(tun_pre, "highpass", 280.0, 0.70);

// Act IV (Drop II, 112..144): Two exact 16-beat (7.218s) sample-locked dark tunnel passes!
for d2_bar in [112.0, 128.0] {
    let tun_d2 = s.snippet(t_tunnel, tunnel_src, d2_bar, 16.0);
    s.set(tun_d2, "gain", 0.34);
    s.set(tun_d2, "gain_to_opacity", 0.45);
    s.set(tun_d2, "speed", 1.0); // locked 100% to 133 BPM!
    s.set(tun_d2, "envelope", 0.14);
    s.set(tun_d2, "additive", 0.72);
    s.set(tun_d2, "feather", 0.24);
    s.transform(tun_d2, 0.0, 0.0, 1.14, 0.0);
    s.audio_filter(tun_d2, "highpass", 260.0, 0.70);
}

let tun_tint = s.track_effect(t_tunnel, "tint");
s.track_fx(t_tunnel, tun_tint, "hue", 32.0);
s.track_fx(t_tunnel, tun_tint, "amount", 0.34);
s.track_fx(t_tunnel, tun_tint, "audio", 0.0);

// ============================================================================
// TRACK 3: Authoritative F1 Sub / Reese Bass (+2.2 dB deeper weight +
//          full-screen wet-highway night scene in Drop I!)
// ============================================================================
let t_sub = s.track("sub_bass");
s.track_blend(t_sub, 0.80);
s.track_opacity_link(t_sub, 0.65);

for bar_start in [32.0, 48.0, 64.0, 112.0, 128.0] {
    let n1 = s.snippet(t_sub, sub_src, bar_start + 0.0, 5.6);
    s.set(n1, "pitch", 0.0);
    s.set(n1, "varispeed", 1.0);
    s.set(n1, "gain", 0.51); // restored +2.2 dB authoritative F1 (43.65 Hz) chest weight!
    s.set(n1, "gain_to_opacity", 0.45);
    s.set(n1, "envelope", 0.16);
    s.set(n1, "additive", 0.80);
    s.set(n1, "feather", 0.26);
    s.transform(n1, 0.0, 0.12, 1.26, 0.0);

    let step2_st = if bar_start >= 112.0 { 3.0 } else { -2.0 };
    let n2 = s.snippet(t_sub, sub_src, bar_start + 6.0, 1.8);
    s.set(n2, "pitch", step2_st);
    s.set(n2, "varispeed", 1.0);
    s.set(n2, "gain", 0.47);
    s.set(n2, "gain_to_opacity", 0.45);
    s.set(n2, "envelope", 0.20);
    s.set(n2, "additive", 0.80);
    s.set(n2, "feather", 0.26);
    s.transform(n2, -0.05, 0.12, 1.22, 0.0);

    let n3 = s.snippet(t_sub, sub_src, bar_start + 8.0, 4.65);
    s.set(n3, "pitch", -4.0);
    s.set(n3, "varispeed", 1.0);
    s.set(n3, "gain", 0.52);
    s.set(n3, "gain_to_opacity", 0.48);
    s.set(n3, "envelope", 0.16);
    s.set(n3, "additive", 0.80);
    s.set(n3, "feather", 0.26);
    s.transform(n3, 0.05, 0.14, 1.28, 0.0);

    let turn_st = if bar_start == 48.0 { -2.0 } else { -5.0 };
    let n4 = s.snippet(t_sub, sub_src, bar_start + 13.0, 2.65);
    s.set(n4, "pitch", turn_st);
    s.set(n4, "varispeed", 1.0);
    s.set(n4, "gain", 0.48);
    s.set(n4, "gain_to_opacity", 0.45);
    s.set(n4, "envelope", 0.20);
    s.set(n4, "additive", 0.80);
    s.set(n4, "feather", 0.26);
    s.transform(n4, 0.0, 0.13, 1.24, 0.0);
}

let sub_smear = s.track_effect(t_sub, "reverb");
s.track_fx(t_sub, sub_smear, "size", 0.84);
s.track_fx(t_sub, sub_smear, "damp", 0.66);
s.track_fx(t_sub, sub_smear, "mix", 0.52);
s.track_fx(t_sub, sub_smear, "audio", 0.0);
s.track_fx(t_sub, sub_smear, "video", 1.0);

// ============================================================================
// TRACK 4: Swung 2-Step UKG Drums + Strong Audio-Reactive Visual Coupling!
// ============================================================================
let t_drums = s.track("ukg_2step_drums");
s.track_blend(t_drums, 0.92);
s.track_rms_opacity(t_drums, 0.85); // every kick/rim/hat transient drives crisp visual bloom!
s.track_tape(t_drums, 0.04, 0.18, 0.03, 0.0);

// Low-band sidechain (<155 Hz): kicks pump sub & pad audio AND visibly breathe background layers!
s.track_duck(t_sub, t_drums, 0.76, 0.004, 0.18, 155.0);
s.track_duck_opacity(t_sub, -0.45); // headlights bloom brighter on every kick pulse!
s.track_duck(t_pad, t_drums, 0.48, 0.008, 0.22, 155.0);
s.track_duck_opacity(t_pad, 0.55);  // nocturnal water dims ~30% on kicks so whole frame breathes!
s.track_duck(t_tunnel, t_drums, 0.36, 0.006, 0.18, 155.0);
s.track_duck_opacity(t_tunnel, -0.52); // subway tunnel walls pulse forward on every kick!

let drm_tint = s.track_effect(t_drums, "tint");
s.track_fx(t_drums, drm_tint, "hue", 34.0);
s.track_fx(t_drums, drm_tint, "amount", 0.36);
s.track_fx(t_drums, drm_tint, "audio", 0.0);

// Pattern A: Intro sparse ghost foley (8 beats = 32 steps, 60% shuffle)
let p_intro = s.pattern("intro_ghosts");
s.pattern_grid(p_intro, 8.0, 4);
s.pattern_swing(p_intro, 0.60);

let ig_rim = s.row(p_intro, rim_src);
s.row_set(p_intro, ig_rim, "duration", 0.32);
s.row_set(p_intro, ig_rim, "gain", 0.30);
s.row_set(p_intro, ig_rim, "pan", -0.25);
s.row_set(p_intro, ig_rim, "envelope", 0.14);
s.row_set(p_intro, ig_rim, "env_skew", -0.78);
s.step(p_intro, ig_rim, 12, true);
s.step_offset(p_intro, ig_rim, 12, 0.06);
s.step(p_intro, ig_rim, 28, true);
s.step_gain(p_intro, ig_rim, 28, 0.55);

let ig_shk = s.row(p_intro, hat_src);
s.row_set(p_intro, ig_shk, "position", 0.50);
s.row_set(p_intro, ig_shk, "duration", 0.15);
s.row_set(p_intro, ig_shk, "gain", 0.28);
s.row_set(p_intro, ig_shk, "pan_spread", 0.72);
s.row_set(p_intro, ig_shk, "env_skew", -0.65);
s.row_humanize(p_intro, ig_shk, 0.006);
for st in [2, 6, 11, 14, 18, 22, 27] {
    s.step(p_intro, ig_shk, st, true);
}

// Pattern B: Drop I 2-step UKG groove (with pass-conditional ghost fill on every 2nd pass!)
let p_drop1 = s.pattern("ukg_drop1");
s.pattern_grid(p_drop1, 8.0, 4);
s.pattern_swing(p_drop1, 0.60);

let r_kick = s.row(p_drop1, kick_src);
s.row_set(p_drop1, r_kick, "duration", 0.30);
s.row_set(p_drop1, r_kick, "gain", 0.74);
s.row_set(p_drop1, r_kick, "pan_spread", 0.0);
s.row_set(p_drop1, r_kick, "envelope", 0.08);
s.row_set(p_drop1, r_kick, "env_skew", -0.88);
for st in [0, 10, 16, 19, 26] {
    s.step(p_drop1, r_kick, st, true);
}
s.step_gain(p_drop1, r_kick, 10, 0.86);
s.step_gain(p_drop1, r_kick, 19, 0.46);
s.step_offset(p_drop1, r_kick, 19, 0.05);
s.step_gain(p_drop1, r_kick, 26, 0.84);

let r_rim = s.row(p_drop1, rim_src);
s.row_set(p_drop1, r_rim, "duration", 0.30);
s.row_set(p_drop1, r_rim, "gain", 0.65);
s.row_set(p_drop1, r_rim, "pan", -0.18);
s.row_set(p_drop1, r_rim, "pan_spread", 0.28);
s.row_set(p_drop1, r_rim, "envelope", 0.08);
s.row_set(p_drop1, r_rim, "env_skew", -0.82);
s.row_humanize(p_drop1, r_rim, 0.004);
for st in [4, 7, 12, 15, 20, 28, 31] {
    s.step(p_drop1, r_rim, st, true);
}
for backbeat in [4, 12, 20, 28] {
    s.step_offset(p_drop1, r_rim, backbeat, 0.05);
}
s.step_gain(p_drop1, r_rim, 7, 0.30);  s.step_pitch(p_drop1, r_rim, 7, -2.0);
s.step_gain(p_drop1, r_rim, 15, 0.25); s.step_pitch(p_drop1, r_rim, 15, -1.0);
s.step_gain(p_drop1, r_rim, 31, 0.36); s.step_every(p_drop1, r_rim, 31, 2, 1); // turnaround rim only every 4th bar!

let r_ch = s.row(p_drop1, hat_src);
s.row_swing(p_drop1, r_ch, 0.62);
s.row_humanize(p_drop1, r_ch, 0.005);
s.row_set(p_drop1, r_ch, "position", 0.0);
s.row_set(p_drop1, r_ch, "duration", 0.11);
s.row_set(p_drop1, r_ch, "gain", 0.42);
s.row_set(p_drop1, r_ch, "pan_spread", 0.76);
s.row_set(p_drop1, r_ch, "envelope", 0.12);
s.row_set(p_drop1, r_ch, "env_skew", -0.78);
for st in [0, 2, 3, 6, 8, 11, 14, 16, 18, 22, 23, 26, 27, 30] {
    s.step(p_drop1, r_ch, st, true);
}
for ghost_st in [3, 11, 23, 27] {
    s.step_gain(p_drop1, r_ch, ghost_st, 0.50);
    s.step_pitch(p_drop1, r_ch, ghost_st, -2.0);
}

let r_oh = s.row(p_drop1, hat_src);
s.row_set(p_drop1, r_oh, "position", 0.25);
s.row_set(p_drop1, r_oh, "duration", 0.24);
s.row_set(p_drop1, r_oh, "gain", 0.36);
s.row_set(p_drop1, r_oh, "pan", 0.24);
s.row_set(p_drop1, r_oh, "pan_spread", 0.42);
s.row_set(p_drop1, r_oh, "envelope", 0.14);
s.row_set(p_drop1, r_oh, "env_skew", -0.72);
for st in [2, 10, 18, 26] {
    s.step(p_drop1, r_oh, st, true);
}

let r_wb = s.row(p_drop1, hat_src);
s.row_set(p_drop1, r_wb, "position", 0.75);
s.row_set(p_drop1, r_wb, "duration", 0.17);
s.row_set(p_drop1, r_wb, "gain", 0.34);
s.row_set(p_drop1, r_wb, "pan", -0.28);
s.row_set(p_drop1, r_wb, "pan_spread", 0.38);
s.row_set(p_drop1, r_wb, "env_skew", -0.80);
for st in [6, 15, 21, 29] {
    s.step(p_drop1, r_wb, st, true);
}
s.step_pitch(p_drop1, r_wb, 15, -3.0);
s.step_pitch(p_drop1, r_wb, 29, -5.0);

// Pattern C: Evolved Drop II groove
let p_drop2 = s.pattern("ukg_drop2_evolved");
s.pattern_grid(p_drop2, 8.0, 4);
s.pattern_swing(p_drop2, 0.61);

let r2_kick = s.row(p_drop2, kick_src);
s.row_set(p_drop2, r2_kick, "duration", 0.30);
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
s.row_set(p_drop2, r2_rim, "duration", 0.30);
s.row_set(p_drop2, r2_rim, "gain", 0.66);
s.row_set(p_drop2, r2_rim, "pan", 0.18);
s.row_set(p_drop2, r2_rim, "pan_spread", 0.30);
s.row_set(p_drop2, r2_rim, "envelope", 0.08);
s.row_set(p_drop2, r2_rim, "env_skew", -0.82);
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
s.step_gain(p_drop2, r2_rim, 30, 0.38); s.step_pitch(p_drop2, r2_rim, 30, -2.0); s.step_every(p_drop2, r2_rim, 30, 2, 1);
s.step_gain(p_drop2, r2_rim, 31, 0.44); s.step_every(p_drop2, r2_rim, 31, 2, 1);

let r2_ch = s.row(p_drop2, hat_src);
s.row_swing(p_drop2, r2_ch, 0.62);
s.row_humanize(p_drop2, r2_ch, 0.005);
s.row_set(p_drop2, r2_ch, "position", 0.0);
s.row_set(p_drop2, r2_ch, "duration", 0.11);
s.row_set(p_drop2, r2_ch, "gain", 0.44);
s.row_set(p_drop2, r2_ch, "pan_spread", 0.80);
s.row_set(p_drop2, r2_ch, "envelope", 0.12);
s.row_set(p_drop2, r2_ch, "env_skew", -0.78);
for st in [0, 1, 2, 3, 6, 8, 10, 11, 14, 16, 18, 21, 22, 23, 26, 27, 30] {
    s.step(p_drop2, r2_ch, st, true);
}
for gs in [1, 3, 11, 21, 23, 27] {
    s.step_gain(p_drop2, r2_ch, gs, 0.48);
    s.step_pitch(p_drop2, r2_ch, gs, -2.0);
}

let r2_oh = s.row(p_drop2, hat_src);
s.row_set(p_drop2, r2_oh, "position", 0.25);
s.row_set(p_drop2, r2_oh, "duration", 0.24);
s.row_set(p_drop2, r2_oh, "gain", 0.36);
s.row_set(p_drop2, r2_oh, "pan", -0.22);
s.row_set(p_drop2, r2_oh, "pan_spread", 0.44);
s.row_set(p_drop2, r2_oh, "envelope", 0.14);
s.row_set(p_drop2, r2_oh, "env_skew", -0.72);
for st in [2, 10, 18, 26] {
    s.step(p_drop2, r2_oh, st, true);
}

let r2_shk = s.row(p_drop2, hat_src);
s.row_steps(p_drop2, r2_shk, 7);
s.row_set(p_drop2, r2_shk, "position", 0.50);
s.row_set(p_drop2, r2_shk, "duration", 0.13);
s.row_set(p_drop2, r2_shk, "gain", 0.32);
s.row_set(p_drop2, r2_shk, "pan_spread", 0.85);
s.row_set(p_drop2, r2_shk, "env_skew", -0.65);
s.step(p_drop2, r2_shk, 0, true);
s.step(p_drop2, r2_shk, 3, true); s.step_gain(p_drop2, r2_shk, 3, 0.65);
s.step(p_drop2, r2_shk, 5, true); s.step_gain(p_drop2, r2_shk, 5, 0.55);

// Larger feathered bokeh & tunnel kick blooms (flicker=0.28 -> crisp 2-frame rise + 5-frame decay, zero strobe!)
let c_d_intro = s.pattern_clip(t_drums, p_intro, 16.0, 14.0);
s.set(c_d_intro, "additive", 0.82);
s.set(c_d_intro, "flicker", 0.24);
s.set(c_d_intro, "feather", 0.45);
s.set(c_d_intro, "size_scale", 1.35);
s.automate(c_d_intro, "gain", 0.0, 0.42);
s.automate(c_d_intro, "gain", 14.0, 0.88);

let c_d_drop1 = s.pattern_clip(t_drums, p_drop1, 32.0, 48.0);
s.set(c_d_drop1, "additive", 0.88);
s.set(c_d_drop1, "flicker", 0.28);
s.set(c_d_drop1, "feather", 0.45);
s.set(c_d_drop1, "size_scale", 1.45);
s.set(c_d_drop1, "min_size", 0.18);
s.set(c_d_drop1, "max_size", 0.52);

let c_d_riser = s.pattern_clip(t_drums, p_intro, 104.0, 6.0);
s.set(c_d_riser, "additive", 0.82);
s.set(c_d_riser, "flicker", 0.24);
s.set(c_d_riser, "feather", 0.45);
s.automate(c_d_riser, "gain", 0.0, 0.38);
s.automate(c_d_riser, "gain", 6.0, 0.90);

let c_d_drop2 = s.pattern_clip(t_drums, p_drop2, 112.0, 32.0);
s.set(c_d_drop2, "additive", 0.90);
s.set(c_d_drop2, "flicker", 0.28);
s.set(c_d_drop2, "feather", 0.45);
s.set(c_d_drop2, "size_scale", 1.50);
s.set(c_d_drop2, "min_size", 0.18);
s.set(c_d_drop2, "max_size", 0.55);

// ============================================================================
// TRACK 5: Dub Rimshot Throws (deeply feathered warm amber echoes, zero sharp box!)
// ============================================================================
let t_dub = s.track("dub_rim_throws");
s.track_blend(t_dub, 0.85);
s.track_rms_opacity(t_dub, 0.70);
for hit_b in [28.0, 44.0, 60.0, 76.0, 124.0, 140.0] {
    let th = s.snippet(t_dub, rim_src, hit_b, 0.70);
    s.set(th, "pitch", -3.0);
    s.set(th, "varispeed", 1.0);
    s.set(th, "gain", 0.34);
    s.set(th, "additive", 0.85);
    s.set(th, "feather", 0.65); // soft border feathering eliminates any rectangular tile edge!
    s.set(th, "shadow", 0.25);
    s.transform(th, -0.26, -0.14, 0.46, -2.0);
}
let dub_tint = s.track_effect(t_dub, "tint");
s.track_fx(t_dub, dub_tint, "hue", 30.0);
s.track_fx(t_dub, dub_tint, "amount", 0.48);
s.track_fx(t_dub, dub_tint, "audio", 0.0);

let dub_dl = s.track_effect(t_dub, "delay");
s.track_fx(t_dub, dub_dl, "time", 0.338);
s.track_fx(t_dub, dub_dl, "feedback", 0.56);
s.track_fx(t_dub, dub_dl, "mix", 0.60);
s.track_fx(t_dub, dub_dl, "shift_x", 0.075);
s.track_fx(t_dub, dub_dl, "shift_y", -0.015);

// ============================================================================
// TRACK 6: Reversed Resampled Vowel Swells (grounded nocturnal shadow ghosts)
// ============================================================================
let t_vghost = s.track("vocal_ghosts");
s.track_blend(t_vghost, 0.58);
s.track_rms_opacity(t_vghost, 0.65);
s.track_eq(t_vghost, 340.0, 1.3, -3.5);

for sw_b in [29.5, 61.5, 109.5, 141.5] {
    let rev = s.snippet(t_vghost, vbench_cut, sw_b, 1.8);
    s.key(rev, "F", "minor");
    s.set(rev, "quantize_amount", 1.0);
    s.set(rev, "position", 0.06);
    s.set(rev, "pitch", -5.0);
    s.set(rev, "varispeed", 1.0);
    s.set(rev, "reverse_prob", 1.0);
    s.set(rev, "envelope", 0.85);
    s.set(rev, "env_skew", 0.82);
    s.set(rev, "gain", 0.34);
    s.set(rev, "additive", 0.55);
    s.set(rev, "feather", 0.42);
    s.set(rev, "shadow", 0.55);
    s.audio_filter(rev, "lowpass", 3600.0, 0.70);
    s.audio_filter(rev, "highpass", 240.0, 0.70);
    s.transform(rev, 0.20, 0.22, 0.92, 0.0);
}

let vg_verb = s.track_effect(t_vghost, "reverb");
s.track_fx(t_vghost, vg_verb, "size", 0.84);
s.track_fx(t_vghost, vg_verb, "damp", 0.46);
s.track_fx(t_vghost, vg_verb, "mix", 0.50);

// ============================================================================
// TRACK 7: Sparse Vinyl-Resampled Vocal Fragments & Grounded Shadow Silhouettes
//          - ZERO 0.0 st raw AI stem exposure (always shifted -2/-4/-5/+3 st +
//            warm sampler bandpass 240-3900 Hz + tape saturation)
//          - ~55% fewer triggers via `step_every`, leaving generous 2-bar pockets!
//          - Grounded shadow-crushed human cutouts (`shadow = 0.52`, `y = +0.22`)
// ============================================================================
let t_voc = s.track("vocal_resampled_fragments");
s.track_blend(t_voc, 0.36);
s.track_rms_opacity(t_voc, 0.78);
s.track_tape(t_voc, 0.12, 0.26, 0.14, 0.0); // authentic warm vinyl-dubplate resampled character!
s.track_eq(t_voc, 340.0, 1.35, -3.5);       // zero 340 Hz boxiness!

let voc_tint = s.track_effect(t_voc, "tint");
s.track_fx(t_voc, voc_tint, "hue", 32.0);   // nocturnal sodium rim-light on shadow silhouettes
s.track_fx(t_voc, voc_tint, "amount", 0.38);
s.track_fx(t_voc, voc_tint, "audio", 0.0);

let b_ooh  = 0.03;
let b_turn = 0.14;
let b_sigh = 0.35;
let m_hoo  = 0.03;
let m_oh   = 0.35;
let m_air  = 0.69;

// --- ACT I (INTRO): Two brief, elusive resampled sighs (-2 st & -5 st, <0.85s) ---
let vi_1 = s.snippet(t_voc, vbench_cut, 8.0, 1.6);
s.key(vi_1, "F", "minor");
s.set(vi_1, "quantize_amount", 1.0);
s.set(vi_1, "position", b_ooh);
s.set(vi_1, "pitch", -2.0);
s.set(vi_1, "varispeed", 1.0);
s.set(vi_1, "gain", 0.52);
s.set(vi_1, "envelope", 0.38);
s.set(vi_1, "feather", 0.30);
s.set(vi_1, "shadow", 0.52);
s.audio_filter(vi_1, "highpass", 240.0, 0.70);
s.audio_filter(vi_1, "lowpass", 3900.0, 0.72);
s.transform(vi_1, -0.18, 0.24, 0.96, 0.0);

let vi_2 = s.snippet(t_voc, vstreet_cut, 21.0, 1.7);
s.key(vi_2, "F", "minor");
s.set(vi_2, "quantize_amount", 1.0);
s.set(vi_2, "position", m_hoo);
s.set(vi_2, "pitch", -4.0);
s.set(vi_2, "varispeed", 1.0);
s.set(vi_2, "gain", 0.50);
s.set(vi_2, "envelope", 0.40);
s.set(vi_2, "feather", 0.30);
s.set(vi_2, "shadow", 0.50);
s.audio_filter(vi_2, "highpass", 230.0, 0.70);
s.audio_filter(vi_2, "lowpass", 3800.0, 0.72);
s.transform(vi_2, 0.20, 0.22, 1.02, 0.0);

// --- ACT II (DROP I, 32..80): Sparse Pass-Conditional 8-Bar Call & Response! ---
// Only 3-4 concise chops per 16-beat (4-bar) pass, alternating Pass 0 vs Pass 1 via
// `step_every(..., 2, 0/1)` so Beat 1 NEVER loops mechanically and whole 2-bar pockets breathe!
let p_voc1 = s.pattern("drop1_sparse_apparitions");
s.pattern_grid(p_voc1, 16.0, 4); // 64 steps = 4 bars
s.pattern_swing(p_voc1, 0.60);

let rv_fem = s.row(p_voc1, vbench_cut);
s.row_key(p_voc1, rv_fem, "F", "minor");
s.row_set(p_voc1, rv_fem, "quantize_amount", 1.0);
s.row_set(p_voc1, rv_fem, "duration", 0.44);
s.row_set(p_voc1, rv_fem, "gain", 0.60);
s.row_set(p_voc1, rv_fem, "envelope", 0.32);
s.row_set(p_voc1, rv_fem, "pan", -0.14);
s.row_set(p_voc1, rv_fem, "pan_spread", 0.20);

// Pass 0 (bars 1-4): syncopated entry on step 3 (+3 st -> -2 st), leaving bars 3-4 open!
s.step(p_voc1, rv_fem, 3, true);  s.step_slice(p_voc1, rv_fem, 3, b_ooh);  s.step_pitch(p_voc1, rv_fem, 3, 3.0);  s.step_every(p_voc1, rv_fem, 3, 2, 0);
s.step(p_voc1, rv_fem, 11, true); s.step_slice(p_voc1, rv_fem, 11, b_sigh);s.step_pitch(p_voc1, rv_fem, 11, -2.0); s.step_length(p_voc1, rv_fem, 11, 1.15); s.step_every(p_voc1, rv_fem, 11, 2, 0);

// Pass 1 (bars 5-8): bar 5 stays completely instrumental! Vocal enters late on bar 7 (step 35, -2 -> -5 st)
s.step(p_voc1, rv_fem, 35, true); s.step_slice(p_voc1, rv_fem, 35, b_turn);s.step_pitch(p_voc1, rv_fem, 35, -2.0); s.step_every(p_voc1, rv_fem, 35, 2, 1);
s.step(p_voc1, rv_fem, 43, true); s.step_slice(p_voc1, rv_fem, 43, b_sigh);s.step_pitch(p_voc1, rv_fem, 43, -5.0); s.step_length(p_voc1, rv_fem, 43, 1.20); s.step_every(p_voc1, rv_fem, 43, 2, 1);

let rv_mal = s.row(p_voc1, vstreet_cut);
s.row_key(p_voc1, rv_mal, "F", "minor");
s.row_set(p_voc1, rv_mal, "quantize_amount", 1.0);
s.row_set(p_voc1, rv_mal, "duration", 0.42);
s.row_set(p_voc1, rv_mal, "gain", 0.56);
s.row_set(p_voc1, rv_mal, "envelope", 0.34);
s.row_set(p_voc1, rv_mal, "pan", 0.18);
s.row_set(p_voc1, rv_mal, "pan_spread", 0.20);

// Sparse male answer only once per 4-bar pass!
s.step(p_voc1, rv_mal, 26, true); s.step_slice(p_voc1, rv_mal, 26, m_hoo); s.step_pitch(p_voc1, rv_mal, 26, -2.0); s.step_length(p_voc1, rv_mal, 26, 1.15); s.step_every(p_voc1, rv_mal, 26, 2, 0);
s.step(p_voc1, rv_mal, 58, true); s.step_slice(p_voc1, rv_mal, 58, m_oh);  s.step_pitch(p_voc1, rv_mal, 58, -4.0); s.step_length(p_voc1, rv_mal, 58, 1.10); s.step_every(p_voc1, rv_mal, 58, 2, 1);

let c_voc_d1 = s.pattern_clip(t_voc, p_voc1, 32.0, 48.0);
s.set(c_voc_d1, "additive", 0.38);
s.set(c_voc_d1, "flicker", 0.22);
s.set(c_voc_d1, "feather", 0.28);
s.set(c_voc_d1, "shadow", 0.52);
s.set(c_voc_d1, "scatter_y", 0.12);
s.set(c_voc_d1, "min_size", 0.48);
s.set(c_voc_d1, "max_size", 0.74);
s.transform(c_voc_d1, 0.0, 0.22, 1.0, 0.0); // ground silhouettes at bottom of canvas!
s.audio_filter(c_voc_d1, "highpass", 240.0, 0.70);
s.audio_filter(c_voc_d1, "lowpass", 3900.0, 0.72);

// --- ACT III (BREAKDOWN, 80..110): Concise resampled dubplate fragments (<1.1s, pitched
//     -2/-4/+3 st, never raw 0 st!), letting dub echo & reverb bloom across bare water ---
let vb_1 = s.snippet(t_voc, vbench_cut, 82.5, 2.2); // ~0.99s resampled fragment at -2 st!
s.key(vb_1, "F", "minor");
s.set(vb_1, "quantize_amount", 1.0);
s.set(vb_1, "position", b_ooh);
s.set(vb_1, "pitch", -2.0);
s.set(vb_1, "varispeed", 1.0);
s.set(vb_1, "gain", 0.60);
s.set(vb_1, "envelope", 0.34);
s.set(vb_1, "feather", 0.30);
s.set(vb_1, "shadow", 0.50);
s.audio_filter(vb_1, "highpass", 240.0, 0.70);
s.audio_filter(vb_1, "lowpass", 3800.0, 0.72);
s.transform(vb_1, -0.14, 0.24, 1.16, 0.0);

let vb_2 = s.snippet(t_voc, vstreet_cut, 91.0, 2.1); // ~0.94s falsetto answer at -4 st!
s.key(vb_2, "F", "minor");
s.set(vb_2, "quantize_amount", 1.0);
s.set(vb_2, "position", m_hoo);
s.set(vb_2, "pitch", -4.0);
s.set(vb_2, "varispeed", 1.0);
s.set(vb_2, "gain", 0.56);
s.set(vb_2, "envelope", 0.36);
s.set(vb_2, "feather", 0.30);
s.set(vb_2, "shadow", 0.50);
s.audio_filter(vb_2, "highpass", 230.0, 0.70);
s.audio_filter(vb_2, "lowpass", 3700.0, 0.72);
s.transform(vb_2, 0.16, 0.22, 1.18, 0.0);

let vb_3 = s.snippet(t_voc, vbench_cut, 99.5, 2.8); // ~1.25s granular sigh dissolving at -5 st!
s.key(vb_3, "F", "minor");
s.set(vb_3, "quantize_amount", 1.0);
s.set(vb_3, "position", b_sigh);
s.set(vb_3, "pitch", -5.0);
s.set(vb_3, "speed", 0.82);
s.set(vb_3, "texture", 0.28);
s.set(vb_3, "density", 13.0);
s.set(vb_3, "duration", 0.28);
s.set(vb_3, "gain", 0.54);
s.set(vb_3, "feather", 0.34);
s.set(vb_3, "shadow", 0.52);
s.set(vb_3, "flicker", 0.18);
s.audio_filter(vb_3, "highpass", 240.0, 0.70);
s.audio_filter(vb_3, "lowpass", 3600.0, 0.72);
s.transform(vb_3, 0.0, 0.24, 1.20, 0.0);

// --- ACT IV (DROP II, 112..144): Restrained syncopated stutter interlocked with rail rhythm ---
let p_voc2 = s.pattern("drop2_restrained_stutter");
s.pattern_grid(p_voc2, 16.0, 4);
s.pattern_swing(p_voc2, 0.61);

let rv2_a = s.row(p_voc2, vbench_cut);
s.row_key(p_voc2, rv2_a, "F", "minor");
s.row_set(p_voc2, rv2_a, "quantize_amount", 1.0);
s.row_set(p_voc2, rv2_a, "duration", 0.38);
s.row_set(p_voc2, rv2_a, "gain", 0.62);
s.row_set(p_voc2, rv2_a, "envelope", 0.28);
s.row_set(p_voc2, rv2_a, "pan", -0.16);
s.row_set(p_voc2, rv2_a, "pan_spread", 0.24);

// Pass 0: syncopated 2-hit stutter on beats 1.75 & 3.75 (+3 -> -2 st), then silence for 2 bars!
s.step(p_voc2, rv2_a, 3, true);  s.step_slice(p_voc2, rv2_a, 3, b_ooh);  s.step_pitch(p_voc2, rv2_a, 3, 3.0);  s.step_length(p_voc2, rv2_a, 3, 0.75); s.step_every(p_voc2, rv2_a, 3, 2, 0);
s.step(p_voc2, rv2_a, 6, true);  s.step_slice(p_voc2, rv2_a, 6, b_turn); s.step_pitch(p_voc2, rv2_a, 6, -2.0); s.step_every(p_voc2, rv2_a, 6, 2, 0);
s.step(p_voc2, rv2_a, 43, true); s.step_slice(p_voc2, rv2_a, 43, b_sigh);s.step_pitch(p_voc2, rv2_a, 43, -4.0); s.step_length(p_voc2, rv2_a, 43, 1.25); s.step_every(p_voc2, rv2_a, 43, 2, 0);

// Pass 1: let subterranean rail rhythm & 2-step drums roll solo for bars 5-6, answer on bar 7!
s.step(p_voc2, rv2_a, 34, true); s.step_slice(p_voc2, rv2_a, 34, b_ooh); s.step_pitch(p_voc2, rv2_a, 34, -2.0); s.step_length(p_voc2, rv2_a, 34, 0.80); s.step_every(p_voc2, rv2_a, 34, 2, 1);
s.step(p_voc2, rv2_a, 42, true); s.step_slice(p_voc2, rv2_a, 42, b_sigh);s.step_pitch(p_voc2, rv2_a, 42, -5.0); s.step_length(p_voc2, rv2_a, 42, 1.30); s.step_every(p_voc2, rv2_a, 42, 2, 1);

let rv2_b = s.row(p_voc2, vstreet_cut);
s.row_key(p_voc2, rv2_b, "F", "minor");
s.row_set(p_voc2, rv2_b, "quantize_amount", 1.0);
s.row_set(p_voc2, rv2_b, "duration", 0.42);
s.row_set(p_voc2, rv2_b, "gain", 0.56);
s.row_set(p_voc2, rv2_b, "envelope", 0.30);
s.row_set(p_voc2, rv2_b, "pan", 0.18);
s.row_set(p_voc2, rv2_b, "pan_spread", 0.24);

s.step(p_voc2, rv2_b, 22, true); s.step_slice(p_voc2, rv2_b, 22, m_oh);  s.step_pitch(p_voc2, rv2_b, 22, -4.0); s.step_every(p_voc2, rv2_b, 22, 2, 0);
s.step(p_voc2, rv2_b, 58, true); s.step_slice(p_voc2, rv2_b, 58, m_air); s.step_pitch(p_voc2, rv2_b, 58, -5.0); s.step_length(p_voc2, rv2_b, 58, 1.20); s.step_every(p_voc2, rv2_b, 58, 2, 1);

let c_voc_d2 = s.pattern_clip(t_voc, p_voc2, 112.0, 32.0);
s.set(c_voc_d2, "additive", 0.42);
s.set(c_voc_d2, "flicker", 0.22);
s.set(c_voc_d2, "feather", 0.28);
s.set(c_voc_d2, "shadow", 0.52);
s.set(c_voc_d2, "scatter_y", 0.10);
s.set(c_voc_d2, "min_size", 0.48);
s.set(c_voc_d2, "max_size", 0.76);
s.transform(c_voc_d2, 0.0, 0.22, 1.0, 0.0);
s.audio_filter(c_voc_d2, "highpass", 240.0, 0.70);
s.audio_filter(c_voc_d2, "lowpass", 3900.0, 0.72);

let d2_tile = s.effect(c_voc_d2, "tile");
s.fx(c_voc_d2, d2_tile, "count", 2.0);
s.fx(c_voc_d2, d2_tile, "mirror", 1.0);
s.fx(c_voc_d2, d2_tile, "audio", 0.0);
s.fx(c_voc_d2, d2_tile, "video", 0.45);

// --- ACT V (OUTRO, 144..160): Single concise resampled sigh fading into rain & vinyl dust ---
let v_out = s.snippet(t_voc, vbench_cut, 146.5, 2.4);
s.key(v_out, "F", "minor");
s.set(v_out, "quantize_amount", 1.0);
s.set(v_out, "position", b_sigh);
s.set(v_out, "pitch", -4.0);
s.set(v_out, "varispeed", 1.0);
s.set(v_out, "gain", 0.48);
s.set(v_out, "envelope", 0.45);
s.set(v_out, "feather", 0.34);
s.set(v_out, "shadow", 0.55);
s.audio_filter(v_out, "highpass", 240.0, 0.70);
s.audio_filter(v_out, "lowpass", 3600.0, 0.72);
s.transform(v_out, 0.0, 0.24, 1.08, 0.0);

let voc_dl = s.track_effect(t_voc, "delay");
s.track_fx(t_voc, voc_dl, "time", 0.338);
s.track_fx(t_voc, voc_dl, "feedback", 0.46);
s.track_fx(t_voc, voc_dl, "mix", 0.34);
s.track_fx(t_voc, voc_dl, "shift_x", 0.045);
s.track_fx(t_voc, voc_dl, "shift_y", -0.01);

let voc_rv = s.track_effect(t_voc, "reverb");
s.track_fx(t_voc, voc_rv, "size", 0.76);
s.track_fx(t_voc, voc_rv, "damp", 0.46);
s.track_fx(t_voc, voc_rv, "mix", 0.34);

// ============================================================================
// MASTER BUS: Crisp post-LP vinyl dust, subtle 16mm gate weave & DCT film grit
// ============================================================================
s.master_tape(0.10, 0.16, 0.04, 0.16);

let m_comp = s.master_effect("compress");
s.master_fx(m_comp, "quality", 0.78);
s.master_fx(m_comp, "audio", 0.03);
s.master_fx(m_comp, "video", 0.34);

let m_verb = s.master_effect("reverb");
s.master_fx(m_verb, "size", 0.56);
s.master_fx(m_verb, "damp", 0.46);
s.master_fx(m_verb, "mix", 0.09);

s.render(0.0, 160.0, "outputs/261111_burial-demo/v004/261111_chromagrain_burial-demo_v004.mp4");
