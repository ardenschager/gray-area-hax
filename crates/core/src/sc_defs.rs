//! chromagrain's instruments, as Rust SynthDefs (see `sc_graph`).
//!
//! These are SuperCollider SynthDefs in all but syntax: they run on
//! plyphon, the in-process scsynth, and need no sclang and no files.
//! Each def keeps the interface the Rust side relies on:
//!   out, freq, amp, pan, gate  — always present
//!   amp arrives as velocity * 0.5, so `amp * 2` is velocity 0..1
//!   sustained instruments free themselves after gate -> 0 (doneAction 2)
//!   percussion ignores gate and frees after its decay
//!   other controls are the knobs (ranges/labels live in sc.rs)
//! Renders are loudness-normalised afterwards, so internal levels only set
//! the balance between layers.
//!
//! Voicing (v001 re-voice, see outputs/261111_sound-revoice/NOTES.md):
//! chromagrain is a video sampler first, so the voices are built to sit
//! under or against footage: drums with real transients, a bass with
//! harmonic bite that cuts through dialogue, and two textures (tape drone,
//! grain cloud) meant as beds. Everything gets a little instability
//! (drift, wow, flutter, grit) so nothing sounds like a preset.

use crate::sc_graph::*;
use plyphon::SynthDef;

macro_rules! sv {
    ($($x:expr),* $(,)?) => { vec![$(S::from($x)),*] };
}

/// Every def, by name.
pub const NAMES: &[&str] = &[
    "cg_pad", "cg_keys", "cg_pluck", "cg_bass", "cg_acid", "cg_bell", "cg_lead", "cg_choir", "cg_drone", "cg_kick", "cg_snare", "cg_hat",
];

/// Build the SynthDef called `name`.
pub fn build(name: &str) -> Option<SynthDef> {
    Some(match name {
        "cg_pad" => pad(),
        "cg_keys" => keys(),
        "cg_pluck" => pluck_def(),
        "cg_bass" => bass(),
        "cg_acid" => acid(),
        "cg_bell" => bell(),
        "cg_lead" => lead(),
        "cg_choir" => choir(),
        "cg_drone" => drone(),
        "cg_kick" => kick(),
        "cg_snare" => snare(),
        "cg_hat" => hat(),
        _ => return None,
    })
}

const C4: Curve = Curve::C(-4.0);

/// Tape transport: slow wow + fast flutter through a modulated delay.
/// depth 1 ~ +-10 cents of wow; call once per channel for stereo smear.
fn tape(sig: S, depth: S, seed: f64) -> S {
    let wow = sin_osc(KR, (0.5 + seed * 0.07) + lf_noise1(KR, 0.13).range(-0.08, 0.08), seed) * 0.0016;
    let flutter = lf_noise2(KR, 6.5 + seed) * 0.00008 + sin_osc(KR, 11.3 + seed, 0.0) * 0.00003;
    delay_c(sig, 0.03, 0.006 + (wow + flutter) * depth)
}

const fn p(name: &'static str, v: f32) -> (&'static str, f32) {
    (name, v)
}
const BASE: [(&str, f32); 5] = [p("out", 0.0), p("freq", 440.0), p("amp", 0.3), p("pan", 0.0), p("gate", 1.0)];

