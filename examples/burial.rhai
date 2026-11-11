// examples/burial.rhai — Nocturnal 2-step UK garage AV benchmark ("Half Past Three")
// Run headless:
//   target/release/chromagrain --script examples/burial.rhai
//
// Key: F minor | Tempo: 134 BPM (40 bars = 160 beats = 71.6 s)
// Structure:
//   0..32   (0:00-0:14) Intro: rain on night glass, vinyl crackle, paulstretch Fm9 wash, UK whisper
//   32..80  (0:14-0:36) Drop I: shuffled 2-step UKG drums, F1 Reese sub smears, chopped R&B vocal hook
//   80..112 (0:36-0:50) Breakdown: drums & sub drop out; solitary segmented vocal laid bare in the rain
//   112..144(0:50-1:04) Drop II: heavier re-entry with polymeter foley, octave vocal harmony & dub throws
//   144..160(1:04-1:12) Outro: drums strip away to rain, crackle, sodium glow and a last vocal sigh

let s = session(134.0);
s.canvas(640, 360, 24.0);

// ---- Load curated AV sample sources (assets/demo/) ----
let rain_src   = s.load("assets/demo/rain_window.mp4");
let chords_src = s.load("assets/demo/night_chords.mp4");
let sub_src    = s.load("assets/demo/sub_highway.mp4");
let kick_src   = s.load("assets/demo/kick_tunnel.mp4");
let rim_src    = s.load("assets/demo/rim_train.mp4");
let hat_src    = s.load("assets/demo/hat_glitter.mp4");
let vbench_src = s.load("assets/demo/vocal_bench.mp4");
let vstreet_src= s.load("assets/demo/vocal_street.mp4");

// Lock sub fundamental to F2 (87.31 Hz; audio carries F1 43.65 Hz + F2 Reese)
s.set_base_hz(sub_src, 87.307);
s.set_base_hz(chords_src, 174.614);

// Derive paulstretched AV chord wash (6x slower spectral + visual smear)
let chord_wash = s.paulstretch(chords_src, 6.0);

// Apple Vision foreground people cutouts so vocal chops flicker as human silhouettes
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
// TRACK 0: Rain & Vinyl Crackle Bed (bottom z-layer: sodium streetlight window)
// ============================================================================
let t_rain = s.track("rain_vinyl_bed");
s.track_opacity_link(t_rain, 0.25);
s.track_level_point(t_rain, 0.0, 0.0);
s.track_level_point(t_rain, 6.0, 0.95);
s.track_level_point(t_rain, 148.0, 0.95);
s.track_level_point(t_rain, 160.0, 0.0);

let b = 0.0;
while b < 160.0 {
    let seg = s.snippet(t_rain, rain_src, b, 16.0);
    s.set(seg, "gain", 0.32);
    s.set(seg, "gain_to_opacity", 0.15);
    s.set(seg, "speed", 0.65);
    s.set(seg, "envelope", 0.12);
    b += 16.0;
}
let rain_tint = s.track_effect(t_rain, "tint");
s.track_fx(t_rain, rain_tint, "hue", 28.0);     // warm sodium streetlight amber
s.track_fx(t_rain, rain_tint, "amount", 0.32);
s.track_fx(t_rain, rain_tint, "audio", 0.0);    // keep vinyl & rain full-spectrum

// ============================================================================
// TRACK 1: Paulstretched Minor Chords (smeared nocturnal harmonic pad)
// ============================================================================
let t_pad = s.track("paulstretch_pad");
s.track_opacity_link(t_pad, 0.55);
s.track_level_point(t_pad, 0.0, 0.0);
s.track_level_point(t_pad, 12.0, 0.82);
s.track_level_point(t_pad, 32.0, 0.68);
s.track_level_point(t_pad, 80.0, 0.92);
s.track_level_point(t_pad, 112.0, 0.74);
s.track_level_point(t_pad, 144.0, 0.65);
s.track_level_point(t_pad, 160.0, 0.0);

