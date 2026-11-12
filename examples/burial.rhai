// examples/burial.rhai — Nocturnal Subterranean 2-Step AV Piece ("Night Platform")
// Run headless:
//   target/release/chromagrain --script examples/burial.rhai
//
// Key: F minor | Tempo: 133 BPM (40 bars = 160 beats = 72.2 s)
// Zero lyric sentences, zero TTS speech — built entirely from public-domain NYC
// subway & night-highway archival footage, open-air wordless human vowel/breath
// chops, 60%-swung 2-step percussion, 133 BPM subterranean rail-joint rhythm,
// low-band sidechained F1 Reese bass, critical-band paulstretch washes, and AV tape.
//
// 5-Act Audiovisual Montage:
//   0..32   (0:00-0:14.4) Act I - Intro: rain-streaked sodium glass, crisp vinyl dust,
//                         velvet Fm9 paulstretch wash, sparse open-vowel call; 30.5-32 hush
//   32..80  (0:14.4-0:36.1) Act II - Drop I: rain recedes; wet night highway headlights,
//                         swung UKG pocket + brushed 12 kHz hats, recurring 4-note open
//                         'Ooooh / Hooo' vocal motif across segmented commuter silhouettes
//   80..112 (0:36.1-0:50.5) Act III - Breakdown: drums & sub drop out; nocturnal water
//                         reflections & large intimate cut-out figures laid bare in space
//   112..144(0:50.5-1:05.0) Act IV - Drop II (Subterranean Climax): forward-rushing 1941
//                         NYC dark subway tunnel + bogie rail-joint 'da-CLACK' interlock,
//                         stuttered vowel hook & mirrored dual silhouettes
//   144..160(1:05.0-1:12.2) Act V - Outro: tunnel dissolves back to night rain, vinyl
//                         crackle, 16mm dust, and a single open sigh fading into shadow

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

s.set_base_hz(sub_src,    87.307);  // F2 (audio carries tight F1 43.65 Hz + warm F2 Reese)
s.set_base_hz(chords_src, 174.614); // F3

// Derive 6x paulstretched AV chord wash (85-cent critical-band smear: silky air, zero whistle)
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

// ============================================================================
// TRACK 0: Rain & Vinyl Crackle Bed (1 unbroken crossfade-looping snippet,
//          receding in Drops & Breakdown so foreground scenes take over!)
// ============================================================================
let t_rain = s.track("rain_vinyl_bed");
s.track_opacity_link(t_rain, 0.55);

// Dynamic scene-contrast envelope (never drops to 0.0 mid-piece -> zero blackouts!)
s.track_level_point(t_rain, 0.0, 0.0);
s.track_level_point(t_rain, 4.5, 0.90);   // Act I: lush rain window & crisp vinyl crackle
s.track_level_point(t_rain, 29.5, 0.86);
s.track_level_point(t_rain, 31.5, 0.48);  // subtle dim during pre-drop breath (stays visible!)
s.track_level_point(t_rain, 33.0, 0.36);  // Act II: rain recedes behind wet highway & commuters
s.track_level_point(t_rain, 79.0, 0.36);
s.track_level_point(t_rain, 82.0, 0.28);  // Act III: deep dark space behind bare human cutouts
s.track_level_point(t_rain, 110.5, 0.34);
s.track_level_point(t_rain, 113.0, 0.22); // Act IV: subterranean tunnel takes over!
s.track_level_point(t_rain, 143.0, 0.24);
s.track_level_point(t_rain, 146.0, 0.88); // Act V: return to rainy night glass & vinyl dust
s.track_level_point(t_rain, 152.0, 0.82);
s.track_level_point(t_rain, 160.0, 0.0);

let rain_seg = s.snippet(t_rain, rain_src, 0.0, 160.0);
s.set(rain_seg, "gain", 0.28);
s.set(rain_seg, "gain_to_opacity", 0.25);
s.set(rain_seg, "speed", 0.72);
s.set(rain_seg, "envelope", 0.04);

let rain_tint = s.track_effect(t_rain, "tint");
s.track_fx(t_rain, rain_tint, "hue", 28.0);     // warm sodium streetlight amber
s.track_fx(t_rain, rain_tint, "amount", 0.26);
s.track_fx(t_rain, rain_tint, "audio", 0.0);

// ============================================================================
// TRACK 1: Nocturnal Water Reflections & Paulstretched Fm9 Chords
// ============================================================================
let t_pad = s.track("paulstretch_pad");
s.track_blend(t_pad, 0.58);
s.track_opacity_link(t_pad, 0.55);
s.track_tape(t_pad, 0.22, 0.18, 0.10, 0.0);

s.track_level_point(t_pad, 0.0, 0.0);
s.track_level_point(t_pad, 9.0, 0.76);
s.track_level_point(t_pad, 29.5, 0.76);
s.track_level_point(t_pad, 31.3, 0.16); // pre-drop breath!
s.track_level_point(t_pad, 32.5, 0.62);
s.track_level_point(t_pad, 80.0, 0.88); // Breakdown: glowing water reflections swell up!
s.track_level_point(t_pad, 109.5, 0.84);
s.track_level_point(t_pad, 111.3, 0.14); // second pre-drop hush before Drop II!
s.track_level_point(t_pad, 112.5, 0.64);
s.track_level_point(t_pad, 144.0, 0.58);
s.track_level_point(t_pad, 160.0, 0.0);