fn params(freq: f32, amp: f32, knobs: &[(&'static str, f32)]) -> Vec<(&'static str, f32)> {
    let mut v = BASE.to_vec();
    v[1].1 = freq;
    v[2].1 = amp;
    v.extend_from_slice(knobs);
    v
}

// ---------------------------------------------------------------- pad
// GRAIN CLOUD. A granular pad for a granular sampler: every note is a
// swarm of tiny FM grains scattered across octaves and the stereo field,
// over a soft drifting bed. Glints and shimmers instead of a saw wall.
fn pad() -> SynthDef {
    let ps = params(220.0, 0.3, &[p("attack", 0.9), p("release", 2.5), p("cutoff", 1800.0), p("detune", 0.18), p("width", 0.8), p("air", 0.3)]);
    synthdef("cg_pad", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (attack, release, cutoff, detune, width, air) = (c.get("attack"), c.get("release"), c.get("cutoff"), c.get("detune"), c.get("width"), c.get("air"));
        let env = env_gen(KR, &Env::adsr(attack, 0.5, 0.85, release, &[Curve::C(-2.0)]), gate, 2.0);
        let fenv = env_gen(KR, &Env::adsr(attack * 1.4, 1.2, 0.7, release, &[C4]), gate, 0.0);
        // Grains: density rises with air; each picks note / +oct / +fifth+oct / +2 oct.
        let trig = dust(AR, 14.0 + air * 30.0) + impulse(AR, 6.0, 0.0);
        let oct = select(AR, ti_rand(AR, 0.0, 4.0, trig), &[dc(AR, 1.0), dc(AR, 2.0), dc(AR, 3.0), dc(AR, 4.0), dc(AR, 2.0)]);
        let gf = (freq * oct * t_rand(AR, -detune, detune, trig).midiratio()).clip(40.0, 9000.0);
        let dur = t_rand(AR, 0.06, 0.28, trig);
        let idx = t_rand(AR, 0.0, 1.6, trig) * (0.4 + fenv * 0.6);
        let g = grain_fm2(trig, dur, gf, gf * 2.0, idx, t_rand(AR, -width, width, trig), 256.0);
        let cf = (cutoff * (0.4 + fenv * 0.6)).clip(120.0, 16000.0);
        let ch: Vec<S> = (0..2)
            .map(|i| {
                // Bed: two drifting pulse-width saws, filtered dark, so low notes still hold.
                let bf = freq * [1.0, 1.0035][i] * lf_noise1(KR, 0.15).range(0.998, 1.002);
                let bed = var_saw(AR, bf, [0.0, 0.5][i], lf_noise1(KR, 0.2).range(0.2, 0.8)) * 0.16;
                let bed = lpf(bed, (freq * 3.0).clip(200.0, 3000.0));
                let mut s = lpf(g[i] * 0.5 + bed, cf);
                // Diffusion: two allpasses smear grains into a cloud without a reverb tail.
                s = allpass_c(s, 0.05, [0.031, 0.037][i], 0.6);
                s = allpass_c(s, 0.05, [0.019, 0.023][i], 0.4);
                let hiss = hpf(pink_noise(AR) * 0.03, 5000.0) * air;
                s = tape(s, 0.6.into(), i as f64) + hiss * env;
                leak_dc(s, 0.995) * env * amp
            })
            .collect();
        out(c.get("out"), balance2(ch[0], ch[1], c.get("pan"), 1.0));
    })
}

// ---------------------------------------------------------------- keys
// CASSETTE KEYS. An FM tine piano bounced to a worn cassette: wow and
// flutter (the `tremolo` knob is the wobble), soft tape saturation,
// rolled-off top, and a whisper of hiss. Video-essay and lo-fi beds.
fn keys() -> SynthDef {
    let ps = params(220.0, 0.3, &[p("bell", 0.5), p("tone", 0.5), p("decay", 2.5), p("tremolo", 0.25)]);
    synthdef("cg_keys", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (bell, tone, dec, tremolo) = (c.get("bell"), c.get("tone"), c.get("decay"), c.get("tremolo"));
        let vel = (amp * 2.0).clip(0.0, 1.0);
        let f = freq * rand(IR, -0.06, 0.06).midiratio(); // each key a little out of tune
        let env = env_gen(KR, &Env::adsr(0.003, dec, 0.22, 0.45, &[C4]), gate, 2.0);
        let ienv = env_gen(KR, &Env::perc(0.001, 0.5 + tone * 1.2, 1.0, -6.0), 1.0, 0.0);
        let m = sin_osc(AR, f, 0.0) * f * (0.5 + tone * 2.2 + vel * 0.8) * ienv;
        let car = sin_osc(AR, f + m, 0.0);
        let tine = sin_osc(AR, f * 14.0 + sin_osc(AR, f * 3.0, 0.0) * f, 0.0) * env_gen(KR, &Env::perc(0.001, 0.1, 1.0, -4.0), 1.0, 0.0) * bell * 0.22;
        let mut sig = (car + tine + sin_osc(AR, f * 2.0, 0.0) * 0.1 * ienv) * env;
        // Bark: harder hits push into a tube-ish asymmetric clip.
        sig = (sig * (1.2 + vel) + 0.08).tanh() - 0.08f64.tanh();
        sig = leak_dc(sig, 0.995);
        // Cassette: two transports for L/R so the wobble is also a width.
        let ch: Vec<S> = [0.0, 2.0]
            .iter()
            .map(|seed| {
                let t = tape(sig, 0.4 + tremolo * 3.2, *seed);
                (lpf(t, 6500.0) + hpf(pink_noise(AR) * 0.006, 3000.0) * env) * amp * 0.6
            })
            .collect();
        out(c.get("out"), balance2(ch[0], ch[1], c.get("pan"), 1.0));
    })
}

