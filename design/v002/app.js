/* chromagrain · design v001 — interactive prototype
   One file, no build. Sections:
   0 util · 1 data · 2 media · 3 audio · 4 clock · 5 renderer · 6 play · 7 sequence
   8 perform · 9 deep drawer · 10 chrome (views, tempo, key, keys) · 11 output window · 12 demo */
(() => {
'use strict';

/* ───────────── 0 · util ───────────── */
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const lerp = (a, b, t) => a + (b - a) * t;
const rnd = (a = 1, b) => (b === undefined ? Math.random() * a : a + Math.random() * (b - a));
const gauss = () => (Math.random() + Math.random() + Math.random() - 1.5) / 1.5;
const now = () => performance.now() / 1000;
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
const params = new URLSearchParams(location.search);
const OUT_MODE = params.has('out');
const DEMO = params.has('demo');
let toastT = 0;
function toast(msg) { const t = $('#toast'); t.textContent = msg; t.classList.add('show'); clearTimeout(toastT); toastT = setTimeout(() => t.classList.remove('show'), 1900); }

/* ───────────── 1 · data ───────────── */
const ATLAS = { cols: 8, rows: 6, fw: 240, fh: 135, frames: 48, fps: 8, dur: 6 };
const SOURCES = [
  { id: 'oregon',      name: 'horses',      tone: { wave: 'triangle', oct: 0 },  sw: '#8f96ad' },
  { id: 'betty',       name: 'beanstalk',   tone: { wave: 'square',   oct: 1 },  sw: '#aeb2aa' },
  { id: 'aurora',      name: 'aurora',      tone: { wave: 'sine',     oct: -1 }, sw: '#6266ad' },
  { id: 'bees',        name: 'comb',        tone: { wave: 'sawtooth', oct: 1 },  sw: '#b7ad92' },
  { id: 'nyc',         name: 'concourse',   tone: { wave: 'triangle', oct: -1 }, sw: '#8e8b93' },
  { id: 'hummingbird', name: 'hummingbird', tone: { wave: 'sine',     oct: 2 },  sw: '#93b061' },
  { id: 'map',         name: 'rivers',      tone: { wave: 'triangle', oct: 1 },  sw: '#a19ea7' },
];
const V1 = '../v001/media/', V2 = 'media/';
const ORIGIN = { oregon: 'State of Oregon promotional reel · 1938', betty: 'Betty Boop · Jack and the Beanstalk · 1931', aurora: 'Aurora time-lapse · NASA Goddard', bees: 'The honey bee · macro · 1920s', nyc: 'Arteries of New York City · 1950s', hummingbird: 'Hummingbirds at play', map: 'Arteries of New York City · map' };
SOURCES.forEach(s => { s.base = V1; s.origin = ORIGIN[s.id]; s.file = `${s.id}.mp4`; });
/* "found" videos that Dig brings back (random, rarely seen; provenance kept on the source) */
const DUG = [
  { id: 'dog', name: 'IMG_4821', tone: { wave: 'triangle', oct: 0 }, sw: '#a8937a', base: V2, prov: { q: 'IMG_4821', title: 'IMG_4821.MOV', views: 37, up: 'Mar 2013', by: '@kmh1977' } },
  { id: 'fire', name: 'MVI_0032', tone: { wave: 'sine', oct: -1 }, sw: '#b07a4a', base: V2, prov: { q: 'MVI_0032', title: 'MVI_0032.AVI', views: 112, up: 'Nov 2009', by: '@gregorsen' } },
  { id: 'subway', name: 'DSC_1177', tone: { wave: 'triangle', oct: 1 }, sw: '#9a9a96', base: V2, prov: { q: 'DSC_1177', title: 'DSC_1177.MOV', views: 9, up: 'Jun 2011', by: '@railfan_pa' } },
];
const ALLSRC = [...SOURCES, ...DUG];
const SRC = Object.fromEntries(ALLSRC.map((s, i) => [s.id, Object.assign(s, { idx: i })]));
const GRID_COLS = [...SOURCES.map(s => s.id), null]; // 8th column empty: "drop a video"

const LOOKS = ['clean', 'tint', 'crush', 'kaleido', 'cutout', 'negative'];
const KEYS = [
  { root: 'A', midi: 57, scale: 'minor' }, { root: 'C', midi: 60, scale: 'major' },
  { root: 'D', midi: 62, scale: 'dorian' }, { root: 'E', midi: 52, scale: 'phrygian' },
  { root: 'F♯', midi: 54, scale: 'minor' }, { root: 'G', midi: 55, scale: 'mixolydian' },
];
const SCALES = { minor: [0, 2, 3, 5, 7, 8, 10], major: [0, 2, 4, 5, 7, 9, 11], dorian: [0, 2, 3, 5, 7, 9, 10], phrygian: [0, 1, 3, 5, 7, 8, 10], mixolydian: [0, 2, 4, 5, 7, 9, 10] };
const NOTE = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];

const S = {
  view: 'play', bpm: 112, keyIdx: 0, playing: false, recording: false, recT0: 0,
  step: -1, stepTime: 0,
  quant: 16, out: 'here',
  selEm: 0,
  scene: 0, queuedScene: null, chainPos: 0,
};
const key = () => KEYS[S.keyIdx];
function degToSemi(d) { const sc = SCALES[key().scale], n = sc.length; const o = Math.floor(d / n); return o * 12 + sc[((d % n) + n) % n]; }
function noteName(d) { return NOTE[(key().midi + degToSemi(d)) % 12]; }

/* emitters — Tempera-style brushes. Each holds its own grain settings. */
const EMITTERS = [
  { col: 0, y: 0.30, grain: 0.18, pitch: 0, smear: 0.25, look: 0 },
  { col: 2, y: 0.62, grain: 0.82, pitch: 2, smear: 0.55, look: 1 },
  { col: 5, y: 0.20, grain: 0.55, pitch: 4, smear: 0.35, look: 0 },
  { col: 4, y: 0.80, grain: 0.35, pitch: -3, smear: 0.15, look: 2 },
].map((e, i) => Object.assign(e, { i, on: false, acc: 0, lastCol: e.col }));

/* sequencer — lanes are sources; 16 steps; per-lane length (polymeter); patterns A–H; scenes; song chain */
function mkPat(len, hits, opts = {}) {
  return Array.from({ length: 16 }, (_, i) => {
    const h = hits[i];
    return { on: h !== undefined, slice: h?.s ?? ((i * (opts.stride || 1) + (opts.off || 0)) % 16), pitch: h?.p ?? 0, gate: h?.g ?? 1, chance: h?.c ?? 100 };
  });
}
const H = (arr) => Object.fromEntries(arr.map(a => (Array.isArray(a) ? [a[0], { s: a[1], p: a[2], g: a[3] }] : [a, {}])));
const LANES = [
  { src: 'oregon', len: 16, mode: 'frame', pats: { A: mkPat(16, H([[0, 0], [4, 4], [8, 8], [10, 9], [12, 12]])), B: mkPat(16, H([[0, 0], [3, 2], [6, 6], [8, 8], [11, 10], [12, 12], [14, 14]])) } },
  { src: 'betty', len: 12, mode: 'frame', pats: { A: mkPat(12, H([[2, 3, 0], [5, 6, 2], [9, 10, 4], [11, 12, -1]])), B: mkPat(12, H([[0, 1], [2, 3, 2], [5, 6, 4], [7, 8, 5], [9, 10, 7]])) } },
  { src: 'nyc', len: 16, mode: 'band', pats: { A: mkPat(16, H([[2, 9], [6, 10], [10, 11], [14, 12], [15, 13, 3]])), B: mkPat(16, H([[2, 4], [3, 5], [6, 6], [10, 11], [13, 12], [14, 13]])) } },
  { src: 'hummingbird', len: 7, mode: 'cloud', pats: { A: mkPat(7, H([[0, 3, 4], [3, 7, 2], [5, 11, 7]])), B: mkPat(7, H([[0, 2, 7], [2, 6, 9], [4, 9, 4], [6, 13, 2]])) } },
  { src: 'aurora', len: 16, mode: 'frame', pats: { A: mkPat(16, H([[0, 0, 0, 4], [8, 6, -2, 4]])), B: mkPat(16, H([[0, 2, 0, 6], [12, 10, 3, 4]])) } },
  { src: 'bees', len: 10, mode: 'cloud', pats: { A: mkPat(10, H([[1, 2, 7], [4, 5, 9], [6, 9, 11], [9, 12, 4]])), B: mkPat(10, H([[1, 1, 9], [3, 4, 11], [7, 8, 14]])) } },
  { src: 'map', len: 16, mode: 'band', pats: { A: mkPat(16, H([[7, 4, 0], [15, 12, 2]])), B: mkPat(16, H([[3, 2], [7, 6], [11, 10], [15, 14, 5]])) } },
].map((l, i) => Object.assign(l, { i, mute: false, pos: -1, flashAt: new Float64Array(16) }));
const SCENES = [
  ['A', null, 'A', null, 'A', null, null],
  ['A', 'A', 'A', null, 'A', 'A', null],
  ['A', 'A', 'A', 'A', 'A', 'A', 'A'],
  ['B', 'B', 'B', 'B', 'B', 'B', 'B'],
  null, null, null, null,
];
const CHAIN = [{ s: 0, r: 2 }, { s: 1, r: 2 }, { s: 2, r: 4 }, { s: 3, r: 2 }, { s: 2, r: 2 }];

/* perform pads — 4×4 */
const PADS = [
  { src: 'oregon', mode: 'loop', look: 0 }, { src: 'betty', mode: 'loop', look: 0 }, { src: 'aurora', mode: 'loop', look: 0 }, { src: 'map', mode: 'loop', look: 4 },
  { src: 'hummingbird', mode: 'grain', look: 0 }, { src: 'bees', mode: 'grain', look: 0 }, { src: 'nyc', mode: 'grain', look: 1 }, { src: 'oregon', mode: 'grain', look: 0 },
  { src: 'betty', mode: 'chop', look: 0 }, { src: 'nyc', mode: 'chop', look: 0 }, { src: 'map', mode: 'chop', look: 5 }, { src: 'hummingbird', mode: 'chop', look: 2 },
  { src: 'aurora', mode: 'loop', look: 3 }, { src: 'bees', mode: 'chop', look: 2 }, { src: 'oregon', mode: 'loop', look: 4 }, null,
].map((p, i) => p && Object.assign(p, { i, on: false, q: false, t0: 0, frame: rnd(48) | 0 }));

/* ───────────── 2 · media ───────────── */
const media = {};
ALLSRC.forEach(s => {
  const img = new Image(); img.src = `${s.base}${s.id}_atlas.jpg`;
  const v = document.createElement('video');
  Object.assign(v, { src: `${s.base}${s.id}.mp4`, muted: true, loop: true, playsInline: true, autoplay: true, preload: 'auto', poster: `${s.base}${s.id}_poster.jpg` });
  v.setAttribute('muted', ''); v.setAttribute('playsinline', '');
  v.playbackRate = 1;
  $('#media-bin').appendChild(v);
  v.play().catch(() => {});
  media[s.id] = { img, v, buf: null };
});
const atlasURL = id => `url(${SRC[id].base}${id}_atlas.jpg)`;
function frameXY(f) { f = ((f | 0) % ATLAS.frames + ATLAS.frames) % ATLAS.frames; return [f % ATLAS.cols, (f / ATLAS.cols) | 0]; }
/* CSS: show atlas frame f in an element with cover-crop for the element's aspect a = w/h */
function bgFrame(e, id, f, a) {
  const [c, r] = frameXY(f); const fa = 16 / 9;
  e.style.backgroundImage = atlasURL(id);
  if (a < fa) {
    const fw = fa / a, bw = ATLAS.cols * fw; const ox = c * fw + (fw - 1) / 2;
    e.style.backgroundSize = `${bw * 100}% ${ATLAS.rows * 100}%`;
    e.style.backgroundPosition = `${(ox / (bw - 1)) * 100}% ${(r / (ATLAS.rows - 1)) * 100}%`;
  } else {
    const fh = a / fa, bh = ATLAS.rows * fh; const oy = r * fh + (fh - 1) / 2;
    e.style.backgroundSize = `${ATLAS.cols * 100}% ${bh * 100}%`;
    e.style.backgroundPosition = `${(c / (ATLAS.cols - 1)) * 100}% ${(oy / (bh - 1)) * 100}%`;
  }
}

/* ───────────── 3 · audio: every grain is a sound ───────────── */
const A = { ctx: null };
function initAudio() {
  if (A.ctx || OUT_MODE) return;
  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const master = ctx.createGain(); master.gain.value = 0.55;
    const comp = ctx.createDynamicsCompressor(); comp.threshold.value = -14; comp.ratio.value = 4;
    master.connect(comp).connect(ctx.destination);
    // smear bus: reverb + echo
    const send = ctx.createGain(); send.gain.value = 1;
    const conv = ctx.createConvolver();
    const len = ctx.sampleRate * 3, ir = ctx.createBuffer(2, len, ctx.sampleRate);
    for (let ch = 0; ch < 2; ch++) { const d = ir.getChannelData(ch); for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3.2); }
    conv.buffer = ir;
    const dly = ctx.createDelay(2); dly.delayTime.value = (60 / S.bpm) * 0.75;
    const fb = ctx.createGain(); fb.gain.value = 0.38;
    const dlyOut = ctx.createGain(); dlyOut.gain.value = 0.5;
    send.connect(conv).connect(master);
    send.connect(dly); dly.connect(fb).connect(dly); dly.connect(dlyOut).connect(master);
    Object.assign(A, { ctx, master, send, dly });
    if (location.protocol !== 'file:') SOURCES.forEach(s => {
      fetch(`${s.base}${s.id}.mp4`).then(r => r.arrayBuffer()).then(b => ctx.decodeAudioData(b)).then(buf => {
        // only use real audio if the clip actually has some energy
        const d = buf.getChannelData(0); let e = 0; for (let i = 0; i < d.length; i += 64) e += d[i] * d[i];
        if (e / (d.length / 64) > 1e-5) media[s.id].buf = buf;
      }).catch(() => { /* file:// or no audio track → synth fallback */ });
    });
  } catch (e) { /* no audio, visuals only */ }
}
/* one grain: src position (0..1), duration (s), degree offset, pan, gain, smear */
function audioGrain(id, pos, dur, deg, pan = 0, gain = 0.5, smear = 0.2, synthDeg = 0) {
  const ctx = A.ctx; if (!ctx || ctx.state !== 'running') return;
  const t = ctx.currentTime + 0.005;
  const g = ctx.createGain(); const p = ctx.createStereoPanner ? ctx.createStereoPanner() : null;
  const atk = Math.min(0.02, dur * 0.3), rel = Math.max(0.03, dur * 0.7);
  g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(gain, t + atk); g.gain.linearRampToValueAtTime(0, t + dur + rel * 0.3);
  const semis = degToSemi(deg);
  const buf = media[id].buf;
  let srcNode;
  if (buf) {
    srcNode = ctx.createBufferSource(); srcNode.buffer = buf; srcNode.playbackRate.value = Math.pow(2, semis / 12);
    srcNode.start(t, clamp(pos, 0, 0.98) * buf.duration, dur + 0.05);
  } else {
    const tone = SRC[id].tone;
    const f = 440 * Math.pow(2, (key().midi + semis + degToSemi(synthDeg) - 69) / 12 + tone.oct);
    srcNode = ctx.createOscillator(); srcNode.type = tone.wave; srcNode.frequency.value = f;
    const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = clamp(f * 3, 400, 6000); lp.Q.value = 0.7;
    srcNode.connect(lp); srcNode = Object.assign(srcNode, { _out: lp });
    srcNode.start(t); srcNode.stop(t + dur + rel + 0.05);
    gain *= tone.wave === 'sawtooth' || tone.wave === 'square' ? 0.22 : 0.45;
    g.gain.cancelScheduledValues(t); g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(gain, t + atk); g.gain.exponentialRampToValueAtTime(0.0008, t + dur + rel);
  }
  (srcNode._out || srcNode).connect(g);
  let out = g; if (p) { p.pan.value = clamp(pan, -1, 1); g.connect(p); out = p; }
  out.connect(A.master);
  if (smear > 0.02) { const sg = ctx.createGain(); sg.gain.value = smear * 0.7; out.connect(sg).connect(A.send); }
}

/* ───────────── 4 · clock ───────────── */
const stepDur = () => 60 / S.bpm / 4;
let nextStepAt = 0;
function setPlaying(on) {
  if (on === S.playing) return;
  S.playing = on; document.getElementById('app').classList.toggle('playing', on);
  if (on) { initAudio(); A.ctx?.resume?.(); S.step = -1; nextStepAt = now() + 0.02; }
  else { LANES.forEach(l => (l.pos = -1)); PADS.forEach(p => p && (p.q = false)); renderPads(); }
  broadcast();
}
setInterval(() => {
  if (!S.playing) return;
  const t = now();
  let guard = 0;
  while (t >= nextStepAt && guard++ < 8) { onStep(++S.step); nextStepAt += stepDur(); }
}, 6);

function chainSceneAt(bar) {
  const total = CHAIN.reduce((a, c) => a + c.r, 0); let b = bar % total;
  for (let k = 0; k < CHAIN.length; k++) { if (b < CHAIN[k].r) return { k, s: CHAIN[k].s, rep: b, r: CHAIN[k].r }; b -= CHAIN[k].r; }
  return { k: 0, s: 0, rep: 0, r: 1 };
}
let chainOffset = 0; // bars offset so tapping a scene jumps the chain
function onStep(g) {
  S.stepTime = now();
  const bar = Math.floor(g / 16), inBar = g % 16;
  if (inBar === 0) {
    if (S.queuedScene != null) {
      const k = CHAIN.findIndex(c => c.s === S.queuedScene);
      if (k >= 0) { const start = CHAIN.slice(0, k).reduce((a, c) => a + c.r, 0); chainOffset = start - bar; }
      S.queuedScene = null;
    }
    const cs = chainSceneAt(bar + chainOffset); S.scene = cs.s; S.chainPos = cs.k; S.chainRep = cs.rep;
    if (S.view === 'seq') renderSong(), renderLanes();
  }
  // sequencer
  const sc = SCENES[S.scene];
  LANES.forEach(l => {
    l.pos = g % l.len;
    const slot = sc && sc[l.i]; if (!slot || l.mute) return;
    const st = l.pats[slot][l.pos];
    if (st.on && Math.random() * 100 < st.chance) triggerLane(l, st);
  });
  // pads: quantized launch
  const q = S.quant;
  if (q === 0 || g % q === 0) {
    let changed = false;
    PADS.forEach(p => { if (p && p.q) { p.q = false; setPad(p, !p.on); changed = true; } });
    if (changed) { limitPads(); renderPads(); broadcast(); }
  }
  PADS.forEach(p => p && p.on && p.mode !== 'slice' && padStep(p, g));
  if (S.recording && inBar === 0) {}
  broadcast({ t: 'tick', g });
}