let pad_intro = s.clip(t_pad, chord_wash, 2.0, 78.0);
s.key(pad_intro, "F", "minor");
s.set(pad_intro, "quantize_amount", 0.75);
s.set(pad_intro, "detune_cents", -11.0);
s.set(pad_intro, "density", 8.5);
s.set(pad_intro, "duration", 0.65);
s.set(pad_intro, "spray", 1.8);
s.set(pad_intro, "scan_speed", 0.45);
s.set(pad_intro, "gain", 0.42);
s.set(pad_intro, "additive", 0.45);
s.set(pad_intro, "size_scale", 1.8);
s.audio_filter(pad_intro, "lowpass", 1900.0, 0.75);
s.audio_filter(pad_intro, "highpass", 130.0, 0.7);

// Breakdown pad: warmer, wider, pitched down an octave with fifth shimmer
let pad_brk = s.clip(t_pad, chord_wash, 80.0, 32.0);
s.key(pad_brk, "F", "minor");
s.set(pad_brk, "quantize_amount", 0.8);
s.set(pad_brk, "detune_cents", 9.0);
s.set(pad_brk, "density", 10.0);
s.set(pad_brk, "duration", 0.75);
s.set(pad_brk, "position", 0.22);
s.set(pad_brk, "spray", 2.4);
s.set(pad_brk, "scan_speed", 0.35);
s.set(pad_brk, "gain", 0.48);
s.set(pad_brk, "voices", 2.0);
s.set(pad_brk, "voice_interval", 7.0);
s.set(pad_brk, "voice_detune", 0.12);
s.set(pad_brk, "additive", 0.55);
s.audio_filter(pad_brk, "lowpass", 2400.0, 0.75);
s.audio_filter(pad_brk, "highpass", 120.0, 0.7);

let pad_re = s.clip(t_pad, chord_wash, 112.0, 46.0);
s.key(pad_re, "F", "minor");
s.set(pad_re, "detune_cents", -8.0);
s.set(pad_re, "density", 9.0);
s.set(pad_re, "duration", 0.62);
s.set(pad_re, "position", 0.45);
s.set(pad_re, "spray", 1.6);
s.set(pad_re, "gain", 0.42);
s.set(pad_re, "additive", 0.45);
s.audio_filter(pad_re, "lowpass", 2100.0, 0.75);
s.audio_filter(pad_re, "highpass", 130.0, 0.7);

let pad_verb = s.track_effect(t_pad, "reverb");
s.track_fx(t_pad, pad_verb, "size", 0.82);
s.track_fx(t_pad, pad_verb, "damp", 0.65);
s.track_fx(t_pad, pad_verb, "mix", 0.42);

// ============================================================================
// TRACK 2: Deep Sub / Reese Bass (heavy wet-highway smears along lower canvas)
// ============================================================================
let t_sub = s.track("sub_bass");
s.track_opacity_link(t_sub, 0.6);

// Helper-like loop placing the 4-bar (16-beat) Burial progression:
//   Beats 0..6:   F  (0 st)
//   Beats 6..8:   Ab (+3 st)
//   Beats 8..13:  Db (-4 st)
//   Beats 13..16: C  (-5 st) / Eb (-2 st)
for bar_start in [32.0, 48.0, 64.0, 112.0, 128.0] {
    let n1 = s.snippet(t_sub, sub_src, bar_start + 0.0, 5.75);
    s.set(n1, "pitch", 0.0);
    s.set(n1, "speed", 0.6);
    s.set(n1, "gain", 0.82);
    s.set(n1, "envelope", 0.18);
    s.set(n1, "additive", 0.45);
    s.transform(n1, 0.0, 0.34, 1.15, 0.0);

    let n2 = s.snippet(t_sub, sub_src, bar_start + 6.0, 1.85);
    s.set(n2, "pitch", 3.0);
    s.set(n2, "speed", 0.7);
    s.set(n2, "gain", 0.74);
    s.set(n2, "envelope", 0.22);
    s.set(n2, "additive", 0.45);
    s.transform(n2, -0.06, 0.34, 1.1, 0.0);

    let n3 = s.snippet(t_sub, sub_src, bar_start + 8.0, 4.75);
    s.set(n3, "pitch", -4.0);
    s.set(n3, "speed", 0.6);
    s.set(n3, "gain", 0.84);
    s.set(n3, "envelope", 0.18);
    s.set(n3, "additive", 0.45);
    s.transform(n3, 0.05, 0.36, 1.18, 0.0);

    let turn_st = if bar_start == 64.0 || bar_start == 128.0 { -5.0 } else { -2.0 };
    let n4 = s.snippet(t_sub, sub_src, bar_start + 13.0, 2.75);
    s.set(n4, "pitch", turn_st);
    s.set(n4, "speed", 0.65);
    s.set(n4, "gain", 0.76);
    s.set(n4, "envelope", 0.22);
    s.set(n4, "additive", 0.45);
    s.transform(n4, 0.0, 0.35, 1.12, 0.0);
}