let pad_intro = s.clip(t_pad, chord_wash, 1.0, 79.0);
s.key(pad_intro, "F", "minor");
s.set(pad_intro, "quantize_amount", 0.75);
s.set(pad_intro, "detune_cents", -8.0);
s.set(pad_intro, "density", 7.5);
s.set(pad_intro, "duration", 0.72);
s.set(pad_intro, "spray", 2.0);
s.set(pad_intro, "scan_speed", 0.42);
s.set(pad_intro, "gain", 0.30);
s.set(pad_intro, "additive", 0.55);
s.set(pad_intro, "feather", 0.38);
s.set(pad_intro, "flicker", 0.10);
s.set(pad_intro, "size_scale", 1.75);
s.audio_filter(pad_intro, "lowpass", 3800.0, 0.70);
s.audio_filter(pad_intro, "highpass", 210.0, 0.70); // clean pocket for sub & kick!

// Breakdown: lush nocturnal water reflection bed + lower-octave warmth
let pad_brk = s.clip(t_pad, chord_wash, 80.0, 31.5);
s.key(pad_brk, "F", "minor");
s.set(pad_brk, "quantize_amount", 0.80);
s.set(pad_brk, "detune_cents", 7.0);
s.set(pad_brk, "density", 9.0);
s.set(pad_brk, "duration", 0.82);
s.set(pad_brk, "position", 0.25);
s.set(pad_brk, "spray", 2.4);
s.set(pad_brk, "scan_speed", 0.34);
s.set(pad_brk, "gain", 0.34);
s.set(pad_brk, "voices", 2.0);
s.set(pad_brk, "voice_interval", -12.0);
s.set(pad_brk, "voice_detune", 0.08);
s.set(pad_brk, "additive", 0.60);
s.set(pad_brk, "feather", 0.42);
s.set(pad_brk, "flicker", 0.10);
s.set(pad_brk, "size_scale", 1.95);
s.audio_filter(pad_brk, "lowpass", 4200.0, 0.70);
s.audio_filter(pad_brk, "highpass", 185.0, 0.70);

let pad_re = s.clip(t_pad, chord_wash, 112.0, 46.0);
s.key(pad_re, "F", "minor");
s.set(pad_re, "detune_cents", -6.0);
s.set(pad_re, "density", 8.0);
s.set(pad_re, "duration", 0.68);
s.set(pad_re, "position", 0.48);
s.set(pad_re, "spray", 1.8);
s.set(pad_re, "gain", 0.28);
s.set(pad_re, "additive", 0.52);
s.set(pad_re, "feather", 0.38);
s.set(pad_re, "flicker", 0.10);
s.set(pad_re, "size_scale", 1.65);
s.audio_filter(pad_re, "lowpass", 3800.0, 0.70);
s.audio_filter(pad_re, "highpass", 215.0, 0.70);

let pad_verb = s.track_effect(t_pad, "reverb");
s.track_fx(t_pad, pad_verb, "size", 0.80);
s.track_fx(t_pad, pad_verb, "damp", 0.52);
s.track_fx(t_pad, pad_verb, "mix", 0.36);

// ============================================================================
// TRACK 2: Subterranean 1941 NYC Subway Tunnel & Bogie Rail-Joint Groove
//          (Foreshadowed in Intro, full rushing tunnel world in Drop II!)
// ============================================================================
let t_tunnel = s.track("subway_tunnel_rhythm");
s.track_blend(t_tunnel, 0.72);
s.track_opacity_link(t_tunnel, 0.65);

// Brief subterranean tunnel glimpse before Drop I (bars 5..7)
let tun_pre = s.snippet(t_tunnel, tunnel_src, 16.0, 12.0);
s.set(tun_pre, "gain", 0.20);
s.set(tun_pre, "gain_to_opacity", 0.30);
s.set(tun_pre, "speed", 1.0);
s.set(tun_pre, "envelope", 0.22);
s.set(tun_pre, "feather", 0.30);
s.audio_filter(tun_pre, "highpass", 260.0, 0.70);

// Act IV (Drop II, 112..144): Full forward-rushing dark NYC subway tunnel + 133 BPM rail clatter!
let tun_d2 = s.snippet(t_tunnel, tunnel_src, 112.0, 32.0);
s.set(tun_d2, "gain", 0.34);
s.set(tun_d2, "gain_to_opacity", 0.45);
s.set(tun_d2, "speed", 1.0);
s.set(tun_d2, "envelope", 0.10);
s.set(tun_d2, "additive", 0.68);
s.set(tun_d2, "feather", 0.22);
s.transform(tun_d2, 0.0, 0.0, 1.16, 0.0);
s.audio_filter(tun_d2, "highpass", 240.0, 0.70);