/* ───────────── 5 · renderer (canvas2D; maps to a wgpu quad pass in Rust) ───────────── */
const world = { grains: [], hits: [] };
const off = document.createElement('canvas'); const offx = off.getContext('2d');
class Stage {
  constructor(canvas) { this.c = canvas; this.x = canvas.getContext('2d', { alpha: false }); this.w = 0; this.h = 0; this.baseA = 0.9; this.slots = []; this.lastRects = {}; new ResizeObserver(() => this.resize()).observe(canvas); this.resize(); }
  resize() { const r = this.c.getBoundingClientRect(); const dpr = Math.min(devicePixelRatio || 1, 2); const w = Math.max(2, Math.round(Math.min(r.width * dpr, 1920))), h = Math.max(2, Math.round(Math.min(r.height * dpr, 1080))); if (w !== this.w || h !== this.h) { this.c.width = this.w = w; this.c.height = this.h = h; this.x.fillStyle = '#000'; this.x.fillRect(0, 0, w, h); } }
  visible() { return this.c.offsetParent !== null || document.fullscreenElement?.contains(this.c); }
}
function drawLook(x, src, sx, sy, sw, sh, dx, dy, dw, dh, look, alpha = 1) {
  if (alpha <= 0) return;
  x.save(); x.globalAlpha = alpha;
  const L = LOOKS[look] || 'clean';
  try {
    if (L === 'crush') {
      const k = 9; off.width = Math.max(2, dw / k | 0); off.height = Math.max(2, dh / k | 0);
      offx.imageSmoothingEnabled = true; offx.drawImage(src, sx, sy, sw, sh, 0, 0, off.width, off.height);
      x.imageSmoothingEnabled = false; x.drawImage(off, 0, 0, off.width, off.height, dx, dy, dw, dh); x.imageSmoothingEnabled = true;
    } else if (L === 'kaleido') {
      const hw = dw / 2, hh = dh / 2, csx = sx + sw * 0.2, csw = sw * 0.3, csh = sh * 0.5, csy = sy + sh * 0.2;
      for (let q = 0; q < 4; q++) { x.save(); x.translate(dx + hw, dy + hh); x.scale(q % 2 ? -1 : 1, q > 1 ? -1 : 1); x.drawImage(src, csx, csy, csw, csh, -hw, -hh, hw, hh); x.restore(); }
    } else if (L === 'cutout') {
      x.globalCompositeOperation = 'lighten'; x.drawImage(src, sx, sy, sw, sh, dx, dy, dw, dh);
    } else {
      x.drawImage(src, sx, sy, sw, sh, dx, dy, dw, dh);
      if (L === 'tint') { x.globalCompositeOperation = 'color'; x.globalAlpha = alpha * 0.8; x.fillStyle = '#c98a4b'; x.fillRect(dx, dy, dw, dh); }
      if (L === 'negative') { x.globalCompositeOperation = 'difference'; x.fillStyle = '#fff'; x.fillRect(dx, dy, dw, dh); }
    }
  } catch (e) {}
  x.restore();
}
function srcDims(m) { const v = m.v; return v.readyState >= 2 && v.videoWidth ? [v, v.videoWidth, v.videoHeight] : null; }
/* cover-crop a source of size sw×sh into a rect of rw×rh → source sub-rect */
function cover(sw, sh, rw, rh) { const sa = sw / sh, ra = rw / rh; if (ra > sa) { const h = sw / ra; return [0, (sh - h) / 2, sw, h]; } const w = sh * ra; return [(sw - w) / 2, 0, w, sh]; }
const full = st => ({ x: 0, y: 0, w: st.w, h: st.h });

/* ADSR — one envelope shapes the sound AND the picture.
   cut (default): picture is fully on while the envelope is above a threshold, then a hard cut.
   fade: picture opacity follows the envelope. */
function envAt(o, t0, tOff, t) {
  const A = Math.max(0.001, o.a), D = Math.max(0.001, o.d), Sv = o.s, R = Math.max(0.001, o.r);
  const pre = tt => (tt < 0 ? 0 : tt < A ? tt / A : tt < A + D ? 1 - (1 - Sv) * (tt - A) / D : Sv);
  if (tOff == null || t < tOff) return pre(t - t0);
  return Math.max(0, pre(tOff - t0) * (1 - (t - tOff) / R));
}
const vis = (o, e) => (o.cut === 'fade' ? e : e > o.thresh ? 1 : 0);

/* grains: a patch of picture with the same life as its sound */
function spawnGrain(o) { world.grains.push(Object.assign({ t0: now() }, o)); if (world.grains.length > 900) world.grains.splice(0, world.grains.length - 900); }
function drawGrains(st, t, filter, R) {
  const x = st.x; R = R || full(st);
  const clip = R.w !== st.w || R.h !== st.h; if (clip) { x.save(); x.beginPath(); x.rect(R.x, R.y, R.w, R.h); x.clip(); }
  for (const g of world.grains) {
    if (filter && !filter(g)) continue;
    const ph = (t - g.t0) / g.dur; if (ph < 0 || ph > 1) continue;
    const env = g.cut ? 1 : Math.sin(Math.PI * ph);
    const img = media[g.src].img; if (!img.complete || !img.naturalWidth) continue;
    const [c, r] = frameXY(g.f + ph * g.dur * ATLAS.fps * (g.rev ? -1 : 1));
    const fx = c * ATLAS.fw, fy = r * ATLAS.fh;
    const dx = R.x + g.x * R.w, dy = R.y + g.y * R.h, dw = g.w * R.w, dh = g.h * R.h;
    const sx = fx + clamp(g.u, 0, 1 - g.w) * ATLAS.fw, sy = fy + clamp(g.v, 0, 1 - g.h) * ATLAS.fh;
    drawLook(x, img, sx, sy, g.w * ATLAS.fw, g.h * ATLAS.fh, dx - dw / 2, dy - dh / 2, dw, dh, g.look || 0, env * (g.a ?? 1));
    if (g.pitch) { x.save(); x.globalCompositeOperation = 'color'; x.globalAlpha = env * Math.min(0.32, Math.abs(g.pitch) / 14); x.fillStyle = `hsl(${(30 + g.pitch * 22 + 360) % 360},60%,55%)`; x.fillRect(dx - dw / 2, dy - dh / 2, dw, dh); x.restore(); }
  }
  if (clip) x.restore();
}
function hitAlpha(h, t) {
  const age = t - h.t0; if (age < 0 || age > h.dur) return 0;
  if (h.o) return vis(h.o, envAt(h.o, h.t0, h.t0 + h.gate, t));
  const ph = age / h.dur; return age < 0.015 ? age / 0.015 : ph < 0.55 ? 1 : Math.pow(1 - (ph - 0.55) / 0.45, 1.6);
}
function drawHits(st, t, filter, R) {
  const x = st.x; R = R || full(st);
  const clip = R.w !== st.w || R.h !== st.h; if (clip) { x.save(); x.beginPath(); x.rect(R.x, R.y, R.w, R.h); x.clip(); }
  for (const h of world.hits) {
    if (filter && !filter(h)) continue;
    const env = hitAlpha(h, t); if (env <= 0) continue;
    const age = t - h.t0;
    const img = media[h.src].img; if (!img.complete) continue;
    const [c, r] = frameXY(h.f + age * ATLAS.fps);
    const fx = c * ATLAS.fw, fy = r * ATLAS.fh;
    const look = h.o ? h.o.look : h.look || 0;
    if (h.mode === 'band') {
      drawLook(x, img, fx, fy + h.by * ATLAS.fh, ATLAS.fw, h.bh * ATLAS.fh, R.x, R.y + h.by * R.h, R.w, h.bh * R.h, look, env);
    } else {
      x.save(); if (h.blend && !clip) x.globalCompositeOperation = h.blend;
      const [sx, sy, sw, sh] = cover(ATLAS.fw, ATLAS.fh, R.w, R.h);
      drawLook(x, img, fx + sx, fy + sy, sw, sh, R.x, R.y, R.w, R.h, look, env * 0.94); x.restore();
    }
    if (h.pitch) { x.save(); x.globalCompositeOperation = 'color'; x.globalAlpha = env * Math.min(0.3, Math.abs(h.pitch) / 14); x.fillStyle = `hsl(${(30 + h.pitch * 22 + 360) % 360},60%,55%)`; if (h.mode === 'band') x.fillRect(R.x, R.y + h.by * R.h, R.w, h.bh * R.h); else x.fillRect(R.x, R.y, R.w, R.h); x.restore(); }
  }
  if (clip) x.restore();
}
function prune(t) {
  if (world.grains.length && world.grains[0].t0 + 2 < t) world.grains = world.grains.filter(g => t - g.t0 < g.dur + 0.05);
  if (world.hits.length > 40 || (world.hits.length && t - world.hits[0].t0 > 5)) world.hits = world.hits.filter(h => t - h.t0 < h.dur);
}

/* TILE layout: every sounding voice gets its own tile. Stable slots (tiles never jump),
   hard cuts governed by ADSR cut, hairline gap, no animation. Tile column → stereo pan. */
function tileRects(st, keys) {
  const sl = st.slots;
  for (let k = 0; k < sl.length; k++) if (sl[k] != null && !keys.includes(sl[k])) sl[k] = null;
  keys.forEach(k => { if (!sl.includes(k)) { const f = sl.indexOf(null); if (f >= 0) sl[f] = k; else sl.push(k); } });
  while (sl.length && sl[sl.length - 1] == null) sl.pop();
  const n = sl.length; const [c, r] = n <= 1 ? [1, 1] : n === 2 ? [2, 1] : n <= 4 ? [2, 2] : n <= 6 ? [3, 2] : n <= 9 ? [3, 3] : [4, 4];
  const gap = Math.max(2, Math.round(st.w / 640));
  const tw = (st.w - gap * (c - 1)) / c, th = (st.h - gap * (r - 1)) / r;
  const out = {};
  sl.forEach((k, i) => { if (k == null) return; const cx = i % c, cy = (i / c) | 0; out[k] = { x: cx * (tw + gap), y: cy * (th + gap), w: tw, h: th, col: cx, cols: c, row: cy, rows: r }; });
  st.lastRects = out; return out;
}
function slotPan(st, key) { const r = st && st.lastRects[key]; return r && r.cols > 1 ? (r.col / (r.cols - 1)) * 2 - 1 : 0; }
function blackout(st) { const x = st.x; x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1; x.fillStyle = '#000'; x.fillRect(0, 0, st.w, st.h); }

/* ───────────── 6 · PLAY — the video grid + emitters ───────────── */
const vgrid = $('#vgrid');
const cells = [];
function buildGrid() {
  vgrid.innerHTML = '';
  for (let r = 0; r < 8; r++) for (let c = 0; c < 8; c++) {
    const id = GRID_COLS[c]; const d = el('div', 'vcell'); d.dataset.c = c; d.dataset.r = r;
    if (!id) { d.classList.add('empty'); if (r === 0) { d.classList.add('first'); d.textContent = '+'; } if (DIGGING.has(c)) d.classList.add('digging'); }
    vgrid.appendChild(d); cells[r * 8 + c] = d;
  }
  EMITTERS.forEach(e => { const m = el('div', 'emk', `<svg class="emr" viewBox="0 0 80 80"></svg><i class="ring"></i><i class="ring"></i><i class="ring"></i><b>${e.i + 1}</b>`); vgrid.appendChild(m); e.mk = m; });
  buildHeads(); layoutGrid();
}
function layoutGrid() {
  const c0 = cells[0]; if (!c0) return; const r = c0.getBoundingClientRect(); const a = r.width / Math.max(1, r.height) || 1;
  cells.forEach((d, k) => { const id = GRID_COLS[k % 8]; if (id) bgFrame(d, id, ((k / 8) | 0) * 6 + 2, a); });
  placeEmitters();
}
function gridGeom() { const r = vgrid.getBoundingClientRect(); return { r, cw: (r.width + 4) / 8 }; }
function placeEmitters() {
  const { r, cw } = gridGeom();
  EMITTERS.forEach(e => { e.mk.style.left = `${e.col * cw + (cw - 4) / 2}px`; e.mk.style.top = `${e.y * r.height}px`; e.mk.classList.toggle('sel', e.i === S.selEm); e.mk.classList.toggle('on', e.on); const sv = e.mk.querySelector('.emr'); if (sv) sv.innerHTML = S.dir === 'C' ? markerRing(e) : ''; });
  cells.forEach((d, k) => d.classList.toggle('col-on', EMITTERS.some(e => e.on && e.col === k % 8)));
}
const touches = new Map();
vgrid.addEventListener('pointerdown', ev => {
  initAudio(); A.ctx?.resume?.();
  const { r, cw } = gridGeom(); const px = ev.clientX - r.left, py = ev.clientY - r.top;
  const col = clamp(Math.floor(px / cw), 0, 7);
  if (!GRID_COLS[col]) { if (!DIGGING.has(col)) openAdd(); return; }
  // grab an emitter under the finger, otherwise the selected brush jumps here
  let em = EMITTERS.find(e => !e.on && Math.hypot(e.col * cw + cw / 2 - px, e.y * r.height - py) < 30);
  if (!em) { em = EMITTERS[S.selEm]; if (em.on) em = EMITTERS.find(e => !e.on) || em; }
  selectEmitter(em.i);
  vgrid.setPointerCapture(ev.pointerId);
  touches.set(ev.pointerId, em);
  moveEm(em, px, py); em.on = true; em.envT0 = now(); em.relT = null; em.mk.classList.add('dragging'); placeEmitters();
});
vgrid.addEventListener('pointermove', ev => { const em = touches.get(ev.pointerId); if (!em) return; const { r } = gridGeom(); moveEm(em, ev.clientX - r.left, ev.clientY - r.top); placeEmitters(); });
const endTouch = ev => { const em = touches.get(ev.pointerId); if (!em) return; touches.delete(ev.pointerId); em.on = false; em.relT = now(); em.mk.classList.remove('dragging'); placeEmitters(); refreshEmitterButtons(); };
vgrid.addEventListener('pointerup', endTouch); vgrid.addEventListener('pointercancel', endTouch);
function moveEm(em, px, py) {
  const { r, cw } = gridGeom(); let col = clamp(Math.floor(px / cw), 0, 7); if (!GRID_COLS[col]) col = em.col;
  const changed = em.col !== col; em.col = col; em.y = clamp(py / r.height, 0.001, 0.999); em.lastCol = col; if (changed) refreshEmitterButtons();
  if (S.recording) S.recEvents.push([now() - S.recT0, em.i, col, em.y]);
}
/* emission: runs every frame for each held emitter */
const litAt = new Float64Array(64);
function emitFrom(em, dt) {
  const id = GRID_COLS[em.col]; if (!id) return;
  const env = envAt(em, em.envT0, em.relT, now()), va = vis(em, env);
  if (env <= 0.001 && !em.on) return;
  const g = em.grain; // 0 = sample, 1 = cloud
  const rate = lerp(5, 70, g * g);
  em.acc += dt * rate;
  while (em.acc >= 1) {
    em.acc -= 1;
    const pos = clamp(em.y + gauss() * lerp(0.01, 0.05, g), 0, 0.999);
    const dur = lerp(0.75, 0.05, Math.pow(g, 0.7)) * rnd(0.8, 1.2);
    const size = lerp(0.85, 0.07, Math.pow(g, 0.8)) * rnd(0.7, 1.15);
    const pan = gauss() * lerp(0.15, 0.9, g);
    const cx = clamp(0.5 + pan * 0.45, 0.05, 0.95), cy = clamp(0.5 + gauss() * lerp(0.1, 0.45, g), 0.05, 0.95);
    const w = Math.min(size, 1), h = Math.min(size * rnd(0.7, 1.1), 1);
    if (va > 0) spawnGrain({ src: id, f: pos * 47, x: cx, y: cy, w, h, u: cx - w / 2, v: cy - h / 2, dur, look: em.look, pitch: em.pitch, a: (g < 0.3 ? 0.95 : 0.9) * va, em: em.i, cut: em.cut === 'cut' });
    audioGrain(id, pos, dur * 0.8, em.pitch, pan, lerp(0.5, 0.18, g) * env * em.level, em.smear, Math.round(pos * 7));
    litAt[((pos * 8) | 0) * 8 + em.col] = now();
  }
}
/* emitter buttons + knobs (bound to the selected emitter) */
const emBox = $('#emitters');
function buildEmitterButtons() {
  emBox.innerHTML = '';
  EMITTERS.forEach(e => {
    const b = el('button', 'em', `<span class="n">${e.i + 1}</span><span class="t">${LOOKS[e.look] === 'clean' ? '' : LOOKS[e.look]}</span>`);
    b.setAttribute('role', 'radio'); b.id = `em-${e.i + 1}`;
    b.addEventListener('click', () => selectEmitter(e.i));
    let ht; b.addEventListener('pointerdown', () => { ht = setTimeout(() => emitterHold(e), 520); });
    b.addEventListener('pointerup', () => clearTimeout(ht)); b.addEventListener('pointerleave', () => clearTimeout(ht));
    e.btn = b; emBox.appendChild(b);
  });
  refreshEmitterButtons();
}
function refreshEmitterButtons() {
  EMITTERS.forEach(e => {
    const id = GRID_COLS[e.col]; e.btn.classList.toggle('sel', e.i === S.selEm); e.btn.setAttribute('aria-checked', e.i === S.selEm);
    e.btn.style.backgroundImage = atlasURL(id);
    const [c, r] = frameXY(((e.y * 8) | 0) * 6 + 2); e.btn.style.backgroundSize = '800% 600%'; e.btn.style.backgroundPosition = `${c / 7 * 100}% ${r / 5 * 100}%`;
    e.btn.querySelector('.t').textContent = LOOKS[e.look] === 'clean' ? GRID_COLS[e.col] && SRC[GRID_COLS[e.col]].name : LOOKS[e.look];
  });
}
function selectEmitter(i) { select({ kind: 'emitter', ref: EMITTERS[i] }); }

/* knob: custom-painted arc. drag vertical; double-tap resets. */
const knobs = [];
const MACROS = {
  grain: { label: 'Grain', min: 0, max: 1, def: 0.4, fmt: v => (v < 0.15 ? 'sample' : v > 0.85 ? 'cloud' : `${Math.round(v * 100)}`) },
  pitch: { label: 'Pitch', min: -7, max: 7, def: 0, step: 1, bipolar: true, fmt: v => `${v > 0 ? '+' : ''}${v} ${noteName(v)}` },
  smear: { label: 'Smear', min: 0, max: 1, def: 0.2, fmt: v => `${Math.round(v * 100)}` },
  look: { label: 'Look', min: 0, max: LOOKS.length - 1, def: 0, step: 1, detents: LOOKS.length, fmt: v => LOOKS[v] },
};
function arcPath(cx, cy, r, a0, a1) { const p = a => [cx + r * Math.sin(a), cy - r * Math.cos(a)]; const [x0, y0] = p(a0), [x1, y1] = p(a1); const large = Math.abs(a1 - a0) > Math.PI ? 1 : 0; return `M${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 ${large} ${a1 > a0 ? 1 : 0} ${x1.toFixed(2)} ${y1.toFixed(2)}`; }
function makeKnob(host) {
  const name = host.dataset.macro, M = MACROS[name];
  host.innerHTML = `<svg viewBox="0 0 100 100"></svg><span class="k-l">${M.label}</span><span class="k-v"></span>`;
  const svg = host.querySelector('svg'), vEl = host.querySelector('.k-v');
  const A0 = -0.75 * Math.PI, A1 = 0.75 * Math.PI;
  const k = {
    name, get: () => EMITTERS[S.selEm][name], set: v => { EMITTERS[S.selEm][name] = v; },
    draw() {
      const v = k.get(), n = (v - M.min) / (M.max - M.min), a = lerp(A0, A1, n);
      let s = `<defs><linearGradient id="kb-${name}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2c2c30"/><stop offset="1" stop-color="#18181a"/></linearGradient></defs>`;
      s += `<path d="${arcPath(50, 50, 44, A0, A1)}" stroke="rgba(255,255,255,.07)" stroke-width="5" fill="none" stroke-linecap="round"/>`;
      if (M.detents) {
        for (let i = 0; i < M.detents; i++) { const da = lerp(A0, A1, i / (M.detents - 1)); s += `<circle cx="${50 + 44 * Math.sin(da)}" cy="${50 - 44 * Math.cos(da)}" r="${i === v ? 3.6 : 2.2}" fill="${i === v ? '#f3f2ee' : 'rgba(255,255,255,.22)'}"/>`; }
      } else {
        const from = M.bipolar ? 0 : A0; if (Math.abs(a - from) > 0.01) s += `<path d="${arcPath(50, 50, 44, Math.min(from, a), Math.max(from, a))}" stroke="#f3f2ee" stroke-width="5" fill="none" stroke-linecap="round"/>`;
        if (M.bipolar) s += `<line x1="50" y1="2" x2="50" y2="9" stroke="rgba(255,255,255,.25)" stroke-width="1.5"/>`;
      }
      s += `<circle cx="50" cy="50" r="33" fill="url(#kb-${name})"/><circle cx="50" cy="50" r="32.5" fill="none" stroke="rgba(255,255,255,.07)"/>`;
      s += `<path d="M27 38 A 26 26 0 0 1 73 38" stroke="rgba(255,255,255,.06)" stroke-width="1" fill="none"/>`;
      s += `<line x1="${50 + 14 * Math.sin(a)}" y1="${50 - 14 * Math.cos(a)}" x2="${50 + 27 * Math.sin(a)}" y2="${50 - 27 * Math.cos(a)}" stroke="#f3f2ee" stroke-width="3" stroke-linecap="round"/>`;
      svg.innerHTML = s; vEl.textContent = M.fmt(v);
    },
  };
  let y0 = 0, v0 = 0, moved = false, lastTap = 0;
  host.addEventListener('pointerdown', ev => { host.setPointerCapture(ev.pointerId); y0 = ev.clientY; v0 = k.get(); moved = false; host.classList.add('active'); });
  host.addEventListener('pointermove', ev => {
    if (!host.hasPointerCapture(ev.pointerId)) return; const dy = y0 - ev.clientY; if (Math.abs(dy) > 3) moved = true; if (!moved) return;
    let v = clamp(v0 + dy / (M.detents ? 26 : 180) * (M.max - M.min) / (M.detents ? (M.max - M.min) : 1), M.min, M.max);
    if (M.step) v = Math.round(v); k.set(v); k.draw(); onMacro(name);
  });
  host.addEventListener('pointerup', () => {
    host.classList.remove('active');
    if (!moved) { const t = now(); if (name === 'look') { k.set((k.get() + 1) % LOOKS.length); k.draw(); onMacro(name); } else if (t - lastTap < 0.35) { k.set(M.def); k.draw(); onMacro(name); } lastTap = t; }
  });
  host.addEventListener('wheel', ev => { ev.preventDefault(); let v = clamp(k.get() - Math.sign(ev.deltaY) * (M.step || 0.02), M.min, M.max); k.set(v); k.draw(); onMacro(name); }, { passive: false });
  knobs.push(k); k.draw();
}
let lookTagT = 0;
function onMacro(name) {
  if (name === 'look') { const t = $('#play-look-tag'); t.textContent = LOOKS[EMITTERS[S.selEm].look]; t.classList.add('show'); clearTimeout(lookTagT); lookTagT = setTimeout(() => t.classList.remove('show'), 1200); refreshEmitterButtons(); }
}
function setRecording(on) {
  if (on === S.recording) return;
  S.recording = on; $('#app').classList.toggle('recording', on);
  if (on) { initAudio(); S.recT0 = now(); S.recEvents = []; if (!S.playing) setPlaying(true); }
  else { const d = now() - S.recT0; S.takes = (S.takes || 3) + 1; toast(`Take ${S.takes} · ${d.toFixed(1)}s — added to Sequence`); }
}

