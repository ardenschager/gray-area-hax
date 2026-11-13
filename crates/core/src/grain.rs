//! The heart of chromagrain: a single grain scheduler whose events drive
//! BOTH the audio renderer and the visual compositor. One `GrainEvent` is
//! one audio grain *and* one visual grain — same onset, duration, envelope,
//! playback rate, pan and gain — so what you hear is literally what you see.

use crate::dsp::Rng;
use crate::music::Key;
use serde::{Deserialize, Serialize};

/// User-facing granular parameters on a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrainSettings {
    /// Grains per second.
    pub density: f32,
    /// Grain length in seconds.
    pub duration: f32,
    /// Random variation of grain length (fraction of duration, 0..1).
    pub duration_jitter: f32,
    /// Normalized read position in the source (0..1).
    pub position: f32,
    /// Random spread of the read position, in seconds.
    pub spray: f32,
    /// How fast the read head scans through the source relative to
    /// timeline time (1.0 = realtime, 0.0 = frozen).
    pub scan_speed: f32,
    /// Media playback speed (snippets/stretch): 1.0 = realtime. Changes
    /// duration WITHOUT changing pitch — pitch is the separate param below.
    pub speed: f32,
    /// Pitch offset in semitones (also visual playback rate / hue shift).
    pub pitch: f32,
    /// Random pitch variation in semitones.
    pub pitch_jitter: f32,
    /// Cents offset applied AFTER key quantization (-100..100) — subtle
    /// off-pitch shading that survives the snap.
    pub detune_cents: f32,
    /// Key quantize strength: 1 = hard snap, 0.5 = pulled halfway toward
    /// the scale, 0 = free pitch even with a key set.
    pub quantize_amount: f32,
    /// Linear gain applied per grain.
    pub gain: f32,
    /// Center pan -1..1 (audio position == visual x position).
    pub pan: f32,
    /// Random pan spread around the center, 0..1.
    pub pan_spread: f32,
    /// Envelope shape 0..1 (tukey taper fraction; 1 = hann).
    pub envelope: f32,
    /// Envelope skew -1..1: -1 percussive (instant attack, long tail),
    /// 0 symmetric, +1 reverse swell. Shapes opacity too.
    pub env_skew: f32,
    /// Probability a grain plays in reverse.
    pub reverse_prob: f32,
    /// Beat-sync: when > 0, grain onsets snap to this beat grid (e.g.
    /// 0.25 = 16ths) and `density` becomes the average trigger rate.
    /// 0 = free (classic asynchronous cloud).
    pub sync_div: f32,
    /// Harmony voices per grain (1..4). Voice v plays at
    /// `pitch + v * voice_interval` semitones (then key-quantized).
    pub voices: u32,
    /// Interval between harmony voices, semitones (12 = octaves, 7 = fifths).
    pub voice_interval: f32,
    /// Random detune per voice, semitones.
    pub voice_detune: f32,
    /// RNG seed; same seed -> identical grain cloud.
    pub seed: u64,
    /// Sample <-> grains morph for media clips: 0 = the media plays
    /// straight, 1 = it dissolves into a grain cloud that still follows the
    /// read head. Everything in between is a smooth blend (grain length,
    /// overlap, spray, pitch/pan scatter and patch size all follow it).
    #[serde(default)]
    pub texture: f32,
    /// Shuffle / swing for beat-synced clouds (`sync_div > 0`): 0.5 = straight,
    /// 0.60 = classic 60% shuffle delaying odd grid slots.
    #[serde(default = "default_swing")]
    pub swing: f32,
    /// Turntable / sampler coupling for snippets: when true, `speed`
    /// automatically tracks the pitch ratio (varispeed + formant shift,
    /// single pristine grain with zero OLA time-stretch warble).
    #[serde(default)]
    pub varispeed: bool,
    /// ADSR for the hits / voices these settings make (sound + picture).
    /// None = the classic grain windows only.
    #[serde(default)]
    pub adsr: Option<crate::envelope::Adsr>,
    /// Unopinionated pure-sample voice sculptor & tape-loop physics.
    #[serde(default)]
    pub sculpt: VoiceSculpt,
    /// Optional monophonic choke group (`0` = off, `1..=4`).
    #[serde(default)]
    pub choke_group: u8,
}

fn default_swing() -> f32 {
    0.5
}

fn default_pitch_env_ms() -> f32 {
    42.0
}

fn default_hpf_hz() -> f32 {
    20.0
}

fn default_lpf_hz() -> f32 {
    20_000.0
}

fn default_resonance_q() -> f32 {
    0.707
}

fn default_comb_feedback() -> f32 {
    0.84
}

pub(crate) fn default_focus() -> (f32, f32) {
    (0.5, 0.5)
}

/// Polyphonic resampled chord voicings (`1/sqrt(N)` gain-compensated,
/// stereo-fanned, `HPF >= 210 Hz`, varispeed-coupled to stage panels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChordVoicing {
    #[default]
    Off,
    Octaves,
    Fifths,
    Min7,
    Min9,
    Maj7,
    Sus4,
    Cluster,
}

impl ChordVoicing {
    pub const ALL: [ChordVoicing; 8] = [
        ChordVoicing::Off,
        ChordVoicing::Octaves,
        ChordVoicing::Fifths,
        ChordVoicing::Min7,
        ChordVoicing::Min9,
        ChordVoicing::Maj7,
        ChordVoicing::Sus4,
        ChordVoicing::Cluster,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ChordVoicing::Off => "off",
            ChordVoicing::Octaves => "oct",
            ChordVoicing::Fifths => "5ths",
            ChordVoicing::Min7 => "min7",
            ChordVoicing::Min9 => "min9",
            ChordVoicing::Maj7 => "maj7",
            ChordVoicing::Sus4 => "sus4",
            ChordVoicing::Cluster => "cluster",
        }
    }

    pub fn parse(s: &str) -> Option<ChordVoicing> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "mono" => Some(ChordVoicing::Off),
            "oct" | "octave" | "octaves" => Some(ChordVoicing::Octaves),
            "5ths" | "fifths" | "fifth" | "power" => Some(ChordVoicing::Fifths),
            "min7" | "m7" => Some(ChordVoicing::Min7),
            "min9" | "m9" => Some(ChordVoicing::Min9),
            "maj7" | "M7" => Some(ChordVoicing::Maj7),
            "sus4" | "sus" => Some(ChordVoicing::Sus4),
            "cluster" | "add9" => Some(ChordVoicing::Cluster),
            _ => None,
        }
    }

    pub fn intervals(self) -> &'static [f32] {
        match self {
            ChordVoicing::Off => &[0.0],
            ChordVoicing::Octaves => &[0.0, 12.0],
            ChordVoicing::Fifths => &[0.0, 7.0, 12.0],
            ChordVoicing::Min7 => &[0.0, 3.0, 7.0, 10.0],
            ChordVoicing::Min9 => &[0.0, 3.0, 7.0, 10.0, 14.0],
            ChordVoicing::Maj7 => &[0.0, 4.0, 7.0, 11.0],
            ChordVoicing::Sus4 => &[0.0, 5.0, 7.0, 12.0],
            ChordVoicing::Cluster => &[0.0, 2.0, 7.0, 9.0],
        }
    }
}

