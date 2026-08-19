// Step-sequenced AV composition (Koala / Nanoloop style), rendered to mp4.
//   chromagrain --script examples/sequencer.rhai

let s = session(96.0);
s.canvas(640, 360, 24.0);
let src = s.demo_source();

// A pattern: low hits on the floor, an answering line up top.
let p = s.pattern("floor");
s.pattern_grid(p, 4.0, 4);            // 4 beats, 16ths

let kick = s.row(p, src);
s.row_set(p, kick, "pitch", -17.0);
s.row_set(p, kick, "duration", 0.35);
s.row_key(p, kick, "A", "minor_pentatonic");
s.step(p, kick, 0, true); s.step(p, kick, 4, true);
s.step(p, kick, 8, true); s.step(p, kick, 12, true);

// A 5-step polymeter lead: its own step count cycles against the
// 16-step grid, drifting in and out of phase. Weird offset goodness.
let lead = s.row(p, src);
s.row_steps(p, lead, 5);
s.row_set(p, lead, "pitch", 7.0);
s.row_set(p, lead, "duration", 0.15);
s.row_set(p, lead, "gain", 0.6);
s.row_set(p, lead, "pan_spread", 0.8);
s.row_key(p, lead, "A", "minor_pentatonic");
s.step(p, lead, 0, true);
s.step(p, lead, 3, true); s.step_pitch(p, lead, 3, 12.0);

// A loop row: not sequenced — just repeats a slice of the source,
// pitched down, underneath everything.
let drone = s.row(p, src);
s.row_loop(p, drone, 0.1, 0.35);
s.row_set(p, drone, "pitch", -12.0);
s.row_set(p, drone, "gain", 0.35);
s.row_key(p, drone, "A", "minor_pentatonic");

// A dim snippet bed first — track order is the video stacking order,
// so later tracks composite on top of this.
let bed_t = s.track("bed");
let bed = s.snippet(bed_t, src, 0.0, 16.0);
s.set(bed, "pitch", -12.0);
s.set(bed, "gain", 0.3);
s.track_level(bed_t, 0.8);

// The sequencer lives on the timeline: a pattern clip, looping 16 beats,
// on a track drenched in AV delay (echoes drift across the canvas).
let t = s.track("seq");
let c = s.pattern_clip(t, p, 0.0, 16.0);
let d = s.track_effect(t, "delay");
s.track_fx(t, d, "time", 0.469);
s.track_fx(t, d, "feedback", 0.55);
s.track_fx(t, d, "shift_x", 0.05);

// Automation: the pattern clip's hit gain swells, and the bed track
// fades in over the arrangement (audio AND opacity, linked).
s.automate(c, "gain", 0.0, 0.4);
s.automate(c, "gain", 8.0, 1.1);
s.automate(c, "spray", 0.0, 0.0);
s.automate(c, "spray", 16.0, 1.5);
s.track_level_point(bed_t, 0.0, 0.0);
s.track_level_point(bed_t, 8.0, 1.0);

s.master_effect("compress");
s.render(0.0, 16.0, "sequencer-out.mp4");