// ---------------------------------------------------------------- pluck
// PREPARED STRING. Karplus-Strong string with a bolt wedged between the
// strings: inharmonic metal modes ring with each pluck, and a sitar-style
// bridge buzz (jawari) rides the peaks. `body` sets how prepared it is.
fn pluck_def() -> SynthDef {
    let ps = params(440.0, 0.3, &[p("decay", 3.0), p("color", 0.5), p("body", 0.55)]);
    synthdef("cg_pluck", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (dec, color, body) = (c.get("decay"), c.get("color"), c.get("body"));
        let del = freq.reciprocal();
        let soft = lpf(white_noise(AR), 900.0) + sin_osc(AR, freq, 0.0) * 0.6;
        let hard = hpf(white_noise(AR), 1200.0) + decay(impulse(AR, 0.0, 0.0), 0.0004) * 2.0;
        let exc = (soft * (1.0 - color) + hard * color) * env_gen(AR, &Env::perc(0.0005, 0.012 + (1.0 - color) * 0.02, 1.0, -4.0), 1.0, 0.0);
        let mut sig = pluck(exc, impulse(AR, 0.0, 0.0), 0.05, del, dec, (0.65 - color * 0.55).clip(0.05, 0.9));
        // The bolt: free-bar modes, short, slightly sharp of the string.
        let e = exc * 0.15;
        let modes = [(2.756, 0.35, 1.0), (5.404, 0.22, 0.6), (8.933, 0.12, 0.35)];
        let bolt = sum(&modes.iter().map(|(r, d, a)| ringz(e, freq * *r, *d * (0.5 + body)) * *a).collect::<Vec<_>>());
        // Jawari: only the peaks above a threshold get through — a buzz that follows the string.
        let buzz = lpf(hpf(sig.excess(0.12), freq * 2.0), 5000.0) * 2.0;
        sig = sig + bolt * body + buzz * body * 0.6;
        sig = leak_dc(sig, 0.995) * env_gen(KR, &Env::asr(0.0, 1.0, 0.3, -4.0), gate, 2.0);
        detect_silence(sig, 0.0001, 0.2, 2.0);
        let r = delay_c(sig, 0.01, 0.0031);
        out(c.get("out"), balance2(sig, r, c.get("pan"), amp * 1.3));
    })
}

// ---------------------------------------------------------------- bass
// FOLD BASS. West-coast: a sine core run through a wavefolder whose depth
// rides an envelope and the velocity — accents bark, ghost notes stay
// round — then a Moog ladder, with a clean sine underneath for weight
// and a few cents of analog drift. Cuts through dialogue on small speakers.
fn bass() -> SynthDef {
    let ps = params(55.0, 0.4, &[p("cutoff", 700.0), p("res", 1.6), p("drive", 0.5), p("sub", 0.6), p("glide", 0.0)]);
    synthdef("cg_bass", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (cutoff, res, drive, sub, glide) = (c.get("cutoff"), c.get("res"), c.get("drive"), c.get("sub"), c.get("glide"));
        let vel = (amp * 2.0).clip(0.0, 1.0);
        let f = lag_kr(freq, glide) * lf_noise1(KR, 0.4).range(-0.07, 0.07).midiratio();
        let env = env_gen(KR, &Env::adsr(0.002, 0.35, 0.8, 0.07, &[C4]), gate, 2.0);
        let fenv = env_gen(KR, &Env::perc(0.001, 0.22, 1.0, -4.0), 1.0, 0.0);
        let core = sin_osc(AR, f, 0.0) + lf_tri(AR, f * 2.003, 0.0) * 0.15;
        let fold = (core * (1.0 + drive * 10.0 * (0.35 + vel * 0.65) * (0.25 + fenv * 0.75))).fold2(1.0);
        let mut sig = moog_ff(fold, (cutoff * (1.0 + fenv * 3.0 * vel)).clip(40.0, 14000.0), res);
        sig = (sig * 1.6 + 0.1).tanh() - 0.1f64.tanh(); // asymmetric: even harmonics, warmth
        sig = sig + sin_osc(AR, f, 0.0) * sub * 0.4 + sin_osc(AR, f * 0.5, 0.0) * sub * 0.12;
        sig = leak_dc(sig, 0.9995) * env * amp;
        out(c.get("out"), pan2(sig, c.get("pan")));
    })
}