let tun_tint = s.track_effect(t_tunnel, "tint");
s.track_fx(t_tunnel, tun_tint, "hue", 34.0); // warm subterranean tungsten lamp glow
s.track_fx(t_tunnel, tun_tint, "amount", 0.32);
s.track_fx(t_tunnel, tun_tint, "audio", 0.0);

// ============================================================================
// TRACK 3: Tight Varispeed Sub / Reese Bass (controlled -4.2 dB pocket +
//          prominent wet-highway headlight glide in Drop I & II)
// ============================================================================
let t_sub = s.track("sub_bass");
s.track_blend(t_sub, 0.82);
s.track_opacity_link(t_sub, 0.65);

// 4-bar F-minor bass movement (varispeed = 1 -> single pristine Hermite grain):
//   Beats 0..6:   F  (0 st)
//   Beats 6..8:   Eb (-2 st) or Ab (+3 st in Drop II)
//   Beats 8..13:  Db (-4 st)
//   Beats 13..16: C  (-5 st)
for bar_start in [32.0, 48.0, 64.0, 112.0, 128.0] {
    let n1 = s.snippet(t_sub, sub_src, bar_start + 0.0, 5.6);
    s.set(n1, "pitch", 0.0);
    s.set(n1, "varispeed", 1.0);
    s.set(n1, "gain", 0.39);
    s.set(n1, "gain_to_opacity", 0.45);
    s.set(n1, "envelope", 0.16);
    s.set(n1, "additive", 0.82);
    s.set(n1, "feather", 0.28);
    s.transform(n1, 0.0, 0.22, 1.22, 0.0);

    let step2_st = if bar_start >= 112.0 { 3.0 } else { -2.0 };
    let n2 = s.snippet(t_sub, sub_src, bar_start + 6.0, 1.8);
    s.set(n2, "pitch", step2_st);
    s.set(n2, "varispeed", 1.0);
    s.set(n2, "gain", 0.36);
    s.set(n2, "gain_to_opacity", 0.45);
    s.set(n2, "envelope", 0.20);
    s.set(n2, "additive", 0.82);
    s.set(n2, "feather", 0.28);
    s.transform(n2, -0.06, 0.22, 1.18, 0.0);

    let n3 = s.snippet(t_sub, sub_src, bar_start + 8.0, 4.65);
    s.set(n3, "pitch", -4.0);
    s.set(n3, "varispeed", 1.0);
    s.set(n3, "gain", 0.40);
    s.set(n3, "gain_to_opacity", 0.48);
    s.set(n3, "envelope", 0.16);
    s.set(n3, "additive", 0.82);
    s.set(n3, "feather", 0.28);
    s.transform(n3, 0.05, 0.24, 1.24, 0.0);

    let turn_st = if bar_start == 48.0 { -2.0 } else { -5.0 };
    let n4 = s.snippet(t_sub, sub_src, bar_start + 13.0, 2.65);
    s.set(n4, "pitch", turn_st);
    s.set(n4, "varispeed", 1.0);
    s.set(n4, "gain", 0.37);
    s.set(n4, "gain_to_opacity", 0.45);
    s.set(n4, "envelope", 0.20);
    s.set(n4, "additive", 0.82);
    s.set(n4, "feather", 0.28);
    s.transform(n4, 0.0, 0.23, 1.20, 0.0);
}

// Heavy visual motion-blur persistence on wet highway headlights; audio stays dry & tight
let sub_smear = s.track_effect(t_sub, "reverb");
s.track_fx(t_sub, sub_smear, "size", 0.86);
s.track_fx(t_sub, sub_smear, "damp", 0.68);
s.track_fx(t_sub, sub_smear, "mix", 0.58);
s.track_fx(t_sub, sub_smear, "audio", 0.0);
s.track_fx(t_sub, sub_smear, "video", 1.0);

// ============================================================================
// TRACK 4: Swung 2-Step UK Garage Drums (Crisp 12 kHz Hats + Low-Band Sidechain)
// ============================================================================
let t_drums = s.track("ukg_2step_drums");
s.track_blend(t_drums, 0.88);
s.track_tape(t_drums, 0.04, 0.18, 0.04, 0.0); // gentle age=0.04 preserves crisp 12 kHz hat sizzle!

// Low-band sidechain (<155 Hz): ONLY kicks pump sub, pad & tunnel; crisp hi-hats never trigger pump!
s.track_duck(t_sub, t_drums, 0.74, 0.004, 0.18, 155.0);
s.track_duck_opacity(t_sub, 0.25);
s.track_duck(t_pad, t_drums, 0.44, 0.008, 0.22, 155.0);
s.track_duck_opacity(t_pad, 0.20);
s.track_duck(t_tunnel, t_drums, 0.38, 0.006, 0.16, 155.0);

// Warm amber tint on drum visuals so rim_train silver flashes glow warm sodium instead of cold grey!
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
s.row_set(p_intro, ig_rim, "envelope", 0.12);
s.row_set(p_intro, ig_rim, "env_skew", -0.82);
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

// Pattern B: Drop I 2-step UKG groove (8 beats = 32 steps, 60% shuffle + laid-back rim)
let p_drop1 = s.pattern("ukg_drop1");
s.pattern_grid(p_drop1, 8.0, 4);
s.pattern_swing(p_drop1, 0.60);