/// One-tap musical sculptor stamps — each populates the continuous dials of
/// [`VoiceSculpt`] transparently (never a black box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Stamp {
    #[default]
    Raw,
    Kick,
    Snare,
    Hat,
    Pluck,
    Chord,
    Phrase,
    Wash,
    TapeLoop,
    Disintegrate,
}

/// Alias for UI callers.
pub type SculptStamp = Stamp;

impl Stamp {
    pub const ALL: [Stamp; 10] = [
        Stamp::Raw,
        Stamp::Kick,
        Stamp::Snare,
        Stamp::Hat,
        Stamp::Pluck,
        Stamp::Chord,
        Stamp::Phrase,
        Stamp::Wash,
        Stamp::TapeLoop,
        Stamp::Disintegrate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stamp::Raw => "raw",
            Stamp::Kick => "kick",
            Stamp::Snare => "snare",
            Stamp::Hat => "hat",
            Stamp::Pluck => "pluck",
            Stamp::Chord => "chord",
            Stamp::Phrase => "phrase",
            Stamp::Wash => "wash",
            Stamp::TapeLoop => "tapeloop",
            Stamp::Disintegrate => "disintegrate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stamp::Raw => "Raw",
            Stamp::Kick => "Kick",
            Stamp::Snare => "Snare",
            Stamp::Hat => "Hat",
            Stamp::Pluck => "Pluck",
            Stamp::Chord => "Chord",
            Stamp::Phrase => "Phrase",
            Stamp::Wash => "Wash",
            Stamp::TapeLoop => "Tape Loop",
            Stamp::Disintegrate => "Disintegrate",
        }
    }

    pub fn parse(s: &str) -> Option<Stamp> {
        match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
            "raw" | "clean" | "off" => Some(Stamp::Raw),
            "kick" | "sub" | "808" => Some(Stamp::Kick),
            "snare" | "clap" | "snap" => Some(Stamp::Snare),
            "hat" | "hihat" | "air" => Some(Stamp::Hat),
            "pluck" | "comb" | "karplus" => Some(Stamp::Pluck),
            "chord" | "pads" => Some(Stamp::Chord),
            "phrase" | "vox" | "speech" => Some(Stamp::Phrase),
            "wash" | "shimmer" => Some(Stamp::Wash),
            "tapeloop" | "tape" | "loop" => Some(Stamp::TapeLoop),
            "disintegrate" | "wear" | "basinski" | "caretaker" => Some(Stamp::Disintegrate),
            _ => None,
        }
    }
}

/// Pure-sample continuous voice sculptor & Basinski/Caretaker tape-loop physics.
/// Zero oscillators: every sound is excited strictly by the real media slice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceSculpt {
    /// Static transposition offset populated by stamps (e.g. `-14` Kick, `+10` Hat).
    #[serde(default)]
    pub pitch_semitones: f32,
    /// Exponential pitch-drop transient envelope in semitones (`-24..+24`).
    #[serde(default)]
    pub pitch_env_semitones: f32,
    /// Pitch envelope time constant in milliseconds (`10..250`).
    #[serde(default = "default_pitch_env_ms")]
    pub pitch_env_ms: f32,
    /// `68 ms`-transient-gated Cytomic ZDF swept SVF (`155 -> 50 Hz`, `Q = 8.2`), `0..1`.
    #[serde(default)]
    pub sub_exciter: f32,
    /// 3-grain micro-flam (`0 / 6.5 / 14 ms`) + `220 Hz` & `2.6 kHz` presence +
    /// `85 ms` 5-tap stereo gated room + `130 ms` speech-spill tail gate, `0..1`.
    #[serde(default)]
    pub snare_snap: f32,
    /// Pre-filter loudness-compensated `tanh` saturation in dB (`0..18`).
    #[serde(default)]
    pub drive_db: f32,
    /// Post-drive high-pass cutoff in Hz (`20..12000`).
    #[serde(default = "default_hpf_hz")]
    pub hpf_hz: f32,
    /// Post-drive low-pass cutoff in Hz (`60..20000`; `<200 Hz` engages steep `24 dB/oct` kick sub crossover).
    #[serde(default = "default_lpf_hz")]
    pub lpf_hz: f32,
    /// Filter resonance `Q` (`0.5..8.0`).
    #[serde(default = "default_resonance_q")]
    pub resonance_q: f32,
    /// Surgical `-4.0 dB @ 290 Hz` low-mid mud carve on non-sub voices.
    #[serde(default)]
    pub pocket_carve: bool,
    /// Scale-locked fractional-delay Karplus-Strong comb resonator wet mix (`0..1`).
    #[serde(default)]
    pub comb_mix: f32,
    /// Karplus-Strong comb feedback (`0.60..0.93`).
    #[serde(default = "default_comb_feedback")]
    pub comb_feedback: f32,
    /// Polyphonic chord stack voicing.
    #[serde(default)]
    pub chord: ChordVoicing,
    /// Dual-ratio (`+12 st` & `+7 st`) windowed grain halo (`380 Hz–11.5 kHz`), `0..1`.
    #[serde(default)]
    pub shimmer: f32,
    /// Asynchronous real-seconds tape loop period (independent of BPM grid, `80 ms` equal-power seam).
    #[serde(default)]
    pub free_loop_secs: Option<f32>,
    /// Analog capstan `0.82 Hz` wow + `4.25 Hz` flutter + stereo Haas chorus (`0..1`).
    #[serde(default)]
    pub wow_flutter: f32,
    /// Basinski per-pass / over-time tape wear: progressive LPF darkening, wow deepening,
    /// deterministic organic oxide dropouts (audio + video), and ghostly dark reverb (`0..1`).
    #[serde(default)]
    pub disintegrate: f32,
    /// Slow asynchronous breathing swell LFO across passes (`0..1`).
    #[serde(default)]
    pub ebb_flow: f32,
}

