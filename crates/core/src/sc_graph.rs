//! A small SuperCollider-style SynthDef builder, so chromagrain's
//! instruments are plain Rust: no sclang, no `.scsyndef` files.
//!
//! [`S`] is a signal: a constant, a control parameter, or a UGen output.
//! Arithmetic on signals adds `BinaryOpUGen`/`UnaryOpUGen` units the way
//! sclang does, and constant-only expressions fold at build time (in f64,
//! like sclang). Each UGen helper takes the same inputs, in the same
//! order, as its SuperCollider class, so `SinOsc.ar(f, 0)` becomes
//! `sin_osc(AR, f, 0.0)`. [`synthdef`] collects the graph into a
//! [`plyphon::SynthDef`], which plyphon (the in-process scsynth) compiles
//! directly.
//!
//! Only UGens plyphon implements belong here (see
//! `vendor/plyphon-unit/src/unit/registry.rs`).

#![allow(dead_code)]

use plyphon::{InputRef, Param, Rate, SynthDef, UnitSpec};
use std::cell::RefCell;
use std::ops::{Add, Div, Mul, Neg, Sub};

pub const AR: Rate = Rate::Audio;
pub const KR: Rate = Rate::Control;
pub const IR: Rate = Rate::Scalar;

#[derive(Clone, Copy, Debug)]
enum Src {
    C(f64),
    P(u32),
    U(u32, u32),
}

/// A signal in the graph being built.
#[derive(Clone, Copy, Debug)]
pub struct S {
    src: Src,
    rate: Rate,
}

impl From<f64> for S {
    fn from(v: f64) -> S {
        S { src: Src::C(v), rate: IR }
    }
}
impl From<f32> for S {
    fn from(v: f32) -> S {
        S::from(v as f64)
    }
}
impl From<i32> for S {
    fn from(v: i32) -> S {
        S::from(v as f64)
    }
}

thread_local! {
    static UNITS: RefCell<Vec<UnitSpec>> = const { RefCell::new(Vec::new()) };
}

fn rank(r: Rate) -> u8 {
    match r {
        Rate::Scalar => 0,
        Rate::Control => 1,
        Rate::Audio => 2,
        Rate::Demand => 3,
    }
}

fn max_rate(a: Rate, b: Rate) -> Rate {
    if rank(a) >= rank(b) { a } else { b }
}

fn input(s: S) -> InputRef {
    match s.src {
        Src::C(v) => InputRef::Constant(v as f32),
        Src::P(i) => InputRef::Param(i),
        Src::U(unit, output) => InputRef::Unit { unit, output },
    }
}

fn push(name: &str, rate: Rate, inputs: &[S], outs: usize, special: i16) -> u32 {
    UNITS.with(|u| {
        let mut u = u.borrow_mut();
        u.push(UnitSpec { name: name.into(), rate, inputs: inputs.iter().map(|s| input(*s)).collect(), num_outputs: outs, special_index: special });
        (u.len() - 1) as u32
    })
}

/// Any single-output UGen.
pub fn ugen(name: &str, rate: Rate, inputs: &[S]) -> S {
    let u = push(name, rate, inputs, 1, 0);
    S { src: Src::U(u, 0), rate }
}

/// A UGen with `n` outputs.
pub fn ugen_n(name: &str, rate: Rate, inputs: &[S], n: usize) -> Vec<S> {
    let u = push(name, rate, inputs, n, 0);
    (0..n as u32).map(|o| S { src: Src::U(u, o), rate }).collect()
}

/// A UGen used only for its side effect (Out, DetectSilence, a freeing EnvGen).
pub fn effect(name: &str, rate: Rate, inputs: &[S], outs: usize) {
    push(name, rate, inputs, outs, 0);
}