// Keep sub audio tight & mono-centered while smearing its video heavily!
let sub_smear = s.track_effect(t_sub, "reverb");
s.track_fx(t_sub, sub_smear, "size", 0.88);
s.track_fx(t_sub, sub_smear, "damp", 0.75);
s.track_fx(t_sub, sub_smear, "mix", 0.65);
s.track_fx(t_sub, sub_smear, "audio", 0.0);
s.track_fx(t_sub, sub_smear, "video", 1.0);

// ============================================================================
// TRACK 3: Shuffled 2-Step UK Garage Drums (Step Sequencer + Polymeter Foley)
// ============================================================================
let t_drums = s.track("ukg_2step_drums");

// Pattern A: Intro sparse ghost percussion (8 beats = 32 steps)
let p_intro = s.pattern("intro_ghosts");
s.pattern_grid(p_intro, 8.0, 4);

let ig_rim = s.row(p_intro, rim_src);
s.row_set(p_intro, ig_rim, "duration", 0.38);
s.row_set(p_intro, ig_rim, "gain", 0.42);
s.row_set(p_intro, ig_rim, "envelope", 0.12);
s.row_set(p_intro, ig_rim, "env_skew", -0.85);
s.step(p_intro, ig_rim, 12, true);
s.step(p_intro, ig_rim, 28, true); s.step_gain(p_intro, ig_rim, 28, 0.65);

let ig_shk = s.row(p_intro, hat_src);
s.row_set(p_intro, ig_shk, "position", 0.50);
s.row_set(p_intro, ig_shk, "duration", 0.16);
s.row_set(p_intro, ig_shk, "gain", 0.32);
s.row_set(p_intro, ig_shk, "pan_spread", 0.65);
s.row_set(p_intro, ig_shk, "env_skew", -0.7);
for st in [2, 6, 11, 14, 18, 22, 27, 30] {
    s.step(p_intro, ig_shk, st, true);
}

// Pattern B: Full 2-step UK garage groove (8 beats = 32 steps = 2 bars at 134 BPM)
let p_drop = s.pattern("ukg_2step_main");
s.pattern_grid(p_drop, 8.0, 4);

// Row 0: Syncopated 2-step kick (skipping beat 3 -> classic UKG pocket!)
let r_kick = s.row(p_drop, kick_src);
s.row_set(p_drop, r_kick, "duration", 0.36);
s.row_set(p_drop, r_kick, "gain", 0.92);
s.row_set(p_drop, r_kick, "pan_spread", 0.0);
s.row_set(p_drop, r_kick, "envelope", 0.08);
s.row_set(p_drop, r_kick, "env_skew", -0.9);
for st in [0, 10, 16, 19, 26] {
    s.step(p_drop, r_kick, st, true);
}
s.step_gain(p_drop, r_kick, 10, 0.88);
s.step_gain(p_drop, r_kick, 19, 0.52); // ghost double-kick
s.step_gain(p_drop, r_kick, 26, 0.86);

// Row 1: Burial dusty rimshot/cross-stick on 2 & 4 + ghost hits
let r_rim = s.row(p_drop, rim_src);
s.row_set(p_drop, r_rim, "duration", 0.34);
s.row_set(p_drop, r_rim, "gain", 0.82);
s.row_set(p_drop, r_rim, "pan_spread", 0.15);
s.row_set(p_drop, r_rim, "envelope", 0.08);
s.row_set(p_drop, r_rim, "env_skew", -0.88);
for st in [4, 7, 12, 15, 20, 28, 31] {
    s.step(p_drop, r_rim, st, true);
}
s.step_gain(p_drop, r_rim, 7, 0.34);
s.step_pitch(p_drop, r_rim, 7, -2.0);
s.step_gain(p_drop, r_rim, 15, 0.28);
s.step_pitch(p_drop, r_rim, 15, 2.0);
s.step_gain(p_drop, r_rim, 31, 0.38);

