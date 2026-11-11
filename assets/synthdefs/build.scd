// chromagrain instrument library — SuperCollider SynthDefs.
//
// Compiled by sclang into one .scsyndef per def (committed next to this
// file) and run inside chromagrain by plyphon (a Rust scsynth). Rebuild:
//   /Applications/SuperCollider.app/Contents/MacOS/sclang assets/synthdefs/build.scd
// (runs headless from a shell; no flags needed.)
//
// Conventions every def follows (the Rust side relies on them):
//   out, freq, amp, pan, gate  — always present
//   amp arrives as velocity * 0.5, so `amp * 2` is velocity 0..1
//   sustained instruments free themselves after gate -> 0 (doneAction 2)
//   percussion ignores gate and frees after its decay
//   other controls are the knobs (ranges/labels live in crates/core/src/sc.rs)
// Only UGens plyphon implements are used (no Linen, Vibrato, IRand, ...).
// Renders are loudness-normalised afterwards, so internal levels only set
// the balance between layers.
//
// Voicing notes (v001 re-voice, see outputs/261111_sound-revoice/):
// chromagrain is a video sampler first, so the voices are built to sit
// under or against footage: drums with real transients, a bass with
// harmonic bite that cuts through dialogue, and two textures (tape drone,
// grain cloud) meant as beds. Everything gets a little instability (drift,
// wow, flutter, grit) so nothing sounds like a preset.