impl Default for VoiceSculpt {
    fn default() -> Self {
        VoiceSculpt {
            pitch_semitones: 0.0,
            pitch_env_semitones: 0.0,
            pitch_env_ms: default_pitch_env_ms(),
            sub_exciter: 0.0,
            snare_snap: 0.0,
            drive_db: 0.0,
            hpf_hz: default_hpf_hz(),
            lpf_hz: default_lpf_hz(),
            resonance_q: default_resonance_q(),
            pocket_carve: false,
            comb_mix: 0.0,
            comb_feedback: default_comb_feedback(),
            chord: ChordVoicing::Off,
            shimmer: 0.0,
            free_loop_secs: None,
            wow_flutter: 0.0,
            disintegrate: 0.0,
            ebb_flow: 0.0,
        }
    }
}

impl VoiceSculpt {
    /// Populate continuous sculptor parameters from a musical [`Stamp`].
    pub fn preset(stamp: Stamp) -> VoiceSculpt {
        let base = VoiceSculpt::default();
        match stamp {
            Stamp::Raw => base,
            Stamp::Kick => VoiceSculpt {
                pitch_semitones: -14.0,
                pitch_env_semitones: 16.0,
                pitch_env_ms: 42.0,
                sub_exciter: 0.72,
                drive_db: 7.5,
                hpf_hz: 28.0,
                lpf_hz: 155.0,
                resonance_q: 0.85,
                ..base
            },
            Stamp::Snare => VoiceSculpt {
                snare_snap: 0.85,
                drive_db: 8.5,
                hpf_hz: 170.0,
                lpf_hz: 8200.0,
                resonance_q: 0.80,
                ..base
            },
            Stamp::Hat => VoiceSculpt {
                pitch_semitones: 10.0,
                drive_db: 3.5,
                hpf_hz: 5400.0,
                lpf_hz: 18_500.0,
                resonance_q: 0.90,
                ..base
            },
            Stamp::Pluck => VoiceSculpt {
                comb_mix: 0.78,
                comb_feedback: 0.945,
                hpf_hz: 140.0,
                lpf_hz: 9500.0,
                pocket_carve: true,
                ..base
            },
            Stamp::Chord => VoiceSculpt {
                comb_mix: 0.42,
                comb_feedback: 0.91,
                chord: ChordVoicing::Min7,
                shimmer: 0.25,
                hpf_hz: 215.0,
                lpf_hz: 11_000.0,
                pocket_carve: true,
                ..base
            },
            Stamp::Phrase => VoiceSculpt {
                drive_db: 2.0,
                hpf_hz: 165.0,
                lpf_hz: 14_500.0,
                pocket_carve: true,
                ..base
            },
            Stamp::Wash => VoiceSculpt {
                shimmer: 0.55,
                wow_flutter: 0.22,
                ebb_flow: 0.45,
                hpf_hz: 220.0,
                lpf_hz: 8500.0,
                pocket_carve: true,
                ..base
            },
            Stamp::TapeLoop => VoiceSculpt {
                free_loop_secs: Some(6.4),
                wow_flutter: 0.38,
                disintegrate: 0.18,
                ebb_flow: 0.50,
                drive_db: 2.5,
                hpf_hz: 115.0,
                lpf_hz: 7200.0,
                pocket_carve: true,
                ..base
            },
            Stamp::Disintegrate => VoiceSculpt {
                free_loop_secs: Some(7.8),
                wow_flutter: 0.55,
                disintegrate: 0.72,
                ebb_flow: 0.65,
                shimmer: 0.32,
                drive_db: 3.5,
                hpf_hz: 135.0,
                lpf_hz: 5400.0,
                pocket_carve: true,
                ..base
            },
        }
    }

    /// True when any per-voice audio DSP stage deviates from clean bypass.
    #[inline]
    pub fn has_audio_dsp(&self) -> bool {
        self.pitch_env_semitones.abs() > 1e-3
            || self.sub_exciter > 1e-3
            || self.snare_snap > 1e-3
            || self.drive_db > 1e-3
            || self.hpf_hz > 22.0
            || self.lpf_hz < 19_800.0
            || (self.resonance_q - 0.707).abs() > 0.05
            || self.pocket_carve
            || self.comb_mix > 1e-3
            || self.shimmer > 1e-3
            || self.wow_flutter > 1e-3
            || self.disintegrate > 1e-3
            || self.ebb_flow > 1e-3
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.has_audio_dsp()
            || self.pitch_semitones.abs() > 1e-3
            || self.chord != ChordVoicing::Off
            || self.free_loop_secs.is_some()
    }

    /// True for sustained ambient/tape washes that bleed full-screen in [`crate::tile::Layout::Split`].
    #[inline]
    pub fn is_wash(&self) -> bool {
        self.free_loop_secs.is_some()
            || self.disintegrate > 0.05
            || self.ebb_flow > 0.15
            || self.shimmer > 0.35
    }

    /// Extra ring-out seconds past the nominal slice/grain duration (e.g. `+140 ms` Karplus-Strong comb tail).
    #[inline]
    pub fn tail_secs(&self) -> f32 {
        let mut tail = 0.0f32;
        if self.comb_mix > 0.01 {
            tail = tail.max(0.14);
        }
        if self.shimmer > 0.02 || self.disintegrate > 0.15 {
            tail = tail.max(0.18);
        }
        if self.sub_exciter > 0.02 {
            tail = tail.max(0.08);
        }
        tail
    }
}

/// Deterministic Basinski tape-oxide dropout (`0..1`) at `elapsed_secs` on loop pass `pass_index`.
/// Progressively deepens as `pass_index` advances so loops crumble organically into silence & ghost reverb.
#[inline]
pub fn oxide_dropout_at(disintegrate: f32, pass_index: u32, elapsed_secs: f64, seed: u64) -> f32 {
    let d = disintegrate.clamp(0.0, 1.0);
    if d <= 1e-4 {
        return 0.0;
    }
    // Smooth, non-saturating exponential oxide wear across 0..24+ passes.
    let pass_wear =
        1.0 - (-0.115 * (pass_index as f32 + 0.35) - 0.010 * elapsed_secs.max(0.0) as f32).exp();
    let wear = (d * pass_wear).clamp(0.0, 0.95);
    let p0 = ((seed & 0xFFFF) as f64) * 0.000_095;
    let p1 = (((seed >> 16) & 0xFFFF) as f64) * 0.000_095;
    let p2 = (((seed >> 32) & 0xFFFF) as f64) * 0.000_095;
    let t = elapsed_secs + pass_index as f64 * 3.17;
    // Multi-scale irregular tape-flake contour in [0, 1].
    let w = 0.45 * (0.5 + 0.5 * (t * 1.73 + p0).sin())
        + 0.35 * (0.5 + 0.5 * (t * 4.31 + p1).sin())
        + 0.20 * (0.5 + 0.5 * (t * 11.17 + p2).sin());
    let thresh = (1.02 - 0.82 * wear as f64).clamp(0.10, 1.0);
    let flake = ((w - thresh) / (1.0 - thresh).max(0.08)).clamp(0.0, 1.0) as f32;
    // Monotonic progressive baseline oxide loss + localized tape-flake dropouts.
    (wear * 0.58 + flake * (0.20 + 0.36 * wear)).clamp(0.0, 0.95)
}