// Row 2: Closed hat / lighter tick (glitter bokeh patches!)
let r_ch = s.row(p_drop, hat_src);
s.row_set(p_drop, r_ch, "position", 0.0);
s.row_set(p_drop, r_ch, "duration", 0.11);
s.row_set(p_drop, r_ch, "gain", 0.46);
s.row_set(p_drop, r_ch, "pan_spread", 0.75);
s.row_set(p_drop, r_ch, "envelope", 0.12);
s.row_set(p_drop, r_ch, "env_skew", -0.8);
for st in [0, 2, 3, 6, 8, 11, 14, 16, 18, 22, 23, 26, 27, 30] {
    s.step(p_drop, r_ch, st, true);
}
for ghost_st in [3, 11, 23, 27] {
    s.step_gain(p_drop, r_ch, ghost_st, 0.52);
    s.step_pitch(p_drop, r_ch, ghost_st, -1.5);
}

// Row 3: Open garage hat on offbeats
let r_oh = s.row(p_drop, hat_src);
s.row_set(p_drop, r_oh, "position", 0.25);
s.row_set(p_drop, r_oh, "duration", 0.24);
s.row_set(p_drop, r_oh, "gain", 0.38);
s.row_set(p_drop, r_oh, "pan", 0.22);
s.row_set(p_drop, r_oh, "pan_spread", 0.45);
s.row_set(p_drop, r_oh, "envelope", 0.15);
s.row_set(p_drop, r_oh, "env_skew", -0.75);
for st in [2, 10, 18, 26] {
    s.step(p_drop, r_oh, st, true);
}

// Row 4: Polymeter 7-step casing/shaker row — drifts across the 16-step bar!
let r_shk = s.row(p_drop, hat_src);
s.row_steps(p_drop, r_shk, 7);
s.row_set(p_drop, r_shk, "position", 0.50);
s.row_set(p_drop, r_shk, "duration", 0.13);
s.row_set(p_drop, r_shk, "gain", 0.34);
s.row_set(p_drop, r_shk, "pan_spread", 0.85);
s.row_set(p_drop, r_shk, "env_skew", -0.7);
s.step(p_drop, r_shk, 0, true);
s.step(p_drop, r_shk, 3, true); s.step_gain(p_drop, r_shk, 3, 0.65);
s.step(p_drop, r_shk, 5, true); s.step_gain(p_drop, r_shk, 5, 0.50);

// Row 5: Hollow woodblock / turnstile ghost hit
let r_wb = s.row(p_drop, hat_src);
s.row_set(p_drop, r_wb, "position", 0.75);
s.row_set(p_drop, r_wb, "duration", 0.18);
s.row_set(p_drop, r_wb, "gain", 0.44);
s.row_set(p_drop, r_wb, "pan", -0.28);
s.row_set(p_drop, r_wb, "pan_spread", 0.4);
s.row_set(p_drop, r_wb, "env_skew", -0.85);
for st in [6, 15, 21, 29] {
    s.step(p_drop, r_wb, st, true);
}
s.step_pitch(p_drop, r_wb, 15, 3.0);
s.step_pitch(p_drop, r_wb, 29, -2.0);

// Place drum pattern clips across Intro, Drop I, Breakdown riser, and Drop II
let c_d_intro = s.pattern_clip(t_drums, p_intro, 16.0, 16.0);
s.set(c_d_intro, "additive", 0.65);
s.set(c_d_intro, "size_scale", 1.3);
s.automate(c_d_intro, "gain", 0.0, 0.45);
s.automate(c_d_intro, "gain", 16.0, 0.95);

let c_d_drop1 = s.pattern_clip(t_drums, p_drop, 32.0, 48.0);
s.set(c_d_drop1, "additive", 0.72);
s.set(c_d_drop1, "size_scale", 1.25);
s.set(c_d_drop1, "min_size", 0.08);
s.set(c_d_drop1, "max_size", 0.45);

// Late-breakdown ghost percussion reentry (beats 104..112)
let c_d_riser = s.pattern_clip(t_drums, p_intro, 104.0, 8.0);
s.set(c_d_riser, "additive", 0.65);
s.automate(c_d_riser, "gain", 0.0, 0.35);
s.automate(c_d_riser, "gain", 8.0, 1.0);