// ---------------------------------------------------------------- acid
// ACID FUZZ. 303-style squelch with real accents (loud notes open the
// filter further and push the resonance), then a fuzz-pedal stage
// (asymmetric clip + tone control). `wave` morphs saw to a PWM square.
fn acid() -> SynthDef {
    let ps = params(110.0, 0.35, &[p("cutoff", 500.0), p("res", 0.15), p("envmod", 0.7), p("decay", 0.3), p("wave", 0.0)]);
    synthdef("cg_acid", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (cutoff, res, envmod, dec, wave) = (c.get("cutoff"), c.get("res"), c.get("envmod"), c.get("decay"), c.get("wave"));
        let vel = (amp * 2.0).clip(0.0, 1.0);
        let accent = (vel - 0.75).max(0.0) * 4.0; // 0 below vel .75, up to 1 at full
        let env = env_gen(KR, &Env::adsr(0.002, 0.2, 0.7, 0.04, &[C4]), gate, 2.0);
        let fenv = env_gen(KR, &Env::perc(0.001, dec * (1.0 - accent * 0.4), 1.0, -4.0), 1.0, 0.0);
        let sq = pulse(AR, freq, 0.5 - fenv * 0.3 * wave);
        let mut sig = xfade2(saw(AR, freq), sq, wave * 2.0 - 1.0);
        let cf = (cutoff * (1.0 + fenv * envmod * (10.0 + accent * 10.0))).clip(60.0, 12000.0);
        let rq = (res * (1.0 - accent * 0.5)).clip(0.035, 1.0);
        sig = rlpf(sig * 1.4, cf, rq).tanh();
        sig = rlpf(sig, cf * 1.2, 0.9); // second pole pair: steeper, 303-ish
        // Fuzz: an asymmetric hard clip blended with a softer tanh stage.
        sig = (sig * 5.0 + 0.2).clip2(0.8) * 0.6 + (sig * 2.0).tanh() * 0.4;
        sig = lpf(sig, 5500.0); // pedal tone knob
        sig = leak_dc(sig, 0.995) * env * amp * (1.0 + accent * 0.5);
        out(c.get("out"), pan2(sig, c.get("pan")));
    })
}