/* ───────────── 7 · SEQUENCE — nanoloop-simple ───────────── */
const lanesEl = $('#lanes');
let stepAspect = 1.6;
function laneSlot(l) { const sc = SCENES[S.scene]; return sc && sc[l.i]; }
function buildLanes() {
  lanesEl.innerHTML = '';
  LANES.forEach(l => {
    const row = el('div', 'lane'); row.dataset.l = l.i;
    const head = el('button', 'lane-head', `<span class="sw" style="background:${SRC[l.src].sw}"></span><svg class="lov"></svg><span class="len">${l.len === 16 ? '' : l.len}</span><span class="mute-b no-g" role="button" aria-label="Mute">M</span>`);
    head.style.backgroundImage = `url(${SRC[l.src].base}${l.src}_poster.jpg)`;
    head.title = `${SRC[l.src].name} · tap to mute, hold for lane page`;
    let ht, held = false;
    head.querySelector('.mute-b').addEventListener('pointerdown', ev => { ev.stopPropagation(); l.mute = !l.mute; row.classList.toggle('muted', l.mute); });
    head.addEventListener('pointerdown', ev => { if (ev.target.closest('.mute-b')) return; held = false; ht = setTimeout(() => { held = true; laneHeadHold(l); }, 520); });
    head.addEventListener('pointerup', ev => { clearTimeout(ht); if (ev.target.closest('.mute-b')) return; if (!held) laneHeadTap(l); });
    head.addEventListener('pointerleave', () => clearTimeout(ht));
    const steps = el('div', 'steps');
    for (let i = 0; i < 16; i++) { const s = el('div', 'st', '<i class="flash"></i>'); s.dataset.i = i; if (i % 4 === 0) s.classList.add('beat'); bindStep(s, l, i); steps.appendChild(s); }
    const end = el('span', 'lane-end', '↺'); steps.appendChild(end);
    row.append(head, steps); lanesEl.appendChild(row);
    l.row = row; l.cells = $$('.st', steps); l.end = end;
  });
  // the 8th lane: an empty slot that takes a dropped video (same as the Play grid's 8th column)
  const add = el('div', 'lane add'); const ah = el('button', 'lane-head', '+'); ah.addEventListener('click', () => openAdd());
  const as = el('div', 'steps'); for (let i = 0; i < 16; i++) as.appendChild(el('div', 'st' + (i % 4 === 0 ? ' beat' : '')));
  add.append(ah, as); lanesEl.appendChild(add);
  layoutLanes();
}
function layoutLanes() {
  const wrap = $('#grid-wrap'); const h = wrap.clientHeight; const n = LANES.length + 1;
  const rowH = clamp(Math.floor((h - (n - 1) * 6) / n), 46, 120);
  lanesEl.style.setProperty('--lane-row', `${rowH}px`);
  lanesEl.style.setProperty('--lane-h', `${Math.round(rowH * 16 / 9)}px`);
  requestAnimationFrame(() => { const c = LANES[0].cells[1].getBoundingClientRect(); stepAspect = c.width / Math.max(1, c.height); renderLanes(); });
}
function renderLanes() {
  LANES.forEach(l => {
    const slot = laneSlot(l) || 'A', ghost = !laneSlot(l), pat = l.pats[slot];
    l.row.classList.toggle('muted', l.mute); l.row.style.opacity = ghost ? 0.38 : '';
    l.cells.forEach((c, i) => {
      const st = pat[i], inLen = i < l.len;
      c.classList.toggle('off-len', !inLen); c.classList.toggle('on', inLen && st.on);
      if (inLen && st.on) bgFrame(c, l.src, st.slice * 3, stepAspect); else c.style.backgroundImage = '';
      let pt = c.querySelector('.pt'); const txt = st.on && inLen && st.pitch ? `${st.pitch > 0 ? '+' : ''}${st.pitch}` : '';
      if (txt && !pt) { pt = el('span', 'pt'); c.appendChild(pt); } if (pt) pt.textContent = txt;
    });
    if (l.len < 16) { const c = l.cells[l.len]; l.end.style.display = ''; l.end.style.left = `${c.offsetLeft + 8}px`; } else l.end.style.display = 'none';
  });
}
function bindStep(c, l, i) {
  let x0, y0, ht, mode = null, p0 = 0;
  c.addEventListener('pointerdown', ev => {
    if (i >= l.len) return; initAudio();
    c.setPointerCapture(ev.pointerId); x0 = ev.clientX; y0 = ev.clientY; mode = null;
    ht = setTimeout(() => { mode = 'hold'; openHold(l, i, c); }, 420);
  });
  c.addEventListener('pointermove', ev => {
    if (!c.hasPointerCapture(ev.pointerId) || mode === 'hold') return;
    const dy = y0 - ev.clientY;
    if (!mode && Math.abs(dy) > 8) { mode = 'pitch'; clearTimeout(ht); const st = curPat(l)[i]; if (!st.on) { st.on = true; } p0 = st.pitch; }
    if (mode === 'pitch') { const st = curPat(l)[i]; st.pitch = clamp(p0 + Math.round(dy / 14), -7, 14); renderLanes(); }
  });
  c.addEventListener('pointerup', () => {
    clearTimeout(ht);
    if (!mode) { ensureSlot(l); const st = curPat(l)[i]; st.on = !st.on; renderLanes(); if (st.on) { triggerLane(l, st, true); } }
    else if (mode === 'pitch') { const st = curPat(l)[i]; triggerLane(l, st, true); }
    mode = null;
  });
  c.addEventListener('pointercancel', () => clearTimeout(ht));
}
function ensureSlot(l) { const sc = SCENES[S.scene]; if (sc && !sc[l.i]) { sc[l.i] = 'A'; renderSong(); } }
const curPat = l => l.pats[laneSlot(l) || 'A'];
function triggerLane(l, st, preview = false) {
  const t = now(), dur = l.mode === 'frame' ? stepDur() * Math.max(3, st.gate * 2.5) : stepDur() * st.gate * 1.4 + 0.05;
  const f = st.slice * 3;
  if (l.mode === 'cloud') {
    for (let k = 0; k < 14; k++) { const w = rnd(0.06, 0.2), cx = rnd(0.1, 0.9), cy = rnd(0.1, 0.9); spawnGrain({ src: l.src, f: f + rnd(-1, 1), x: cx, y: cy, w, h: w * rnd(0.8, 1.2), u: cx - w / 2, v: cy - w / 2, dur: rnd(0.12, 0.35), pitch: st.pitch, a: 1, lane: l.i, look: l.look, cut: l.cut === 'cut' }); }
  } else {
    const by = rnd(0, 0.6), bh = rnd(0.15, 0.4);
    world.hits.push({ src: l.src, f, t0: t, gate: dur, dur: dur + l.r, o: l, mode: l.mode, by, bh, pitch: st.pitch, lane: l.i, blend: l.i === 4 ? 'lighten' : null });
  }
  audioGrain(l.src, st.slice / 16, Math.min(dur, 0.6), st.pitch, (l.i - 2.5) / 3, (l.mode === 'cloud' ? 0.3 : 0.5) * l.level, l.smear, [0, 4, 2, 7, -7, 9, 5][l.i]);
  if (l.mode === 'cloud') for (let k = 0; k < 3; k++) setTimeout(() => audioGrain(l.src, st.slice / 16 + k * 0.01, 0.08, st.pitch + k * 2, rnd(-0.8, 0.8), 0.18 * l.level, 0.4, [0, 4, 2, 7, -7, 9, 5][l.i]), k * 40);
  if (!preview) { l.flashAt[l.pos] = t; }
}
/* hold popover: slice + 3 params */
const pop = $('#hold-pop');
let held = null;
function openHold(l, i, c) {
  ensureSlot(l); const st = curPat(l)[i]; if (!st.on) { st.on = true; renderLanes(); }
  held = { l, i, c }; $$('.st.held').forEach(e => e.classList.remove('held')); c.classList.add('held');
  const chop = $('#hp-chop'); chop.innerHTML = '';
  for (let k = 0; k < 16; k++) { const ch = el('div', 'ch'); bgFrame(ch, l.src, k * 3, 1.6); ch.classList.toggle('sel', k === st.slice); ch.addEventListener('pointerdown', ev => { ev.stopPropagation(); st.slice = k; $$('.ch', chop).forEach((e, j) => e.classList.toggle('sel', j === k)); renderLanes(); triggerLane(l, st, true); }); chop.appendChild(ch); }
  refreshHold();
  const wr = $('#view-seq').getBoundingClientRect(), cr = c.getBoundingClientRect();
  pop.classList.add('show');
  const pw = pop.offsetWidth, ph = pop.offsetHeight;
  let left = clamp(cr.left - wr.left + cr.width / 2 - pw / 2, 12, wr.width - pw - 12);
  let top = cr.top - wr.top - ph - 10; if (top < 8) top = cr.bottom - wr.top + 10;
  pop.style.left = `${left}px`; pop.style.top = `${top}px`;
}
function refreshHold() {
  if (!held) return; const st = curPat(held.l)[held.i];
  $('#hp-pitch').textContent = `${st.pitch > 0 ? '+' : ''}${st.pitch}`;
  $('#hp-gate').textContent = ['', '1/16', '1/8', '3/16', '1/4', '', '3/8', '', '1/2'][st.gate] || st.gate;
  $('#hp-chance').textContent = `${st.chance}%`;
}
function closeHold() { pop.classList.remove('show'); $$('.st.held').forEach(e => e.classList.remove('held')); held = null; }
$$('.hp-p').forEach(p => {
  let y0, v0, moved;
  const k = p.dataset.p;
  p.addEventListener('pointerdown', ev => { ev.stopPropagation(); p.setPointerCapture(ev.pointerId); y0 = ev.clientY; v0 = curPat(held.l)[held.i][k]; moved = false; });
  p.addEventListener('pointermove', ev => {
    if (!p.hasPointerCapture(ev.pointerId)) return; const dy = y0 - ev.clientY; if (Math.abs(dy) > 4) moved = true; if (!moved) return; const st = curPat(held.l)[held.i];
    if (k === 'pitch') st.pitch = clamp(v0 + Math.round(dy / 12), -7, 14);
    if (k === 'gate') st.gate = [1, 2, 3, 4, 6, 8][clamp([1, 2, 3, 4, 6, 8].indexOf(v0) + Math.round(dy / 20), 0, 5)];
    if (k === 'chance') st.chance = clamp(Math.round((v0 + dy / 2) / 5) * 5, 5, 100);
    refreshHold(); renderLanes();
  });
  p.addEventListener('pointerup', () => {
    if (moved) return; const st = curPat(held.l)[held.i];
    if (k === 'pitch') st.pitch = st.pitch >= 7 ? -7 : st.pitch + 1;
    if (k === 'gate') st.gate = [1, 2, 3, 4, 6, 8][([1, 2, 3, 4, 6, 8].indexOf(st.gate) + 1) % 6];
    if (k === 'chance') st.chance = st.chance <= 25 ? 100 : st.chance - 25;
    refreshHold(); renderLanes();
  });
});
document.addEventListener('pointerdown', ev => { if (held && !pop.contains(ev.target) && !ev.target.closest('.st')) closeHold(); });

/* scenes + song chain */
function renderSong() {
  const sEl = $('#scenes'); sEl.innerHTML = '';
  SCENES.forEach((sc, k) => {
    const bars = sc ? `<span class="bars">${sc.map(x => `<i class="${x === 'B' ? 'b' : x ? '' : 'x'}"></i>`).join('')}</span>` : '';
    const b = el('button', 'scene' + (sc ? '' : ' empty') + (k === S.scene ? ' cur' : '') + (k === S.queuedScene ? ' queued' : ''), `${k + 1}${bars}`);
    b.id = `scene-${k + 1}`;
    b.addEventListener('click', () => { if (!sc) { SCENES[k] = SCENES[S.scene].slice(); renderSong(); toast(`Scene ${k + 1} · copied from ${S.scene + 1}`); return; } if (S.playing) { S.queuedScene = k; } else { S.scene = k; } renderSong(); renderLanes(); });
    sEl.appendChild(b);
  });
  const cEl = $('#chain'); cEl.innerHTML = '';
  CHAIN.forEach((c, k) => {
    const d = el('div', 'link' + (S.playing && k === S.chainPos ? ' cur' : ''), `<i class="fill"></i><span>${c.s + 1}</span><span style="opacity:.5">×${c.r}</span>`);
    d.style.width = `${30 + c.r * 12}px`; cEl.appendChild(d); c.el = d;
  });
  cEl.appendChild(el('span', 'loopmark', '↺'));
}

/* ───────────── 8 · PERFORM — pads + output ───────────── */
const padsEl = $('#pads');
const MODE_GLYPH = { loop: '∞', grain: '⁘', chop: '⫶', slice: '▮' };
function padName(p) { return p.mode === 'slice' ? `${SRC[p.src].name} ${p.sliceN + 1}` : SRC[p.src].name; }
function buildPads() {
  padsEl.innerHTML = '';
  PADS.forEach((p, k) => {
    const d = el('button', 'pad'); d.id = `pad-${k + 1}`;
    if (!p) { d.classList.add('empty'); d.innerHTML = '<span class="pl">+</span>'; d.addEventListener('click', () => openAdd()); padsEl.appendChild(d); return; }
    const look = LOOKS[p.look];
    d.innerHTML = `<i class="img"></i><svg class="ov"></svg><span class="pl"><span class="nm">${padName(p)}</span><span class="m">${MODE_GLYPH[p.mode]}</span></span><i class="prog"></i><span class="edit" aria-label="Open clip">›</span>`;
    d.title = `${padName(p)} · ${p.mode}${look !== 'clean' ? ' · ' + look : ''}`;
    attachPad(d, p);
    p.el = d; p.img = d.querySelector('.img'); p.prog = d.querySelector('.prog'); p.ov = d.querySelector('.ov');
    padsEl.appendChild(d);
  });
  requestAnimationFrame(() => { PADS.forEach(p => p && bgFrame(p.img, p.src, padFrame(p), p.el.clientWidth / Math.max(1, p.el.clientHeight))); drawPadOverlays(); });
  renderPads();
  const hp = $('#hud-pads'); hp.innerHTML = '';
  PADS.forEach(p => { const b = el('b'); if (p) { b.style.backgroundImage = atlasURL(p.src); const [c, r] = frameXY(padFrame(p)); b.style.backgroundSize = '800% 600%'; b.style.backgroundPosition = `${c / 7 * 100}% ${r / 5 * 100}%`; b.addEventListener('pointerdown', ev => { ev.stopPropagation(); tapPad(p); }); p.hud = b; } hp.appendChild(b); });
}
const padFrame = p => (p.mode === 'slice' ? (p.slice[0] * 47) | 0 : p.frame);
function setPad(p, on) {
  if (on) { p.on = true; p.t0 = now(); p.offT = null; launchFlash(p); }
  else if (p.on) { p.on = false; p.offT = now(); }
}
function tapPad(p) {
  initAudio(); A.ctx?.resume?.();
  if (p.mode === 'slice') { // one-shot, plays instantly (Koala)
    setPad(p, true); p.gate = Math.max(0.08, (p.slice[1] - p.slice[0]) * ATLAS.dur);
    audioGrain(p.src, p.slice[0], Math.min(p.gate, 1.4), p.pitch || 0, slotPanFor(p), 0.55 * p.level, p.smear);
    clearTimeout(p._ot); p._ot = setTimeout(() => { setPad(p, false); renderPads(); }, p.gate * 1000);
    renderPads(); return;
  }
  if (!S.playing) { setPlaying(true); setPad(p, !p.on); limitPads(); renderPads(); broadcast(); return; }
  if (S.quant === 0) { setPad(p, !p.on); limitPads(); } else p.q = !p.q;
  renderPads(); broadcast();
}
function limitPads() { const on = PADS.filter(p => p && p.on && p.mode !== 'slice').sort((a, b) => b.t0 - a.t0); on.slice(4).forEach(p => setPad(p, false)); }
function renderPads() {
  PADS.forEach(p => { if (!p) return; p.el.classList.toggle('on', p.on); p.el.classList.toggle('q', p.q); p.hud?.classList.toggle('on', p.on); });
  const names = PADS.filter(p => p && p.on).map(padName); $('#hud-now').textContent = names.join(' · ');
}
function launchFlash(p) { if (S.layout === 'tile' || p.mode === 'slice') return; world.hits.push({ src: p.src, f: rnd(40) | 0, t0: now(), dur: 0.3, mode: 'frame', look: p.look, perf: true }); }
const padVisible = (p, t) => p.on || (p.offT && t - p.offT < p.r + 0.02);
const padEnv = (p, t) => envAt(p, p.t0, p.on ? (p.mode === 'slice' ? p.t0 + p.gate : null) : p.offT, t);
const slotPanFor = p => (S.layout === 'tile' ? slotPan(stages.perf, 'p' + p.i) * p.tilePan : p.pan);
function padStep(p, g) {
  const id = p.src, e = padEnv(p, now()), gain = e * p.level;
  if (p.mode === 'loop' && g % 4 === 0) audioGrain(id, (media[id].v.currentTime || 0) / 6, stepDur() * 3.5, p.pitch, slotPanFor(p), 0.35 * gain, p.smear, [0, 2, 4, 7][(g / 4) % 4 | 0]);
  if (p.mode === 'grain') for (let k = 0; k < 2; k++) audioGrain(id, rnd(0.2, 0.6), 0.09, p.pitch + [0, 2, 4, 7, 9][rnd(5) | 0], slotPanFor(p) + rnd(-0.4, 0.4), 0.16 * gain, p.smear + 0.3, 7);
  if (p.mode === 'chop' && g % 2 === 0) { const sl = rnd(16) | 0; p.chopF = sl * 3; p.chopT = now(); audioGrain(id, sl / 16, stepDur() * 1.6, p.pitch + [0, 3, 5, 7][rnd(4) | 0], slotPanFor(p), 0.4 * gain, p.smear, 0); }
}
function drawPadInto(st, p, t, R, k, stack) {
  const m = media[p.src]; const a = vis(p, padEnv(p, t)); if (a <= 0) return;
  const x = st.x;
  x.save(); x.beginPath(); x.rect(R.x, R.y, R.w, R.h); x.clip();
  if (stack && k > 0 && p.mode === 'loop') x.globalCompositeOperation = 'lighten';
  if (p.mode === 'loop') { const d = srcDims(m); if (d) { const [sx, sy, sw, sh] = cover(d[1], d[2], R.w, R.h); drawLook(x, d[0], sx, sy, sw, sh, R.x, R.y, R.w, R.h, p.look, a * (stack && k > 0 ? 0.85 : 1)); } }
  else if (p.mode === 'chop' || p.mode === 'slice') {
    let f0, age, ce = 1;
    if (p.mode === 'chop') { if (p.chopT != null) { age = t - p.chopT; f0 = p.chopF; ce = vis(p, envAt(p, p.chopT, p.chopT + stepDur() * 1.5, t)); } else ce = 0; }
    else { age = t - p.t0; f0 = p.slice[0] * 47; }
    if (ce > 0 && m.img.complete) { const [c, r] = frameXY(f0 + age * ATLAS.fps); const [sx, sy, sw, sh] = cover(ATLAS.fw, ATLAS.fh, R.w, R.h); drawLook(x, m.img, c * ATLAS.fw + sx, r * ATLAS.fh + sy, sw, sh, R.x, R.y, R.w, R.h, p.look, a * ce * (stack && k > 0 ? 0.9 : 1)); }
  }
  x.restore();
  if (p.mode === 'grain') drawGrains(st, t, g => g.pad === p.i, R);
}
function drawPerform(st, t) {
  const vp = PADS.filter(p => p && padVisible(p, t)).sort((a, b) => a.t0 - b.t0);
  if (S.layout === 'tile') {
    blackout(st); const rects = tileRects(st, vp.map(p => 'p' + p.i));
    vp.forEach(p => { const R = rects['p' + p.i]; if (R) drawPadInto(st, p, t, R, 0, false); });
  } else {
    const x = st.x; x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1; x.fillStyle = 'rgba(0,0,0,0.32)'; x.fillRect(0, 0, st.w, st.h);
    vp.forEach((p, k) => drawPadInto(st, p, t, full(st), k, true));
    drawHits(st, t, h => h.perf || h.aud);
  }
}
function spawnPadGrains(t) {
  PADS.forEach(p => {
    if (!p || p.mode !== 'grain' || !padVisible(p, t)) return; const a = vis(p, padEnv(p, t)); if (a <= 0) return;
    p.acc = (p.acc || 0) + lerp(0.3, 1.6, p.grain);
    while (p.acc >= 1) { p.acc -= 1; const w = lerp(0.3, 0.05, p.grain) * rnd(0.7, 1.3), cx = rnd(0.05, 0.95), cy = rnd(0.05, 0.95); spawnGrain({ src: p.src, f: rnd(48), x: cx, y: cy, w, h: w * rnd(0.6, 1.3), u: cx - w / 2, v: cy - w / 2, dur: rnd(0.2, 0.6), look: p.look, pad: p.i, a, cut: p.cut === 'cut' }); }
  });
}