/// Slow asynchronous Caretaker/Eno breathing swell multiplier (`0.35..1.0`).
#[inline]
pub fn ebb_flow_gain_at(ebb_flow: f32, pass_index: u32, elapsed_secs: f64, seed: u64) -> f32 {
    let amt = ebb_flow.clamp(0.0, 1.0);
    if amt <= 1e-4 {
        return 1.0;
    }
    let phase_off = ((seed.wrapping_mul(0x9E37_79B9) & 0xFFFF) as f64 / 65535.0) * std::f64::consts::TAU;
    let rate_hz = 0.068 + ((seed >> 20) & 0xFF) as f64 / 255.0 * 0.045; // ~9s..15s cycle
    let t = elapsed_secs + pass_index as f64 * 4.7;
    let lfo = 0.5 + 0.5 * (std::f64::consts::TAU * rate_hz * t + phase_off).cos();
    (1.0 - 0.52 * amt * (1.0 - lfo as f32)).clamp(0.25, 1.0)
}

impl GrainSettings {
    /// Set a parameter by name (shared by scripting and automation).
    /// Returns Err for names that are not grain parameters.
    pub fn set_param(&mut self, key: &str, value: f64) -> Result<(), ()> {
        let x = value as f32;
        match key {
            "density" => self.density = x.clamp(0.1, 500.0),
            "duration" => self.duration = x.clamp(0.005, 5.0),
            "duration_jitter" => self.duration_jitter = x.clamp(0.0, 1.0),
            "position" => self.position = x.clamp(0.0, 1.0),
            "spray" => self.spray = x.max(0.0),
            "scan_speed" => self.scan_speed = x,
            "speed" => self.speed = x.clamp(0.05, 8.0),
            "pitch" => self.pitch = x.clamp(-48.0, 48.0),
            "pitch_jitter" => self.pitch_jitter = x.clamp(0.0, 48.0),
            "detune_cents" | "detune" => self.detune_cents = x.clamp(-100.0, 100.0),
            "quantize_amount" | "key_strength" => self.quantize_amount = x.clamp(0.0, 1.0),
            "gain" => self.gain = x.clamp(0.0, 4.0),
            "pan" => self.pan = x.clamp(-1.0, 1.0),
            "pan_spread" => self.pan_spread = x.clamp(0.0, 1.0),
            "envelope" => self.envelope = x.clamp(0.01, 1.0),
            "env_skew" => self.env_skew = x.clamp(-1.0, 1.0),
            "reverse_prob" => self.reverse_prob = x.clamp(0.0, 1.0),
            "sync_div" => self.sync_div = x.clamp(0.0, 4.0),
            "voices" => self.voices = (value as i64).clamp(1, 4) as u32,
            "voice_interval" => self.voice_interval = x.clamp(-24.0, 24.0),
            "voice_detune" => self.voice_detune = x.clamp(0.0, 2.0),
            "seed" => self.seed = value as u64,
            "texture" | "grain" | "grainy" => self.texture = x.clamp(0.0, 1.0),
            "swing" | "shuffle" => self.swing = x.clamp(0.25, 0.75),
            "varispeed" | "tape_pitch" => self.varispeed = x >= 0.5,
            "pitch_env_semitones" | "pitch_drop" => self.sculpt.pitch_env_semitones = x.clamp(-24.0, 24.0),
            "pitch_env_ms" => self.sculpt.pitch_env_ms = x.clamp(10.0, 250.0),
            "sub_exciter" | "sub" => self.sculpt.sub_exciter = x.clamp(0.0, 1.0),
            "snare_snap" | "snap" => self.sculpt.snare_snap = x.clamp(0.0, 1.0),
            "drive_db" | "drive" => self.sculpt.drive_db = x.clamp(0.0, 18.0),
            "hpf_hz" | "hpf" => self.sculpt.hpf_hz = x.clamp(20.0, 12_000.0),
            "lpf_hz" | "lpf" => self.sculpt.lpf_hz = x.clamp(60.0, 20_000.0),
            "resonance_q" | "q" => self.sculpt.resonance_q = x.clamp(0.3, 12.0),
            "comb_mix" | "comb" => self.sculpt.comb_mix = x.clamp(0.0, 1.0),
            "comb_feedback" => self.sculpt.comb_feedback = x.clamp(0.60, 0.93),
            "shimmer" => self.sculpt.shimmer = x.clamp(0.0, 1.0),
            "wow_flutter" | "wow" => self.sculpt.wow_flutter = x.clamp(0.0, 1.0),
            "disintegrate" | "wear" => self.sculpt.disintegrate = x.clamp(0.0, 1.0),
            "ebb_flow" => self.sculpt.ebb_flow = x.clamp(0.0, 1.0),
            "choke_group" | "choke" => self.choke_group = (value as i64).clamp(0, 4) as u8,
            _ => return Err(()),
        }
        Ok(())
    }
}

impl Default for GrainSettings {
    fn default() -> Self {
        GrainSettings {
            density: 20.0,
            duration: 0.12,
            duration_jitter: 0.2,
            position: 0.0,
            spray: 0.05,
            scan_speed: 1.0,
            speed: 1.0,
            pitch: 0.0,
            pitch_jitter: 0.0,
            detune_cents: 0.0,
            quantize_amount: 1.0,
            gain: 0.8,
            pan: 0.0,
            pan_spread: 0.6,
            envelope: 0.6,
            env_skew: 0.0,
            reverse_prob: 0.0,
            sync_div: 0.0,
            voices: 1,
            voice_interval: 12.0,
            voice_detune: 0.0,
            seed: 0xC0FFEE,
            texture: 0.0,
            swing: 0.5,
            varispeed: false,
            adsr: None,
            sculpt: VoiceSculpt::default(),
            choke_group: 0,
        }
    }
}

