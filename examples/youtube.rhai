// Scrape a YouTube video and granulate it, quantized to C dorian.
//   chromagrain --script examples/youtube.rhai
// Requires yt-dlp (pip install yt-dlp) and ffmpeg on PATH.

let s = session(92.0);
s.canvas(640, 360, 24.0);

// Any yt-dlp-supported URL works. Downloads are capped at 480p and the
// first 90 seconds are decoded.
let src = s.youtube("https://www.youtube.com/watch?v=jNQXAC9IVRw");
print("scraped " + s.source_secs(src) + "s, detected base pitch " + s.base_hz(src) + " Hz");

let t = s.track("yt grains");
let c = s.clip(t, src, 0.0, 16.0);
s.key(c, "C", "dorian");
s.set(c, "density", 28.0);
s.set(c, "duration", 0.12);
s.set(c, "pitch_jitter", 9.0);
s.set(c, "spray", 1.2);
s.set(c, "scan_speed", 0.7);
s.audio_filter(c, "bandpass", 1000.0, 1.2);   // filter out audio frequencies
s.color_remove(c, 120.0, 80.0);               // filter out the green hue band

s.render(0.0, 16.0, "youtube-grains.mp4");