/// The control parameters of the def being built, by name.
pub struct Ctl(Vec<&'static str>);

impl Ctl {
    pub fn get(&self, name: &str) -> S {
        let i = self.0.iter().position(|n| *n == name).unwrap_or_else(|| panic!("no control '{name}'"));
        S { src: Src::P(i as u32), rate: KR }
    }
}

/// Build a SynthDef: `params` are (name, default) in order; `body` wires
/// the graph (and must end in an `out`).
pub fn synthdef(name: &str, params: &[(&'static str, f32)], body: impl FnOnce(&Ctl)) -> SynthDef {
    UNITS.with(|u| u.borrow_mut().clear());
    body(&Ctl(params.iter().map(|p| p.0).collect()));
    let units = UNITS.with(|u| std::mem::take(&mut *u.borrow_mut()));
    SynthDef { name: name.into(), params: params.iter().map(|(n, d)| Param::control(*n, *d)).collect(), units }
}

// ------------------------------------------------------------ operators

fn binop(sel: i16, a: S, b: S, f: fn(f64, f64) -> f64) -> S {
    if let (Src::C(x), Src::C(y)) = (a.src, b.src) {
        return f(x, y).into();
    }
    // sclang's identities (BinaryOpUGen.new1).
    let is = |s: S, v: f64| matches!(s.src, Src::C(c) if c == v);
    match sel {
        0 if is(a, 0.0) => return b,
        0 | 1 if is(b, 0.0) => return a,
        1 if is(a, 0.0) => return -b,
        2 if is(a, 0.0) || is(b, 0.0) => return 0.0.into(),
        2 if is(a, 1.0) => return b,
        2 if is(a, -1.0) => return -b,
        2 | 4 if is(b, 1.0) => return a,
        2 | 4 if is(b, -1.0) => return -a,
        _ => {}
    }
    let rate = max_rate(a.rate, b.rate);
    let u = push("BinaryOpUGen", rate, &[a, b], 1, sel);
    S { src: Src::U(u, 0), rate }
}

fn unop(sel: i16, a: S, f: fn(f64) -> f64) -> S {
    if let Src::C(x) = a.src {
        return f(x).into();
    }
    let u = push("UnaryOpUGen", a.rate, &[a], 1, sel);
    S { src: Src::U(u, 0), rate: a.rate }
}

macro_rules! op {
    ($tr:ident, $m:ident, $sel:expr, $f:expr) => {
        impl<T: Into<S>> $tr<T> for S {
            type Output = S;
            fn $m(self, b: T) -> S {
                binop($sel, self, b.into(), $f)
            }
        }
        impl $tr<S> for f64 {
            type Output = S;
            fn $m(self, b: S) -> S {
                binop($sel, self.into(), b, $f)
            }
        }
    };
}
op!(Add, add, 0, |a, b| a + b);
op!(Sub, sub, 1, |a, b| a - b);
op!(Mul, mul, 2, |a, b| a * b);
op!(Div, div, 4, |a, b| a / b);

impl Neg for S {
    type Output = S;
    fn neg(self) -> S {
        unop(0, self, |a| -a)
    }
}

fn fold2(a: f64, b: f64) -> f64 {
    // SuperCollider's sc_fold(a, -b, b).
    let (lo, hi) = (-b, b);
    if a >= lo && a < hi {
        return a;
    }
    let range = hi - lo;
    let range2 = range * 2.0;
    let mut c = (a - lo) % range2;
    if c < 0.0 {
        c += range2;
    }
    if c >= range { range2 - c + lo } else { c + lo }
}

impl S {
    pub fn abs(self) -> S {
        unop(5, self, f64::abs)
    }
    pub fn floor(self) -> S {
        unop(9, self, f64::floor)
    }
    pub fn squared(self) -> S {
        unop(12, self, |a| a * a)
    }
    pub fn reciprocal(self) -> S {
        unop(16, self, |a| 1.0 / a)
    }
    pub fn midiratio(self) -> S {
        unop(19, self, |a| (a / 12.0).exp2())
    }
    pub fn tanh(self) -> S {
        unop(36, self, f64::tanh)
    }
    pub fn min(self, b: impl Into<S>) -> S {
        binop(12, self, b.into(), f64::min)
    }
    pub fn max(self, b: impl Into<S>) -> S {
        binop(13, self, b.into(), f64::max)
    }
    pub fn round(self, q: impl Into<S>) -> S {
        binop(19, self, q.into(), |a, q| if q == 0.0 { a } else { (a / q + 0.5).floor() * q })
    }
    /// Signed power, like SuperCollider's `**`.
    pub fn pow(self, b: impl Into<S>) -> S {
        binop(25, self, b.into(), |a, b| if a < 0.0 { -(-a).powf(b) } else { a.powf(b) })
    }
    pub fn clip2(self, b: impl Into<S>) -> S {
        binop(42, self, b.into(), |a, b| a.clamp(-b, b))
    }
    pub fn excess(self, b: impl Into<S>) -> S {
        binop(43, self, b.into(), |a, b| a - a.clamp(-b, b))
    }
    pub fn fold2(self, b: impl Into<S>) -> S {
        binop(44, self, b.into(), fold2)
    }
    /// `Clip` UGen at this signal's rate (folds for constants).
    pub fn clip(self, lo: impl Into<S>, hi: impl Into<S>) -> S {
        let (lo, hi) = (lo.into(), hi.into());
        if let (Src::C(x), Src::C(l), Src::C(h)) = (self.src, lo.src, hi.src) {
            return x.clamp(l, h).into();
        }
        ugen("Clip", self.rate, &[self, lo, hi])
    }
    /// `range` of a bipolar signal: maps -1..1 to lo..hi (sclang's MulAdd form).
    pub fn range(self, lo: impl Into<S>, hi: impl Into<S>) -> S {
        let (lo, hi) = (lo.into(), hi.into());
        let mul = (hi - lo) * 0.5;
        let add = mul + lo;
        self * mul + add
    }
    /// `exprange` of a bipolar signal: `LinExp(this, -1, 1, lo, hi)`.
    pub fn exprange(self, lo: impl Into<S>, hi: impl Into<S>) -> S {
        ugen("LinExp", self.rate, &[self, (-1.0).into(), 1.0.into(), lo.into(), hi.into()])
    }
}

/// Sum of signals, left to right (sclang's `Array.sum`).
pub fn sum(xs: &[S]) -> S {
    xs.iter().skip(1).fold(xs[0], |a, b| a + *b)
}

// ------------------------------------------------------------ envelopes

/// A segment shape: linear, or SuperCollider's numeric curve.
#[derive(Clone, Copy, Debug)]
pub enum Curve {
    Lin,
    C(f64),
}

/// SuperCollider's `Env`.
#[derive(Clone, Debug)]
pub struct Env {
    levels: Vec<S>,
    times: Vec<S>,
    curves: Vec<Curve>,
    release: Option<usize>,
}

impl Env {
    /// `Env(levels, times, curve)`; one curve applies to every segment.
    pub fn new(levels: Vec<S>, times: Vec<S>, curves: &[Curve]) -> Env {
        let n = times.len();
        let curves = (0..n).map(|i| curves[i.min(curves.len() - 1)]).collect();
        Env { levels, times, curves, release: None }
    }
    pub fn lin(levels: Vec<S>, times: Vec<S>) -> Env {
        Env::new(levels, times, &[Curve::Lin])
    }
    pub fn perc(attack: impl Into<S>, release: impl Into<S>, level: impl Into<S>, curve: f64) -> Env {
        Env::new(vec![0.0.into(), level.into(), 0.0.into()], vec![attack.into(), release.into()], &[Curve::C(curve)])
    }
    /// `Env.adsr(a, d, s, r, peakLevel: 1, curve)`.
    pub fn adsr(a: impl Into<S>, d: impl Into<S>, s: impl Into<S>, r: impl Into<S>, curves: &[Curve]) -> Env {
        let mut e = Env::new(vec![0.0.into(), 1.0.into(), s.into() * 1.0, 0.0.into()], vec![a.into(), d.into(), r.into()], curves);
        e.release = Some(2);
        e
    }
    pub fn asr(a: impl Into<S>, s: impl Into<S>, r: impl Into<S>, curve: f64) -> Env {
        let mut e = Env::new(vec![0.0.into(), s.into(), 0.0.into()], vec![a.into(), r.into()], &[Curve::C(curve)]);
        e.release = Some(1);
        e
    }
}

/// `EnvGen(env, gate, levelScale 1, levelBias 0, timeScale 1, doneAction)`.
pub fn env_gen(rate: Rate, env: &Env, gate: impl Into<S>, done_action: f64) -> S {
    let mut ins: Vec<S> = vec![gate.into(), 1.0.into(), 0.0.into(), 1.0.into(), done_action.into()];
    ins.push(env.levels[0]);
    ins.push((env.times.len() as f64).into());
    ins.push(env.release.map(|r| r as f64).unwrap_or(-99.0).into());
    ins.push((-99.0).into());
    for i in 0..env.times.len() {
        ins.push(env.levels[i + 1]);
        ins.push(env.times[i]);
        match env.curves[i] {
            Curve::Lin => {
                ins.push(1.0.into());
                ins.push(0.0.into());
            }
            Curve::C(c) => {
                ins.push(5.0.into());
                ins.push(c.into());
            }
        }
    }
    ugen("EnvGen", rate, &ins)
}

// ------------------------------------------------------------ UGens
// Argument order follows the SuperCollider classes (minus mul/add).

macro_rules! src0 {
    ($f:ident, $n:expr) => {
        pub fn $f(r: Rate) -> S {
            ugen($n, r, &[])
        }
    };
}
macro_rules! src1 {
    ($f:ident, $n:expr) => {
        pub fn $f(r: Rate, a: impl Into<S>) -> S {
            ugen($n, r, &[a.into()])
        }
    };
}
macro_rules! src2 {
    ($f:ident, $n:expr) => {
        pub fn $f(r: Rate, a: impl Into<S>, b: impl Into<S>) -> S {
            ugen($n, r, &[a.into(), b.into()])
        }
    };
}
macro_rules! src3 {
    ($f:ident, $n:expr) => {
        pub fn $f(r: Rate, a: impl Into<S>, b: impl Into<S>, c: impl Into<S>) -> S {
            ugen($n, r, &[a.into(), b.into(), c.into()])
        }
    };
}
macro_rules! flt1 {
    ($f:ident, $n:expr) => {
        pub fn $f(input: S, a: impl Into<S>) -> S {
            ugen($n, AR, &[input, a.into()])
        }
    };
}
macro_rules! flt2 {
    ($f:ident, $n:expr) => {
        pub fn $f(input: S, a: impl Into<S>, b: impl Into<S>) -> S {
            ugen($n, AR, &[input, a.into(), b.into()])
        }
    };
}
macro_rules! flt3 {
    ($f:ident, $n:expr) => {
        pub fn $f(input: S, a: impl Into<S>, b: impl Into<S>, c: impl Into<S>) -> S {
            ugen($n, AR, &[input, a.into(), b.into(), c.into()])
        }
    };
}

src0!(white_noise, "WhiteNoise");
src0!(pink_noise, "PinkNoise");
src1!(lf_noise1, "LFNoise1");
src1!(lf_noise2, "LFNoise2");
src1!(lfd_noise3, "LFDNoise3");
src1!(dust, "Dust");
src1!(saw, "Saw");
src1!(dc, "DC");
src2!(sin_osc, "SinOsc");
src2!(sin_osc_fb, "SinOscFB");
src2!(pulse, "Pulse");
src2!(lf_tri, "LFTri");
src2!(blip, "Blip");
src2!(impulse, "Impulse");
src2!(rand, "Rand");
src3!(var_saw, "VarSaw");
src3!(formant, "Formant");
src3!(t_rand, "TRand");
src3!(ti_rand, "TIRand");

flt1!(lpf, "LPF");
flt1!(hpf, "HPF");
flt1!(leak_dc, "LeakDC");
flt1!(decay, "Decay");
flt1!(latch, "Latch");
flt2!(bpf, "BPF");
flt2!(rlpf, "RLPF");
flt2!(ringz, "Ringz");
flt2!(delay_c, "DelayC");
flt2!(decay2, "Decay2");
flt3!(comb_c, "CombC");
flt3!(allpass_c, "AllpassC");

pub fn moog_ff(input: S, freq: impl Into<S>, gain: impl Into<S>) -> S {
    ugen("MoogFF", AR, &[input, freq.into(), gain.into(), 0.0.into()])
}
pub fn lag_kr(input: S, time: impl Into<S>) -> S {
    ugen("Lag", KR, &[input, time.into()])
}
pub fn pluck(input: S, trig: S, maxdelay: f64, delay: impl Into<S>, decay: impl Into<S>, coef: impl Into<S>) -> S {
    ugen("Pluck", AR, &[input, trig, maxdelay.into(), delay.into(), decay.into(), coef.into()])
}
pub fn select(r: Rate, which: S, xs: &[S]) -> S {
    let mut ins = vec![which];
    ins.extend_from_slice(xs);
    ugen("Select", r, &ins)
}
pub fn xfade2(a: S, b: S, pan: impl Into<S>) -> S {
    ugen("XFade2", AR, &[a, b, pan.into(), 1.0.into()])
}
pub fn pan2(input: S, pos: impl Into<S>) -> [S; 2] {
    let v = ugen_n("Pan2", AR, &[input, pos.into(), 1.0.into()], 2);
    [v[0], v[1]]
}
pub fn balance2(l: S, r: S, pos: impl Into<S>, level: impl Into<S>) -> [S; 2] {
    let v = ugen_n("Balance2", AR, &[l, r, pos.into(), level.into()], 2);
    [v[0], v[1]]
}
/// `GrainFM.ar(2, trig, dur, carfreq, modfreq, index, pan, envbuf -1, maxGrains)`.
pub fn grain_fm2(trig: S, dur: S, car: S, modf: S, index: S, pan: S, max_grains: f64) -> [S; 2] {
    let v = ugen_n("GrainFM", AR, &[trig, dur, car, modf, index, pan, (-1.0).into(), max_grains.into()], 2);
    [v[0], v[1]]
}
pub fn detect_silence(input: S, amp: f64, time: f64, done_action: f64) {
    push("DetectSilence", AR, &[input, amp.into(), time.into(), done_action.into()], 1, 0);
}
/// `Out.ar(bus, [l, r])`.
pub fn out(bus: S, chans: [S; 2]) {
    effect("Out", AR, &[bus, chans[0], chans[1]], 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_fold_and_ops_build_units() {
        let def = synthdef("t", &[("out", 0.0), ("freq", 220.0)], |c| {
            let k = S::from(0.5) * 2.0 + 1.0; // folds to 2
            let sig = sin_osc(AR, c.get("freq") * k, 0.0) * 1.0; // * 1 is dropped
            out(c.get("out"), pan2(sig.tanh(), 0.0));
        });
        let names: Vec<_> = def.units.iter().map(|u| (u.name.as_str(), u.special_index)).collect();
        assert_eq!(names, [("BinaryOpUGen", 2), ("SinOsc", 0), ("UnaryOpUGen", 36), ("Pan2", 0), ("Out", 0)]);
        assert!(matches!(def.units[0].inputs[1], InputRef::Constant(v) if v == 2.0));
        assert_eq!(def.params[1].name, "freq");
    }

    #[test]
    fn env_layout_matches_sclang() {
        // Env.perc(0.01, 1, 1, -4) as sclang writes it.
        let def = synthdef("t", &[("out", 0.0)], |c| {
            let e = env_gen(KR, &Env::perc(0.01, 1.0, 1.0, -4.0), 1.0, 2.0);
            out(c.get("out"), [e, e]);
        });
        let ins: Vec<f32> = def.units[0].inputs.iter().map(|i| match i {
            InputRef::Constant(v) => *v,
            _ => f32::NAN,
        }).collect();
        assert_eq!(ins, [1.0, 1.0, 0.0, 1.0, 2.0, 0.0, 2.0, -99.0, -99.0, 1.0, 0.01, 5.0, -4.0, 0.0, 1.0, 5.0, -4.0]);
    }
}