/// Duration of the click-free cosine mute when a choke group cuts an earlier voice (`3 ms`).
pub const CHOKE_RELEASE_SECS: f64 = 0.003;

/// Raised-cosine choke multiplier (`1.0` before `choke_at`, `0.0` at `choke_at + 3 ms`).
#[inline]
pub fn choke_gain_at(choke_at: Option<f64>, t: f64) -> f32 {
    let Some(t_cut) = choke_at else { return 1.0 };
    if t <= t_cut {
        1.0
    } else if t >= t_cut + CHOKE_RELEASE_SECS {
        0.0
    } else {
        let u = ((t - t_cut) / CHOKE_RELEASE_SECS) as f32;
        0.5 * (1.0 + (std::f32::consts::PI * u).cos())
    }
}

/// Which voice a grain belongs to, for ADSR, choke groups, and stage layout.
/// Compact `Copy` struct shared between the audio engine and GPU/CPU stage compositors.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceTag {
    /// Stable voice id (0 = none): a held note, a pad loop, a step hit,
    /// a clip, an emitter gesture.
    #[serde(default)]
    pub voice: u64,
    /// The voice's envelope; scales audio per sample and picture opacity.
    #[serde(default)]
    pub env: Option<crate::envelope::VoiceEnv>,
    /// Tile slot in [`Layout::Tile`](crate::tile::Layout) / [`Layout::Split`](crate::tile::Layout).
    #[serde(default)]
    pub tile: Option<u8>,
    /// Normalized motion/subject center `(cx, cy)` in `0..1` (`(0.5, 0.5)` default).
    #[serde(default = "default_focus")]
    pub focus: (f32, f32),
    /// True on odd ratchet sub-hits (`r % 2 == 1`) so the stage mirrors `flipX`.
    #[serde(default)]
    pub ratchet_odd: bool,
    /// Transient zoom impulse (`0..0.25`) decaying from note onset.
    #[serde(default)]
    pub zoom_punch: f32,
    /// Basinski tape-oxide dropout wear (`0..1`) coupling audio dropouts to visual alpha/chroma wear.
    #[serde(default)]
    pub oxide_dropout: f32,
    /// Monophonic choke group (`0 = off`, `1..=4`).
    #[serde(default)]
    pub choke_group: u8,
    /// True for sustained granular/tape wash layers that bleed full-screen at `30%` in `Layout::Split`.
    #[serde(default)]
    pub is_wash: bool,
    /// Absolute choke cut timestamp (`None` = unchoked; ramps to 0 over `3 ms` cosine).
    #[serde(default)]
    pub choke_at: Option<f64>,
    /// Optional spoken-hook stage caption carried from [`crate::chop::Slice::caption`].
    #[serde(default)]
    pub caption: crate::chop::Caption,
    /// *MODELL 5* full-cell hard-frame micro-burst (`true` when `PictureCut::Cut` +
    /// tight `spray <= 0.08` + micro-grain `duration <= 95 ms`): renders 100%
    /// full-cell / full-frame (`size = 1.0`, `feather = 0.0`, sharp cut).
    #[serde(default)]
    pub hard_burst: bool,
}

impl Default for VoiceTag {
    fn default() -> Self {
        VoiceTag {
            voice: 0,
            env: None,
            tile: None,
            focus: (0.5, 0.5),
            ratchet_odd: false,
            zoom_punch: 0.0,
            oxide_dropout: 0.0,
            choke_group: 0,
            is_wash: false,
            choke_at: None,
            caption: crate::chop::Caption::default(),
            hard_burst: false,
        }
    }
}

impl VoiceTag {
    /// Envelope + choke gain at absolute time `t` (1 without an envelope or choke).
    #[inline]
    pub fn level_at(&self, t: f64) -> f32 {
        self.env.map_or(1.0, |e| e.level_at(t)) * choke_gain_at(self.choke_at, t)
    }

    /// Picture weight at absolute time `t` (1 without an envelope or choke).
    #[inline]
    pub fn picture_at(&self, t: f64) -> f32 {
        self.env.map_or(1.0, |e| e.picture_at(t)) * choke_gain_at(self.choke_at, t)
    }
}

/// One scheduled grain, shared by the audio and video renderers.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GrainEvent {
    /// Absolute onset on the timeline, seconds.
    pub onset: f64,
    /// Read position in the source media, seconds.
    pub source_pos: f64,
    /// Grain length in seconds (output time).
    pub duration: f32,
    /// Playback rate; audio pitch ratio == visual playback rate.
    pub pitch_ratio: f32,
    /// Linear gain (audio) == brightness/opacity weight (video).
    pub gain: f32,
    /// -1..1; audio pan == horizontal placement of the visual grain.
    pub pan: f32,
    /// Envelope shape, forwarded from settings.
    pub envelope: f32,
    /// Envelope skew, forwarded from settings.
    pub env_skew: f32,
    /// Play the grain backwards (audio and video alike).
    pub reverse: bool,
    /// Stable per-grain id for deterministic visual scatter.
    pub id: u64,
    /// Voice (ADSR envelope, tile slot, focus, choke, wear).
    #[serde(default)]
    pub tag: VoiceTag,
    /// Per-voice pure-sample sculptor & tape-loop physics (`Raw` default = zero overhead).
    #[serde(default)]
    pub sculpt: VoiceSculpt,
    /// Target fundamental frequency (Hz) for the scale-locked Karplus-Strong comb resonator (`0 = auto`).
    #[serde(default)]
    pub comb_hz: f32,
    /// Loop pass index (`0, 1, 2, ...`) for progressive Basinski `disintegrate` wear.
    #[serde(default)]
    pub loop_pass: u32,
}

impl GrainEvent {
    /// Effective audio end time in seconds (including sculptor tail ring-out, bounded by choke cut).
    #[inline]
    pub fn audio_end(&self) -> f64 {
        let raw_end = self.onset + (self.duration + self.sculpt.tail_secs()) as f64;
        match self.tag.choke_at {
            Some(tc) => raw_end.min(tc + CHOKE_RELEASE_SECS),
            None => raw_end,
        }
    }

    pub fn end(&self) -> f64 {
        let raw_end = self.onset + self.duration as f64;
        match self.tag.choke_at {
            Some(tc) => raw_end.min(tc + CHOKE_RELEASE_SECS),
            None => raw_end,
        }
    }