/* ───────────── frame loop ───────────── */
const stages = {};
let lastT = now();
const emActive = (e, t) => e.on || (e.relT != null && t - e.relT < e.r);
function frame() {
  const t = now(), dt = Math.min(0.05, t - lastT); lastT = t;
  EMITTERS.forEach(e => emActive(e, t) && emitFrom(e, dt));
  spawnPadGrains(t);
  // PLAY canvas
  const sp = stages.play;
  if (sp && sp.visible()) {
    sp.resize(); const x = sp.x;
    const actives = EMITTERS.filter(e => emActive(e, t) && vis(e, envAt(e, e.envT0, e.relT, t)) > 0);
    if (S.layout === 'tile' && actives.length) {
      blackout(sp); const rects = tileRects(sp, actives.map(e => 'e' + e.i));
      actives.forEach(e => {
        const R = rects['e' + e.i]; const id = GRID_COLS[e.lastCol]; const d = id && srcDims(media[id]);
        const a = vis(e, envAt(e, e.envT0, e.relT, t));
        if (d) { const [sx, sy, sw, sh] = cover(d[1], d[2], R.w, R.h); drawLook(x, d[0], sx, sy, sw, sh, R.x, R.y, R.w, R.h, e.look, a * 0.55); }
        drawGrains(sp, t, g => g.em === e.i, R);
      });
    } else {
      const active = actives[0] || EMITTERS[S.selEm];
      const anyOn = actives.length > 0;
      const smear = Math.max(...actives.map(e => e.smear), anyOn ? 0 : active.smear);
      sp.baseA = lerp(sp.baseA, anyOn ? lerp(0.5, 0.12, smear) : lerp(0.9, 0.35, smear), 0.08);
      x.globalCompositeOperation = 'source-over';
      const id = GRID_COLS[active.lastCol]; const d = id && srcDims(media[id]);
      if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, sp.w, sp.h, active.look, sp.baseA);
      else { x.fillStyle = `rgba(0,0,0,${sp.baseA})`; x.fillRect(0, 0, sp.w, sp.h); }
      drawGrains(sp, t, g => g.em != null);
      drawGrains(sp, t, g => g.lane != null);
      drawHits(sp, t, h => !h.perf);
    }
  }
  // SEQUENCE preview
  const ss = stages.seq;
  if (ss && ss.visible()) {
    ss.resize(); const x = ss.x;
    if (S.layout === 'tile') {
      const live = LANES.filter(l => world.hits.some(h => h.lane === l.i && hitAlpha(h, t) > 0) || world.grains.some(g => g.lane === l.i && t - g.t0 < g.dur));
      blackout(ss); const rects = tileRects(ss, live.map(l => 'l' + l.i));
      live.forEach(l => { const R = rects['l' + l.i]; drawHits(ss, t, h => h.lane === l.i, R); drawGrains(ss, t, g => g.lane === l.i, R); });
    } else {
      x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1; x.fillStyle = 'rgba(0,0,0,0.12)'; x.fillRect(0, 0, ss.w, ss.h);
      if (!S.playing) { const d = srcDims(media[LANES[0].src]); if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, ss.w, ss.h, 0, 0.08); }
      drawHits(ss, t, h => h.lane != null || h.aud); drawGrains(ss, t, g => g.lane != null);
    }
  }
  // PERFORM output (also drives Display 2 / fullscreen)
  const pf = stages.perf;
  if (pf && (pf.visible() || S.out === 'd2')) { pf.resize(); drawPerform(pf, t); }
  // CLIP page picture (direction B)
  const sc = stages.clip;
  if (sc && sc.visible()) { sc.resize(); drawClipStage(sc, t); }

  if (S.view === 'play') for (let k = 0; k < 64; k++) { const lit = t - litAt[k] < 0.12; if (cells[k] && cells[k].classList.contains('lit') !== lit) cells[k].classList.toggle('lit', lit); }
  if (S.view === 'seq') LANES.forEach(l => l.cells.forEach((c, i) => {
    const hit = t - l.flashAt[i] < 0.09; if (c.classList.contains('hit') !== hit) c.classList.toggle('hit', hit);
    const ph = S.playing && i === l.pos; if (c.classList.contains('ph') !== ph) c.classList.toggle('ph', ph);
  }));
  if (S.view === 'seq' && S.playing) { const c = CHAIN[S.chainPos]; if (c?.el) { const frac = ((S.chainRep || 0) + ((S.step % 16) + 1) / 16) / c.r; c.el.querySelector('.fill').style.width = `${frac * 100}%`; } }
  if (S.view === 'perform' || document.fullscreenElement) {
    const fi = Math.floor(t * 8);
    PADS.forEach(p => {
      if (!p) return;
      if (p.on && p._fi !== fi) { p._fi = fi; p.frame = (p.frame + 1) % 48; const f = p.mode === 'chop' && p.chopF != null ? p.chopF : p.mode === 'slice' ? padFrame(p) + ((t - p.t0) * 8 | 0) : p.frame; bgFrame(p.img, p.src, f, p.el.clientWidth / Math.max(1, p.el.clientHeight)); }
      p.prog.style.width = p.on && S.playing && p.mode !== 'slice' ? `${(((S.step % 16) + 1) / 16) * 100}%` : p.on && p.mode === 'slice' ? `${clamp((t - p.t0) / p.gate, 0, 1) * 100}%` : '0';
    });
  }
  if (S.playing && S.step >= 0) {
    const bar = Math.floor(S.step / 16) + 1, beat = Math.floor((S.step % 16) / 4) + 1, six = (S.step % 4) + 1;
    $('#readout-pos .bar').textContent = String(bar).padStart(3, '0'); $('#readout-pos .beat').textContent = beat; $('#readout-pos .six').textContent = six;
    $('#hud-time').textContent = `${String(bar).padStart(3, '0')}.${beat}`;
    $$('#beat-dots i, #hud-beats i').forEach((d, k) => d.classList.toggle('on', (k % 4) === beat - 1));
  }
  if (S.recording) { const d = t - S.recT0; $('#rec-time').textContent = `${Math.floor(d / 60)}:${String(Math.floor(d % 60)).padStart(2, '0')}`; }
  dirFrame(t);
  prune(t);
  requestAnimationFrame(frame);
}

/* ───────────── 10 · chrome: views, transport, tempo, key, keys ───────────── */
function setView(v) {
  S.view = v; $('#app').dataset.view = v; closeHold(); closeClip(); hideMini(); closeSheets();
  if (S.dir === 'A') { if (v === 'seq' && S.sel.kind !== 'lane' && S.sel.kind !== 'project') S.sel = { kind: 'lane', ref: LANES[0] }; if (v === 'perform' && S.sel.kind !== 'pad' && S.sel.kind !== 'project') S.sel = { kind: 'pad', ref: PADS.find(p => p && p.on) || PADS.find(p => p) }; if (v === 'play' && !['emitter', 'clip', 'project'].includes(S.sel.kind)) S.sel = { kind: 'emitter', ref: EMITTERS[S.selEm] }; }
  $$('.views button').forEach(b => b.setAttribute('aria-selected', b.dataset.view === v));
  const b = $(`.views button[data-view="${v}"]`); const th = $('.views-thumb');
  th.style.width = `${b.offsetWidth}px`; th.style.transform = `translateX(${b.offsetLeft - 3}px)`;
  requestAnimationFrame(() => { if (v === 'play') layoutGrid(); if (v === 'seq') { renderSong(); layoutLanes(); } if (v === 'perform') PADS.forEach(p => p && bgFrame(p.img, p.src, padFrame(p), p.el.clientWidth / Math.max(1, p.el.clientHeight))); markSelection(); renderStrip(); drawPadOverlays(); drawLaneOverlays(); drawEmCards(); refreshEmCards(); liveDraw(); });
  const u = new URL(location.href); u.hash = v; history.replaceState(null, '', u);
}
$$('.views button').forEach(b => b.addEventListener('click', () => setView(b.dataset.view)));
$('#btn-play').addEventListener('click', () => setPlaying(!S.playing));
$('#btn-rec').addEventListener('click', () => setRecording(!S.recording));
$('#btn-rec-big').addEventListener('click', () => setRecording(!S.recording));
$('#btn-export').addEventListener('click', () => toast('Export · renders mp4 + wav offline, sample-identical to playback'));
(() => { // tempo: drag; tap = the project's time & key (A: MAIN page · B: project page · C: pop)
  const c = $('#chip-tempo'); let y0, b0, moved = false;
  c.addEventListener('pointerdown', ev => { c.setPointerCapture(ev.pointerId); y0 = ev.clientY; b0 = S.bpm; moved = false; });
  c.addEventListener('pointerup', ev => { if (moved) return; if (S.dir === 'C') topPop('tempo', c); else if (S.dir === 'B') openProject(); else { S.page.project = 'MAIN'; select({ kind: 'project', ref: PROJ }); } });
  c.addEventListener('pointermove', ev => { if (!c.hasPointerCapture(ev.pointerId)) return; if (Math.abs(y0 - ev.clientY) > 4) moved = true; if (!moved) return; S.bpm = clamp(Math.round(b0 + (y0 - ev.clientY) / 3), 60, 200); $('#tempo-val').textContent = S.bpm; $('#hud-bpm').textContent = S.bpm; if (A.dly) A.dly.delayTime.value = (60 / S.bpm) * 0.75; });
})();
function cycleKey() { S.keyIdx = (S.keyIdx + 1) % KEYS.length; $('#key-root').textContent = key().root; $('#key-scale').textContent = key().scale; knobs.forEach(k => k.draw()); liveDraw(); }
$('#chip-key').addEventListener('click', ev => { if (S.dir === 'C' && ev.isTrusted) topPop('key', $('#chip-key')); else cycleKey(); });
$('#btn-deep').addEventListener('click', ev => projectAction(ev.currentTarget));
$$('#dirsw button').forEach(b => b.addEventListener('click', () => setDir(b.dataset.dir)));
$$('#seg-quant button').forEach(b => b.addEventListener('click', () => { S.quant = +b.dataset.q; $$('#seg-quant button').forEach(x => x.classList.toggle('on', x === b)); }));
$('#btn-stopall').addEventListener('click', () => { PADS.forEach(p => p && (setPad(p, false), (p.q = false))); renderPads(); broadcast(); });

/* fullscreen: the output, nothing else. HUD on tap; cursor hides when idle. */
const perfStage = $('#perf-stage');
let hudT = 0, curT = 0;
function toggleFS() {
  if (document.fullscreenElement) { document.exitFullscreen(); return; }
  if (S.view !== 'perform') setView('perform');
  (perfStage.requestFullscreen || perfStage.webkitRequestFullscreen)?.call(perfStage).catch?.(() => toast('Fullscreen not allowed here'));
}
function pokeHUD() { perfStage.classList.add('hud-on'); perfStage.classList.remove('nocursor'); clearTimeout(hudT); clearTimeout(curT); hudT = setTimeout(() => perfStage.classList.remove('hud-on'), 2600); curT = setTimeout(() => perfStage.classList.add('nocursor'), 1500); }
document.addEventListener('fullscreenchange', () => { if (document.fullscreenElement) { perfStage.classList.add('nocursor'); perfStage.classList.remove('hud-on'); } else perfStage.classList.remove('hud-on', 'nocursor'); });
perfStage.addEventListener('pointermove', () => document.fullscreenElement && pokeHUD());
perfStage.addEventListener('pointerdown', ev => { if (document.fullscreenElement && !ev.target.closest('.hud-pads,.hud-x')) { perfStage.classList.contains('hud-on') ? perfStage.classList.remove('hud-on') : pokeHUD(); } });
$('#hud-exit').addEventListener('click', () => document.exitFullscreen());
$('#btn-fs').addEventListener('click', toggleFS);

document.addEventListener('keydown', ev => {
  if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
  const k = ev.key.toLowerCase();
  if (k === ' ') { ev.preventDefault(); setPlaying(!S.playing); }
  else if (k === 'tab') { ev.preventDefault(); const vs = ['play', 'seq', 'perform']; setView(vs[(vs.indexOf(S.view) + (ev.shiftKey ? 2 : 1)) % 3]); }
  else if (k === '1') setView('play'); else if (k === '2') setView('seq'); else if (k === '3') setView('perform');
  else if (k === 'f') toggleFS();
  else if (k === 'r') setRecording(!S.recording);
  else if (k === 'h' && document.fullscreenElement) pokeHUD();
  else if (k === 'escape') { closeHold(); closeSheets(); closeClip(); hideMini(); }
  else if ((k === 'a' || k === 'b' || k === 'c') && !ev.target.closest('input, [contenteditable]')) setDir(k.toUpperCase());
});
window.addEventListener('resize', () => { if (S.view === 'play') layoutGrid(); if (S.view === 'seq') layoutLanes(); hideMini(); drawPadOverlays(); drawLaneOverlays(); drawEmCards(); });

/* ───────────── 11 · Display 2 — output window driven by this one ───────────── */
let bc = null; try { bc = new BroadcastChannel('chromagrain-out'); } catch (e) {}
let outWin = null;
function broadcast(extra) {
  if (!bc || OUT_MODE) return;
  try { bc.postMessage(extra || { t: 'state', pads: PADS.map(p => p && p.on), bpm: S.bpm, playing: S.playing }); } catch (e) {}
}
$$('#seg-out button').forEach(b => b.addEventListener('click', () => {
  S.out = b.dataset.out; $$('#seg-out button').forEach(x => x.classList.toggle('on', x === b));
  $('#view-perform').classList.toggle('d2', S.out === 'd2');
  if (S.out === 'd2') {
    try { outWin = window.open(location.href.split('#')[0].split('?')[0] + '?out=1', 'chromagrain-output', 'popup,width=960,height=540'); } catch (e) {}
    if (!outWin) toast('Output window blocked — allow pop-ups, then drag it to the second display');
    else { toast('Output window open — drag it to Display 2, press F there'); setTimeout(broadcast, 800); }
  } else if (outWin) { try { outWin.close(); } catch (e) {} outWin = null; }
}));

/* ═════════════ 13 · v002 — parameters, exposed three ways ═════════════
   A · Pages      context-sensitive parameter pages for the selection (Elektron / Tempera)
   B · Clip page  one calm full-screen editor per clip (Koala sample-edit, but AV)
   C · On-object  parameters drawn on, and gestured on, the things themselves
   Shared: ADSR (sound + picture cut/fade), Chop (tap/auto/equal → pads), stack|tile, Add sheet (Files/Browse/Link/Dig) */
const pct = v => `${Math.round(v * 100)}%`;
const sec = v => (v < 1 ? `${Math.round(v * 1000)}ms` : `${v.toFixed(2)}s`);
const PDEF = {
  grain: MACROS.grain, pitch: MACROS.pitch, smear: MACROS.smear, look: MACROS.look,
  level: { label: 'Level', min: 0, max: 1, def: 0.8, fmt: pct },
  pan: { label: 'Pan', min: -1, max: 1, def: 0, bipolar: true, fmt: v => (Math.abs(v) < 0.02 ? 'C' : (v < 0 ? 'L' : 'R') + Math.round(Math.abs(v) * 100)) },
  density: { label: 'Density', min: 1, max: 120, def: 30, fmt: v => `${Math.round(v)}/s` },
  length: { label: 'Length', min: 10, max: 1000, def: 80, fmt: v => `${Math.round(v)}ms` },
  spray: { label: 'Spray', min: 0, max: 1, def: 0.3, fmt: pct },
  jitter: { label: 'Jitter', min: 0, max: 1, def: 0.1, fmt: pct },
  scan: { label: 'Scan', min: -2, max: 2, def: 1, bipolar: true, fmt: v => `${v.toFixed(2)}×` },
  reverse: { label: 'Reverse', min: 0, max: 1, def: 0, fmt: pct },
  spread: { label: 'Spread', min: 0, max: 1, def: 0.6, fmt: pct },
  seed: { label: 'Seed', min: 0, max: 999, def: 17, step: 1, fmt: v => String(v) },
  a: { label: 'Attack', min: 0, max: 2, def: 0.005, fmt: sec }, d: { label: 'Decay', min: 0, max: 2, def: 0.25, fmt: sec },
  s: { label: 'Sustain', min: 0, max: 1, def: 0.75, fmt: pct }, r: { label: 'Release', min: 0, max: 3, def: 0.12, fmt: sec },
  thresh: { label: 'Cut at', min: 0.01, max: 0.5, def: 0.08, fmt: pct },
  fine: { label: 'Fine', min: -50, max: 50, def: 0, step: 1, bipolar: true, fmt: v => `${v > 0 ? '+' : ''}${v}ct` },
  pjit: { label: 'Wobble', min: 0, max: 12, def: 0, fmt: v => `${v.toFixed(1)}st` },
  glide: { label: 'Glide', min: 0, max: 1, def: 0, fmt: sec },
  octave: { label: 'Octave', min: -3, max: 3, def: 0, step: 1, bipolar: true, fmt: v => `${v > 0 ? '+' : ''}${v}` },
  tilePan: { label: 'Tile→pan', min: 0, max: 1, def: 1, fmt: pct },
  len: { label: 'Steps', min: 1, max: 16, def: 16, step: 1, fmt: v => String(v) },
  swing: { label: 'Swing', min: 0, max: 0.5, def: 0.08, fmt: pct },
  chance: { label: 'Chance', min: 0, max: 1, def: 1, fmt: pct },
};
const TEMPO_DEF = { label: 'Tempo', min: 60, max: 200, def: 112, step: 1, fmt: v => `${v}` };
const FXN = ['crush', 'delay', 'reverb', 'compress', 'tint', 'stretch', 'lowpass', 'highpass'];
const FXL = { crush: 'Crush', delay: 'Echo', reverb: 'Reverb', compress: 'Codec', tint: 'Tint', stretch: 'Stretch', lowpass: 'Low-pass', highpass: 'High-pass' };
const LINKN = [['pitch', 'hue'], ['gain', 'opacity'], ['pan', 'x'], ['position', 'frame'], ['reverse', 'reverse'], ['envelope', 'fade'], ['layout', 'pan']];
const BASE_P = { level: 0.8, pan: 0, grain: 0.5, pitch: 0, smear: 0.2, look: 0, density: 30, length: 80, spray: 0.3, jitter: 0.1, scan: 1, reverse: 0, spread: 0.6, seed: 17, a: 0.005, d: 0.25, s: 0.75, r: 0.12, cut: 'cut', thresh: 0.08, fine: 0, pjit: 0, glide: 0, octave: 0, snap: true, swing: 0.08, chance: 1, tilePan: 1 };
function ensureP(o, over = {}) {
  for (const k in BASE_P) if (o[k] === undefined) o[k] = k in over ? over[k] : BASE_P[k];
  if (!o.fx) o.fx = Object.fromEntries(FXN.map(f => [f, { a: over.fx?.[f]?.[0] ?? 0, v: over.fx?.[f]?.[1] ?? 0 }]));
  if (!o.links) o.links = Object.fromEntries(LINKN.map(([a]) => [a, a === 'envelope' ? 0.8 : 1]));
  return o;
}
EMITTERS.forEach((e, i) => ensureP(e, { a: 0.02, d: 0.3, s: 0.8, r: [0.35, 0.9, 0.2, 0.05][i], cut: i === 1 ? 'fade' : 'cut', fx: { delay: [0.3, 0.25], reverb: [0.35, 0.4], crush: i === 3 ? [0.5, 0.7] : [0, 0] } }));
PADS.forEach(p => p && ensureP(p, { r: 0.08, grain: 0.6 }));
LANES.forEach(l => ensureP(l, { a: 0.002, d: 0.12, s: 0.6, r: l.src === 'aurora' ? 0.8 : 0.05, cut: l.src === 'aurora' ? 'fade' : 'cut' }));
const PROJ = ensureP({ kind: 'project' }, { fx: { reverb: [0.2, 0.15] } });
const LIVE = new Set(), TICKERS = new Set();
const liveDraw = () => LIVE.forEach(w => (w.el.isConnected ? w.draw() : LIVE.delete(w)));
const posterURL = id => `${SRC[id].base}${id}_poster.jpg`;
function setBpm(v) { S.bpm = v; $('#tempo-val').textContent = v; $('#hud-bpm').textContent = v; if (A.dly) A.dly.delayTime.value = (60 / S.bpm) * 0.75; }