let r_kick = s.row(p_drop1, kick_src);
s.row_set(p_drop1, r_kick, "duration", 0.30);
s.row_set(p_drop1, r_kick, "gain", 0.72);
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
s.row_set(p_drop1, r_rim, "gain", 0.64);
s.row_set(p_drop1, r_rim, "pan", -0.18);
s.row_set(p_drop1, r_rim, "pan_spread", 0.28);
s.row_set(p_drop1, r_rim, "envelope", 0.08);
s.row_set(p_drop1, r_rim, "env_skew", -0.85);
s.row_humanize(p_drop1, r_rim, 0.004);
for st in [4, 7, 12, 15, 20, 28, 31] {
    s.step(p_drop1, r_rim, st, true);
}
for backbeat in [4, 12, 20, 28] {
    s.step_offset(p_drop1, r_rim, backbeat, 0.05);
}
s.step_gain(p_drop1, r_rim, 7, 0.30);  s.step_pitch(p_drop1, r_rim, 7, -2.0);
s.step_gain(p_drop1, r_rim, 15, 0.25); s.step_pitch(p_drop1, r_rim, 15, -1.0);
s.step_gain(p_drop1, r_rim, 31, 0.34); s.step_chance(p_drop1, r_rim, 31, 0.75);

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

// Pattern C: Evolved Drop II groove (syncopated rim stutter + 7-step polymeter shaker + crisp open hats)
let p_drop2 = s.pattern("ukg_drop2_evolved");
s.pattern_grid(p_drop2, 8.0, 4);
s.pattern_swing(p_drop2, 0.61);

let r2_kick = s.row(p_drop2, kick_src);
s.row_set(p_drop2, r2_kick, "duration", 0.30);
s.row_set(p_drop2, r2_kick, "gain", 0.74);
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
s.row_steps(p_drop2, r2_shk, 7); // 7-step polymeter shaker drifting across bars!
s.row_set(p_drop2, r2_shk, "position", 0.50);
s.row_set(p_drop2, r2_shk, "duration", 0.13);
s.row_set(p_drop2, r2_shk, "gain", 0.32);
s.row_set(p_drop2, r2_shk, "pan_spread", 0.85);
s.row_set(p_drop2, r2_shk, "env_skew", -0.65);
s.step(p_drop2, r2_shk, 0, true);
s.step(p_drop2, r2_shk, 3, true); s.step_gain(p_drop2, r2_shk, 3, 0.65);
s.step(p_drop2, r2_shk, 5, true); s.step_gain(p_drop2, r2_shk, 5, 0.55);

// Place drum clips (feathered edges = 0.32 + calm flicker = 0.18 -> glowing bokeh, zero harsh box!)
let c_d_intro = s.pattern_clip(t_drums, p_intro, 16.0, 14.0);
s.set(c_d_intro, "additive", 0.78);
s.set(c_d_intro, "flicker", 0.16);
s.set(c_d_intro, "feather", 0.34);
s.set(c_d_intro, "size_scale", 1.15);
s.automate(c_d_intro, "gain", 0.0, 0.42);
s.automate(c_d_intro, "gain", 14.0, 0.88);

let c_d_drop1 = s.pattern_clip(t_drums, p_drop1, 32.0, 48.0);
s.set(c_d_drop1, "additive", 0.82);
s.set(c_d_drop1, "flicker", 0.18);
s.set(c_d_drop1, "feather", 0.32);
s.set(c_d_drop1, "size_scale", 1.08);
s.set(c_d_drop1, "min_size", 0.08);
s.set(c_d_drop1, "max_size", 0.34);

let c_d_riser = s.pattern_clip(t_drums, p_intro, 104.0, 6.0);
s.set(c_d_riser, "additive", 0.78);
s.set(c_d_riser, "flicker", 0.16);
s.set(c_d_riser, "feather", 0.34);
s.automate(c_d_riser, "gain", 0.0, 0.38);
s.automate(c_d_riser, "gain", 6.0, 0.90);

let c_d_drop2 = s.pattern_clip(t_drums, p_drop2, 112.0, 32.0);
s.set(c_d_drop2, "additive", 0.85);
s.set(c_d_drop2, "flicker", 0.18);
s.set(c_d_drop2, "feather", 0.32);
s.set(c_d_drop2, "size_scale", 1.12);
s.set(c_d_drop2, "min_size", 0.08);
s.set(c_d_drop2, "max_size", 0.36);

// ============================================================================
// TRACK 5: Dub Rimshot Throws (warm amber-tinted off-center train echoes)
// ============================================================================
let t_dub = s.track("dub_rim_throws");
s.track_blend(t_dub, 0.85);
for hit_b in [28.0, 44.0, 60.0, 76.0, 108.0, 124.0, 140.0, 148.0] {
    let th = s.snippet(t_dub, rim_src, hit_b, 0.70);
    s.set(th, "pitch", -3.0);
    s.set(th, "varispeed", 1.0);
    s.set(th, "gain", 0.34);
    s.set(th, "additive", 0.82);
    s.set(th, "feather", 0.40);
    s.transform(th, -0.26, -0.14, 0.48, -2.0);
}
let dub_tint = s.track_effect(t_dub, "tint");
s.track_fx(t_dub, dub_tint, "hue", 30.0);
s.track_fx(t_dub, dub_tint, "amount", 0.45);
s.track_fx(t_dub, dub_tint, "audio", 0.0);