    /// Pitch offset of this grain in semitones (drives visual hue shift).
    pub fn semitones(&self) -> f32 {
        crate::music::ratio_to_semitones(self.pitch_ratio)
    }
}

/// Snap a frequency `hz` onto the nearest note of `key` inside `[65.0, 1046.5] Hz` (`C2..C6`).
#[inline]
pub fn quantize_comb_hz(hz: f32, key: Option<&Key>) -> f32 {
    let hz = if hz.is_finite() && hz > 20.0 { hz } else { 130.8128 };
    let mut midi = crate::music::hz_to_midi(hz);
    while midi < 36.0 {
        midi += 12.0;
    }
    while midi > 84.0 {
        midi -= 12.0;
    }
    let snapped = match key {
        Some(k) => k.quantize_midi(midi),
        None => midi.round(),
    };
    crate::music::midi_to_hz(snapped).clamp(55.0, 1100.0)
}

/// Allocation-free polyphonic chord expansion (`1/sqrt(N)` gain, stereo fan,
/// `HPF >= 210 Hz`, and per-voice varispeed `2^(semi/12)` for multi-panel video harmony).
pub fn expand_chord_grain_inline(
    base: &GrainEvent,
    chord: ChordVoicing,
    base_hz: f32,
    key: Option<&Key>,
) -> ([GrainEvent; 5], usize) {
    let ivs = chord.intervals();
    let n = ivs.len().clamp(1, 5);
    let mut out = [*base; 5];
    if n == 1 {
        if out[0].sculpt.comb_mix > 0.001 && out[0].comb_hz <= 0.0 {
            out[0].comb_hz = quantize_comb_hz(base_hz.max(65.4) * out[0].pitch_ratio, key);
        }
        return (out, 1);
    }
    let gain_norm = 1.0 / (n as f32).sqrt();
    for (c, &semi) in ivs.iter().take(n).enumerate() {
        let mut ev = *base;
        let ratio_mult = crate::music::semitones_to_ratio(semi);
        ev.pitch_ratio = (base.pitch_ratio * ratio_mult).clamp(0.0625, 8.0);
        ev.gain = base.gain * gain_norm;
        let spread = ((c as f32 / (n - 1) as f32) - 0.5) * 0.75;
        ev.pan = (base.pan + spread).clamp(-1.0, 1.0);
        ev.sculpt.chord = ChordVoicing::Off;
        // Chord-root HPF law: keep chord stacks >= 210 Hz so stacked room tone never muddies 180-400 Hz.
        ev.sculpt.hpf_hz = ev.sculpt.hpf_hz.max(210.0);
        ev.id = base.id.wrapping_mul(31).wrapping_add(c as u64);
        if base.tag.voice != 0 {
            if c > 0 {
                ev.tag.voice = base.tag.voice ^ ((c as u64 + 1) * 0x0001_0000_0000_0007);
            }
        } else {
            ev.tag.voice = 0xD000_0000_0000_0000
                | ((base.id & 0x0FFF_FFFF_FFFF) << 4)
                | (c as u64 & 0xF);
        }
        if ev.sculpt.comb_mix > 0.001 || base.comb_hz > 0.0 {
            let root_hz = if base.comb_hz > 0.0 { base.comb_hz } else { base_hz.max(65.4) * base.pitch_ratio };
            ev.comb_hz = quantize_comb_hz(root_hz * ratio_mult, key);
        }
        out[c] = ev;
    }
    (out, n)
}

/// Convenience `Vec`-appending wrapper around [`expand_chord_grain_inline`].
pub fn expand_chord_grain(
    base: &GrainEvent,
    chord: ChordVoicing,
    base_hz: f32,
    key: Option<&Key>,
    out: &mut Vec<GrainEvent>,
) {
    let (arr, n) = expand_chord_grain_inline(base, chord, base_hz, key);
    out.extend_from_slice(&arr[..n]);
}

/// Schedule the grain cloud for a clip.
///
/// * `clip_start`/`clip_len`: seconds on the timeline.
/// * `source_len`: seconds of source media available.
/// * `base_hz` + `key`: when a key is given, every grain's pitch ratio is
///   quantized so `base_hz * ratio` lands on the key's scale.
pub fn schedule_grains(
    settings: &GrainSettings,
    clip_start: f64,
    clip_len: f64,
    source_len: f64,
    base_hz: f32,
    key: Option<&Key>,
) -> Vec<GrainEvent> {
    schedule_grains_auto(settings, &[], 1.0, clip_start, clip_len, source_len, base_hz, key)
}