// ---------------------------------------------------------------- bell
// GAMELAN METAL. A struck bar: modal resonators (free-bar ratios, warped
// by `ratio`) hit by a mallet whose hardness is `strike`, each mode
// doubled a few Hz apart between left and right so it beats like a
// gamelan pair (ombak). A low hum partial underneath like a gong.
fn bell() -> SynthDef {
    let ps = params(660.0, 0.25, &[p("ratio", 3.5), p("index", 3.0), p("decay", 5.0), p("shimmer", 0.4)]);
    synthdef("cg_bell", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (ratio, index, dec, shimmer) = (c.get("ratio"), c.get("index"), c.get("decay"), c.get("shimmer"));
        let stretch = ratio / 3.5;
        let ratios: Vec<S> = [1.0, 2.756, 5.404, 8.933, 13.34].iter().map(|b| 1.0 + (b - 1.0) * stretch).collect();
        let amps = [1.0, 0.55, 0.35, 0.22, 0.12];
        let decs: Vec<S> = [1.0, 0.55, 0.3, 0.18, 0.1].iter().map(|d| *d * dec).collect();
        let hard = (index / 10.0).clip(0.0, 1.0);
        let mut exc = impulse(AR, 0.0, 0.0) + white_noise(AR) * env_gen(AR, &Env::perc(0.0002, 0.003, 1.0, -4.0), 1.0, 0.0) * hard;
        exc = lpf(exc, 600.0 + hard * 9000.0) * (0.4 + hard);
        let beat = 0.6 + shimmer * 4.0; // Hz between the pair
        let spread = [1.0, 1.3, 1.7, 2.1, 2.5];
        let bank = |right: bool| {
            let rs: Vec<S> = (0..5)
                .map(|i| {
                    let f = if right { freq * ratios[i] + beat * spread[i] } else { freq * ratios[i] };
                    ringz(exc, f.clip(20.0, 18000.0), decs[i]) * amps[i]
                })
                .collect();
            sum(&rs)
        };
        let (l, r) = (bank(false), bank(true));
        let hf = freq * 0.5;
        let henv = env_gen(KR, &Env::perc(0.02, dec * 0.8, 1.0, -4.0), 1.0, 0.0);
        let hum = [sin_osc(AR, hf, 0.0) * henv * 0.18, sin_osc(AR, hf + beat * 0.5, 0.0) * henv * 0.18];
        let genv = env_gen(KR, &Env::asr(0.0, 1.0, dec * 0.6, -4.0), gate, 0.0);
        let sl = ((l + hum[0]) * 0.25 * genv).tanh();
        let sr = ((r + hum[1]) * 0.25 * genv).tanh();
        env_gen(KR, &Env::lin(sv![1.0, 1.0, 0.0], vec![dec + 0.2, 0.05.into()]), 1.0, 2.0);
        out(c.get("out"), balance2(sl, sr, c.get("pan"), amp * 2.0));
    })
}

// ---------------------------------------------------------------- lead
// FORMANT SYNC LEAD. Band-limited formant oscillators (the sound of hard
// sync without the aliasing) whose formant sweeps down on every note —
// the tearing, vocal 'yow' of a sync lead — over a feedback-FM sine an
// octave below for grit. `sync` sets how far the formant sweeps.
fn lead() -> SynthDef {
    let ps = params(440.0, 0.25, &[p("cutoff", 4000.0), p("sync", 0.4), p("vibrato", 0.15), p("release", 0.4)]);
    synthdef("cg_lead", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (cutoff, sync, vibrato, release) = (c.get("cutoff"), c.get("sync"), c.get("vibrato"), c.get("release"));
        let env = env_gen(KR, &Env::adsr(0.006, 0.25, 0.75, release, &[C4]), gate, 2.0);
        let senv = env_gen(KR, &Env::perc(0.002, 0.4, 1.0, -3.0), 1.0, 0.0);
        let vdepth = vibrato * 0.35 * env_gen(KR, &Env::lin(sv![0.0, 0.0, 1.0], sv![0.2, 0.5]), 1.0, 0.0);
        let vib = (sin_osc(KR, 5.3 + lf_noise1(KR, 0.8).range(-0.3, 0.3), 0.0) * vdepth).midiratio();
        let f = freq * vib;
        let form = f * (1.5 + sync * 7.0 * (0.3 + senv * 0.7));
        let bw = f * 1.5;
        let osc = [formant(AR, f, form, bw) * 0.6, formant(AR, f * 1.004, form * 1.02, bw) * 0.6];
        let fb = sin_osc_fb(AR, f * 0.5, 0.4 + senv * 0.7 + sync * 0.3) * 0.45; // < ~1.5 or it turns to noise
        let ch: Vec<S> = osc
            .iter()
            .map(|o| {
                let s = rlpf(*o + fb, (cutoff * (1.0 + senv * 0.8)).clip(200.0, 16000.0), 0.6);
                leak_dc((s * 1.5).tanh(), 0.995) * env * amp
            })
            .collect();
        out(c.get("out"), balance2(ch[0], delay_c(ch[1], 0.02, 0.009), c.get("pan"), 1.0));
    })
}