let dub_dl = s.track_effect(t_dub, "delay");
s.track_fx(t_dub, dub_dl, "time", 0.338); // dotted-8th at 133 BPM
s.track_fx(t_dub, dub_dl, "feedback", 0.56);
s.track_fx(t_dub, dub_dl, "mix", 0.60);
s.track_fx(t_dub, dub_dl, "shift_x", 0.075);
s.track_fx(t_dub, dub_dl, "shift_y", -0.015);

// ============================================================================
// TRACK 6: Reversed Open-Vowel Breath Swells & Soft Halo (natural -5 st air!)
// ============================================================================
let t_vghost = s.track("vocal_ghosts");
s.track_blend(t_vghost, 0.65);

// Soft reversed open 'Ooooh' swells leading into phrase transitions
for sw_b in [29.5, 61.5, 77.5, 109.5, 141.5] {
    let rev = s.snippet(t_vghost, vbench_cut, sw_b, 2.0);
    s.set(rev, "position", 0.06); // open 'Ooooh' vowel!
    s.set(rev, "pitch", -5.0);
    s.set(rev, "varispeed", 1.0);
    s.set(rev, "reverse_prob", 1.0);
    s.set(rev, "envelope", 0.85);
    s.set(rev, "env_skew", 0.82);
    s.set(rev, "gain", 0.36);
    s.set(rev, "additive", 0.65);
    s.set(rev, "feather", 0.38);
    s.audio_filter(rev, "lowpass", 4800.0, 0.70);
    s.transform(rev, 0.16, -0.03, 0.92, 0.0);
}

// Gentle shimmering vowel halo in Breakdown & Drop II (0 / -5 st, open up to 4.5 kHz)
let v_halo = s.clip(t_vghost, vstreet_cut, 84.0, 58.0);
s.key(v_halo, "F", "minor_pentatonic");
s.set(v_halo, "position", 0.08); // open falsetto 'Hoooo'
s.set(v_halo, "density", 4.5);
s.set(v_halo, "duration", 0.42);
s.set(v_halo, "spray", 1.2);
s.set(v_halo, "pitch", -5.0);
s.set(v_halo, "pitch_jitter", 2.0);
s.set(v_halo, "reverse_prob", 0.35);
s.set(v_halo, "pan_spread", 0.84);
s.set(v_halo, "gain", 0.18);
s.set(v_halo, "additive", 0.62);
s.set(v_halo, "feather", 0.42);
s.set(v_halo, "flicker", 0.12);
s.audio_filter(v_halo, "lowpass", 4600.0, 0.70);
s.audio_filter(v_halo, "highpass", 220.0, 0.70);
s.automate(v_halo, "gain", 0.0, 0.08);
s.automate(v_halo, "gain", 22.0, 0.20);
s.automate(v_halo, "gain", 58.0, 0.04);

let vg_verb = s.track_effect(t_vghost, "reverb");
s.track_fx(t_vghost, vg_verb, "size", 0.84);
s.track_fx(t_vghost, vg_verb, "damp", 0.48);
s.track_fx(t_vghost, vg_verb, "mix", 0.48);

// ============================================================================
// TRACK 7: Lead Sampled Open-Vowel Hook & Commuter Cutouts
//          (Full 2–10.5 kHz human breath air, recurring F-minor melodic motif,
//           ZERO lyrics, ZERO -12 st monster grunts!)
// ============================================================================
let t_voc = s.track("vocal_open_motif");
s.track_blend(t_voc, 0.42);
s.track_tape(t_voc, 0.10, 0.16, 0.04, 0.0); // age=0.04 preserves silky human breath & air!

// Visually dim rain window whenever lead human silhouettes appear so figures pop clearly!
s.track_visual_duck(t_rain, t_voc, 0.52, 0.015, 0.38);

// Curated wordless open-vowel slice positions in our 12.0s stems:
//   vbench_cut:  0.03 = open soulful "Ooooooh", 0.14 = melisma turn, 0.35 = descending "Ooo-mmm", 0.70 = soft breath/hum
//   vstreet_cut: 0.03 = airy falsetto "Hoooo",  0.35 = warm chest "Ohhh", 0.69 = pure "Haaaah" exhale
let b_ooh  = 0.03;
let b_turn = 0.14;
let b_sigh = 0.35;
let b_hum  = 0.70;

let m_hoo  = 0.03;
let m_oh   = 0.35;
let m_air  = 0.69;