/// Like [`schedule_grains`], with automation lanes evaluated at each
/// grain's onset (lane beats are relative to the clip start;
/// `secs_per_beat` converts grain time to beats). Density automation
/// changes the scheduling rate itself.
#[allow(clippy::too_many_arguments)]
pub fn schedule_grains_auto(
    settings: &GrainSettings,
    lanes: &[crate::auto::AutomationLane],
    secs_per_beat: f64,
    clip_start: f64,
    clip_len: f64,
    source_len: f64,
    base_hz: f32,
    key: Option<&Key>,
) -> Vec<GrainEvent> {
    let mut events = Vec::new();
    if clip_len <= 0.0 || source_len <= 0.0 || settings.density <= 0.0 {
        return events;
    }
    let mut rng = Rng::new(settings.seed ^ 0xA5A5_5A5A);

    /// One grain and its harmony voices at clip-time `t`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        t: f64,
        clip_start: f64,
        source_len: f64,
        base_hz: f32,
        key: Option<&Key>,
        s: &GrainSettings,
        rng: &mut Rng,
        events: &mut Vec<GrainEvent>,
        id: &mut u64,
    ) {
        let dur = (s.duration * (1.0 + s.duration_jitter * rng.bipolar())).clamp(0.005, 5.0);

        // Read head: base position + scan + spray.
        let mut pos = s.position as f64 * source_len
            + s.scan_speed as f64 * t
            + s.spray as f64 * rng.bipolar() as f64;
        pos = pos.rem_euclid(source_len.max(1e-6));

        let pitch_j = s.pitch_jitter * rng.bipolar();
        let reverse = rng.next_f32() < s.reverse_prob;

        let n_voices = s.voices.clamp(1, 4);
        // Keep the cloud's loudness roughly level as voices AND grain overlap
        // (`density * duration`) stack, so dense pads don't slam +10 dB into
        // master limiting. Below overlap 2.5 we leave single-grain / sparse
        // gain untouched so existing voice-compensation behavior holds.
        let overlap = s.density.max(0.1) * s.duration.max(0.005);
        let overlap_norm = if overlap > 2.5 {
            (2.5 / overlap).powf(0.38)
        } else {
            1.0
        };
        let vgain = s.gain * overlap_norm / (n_voices as f32).sqrt();
        for v in 0..n_voices {
            let detune = if v == 0 { 0.0 } else { s.voice_detune * rng.bipolar() };
            let st = s.pitch + s.sculpt.pitch_semitones + v as f32 * s.voice_interval + pitch_j + detune;
            let ratio = crate::music::effective_ratio(
                st,
                key,
                base_hz,
                s.quantize_amount,
                s.detune_cents,
            );
            // Equal-loudness warmth compensation: upward-pitched grains land
            // in the ear's 2-5 kHz sensitivity peak, so gently attenuate
            // `ratio > 1` (`ratio == 1` stays exact unity).
            let pitch_warmth = if ratio > 1.0 { ratio.powf(-0.35) } else { 1.0 };
            let pan = (s.pan + s.pan_spread.clamp(0.0, 1.0) * rng.bipolar())
                .clamp(-1.0, 1.0);
            let comb_hz = if s.sculpt.comb_mix > 1e-3 {
                quantize_comb_hz(base_hz.max(65.4) * ratio, key)
            } else {
                0.0
            };
            let hard_burst = s.spray <= 0.08
                && dur <= 0.095
                && s.adsr.is_some_and(|a| a.picture == crate::envelope::PictureCut::Cut);
            let is_wash = !hard_burst
                && (s.sculpt.is_wash() || (s.adsr.is_none() && s.spray > 0.08));
            let base_ev = GrainEvent {
                onset: clip_start + t,
                source_pos: pos,
                duration: dur,
                pitch_ratio: ratio,
                gain: vgain * pitch_warmth * ebb_flow_gain_at(s.sculpt.ebb_flow, 0, t, s.seed),
                pan,
                envelope: s.envelope,
                env_skew: s.env_skew,
                reverse,
                id: *id,
                tag: VoiceTag {
                    choke_group: s.choke_group,
                    is_wash,
                    oxide_dropout: oxide_dropout_at(s.sculpt.disintegrate, 0, t, s.seed),
                    hard_burst,
                    ..Default::default()
                },
                sculpt: s.sculpt,
                comb_hz,
                loop_pass: 0,
            };
            if s.sculpt.chord != ChordVoicing::Off {
                expand_chord_grain(&base_ev, s.sculpt.chord, base_hz, key, events);
            } else {
                events.push(base_ev);
            }
            *id += 1;
        }
    }

    let eff_at = |t: f64| -> GrainSettings {
        if lanes.is_empty() {
            settings.clone()
        } else {
            crate::auto::settings_at(settings, lanes, t / secs_per_beat.max(1e-9))
        }
    };

    let mut id: u64 = 0;
    if settings.sync_div > 0.0 {
        // Beat-synced: onsets live on the sync grid (with optional swing on
        // odd slots); density is the mean trigger rate, realized as a
        // per-slot probability.
        let slot_secs = (settings.sync_div as f64 * secs_per_beat).max(1e-3);
        let mut k = 0u64;
        loop {
            let base_t = k as f64 * slot_secs;
            if base_t >= clip_len {
                break;
            }
            let roll = rng.next_f32();
            let s = eff_at(base_t);
            let swing_off = if k % 2 == 1 {
                (s.swing.clamp(0.25, 0.75) as f64 - 0.5) * 2.0 * slot_secs
            } else {
                0.0
            };
            let t = (base_t + swing_off).clamp(0.0, (clip_len - 1e-6).max(0.0));
            let p = (s.density as f64 * slot_secs).min(1.0) as f32;
            if roll < p {
                emit(t, clip_start, source_len, base_hz, key, &s, &mut rng, &mut events, &mut id);
            }
            k += 1;
        }
    } else {
        // Free: classic asynchronous cloud with jittered inter-onset times.
        let mut t = 0.0f64;
        while t < clip_len {
            let s = eff_at(t);
            let mean_interval = 1.0 / s.density.max(0.01) as f64;
            // Jitter inter-onset interval +-50% for an organic cloud.
            let interval = mean_interval * (0.5 + rng.next_f32() as f64);
            emit(t, clip_start, source_len, base_hz, key, &s, &mut rng, &mut events, &mut id);
            t += interval;
        }
    }
    events
}