// ---------------------------------------------------------------- choir
// SPEAK & SPELL CHOIR. Three jittery glottal buzzes per note through
// moving vowel formants (each singer drifts around `vowel`, so the chord
// talks), then a share of it crushed to an 8 kHz / 7-bit toy-speech chip
// (`grit`). Haunted answering machine more than cathedral.
fn choir() -> SynthDef {
    let ps = params(220.0, 0.3, &[p("vowel", 0.2), p("breath", 0.3), p("attack", 0.6), p("release", 1.5), p("grit", 0.4)]);
    synthdef("cg_choir", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (vowel, breath, attack, release, grit) = (c.get("vowel"), c.get("breath"), c.get("attack"), c.get("release"), c.get("grit"));
        let env = env_gen(KR, &Env::adsr(attack, 0.3, 0.9, release, &[C4]), gate, 2.0);
        let ftab = [[800.0, 400.0, 270.0, 450.0, 325.0], [1150.0, 1600.0, 2140.0, 800.0, 700.0], [2900.0, 2700.0, 2950.0, 2830.0, 2530.0]];
        let voices: Vec<[S; 2]> = (0..3)
            .map(|i| {
                let jit = lf_noise1(KR, 4.0 + i as f64).range(-0.12, 0.12).midiratio();
                // Blip is a flat pulse train; a low-pass gives it a glottal roll-off.
                let src = lpf(blip(AR, freq * [1.0, 1.006, 0.995][i] * jit, 60.0), 1600.0) + pink_noise(AR) * breath * 0.5;
                let v = (vowel * 4.0 + lfd_noise3(KR, 0.25 + i as f64 * 0.11).range(-0.7, 0.7)).clip(0.0, 3.999);
                let lo = v.floor();
                let fr = v - lo;
                let fs: Vec<S> = ftab
                    .iter()
                    .map(|tab| {
                        let t: Vec<S> = tab.iter().map(|x| S::from(*x)).collect();
                        select(KR, lo, &t) * (1.0 - fr) + select(KR, lo + 1.0, &t) * fr
                    })
                    .collect();
                let s = bpf(src, fs[0], 0.09) * 1.0 + bpf(src, fs[1], 0.08) * 0.55 + bpf(src, fs[2], 0.07) * 0.3;
                pan2(s, [-0.6, 0.0, 0.6][i])
            })
            .collect();
        let ch: Vec<S> = (0..2)
            .map(|k| {
                let sig = sum(&voices.iter().map(|v| v[k]).collect::<Vec<_>>()) * 2.5;
                let lofi = latch(sig, impulse(AR, 8000.0, 0.0)).round(1.0 / 64.0);
                let s = sig * (1.0 - grit) + lpf(lpf(lofi, 4200.0), 4200.0) * grit;
                leak_dc(s, 0.995) * env * amp
            })
            .collect();
        out(c.get("out"), balance2(ch[0], ch[1], c.get("pan"), 1.0));
    })
}