// --- ACT I (INTRO): Foreshadow the open-vowel hook in clear natural register ---
let vi_1 = s.snippet(t_voc, vbench_cut, 7.5, 2.0);
s.key(vi_1, "F", "minor");
s.set(vi_1, "position", b_ooh);
s.set(vi_1, "pitch", 0.0);
s.set(vi_1, "varispeed", 1.0);
s.set(vi_1, "gain", 0.54);
s.set(vi_1, "envelope", 0.32);
s.set(vi_1, "feather", 0.24);
s.transform(vi_1, -0.14, 0.03, 1.08, 0.0);

let vi_2 = s.snippet(t_voc, vstreet_cut, 19.5, 2.2);
s.key(vi_2, "F", "minor");
s.set(vi_2, "position", m_hoo);
s.set(vi_2, "pitch", -2.0);
s.set(vi_2, "varispeed", 1.0);
s.set(vi_2, "gain", 0.52);
s.set(vi_2, "envelope", 0.34);
s.set(vi_2, "feather", 0.24);
s.transform(vi_2, 0.16, 0.03, 1.10, 0.0);

// --- ACT II (DROP I, 32..80): Recurring 4-bar Call & Response Open-Vowel Motif ---
// Phrase A (bars 1-2): Female "Ooooh" (0 st) -> "oooh" (-2 st) -> "ooo-mmm" (-4 st)
//                      answered by Male falsetto "Hoooo" (-2 st -> -5 st)
// Phrase B (bars 3-4): Female "+3 st" gentle peak -> "Ooooh" (0 st) -> Male chest "Ohhh" (-4 st)
let p_voc1 = s.pattern("drop1_open_motif");
s.pattern_grid(p_voc1, 16.0, 4); // 64 steps = 4 bars
s.pattern_swing(p_voc1, 0.60);

let rv_fem = s.row(p_voc1, vbench_cut);
s.row_key(p_voc1, rv_fem, "F", "minor");
s.row_set(p_voc1, rv_fem, "duration", 0.58);
s.row_set(p_voc1, rv_fem, "gain", 0.62);
s.row_set(p_voc1, rv_fem, "envelope", 0.28);
s.row_set(p_voc1, rv_fem, "pan", -0.12);
s.row_set(p_voc1, rv_fem, "pan_spread", 0.22);

s.step(p_voc1, rv_fem, 0, true);  s.step_slice(p_voc1, rv_fem, 0, b_ooh);  s.step_pitch(p_voc1, rv_fem, 0, 0.0);  s.step_length(p_voc1, rv_fem, 0, 1.25);
s.step(p_voc1, rv_fem, 6, true);  s.step_slice(p_voc1, rv_fem, 6, b_turn); s.step_pitch(p_voc1, rv_fem, 6, -2.0); s.step_length(p_voc1, rv_fem, 6, 0.90);
s.step(p_voc1, rv_fem, 11, true); s.step_slice(p_voc1, rv_fem, 11, b_sigh);s.step_pitch(p_voc1, rv_fem, 11, -4.0);s.step_length(p_voc1, rv_fem, 11, 1.15);

s.step(p_voc1, rv_fem, 32, true); s.step_slice(p_voc1, rv_fem, 32, b_ooh); s.step_pitch(p_voc1, rv_fem, 32, 3.0);  s.step_length(p_voc1, rv_fem, 32, 1.10);
s.step(p_voc1, rv_fem, 38, true); s.step_slice(p_voc1, rv_fem, 38, b_ooh); s.step_pitch(p_voc1, rv_fem, 38, 0.0);  s.step_length(p_voc1, rv_fem, 38, 0.95);
s.step(p_voc1, rv_fem, 43, true); s.step_slice(p_voc1, rv_fem, 43, b_sigh);s.step_pitch(p_voc1, rv_fem, 43, -2.0); s.step_length(p_voc1, rv_fem, 43, 1.20);

let rv_mal = s.row(p_voc1, vstreet_cut);
s.row_key(p_voc1, rv_mal, "F", "minor");
s.row_set(p_voc1, rv_mal, "duration", 0.54);
s.row_set(p_voc1, rv_mal, "gain", 0.58);
s.row_set(p_voc1, rv_mal, "envelope", 0.30);
s.row_set(p_voc1, rv_mal, "pan", 0.16);
s.row_set(p_voc1, rv_mal, "pan_spread", 0.22);

s.step(p_voc1, rv_mal, 22, true); s.step_slice(p_voc1, rv_mal, 22, m_hoo); s.step_pitch(p_voc1, rv_mal, 22, 0.0);  s.step_length(p_voc1, rv_mal, 22, 1.15);
s.step(p_voc1, rv_mal, 27, true); s.step_slice(p_voc1, rv_mal, 27, m_oh);  s.step_pitch(p_voc1, rv_mal, 27, -4.0); s.step_gain(p_voc1, rv_mal, 27, 0.78);
s.step(p_voc1, rv_mal, 54, true); s.step_slice(p_voc1, rv_mal, 54, m_oh);  s.step_pitch(p_voc1, rv_mal, 54, -2.0); s.step_length(p_voc1, rv_mal, 54, 1.10);
s.step(p_voc1, rv_mal, 59, true); s.step_slice(p_voc1, rv_mal, 59, m_air); s.step_pitch(p_voc1, rv_mal, 59, -5.0); s.step_gain(p_voc1, rv_mal, 59, 0.72);