/// Build the grain a MIDI note plays: the sampler mapping. Note 60 (C4)
/// plays the source at the clip's base pitch; other notes transpose in
/// semitones. Velocity scales gain. No key quantization — MIDI notes are
/// explicit pitches.
pub fn grain_from_note(
    settings: &GrainSettings,
    note: u8,
    velocity: u8,
    source_len: f64,
    onset: f64,
    id: u64,
) -> GrainEvent {
    let st = settings.pitch + settings.sculpt.pitch_semitones + (note as f32 - 60.0);
    let comb_hz = if settings.sculpt.comb_mix > 1e-3 {
        crate::music::midi_to_hz(note as f32).clamp(55.0, 1100.0)
    } else {
        0.0
    };
    GrainEvent {
        onset,
        source_pos: (settings.position as f64 * source_len).min(source_len),
        duration: settings.duration.clamp(0.01, 5.0),
        // Notes are explicit pitches (no key snap), but detune still
        // shades them off-center.
        pitch_ratio: crate::music::effective_ratio(st, None, 440.0, 0.0, settings.detune_cents),
        gain: settings.gain * (velocity as f32 / 127.0),
        pan: settings.pan.clamp(-1.0, 1.0),
        envelope: settings.envelope,
        env_skew: settings.env_skew,
        reverse: settings.reverse_prob >= 0.5,
        id,
        tag: VoiceTag {
            choke_group: settings.choke_group,
            is_wash: settings.sculpt.is_wash(),
            ..Default::default()
        },
        sculpt: settings.sculpt,
        comb_hz,
        loop_pass: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::{hz_to_midi, Key};

    #[test]
    fn schedules_expected_grain_count() {
        let s = GrainSettings { density: 50.0, ..Default::default() };
        let ev = schedule_grains(&s, 0.0, 2.0, 10.0, 440.0, None);
        // ~50/s over 2s with +-50% interval jitter: expect roughly 100.
        assert!(ev.len() > 60 && ev.len() < 220, "got {}", ev.len());
        for e in &ev {
            assert!(e.onset >= 0.0 && e.onset < 2.0);
            assert!(e.source_pos >= 0.0 && e.source_pos < 10.0);
            assert!(e.duration > 0.0);
        }
    }

    #[test]
    fn deterministic_with_seed() {
        let s = GrainSettings::default();
        let a = schedule_grains(&s, 0.0, 1.0, 5.0, 440.0, None);
        let b = schedule_grains(&s, 0.0, 1.0, 5.0, 440.0, None);
        assert_eq!(a, b);
        let s2 = GrainSettings { seed: 999, ..Default::default() };
        let c = schedule_grains(&s2, 0.0, 1.0, 5.0, 440.0, None);
        assert_ne!(a, c);
    }

    #[test]
    fn key_quantization_applies_to_all_grains() {
        let key = Key::parse("C", "majorpentatonic").unwrap();
        let s = GrainSettings {
            pitch_jitter: 7.0,
            density: 40.0,
            ..Default::default()
        };
        let ev = schedule_grains(&s, 0.0, 1.0, 5.0, 261.626, Some(&key));
        assert!(!ev.is_empty());
        for e in &ev {
            let midi = hz_to_midi(261.626 * e.pitch_ratio);
            let pc = (midi.round() as i32).rem_euclid(12);
            let rel = (pc - key.root).rem_euclid(12);
            assert!(
                key.scale.intervals().contains(&rel),
                "grain pitch pc {pc} escaped the key"
            );
        }
    }

    #[test]
    fn sync_locks_onsets_to_the_grid() {
        let s = GrainSettings {
            density: 200.0, // saturates every slot
            sync_div: 0.25,
            duration_jitter: 0.0,
            ..Default::default()
        };
        // secs_per_beat = 0.5 -> grid of 0.125s.
        let ev = schedule_grains_auto(&s, &[], 0.5, 0.0, 2.0, 10.0, 440.0, None);
        assert!(!ev.is_empty());
        for e in &ev {
            let slots = e.onset / 0.125;
            assert!(
                (slots - slots.round()).abs() < 1e-9,
                "onset {} not on the 16th grid",
                e.onset
            );
        }
        // Full saturation: one grain per slot over 2s = 16 slots.
        assert_eq!(ev.len(), 16);

        // Low density -> sparse but still on-grid.
        let sparse = GrainSettings { density: 3.0, ..s };
        let ev2 = schedule_grains_auto(&sparse, &[], 0.5, 0.0, 4.0, 10.0, 440.0, None);
        assert!(ev2.len() < 32 && !ev2.is_empty(), "{} hits", ev2.len());
    }

    #[test]
    fn harmony_voices_stack_intervals() {
        let key = Key::parse("C", "major").unwrap();
        let s = GrainSettings {
            density: 10.0,
            voices: 3,
            voice_interval: 12.0,
            voice_detune: 0.0,
            pitch_jitter: 0.0,
            pan_spread: 0.0,
            ..Default::default()
        };
        let base_hz = 261.626; // C4
        let ev = schedule_grains(&s, 0.0, 1.0, 5.0, base_hz, Some(&key));
        // Events come in groups of 3 sharing an onset.
        assert_eq!(ev.len() % 3, 0);
        for g in ev.chunks(3) {
            assert_eq!(g[0].onset, g[1].onset);
            assert_eq!(g[1].onset, g[2].onset);
            // Octave stack: ratios 1, 2, 4.
            assert!((g[1].pitch_ratio / g[0].pitch_ratio - 2.0).abs() < 0.01);
            assert!((g[2].pitch_ratio / g[0].pitch_ratio - 4.0).abs() < 0.02);
            // Voice gain compensated.
            assert!((g[0].gain - s.gain / 3.0f32.sqrt()).abs() < 1e-6);
        }
        // Fifths stay in key when quantized.
        let s5 = GrainSettings { voice_interval: 7.0, ..s };
        let ev5 = schedule_grains(&s5, 0.0, 1.0, 5.0, base_hz, Some(&key));
        for e in &ev5 {
            let midi = hz_to_midi(base_hz * e.pitch_ratio);
            let pc = (midi.round() as i32).rem_euclid(12);
            assert!(key.scale.intervals().contains(&pc), "voice off-key: pc {pc}");
        }
    }

    #[test]
    fn env_skew_shapes_attack_vs_release() {
        use crate::dsp::grain_env_skewed;
        // Percussive: loud early, gone late.
        let perc_early = grain_env_skewed(0.05, 0.8, -1.0);
        let perc_late = grain_env_skewed(0.7, 0.8, -1.0);
        assert!(perc_early > 0.9, "instant attack: {perc_early}");
        assert!(perc_late < perc_early, "long tail decays: {perc_late}");
        // Swell: quiet early, loud late.
        let swell_early = grain_env_skewed(0.3, 0.8, 1.0);
        let swell_late = grain_env_skewed(0.95, 0.8, 1.0);
        assert!(swell_early < 0.9);
        assert!(swell_late > 0.9, "swell peaks late: {swell_late}");
        // Symmetric matches the classic env.
        for p in [0.1, 0.3, 0.5, 0.8] {
            assert!((grain_env_skewed(p, 0.6, 0.0) - crate::dsp::grain_env(p, 0.6)).abs() < 1e-6);
        }
    }

    #[test]
    fn note_grain_is_a_sampler_mapping() {
        let s = GrainSettings { gain: 1.0, pitch: 0.0, ..Default::default() };
        let c4 = grain_from_note(&s, 60, 127, 4.0, 0.0, 0);
        assert!((c4.pitch_ratio - 1.0).abs() < 1e-6, "C4 plays at unity");
        let c5 = grain_from_note(&s, 72, 127, 4.0, 0.0, 1);
        assert!((c5.pitch_ratio - 2.0).abs() < 1e-5, "octave up doubles rate");
        let soft = grain_from_note(&s, 60, 64, 4.0, 0.0, 2);
        assert!((soft.gain - 64.0 / 127.0).abs() < 1e-5, "velocity -> gain");
    }

    #[test]
    fn scan_advances_read_head() {
        let s = GrainSettings {
            spray: 0.0,
            scan_speed: 1.0,
            position: 0.0,
            duration_jitter: 0.0,
            ..Default::default()
        };
        let ev = schedule_grains(&s, 0.0, 2.0, 10.0, 440.0, None);
        // Later grains should read later in the source.
        let first = ev.first().unwrap();
        let last = ev.last().unwrap();
        assert!(last.source_pos > first.source_pos + 1.0);
    }
}