/* ───── selection: one thing is always selected; every direction edits the selection ───── */
S.sel = { kind: 'emitter', ref: EMITTERS[0] }; S.page = {}; S.dir = 'A'; S.layout = 'stack';
function select(sel) {
  S.sel = sel;
  if (sel.kind === 'emitter') { S.selEm = sel.ref.i; refreshEmitterButtons(); placeEmitters(); knobs.forEach(k => k.draw()); }
  markSelection(); dirOnSelect();
}
function selSrc(sel = S.sel) { const r = sel.ref; return sel.kind === 'clip' ? r.id : sel.kind === 'emitter' ? GRID_COLS[r.col] : sel.kind === 'pad' || sel.kind === 'lane' ? r.src : null; }
function selInfo(sel = S.sel) {
  const r = sel.ref;
  if (sel.kind === 'emitter') return { t: `Emitter ${r.i + 1}`, s: SRC[GRID_COLS[r.col]]?.name || '', src: GRID_COLS[r.col], f: ((r.y * 8) | 0) * 6 + 2 };
  if (sel.kind === 'pad') return { t: `Pad ${r.i + 1}`, s: `${padName(r)} · ${r.mode}`, src: r.src, f: padFrame(r) };
  if (sel.kind === 'lane') return { t: `Lane ${r.i + 1}`, s: SRC[r.src].name, src: r.src, f: 12 };
  if (sel.kind === 'clip') return { t: r.name, s: r.prov ? r.prov.title : 'clip defaults', src: r.id, f: 12 };
  return { t: 'Project', s: `${S.bpm} bpm · ${key().root} ${key().scale}`, src: null, f: 0 };
}
function markSelection() {
  PADS.forEach(p => p && p.el && p.el.classList.toggle('picked', S.sel.kind === 'pad' && S.sel.ref === p));
  LANES.forEach(l => l.row && l.row.classList.toggle('picked', S.sel.kind === 'lane' && S.sel.ref === l));
  $$('#vheads .vh').forEach((b, c) => b.classList.toggle('picked', S.sel.kind === 'clip' && GRID_COLS[c] === S.sel.ref.id));
}
function onParam(o, k) {
  if (k === 'len' && o.cells) renderLanes();
  if (k === 'look' && o.btn) refreshEmitterButtons();
  if (S.sel.kind === 'emitter' && o === S.sel.ref) knobs.forEach(kk => kk.draw());
  if (S.dir === 'C') { drawPadOverlays(); drawLaneOverlays(); drawEmCards(); placeEmitters(); }
  liveDraw();
}

/* ───── generic knob (fixed-position page knobs, card knobs) ───── */
let kuid = 0;
function knobSVG(M, v, dual, uid) {
  const A0 = -0.75 * Math.PI, A1 = 0.75 * Math.PI;
  let s = `<defs><linearGradient id="kg${uid}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2c2c30"/><stop offset="1" stop-color="#18181a"/></linearGradient></defs>`;
  s += `<path d="${arcPath(50, 50, 44, A0, A1)}" stroke="rgba(255,255,255,.07)" stroke-width="5" fill="none" stroke-linecap="round"/>`;
  if (dual) {
    s += `<path d="${arcPath(50, 50, 36, A0, A1)}" stroke="rgba(255,255,255,.05)" stroke-width="4" fill="none" stroke-linecap="round"/>`;
    if (dual.a > 0.005) s += `<path d="${arcPath(50, 50, 44, A0, lerp(A0, A1, dual.a))}" stroke="#f3f2ee" stroke-width="5" fill="none" stroke-linecap="round"/>`;
    if (dual.v > 0.005) s += `<path d="${arcPath(50, 50, 36, A0, lerp(A0, A1, dual.v))}" stroke="rgba(243,242,238,.5)" stroke-width="4" fill="none" stroke-linecap="round"/>`;
    s += `<circle cx="50" cy="50" r="27" fill="url(#kg${uid})"/><text x="50" y="47" text-anchor="middle" font-size="10" fill="${dual.a > 0.005 ? '#ecebe8' : '#5f5d5a'}" font-family="Geist Mono, monospace">♪</text><text x="50" y="62" text-anchor="middle" font-size="10" fill="${dual.v > 0.005 ? '#ecebe8' : '#5f5d5a'}" font-family="Geist Mono, monospace">▣</text>`;
    return s;
  }
  const n = (v - M.min) / (M.max - M.min), a = lerp(A0, A1, n);
  if (M.detents) {
    for (let i = 0; i < M.detents; i++) { const da = lerp(A0, A1, i / (M.detents - 1)); s += `<circle cx="${50 + 44 * Math.sin(da)}" cy="${50 - 44 * Math.cos(da)}" r="${i === v ? 3.6 : 2.2}" fill="${i === v ? '#f3f2ee' : 'rgba(255,255,255,.22)'}"/>`; }
  } else {
    const from = M.bipolar ? 0 : A0; if (Math.abs(a - from) > 0.01) s += `<path d="${arcPath(50, 50, 44, Math.min(from, a), Math.max(from, a))}" stroke="#f3f2ee" stroke-width="5" fill="none" stroke-linecap="round"/>`;
    if (M.bipolar) s += `<line x1="50" y1="2" x2="50" y2="9" stroke="rgba(255,255,255,.25)" stroke-width="1.5"/>`;
  }
  s += `<circle cx="50" cy="50" r="33" fill="url(#kg${uid})"/><circle cx="50" cy="50" r="32.5" fill="none" stroke="rgba(255,255,255,.07)"/>`;
  s += `<line x1="${50 + 14 * Math.sin(a)}" y1="${50 - 14 * Math.cos(a)}" x2="${50 + 27 * Math.sin(a)}" y2="${50 - 27 * Math.cos(a)}" stroke="#f3f2ee" stroke-width="3" stroke-linecap="round"/>`;
  return s;
}
function pknob(M, get, set, opts = {}) {
  const uid = ++kuid, host = el('div', 'pk' + (opts.cls ? ' ' + opts.cls : ''));
  host.innerHTML = `<svg viewBox="0 0 100 100"></svg><span class="pk-l">${M.label}</span><span class="pk-v mono"></span>`;
  const svg = host.querySelector('svg'), vEl = host.querySelector('.pk-v');
  const k = { el: host, draw() { const v = get(); if (M.dual) { svg.innerHTML = knobSVG(M, 0, v, uid); vEl.textContent = v.a + v.v < 0.01 ? 'off' : `${Math.round(v.a * 100)}·${Math.round(v.v * 100)}`; } else { svg.innerHTML = knobSVG(M, v, null, uid); vEl.textContent = M.fmt(v); } } };
  let y0, v0, moved, lastTap = 0;
  host.addEventListener('pointerdown', ev => { ev.stopPropagation(); host.setPointerCapture(ev.pointerId); y0 = ev.clientY; v0 = M.dual ? { ...get() } : get(); moved = false; host.classList.add('active'); });
  host.addEventListener('pointermove', ev => {
    if (!host.hasPointerCapture(ev.pointerId)) return; const dy = y0 - ev.clientY; if (Math.abs(dy) > 3) moved = true; if (!moved) return;
    if (M.dual) { const d = dy / 160; set({ a: clamp(v0.a + d, 0, 1), v: clamp(v0.v + d, 0, 1) }); }
    else { let v = clamp(v0 + (dy / (M.detents ? 26 : 170)) * (M.detents ? 1 : M.max - M.min), M.min, M.max); if (M.step) v = Math.round(v / M.step) * M.step; set(v); }
    k.draw();
  });
  host.addEventListener('pointerup', () => {
    host.classList.remove('active'); if (moved) return; const t = now();
    if (M.detents) { set((get() + 1) % (M.max + 1)); k.draw(); }
    else if (M.dual) { const v = get(); set(v.a + v.v > 0.01 ? { a: 0, v: 0 } : { a: 0.5, v: 0.5 }); k.draw(); }
    else if (t - lastTap < 0.35) { set(M.def); k.draw(); }
    lastTap = t;
  });
  host.addEventListener('wheel', ev => { ev.preventDefault(); if (M.dual) return; const v = clamp(get() - Math.sign(ev.deltaY) * (M.step || (M.max - M.min) / 60), M.min, M.max); set(v); k.draw(); }, { passive: false });
  k.draw(); LIVE.add(k); return k;
}
const bindK = (o, key, opts) => pknob(key === 'len' ? PDEF.len : PDEF[key], () => o[key], v => { o[key] = v; onParam(o, key); }, opts);
const bindFx = (o, f, opts) => pknob({ label: FXL[f], dual: true }, () => o.fx[f], v => { o.fx[f] = v; onParam(o, 'fx'); }, opts);
const bindLink = (o, l, opts) => pknob({ label: `${l[0]}→${l[1]}`, min: 0, max: 1, def: 1, fmt: pct }, () => o.links[l[0]], v => { o.links[l[0]] = v; onParam(o, 'link'); }, opts);

/* pills: 44 px rows for FX (♪ audio / ▣ picture amounts) and Links — drag sideways, tap toggles */
function pill(label, get, set, dual) {
  const p = el('div', 'pill', `<span class="pl-n">${label}</span><span class="pl-bars">${dual ? '<i class="a"><b></b></i><i class="v"><b></b></i>' : '<i class="a"><b></b></i>'}</span>`);
  const draw = () => { const v = get(); const a = dual ? v.a : v; p.querySelector('.a b').style.width = `${a * 100}%`; if (dual) p.querySelector('.v b').style.width = `${v.v * 100}%`; p.classList.toggle('on', dual ? v.a + v.v > 0.01 : v > 0.01); };
  let x0, v0, moved;
  p.addEventListener('pointerdown', ev => { ev.stopPropagation(); p.setPointerCapture(ev.pointerId); x0 = ev.clientX; v0 = dual ? { ...get() } : get(); moved = false; });
  p.addEventListener('pointermove', ev => { if (!p.hasPointerCapture(ev.pointerId)) return; const dx = (ev.clientX - x0) / p.clientWidth; if (Math.abs(dx) > 0.02) moved = true; if (!moved) return; set(dual ? { a: clamp(v0.a + dx, 0, 1), v: clamp(v0.v + dx, 0, 1) } : clamp(v0 + dx, 0, 1)); draw(); });
  p.addEventListener('pointerup', () => { if (moved) return; const v = get(); set(dual ? (v.a + v.v > 0.01 ? { a: 0, v: 0 } : { a: 0.5, v: 0.5 }) : v > 0.01 ? 0 : 1); draw(); });
  draw(); const w = { el: p, draw }; LIVE.add(w); return p;
}
const fxPills = o => { const g = el('div', 'pills'); FXN.forEach(f => g.appendChild(pill(FXL[f], () => o.fx[f], v => { o.fx[f] = v; onParam(o, 'fx'); }, true))); return g; };
const linkPills = o => { const g = el('div', 'pills'); LINKN.forEach(([a, b]) => g.appendChild(pill(`${a} → ${b}`, () => o.links[a], v => { o.links[a] = v; }, false))); return g; };

/* small segmented controls */
function segCtl(opts, get, set, cls = '') {
  const s = el('div', 'seg mini-seg ' + cls);
  const draw = () => [...s.children].forEach((b, i) => b.classList.toggle('on', opts[i][0] === get()));
  opts.forEach(([v, l]) => { const b = el('button', '', l); b.addEventListener('click', ev => { ev.stopPropagation(); set(v); draw(); }); s.appendChild(b); });
  draw(); const w = { el: s, draw }; LIVE.add(w); return s;
}
const cutFade = o => segCtl([['cut', 'cut'], ['fade', 'fade']], () => o.cut, v => { o.cut = v; onParam(o, 'cut'); }, 'vseg');
const layoutSeg = (cls = 'vseg') => segCtl([['stack', 'stack'], ['tile', 'tile']], () => S.layout, setLayout, cls);
function setLayout(v) { S.layout = v; [stages.play, stages.seq, stages.perf].forEach(st => st && (st.slots = [])); liveDraw(); }
const keyBtn = () => { const b = el('button', 'tile-btn', `<b>${key().root}</b><span>${key().scale}</span>`); b.addEventListener('click', () => { $('#chip-key').click(); b.innerHTML = `<b>${key().root}</b><span>${key().scale}</span>`; }); return b; };

/* ───── ADSR: one envelope, sound + picture. Drawn over the clip's own thumbnail;
         the lit band shows exactly when the picture is on (cut) or how bright (fade). ───── */
function adsrWidget(o, opts = {}) {
  const root = el('div', 'adsr' + (opts.mini ? ' mini' : ''));
  root.innerHTML = '<svg></svg><span class="adsr-tip mono"></span>';
  const svg = root.firstChild, tip = root.lastChild, uid = ++kuid;
  let W = 0, H = 0, drag = null;
  const geo = () => { const p = opts.mini ? 5 : 14, w = W - 2 * p, h = H - 2 * p; const xA = p + w * 0.25 * Math.sqrt(o.a / 2), xD = xA + w * 0.25 * Math.sqrt(o.d / 2), xS = xD + w * 0.2, xR = xS + w * 0.3 * Math.sqrt(o.r / 3); return { p, w, h, xA, xD, xS, xR, y: l => p + h * (1 - l) }; };
  function stops(g) {
    const X = x => clamp(x / W, 0, 1).toFixed(4); const pts = [[g.p, 0], [g.xA, 1], [g.xD, o.s], [g.xS, o.s], [g.xR, 0]];
    if (o.cut === 'fade') return pts.map(([x, l]) => `<stop offset="${X(x)}" stop-color="#fff" stop-opacity="${l.toFixed(3)}"/>`).join('');
    const th = o.thresh; const xOn = g.p + (g.xA - g.p) * th;
    const xOff = o.s > th ? g.xS + (g.xR - g.xS) * (1 - th / o.s) : g.xA + (g.xD - g.xA) * ((1 - th) / Math.max(1e-3, 1 - o.s));
    return `<stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset="${X(xOn)}" stop-color="#fff" stop-opacity="0"/><stop offset="${X(xOn)}" stop-color="#fff" stop-opacity="1"/><stop offset="${X(xOff)}" stop-color="#fff" stop-opacity="1"/><stop offset="${X(xOff)}" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff" stop-opacity="0"/>`;
  }
  function draw() {
    const r = root.getBoundingClientRect(); W = r.width; H = r.height; if (!W || !H) return;
    svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const g = geo(), src = opts.src ? opts.src() : null;
    let s = '';
    if (src) {
      s += `<defs><linearGradient id="ag${uid}" x1="0" y1="0" x2="${W}" y2="0" gradientUnits="userSpaceOnUse">${stops(g)}</linearGradient><mask id="am${uid}"><rect width="${W}" height="${H}" fill="url(#ag${uid})"/></mask></defs>`;
      s += `<image href="${posterURL(src)}" x="0" y="0" width="${W}" height="${H}" preserveAspectRatio="xMidYMid slice" opacity=".14"/>`;
      s += `<image href="${posterURL(src)}" x="0" y="0" width="${W}" height="${H}" preserveAspectRatio="xMidYMid slice" mask="url(#am${uid})" opacity=".85"/>`;
    }
    const pts = `${g.p},${g.y(0)} ${g.xA},${g.y(1)} ${g.xD},${g.y(o.s)} ${g.xS},${g.y(o.s)} ${g.xR},${g.y(0)}`;
    s += `<polygon points="${pts}" fill="rgba(10,10,11,.35)"/>`;
    s += `<polyline points="${pts}" fill="none" stroke="#f3f2ee" stroke-width="${opts.mini ? 1.5 : 2}" stroke-linejoin="round" stroke-linecap="round"/>`;
    if (o.cut !== 'fade') { const ty = g.y(o.thresh); s += `<line x1="${g.p}" x2="${W - g.p}" y1="${ty}" y2="${ty}" stroke="rgba(243,242,238,.4)" stroke-dasharray="2 3"/>`; }
    if (!opts.mini) for (const [k, hx, hy] of [['a', g.xA, g.y(1)], ['d', g.xD, g.y(o.s)], ['r', g.xR, g.y(0)]]) s += `<circle cx="${hx}" cy="${hy}" r="22" fill="transparent" data-h="${k}" style="cursor:grab"/><circle cx="${hx}" cy="${hy}" r="${drag === k ? 8.5 : 7}" fill="#f3f2ee" stroke="#0b0b0c" stroke-width="2" pointer-events="none"/>`;
    svg.innerHTML = s;
  }
  if (!opts.mini) {
    svg.addEventListener('pointerdown', ev => { const h = ev.target.dataset?.h; if (!h) return; ev.stopPropagation(); drag = h; svg.setPointerCapture(ev.pointerId); });
    svg.addEventListener('pointermove', ev => {
      if (!drag) return; const r = svg.getBoundingClientRect(); const g = geo(); const x = ev.clientX - r.left, y = ev.clientY - r.top;
      if (drag === 'a') o.a = 2 * clamp((x - g.p) / (g.w * 0.25), 0, 1) ** 2;
      if (drag === 'd') { o.d = 2 * clamp((x - g.xA) / (g.w * 0.25), 0, 1) ** 2; o.s = clamp(1 - (y - g.p) / g.h, 0, 1); }
      if (drag === 'r') o.r = 3 * clamp((x - g.xS) / (g.w * 0.3), 0, 1) ** 2;
      tip.textContent = drag === 'a' ? `A ${sec(o.a)}` : drag === 'd' ? `D ${sec(o.d)} · S ${pct(o.s)}` : `R ${sec(o.r)}`;
      tip.style.left = `${clamp(x, 40, W - 40)}px`; tip.style.top = `${Math.max(14, y - 26)}px`; tip.classList.add('show');
      onParam(o, drag);
    });
    const end = () => { drag = null; tip.classList.remove('show'); draw(); };
    svg.addEventListener('pointerup', end); svg.addEventListener('pointercancel', end);
  } else if (opts.onTap) root.addEventListener('click', opts.onTap);
  new ResizeObserver(draw).observe(root);
  const w = { el: root, draw }; LIVE.add(w); return w;
}

/* ───── CHOP (Koala-style): waveform + filmstrip, slices as alternating bands.
         auto (transients) · equal 4/8/16 · tap (play and tap to drop chops) · drag edges · double-tap deletes ───── */
