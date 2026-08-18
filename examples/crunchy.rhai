// Crunchy AV demo: snippets + grain clouds through corresponding effect
// chains — bitcrush, delay, reverb smear, codec compression.
//   chromagrain --script examples/crunchy.rhai

let s = session(96.0);
s.canvas(640, 360, 24.0);

let src = s.demo_source();

// A snippet bed: the source played straight, an octave down and dim.
// Gain drives opacity (correspondence on by default).
let bed_t = s.track("bed");
let bed = s.snippet(bed_t, src, 0.0, 16.0);
s.set(bed, "pitch", -12.0);
s.set(bed, "gain", 0.4);
let bed_verb = s.effect(bed, "reverb");
s.fx(bed, bed_verb, "size", 0.8);
s.fx(bed, bed_verb, "damp", 0.6);   // audio damping == visual blur
s.fx(bed, bed_verb, "mix", 0.5);

// A crushed grain cloud: the downres/bitcrush you hear is the
// pixelation/posterize you see.
let cloud_t = s.track("crushed cloud");
let cloud = s.clip(cloud_t, src, 0.0, 16.0);
s.key(cloud, "A", "minor_pentatonic");
s.set(cloud, "density", 30.0);
s.set(cloud, "duration", 0.12);
s.set(cloud, "pitch_jitter", 12.0);
s.set(cloud, "spray", 0.6);
let cr = s.effect(cloud, "crush");
s.fx(cloud, cr, "downsample", 8.0);
s.fx(cloud, cr, "bits", 4.0);

// Sparse echoing hits: audio echoes == drifting visual ghosts.
let echo_t = s.track("echoes");
let hits = s.clip(echo_t, src, 4.0, 12.0);
s.key(hits, "A", "minor_pentatonic");
s.set(hits, "density", 3.0);
s.set(hits, "duration", 0.3);
s.set(hits, "pitch", 12.0);
s.set(hits, "gain", 0.6);
let dl = s.effect(hits, "delay");
s.fx(hits, dl, "time", 0.469);      // dotted 8th at 96 bpm
s.fx(hits, dl, "feedback", 0.6);
s.fx(hits, dl, "shift_x", 0.06);    // ghosts drift right as they decay

// Master bus: codec crunch over everything. Same transform-domain
// quantization in both domains: FFT bins (audio), DCT blocks (video).
let comp = s.master_effect("compress");
s.master_fx(comp, "quality", 0.3);

s.render(0.0, 16.0, "crunchy-out.mp4");