let c_voc_d1 = s.pattern_clip(t_voc, p_voc1, 32.0, 48.0);
s.set(c_voc_d1, "additive", 0.48);
s.set(c_voc_d1, "flicker", 0.18);
s.set(c_voc_d1, "feather", 0.20);
s.set(c_voc_d1, "min_size", 0.38);
s.set(c_voc_d1, "max_size", 0.78);

// --- ACT III (BREAKDOWN, 80..110): Large prominent cutouts & bare open-air singing ---
let vb_1 = s.snippet(t_voc, vbench_cut, 81.5, 5.2); // ~2.3s soulful open "Ooooooh" melisma, natural register!
s.key(vb_1, "F", "minor");
s.set(vb_1, "position", b_ooh);
s.set(vb_1, "pitch", 0.0);
s.set(vb_1, "varispeed", 1.0);
s.set(vb_1, "gain", 0.64);
s.set(vb_1, "envelope", 0.26);
s.set(vb_1, "feather", 0.24);
s.transform(vb_1, -0.08, 0.02, 1.28, 0.0);

let vb_2 = s.snippet(t_voc, vstreet_cut, 89.5, 5.0); // ~2.2s airy falsetto "Hoooo... ooooh" answer
s.key(vb_2, "F", "minor");
s.set(vb_2, "position", m_hoo);
s.set(vb_2, "pitch", 0.0);
s.set(vb_2, "varispeed", 1.0);
s.set(vb_2, "gain", 0.60);
s.set(vb_2, "envelope", 0.28);
s.set(vb_2, "feather", 0.24);
s.transform(vb_2, 0.12, 0.02, 1.26, 0.0);

let vb_3 = s.snippet(t_voc, vbench_cut, 97.5, 5.8); // ~2.6s descending "Ooo-oo-ooo-mmm" dissolving into grains
s.key(vb_3, "F", "minor");
s.set(vb_3, "position", b_sigh);
s.set(vb_3, "pitch", -2.0);
s.set(vb_3, "speed", 0.85);
s.set(vb_3, "texture", 0.20);
s.set(vb_3, "density", 14.0);
s.set(vb_3, "duration", 0.30);
s.set(vb_3, "gain", 0.58);
s.set(vb_3, "feather", 0.28);
s.set(vb_3, "flicker", 0.14);
s.transform(vb_3, 0.0, 0.03, 1.32, 0.0);

// Pure intimate human exhale before Drop II hush (zero orchestral noise spike!)
let vb_4 = s.snippet(t_voc, vstreet_cut, 105.5, 3.8);
s.key(vb_4, "F", "minor");
s.set(vb_4, "position", m_oh);
s.set(vb_4, "pitch", -4.0);
s.set(vb_4, "varispeed", 1.0);
s.set(vb_4, "gain", 0.54);
s.set(vb_4, "envelope", 0.34);
s.set(vb_4, "feather", 0.28);
s.transform(vb_4, 0.10, 0.02, 1.18, 0.0);

// --- ACT IV (DROP II, 112..144): Syncopated UKG stutter variation of the SAME open-vowel hook ---
let p_voc2 = s.pattern("drop2_motif_stutter");
s.pattern_grid(p_voc2, 16.0, 4);
s.pattern_swing(p_voc2, 0.61);

let rv2_a = s.row(p_voc2, vbench_cut);
s.row_key(p_voc2, rv2_a, "F", "minor");
s.row_set(p_voc2, rv2_a, "duration", 0.44);
s.row_set(p_voc2, rv2_a, "gain", 0.64);
s.row_set(p_voc2, rv2_a, "envelope", 0.24);
s.row_set(p_voc2, rv2_a, "pan", -0.16);
s.row_set(p_voc2, rv2_a, "pan_spread", 0.28);

for st in [0, 3, 6, 11, 32, 35, 38, 43] {
    s.step(p_voc2, rv2_a, st, true);
}
s.step_slice(p_voc2, rv2_a, 0, b_ooh);   s.step_pitch(p_voc2, rv2_a, 0, 0.0);
s.step_slice(p_voc2, rv2_a, 3, b_ooh);   s.step_pitch(p_voc2, rv2_a, 3, 0.0);  s.step_gain(p_voc2, rv2_a, 3, 0.68); s.step_length(p_voc2, rv2_a, 3, 0.65);
s.step_slice(p_voc2, rv2_a, 6, b_turn);  s.step_pitch(p_voc2, rv2_a, 6, -2.0);
s.step_slice(p_voc2, rv2_a, 11, b_sigh); s.step_pitch(p_voc2, rv2_a, 11, -4.0); s.step_length(p_voc2, rv2_a, 11, 1.25);