(
var dir = thisProcess.nowExecutingPath.dirname;

// Tape transport: slow wow + fast flutter through a modulated delay.
// depth 1 ~ +-10 cents of wow; call once per channel for stereo smear.
var tape = { |sig, depth = 1, seed = 0|
    var wow = SinOsc.kr(0.5 + (seed * 0.07) + LFNoise1.kr(0.13).range(-0.08, 0.08), seed) * 0.0016;
    var flutter = (LFNoise2.kr(6.5 + seed) * 0.00008) + (SinOsc.kr(11.3 + seed) * 0.00003);
    DelayC.ar(sig, 0.03, 0.006 + ((wow + flutter) * depth))
};

// ---------------------------------------------------------------- pad
// GRAIN CLOUD. A granular pad for a granular sampler: every note is a
// swarm of tiny FM grains scattered across octaves and the stereo field,
// over a soft drifting bed. Glints and shimmers instead of a saw wall.
SynthDef(\cg_pad, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    attack=0.9, release=2.5, cutoff=1800, detune=0.18, width=0.8, air=0.3|
    var env, fenv, trig, oct, gf, dur, idx, grains, bed, sig, cf, hiss;
    env = EnvGen.kr(Env.adsr(attack, 0.5, 0.85, release, curve: -2), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.adsr(attack * 1.4, 1.2, 0.7, release), gate);
    // Grains: density rises with air; each picks note / +oct / +fifth+oct / +2 oct.
    trig = Dust.ar(14 + (air * 30)) + Impulse.ar(6);
    oct = Select.ar(TIRand.ar(0, 4, trig), [DC.ar(1), DC.ar(2), DC.ar(3), DC.ar(4), DC.ar(2)]);
    gf = (freq * oct * TRand.ar(detune.neg, detune, trig).midiratio).clip(40, 9000);
    dur = TRand.ar(0.06, 0.28, trig);
    idx = TRand.ar(0.0, 1.6, trig) * (0.4 + (fenv * 0.6));
    grains = GrainFM.ar(2, trig, dur, gf, gf * 2.0, idx, TRand.ar(width.neg, width, trig), -1, 256) * 0.5;
    // Bed: two drifting pulse-width saws, filtered dark, so low notes still hold.
    bed = VarSaw.ar(freq * [1, 1.0035] * LFNoise1.kr(0.15 ! 2).range(0.998, 1.002), [0, 0.5],
        LFNoise1.kr(0.2 ! 2).range(0.2, 0.8)) * 0.16;
    bed = LPF.ar(bed, (freq * 3).clip(200, 3000));
    sig = grains + bed;
    cf = (cutoff * (0.4 + (fenv * 0.6))).clip(120, 16000);
    sig = LPF.ar(sig, cf);
    // Diffusion: two allpasses smear grains into a cloud without a reverb tail.
    sig = AllpassC.ar(sig, 0.05, [0.031, 0.037], 0.6);
    sig = AllpassC.ar(sig, 0.05, [0.019, 0.023], 0.4);
    hiss = HPF.ar(PinkNoise.ar(0.03 ! 2), 5000) * air;
    sig = [tape.(sig[0], 0.6, 0), tape.(sig[1], 0.6, 1)] + (hiss * env);
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- keys
// CASSETTE KEYS. An FM tine piano bounced to a worn cassette: wow and
// flutter (the `tremolo` knob is the wobble), soft tape saturation,
// rolled-off top, and a whisper of hiss. Video-essay and lo-fi beds.
SynthDef(\cg_keys, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    bell=0.5, tone=0.5, decay=2.5, tremolo=0.25|
    var env, ienv, mod, car, tine, sig, f, vel;
    vel = (amp * 2).clip(0, 1);
    f = freq * Rand(-0.06, 0.06).midiratio;  // each key a little out of tune
    env = EnvGen.kr(Env.adsr(0.003, decay, 0.22, 0.45, curve: -4), gate, doneAction: 2);
    ienv = EnvGen.kr(Env.perc(0.001, 0.5 + (tone * 1.2), 1, -6));
    mod = SinOsc.ar(f) * f * (0.5 + (tone * 2.2) + (vel * 0.8)) * ienv;
    car = SinOsc.ar(f + mod);
    tine = SinOsc.ar(f * 14.0 + (SinOsc.ar(f * 3) * f)) * EnvGen.kr(Env.perc(0.001, 0.1)) * bell * 0.22;
    sig = (car + tine + (SinOsc.ar(f * 2) * 0.1 * ienv)) * env;
    // Bark: harder hits push into a tube-ish asymmetric clip.
    sig = ((sig * (1.2 + vel)) + 0.08).tanh - 0.08.tanh;
    sig = LeakDC.ar(sig);
    // Cassette: two transports for L/R so the wobble is also a width.
    sig = [tape.(sig, 0.4 + (tremolo * 3.2), 0), tape.(sig, 0.4 + (tremolo * 3.2), 2)];
    sig = LPF.ar(sig, 6500) + (HPF.ar(PinkNoise.ar(0.006 ! 2), 3000) * env);
    sig = sig * amp * 0.6;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- pluck
// PREPARED STRING. Karplus-Strong string with a bolt wedged between the
// strings: inharmonic metal modes ring with each pluck, and a sitar-style
// bridge buzz (jawari) rides the peaks. `body` sets how prepared it is.
SynthDef(\cg_pluck, { |out=0, freq=440, amp=0.3, pan=0, gate=1,
    decay=3.0, color=0.5, body=0.55|
    var exc, sig, del, bolt, buzz, soft, hard;
    del = freq.reciprocal;
    soft = LPF.ar(WhiteNoise.ar, 900) + (SinOsc.ar(freq) * 0.6);
    hard = HPF.ar(WhiteNoise.ar, 1200) + (Decay.ar(Impulse.ar(0), 0.0004) * 2);
    exc = ((soft * (1 - color)) + (hard * color)) * EnvGen.ar(Env.perc(0.0005, 0.012 + ((1 - color) * 0.02)));
    sig = Pluck.ar(exc, Impulse.ar(0), 0.05, del, decay, (0.65 - (color * 0.55)).clip(0.05, 0.9));
    // The bolt: free-bar modes, short, slightly sharp of the string.
    bolt = Ringz.ar(exc * 0.15, freq * [2.756, 5.404, 8.933], [0.35, 0.22, 0.12] * (0.5 + body), [1, 0.6, 0.35]).sum;
    // Jawari: only the peaks above a threshold get through — a buzz that follows the string.
    buzz = LPF.ar(HPF.ar(sig.excess(0.12), freq * 2), 5000) * 2;
    sig = sig + (bolt * body) + (buzz * body * 0.6);
    sig = LeakDC.ar(sig) * EnvGen.kr(Env.asr(0, 1, 0.3), gate, doneAction: 2);
    DetectSilence.ar(sig, 0.0001, 0.2, doneAction: 2);
    sig = [sig, DelayC.ar(sig, 0.01, 0.0031)];
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan, amp * 1.3));
}).writeDefFile(dir);

// ---------------------------------------------------------------- bass
// FOLD BASS. West-coast: a sine core run through a wavefolder whose depth
// rides an envelope and the velocity — accents bark, ghost notes stay
// round — then a Moog ladder, with a clean sine underneath for weight
// and a few cents of analog drift. Cuts through dialogue on small speakers.
SynthDef(\cg_bass, { |out=0, freq=55, amp=0.4, pan=0, gate=1,
    cutoff=700, res=1.6, drive=0.5, sub=0.6, glide=0.0|
    var f, vel, env, fenv, core, fold, sig;
    vel = (amp * 2).clip(0, 1);
    f = Lag.kr(freq, glide) * LFNoise1.kr(0.4).range(-0.07, 0.07).midiratio;
    env = EnvGen.kr(Env.adsr(0.002, 0.35, 0.8, 0.07), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.perc(0.001, 0.22, 1, -4));
    core = SinOsc.ar(f) + (LFTri.ar(f * 2.003) * 0.15);
    fold = (core * (1 + (drive * 10 * (0.35 + (vel * 0.65)) * (0.25 + (fenv * 0.75))))).fold2(1);
    sig = MoogFF.ar(fold, (cutoff * (1 + (fenv * 3 * vel))).clip(40, 14000), res);
    sig = ((sig * 1.6) + 0.1).tanh - 0.1.tanh;   // asymmetric: even harmonics, warmth
    sig = sig + (SinOsc.ar(f) * sub * 0.4) + (SinOsc.ar(f * 0.5) * sub * 0.12);
    sig = LeakDC.ar(sig, 0.9995) * env * amp;
    Out.ar(out, Pan2.ar(sig, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- acid
// ACID FUZZ. 303-style squelch with real accents (loud notes open the
// filter further and push the resonance), then a fuzz-pedal stage
// (asymmetric clip + tone control). `wave` morphs saw to a PWM square.
SynthDef(\cg_acid, { |out=0, freq=110, amp=0.35, pan=0, gate=1,
    cutoff=500, res=0.15, envmod=0.7, decay=0.3, wave=0.0|
    var env, fenv, vel, accent, sig, sq, rq, cf;
    vel = (amp * 2).clip(0, 1);
    accent = (vel - 0.75).max(0) * 4;   // 0 below vel .75, up to 1 at full
    env = EnvGen.kr(Env.adsr(0.002, 0.2, 0.7, 0.04), gate, doneAction: 2);
    fenv = EnvGen.kr(Env.perc(0.001, decay * (1 - (accent * 0.4)), 1, -4));
    sq = Pulse.ar(freq, 0.5 - (fenv * 0.3 * wave));
    sig = XFade2.ar(Saw.ar(freq), sq, wave * 2 - 1);
    cf = (cutoff * (1 + (fenv * envmod * (10 + (accent * 10))))).clip(60, 12000);
    rq = (res * (1 - (accent * 0.5))).clip(0.035, 1);
    sig = RLPF.ar(sig * 1.4, cf, rq).tanh;
    sig = RLPF.ar(sig, cf * 1.2, 0.9);              // second pole pair: steeper, 303-ish
    // Fuzz: an asymmetric hard clip blended with a softer tanh stage.
    sig = (((sig * 5) + 0.2).clip2(0.8) * 0.6) + ((sig * 2).tanh * 0.4);
    sig = LPF.ar(sig, 5500);                         // pedal tone knob
    sig = LeakDC.ar(sig) * env * amp * (1 + (accent * 0.5));
    Out.ar(out, Pan2.ar(sig, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- bell
// GAMELAN METAL. A struck bar: modal resonators (free-bar ratios, warped
// by `ratio`) hit by a mallet whose hardness is `strike`, each mode
// doubled a few Hz apart between left and right so it beats like a
// gamelan pair (ombak). A low hum partial underneath like a gong.
SynthDef(\cg_bell, { |out=0, freq=660, amp=0.25, pan=0, gate=1,
    ratio=3.5, index=3.0, decay=5.0, shimmer=0.4|
    var stretch, base, ratios, amps, decs, exc, hard, l, r, beat, sig;
    stretch = ratio / 3.5;
    base = [1, 2.756, 5.404, 8.933, 13.34];
    ratios = base.collect { |b| 1 + ((b - 1) * stretch) };
    amps = [1, 0.55, 0.35, 0.22, 0.12];
    decs = [1, 0.55, 0.3, 0.18, 0.1] * decay;
    hard = (index / 10).clip(0, 1);
    exc = Impulse.ar(0) + (WhiteNoise.ar * EnvGen.ar(Env.perc(0.0002, 0.003)) * hard);
    exc = LPF.ar(exc, 600 + (hard * 9000)) * (0.4 + hard);
    beat = 0.6 + (shimmer * 4);  // Hz between the pair
    l = Ringz.ar(exc, (freq * ratios).clip(20, 18000), decs, amps).sum;
    r = Ringz.ar(exc, ((freq * ratios) + (beat * [1, 1.3, 1.7, 2.1, 2.5])).clip(20, 18000), decs, amps).sum;
    sig = [l, r] + (SinOsc.ar(freq * 0.5 + [0, beat * 0.5]) * EnvGen.kr(Env.perc(0.02, decay * 0.8)) * 0.18);
    sig = (sig * 0.25 * EnvGen.kr(Env.asr(0, 1, decay * 0.6), gate)).tanh;
    EnvGen.kr(Env([1, 1, 0], [decay + 0.2, 0.05]), doneAction: 2);
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan, amp * 2));
}).writeDefFile(dir);

// ---------------------------------------------------------------- lead
// FORMANT SYNC LEAD. Band-limited formant oscillators (the sound of hard
// sync without the aliasing) whose formant sweeps down on every note —
// the tearing, vocal 'yow' of a sync lead — over a feedback-FM sine an
// octave below for grit. `sync` sets how far the formant sweeps.
SynthDef(\cg_lead, { |out=0, freq=440, amp=0.25, pan=0, gate=1,
    cutoff=4000, sync=0.4, vibrato=0.15, release=0.4|
    var env, senv, vib, f, form, sig, fb;
    env = EnvGen.kr(Env.adsr(0.006, 0.25, 0.75, release), gate, doneAction: 2);
    senv = EnvGen.kr(Env.perc(0.002, 0.4, 1, -3));
    vib = SinOsc.kr(5.3 + LFNoise1.kr(0.8).range(-0.3, 0.3), 0,
        vibrato * 0.35 * EnvGen.kr(Env([0, 0, 1], [0.2, 0.5]))).midiratio;
    f = freq * vib;
    form = f * (1.5 + (sync * 7 * (0.3 + (senv * 0.7))));
    sig = Formant.ar(f * [1, 1.004], form * [1, 1.02], f * 1.5) * 0.6;
    fb = SinOscFB.ar(f * 0.5, 0.4 + (senv * 0.7) + (sync * 0.3)) * 0.45;   // < ~1.5 or it turns to noise
    sig = sig + fb;
    sig = RLPF.ar(sig, (cutoff * (1 + (senv * 0.8))).clip(200, 16000), 0.6);
    sig = (sig * 1.5).tanh;
    sig = LeakDC.ar(sig) * env * amp;
    sig = [sig[0], DelayC.ar(sig[1], 0.02, 0.009)];
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- choir
// SPEAK & SPELL CHOIR. Three jittery glottal buzzes per note through
// moving vowel formants (each singer drifts around `vowel`, so the chord
// talks), then a share of it crushed to an 8 kHz / 7-bit toy-speech chip
// (`grit`). Haunted answering machine more than cathedral.
SynthDef(\cg_choir, { |out=0, freq=220, amp=0.3, pan=0, gate=1,
    vowel=0.2, breath=0.3, attack=0.6, release=1.5, grit=0.4|
    var env, voices, sig, lofi, ftab;
    env = EnvGen.kr(Env.adsr(attack, 0.3, 0.9, release), gate, doneAction: 2);
    ftab = [[800, 400, 270, 450, 325], [1150, 1600, 2140, 800, 700], [2900, 2700, 2950, 2830, 2530]];
    voices = 3.collect { |i|
        var jit = LFNoise1.kr(4 + i).range(-0.12, 0.12).midiratio;
        // Blip is a flat pulse train; a low-pass gives it a glottal roll-off.
        var src = LPF.ar(Blip.ar(freq * [1.0, 1.006, 0.995][i] * jit, 60), 1600) + (PinkNoise.ar * breath * 0.5);
        var v = (vowel * 4 + LFDNoise3.kr(0.25 + (i * 0.11)).range(-0.7, 0.7)).clip(0, 3.999);
        var lo = v.floor, fr = v - lo;
        var fs = ftab.collect { |tab| (Select.kr(lo, tab) * (1 - fr)) + (Select.kr(lo + 1, tab) * fr) };
        var s = (BPF.ar(src, fs[0], 0.09) * 1.0) + (BPF.ar(src, fs[1], 0.08) * 0.55) + (BPF.ar(src, fs[2], 0.07) * 0.3);
        Pan2.ar(s, [-0.6, 0.0, 0.6][i])
    };
    sig = voices.sum * 2.5;
    lofi = Latch.ar(sig, Impulse.ar(8000)).round(1 / 64);
    sig = (sig * (1 - grit)) + (LPF.ar(LPF.ar(lofi, 4200), 4200) * grit);
    sig = LeakDC.ar(sig) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- drone
// TAPE DRONE. A stack of partials that each drift in level and tune on
// their own clock, a resonant filter wandering through them, all played
// off a worn tape loop (wow, flutter, saturation, hiss and dust). Built
// to sit under footage for minutes without repeating.
SynthDef(\cg_drone, { |out=0, freq=55, amp=0.3, pan=0, gate=1,
    motion=0.4, color=0.5, attack=3.0, release=4.0|
    var env, parts, sig, cf, dust, hiss;
    env = EnvGen.kr(Env.adsr(attack, 1, 1, release, curve: [2, -2, -4]), gate, doneAction: 2);
    parts = [1, 2, 3, 4, 5, 6, 7, 9, 11].collect { |h, i|
        var rate = (0.05 + (motion * 0.5)) * (1 + (i * 0.17));
        var lvl = LFDNoise3.kr(rate).range(0, 1).squared;
        var det = (LFDNoise3.kr(rate * 0.7) * (0.04 + (motion * 0.1))).midiratio;
        var osc = SinOsc.ar(freq * h * det, Rand(0, 6.28)) * (1 - color)
            + (VarSaw.ar(freq * h * det, Rand(0, 1), LFDNoise3.kr(0.05).range(0.1, 0.9)) * color * 0.5);
        Pan2.ar(osc * lvl / (h ** 0.7), Rand(-0.8, 0.8))
    };
    sig = parts.sum * 0.6;
    sig = sig + (SinOsc.ar(freq * [1, 1.002]) * 0.2);  // steady root so it never thins out
    cf = (freq * LFDNoise3.kr(0.04 + (motion * 0.1) ! 2).exprange(1.5, 8 + (color * 30))).clip(80, 12000);
    sig = RLPF.ar(sig, cf, 0.18);
    sig = (sig * (1 + color)).tanh;                     // tape saturation
    // Overtone singer: a narrow band wandering up and down the harmonics,
    // like a throat singer over the drone. Movement you can hear on a laptop.
    sig = sig + (BPF.ar(sig, (freq * LFDNoise3.kr(0.06 + (motion * 0.25) ! 2).exprange(4, 20)).clip(100, 9000), 0.05) * 5);
    hiss = HPF.ar(PinkNoise.ar(0.012 ! 2), 2500);
    dust = HPF.ar(Decay2.ar(Dust.ar([2.3, 2.9] * (0.5 + color)), 0.0005, 0.004) * WhiteNoise.ar(0.15 ! 2), 1500);
    sig = sig + hiss + dust;
    sig = [tape.(sig[0], 1 + motion, 0), tape.(sig[1], 1 + motion, 3)];
    sig = LeakDC.ar(sig, 0.9995) * env * amp;
    Out.ar(out, Balance2.ar(sig[0], sig[1], pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- kick
// KICK. Skin, beater and a slight overdrive: a sine that dives from two
// octaves up and sags just under the note as it dies, a tick and a short
// mid knock for the beater (`click`), saturated harder with `punch`.
SynthDef(\cg_kick, { |out=0, freq=50, amp=0.6, pan=0, gate=1,
    punch=0.6, decay=0.45, click=0.3|
    var penv, env, body, knock, tick, sig;
    penv = EnvGen.ar(Env([freq * (3 + (punch * 7)), freq * 1.3, freq, freq * 0.9],
        [0.011, 0.05 + (punch * 0.04), decay], [-5, -4, -1]));
    env = EnvGen.ar(Env([0, 1, 0.7, 0], [0.0006, 0.05, decay], [0, -2, -4]), doneAction: 2);
    body = SinOsc.ar(penv) * env;
    knock = Ringz.ar(Impulse.ar(0), freq * 3.3, 0.045) * 0.3;
    tick = (HPF.ar(WhiteNoise.ar, 2500) * EnvGen.ar(Env.perc(0.0002, 0.005, 1, -6)) * 0.7)
        + (Ringz.ar(Impulse.ar(0), 4200, 0.004) * 0.4);
    sig = body + ((knock + tick) * click * 1.6);
    sig = (sig * (1.3 + (punch * 1.7))).tanh;
    Out.ar(out, Pan2.ar(sig * amp, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- snare
// SNARE. A membrane (five struck drum modes with a pitch drop), a stick
// crack, and wires that rattle *with* the head: noise ring-modulated by
// the membrane plus a comb tuned to the shell. `tone` is the head,
// `snap` the wires and crack.
SynthDef(\cg_snare, { |out=0, freq=190, amp=0.45, pan=0, gate=1,
    tone=0.4, snap=0.7, decay=0.22|
    var bend, exc, head, fund, nenv, noise, wires, crack, sig;
    bend = EnvGen.ar(Env([1.4, 1], [0.02], -4));
    exc = Impulse.ar(0) + (WhiteNoise.ar * EnvGen.ar(Env.perc(0.0003, 0.003)) * 0.6);
    head = Ringz.ar(exc, freq * [1, 1.59, 2.14, 2.30, 2.65] * bend,
        [0.16, 0.09, 0.07, 0.06, 0.05] * (0.5 + tone), [1, 0.6, 0.4, 0.35, 0.25]).sum * 0.45;
    fund = Ringz.ar(exc, freq * bend, 0.12 + (decay * 0.5));
    nenv = EnvGen.ar(Env([0, 1, 0.4, 0], [0.0008, 0.025, decay], [0, -3, -4]));
    noise = WhiteNoise.ar;
    wires = (noise * nenv) + (noise * fund.abs * 1.2);
    wires = (BPF.ar(wires, 4500, 0.9) * 1.3) + (HPF.ar(wires, 7500) * 0.5) + (BPF.ar(wires, 1700, 1.2) * 0.35);
    wires = wires + (CombC.ar(wires, 0.02, (freq * 2).reciprocal, 0.04) * 0.3);
    crack = HPF.ar(Ringz.ar(Impulse.ar(0), 5600, 0.003), 2000) * 0.7;
    sig = (head * (0.3 + tone)) + (wires * snap) + (crack * snap);
    sig = (sig * 1.4).tanh;
    sig = sig * EnvGen.ar(Env([1, 1, 0], [decay + 0.3, 0.03]), doneAction: 2);
    Out.ar(out, Pan2.ar(sig * amp, pan));
}).writeDefFile(dir);

// ---------------------------------------------------------------- hat
// HAT. Six square oscillators at the TR-808's cymbal ratios, ring-
// modulated in pairs for a denser, clangier spectrum, band-passed around
// `freq` (closed vs open hat) with a sizzle of noise. `tone` retunes
// the metal from dark to glassy.
SynthDef(\cg_hat, { |out=0, freq=8000, amp=0.3, pan=0, gate=1,
    decay=0.07, tone=0.6|
    var env, nenv, base, osc, metal, noise, sig;
    env = EnvGen.ar(Env([0, 1, 0.45, 0], [0.0004, 0.01, decay], [0, -4, -5]), doneAction: 2);
    nenv = EnvGen.ar(Env.perc(0.0003, decay * 0.4, 1, -6));
    base = 280 + (tone * 300);
    osc = [1, 1.4827, 1.8003, 2.5461, 2.6305, 3.8968].collect { |r| Pulse.ar(base * r) };
    metal = (osc[0] * osc[1]) + (osc[2] * osc[3]) + (osc[4] * osc[5]) + (osc.sum * 0.3);
    metal = BPF.ar(metal, freq.clip(3000, 14000), 0.7) + (BPF.ar(metal, (freq * 1.45).clip(3000, 16000), 0.5) * 0.6);
    noise = HPF.ar(WhiteNoise.ar, 8000) * nenv;
    sig = HPF.ar((metal * env) + (noise * (0.25 + ((1 - tone) * 0.25))), 6000);
    Out.ar(out, Pan2.ar(sig * amp * 0.7, pan));
}).writeDefFile(dir);

"chromagrain synthdefs written".postln;
0.exit;
)