let c_d_drop2 = s.pattern_clip(t_drums, p_drop, 112.0, 32.0);
s.set(c_d_drop2, "additive", 0.78);
s.set(c_d_drop2, "size_scale", 1.35);
s.set(c_d_drop2, "min_size", 0.08);
s.set(c_d_drop2, "max_size", 0.48);

// ============================================================================
// TRACK 4: Dub Rimshot Throws (AV Delay -> drifting silver train ghosts)
// ============================================================================
let t_dub = s.track("dub_rim_throws");
for hit_b in [28.0, 44.0, 60.0, 76.0, 108.0, 124.0, 140.0, 148.0] {
    let th = s.snippet(t_dub, rim_src, hit_b, 0.9);
    s.set(th, "pitch", -3.0);
    s.set(th, "gain", 0.48);
    s.set(th, "additive", 0.75);
    s.transform(th, -0.15, -0.08, 0.65, -3.0);
}
let dub_dl = s.track_effect(t_dub, "delay");
s.track_fx(t_dub, dub_dl, "time", 0.336);      // dotted-8th at 134 BPM
s.track_fx(t_dub, dub_dl, "feedback", 0.62);
s.track_fx(t_dub, dub_dl, "mix", 0.65);
s.track_fx(t_dub, dub_dl, "shift_x", 0.065);
s.track_fx(t_dub, dub_dl, "shift_y", -0.015);

// ============================================================================
// TRACK 5: Reversed Vocal Ghosts & Granular Shimmer Clouds (cut-out silhouettes)
// ============================================================================
let t_vghost = s.track("vocal_ghosts");

// Reversed vocal breath swells into structural transitions
for sw_b in [29.5, 61.5, 77.5, 109.5, 141.5] {
    let rev = s.snippet(t_vghost, vbench_cut, sw_b, 2.5);
    s.set(rev, "position", 0.02);
    s.set(rev, "pitch", 3.0);
    s.set(rev, "speed", 0.75);
    s.set(rev, "reverse_prob", 1.0);
    s.set(rev, "envelope", 0.85);
    s.set(rev, "env_skew", 0.85);   // slow reverse swell peaking right at the bar line
    s.set(rev, "gain", 0.52);
    s.set(rev, "additive", 0.6);
    s.transform(rev, 0.12, -0.05, 0.95, 0.0);
}

// Shimmering F-minor pentatonic vocal grain halo during Breakdown & Drop II
let v_halo = s.clip(t_vghost, vbench_cut, 88.0, 56.0);
s.key(v_halo, "F", "minor_pentatonic");
s.set(v_halo, "position", 0.03);
s.set(v_halo, "density", 9.0);
s.set(v_halo, "duration", 0.24);
s.set(v_halo, "spray", 1.2);
s.set(v_halo, "pitch", 12.0);
s.set(v_halo, "pitch_jitter", 5.0);
s.set(v_halo, "reverse_prob", 0.45);
s.set(v_halo, "pan_spread", 0.85);
s.set(v_halo, "gain", 0.28);
s.set(v_halo, "additive", 0.7);
s.audio_filter(v_halo, "bandpass", 1600.0, 0.9);
s.automate(v_halo, "gain", 0.0, 0.16);
s.automate(v_halo, "gain", 24.0, 0.34);
s.automate(v_halo, "gain", 56.0, 0.12);

let vg_verb = s.track_effect(t_vghost, "reverb");
s.track_fx(t_vghost, vg_verb, "size", 0.86);
s.track_fx(t_vghost, vg_verb, "damp", 0.55);
s.track_fx(t_vghost, vg_verb, "mix", 0.52);

// ============================================================================
// TRACK 6: Lead Chopped & Pitched Vocals (segmented human figures in foreground)
// ============================================================================
let t_voc = s.track("vocal_chops_lead");

// --- INTRO (8..28): Spoken Bristol late-night bus memory ---
// "I thought I saw your face through the rain... waiting on the platform..."
let v_intro1 = s.snippet(t_voc, vstreet_cut, 6.0, 9.5);
s.set(v_intro1, "position", 0.50);
s.set(v_intro1, "pitch", -1.0);
s.set(v_intro1, "speed", 0.92);
s.set(v_intro1, "gain", 0.72);
s.set(v_intro1, "envelope", 0.15);
s.audio_filter(v_intro1, "highpass", 220.0, 0.7);
s.audio_filter(v_intro1, "lowpass", 4200.0, 0.8);
s.transform(v_intro1, 0.18, 0.0, 0.92, 0.0);