function peaksOf(id) { const p = window.PEAKS && window.PEAKS[id]; return p ? p.audio || p.motion : new Array(192).fill(0.2); }
const hasSound = id => !!(window.PEAKS && window.PEAKS[id] && window.PEAKS[id].audio);
function autoCuts(id) {
  const p = peaksOf(id), N = p.length, out = []; let last = -99;
  for (let i = 2; i < N - 2; i++) if (p[i] > 0.42 && p[i] >= p[i - 1] && p[i] >= p[i + 1] && p[i] - Math.min(p[i - 2], p[i - 1]) > 0.1 && i - last > 9) { out.push(i / N); last = i; }
  return out.length < 3 ? [0.25, 0.5, 0.75] : out.slice(0, 15);
}
function cutsOf(id) { const s = SRC[id]; if (!s.cuts) s.cuts = autoCuts(id); return s.cuts; }
function slicesOf(id) { const b = [0, ...cutsOf(id), 1]; return b.slice(0, -1).map((s, i) => [s, b[i + 1]]); }
function chopView(id, opts = {}) {
  const root = el('div', 'chopv' + (opts.compact ? ' compact' : ''));
  const film = el('div', 'cv-film'); for (let k = 0; k < 16; k++) film.appendChild(el('i'));
  const wave = el('div', 'cv-wave'); const cv = el('canvas'); wave.appendChild(cv);
  if (!hasSound(id)) wave.appendChild(el('span', 'cv-lab', 'picture'));
  root.append(film, wave);
  const st = { mode: null, ph: null, sel: -1, t0: 0, aS: null };
  let tools = null;
  if (!opts.compact) {
    tools = el('div', 'cv-tools');
    const seg = el('div', 'seg'); [['auto', 'Auto'], [4, '4'], [8, '8'], [16, '16'], ['tap', 'Tap']].forEach(([m, l]) => { const b = el('button', '', l); b.dataset.m = m; b.addEventListener('click', ev => { ev.stopPropagation(); mode(m); }); seg.appendChild(b); });
    const tapBtn = el('button', 'btn tapbtn', 'Tap'); tapBtn.addEventListener('pointerdown', ev => { ev.preventDefault(); ev.stopPropagation(); dropCut(); });
    const n = el('span', 'cv-n mono');
    const toPads = el('button', 'btn primary', 'Slices → pads'); toPads.addEventListener('click', ev => { ev.stopPropagation(); slicesToPads(id); });
    tools.append(seg, tapBtn, n, toPads); root.appendChild(tools); st.seg = seg; st.n = n;
  }
  function mode(m) {
    if (st.seg) $$('button', st.seg).forEach(x => x.classList.toggle('on', String(x.dataset.m) === String(m)));
    if (m === 'auto') { SRC[id].cuts = autoCuts(id); st.mode = null; }
    else if (m === 'tap') { SRC[id].cuts = []; st.mode = 'tap'; st.t0 = now(); initAudio(); }
    else { SRC[id].cuts = Array.from({ length: m - 1 }, (_, k) => (k + 1) / m); st.mode = null; }
    root.classList.toggle('tapping', st.mode === 'tap'); liveDraw();
  }
  function dropCut() {
    if (st.mode !== 'tap') { mode('tap'); return; }
    const p = ((now() - st.t0) / ATLAS.dur) % 1; const c = cutsOf(id);
    if (c.every(x => Math.abs(x - p) > 0.015)) { c.push(p); c.sort((a, b) => a - b); }
    root.classList.remove('flash'); void root.offsetWidth; root.classList.add('flash'); liveDraw();
  }
  function audition(k) {
    const sl = slicesOf(id)[k]; if (!sl) return; const [s0, s1] = sl; st.sel = k; st.aT = now(); st.aS = sl;
    const o = opts.owner ? opts.owner() : PROJ; const gate = (s1 - s0) * ATLAS.dur;
    world.hits.push({ src: id, f: s0 * 47, t0: now(), gate, dur: gate + o.r, o, mode: 'frame', aud: true });
    audioGrain(id, s0, Math.min(gate, 1.5), o.pitch || 0, 0, 0.55, o.smear || 0.1);
  }
  function draw() {
    const fr = film.getBoundingClientRect(); const a = fr.width / 16 / Math.max(1, fr.height);
    [...film.children].forEach((f, k) => bgFrame(f, id, k * 3, a || 1.6));
    const r = wave.getBoundingClientRect(); const dpr = Math.min(2, devicePixelRatio || 1);
    const W = Math.max(2, (r.width * dpr) | 0), H = Math.max(2, (r.height * dpr) | 0); if (cv.width !== W || cv.height !== H) { cv.width = W; cv.height = H; }
    const x = cv.getContext('2d'); x.clearRect(0, 0, W, H);
    const sl = slicesOf(id);
    sl.forEach(([s0, s1], k) => { if (k % 2) { x.fillStyle = 'rgba(255,255,255,.04)'; x.fillRect(s0 * W, 0, (s1 - s0) * W, H); } if (k === st.sel) { x.fillStyle = 'rgba(243,242,238,.13)'; x.fillRect(s0 * W, 0, (s1 - s0) * W, H); } });
    const p = peaksOf(id), N = p.length, bw = W / N, mid = H / 2, snd = hasSound(id);
    x.fillStyle = snd ? 'rgba(236,235,232,.8)' : 'rgba(236,235,232,.34)';
    for (let i = 0; i < N; i++) { if (snd) { const h = Math.max(dpr, p[i] * H * 0.42); x.fillRect(i * bw, mid - h, Math.max(1, bw - dpr), h * 2); } else { const h = Math.max(dpr, p[i] * H * 0.8); x.fillRect(i * bw, H - h, Math.max(1, bw - dpr), h); } }
    x.fillStyle = 'rgba(243,242,238,.92)'; cutsOf(id).forEach(c => x.fillRect(Math.round(c * W - dpr / 2), 0, dpr, H));
    if (st.ph != null) { x.fillStyle = '#fff'; x.fillRect(st.ph * W - dpr, 0, 2 * dpr, H); }
    if (st.n) st.n.textContent = `${sl.length} slices`;
  }
  let dragC = -1, lastTap = 0;
  wave.addEventListener('pointerdown', ev => {
    ev.stopPropagation(); initAudio(); A.ctx?.resume?.(); const r = wave.getBoundingClientRect(); const px = (ev.clientX - r.left) / r.width;
    if (st.mode === 'tap') { dropCut(); return; }
    const c = cutsOf(id); const near = c.findIndex(v => Math.abs(v - px) * r.width < 12);
    if (near >= 0 && !opts.compact) { dragC = near; wave.setPointerCapture(ev.pointerId); return; }
    const k = slicesOf(id).findIndex(([a, b]) => px >= a && px < b);
    const t = now(); if (t - lastTap < 0.32 && k === st.sel && k > 0 && !opts.compact) { c.splice(k - 1, 1); st.sel = -1; liveDraw(); lastTap = 0; return; }
    lastTap = t; audition(k);
  });
  wave.addEventListener('pointermove', ev => { if (dragC < 0) return; const r = wave.getBoundingClientRect(); const c = cutsOf(id); c[dragC] = clamp((ev.clientX - r.left) / r.width, 0.005, 0.995); c.sort((a, b) => a - b); draw(); });
  wave.addEventListener('pointerup', () => { if (dragC >= 0) liveDraw(); dragC = -1; });
  const w = { el: root, draw, tick(t) {
    if (st.mode === 'tap') { st.ph = ((t - st.t0) / ATLAS.dur) % 1; const s = Math.floor(st.ph * 24); if (s !== st.ls) { st.ls = s; if (s % 2 === 0) audioGrain(id, st.ph, 0.12, 0, 0, 0.22, 0.1); } draw(); }
    else if (st.aS) { const ph = st.aS[0] + (t - st.aT) / ATLAS.dur; if (ph > st.aS[1]) { st.aS = null; st.ph = null; } else st.ph = ph; draw(); }
  }, mode, dropCut };
  new ResizeObserver(draw).observe(root);
  LIVE.add(w); TICKERS.add(w); return w;
}
function slicesToPads(id) {
  const sl = slicesOf(id).slice(0, 15);
  PADS.forEach(p => p && p.on && setPad(p, false));
  sl.forEach((s, k) => { PADS[k] = ensureP({ src: id, mode: 'slice', slice: s, sliceN: k, look: 0, i: k, on: false, q: false, t0: 0, frame: 0 }, { r: 0.03, a: 0.002, d: 0.1, s: 0.9 }); });
  closeSheets(); closeClip(); buildPads(); setView('perform');
  toast(`${SRC[id].name} · ${sl.length} slices → pads 1–${sl.length}`);
}

/* ───── sheets & popovers ───── */
function closeSheets() { $$('.sheet, .sheet-scrim').forEach(e => e.remove()); }
function openSheet(content, { cls = '', title = '', sub = '', anchor = null } = {}) {
  closeSheets(); hideMini(false);
  const scrim = el('div', 'sheet-scrim' + (anchor ? ' light' : '')); const sh = el('div', 'sheet ' + cls + (anchor ? ' anchored' : ''));
  if (title) { const h = el('div', 'sheet-head', `<div><div class="sheet-t">${title}</div>${sub ? `<div class="sheet-s">${sub}</div>` : ''}</div>`); const b = el('button', 'chip ghost', 'Done'); b.addEventListener('click', closeSheets); h.appendChild(b); sh.appendChild(h); }
  sh.appendChild(content);
  scrim.addEventListener('pointerdown', closeSheets);
  $('#app').append(scrim, sh);
  if (anchor) { // C: grows out of the object it belongs to
    const ar = anchor.getBoundingClientRect(), W = innerWidth, H = innerHeight; const sw = sh.offsetWidth, shh = sh.offsetHeight;
    let left = clamp(ar.left + ar.width / 2 - sw / 2, 12, W - sw - 12), top = ar.bottom + 10; if (top + shh > H - 12) top = Math.max(12, ar.top - shh - 10);
    sh.style.left = `${left}px`; sh.style.top = `${top}px`;
    const arrowX = clamp(ar.left + ar.width / 2 - left, 20, sw - 20); sh.style.setProperty('--ax', `${arrowX}px`); sh.classList.toggle('above', top < ar.top);
  }
  liveDraw(); return sh;
}
function provLine(id) { const s = SRC[id]; return s.prov ? `${s.prov.title} · ${s.prov.views} views · ${s.prov.up} · ${s.prov.by}` : s.origin || ''; }
function openChop(id, anchor) { if (!id) return; const c = chopView(id, { owner: () => (S.sel.ref.a !== undefined ? S.sel.ref : PROJ) }); openSheet(c.el, { cls: 'chop-sheet', title: `Chop · ${SRC[id].name}`, sub: provLine(id), anchor: S.dir === 'C' ? anchor : null }); }

/* ───── ADD sheet: Files · Browse · Link · Dig (random unseen videos) ───── */
const DIGGING = new Set();
const DUG_LEFT = DUG.slice();
const LIB = SOURCES.map(s => s.id);
function openAdd(tab = 'files') {
  const c = el('div', 'add');
  const seg = el('div', 'seg add-tabs'); const body = el('div', 'add-body');
  [['files', 'Files'], ['browse', 'Browse'], ['link', 'Link'], ['dig', 'Dig']].forEach(([k, n]) => { const b = el('button', k === tab ? 'on' : '', n); b.id = `add-${k}`; b.addEventListener('click', () => { $$('button', seg).forEach(x => x.classList.toggle('on', x === b)); show(k); }); seg.appendChild(b); });
  c.append(seg, body);
  function card(id) { const s = SRC[id]; const d = el('div', 'lib-card', `<div class="lc-th" style="background-image:url(${posterURL(id)})"></div><div class="lc-t">${s.prov ? s.prov.title : s.name}</div><div class="lc-s mono">${s.prov ? `${s.prov.views} views` : s.file || ''}</div>`); d.addEventListener('click', () => { toast(`${s.name} → next empty column`); closeSheets(); }); return d; }
  function show(t) {
    body.innerHTML = ''; body.dataset.t = t;
    if (t === 'files') {
      const drop = el('label', 'drop', `<svg viewBox="0 0 24 24" class="ico big"><path d="M12 16V4M7.5 8.5 12 4l4.5 4.5M5 14v5h14v-5"/></svg><b>Drop videos anywhere</b><span>or choose files</span><input type="file" accept="video/*" multiple hidden>`);
      drop.querySelector('input').addEventListener('change', () => toast('Imported — open Chop to slice it'));
      const rec = el('div', 'lib-row'); LIB.slice(0, 4).forEach(id => rec.appendChild(card(id)));
      body.append(drop, el('div', 'cap', 'Recent'), rec);
    } else if (t === 'browse') {
      const g = el('div', 'lib-grid'); [...LIB, ...ALLSRC.filter(s => !LIB.includes(s.id) && !DUG_LEFT.includes(s)).map(s => s.id)].forEach(id => g.appendChild(card(id)));
      body.append(el('div', 'cap', 'Videos on this Mac'), g);
    } else if (t === 'link') {
      const f = el('div', 'linkf', `<input id="link-in" placeholder="Paste a YouTube or any video link" spellcheck="false"><button class="btn primary" id="link-go">Fetch</button>`);
      f.querySelector('button').addEventListener('click', () => toast('Fetching ≤480p · first 60 s'));
      body.append(f, el('div', 'cap dim', 'yt-dlp · up to 480p · keeps title, uploader and date'));
    } else {
      const top = el('div', 'dig-top');
      const btn = el('button', 'digbtn', '<b>Dig</b><span>hold: dig 4</span>'); btn.id = 'dig-go';
      const q = el('div', 'dig-q mono', DIG_Q.join('  ·  '));
      top.append(btn, q);
      const res = el('div', 'dig-res'); res.id = 'dig-res';
      let ht, held = false;
      btn.addEventListener('pointerdown', () => { held = false; ht = setTimeout(() => { held = true; for (let k = 0; k < 4; k++) setTimeout(() => digOne(res), k * 350); }, 550); });
      btn.addEventListener('pointerup', () => { clearTimeout(ht); if (!held) digOne(res); });
      body.append(top, res, el('div', 'cap dim', 'random, rarely seen · camera filenames · < 500 views · 10 s–10 min · cuts a 20–60 s window'));
      DUG_DONE.forEach(id => res.appendChild(digCard(id)));
    }
  }
  show(tab);
  return openSheet(c, { cls: 'add-sheet', title: 'Add video' });
}
const DIG_Q = ['IMG_4821', 'MVI_0032', 'DSC_1177', 'VID_20140611', 'MOV_0418', 'DSCN0090'];
const DUG_DONE = [];
function digCard(id) { const s = SRC[id]; const d = el('div', 'dig-card', `<div class="dc-th" style="background-image:url(${posterURL(id)})"></div><div class="dc-t mono">${s.prov.title}</div><div class="dc-s">${s.prov.views} views · ${s.prov.up} · ${s.prov.by}</div>`); const b = el('button', 'btn', 'Chop'); b.addEventListener('click', () => openChop(id)); d.appendChild(b); return d; }
function digOne(res) {
  const d = DUG_LEFT.shift(); if (!d) { toast('Dug everything nearby — try again later'); return; }
  const card = el('div', 'dig-card loading', `<div class="dc-th"><i class="shimmer"></i></div><div class="dc-t mono">ytsearch: ${d.prov.q}</div><div class="dc-s">searching · &lt; 500 views</div>`);
  res && res.prepend(card);
  const col = GRID_COLS.indexOf(null); if (col >= 0) { DIGGING.add(col); cells.forEach((c, k) => k % 8 === col && c.classList.add('digging')); }
  setTimeout(() => { const s = card.querySelector('.dc-s'); if (s) s.textContent = 'downloading 480p · cutting a window'; }, 650);
  setTimeout(() => {
    DUG_DONE.unshift(d.id); LIB.push(d.id);
    if (card.isConnected) card.replaceWith(digCard(d.id));
    if (col >= 0) { GRID_COLS[col] = d.id; DIGGING.delete(col); buildGrid(); toast(`${d.prov.title} → column ${col + 1}`); }
    else toast(`${d.prov.title} → library`);
  }, 1700);
}

/* ═════ A · PAGES ═════ */
const SPAN = { adsr: 4, adsrMini: 2, chopMini: 4, chain: 8, library: 8, script: 8 };
const SRC_PAGE = ['clip', 'chopMini', 'btn:chop', 'btn:toPads'];
const SHAPE_PAGE = ['adsr', 'cutfade', 'k:thresh', 'layout', 'k:tilePan'];
const PITCH_PAGE = ['k:pitch', 'k:fine', 'k:octave', 'k:pjit', 'k:glide', 'snap', 'key'];
const GRAIN_PAGE = ['k:density', 'k:length', 'k:spray', 'k:jitter', 'k:scan', 'k:reverse', 'k:spread', 'k:seed'];
const FX_PAGE = FXN.map(f => 'fx:' + f), LINK_PAGE = LINKN.map(l => 'link:' + l[0]);
const PAGES = {
  emitter: { MACRO: ['k:grain', 'k:pitch', 'k:smear', 'k:look', 'k:level', 'k:pan', 'adsrMini'], SOURCE: SRC_PAGE, GRAIN: GRAIN_PAGE, SHAPE: SHAPE_PAGE, PITCH: PITCH_PAGE, FX: FX_PAGE, LINK: LINK_PAGE },
  pad: { MACRO: ['k:level', 'k:pitch', 'k:look', 'mode', 'k:smear', 'k:grain', 'adsrMini'], SOURCE: SRC_PAGE, GRAIN: GRAIN_PAGE, SHAPE: SHAPE_PAGE, PITCH: PITCH_PAGE, FX: FX_PAGE, LINK: LINK_PAGE },
  lane: { MACRO: ['k:level', 'k:pitch', 'k:look', 'k:len', 'k:swing', 'k:chance', 'mute', 'pattern'], SOURCE: SRC_PAGE, SHAPE: SHAPE_PAGE, PITCH: PITCH_PAGE, FX: FX_PAGE, LINK: LINK_PAGE },
  clip: { SOURCE: SRC_PAGE, SHAPE: SHAPE_PAGE, GRAIN: GRAIN_PAGE, PITCH: PITCH_PAGE, FX: FX_PAGE, LINK: LINK_PAGE },
  project: { MAIN: ['tempo', 'key', 'k:swing', 'k:level', 'layout', 'quant', 'btn:export'], SONG: ['chain'], LIBRARY: ['library'], FX: FX_PAGE, SCRIPT: ['script'] },
};
function renderStrip() {
  const s = $('#strip'); if (S.dir !== 'A' || !s) return;
  const kind = S.sel.kind, pages = PAGES[kind], names = Object.keys(pages);
  let page = S.page[kind]; if (!names.includes(page)) page = S.page[kind] = names[0];
  const inf = selInfo(), o = S.sel.ref;
  s.innerHTML = '';
  const id = el('div', 'st-id'); const th = el('div', 'st-thumb' + (inf.src ? '' : ' proj'));
  if (!inf.src) th.innerHTML = '<span class="brand-mark"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></span>';
  id.append(th, el('div', 'st-name', `<div class="st-t">${inf.t}</div><div class="st-s">${inf.s}</div>`));
  const tabs = el('div', 'st-tabs'); tabs.setAttribute('role', 'tablist');
  names.forEach(n => { const b = el('button', n === page ? 'on' : '', n); b.id = `page-${n.toLowerCase()}`; b.addEventListener('click', () => { S.page[kind] = n; renderStrip(); }); tabs.appendChild(b); });
  const slots = el('div', 'st-slots'); let used = 0;
  pages[page].forEach(item => { const sp = SPAN[item.split(':')[0]] || 1; const w = makeWidget(item, o, kind); w.style.gridColumn = `span ${sp}`; used += sp; slots.appendChild(w); });
  for (; used < 8; used++) slots.appendChild(el('div', 'slot empty'));
  s.append(id, tabs, slots);
  requestAnimationFrame(() => { inf.src && bgFrame(th, inf.src, inf.f, th.clientWidth / Math.max(1, th.clientHeight)); liveDraw(); });
}
function tileBtn(label, sub, fn, cls = '') { const b = el('button', 'tile-btn ' + cls, `<b>${label}</b>${sub ? `<span>${sub}</span>` : ''}`); b.addEventListener('click', ev => { ev.stopPropagation(); fn(b); }); return b; }
function makeWidget(item, o, kind) {
  const [t, arg] = item.split(':'); const w = el('div', 'slot s-' + t);
  if (t === 'k') w.appendChild(bindK(o, arg).el);
  else if (t === 'fx') w.appendChild(bindFx(o, arg).el);
  else if (t === 'link') w.appendChild(bindLink(o, LINKN.find(l => l[0] === arg)).el);
  else if (t === 'adsr') w.appendChild(adsrWidget(o, { src: () => selSrc(), frame: () => selInfo().f }).el);
  else if (t === 'adsrMini') { w.append(adsrWidget(o, { mini: true, src: () => selSrc(), onTap: () => { S.page[kind] = 'SHAPE'; renderStrip(); } }).el, el('span', 'slot-l', 'Shape ›')); }
  else if (t === 'cutfade') w.append(cutFade(o), el('span', 'slot-l', 'Picture'));
  else if (t === 'layout') w.append(layoutSeg(), el('span', 'slot-l', 'Layout'));
  else if (t === 'quant') w.append(segCtl([[0, 'now'], [4, 'beat'], [16, 'bar']], () => S.quant, v => { S.quant = v; $$('#seg-quant button').forEach(x => x.classList.toggle('on', +x.dataset.q === v)); }, 'vseg three'), el('span', 'slot-l', 'Launch'));
  else if (t === 'chopMini') { const src = selSrc(); if (src) w.appendChild(chopView(src, { compact: true, owner: () => o }).el); }
  else if (t === 'clip') { const src = selSrc(); const c = el('button', 'clip-pick', `<i style="background-image:url(${posterURL(src)})"></i><b>${SRC[src].name}</b><span>Replace</span>`); c.addEventListener('click', () => openAdd()); w.appendChild(c); }
  else if (t === 'btn') { const B = { chop: ['Chop', 'tap · auto · 16', () => openChop(selSrc())], toPads: ['Slices', '→ pads', () => slicesToPads(selSrc())], export: ['Export', 'mp4 + wav', () => toast('Export · renders mp4 + wav')] }[arg]; w.appendChild(tileBtn(B[0], B[1], B[2])); }
  else if (t === 'mode') w.appendChild(tileBtn(MODE_GLYPH[o.mode], o.mode, b => { if (o.mode === 'slice') return; o.mode = { loop: 'grain', grain: 'chop', chop: 'loop' }[o.mode]; b.innerHTML = `<b>${MODE_GLYPH[o.mode]}</b><span>${o.mode}</span>`; buildPads(); markSelection(); }));
  else if (t === 'mute') w.appendChild(tileBtn('M', o.mute ? 'muted' : 'mute', b => { o.mute = !o.mute; b.classList.toggle('on', o.mute); b.querySelector('span').textContent = o.mute ? 'muted' : 'mute'; renderLanes(); }, o.mute ? 'on' : ''));
  else if (t === 'pattern') w.appendChild(tileBtn(laneSlot(o) || '–', 'pattern', b => { const sc = SCENES[S.scene]; if (!sc) return; sc[o.i] = sc[o.i] === 'A' ? 'B' : sc[o.i] === 'B' ? null : 'A'; b.querySelector('b').textContent = sc[o.i] || '–'; renderLanes(); renderSong(); }));
  else if (t === 'snap') w.appendChild(tileBtn('♮', o.snap ? 'in key' : 'free', b => { o.snap = !o.snap; b.classList.toggle('on', o.snap); b.querySelector('span').textContent = o.snap ? 'in key' : 'free'; }, o.snap ? 'on' : ''));
  else if (t === 'key') w.appendChild(keyBtn());
  else if (t === 'tempo') w.appendChild(pknob(TEMPO_DEF, () => S.bpm, setBpm).el);
  else if (t === 'chain') w.appendChild(chainEditor());
  else if (t === 'library') w.appendChild(libraryRow());
  else if (t === 'script') w.appendChild(scriptBlock());
  return w;
}
function chainEditor() {
  const r = el('div', 'chain-ed');
  const draw = () => {
    r.innerHTML = '';
    CHAIN.forEach((c, k) => { const b = el('button', 'ce' + (S.playing && k === S.chainPos ? ' cur' : ''), `<b>${c.s + 1}</b><span>×${c.r}</span>`); b.addEventListener('click', () => { c.r = { 1: 2, 2: 4, 4: 8, 8: 1 }[c.r] || 1; draw(); renderSong(); }); r.appendChild(b); });
    const add = el('button', 'ce add', '<b>+</b><span>scene</span>'); add.addEventListener('click', () => { CHAIN.push({ s: (CHAIN[CHAIN.length - 1].s + 1) % 4, r: 2 }); draw(); renderSong(); }); r.appendChild(add);
  };
  draw(); const w = { el: r, draw }; LIVE.add(w); return r;
}
function libraryRow() {
  const r = el('div', 'lib-strip');
  LIB.forEach(id => { const d = el('button', 'ls', `<i style="background-image:url(${posterURL(id)})"></i><span>${SRC[id].name}</span>`); d.addEventListener('click', () => openChop(id)); r.appendChild(d); });
  r.append(tileBtn('+', 'add', () => openAdd()), tileBtn('Dig', 'unseen', () => openAdd('dig')));
  return r;
}
function scriptBlock() { const d = el('div', 'script'); d.contentEditable = 'true'; d.spellcheck = false; d.innerHTML = `<span class="k">fn</span> on_step(lane, step) {\n  <span class="k">if</span> step % 4 == 0 { lane.pitch = scale_degree(rand(0, 5)); }\n  lane.look = <span class="k">if</span> bar() % 8 == 7 { "kaleido" } <span class="k">else</span> { "clean" };\n}`; return d; }