// ---------------------------------------------------------------- drone
// TAPE DRONE. A stack of partials that each drift in level and tune on
// their own clock, a resonant filter wandering through them, all played
// off a worn tape loop (wow, flutter, saturation, hiss and dust). Built
// to sit under footage for minutes without repeating.
fn drone() -> SynthDef {
    let ps = params(55.0, 0.3, &[p("motion", 0.4), p("color", 0.5), p("attack", 3.0), p("release", 4.0)]);
    synthdef("cg_drone", &ps, |c| {
        let (freq, amp, gate) = (c.get("freq"), c.get("amp"), c.get("gate"));
        let (motion, color, attack, release) = (c.get("motion"), c.get("color"), c.get("attack"), c.get("release"));
        let env = env_gen(KR, &Env::adsr(attack, 1.0, 1.0, release, &[Curve::C(2.0), Curve::C(-2.0), C4]), gate, 2.0);
        let parts: Vec<[S; 2]> = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 9.0, 11.0]
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let rate = (0.05 + motion * 0.5) * (1.0 + i as f64 * 0.17);
                let lvl = lfd_noise3(KR, rate).range(0.0, 1.0).squared();
                let det = (lfd_noise3(KR, rate * 0.7) * (0.04 + motion * 0.1)).midiratio();
                let osc = sin_osc(AR, freq * *h * det, rand(IR, 0.0, 6.28)) * (1.0 - color)
                    + var_saw(AR, freq * *h * det, rand(IR, 0.0, 1.0), lfd_noise3(KR, 0.05).range(0.1, 0.9)) * color * 0.5;
                pan2(osc * lvl / h.powf(0.7), rand(IR, -0.8, 0.8))
            })
            .collect();
        let crate_ = 0.04 + motion * 0.1;
        let chi = 8.0 + color * 30.0;
        let srate = 0.06 + motion * 0.25;
        let ch: Vec<S> = (0..2)
            .map(|k| {
                let mut sig = sum(&parts.iter().map(|v| v[k]).collect::<Vec<_>>()) * 0.6;
                sig = sig + sin_osc(AR, freq * [1.0, 1.002][k], 0.0) * 0.2; // steady root so it never thins out
                let cf = (freq * lfd_noise3(KR, crate_).exprange(1.5, chi)).clip(80.0, 12000.0);
                sig = rlpf(sig, cf, 0.18);
                sig = (sig * (1.0 + color)).tanh(); // tape saturation
                // Overtone singer: a narrow band wandering up and down the harmonics,
                // like a throat singer over the drone. Movement you can hear on a laptop.
                sig = sig + bpf(sig, (freq * lfd_noise3(KR, srate).exprange(4.0, 20.0)).clip(100.0, 9000.0), 0.05) * 5.0;
                let hiss = hpf(pink_noise(AR) * 0.012, 2500.0);
                let specks = hpf(decay2(dust(AR, [2.3, 2.9][k] * (0.5 + color)), 0.0005, 0.004) * (white_noise(AR) * 0.15), 1500.0);
                sig = sig + hiss + specks;
                sig = tape(sig, 1.0 + motion, [0.0, 3.0][k]);
                leak_dc(sig, 0.9995) * env * amp
            })
            .collect();
        out(c.get("out"), balance2(ch[0], ch[1], c.get("pan"), 1.0));
    })
}

// ---------------------------------------------------------------- kick
// KICK. Skin, beater and a slight overdrive: a sine that dives from two
// octaves up and sags just under the note as it dies, a tick and a short
// mid knock for the beater (`click`), saturated harder with `punch`.
fn kick() -> SynthDef {
    let ps = params(50.0, 0.6, &[p("punch", 0.6), p("decay", 0.45), p("click", 0.3)]);
    synthdef("cg_kick", &ps, |c| {
        let (freq, amp) = (c.get("freq"), c.get("amp"));
        let (punch, dec, click) = (c.get("punch"), c.get("decay"), c.get("click"));
        let penv = env_gen(
            AR,
            &Env::new(sv![freq * (3.0 + punch * 7.0), freq * 1.3, freq, freq * 0.9], sv![0.011, 0.05 + punch * 0.04, dec], &[Curve::C(-5.0), Curve::C(-4.0), Curve::C(-1.0)]),
            1.0,
            0.0,
        );
        let env = env_gen(AR, &Env::new(sv![0.0, 1.0, 0.7, 0.0], sv![0.0006, 0.05, dec], &[Curve::C(0.0), Curve::C(-2.0), C4]), 1.0, 2.0);
        let body = sin_osc(AR, penv, 0.0) * env;
        let knock = ringz(impulse(AR, 0.0, 0.0), freq * 3.3, 0.045) * 0.3;
        let tick = hpf(white_noise(AR), 2500.0) * env_gen(AR, &Env::perc(0.0002, 0.005, 1.0, -6.0), 1.0, 0.0) * 0.7 + ringz(impulse(AR, 0.0, 0.0), 4200.0, 0.004) * 0.4;
        let mut sig = body + (knock + tick) * click * 1.6;
        sig = (sig * (1.3 + punch * 1.7)).tanh();
        out(c.get("out"), pan2(sig * amp, c.get("pan")));
    })
}