// Tease pitched-up 'ooh' chop at beat 20
let v_intro2 = s.snippet(t_voc, vbench_cut, 20.0, 4.5);
s.set(v_intro2, "position", 0.01);
s.set(v_intro2, "pitch", 3.0);
s.set(v_intro2, "speed", 0.85);
s.set(v_intro2, "gain", 0.58);
s.set(v_intro2, "texture", 0.22);
s.set(v_intro2, "density", 18.0);
s.set(v_intro2, "duration", 0.18);
s.transform(v_intro2, -0.1, 0.04, 1.05, 0.0);

// --- DROP I (32..80): Signature Burial chopped & pitched 2-step vocal hook ---
for cycle in [32.0, 48.0, 64.0] {
    // Chop 1 (beat 0.5): pitched-up aching 'Ooh...' (pos 0.01)
    let c1 = s.snippet(t_voc, vbench_cut, cycle + 0.5, 3.0);
    s.set(c1, "position", 0.015);
    s.set(c1, "pitch", 3.0);
    s.set(c1, "speed", 0.95);
    s.set(c1, "gain", 0.76);
    s.set(c1, "envelope", 0.18);
    s.transform(c1, -0.12, 0.02, 1.08, 0.0);

    // Chop 2 (beat 4.5): falsetto 'In the dark...' pitched -2 st (androgynous!)
    let c2 = s.snippet(t_voc, vstreet_cut, cycle + 4.5, 3.0);
    s.set(c2, "position", 0.0);
    s.set(c2, "pitch", -2.0);
    s.set(c2, "speed", 0.92);
    s.set(c2, "gain", 0.78);
    s.set(c2, "envelope", 0.18);
    s.transform(c2, 0.18, 0.0, 0.96, 0.0);

    // Chop 3 (beat 8.5): 'half past three...'chopped from slot 1 (pos 0.31)
    let c3 = s.snippet(t_voc, vbench_cut, cycle + 8.5, 3.5);
    s.set(c3, "position", 0.31);
    s.set(c3, "pitch", 0.0);
    s.set(c3, "speed", 0.90);
    s.set(c3, "gain", 0.80);
    s.set(c3, "texture", 0.18);
    s.set(c3, "density", 22.0);
    s.set(c3, "duration", 0.16);
    s.transform(c3, -0.05, 0.03, 1.12, 0.0);

    // Chop 4 (beat 13.0): 'can't let go...' answer chop (pos 0.26)
    let ans_pitch = if cycle == 64.0 { 5.0 } else { 0.0 };
    let c4 = s.snippet(t_voc, vstreet_cut, cycle + 13.0, 2.75);
    s.set(c4, "position", 0.26);
    s.set(c4, "pitch", ans_pitch);
    s.set(c4, "speed", 0.95);
    s.set(c4, "gain", 0.74);
    s.set(c4, "envelope", 0.20);
    s.transform(c4, 0.14, -0.02, 1.0, 0.0);
}

// --- BREAKDOWN (80..112): Vocal laid bare over rain & paulstretch wash ---
// "Watch the clock, it's half past three..."
let vb1 = s.snippet(t_voc, vbench_cut, 81.0, 10.0);
s.set(vb1, "position", 0.25);
s.set(vb1, "pitch", 0.0);
s.set(vb1, "speed", 0.92);
s.set(vb1, "gain", 0.88);
s.set(vb1, "envelope", 0.16);
s.transform(vb1, 0.0, 0.02, 1.22, 0.0);

// "...Just the memory of your touch..."
let vb2 = s.snippet(t_voc, vbench_cut, 92.0, 10.0);
s.set(vb2, "position", 0.50);
s.set(vb2, "pitch", 0.0);
s.set(vb2, "speed", 0.90);
s.set(vb2, "gain", 0.88);
s.set(vb2, "texture", 0.12); // dissolves gently at edges
s.set(vb2, "density", 24.0);
s.set(vb2, "duration", 0.20);
s.transform(vb2, -0.04, 0.02, 1.25, 0.0);