/* ═════ B · CLIP PAGE ═════ */
function usersOf(src) {
  const u = [];
  EMITTERS.forEach(e => GRID_COLS[e.col] === src && u.push({ kind: 'emitter', ref: e, l: `Emitter ${e.i + 1}` }));
  LANES.forEach(l => l.src === src && u.push({ kind: 'lane', ref: l, l: `Lane ${l.i + 1}` }));
  PADS.forEach(p => p && p.src === src && u.push({ kind: 'pad', ref: p, l: `Pad ${p.i + 1}` }));
  return u;
}
const VIEW_NAME = { play: 'Play', seq: 'Sequence', perform: 'Perform' };
function openClip(sel) {
  const src = selSrc(sel); if (!src) return;
  closeSheets(); hideMini(false); select(sel);
  const cp = $('#clip-page'); cp.innerHTML = ''; cp.className = 'clip-page show'; $('#app').classList.add('clip-open');
  const o = sel.ref, s = SRC[src];
  const head = el('div', 'cp-head');
  const back = el('button', 'btn back', `<svg viewBox="0 0 24 24" class="ico"><path d="M14.5 6 8.5 12l6 6"/></svg>${VIEW_NAME[S.view]}`); back.id = 'clip-back'; back.addEventListener('click', closeClip);
  const title = el('div', 'cp-title', `<b>${s.name}</b><span>${provLine(src)}</span>`);
  const owners = el('div', 'seg cp-owners'); [{ kind: 'clip', ref: ensureP(SRC[src]), l: 'Clip' }, ...usersOf(src)].forEach(u => { const b = el('button', u.ref === o ? 'on' : '', u.l); b.addEventListener('click', () => openClip(u)); owners.appendChild(b); });
  const acts = el('div', 'cp-acts'); acts.append(tileBtnSm('Replace', () => openAdd()), tileBtnSm('Dig', () => openAdd('dig')));
  head.append(back, title, owners, acts);
  const top = el('div', 'cp-top');
  const stage = el('div', 'stage cp-stage', '<canvas id="cv-clip"></canvas><div class="stage-ov"></div>'); stage.querySelector('.stage-ov').appendChild(layoutSwitch());
  const mac = el('div', 'cp-macros'); ['grain', 'pitch', 'smear', 'look', 'level', 'pan'].forEach(k => mac.appendChild(bindK(o, k).el));
  top.append(stage, mac);
  const chop = el('section', 'cp-card cp-chop'); chop.append(el('div', 'cap', 'Chop'), chopView(src, { owner: () => o }).el);
  const cards = el('div', 'cp-cards');
  const card = (t, ...kids) => { const c = el('section', 'cp-card'); c.append(el('div', 'cap', t), ...kids); return c; };
  const shapeRow = el('div', 'row'); shapeRow.append(cutFade(o), bindK(o, 'thresh', { cls: 'sm' }).el);
  const grid4 = keys => { const g = el('div', 'k4'); keys.forEach(k => g.appendChild(bindK(o, k, { cls: 'sm' }).el)); return g; };
  cards.append(
    card('Shape · sound + picture', adsrWidget(o, { src: () => src, frame: () => 12 }).el, shapeRow),
    card('Grain', grid4(['density', 'length', 'spray', 'jitter'])),
    card('Pitch', grid4(['fine', 'octave', 'pjit', 'glide'])),
    card('Effects  ♪ ▣', fxPills(o)),
    card('Links', linkPills(o)),
  );
  cp.append(head, top, chop, cards);
  stages.clip = new Stage($('#cv-clip')); S.clipSrc = src; S.clipOwner = o;
  liveDraw();
}
function tileBtnSm(l, fn) { const b = el('button', 'btn', l); b.addEventListener('click', fn); return b; }
function closeClip() { const cp = $('#clip-page'); if (!cp) return; cp.classList.remove('show'); cp.innerHTML = ''; $('#app').classList.remove('clip-open'); stages.clip = null; }
function drawClipStage(st, t) {
  const x = st.x, o = S.clipOwner, id = S.clipSrc;
  x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1; x.fillStyle = 'rgba(0,0,0,.25)'; x.fillRect(0, 0, st.w, st.h);
  const d = srcDims(media[id]); if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, st.w, st.h, o.look, world.hits.some(h => h.aud && hitAlpha(h, t) > 0) ? 0.25 : 0.9);
  drawHits(st, t, h => h.aud);
}
function openProject() {
  closeSheets(); hideMini(false);
  const cp = $('#clip-page'); cp.innerHTML = ''; cp.className = 'clip-page show proj'; $('#app').classList.add('clip-open');
  const head = el('div', 'cp-head');
  const back = el('button', 'btn back', `<svg viewBox="0 0 24 24" class="ico"><path d="M14.5 6 8.5 12l6 6"/></svg>${VIEW_NAME[S.view]}`); back.addEventListener('click', closeClip);
  head.append(back, el('div', 'cp-title', `<b>Project</b><span>untitled · 7 clips · 4 scenes</span>`), el('div'), el('div', 'cp-acts'));
  head.lastChild.append(tileBtnSm('Export', () => toast('Export · mp4 + wav')));
  const card = (t, cls, ...kids) => { const c = el('section', 'cp-card ' + cls); c.append(el('div', 'cap', t), ...kids); return c; };
  const time = el('div', 'pj-time'); time.append(pknob(TEMPO_DEF, () => S.bpm, setBpm).el, keyBtn(), bindK(PROJ, 'swing').el, bindK(PROJ, 'level').el);
  const lay = el('div', 'row'); lay.append(layoutSeg(''), segCtl([[0, 'now'], [4, 'beat'], [16, 'bar']], () => S.quant, v => (S.quant = v)));
  const lib = el('div', 'lib-grid sm'); LIB.forEach(id => { const d = el('button', 'lib-card', `<div class="lc-th" style="background-image:url(${posterURL(id)})"></div><div class="lc-t">${SRC[id].name}</div>`); d.addEventListener('click', () => { const u = usersOf(id)[0]; u ? openClip(u) : openChop(id); }); lib.appendChild(d); });
  lib.append(tileBtn('+', 'add', () => openAdd()), tileBtn('Dig', 'unseen', () => openAdd('dig')));
  const grid = el('div', 'pj-grid');
  const scn = el('div', 'pj-scenes'); SCENES.forEach((sc, k) => { const b = el('button', 'scene' + (sc ? '' : ' empty') + (k === S.scene ? ' cur' : ''), `${k + 1}${sc ? `<span class="bars">${sc.map(x => `<i class="${x === 'B' ? 'b' : x ? '' : 'x'}"></i>`).join('')}</span>` : ''}`); scn.appendChild(b); });
  grid.append(card('Time & key', 'pj-a', time, lay), card('Song · scenes', 'pj-b', chainEditor(), scn), card('Library', 'pj-c', lib), card('Master effects  ♪ ▣', 'pj-d', fxPills(PROJ)), card('Script', 'pj-e', scriptBlock()));
  cp.append(head, grid); liveDraw();
}

/* ═════ C · ON-OBJECT ═════ */
/* gesture grammar, same on every object:
   tap = play / select · hold = its mini-strip · swipe ↕ = smear · twist (or drag the pitch dot, or wheel) = pitch
   pinch (or drag the ring, or ⇧wheel) = grain size · swipe ↔ = level */
function ringGeo(o, W, H, small) { const R1 = Math.min(W, H) * (small ? 0.34 : 0.36), R0 = Math.min(W, H) * 0.1; return { cx: W / 2, cy: H * (small ? 0.46 : 0.45), rr: lerp(R1, R0, o.grain), R1, R0 }; }
function gesture(elm, o, cfg) {
  const pts = new Map(); let mode = null, st = null, ht = 0, moved = false;
  const bub = el('span', 'gbub mono'); elm.appendChild(bub);
  const show = k => { const M = PDEF[k]; bub.textContent = `${M.label}  ${M.fmt(o[k])}`; bub.classList.add('show'); };
  const upd = () => { onParam(o, 'gesture'); cfg.redraw && cfg.redraw(); };
  elm.addEventListener('pointerdown', ev => {
    if (ev.target.closest('.no-g')) return;
    ev.stopPropagation(); elm.setPointerCapture(ev.pointerId); pts.set(ev.pointerId, [ev.clientX, ev.clientY]);
    if (pts.size === 2) { if (cfg.cOnly && S.dir !== 'C') return; clearTimeout(ht); const [a, b] = [...pts.values()]; st = { d: Math.hypot(b[0] - a[0], b[1] - a[1]), ang: Math.atan2(b[1] - a[1], b[0] - a[0]), grain: o.grain, pitch: o.pitch }; mode = 'two'; return; }
    moved = false; const r = elm.getBoundingClientRect(); const g = ringGeo(o, r.width, r.height, cfg.small);
    const dx = ev.clientX - (r.left + g.cx), dy = ev.clientY - (r.top + g.cy), dist = Math.hypot(dx, dy);
    const ha = (o.pitch / 7) * 0.75 * Math.PI, hx = Math.sin(ha) * g.rr, hy = -Math.cos(ha) * g.rr;
    mode = cfg.ring !== false && Math.hypot(dx - hx, dy - hy) < 18 ? 'twist1' : cfg.ring !== false && Math.abs(dist - g.rr) < 14 ? 'pinch1' : 'maybe';
    st = { x: ev.clientX, y: ev.clientY, grain: o.grain, pitch: o.pitch, smear: o.smear, level: o.level, cx: r.left + g.cx, cy: r.top + g.cy, g };
    cfg.onDown && cfg.onDown(ev);
    ht = setTimeout(() => { if (!moved && mode === 'maybe') { mode = 'hold'; cfg.onHold && cfg.onHold(); } }, 480);
  });
  elm.addEventListener('pointermove', ev => {
    if (!pts.has(ev.pointerId)) return; pts.set(ev.pointerId, [ev.clientX, ev.clientY]);
    if (cfg.cOnly && S.dir !== 'C') { if (st && Math.hypot(ev.clientX - st.x, ev.clientY - st.y) > 6) { moved = true; clearTimeout(ht); } return; }
    if (mode === 'two' && pts.size >= 2) { const [a, b] = [...pts.values()]; const d = Math.hypot(b[0] - a[0], b[1] - a[1]), da = Math.atan2(b[1] - a[1], b[0] - a[0]) - st.ang; o.grain = clamp(st.grain - Math.log(d / st.d) * 0.9, 0, 1); o.pitch = clamp(Math.round(st.pitch + da / (Math.PI / 10)), -7, 7); show(Math.abs(da) > 0.12 ? 'pitch' : 'grain'); upd(); return; }
    const dx = ev.clientX - st.x, dy = ev.clientY - st.y; if (Math.hypot(dx, dy) > 6) { moved = true; clearTimeout(ht); }
    if (mode === 'twist1') { o.pitch = clamp(Math.round((Math.atan2(ev.clientX - st.cx, -(ev.clientY - st.cy)) / (0.75 * Math.PI)) * 7), -7, 7); show('pitch'); upd(); }
    else if (mode === 'pinch1') { const dist = Math.hypot(ev.clientX - st.cx, ev.clientY - st.cy); o.grain = clamp((st.g.R1 - dist) / (st.g.R1 - st.g.R0), 0, 1); show('grain'); upd(); }
    else if (mode === 'maybe' && moved) mode = Math.abs(dy) > Math.abs(dx) ? 'swipe' : 'swipeX';
    if (mode === 'swipe') { o.smear = clamp(st.smear - dy / 160, 0, 1); show('smear'); upd(); }
    if (mode === 'swipeX') { o.level = clamp(st.level + dx / 220, 0, 1); show('level'); upd(); }
  });
  const end = ev => { pts.delete(ev.pointerId); if (pts.size) return; clearTimeout(ht); if (!moved && mode === 'maybe') cfg.onTap && cfg.onTap(ev); mode = null; setTimeout(() => bub.classList.remove('show'), 500); };
  elm.addEventListener('pointerup', end); elm.addEventListener('pointercancel', end);
  elm.addEventListener('wheel', ev => { if (cfg.cOnly && S.dir !== 'C') return; ev.preventDefault(); if (ev.shiftKey) { o.grain = clamp(o.grain - Math.sign(ev.deltaY) * 0.04, 0, 1); show('grain'); } else { o.pitch = clamp(o.pitch - Math.sign(ev.deltaY), -7, 7); show('pitch'); } upd(); clearTimeout(bub._t); bub._t = setTimeout(() => bub.classList.remove('show'), 700); }, { passive: false });
}
/* the on-object drawing: ring = grain size (cloud dots inside), dot on ring = pitch, trail = smear, line = ADSR, bar = level */
function objSVG(o, W, H, { ring = true, small = false, env = true, src = null } = {}) {
  let s = ''; if (W < 30 || H < 30) return s;
  if (ring) {
    const g = ringGeo(o, W, H, small);
    s += `<circle cx="${g.cx}" cy="${g.cy}" r="${g.rr}" fill="none" stroke="rgba(243,242,238,.85)" stroke-width="1.5"/>`;
    const n = Math.round(lerp(1, 16, o.grain)); for (let k = 0; k < n; k++) { const a = k * 2.39996, rad = g.rr * 0.75 * Math.sqrt((k + 0.5) / n); s += `<circle cx="${g.cx + Math.cos(a) * rad}" cy="${g.cy + Math.sin(a) * rad}" r="${lerp(4, 1.4, o.grain)}" fill="rgba(243,242,238,.7)"/>`; }
    const tl = o.smear * W * 0.3; for (let k = 0; k < 3; k++) { const yy = g.cy + (k - 1) * g.rr * 0.45; const x0 = g.cx + Math.sqrt(Math.max(0, g.rr * g.rr - (yy - g.cy) ** 2)); if (tl > 2) s += `<line x1="${x0 + 4}" x2="${x0 + 4 + tl * (1 - Math.abs(k - 1) * 0.35)}" y1="${yy}" y2="${yy}" stroke="rgba(243,242,238,${0.55 - Math.abs(k - 1) * 0.2})" stroke-width="1.5" stroke-linecap="round"/>`; }
    const ha = (o.pitch / 7) * 0.75 * Math.PI; const hx = g.cx + Math.sin(ha) * g.rr, hy = g.cy - Math.cos(ha) * g.rr;
    s += `<line x1="${g.cx}" y1="${g.cy - g.rr - 5}" x2="${g.cx}" y2="${g.cy - g.rr + 3}" stroke="rgba(243,242,238,.35)" stroke-width="1.5"/>`;
    s += `<circle cx="${hx}" cy="${hy}" r="${small ? 5 : 7}" fill="#f3f2ee" stroke="#0b0b0c" stroke-width="2"/>`;
    if (o.pitch) s += `<text x="${hx}" y="${hy - 11}" text-anchor="middle" font-size="${small ? 9 : 10.5}" fill="#f3f2ee" font-family="Geist Mono, monospace">${o.pitch > 0 ? '+' : ''}${o.pitch}</text>`;
  }
  if (env) {
    const p = 8, w = (small ? W * 0.55 : W) - 2 * p, top = small ? 10 : H - 30, h = small ? 14 : 20;
    const xA = p + w * 0.25 * Math.sqrt(o.a / 2), xD = xA + w * 0.25 * Math.sqrt(o.d / 2), xS = xD + w * 0.2, xR = xS + w * 0.3 * Math.sqrt(o.r / 3);
    const y = l => top + h * (1 - l);
    s += `<polyline points="${p},${y(0)} ${xA},${y(1)} ${xD},${y(o.s)} ${xS},${y(o.s)} ${xR},${y(0)}" fill="none" stroke="rgba(243,242,238,.9)" stroke-width="1.4" stroke-linejoin="round"/>`;
    if (o.cut !== 'fade') s += `<line x1="${p}" x2="${xR}" y1="${y(o.thresh)}" y2="${y(o.thresh)}" stroke="rgba(243,242,238,.35)" stroke-dasharray="2 2"/>`;
  }
  s += `<rect x="${W - 6}" y="${H * (1 - o.level) * 0.9 + 6}" width="2" height="${H * o.level * 0.9 - 12}" rx="1" fill="rgba(243,242,238,.5)"/>`;
  return s;
}
function drawEmCards() { if (S.dir !== 'C') return; EMITTERS.forEach(e => { if (!e.card) return; const r = e.card.getBoundingClientRect(); const svg = e.card.querySelector('svg'); svg.setAttribute('viewBox', `0 0 ${r.width} ${r.height}`); svg.innerHTML = objSVG(e, r.width, r.height); }); }
function buildEmCards() {
  const box = $('#em-cards'); if (!box) return; box.innerHTML = '';
  EMITTERS.forEach(e => {
    const c = el('div', 'emc'); c.id = `emc-${e.i + 1}`;
    c.innerHTML = `<i class="emc-bg"></i><svg></svg><span class="n">${e.i + 1}</span><button class="look no-g">${LOOKS[e.look]}</button>`;
    c.querySelector('.look').addEventListener('click', ev => { ev.stopPropagation(); e.look = (e.look + 1) % LOOKS.length; ev.target.textContent = LOOKS[e.look]; onParam(e, 'look'); });
    gesture(c, e, { onTap: () => select({ kind: 'emitter', ref: e }), onHold: () => { select({ kind: 'emitter', ref: e }); showMini(c); }, redraw: drawEmCards });
    e.card = c; box.appendChild(c);
  });
  refreshEmCards(); requestAnimationFrame(drawEmCards);
}
function refreshEmCards() { EMITTERS.forEach(e => { if (!e.card) return; const bg = e.card.querySelector('.emc-bg'); bgFrame(bg, GRID_COLS[e.col], ((e.y * 8) | 0) * 6 + 2, e.card.clientWidth / Math.max(1, e.card.clientHeight)); e.card.classList.toggle('sel', S.selEm === e.i); e.card.classList.toggle('on', e.on); }); }
function drawPadOverlays() {
  PADS.forEach(p => { if (!p || !p.ov) return; if (S.dir !== 'C') { p.ov.innerHTML = ''; return; } const W = p.el.clientWidth, H = p.el.clientHeight; p.ov.setAttribute('viewBox', `0 0 ${W} ${H}`); p.ov.innerHTML = objSVG(p, W, H, { ring: S.sel.ref === p, small: true, env: true }); });
}
function drawLaneOverlays() {
  LANES.forEach(l => { const s = l.row && l.row.querySelector('.lov'); if (!s) return; if (S.dir !== 'C') { s.innerHTML = ''; return; } const h = l.row.querySelector('.lane-head'); const W = h.clientWidth, H = h.clientHeight; s.setAttribute('viewBox', `0 0 ${W} ${H}`); s.innerHTML = objSVG(l, W, H, { ring: false, small: true }); });
}
/* contextual mini-strip: hangs off the selected object, never a panel */
function miniItems(kind, o) {
  const src = selSrc();
  const it = [];
  if (kind !== 'lane') it.push(['look', `<span class="mi-k">look</span><b>${LOOKS[o.look]}</b>`, () => { o.look = (o.look + 1) % LOOKS.length; onParam(o, 'look'); showMini(MINI.anchor); }]);
  if (kind === 'pad') it.push(['mode', `<span class="mi-k">mode</span><b>${MODE_GLYPH[o.mode]} ${o.mode}</b>`, () => { if (o.mode !== 'slice') { o.mode = { loop: 'grain', grain: 'chop', chop: 'loop' }[o.mode]; buildPads(); select({ kind: 'pad', ref: o }); showMini(o.el); } }]);
  if (kind === 'lane') it.push(['len', `<span class="mi-k">steps</span><b>${o.len}</b>`, () => openPop('len')], ['pattern', `<span class="mi-k">pattern</span><b>${laneSlot(o) || '–'}</b>`, () => { const sc = SCENES[S.scene]; if (sc) { sc[o.i] = sc[o.i] === 'A' ? 'B' : 'A'; renderLanes(); renderSong(); showMini(MINI.anchor); } }], ['mute', `<b>${o.mute ? 'muted' : 'mute'}</b>`, () => { o.mute = !o.mute; renderLanes(); showMini(MINI.anchor); }]);
  it.push(['shape', `<span class="mi-env"></span><b>shape</b>`, () => openPop('shape')]);
  if (src) it.push(['chop', `<b>chop</b>`, () => openChop(src, MINI.anchor)]);
  if (kind !== 'lane') it.push(['grain', '<b>grain</b>', () => openPop('grain')]);
  it.push(['pitch', '<b>pitch</b>', () => openPop('pitch')], ['fx', '<b>fx</b>', () => openPop('fx')], ['link', '<b>links</b>', () => openPop('link')]);
  return it;
}
const MINI = { anchor: null };
function showMini(anchor) {
  hideMini(false); const o = S.sel.ref, kind = S.sel.kind; MINI.anchor = anchor;
  const m = el('div', 'mini'); m.id = 'mini';
  miniItems(kind, o).forEach(([k, html, fn]) => { const b = el('button', 'mi', html); b.dataset.k = k; b.addEventListener('click', ev => { ev.stopPropagation(); fn(); }); m.appendChild(b); });
  const lay = layoutSeg('mini-lay'); m.appendChild(lay);
  $('#app').appendChild(m);
  const env = m.querySelector('.mi-env'); if (env) { const w = adsrWidget(o, { mini: true }); env.appendChild(w.el); }
  const ar = anchor.getBoundingClientRect(), mw = m.offsetWidth, mh = m.offsetHeight;
  let left = clamp(ar.left + ar.width / 2 - mw / 2, 10, innerWidth - mw - 10), top = ar.bottom + 8; if (top + mh > innerHeight - 8) top = ar.top - mh - 8;
  m.style.left = `${left}px`; m.style.top = `${top}px`; MINI.el = m; liveDraw();
}
function hideMini(alsoPop = true) { $$('#mini, .cpop').forEach(e => e.remove()); }
function openPop(type) {
  $$('.cpop').forEach(e => e.remove()); const o = S.sel.ref, src = selSrc();
  const p = el('div', 'cpop pop-' + type); p.addEventListener('pointerdown', ev => ev.stopPropagation());
  const row = keys => { const r = el('div', 'k-row'); keys.forEach(k => r.appendChild(bindK(o, k, { cls: 'sm' }).el)); return r; };
  if (type === 'shape') { p.append(adsrWidget(o, { src: () => src, frame: () => selInfo().f }).el); const r = el('div', 'row'); r.append(cutFade(o), bindK(o, 'thresh', { cls: 'sm' }).el, layoutSeg('')); p.append(r); }
  if (type === 'grain') p.append(row(['density', 'length', 'spray', 'jitter']), row(['scan', 'reverse', 'spread', 'seed']));
  if (type === 'pitch') p.append(row(['pitch', 'fine', 'octave', 'pjit', 'glide']));
  if (type === 'fx') p.append(fxPills(o));
  if (type === 'link') p.append(linkPills(o));
  if (type === 'len') p.append(row(['len', 'swing', 'chance']));
  $('#app').appendChild(p);
  const mr = MINI.el.getBoundingClientRect(), b = MINI.el.querySelector(`[data-k="${type}"]`)?.getBoundingClientRect() || mr; const pw = p.offsetWidth, ph = p.offsetHeight;
  let left = clamp(b.left + b.width / 2 - pw / 2, 10, innerWidth - pw - 10), top = mr.top - ph - 8; if (top < 60) top = mr.bottom + 8;
  p.style.left = `${left}px`; p.style.top = `${top}px`; p.style.setProperty('--ax', `${clamp(b.left + b.width / 2 - left, 18, pw - 18)}px`); p.classList.toggle('below', top > mr.top);
  MINI.el.querySelectorAll('.mi').forEach(x => x.classList.toggle('on', x.dataset.k === type)); liveDraw();
}
document.addEventListener('pointerdown', ev => { if (S.dir === 'C' && !ev.target.closest('#mini, .cpop, .sheet, .emc, .pad, .lane-head, .tb-pop, .chip')) hideMini(); });
/* C · globals: the top-bar chips are the objects */
function topPop(kind, anchor) {
  $$('.tb-pop').forEach(e => e.remove());
  const p = el('div', 'cpop tb-pop tb-' + kind); p.addEventListener('pointerdown', ev => ev.stopPropagation());
  if (kind === 'tempo') { const r = el('div', 'k-row'); r.append(pknob(TEMPO_DEF, () => S.bpm, setBpm).el, bindK(PROJ, 'swing', { cls: 'sm' }).el); const tap = el('button', 'btn', 'Tap'); tap.addEventListener('click', () => toast('tap tempo')); r.appendChild(tap); p.append(r); }
  if (kind === 'key') { const g = el('div', 'keys'); NOTE.forEach(n => { const b = el('button', 'kb' + (n === key().root.replace('♯', '♯') ? ' on' : ''), n); g.appendChild(b); }); const sc = el('div', 'seg'); Object.keys(SCALES).forEach(s => { const b = el('button', s === key().scale ? 'on' : '', s); sc.appendChild(b); }); p.append(g, sc); }
  if (kind === 'project') { p.append(el('div', 'cap', 'Library'), libraryRow()); const r = el('div', 'row'); r.append(tileBtnSm('Script', () => { p.appendChild(scriptBlock()); }), tileBtnSm('Master fx', () => { p.appendChild(fxPills(PROJ)); }), tileBtnSm('Export', () => toast('Export · mp4 + wav'))); p.append(r); }
  $('#app').appendChild(p);
  const ar = anchor.getBoundingClientRect(); const pw = p.offsetWidth; const left = clamp(ar.left + ar.width / 2 - pw / 2, 10, innerWidth - pw - 10);
  p.style.left = `${left}px`; p.style.top = `${ar.bottom + 10}px`; p.classList.add('below'); p.style.setProperty('--ax', `${clamp(ar.left + ar.width / 2 - left, 18, pw - 18)}px`);
  liveDraw();
}
document.addEventListener('pointerdown', ev => { if (!ev.target.closest('.tb-pop, .chip, #btn-deep')) $$('.tb-pop').forEach(e => e.remove()); });

