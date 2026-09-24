// chromagrain instrument library — SuperCollider SynthDefs.
//
// Compiled by sclang into chromagrain.scsyndef (committed next to this
// file) and run inside chromagrain by plyphon (a Rust scsynth). Rebuild:
//   /Applications/SuperCollider.app/Contents/MacOS/sclang assets/synthdefs/build.scd
//
// Conventions every def follows (the Rust side relies on them):
//   out, freq, amp, pan, gate  — always present
//   sustained instruments free themselves after gate -> 0 (doneAction 2)
//   percussion ignores gate and frees after its decay
//   other controls are the knobs (ranges/labels live in crates/core/src/sc.rs)
// Only UGens plyphon implements are used (no Linen, Vibrato, IRand, ...).

(
var dir = thisProcess.nowExecutingPath.dirname;

// ---------------------------------------------------------------- pad
// Seven detuned saws, slow filter bloom, gentle chorus. Warm and wide.
SynthDef(\cg_pad, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    attack=0.9, release=2.5, cutoff=1800, detune=0.18, width=0.8, air=0.3|
    var n = 7, env, fenv, sig, cf;
    env = EnvGen.kr(Env.adsr(attack, 0.5, 0.85, release, curve: -2), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.adsr(attack * 1.4, 1.2, 0.7, release), gate);
    sig = Array.fill(n, { |i|
        var d = (i - (n - 1 / 2)) / (n - 1 / 2) * detune;
        Saw.ar(freq * d.midiratio * LFNoise1.kr(0.2).range(0.998, 1.002))
    });
    sig = Splay.ar(sig, width) * 0.35;
    sig = sig + (SinOsc.ar(freq * 0.5) * 0.25 ! 2);
    cf = (cutoff * (0.35 + (fenv * 0.65))).clip(80, 16000);
    sig = LPF.ar(LPF.ar(sig, cf), cf * 1.5);
    sig = sig + (DelayC.ar(sig, 0.03, SinOsc.kr([0.21, 0.27]).range(0.008, 0.022)) * 0.6);
    sig = sig + (HPF.ar(PinkNoise.ar(0.04 ! 2), 6000) * air * env);
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- keys
// FM electric piano: soft tine bell on the attack, tremolo, warm body.
SynthDef(\cg_keys, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    bell=0.5, tone=0.5, decay=2.5, tremolo=0.25|
    var env, ienv, mod, car, tine, sig;
    env = EnvGen.kr(Env.adsr(0.004, decay, 0.25, 0.5, curve: -4), gate, doneAction: 2);
    ienv = EnvGen.kr(Env.perc(0.001, 0.6 + (tone * 1.2), 1, -6));
    mod = SinOsc.ar(freq) * freq * (0.6 + (tone * 2.4)) * ienv;
    car = SinOsc.ar(freq + mod);
    tine = SinOsc.ar(freq * 14.0 + (SinOsc.ar(freq * 3) * freq)) * EnvGen.kr(Env.perc(0.001, 0.12)) * bell * 0.25;
    sig = (car + tine + (SinOsc.ar(freq * 2) * 0.12 * ienv)) * env;
    sig = sig * (1 - (tremolo * 0.5) + (SinOsc.kr(4.6) * tremolo * 0.5));
    sig = LPF.ar(sig, 9000) * amp * 0.6;
    Out.ar(out, Pan2.ar(sig, pan + (SinOsc.kr(4.6, 1.57) * tremolo * 0.4)));
}).writeDefFile(dir);

// ---------------------------------------------------------------- pluck
// Karplus-Strong string, a touch of body resonance and stereo spread.
SynthDef(\cg_pluck, { |out=0, freq=440, amp=0.3, pan=0, gate=1,
    decay=3.0, color=0.5, body=0.4|
    var exc, sig, del;
    del = freq.reciprocal;
    exc = (WhiteNoise.ar * 0.7 + (Saw.ar(freq) * 0.3)) * EnvGen.ar(Env.perc(0.001, 0.02));
    exc = LPF.ar(exc, (1500 + (color * 9000)).clip(200, 16000));
    sig = Pluck.ar(exc, Impulse.ar(0), 0.05, del, decay, (0.7 - (color * 0.6)).clip(0.02, 0.9));
    sig = sig + (BPF.ar(sig, freq * 2.01, 0.4) * body) + (Resonz.ar(sig, 180, 0.8) * body * 2);
    sig = LeakDC.ar(sig) * EnvGen.kr(Env.asr(0, 1, 0.3), gate, doneAction: 2);
    DetectSilence.ar(sig, 0.0001, 0.2, doneAction: 2);
    Out.ar(out, Pan2.ar(sig * amp * 1.3, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- bass
// Sub + saw through a Moog ladder, a little drive, optional glide.
SynthDef(\cg_bass, { |out=0, freq=55, amp=0.4, pan=0, gate=1,
    cutoff=700, res=1.6, drive=0.4, sub=0.6, glide=0.0|
    var f, env, fenv, sig;
    f = Lag.kr(freq, glide);
    env = EnvGen.kr(Env.adsr(0.003, 0.3, 0.8, 0.12), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.perc(0.002, 0.25));
    sig = Saw.ar(f * [1, 1.004]).sum * 0.35 + (Pulse.ar(f * 0.5, 0.5) * 0.12);
    sig = MoogFF.ar(sig, (cutoff * (1 + (fenv * 2))).clip(40, 12000), res);
    sig = (sig * (1 + (drive * 6))).tanh * 0.6;
    sig = sig + (SinOsc.ar(f * 0.5) * sub * 0.5);
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Pan2.ar(sig, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- acid
// 303-ish: squelchy resonant low-pass with envelope mod and accent.
SynthDef(\cg_acid, { |out=0, freq=110, amp=0.35, pan=0, gate=1,
    cutoff=500, res=0.15, envmod=0.7, decay=0.3, wave=0.0|
    var env, fenv, sig;
    env = EnvGen.kr(Env.adsr(0.002, 0.2, 0.7, 0.05), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.perc(0.001, decay, 1, -4));
    sig = XFade2.ar(Saw.ar(freq), Pulse.ar(freq, 0.5), wave * 2 - 1);
    sig = RLPF.ar(sig, (cutoff * (1 + (fenv * envmod * 12))).clip(60, 14000), res.clip(0.04, 1));
    sig = (sig * 1.6).tanh * 0.5;
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Pan2.ar(sig, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- bell
// Inharmonic FM bell with a shimmering stereo tail.
SynthDef(\cg_bell, { |out=0, freq=660, amp=0.25, pan=0, gate=1,
    ratio=3.5, index=3.0, decay=5.0, shimmer=0.4|
    var env, ienv, mod, sig;
    env = EnvGen.kr(Env.perc(0.002, decay, 1, -5));
    ienv = EnvGen.kr(Env.perc(0.001, decay * 0.4, 1, -4));
    mod = SinOsc.ar(freq * ratio) * freq * index * ienv;
    sig = SinOsc.ar(freq + mod) * env;
    sig = sig + (SinOsc.ar(freq * 2.76) * env.squared * 0.2);
    sig = [sig, DelayC.ar(sig, 0.02, SinOsc.kr(0.3).range(0.002, 0.012) * shimmer + 0.001)];
    DetectSilence.ar(sig[0] + env, 0.0001, 0.1, doneAction: 2);
    sig = sig * amp * EnvGen.kr(Env.asr(0, 1, decay), gate);
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- lead
// Supersaw lead: bright, detuned, with a little vibrato.
SynthDef(\cg_lead, { |out=0, freq=440, amp=0.25, pan=0, gate=1,
    cutoff=4000, detune=0.12, vibrato=0.15, release=0.4|
    var n = 7, env, vib, sig;
    env = EnvGen.kr(Env.adsr(0.01, 0.2, 0.8, release), gate, doneAction: 2);
    vib = SinOsc.kr(5.5, 0, vibrato * 0.3 * EnvGen.kr(Env([0, 0, 1], [0.25, 0.4]))).midiratio;
    sig = Array.fill(n, { |i|
        Saw.ar(freq * vib * ((i - 3) / 3 * detune).midiratio, Rand(0, 1))
    });
    sig = Splay.ar(sig, 0.7) * 0.3;
    sig = BLowPass.ar(sig, cutoff.clip(200, 16000), 0.8);
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- choir
// Vowel formants on a breathy buzz; `vowel` morphs a -> e -> i -> o -> u.
SynthDef(\cg_choir, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    vowel=0.2, breath=0.3, attack=0.6, release=1.5|
    var env, src, f1, f2, f3, v, sig;
    env = EnvGen.kr(Env.adsr(attack, 0.3, 0.9, release), gate, doneAction: 2);
    src = Saw.ar(freq * [1, 1.003, 0.997] * LFNoise1.kr(3 ! 3).range(0.997, 1.003)).sum * 0.3
        + (PinkNoise.ar * breath * 0.3);
    v = vowel.clip(0, 1) * 4;
    f1 = Select.kr(v, [800, 400, 270, 450, 325]);
    f2 = Select.kr(v, [1150, 1600, 2140, 800, 700]);
    f3 = Select.kr(v, [2900, 2700, 2950, 2830, 2530]);
    sig = BPF.ar(src, Lag.kr(f1, 0.2), 0.08) * 1.0
        + (BPF.ar(src, Lag.kr(f2, 0.2), 0.07) * 0.6)
        + (BPF.ar(src, Lag.kr(f3, 0.2), 0.06) * 0.35);
    sig = sig * 4 * env * amp;
    sig = [sig, DelayC.ar(sig, 0.03, 0.017)];
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- drone
// Slowly moving resonant noise and sub-harmonics — a bed for video.
SynthDef(\cg_drone, { |out=0, freq=55, amp=0.3, pan=0, gate=1,
    motion=0.4, color=0.5, attack=3.0, release=4.0|
    var env, sig, harm;
    env = EnvGen.kr(Env.adsr(attack, 1, 1, release), gate, doneAction: 2);
    harm = [1, 2, 3, 4.01, 5.02, 6, 8];
    sig = harm.collect { |h, i|
        var lfo = LFDNoise3.kr(motion * 0.3 + (i * 0.03)).range(0.2, 1);
        Resonz.ar(PinkNoise.ar + (Saw.ar(freq) * 0.02), freq * h, 0.004 + (color * 0.01)) * lfo * (1 / (i + 1)).sqrt
    };
    sig = Splay.ar(sig, 1) * 18;
    sig = sig + (SinOsc.ar(freq * [0.5, 0.501]) * 0.25);
    sig = LPF.ar(sig, 400 + (color * 6000));
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- kick
SynthDef(\cg_kick, { |out=0, freq=50, amp=0.6, pan=0, gate=1,
    punch=0.6, decay=0.45, click=0.3|
    var penv, env, sig;
    penv = EnvGen.kr(Env([freq * (4 + (punch * 6)), freq * 1.4, freq], [0.01, 0.12], [-6, -3]));
    env = EnvGen.kr(Env.perc(0.001, decay, 1, -5), doneAction: 2);
    sig = SinOsc.ar(penv, 0.5pi) * env;
    sig = sig + (LPF.ar(WhiteNoise.ar, 6000) * EnvGen.kr(Env.perc(0.0005, 0.008)) * click);
    sig = (sig * 1.4).tanh;
    Out.ar(out, Pan2.ar(sig * amp, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- snare
SynthDef(\cg_snare, { |out=0, freq=190, amp=0.45, pan=0, gate=1,
    tone=0.4, snap=0.7, decay=0.22|
    var tenv, nenv, body, noise, sig;
    tenv = EnvGen.kr(Env.perc(0.001, 0.12));
    nenv = EnvGen.kr(Env.perc(0.001, decay, 1, -4), doneAction: 2);
    body = SinOsc.ar(freq * [1, 1.6] * EnvGen.kr(Env([1.6, 1], [0.03]))).sum * tenv * tone;
    noise = BPF.ar(WhiteNoise.ar, 2500, 0.9) + HPF.ar(WhiteNoise.ar * 0.5, 6000);
    sig = body + (noise * nenv * snap);
    sig = (sig * 1.3).tanh;
    Out.ar(out, Pan2.ar(sig * amp, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- hat
SynthDef(\cg_hat, { |out=0, freq=8000, amp=0.3, pan=0, gate=1,
    decay=0.07, tone=0.6|
    var env, sig;
    env = EnvGen.kr(Env.perc(0.0005, decay, 1, -6), doneAction: 2);
    sig = [1, 1.342, 1.2312, 1.6532, 1.9523, 2.1523].collect { |r| Pulse.ar(330 * r * (1 + tone)) }.sum;
    sig = HPF.ar(BPF.ar(sig, freq.clip(3000, 14000), 0.6), 6000) + (HPF.ar(WhiteNoise.ar, 9000) * 0.4);
    Out.ar(out, Pan2.ar(sig * env * amp * 0.5, pan));
}).writeDefFile(dir);

"chromagrain synthdefs written".postln;
0.exit;
)