// Whisper before the re-entry: "...don't let the city take you..."
let vb3 = s.snippet(t_voc, vstreet_cut, 103.0, 8.5);
s.set(vb3, "position", 0.75);
s.set(vb3, "pitch", -1.0);
s.set(vb3, "speed", 0.92);
s.set(vb3, "gain", 0.78);
s.set(vb3, "envelope", 0.18);
s.audio_filter(vb3, "highpass", 220.0, 0.7);
s.audio_filter(vb3, "lowpass", 4200.0, 0.8);
s.transform(vb3, 0.16, 0.0, 1.02, 0.0);

// --- DROP II (112..144): Climax vocal hook + high ghostly counterpoint ---
for cycle in [112.0, 128.0] {
    let d1 = s.snippet(t_voc, vbench_cut, cycle + 0.5, 3.2);
    s.set(d1, "position", 0.75); // "Silences are deafening..."
    s.set(d1, "pitch", 3.0);
    s.set(d1, "speed", 0.95);
    s.set(d1, "gain", 0.80);
    s.transform(d1, -0.10, 0.02, 1.15, 0.0);

    let d2 = s.snippet(t_voc, vstreet_cut, cycle + 4.5, 3.2);
    s.set(d2, "position", 0.08); // "...waiting for you..."
    s.set(d2, "pitch", 0.0);
    s.set(d2, "speed", 0.92);
    s.set(d2, "gain", 0.82);
    s.transform(d2, 0.15, 0.0, 1.02, 0.0);

    let d3 = s.snippet(t_voc, vbench_cut, cycle + 8.5, 3.5);
    s.set(d3, "position", 0.01); // soaring melisma
    s.set(d3, "pitch", 5.0);
    s.set(d3, "speed", 0.88);
    s.set(d3, "gain", 0.76);
    s.set(d3, "texture", 0.25);
    s.set(d3, "density", 24.0);
    s.set(d3, "duration", 0.15);
    s.transform(d3, 0.0, -0.04, 1.18, 0.0);

    let d4 = s.snippet(t_voc, vstreet_cut, cycle + 12.75, 3.0);
    s.set(d4, "position", 0.26); // "can't let go..."
    s.set(d4, "pitch", -2.0);
    s.set(d4, "speed", 0.92);
    s.set(d4, "gain", 0.78);
    s.transform(d4, 0.14, 0.02, 1.0, 0.0);
}

// --- OUTRO (144..160): Final fading vocal sigh into the rain ---
let v_out = s.snippet(t_voc, vbench_cut, 145.0, 9.0);
s.set(v_out, "position", 0.01);
s.set(v_out, "pitch", 0.0);
s.set(v_out, "speed", 0.78);
s.set(v_out, "gain", 0.62);
s.set(v_out, "envelope", 0.45);
s.transform(v_out, 0.0, 0.04, 1.1, 0.0);

let voc_dl = s.track_effect(t_voc, "delay");
s.track_fx(t_voc, voc_dl, "time", 0.336);
s.track_fx(t_voc, voc_dl, "feedback", 0.44);
s.track_fx(t_voc, voc_dl, "mix", 0.30);
s.track_fx(t_voc, voc_dl, "shift_x", 0.035);
s.track_fx(t_voc, voc_dl, "shift_y", -0.01);

let voc_rv = s.track_effect(t_voc, "reverb");
s.track_fx(t_voc, voc_rv, "size", 0.76);
s.track_fx(t_voc, voc_rv, "damp", 0.58);
s.track_fx(t_voc, voc_rv, "mix", 0.32);

// ============================================================================
// MASTER BUS: Subtle nocturnal glue & DCT film grit
// ============================================================================
let m_comp = s.master_effect("compress");
s.master_fx(m_comp, "quality", 0.82);
s.master_fx(m_comp, "audio", 0.08);
s.master_fx(m_comp, "video", 0.32);

let m_verb = s.master_effect("reverb");
s.master_fx(m_verb, "size", 0.62);
s.master_fx(m_verb, "damp", 0.55);
s.master_fx(m_verb, "mix", 0.12);

s.render(0.0, 160.0, "outputs/261111_burial-demo/v001/261111_chromagrain_burial-demo_v001.mp4");