/* ═════ direction plumbing ═════ */
function layoutSwitch() { const w = el('div', 'lay-sw'); w.appendChild(layoutSeg('')); return w; }
function attachPad(d, p) {
  gesture(d, p, {
    small: true, ring: false, cOnly: true,
    onDown: ev => { if (S.dir === 'B' && ev.target.closest('.edit')) return; tapPad(p); select({ kind: 'pad', ref: p }); if (S.dir === 'C') drawPadOverlays(); },
    onTap: ev => { if (S.dir === 'B' && ev.target.closest('.edit')) openClip({ kind: 'pad', ref: p }); },
    onHold: () => { if (S.dir === 'A') { S.page.pad = 'SOURCE'; renderStrip(); } else if (S.dir === 'B') openClip({ kind: 'pad', ref: p }); else showMini(d); },
    redraw: drawPadOverlays,
  });
}
function laneHeadTap(l) { if (S.dir === 'B') openClip({ kind: 'lane', ref: l }); else { select({ kind: 'lane', ref: l }); if (S.dir === 'C') showMini(l.row.querySelector('.lane-head')); } }
function laneHeadHold(l) { select({ kind: 'lane', ref: l }); if (S.dir === 'B') openClip({ kind: 'lane', ref: l }); else openChop(l.src, l.row.querySelector('.lane-head')); }
function columnHold(col) { const id = GRID_COLS[col]; if (!id) return; if (S.dir === 'B') { const e = EMITTERS.find(e => e.col === col) || EMITTERS[S.selEm]; openClip({ kind: 'emitter', ref: e }); } else openChop(id, cells[col]); }
function dirOnSelect() { if (S.dir === 'A') renderStrip(); if (S.dir === 'C') { refreshEmCards(); drawPadOverlays(); } }
function dirFrame(t) {
  TICKERS.forEach(w => (w.el.isConnected ? w.tick(t) : TICKERS.delete(w)));
  if (S.dir === 'C' && S.view === 'play') EMITTERS.forEach(e => e.card && e.card.classList.toggle('on', e.on));
}
function projectAction(anchor) { if (S.dir === 'A') select({ kind: 'project', ref: PROJ }); else if (S.dir === 'B') openProject(); else topPop('project', anchor); }
function setDir(d) {
  S.dir = d; $('#app').dataset.dir = d; closeSheets(); closeClip(); hideMini(); $$('.tb-pop').forEach(e => e.remove());
  $$('#dirsw button').forEach(b => b.classList.toggle('on', b.dataset.dir === d));
  if (d === 'C') buildEmCards();
  if (d === 'A') { if (S.view === 'seq' && S.sel.kind !== 'lane') select({ kind: 'lane', ref: LANES[0] }); else if (S.view === 'perform' && S.sel.kind !== 'pad') select({ kind: 'pad', ref: PADS.find(p => p && p.on) || PADS[0] }); else renderStrip(); }
  requestAnimationFrame(() => { layoutGrid(); if (S.view === 'seq') layoutLanes(); PADS.forEach(p => p && p.img && bgFrame(p.img, p.src, padFrame(p), p.el.clientWidth / Math.max(1, p.el.clientHeight))); drawPadOverlays(); drawLaneOverlays(); drawEmCards(); refreshEmCards(); liveDraw(); });
  const u = new URL(location.href); u.searchParams.set('dir', d); history.replaceState(null, '', u);
}


/* column headers above the Play grid — the clips themselves (tap: its params · hold: chop · "+": add) */
function clipSel(id) { return { kind: 'clip', ref: ensureP(SRC[id]) }; }
function buildHeads() {
  const hs = $('#vheads'); if (!hs) return; hs.innerHTML = '';
  GRID_COLS.forEach((id, c) => {
    const b = el('button', 'vh' + (id ? '' : ' empty'), id ? `<span>${SRC[id].name}</span>` : '+'); b.id = `vh-${c + 1}`;
    let ht, held = false;
    b.addEventListener('pointerdown', () => { held = false; ht = setTimeout(() => { held = true; if (id) { if (S.dir === 'B') openClip(clipSel(id)); else openChop(id, b); } }, 520); });
    b.addEventListener('pointerup', () => { clearTimeout(ht); if (held) return; if (!id) { openAdd(DIGGING.size ? 'dig' : 'files'); return; } headTap(id, b); });
    b.addEventListener('pointerleave', () => clearTimeout(ht));
    hs.appendChild(b);
  });
  markSelection();
}
function headTap(id, b) { if (S.dir === 'B') openClip(clipSel(id)); else { select(clipSel(id)); if (S.dir === 'A') { S.page.clip = 'SOURCE'; renderStrip(); } else showMini(b); } }
function emitterHold(e) { select({ kind: 'emitter', ref: e }); if (S.dir === 'B') openClip(S.sel); else if (S.dir === 'A') { S.page.emitter = 'SHAPE'; renderStrip(); } else showMini(e.card || e.btn); }
function markerRing(e) { const rr = lerp(30, 12, e.grain), ha = (e.pitch / 7) * 0.75 * Math.PI; return `<circle cx="40" cy="40" r="${rr}" fill="none" stroke="rgba(243,242,238,.8)" stroke-width="1.3"/><circle cx="${40 + Math.sin(ha) * rr}" cy="${40 - Math.cos(ha) * rr}" r="3.8" fill="#f3f2ee" stroke="#0b0b0c" stroke-width="1.2"/>`; }

/* ───────────── boot ───────────── */
function boot() {
  stages.play = new Stage($('#cv-play')); stages.seq = new Stage($('#cv-seq')); stages.perf = new Stage($('#cv-perf'));
  buildGrid(); buildEmitterButtons(); $$('.knob').forEach(makeKnob);
  buildLanes(); renderSong(); buildPads();
  ['#play-stage', '#seq-stage', '#perf-stage'].forEach(s => $(s).appendChild(layoutSwitch()));
  setDir(['A', 'B', 'C'].includes(params.get('dir')) ? params.get('dir') : 'A');
  if (params.get('layout') === 'tile') setLayout('tile');
  const v0 = (location.hash || '').slice(1); setView(['play', 'seq', 'perform'].includes(params.get('view')) ? params.get('view') : ['play', 'seq', 'perform'].includes(v0) ? v0 : 'play');
  ['click', 'pointerdown', 'keydown'].forEach(e => document.addEventListener(e, () => { initAudio(); A.ctx?.resume?.(); }, { once: true }));
  if (OUT_MODE) bootOut();
  if (DEMO) bootDemo();
  requestAnimationFrame(frame);
}
function bootOut() {
  document.body.classList.add('out-mode'); setView('perform');
  const st = document.createElement('style');
  st.textContent = `.topbar,.pads-cell,.perf-bar{display:none!important}#app{grid-template-rows:0 1fr}.view-perform{padding:0;grid-template-columns:1fr;grid-template-rows:1fr}.stage.perf{border-radius:0;box-shadow:none}`;
  document.head.appendChild(st);
  document.title = 'chromagrain · output';
  bc && (bc.onmessage = ({ data }) => {
    if (data.t === 'state') { S.bpm = data.bpm; data.pads.forEach((on, k) => { const p = PADS[k]; if (p && p.on !== on) { p.on = on; p.t0 = now(); if (on) launchFlash(p); } }); }
    if (data.t === 'tick') { S.step = data.g; PADS.forEach(p => p && p.on && p.mode === 'chop' && data.g % 2 === 0 && ((p.chopF = (rnd(16) | 0) * 3), (p.chopT = now()))); }
  });
  document.addEventListener('dblclick', toggleFS);
}

/* ───────────── 12 · demo (for screenshots / kiosk): ?demo&view=play|seq|perform ───────────── */
function bootDemo() {
  setPlaying(true);
  const v = params.get('view');
  if ((v === 'perform' || params.has('pads')) && !params.has('slices')) { (params.get('pads') || '0,4,9').split(',').map(Number).forEach(k => { PADS[k].on = true; PADS[k].t0 = now() - k * 0.01; }); renderPads(); }
  if (params.has('d2')) $('#seg-out button[data-out="d2"]').classList.add('on'), $('#seg-out button[data-out="here"]').classList.remove('on'), $('#view-perform').classList.add('d2');
  const sel = params.get('sel');
  setTimeout(() => {
    if (sel) { const [k, n] = sel.split(':'); const i = (+n || 1) - 1; select(k === 'pad' ? { kind: 'pad', ref: PADS[i] } : k === 'lane' ? { kind: 'lane', ref: LANES[i] } : k === 'clip' ? { kind: 'clip', ref: SRC[n] } : k === 'project' ? { kind: 'project', ref: PROJ } : { kind: 'emitter', ref: EMITTERS[i] }); }
    if (params.get('page')) { S.page[S.sel.kind] = params.get('page'); renderStrip(); }
    if (params.has('clip')) openClip(S.sel);
    if (params.has('proj')) openProject();
    const sh = params.get('sheet');
    if (sh === 'add') openAdd(); if (sh === 'dig') { openAdd('dig'); setTimeout(() => digOne($('#dig-res')), 200); setTimeout(() => digOne($('#dig-res')), 500); }
    if (sh === 'chop') openChop(selSrc(), S.sel.kind === 'pad' ? S.sel.ref.el : S.sel.kind === 'lane' ? S.sel.ref.row.querySelector('.lane-head') : $('#vheads').children[S.sel.ref.col || 0]);
    if (sh === 'tapchop') { openChop(selSrc()); setTimeout(() => { const b = $('.chop-sheet [data-m="tap"]'); b && b.click(); [500, 1100, 1500, 2100, 2500].forEach(d => setTimeout(() => $('.chop-sheet .tapbtn')?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })), d)); }, 200); }
    const mi = params.get('mini');
    if (mi) { const anchor = S.sel.kind === 'pad' ? S.sel.ref.el : S.sel.kind === 'lane' ? S.sel.ref.row.querySelector('.lane-head') : S.sel.ref.card; showMini(anchor); if (mi !== '1') openPop(mi); }
    const tp = params.get('topop'); if (tp) topPop(tp, tp === 'tempo' ? $('#chip-tempo') : tp === 'key' ? $('#chip-key') : $('#btn-deep'));
    if (params.has('slices')) slicesToPads(params.get('slices') || 'betty');
  }, 500);
  if (params.has('slicesplay')) setInterval(() => { [0, 2, 5].forEach((k, n) => setTimeout(() => PADS[k] && tapPad(PADS[k]), n * 180)); }, 700);
  if (params.has('wheel')) setInterval(() => { const c = EMITTERS[params.get('wheel') - 1 || 1].card; c && c.dispatchEvent(new WheelEvent('wheel', { deltaY: EMITTERS[1].pitch >= 5 ? 100 : -100, cancelable: true })); }, 350);
  if (params.has('pop')) setTimeout(() => { const l = LANES[1]; openHold(l, 5, l.cells[5]); }, 600);
  // a ghost performer moving two emitters around the grid
  let t0 = now();
  const E = [EMITTERS[0], EMITTERS[1]];
  E.forEach(e => { e.on = true; e.envT0 = now(); e.relT = null; });
  setInterval(() => {
    const t = now() - t0;
    E[0].y = 0.5 + 0.38 * Math.sin(t * 0.7); E[0].col = 0; E[0].lastCol = 0;
    E[1].y = 0.5 + 0.3 * Math.sin(t * 0.43 + 1); E[1].col = 2;
    placeEmitters();
  }, 50);
}

boot();
})();
