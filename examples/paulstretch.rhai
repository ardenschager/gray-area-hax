// AV paulstretch: dissolve a sample into a spectral wash, then play it
// perfectly in key but subtly off pitch.
//
//   cargo run --release -- --script examples/paulstretch.rhai

let s = session(90.0);
s.canvas(480, 270, 24.0);

let src = s.demo_source();
let wash = s.paulstretch(src, 8.0);   // 8x slower, same pitch, smeared video

// The wash as a bed: granulated gently, snapped MOSTLY to A minor
// pentatonic (key strength 0.8) and detuned +9 cents after the snap.
let bed = s.track("wash bed");
let c = s.clip(bed, wash, 0.0, 16.0);
s.key(c, "A", "minor_pentatonic");
s.set(c, "quantize_amount", 0.8);
s.set(c, "detune_cents", 9.0);
s.set(c, "density", 12.0);
s.set(c, "duration", 0.6);
s.set(c, "spray", 2.0);
s.set(c, "gain", 0.7);
let rv = s.effect(c, "reverb");
s.fx(c, rv, "mix", 0.4);

// The original sample on top, hard-snapped for contrast.
let hits = s.track("hits");
let h = s.clip(hits, src, 0.0, 16.0);
s.key(h, "A", "minor_pentatonic");
s.set(h, "density", 3.0);
s.set(h, "duration", 0.15);
s.set(h, "pitch_jitter", 12.0);
s.set(h, "gain", 0.5);

s.render(0.0, 16.0, "paulstretch_demo.mp4");