s.step_slice(p_voc2, rv2_a, 32, b_ooh);  s.step_pitch(p_voc2, rv2_a, 32, 3.0);
s.step_slice(p_voc2, rv2_a, 35, b_ooh);  s.step_pitch(p_voc2, rv2_a, 35, 3.0); s.step_gain(p_voc2, rv2_a, 35, 0.65); s.step_length(p_voc2, rv2_a, 35, 0.65);
s.step_slice(p_voc2, rv2_a, 38, b_turn); s.step_pitch(p_voc2, rv2_a, 38, 0.0);
s.step_slice(p_voc2, rv2_a, 43, b_sigh); s.step_pitch(p_voc2, rv2_a, 43, -5.0); s.step_length(p_voc2, rv2_a, 43, 1.30);

let rv2_b = s.row(p_voc2, vstreet_cut);
s.row_key(p_voc2, rv2_b, "F", "minor");
s.row_set(p_voc2, rv2_b, "duration", 0.48);
s.row_set(p_voc2, rv2_b, "gain", 0.58);
s.row_set(p_voc2, rv2_b, "envelope", 0.28);
s.row_set(p_voc2, rv2_b, "pan", 0.18);
s.row_set(p_voc2, rv2_b, "pan_spread", 0.28);

s.step(p_voc2, rv2_b, 18, true); s.step_slice(p_voc2, rv2_b, 18, m_hoo); s.step_pitch(p_voc2, rv2_b, 18, 0.0);
s.step(p_voc2, rv2_b, 22, true); s.step_slice(p_voc2, rv2_b, 22, m_oh);  s.step_pitch(p_voc2, rv2_b, 22, -2.0);
s.step(p_voc2, rv2_b, 27, true); s.step_slice(p_voc2, rv2_b, 27, m_oh);  s.step_pitch(p_voc2, rv2_b, 27, -5.0); s.step_gain(p_voc2, rv2_b, 27, 0.72);
s.step(p_voc2, rv2_b, 50, true); s.step_slice(p_voc2, rv2_b, 50, m_hoo); s.step_pitch(p_voc2, rv2_b, 50, 0.0);
s.step(p_voc2, rv2_b, 54, true); s.step_slice(p_voc2, rv2_b, 54, m_oh);  s.step_pitch(p_voc2, rv2_b, 54, -4.0);
s.step(p_voc2, rv2_b, 59, true); s.step_slice(p_voc2, rv2_b, 59, m_air); s.step_pitch(p_voc2, rv2_b, 59, -5.0); s.step_length(p_voc2, rv2_b, 59, 1.25);

let c_voc_d2 = s.pattern_clip(t_voc, p_voc2, 112.0, 32.0);
s.set(c_voc_d2, "additive", 0.52);
s.set(c_voc_d2, "flicker", 0.18);
s.set(c_voc_d2, "feather", 0.20);
s.set(c_voc_d2, "min_size", 0.40);
s.set(c_voc_d2, "max_size", 0.80);

// Mirrored dual-silhouette reflection in Drop II climax over rushing subway tunnel!
let d2_tile = s.effect(c_voc_d2, "tile");
s.fx(c_voc_d2, d2_tile, "count", 2.0);
s.fx(c_voc_d2, d2_tile, "mirror", 1.0);
s.fx(c_voc_d2, d2_tile, "audio", 0.0);
s.fx(c_voc_d2, d2_tile, "video", 0.48);

// --- ACT V (OUTRO, 144..160): Final open sigh & hum fading into night rain & vinyl dust ---
let v_out = s.snippet(t_voc, vbench_cut, 145.5, 6.2);
s.key(v_out, "F", "minor");
s.set(v_out, "position", b_sigh);
s.set(v_out, "pitch", -2.0);
s.set(v_out, "varispeed", 1.0);
s.set(v_out, "gain", 0.50);
s.set(v_out, "envelope", 0.42);
s.set(v_out, "feather", 0.34);
s.transform(v_out, 0.0, 0.03, 1.12, 0.0);

let voc_dl = s.track_effect(t_voc, "delay");
s.track_fx(t_voc, voc_dl, "time", 0.338);
s.track_fx(t_voc, voc_dl, "feedback", 0.40);
s.track_fx(t_voc, voc_dl, "mix", 0.26);
s.track_fx(t_voc, voc_dl, "shift_x", 0.045);
s.track_fx(t_voc, voc_dl, "shift_y", -0.01);

let voc_rv = s.track_effect(t_voc, "reverb");
s.track_fx(t_voc, voc_rv, "size", 0.72);
s.track_fx(t_voc, voc_rv, "damp", 0.44);
s.track_fx(t_voc, voc_rv, "mix", 0.28);

// ============================================================================
// MASTER BUS: Crisp post-LP vinyl dust, subtle 16mm gate weave & DCT film grit
// ============================================================================
s.master_tape(0.11, 0.16, 0.05, 0.16);

let m_comp = s.master_effect("compress");
s.master_fx(m_comp, "quality", 0.78);
s.master_fx(m_comp, "audio", 0.03);
s.master_fx(m_comp, "video", 0.34);

let m_verb = s.master_effect("reverb");
s.master_fx(m_verb, "size", 0.56);
s.master_fx(m_verb, "damp", 0.46);
s.master_fx(m_verb, "mix", 0.09);

s.render(0.0, 160.0, "outputs/261111_burial-demo/v003/261111_chromagrain_burial-demo_v003.mp4");