// ---------------------------------------------------------------- snare
// SNARE. A membrane (five struck drum modes with a pitch drop), a stick
// crack, and wires that rattle *with* the head: noise ring-modulated by
// the membrane plus a comb tuned to the shell. `tone` is the head,
// `snap` the wires and crack.
fn snare() -> SynthDef {
    let ps = params(190.0, 0.45, &[p("tone", 0.4), p("snap", 0.7), p("decay", 0.22)]);
    synthdef("cg_snare", &ps, |c| {
        let (freq, amp) = (c.get("freq"), c.get("amp"));
        let (tone, snap, dec) = (c.get("tone"), c.get("snap"), c.get("decay"));
        let bend = env_gen(AR, &Env::new(sv![1.4, 1.0], sv![0.02], &[C4]), 1.0, 0.0);
        let exc = impulse(AR, 0.0, 0.0) + white_noise(AR) * env_gen(AR, &Env::perc(0.0003, 0.003, 1.0, -4.0), 1.0, 0.0) * 0.6;
        let modes = [(1.0, 0.16, 1.0), (1.59, 0.09, 0.6), (2.14, 0.07, 0.4), (2.30, 0.06, 0.35), (2.65, 0.05, 0.25)];
        let head = sum(&modes.iter().map(|(r, d, a)| ringz(exc, freq * *r * bend, *d * (0.5 + tone)) * *a).collect::<Vec<_>>()) * 0.45;
        let fund = ringz(exc, freq * bend, 0.12 + dec * 0.5);
        let nenv = env_gen(AR, &Env::new(sv![0.0, 1.0, 0.4, 0.0], sv![0.0008, 0.025, dec], &[Curve::C(0.0), Curve::C(-3.0), C4]), 1.0, 0.0);
        let noise = white_noise(AR);
        let mut wires = noise * nenv + noise * fund.abs() * 1.2;
        wires = bpf(wires, 4500.0, 0.9) * 1.3 + hpf(wires, 7500.0) * 0.5 + bpf(wires, 1700.0, 1.2) * 0.35;
        wires = wires + comb_c(wires, 0.02, (freq * 2.0).reciprocal(), 0.04) * 0.3;
        let crack = hpf(ringz(impulse(AR, 0.0, 0.0), 5600.0, 0.003), 2000.0) * 0.7;
        let mut sig = head * (0.3 + tone) + wires * snap + crack * snap;
        sig = (sig * 1.4).tanh();
        sig = sig * env_gen(AR, &Env::lin(sv![1.0, 1.0, 0.0], sv![dec + 0.3, 0.03]), 1.0, 2.0);
        out(c.get("out"), pan2(sig * amp, c.get("pan")));
    })
}

// ---------------------------------------------------------------- hat
// HAT. Six square oscillators at the TR-808's cymbal ratios, ring-
// modulated in pairs for a denser, clangier spectrum, band-passed around
// `freq` (closed vs open hat) with a sizzle of noise. `tone` retunes
// the metal from dark to glassy.
fn hat() -> SynthDef {
    let ps = params(8000.0, 0.3, &[p("decay", 0.07), p("tone", 0.6)]);
    synthdef("cg_hat", &ps, |c| {
        let (freq, amp) = (c.get("freq"), c.get("amp"));
        let (dec, tone) = (c.get("decay"), c.get("tone"));
        let env = env_gen(AR, &Env::new(sv![0.0, 1.0, 0.45, 0.0], sv![0.0004, 0.01, dec], &[Curve::C(0.0), C4, Curve::C(-5.0)]), 1.0, 2.0);
        let nenv = env_gen(AR, &Env::perc(0.0003, dec * 0.4, 1.0, -6.0), 1.0, 0.0);
        let base = 280.0 + tone * 300.0;
        let o: Vec<S> = [1.0, 1.4827, 1.8003, 2.5461, 2.6305, 3.8968].iter().map(|r| pulse(AR, base * *r, 0.5)).collect();
        let mut metal = o[0] * o[1] + o[2] * o[3] + o[4] * o[5] + sum(&o) * 0.3;
        metal = bpf(metal, freq.clip(3000.0, 14000.0), 0.7) + bpf(metal, (freq * 1.45).clip(3000.0, 16000.0), 0.5) * 0.6;
        let noise = hpf(white_noise(AR), 8000.0) * nenv;
        let sig = hpf(metal * env + noise * (0.25 + (1.0 - tone) * 0.25), 6000.0);
        out(c.get("out"), pan2(sig * amp * 0.7, c.get("pan")));
    })
}
