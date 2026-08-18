// chromagrain demo composition — run with:
//   chromagrain --script examples/demo.rhai
// Renders an 8-beat AV composition to demo-out.mp4 (needs ffmpeg).

let s = session(100.0);
s.canvas(640, 360, 24.0);

let src = s.demo_source(); // procedural saw stack + hue-sweep test video
print("source length: " + s.source_secs(src) + "s, base pitch: " + s.base_hz(src) + " Hz");

// Track 1: slow scanning cloud, quantized to A minor pentatonic,
// low-passed so only the body of the sound comes through.
let ta = s.track("low cloud");
let a = s.clip(ta, src, 0.0, 8.0);
s.key(a, "A", "minor_pentatonic");
s.set(a, "density", 24.0);
s.set(a, "duration", 0.15);
s.set(a, "pitch_jitter", 12.0);
s.set(a, "spray", 0.4);
s.set(a, "scan_speed", 0.5);
s.audio_filter(a, "lowpass", 2500.0, 0.9);

// Track 2: sparkly high grains an octave up, reversed sometimes,
// keeping only the cool (blue/cyan) hues of the video.
let tb = s.track("sparkle");
let b = s.clip(tb, src, 4.0, 4.0);
s.key(b, "A", "minor_pentatonic");
s.set(b, "density", 40.0);
s.set(b, "duration", 0.06);
s.set(b, "pitch", 12.0);
s.set(b, "pitch_jitter", 7.0);
s.set(b, "spray", 0.8);
s.set(b, "gain", 0.5);
s.set(b, "reverse_prob", 0.3);
s.set(b, "seed", 77.0);
s.color_keep(b, 200.0, 160.0);

if s.ffmpeg_available() {
    s.render(0.0, 8.0, "demo-out.mp4");
} else {
    print("ffmpeg not found — rendering wav only");
    s.render(0.0, 8.0, "demo-out.wav");
}
