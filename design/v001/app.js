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
const SRC = Object.fromEntries(SOURCES.map((s, i) => [s.id, Object.assign(s, { idx: i })]));
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
SOURCES.forEach(s => {
  const img = new Image(); img.src = `media/${s.id}_atlas.jpg`;
  const v = document.createElement('video');
  Object.assign(v, { src: `media/${s.id}.mp4`, muted: true, loop: true, playsInline: true, autoplay: true, preload: 'auto', poster: `media/${s.id}_poster.jpg` });
  v.setAttribute('muted', ''); v.setAttribute('playsinline', '');
  v.playbackRate = 1;
  $('#media-bin').appendChild(v);
  v.play().catch(() => {});
  media[s.id] = { img, v, buf: null };
});
const atlasURL = id => `url(media/${id}_atlas.jpg)`;
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
      fetch(`media/${s.id}.mp4`).then(r => r.arrayBuffer()).then(b => ctx.decodeAudioData(b)).then(buf => {
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
    PADS.forEach(p => { if (p && p.q) { p.q = false; p.on = !p.on; p.t0 = now(); changed = true; if (p.on) launchFlash(p); } });
    if (changed) { limitPads(); renderPads(); broadcast(); }
  }
  PADS.forEach(p => p && p.on && padStep(p, g));
  if (S.recording && inBar === 0) {}
  broadcast({ t: 'tick', g });
}

/* ───────────── 5 · renderer (canvas2D; maps to a wgpu quad pass in Rust) ───────────── */
const world = { grains: [], hits: [] };
const off = document.createElement('canvas'); const offx = off.getContext('2d');
class Stage {
  constructor(canvas) { this.c = canvas; this.x = canvas.getContext('2d', { alpha: false }); this.w = 0; this.h = 0; this.fade = 0.2; this.baseA = 0.9; new ResizeObserver(() => this.resize()).observe(canvas); this.resize(); }
  resize() { const r = this.c.getBoundingClientRect(); const dpr = Math.min(devicePixelRatio || 1, 2); const w = Math.max(2, Math.round(Math.min(r.width * dpr, 1920))), h = Math.max(2, Math.round(Math.min(r.height * dpr, 1080))); if (w !== this.w || h !== this.h) { this.c.width = this.w = w; this.c.height = this.h = h; this.x.fillStyle = '#000'; this.x.fillRect(0, 0, w, h); } }
  visible() { return this.c.offsetParent !== null || document.fullscreenElement?.contains(this.c); }
}
function drawLook(x, src, sx, sy, sw, sh, dx, dy, dw, dh, look, alpha = 1) {
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
/* grains: a patch of picture with the same life as its sound */
function spawnGrain(o) { world.grains.push(Object.assign({ t0: now() }, o)); if (world.grains.length > 900) world.grains.splice(0, world.grains.length - 900); }
function drawGrains(st, t, filter) {
  const x = st.x, W = st.w, H = st.h;
  for (const g of world.grains) {
    if (filter && !filter(g)) continue;
    const ph = (t - g.t0) / g.dur; if (ph < 0 || ph > 1) continue;
    const env = Math.sin(Math.PI * ph);
    const img = media[g.src].img; if (!img.complete || !img.naturalWidth) continue;
    const [c, r] = frameXY(g.f + ph * g.dur * ATLAS.fps * (g.rev ? -1 : 1));
    const fx = c * ATLAS.fw, fy = r * ATLAS.fh;
    const dx = g.x * W, dy = g.y * H, dw = g.w * W, dh = g.h * H;
    const sx = fx + clamp(g.u, 0, 1 - g.w) * ATLAS.fw, sy = fy + clamp(g.v, 0, 1 - g.h) * ATLAS.fh;
    drawLook(x, img, sx, sy, g.w * ATLAS.fw, g.h * ATLAS.fh, dx - dw / 2, dy - dh / 2, dw, dh, g.look || 0, env * (g.a ?? 1));
    if (g.pitch) { x.save(); x.globalCompositeOperation = 'color'; x.globalAlpha = env * Math.min(0.32, Math.abs(g.pitch) / 14); x.fillStyle = `hsl(${(30 + g.pitch * 22 + 360) % 360},60%,55%)`; x.fillRect(dx - dw / 2, dy - dh / 2, dw, dh); x.restore(); }
  }
}
function drawHits(st, t) {
  const x = st.x, W = st.w, H = st.h;
  for (const h of world.hits) {
    const age = t - h.t0; if (age < 0 || age > h.dur) continue;
    const ph = age / h.dur; const env = age < 0.015 ? age / 0.015 : ph < 0.55 ? 1 : Math.pow(1 - (ph - 0.55) / 0.45, 1.6);
    const img = media[h.src].img; if (!img.complete) continue;
    const [c, r] = frameXY(h.f + age * ATLAS.fps);
    const fx = c * ATLAS.fw, fy = r * ATLAS.fh;
    if (h.mode === 'band') {
      drawLook(x, img, fx, fy + h.by * ATLAS.fh, ATLAS.fw, h.bh * ATLAS.fh, 0, h.by * H, W, h.bh * H, h.look || 0, env);
    } else {
      x.save(); if (h.blend) x.globalCompositeOperation = h.blend;
      drawLook(x, img, fx, fy, ATLAS.fw, ATLAS.fh, 0, 0, W, H, h.look || 0, env * 0.92); x.restore();
    }
    if (h.pitch) { x.save(); x.globalCompositeOperation = 'color'; x.globalAlpha = env * Math.min(0.3, Math.abs(h.pitch) / 14); x.fillStyle = `hsl(${(30 + h.pitch * 22 + 360) % 360},60%,55%)`; if (h.mode === 'band') x.fillRect(0, h.by * H, W, h.bh * H); else x.fillRect(0, 0, W, H); x.restore(); }
  }
}
function prune(t) {
  if (world.grains.length && world.grains[0].t0 + 2 < t) world.grains = world.grains.filter(g => t - g.t0 < g.dur + 0.05);
  if (world.hits.length > 40 || (world.hits.length && t - world.hits[0].t0 > 4)) world.hits = world.hits.filter(h => t - h.t0 < h.dur);
}

/* ───────────── 6 · PLAY — the video grid + emitters ───────────── */
const vgrid = $('#vgrid');
const cells = [];
function buildGrid() {
  vgrid.innerHTML = '';
  for (let r = 0; r < 8; r++) for (let c = 0; c < 8; c++) {
    const id = GRID_COLS[c]; const d = el('div', 'vcell'); d.dataset.c = c; d.dataset.r = r;
    if (!id) { d.classList.add('empty'); if (r === 0) { d.classList.add('first'); d.textContent = '+'; } }
    vgrid.appendChild(d); cells[r * 8 + c] = d;
  }
  EMITTERS.forEach(e => { const m = el('div', 'emk', `<i class="ring"></i><i class="ring"></i><i class="ring"></i><b>${e.i + 1}</b>`); vgrid.appendChild(m); e.mk = m; });
  layoutGrid();
}
function layoutGrid() {
  const c0 = cells[0]; if (!c0) return; const r = c0.getBoundingClientRect(); const a = r.width / Math.max(1, r.height) || 1;
  cells.forEach((d, k) => { const id = GRID_COLS[k % 8]; if (id) bgFrame(d, id, ((k / 8) | 0) * 6 + 2, a); });
  placeEmitters();
}
function gridGeom() { const r = vgrid.getBoundingClientRect(); return { r, cw: (r.width + 4) / 8 }; }
function placeEmitters() {
  const { r, cw } = gridGeom();
  EMITTERS.forEach(e => { e.mk.style.left = `${e.col * cw + (cw - 4) / 2}px`; e.mk.style.top = `${e.y * r.height}px`; e.mk.classList.toggle('sel', e.i === S.selEm); e.mk.classList.toggle('on', e.on); });
  cells.forEach((d, k) => d.classList.toggle('col-on', EMITTERS.some(e => e.on && e.col === k % 8)));
}
const touches = new Map();
vgrid.addEventListener('pointerdown', ev => {
  initAudio(); A.ctx?.resume?.();
  const { r, cw } = gridGeom(); const px = ev.clientX - r.left, py = ev.clientY - r.top;
  const col = clamp(Math.floor(px / cw), 0, 7);
  if (!GRID_COLS[col]) { toast('Drop a video here to add a column'); return; }
  // grab an emitter under the finger, otherwise the selected brush jumps here
  let em = EMITTERS.find(e => !e.on && Math.hypot(e.col * cw + cw / 2 - px, e.y * r.height - py) < 30);
  if (!em) { em = EMITTERS[S.selEm]; if (em.on) em = EMITTERS.find(e => !e.on) || em; }
  selectEmitter(em.i);
  vgrid.setPointerCapture(ev.pointerId);
  touches.set(ev.pointerId, em);
  moveEm(em, px, py); em.on = true; em.mk.classList.add('dragging'); placeEmitters();
});
vgrid.addEventListener('pointermove', ev => { const em = touches.get(ev.pointerId); if (!em) return; const { r } = gridGeom(); moveEm(em, ev.clientX - r.left, ev.clientY - r.top); placeEmitters(); });
const endTouch = ev => { const em = touches.get(ev.pointerId); if (!em) return; touches.delete(ev.pointerId); em.on = false; em.mk.classList.remove('dragging'); placeEmitters(); refreshEmitterButtons(); };
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
    spawnGrain({ src: id, f: pos * 47, x: cx, y: cy, w, h, u: cx - w / 2, v: cy - h / 2, dur, look: em.look, pitch: em.pitch, a: g < 0.3 ? 0.95 : 0.9, em: em.i });
    audioGrain(id, pos, dur * 0.8, em.pitch, pan, lerp(0.5, 0.18, g), em.smear, Math.round(pos * 7));
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
    let ht; b.addEventListener('pointerdown', () => { ht = setTimeout(() => openDeep('emitter'), 520); });
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
function selectEmitter(i) { S.selEm = i; refreshEmitterButtons(); placeEmitters(); knobs.forEach(k => k.draw()); }

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
    const head = el('button', 'lane-head', `<span class="sw" style="background:${SRC[l.src].sw}"></span><span class="len">${l.len === 16 ? '' : l.len}</span>`);
    head.style.backgroundImage = `url(media/${l.src}_poster.jpg)`;
    head.title = `${SRC[l.src].name} · tap to mute, hold for lane page`;
    let ht, held = false;
    head.addEventListener('pointerdown', () => { held = false; ht = setTimeout(() => { held = true; openDeep('lane', l); }, 520); });
    head.addEventListener('pointerup', () => { clearTimeout(ht); if (!held) { l.mute = !l.mute; row.classList.toggle('muted', l.mute); } });
    head.addEventListener('pointerleave', () => clearTimeout(ht));
    const steps = el('div', 'steps');
    for (let i = 0; i < 16; i++) { const s = el('div', 'st', '<i class="flash"></i>'); s.dataset.i = i; if (i % 4 === 0) s.classList.add('beat'); bindStep(s, l, i); steps.appendChild(s); }
    const end = el('span', 'lane-end', '↺'); steps.appendChild(end);
    row.append(head, steps); lanesEl.appendChild(row);
    l.row = row; l.cells = $$('.st', steps); l.end = end;
  });
  // the 8th lane: an empty slot that takes a dropped video (same as the Play grid's 8th column)
  const add = el('div', 'lane add'); const ah = el('button', 'lane-head', '+'); ah.addEventListener('click', () => toast('Drop a video to add a lane'));
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
    for (let k = 0; k < 14; k++) { const w = rnd(0.06, 0.2), cx = rnd(0.1, 0.9), cy = rnd(0.1, 0.9); spawnGrain({ src: l.src, f: f + rnd(-1, 1), x: cx, y: cy, w, h: w * rnd(0.8, 1.2), u: cx - w / 2, v: cy - w / 2, dur: rnd(0.12, 0.35), pitch: st.pitch, a: 1, lane: l.i }); }
  } else {
    const by = rnd(0, 0.6), bh = rnd(0.15, 0.4);
    world.hits.push({ src: l.src, f, t0: t, dur, mode: l.mode, by, bh, pitch: st.pitch, lane: l.i, blend: l.i === 4 ? 'lighten' : null });
  }
  audioGrain(l.src, st.slice / 16, Math.min(dur, 0.6), st.pitch, (l.i - 2.5) / 3, l.mode === 'cloud' ? 0.3 : 0.5, 0.22, [0, 4, 2, 7, -7, 9][l.i]);
  if (l.mode === 'cloud') for (let k = 0; k < 3; k++) setTimeout(() => audioGrain(l.src, st.slice / 16 + k * 0.01, 0.08, st.pitch + k * 2, rnd(-0.8, 0.8), 0.18, 0.4, [0, 4, 2, 7, -7, 9][l.i]), k * 40);
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
const MODE_GLYPH = { loop: '∞', grain: '⁘', chop: '⫶' };
function buildPads() {
  padsEl.innerHTML = '';
  PADS.forEach((p, k) => {
    const d = el('button', 'pad'); d.id = `pad-${k + 1}`;
    if (!p) { d.classList.add('empty'); d.innerHTML = '<span class="pl">+</span>'; padsEl.appendChild(d); return; }
    const look = LOOKS[p.look];
    d.innerHTML = `<i class="img"></i><span class="pl"><span>${SRC[p.src].name}</span><span class="m">${MODE_GLYPH[p.mode]}</span></span><i class="prog"></i>`;
    d.title = `${SRC[p.src].name} · ${p.mode}${look !== 'clean' ? ' · ' + look : ''}`;
    d.addEventListener('pointerdown', () => tapPad(p));
    p.el = d; p.img = d.querySelector('.img'); p.prog = d.querySelector('.prog');
    padsEl.appendChild(d);
  });
  requestAnimationFrame(() => { PADS.forEach(p => p && bgFrame(p.img, p.src, p.frame, p.el.clientWidth / Math.max(1, p.el.clientHeight))); });
  renderPads();
  // fullscreen HUD mini-grid
  const hp = $('#hud-pads'); hp.innerHTML = '';
  PADS.forEach(p => { const b = el('b'); if (p) { b.style.backgroundImage = atlasURL(p.src); const [c, r] = frameXY(p.frame); b.style.backgroundSize = '800% 600%'; b.style.backgroundPosition = `${c / 7 * 100}% ${r / 5 * 100}%`; b.addEventListener('pointerdown', ev => { ev.stopPropagation(); tapPad(p); }); p.hud = b; } hp.appendChild(b); });
}
function tapPad(p) {
  initAudio(); A.ctx?.resume?.();
  if (!S.playing) { setPlaying(true); p.on = !p.on; p.t0 = now(); if (p.on) launchFlash(p); limitPads(); renderPads(); broadcast(); return; }
  if (S.quant === 0) { p.on = !p.on; p.t0 = now(); if (p.on) launchFlash(p); limitPads(); } else p.q = !p.q;
  renderPads(); broadcast();
}
function limitPads() { const on = PADS.filter(p => p && p.on).sort((a, b) => b.t0 - a.t0); on.slice(4).forEach(p => (p.on = false)); }
function renderPads() {
  PADS.forEach(p => { if (!p) return; p.el.classList.toggle('on', p.on); p.el.classList.toggle('q', p.q); p.hud?.classList.toggle('on', p.on); });
  const names = PADS.filter(p => p && p.on).map(p => SRC[p.src].name); $('#hud-now').textContent = names.join(' · ');
}
function launchFlash(p) { world.hits.push({ src: p.src, f: rnd(40) | 0, t0: now(), dur: 0.35, mode: 'frame', look: p.look, perf: true }); }
function padStep(p, g) {
  const id = p.src;
  if (p.mode === 'loop' && g % 4 === 0) audioGrain(id, (media[id].v.currentTime || 0) / 6, stepDur() * 3.5, 0, 0, 0.35, 0.3, [0, 2, 4, 7][(g / 4) % 4 | 0]);
  if (p.mode === 'grain') for (let k = 0; k < 2; k++) { audioGrain(id, rnd(0.2, 0.6), 0.09, [0, 2, 4, 7, 9][rnd(5) | 0], rnd(-1, 1), 0.16, 0.6, 7); }
  if (p.mode === 'chop' && g % 2 === 0) { const sl = rnd(16) | 0; p.chopF = sl * 3; p.chopT = now(); audioGrain(id, sl / 16, stepDur() * 1.6, [0, 3, 5, 7][rnd(4) | 0], gauss() * 0.4, 0.4, 0.2, 0); }
}
function drawPerform(st, t) {
  const x = st.x, W = st.w, H = st.h;
  x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1;
  x.fillStyle = 'rgba(0,0,0,0.32)'; x.fillRect(0, 0, W, H);
  const on = PADS.filter(p => p && p.on).sort((a, b) => a.t0 - b.t0);
  on.forEach((p, k) => {
    const m = media[p.src]; const blend = k === 0 ? null : p.mode === 'loop' ? 'lighten' : null;
    x.save(); if (blend) x.globalCompositeOperation = blend;
    const fadeIn = clamp((t - p.t0) / 0.25, 0, 1);
    if (p.mode === 'loop') { const d = srcDims(m); if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, W, H, p.look, (k === 0 ? 1 : 0.85) * fadeIn); }
    else if (p.mode === 'chop') {
      if (m.img.complete && p.chopT != null) { const age = t - p.chopT; const [c, r] = frameXY(p.chopF + age * ATLAS.fps); const env = clamp(1 - age / (stepDur() * 1.8), 0, 1); drawLook(x, m.img, c * ATLAS.fw, r * ATLAS.fh, ATLAS.fw, ATLAS.fh, 0, 0, W, H, p.look, env * (k === 0 ? 1 : 0.85)); }
    }
    x.restore();
    if (p.mode === 'grain') {
      p.acc = (p.acc || 0) + 0.9;
      while (p.acc >= 1) { p.acc -= 1; const w = rnd(0.05, 0.22), cx = rnd(0.05, 0.95), cy = rnd(0.05, 0.95); spawnGrain({ src: p.src, f: rnd(48), x: cx, y: cy, w, h: w * rnd(0.6, 1.3), u: cx - w / 2, v: cy - w / 2, dur: rnd(0.2, 0.6), look: p.look, perf: true }); }
    }
  });
  drawGrains(st, t, g => g.perf);
  drawHits(st, t);
}

/* ───────────── frame loop ───────────── */
const stages = {};
let lastT = now();
function frame() {
  const t = now(), dt = Math.min(0.05, t - lastT); lastT = t;
  // emitters
  EMITTERS.forEach(e => e.on && emitFrom(e, dt));
  // PLAY canvas: base video of the active column + grains
  const sp = stages.play;
  if (sp && sp.visible()) {
    sp.resize(); const x = sp.x;
    const active = EMITTERS.find(e => e.on) || EMITTERS[S.selEm];
    const anyOn = EMITTERS.some(e => e.on);
    const smear = Math.max(...EMITTERS.filter(e => e.on).map(e => e.smear), anyOn ? 0 : active.smear);
    sp.baseA = lerp(sp.baseA, anyOn ? lerp(0.5, 0.12, smear) : lerp(0.9, 0.35, smear), 0.08);
    x.globalCompositeOperation = 'source-over';
    const id = GRID_COLS[active.lastCol];
    const d = id && srcDims(media[id]);
    if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, sp.w, sp.h, active.look, sp.baseA);
    else { x.fillStyle = `rgba(0,0,0,${sp.baseA})`; x.fillRect(0, 0, sp.w, sp.h); }
    drawGrains(sp, t, g => !g.perf && g.lane == null);
    drawGrains(sp, t, g => g.lane != null);
    drawHits(sp, t);
  }
  // SEQUENCE preview
  const ss = stages.seq;
  if (ss && ss.visible()) {
    ss.resize(); const x = ss.x; x.globalCompositeOperation = 'source-over'; x.globalAlpha = 1;
    x.fillStyle = 'rgba(0,0,0,0.12)'; x.fillRect(0, 0, ss.w, ss.h);
    if (!S.playing) { const d = srcDims(media[LANES[0].src]); if (d) drawLook(x, d[0], 0, 0, d[1], d[2], 0, 0, ss.w, ss.h, 0, 0.08); }
    drawHits(ss, t); drawGrains(ss, t, g => g.lane != null);
  }
  // PERFORM output (also drives Display 2 / fullscreen)
  const pf = stages.perf;
  if (pf && (pf.visible() || S.out === 'd2')) { pf.resize(); drawPerform(pf, t); }

  // grid cell lighting
  if (S.view === 'play') for (let k = 0; k < 64; k++) { const lit = t - litAt[k] < 0.12; if (cells[k] && cells[k].classList.contains('lit') !== lit) cells[k].classList.toggle('lit', lit); }
  // step flashes + per-lane playhead
  if (S.view === 'seq') LANES.forEach(l => l.cells.forEach((c, i) => {
    const hit = t - l.flashAt[i] < 0.09; if (c.classList.contains('hit') !== hit) c.classList.toggle('hit', hit);
    const ph = S.playing && i === l.pos; if (c.classList.contains('ph') !== ph) c.classList.toggle('ph', ph);
  }));
  if (S.view === 'seq' && S.playing) { const c = CHAIN[S.chainPos]; if (c?.el) { const frac = ((S.chainRep || 0) + ((S.step % 16) + 1) / 16) / c.r; c.el.querySelector('.fill').style.width = `${frac * 100}%`; } }
  // pads: animate thumbnails of playing pads, progress bars
  if (S.view === 'perform' || document.fullscreenElement) {
    const fi = Math.floor(t * 8);
    PADS.forEach(p => {
      if (!p) return;
      if (p.on && p._fi !== fi) { p._fi = fi; p.frame = (p.frame + 1) % 48; bgFrame(p.img, p.src, p.mode === 'chop' && p.chopF != null ? p.chopF : p.frame, p.el.clientWidth / Math.max(1, p.el.clientHeight)); }
      p.prog.style.width = p.on && S.playing ? `${(((S.step % 16) + 1) / 16) * 100}%` : '0';
    });
  }
  // readouts
  if (S.playing && S.step >= 0) {
    const bar = Math.floor(S.step / 16) + 1, beat = Math.floor((S.step % 16) / 4) + 1, six = (S.step % 4) + 1;
    $('#readout-pos .bar').textContent = String(bar).padStart(3, '0'); $('#readout-pos .beat').textContent = beat; $('#readout-pos .six').textContent = six;
    $('#hud-time').textContent = `${String(bar).padStart(3, '0')}.${beat}`;
    $$('#beat-dots i, #hud-beats i').forEach((d, k) => d.classList.toggle('on', (k % 4) === beat - 1));
  }
  if (S.recording) { const d = t - S.recT0; $('#rec-time').textContent = `${Math.floor(d / 60)}:${String(Math.floor(d % 60)).padStart(2, '0')}`; }
  prune(t);
  requestAnimationFrame(frame);
}

/* ───────────── 9 · deep drawer — everything that isn't playing ───────────── */
const DEEP = {
  links: [['pitch', 'hue', 1], ['gain', 'opacity', 1], ['pan', 'x position', 1], ['position', 'frame', 1], ['reverse', 'reverse', 1], ['envelope', 'fade', 0.8]],
  fx: [['crush', 1], ['delay', 1], ['reverb', 1], ['compress', 0], ['tint', 0], ['paulstretch', 0], ['filter', 0]],
};
let deepCtx = 'project', deepTab = 'links';
function slider(v, onv) {
  const s = el('div', 'dslider', '<i class="f"></i><i class="k"></i>'); const f = s.firstChild, k = s.lastChild;
  const set = vv => { v = clamp(vv, 0, 1); f.style.width = `${v * 100}%`; k.style.left = `${v * 100}%`; onv?.(v); }; set(v);
  s.addEventListener('pointerdown', ev => { s.setPointerCapture(ev.pointerId); const r = s.getBoundingClientRect(); set((ev.clientX - r.left) / r.width); });
  s.addEventListener('pointermove', ev => { if (!s.hasPointerCapture(ev.pointerId)) return; const r = s.getBoundingClientRect(); set((ev.clientX - r.left) / r.width); });
  return s;
}
function drow(label, v, fmt = x => x.toFixed(2)) { const r = el('div', 'drow', `<span class="dl">${label}</span>`); const vv = el('span', 'dv', fmt(v)); r.append(slider(v, x => (vv.textContent = fmt(x))), vv); return r; }
function renderDeep() {
  const b = $('#deep-body'); b.innerHTML = '';
  if (deepCtx.kind === 'lane') {
    const l = deepCtx.l; const s = el('div', 'dsec', `<span class="cap">Lane · ${SRC[l.src].name}</span>`);
    s.append(drow('<b>Length</b> (polymeter)', l.len / 16, x => `${Math.max(1, Math.round(x * 16))}`), drow('Swing', 0.12, x => `${Math.round(x * 100)}%`));
    const pats = el('div', 'fxchain'); 'ABCDEFGH'.split('').forEach(p => { const c = el('button', 'fx' + ((laneSlot(l) || 'A') === p ? ' on' : ''), `<i></i>${p}`); pats.appendChild(c); });
    s.append(el('div', 'cap', 'Pattern'), pats); b.appendChild(s);
  }
  if (deepCtx.kind === 'emitter') {
    const e = EMITTERS[S.selEm]; const s = el('div', 'dsec', `<span class="cap">Emitter ${e.i + 1}</span>`);
    s.append(drow('Density', e.grain), drow('Spray', 0.3), drow('Pitch jitter', 0.1), drow('Reverse', 0.0, x => `${Math.round(x * 100)}%`), drow('Scan speed', 0.5)); b.appendChild(s);
  }
  if (deepTab === 'links') {
    const s = el('div', 'dsec', '<span class="cap">Sound ↔ picture links</span>');
    DEEP.links.forEach(([a, v, x]) => s.appendChild(drow(`<b>${a}</b> → ${v}`, x))); b.appendChild(s);
  } else if (deepTab === 'fx') {
    const s = el('div', 'dsec', '<span class="cap">Effects · audio and video together</span>'); const ch = el('div', 'fxchain');
    DEEP.fx.forEach(f => { const c = el('button', 'fx' + (f[1] ? ' on' : ''), `<i></i>${f[0]}`); c.addEventListener('click', () => { f[1] = 1 - f[1]; c.classList.toggle('on'); }); ch.appendChild(c); });
    s.appendChild(ch); b.appendChild(s);
    const s2 = el('div', 'dsec', '<span class="cap">Crush</span>'); s2.append(drow('Audio amount', 0.6), drow('Video amount', 0.8)); b.appendChild(s2);
    const s3 = el('div', 'dsec', '<span class="cap">Filters</span>'); s3.append(drow('Low-pass', 0.85), drow('High-pass', 0.05), drow('Resonance', 0.2)); b.appendChild(s3);
  } else if (deepTab === 'auto') {
    const s = el('div', 'dsec', '<span class="cap">Automation · draw with a finger</span>');
    [['density', 'M0 60 C 60 60, 90 10, 160 20 S 260 70, 400 30'], ['pitch', 'M0 40 L 100 40 L 100 20 L 200 20 L 200 55 L 300 55 L 300 35 L 400 35'], ['level', 'M0 70 C 100 70, 140 15, 400 12']].forEach(([n, d]) => {
      const a = el('div', 'autolane', `<svg viewBox="0 0 400 80" preserveAspectRatio="none"><path d="${d}" fill="none" stroke="#f3f2ee" stroke-width="1.6" vector-effect="non-scaling-stroke"/></svg><span>${n}</span>`);
      const path = a.querySelector('path'); let pts = [];
      a.addEventListener('pointerdown', ev => { a.setPointerCapture(ev.pointerId); pts = []; });
      a.addEventListener('pointermove', ev => { if (!a.hasPointerCapture(ev.pointerId)) return; const r = a.getBoundingClientRect(); pts.push([((ev.clientX - r.left) / r.width) * 400, ((ev.clientY - r.top) / r.height) * 80]); pts.sort((p, q) => p[0] - q[0]); path.setAttribute('d', 'M' + pts.map(p => p.map(v => v.toFixed(1)).join(' ')).join(' L ')); });
      s.appendChild(a);
    }); b.appendChild(s);
  } else {
    const s = el('div', 'dsec', '<span class="cap">Script · rhai</span>');
    s.appendChild(el('div', 'script', `<span class="k">fn</span> on_step(lane, step) {\n  <span class="k">if</span> step % 4 == 0 {\n    lane.pitch = scale_degree(rand(0, 5));\n  }\n  lane.look = <span class="k">if</span> bar() % 8 == 7 { "kaleido" } <span class="k">else</span> { "clean" };\n}`));
    b.appendChild(s);
  }
}
function openDeep(kind, l) {
  deepCtx = { kind: kind || 'project', l };
  $('.deep-t').textContent = kind === 'lane' ? SRC[l.src].name : kind === 'emitter' ? `Emitter ${S.selEm + 1}` : 'Deep';
  $('.deep-s').textContent = kind === 'lane' ? 'lane page' : kind === 'emitter' ? 'everything this brush can do' : 'everything that isn’t playing';
  renderDeep(); $('#app').classList.add('deep-open');
}
const closeDeep = () => $('#app').classList.remove('deep-open');
$('#btn-deep').addEventListener('click', () => openDeep());
$('#btn-more-emitter').addEventListener('click', () => openDeep('emitter'));
$('#deep-close').addEventListener('click', closeDeep); $('#scrim').addEventListener('click', closeDeep);
$$('#deep-tabs button').forEach(b => b.addEventListener('click', () => { deepTab = b.dataset.dt; $$('#deep-tabs button').forEach(x => x.classList.toggle('on', x === b)); renderDeep(); }));

/* ───────────── 10 · chrome: views, transport, tempo, key, keys ───────────── */
function setView(v) {
  S.view = v; $('#app').dataset.view = v; closeHold();
  $$('.views button').forEach(b => b.setAttribute('aria-selected', b.dataset.view === v));
  const b = $(`.views button[data-view="${v}"]`); const th = $('.views-thumb');
  th.style.width = `${b.offsetWidth}px`; th.style.transform = `translateX(${b.offsetLeft - 3}px)`;
  requestAnimationFrame(() => { if (v === 'play') layoutGrid(); if (v === 'seq') { renderSong(); layoutLanes(); } if (v === 'perform') PADS.forEach(p => p && bgFrame(p.img, p.src, p.frame, p.el.clientWidth / Math.max(1, p.el.clientHeight))); });
  history.replaceState(null, '', `#${v}`);
}
$$('.views button').forEach(b => b.addEventListener('click', () => setView(b.dataset.view)));
$('#btn-play').addEventListener('click', () => setPlaying(!S.playing));
$('#btn-rec').addEventListener('click', () => setRecording(!S.recording));
$('#btn-rec-big').addEventListener('click', () => setRecording(!S.recording));
$('#btn-export').addEventListener('click', () => toast('Export · renders mp4 + wav offline, sample-identical to playback'));
(() => { // tempo: drag
  const c = $('#chip-tempo'); let y0, b0;
  c.addEventListener('pointerdown', ev => { c.setPointerCapture(ev.pointerId); y0 = ev.clientY; b0 = S.bpm; });
  c.addEventListener('pointermove', ev => { if (!c.hasPointerCapture(ev.pointerId)) return; S.bpm = clamp(Math.round(b0 + (y0 - ev.clientY) / 3), 60, 200); $('#tempo-val').textContent = S.bpm; $('#hud-bpm').textContent = S.bpm; if (A.dly) A.dly.delayTime.value = (60 / S.bpm) * 0.75; });
})();
$('#chip-key').addEventListener('click', () => { S.keyIdx = (S.keyIdx + 1) % KEYS.length; $('#key-root').textContent = key().root; $('#key-scale').textContent = key().scale; knobs.forEach(k => k.draw()); });
$$('#seg-quant button').forEach(b => b.addEventListener('click', () => { S.quant = +b.dataset.q; $$('#seg-quant button').forEach(x => x.classList.toggle('on', x === b)); }));
$('#btn-stopall').addEventListener('click', () => { PADS.forEach(p => p && ((p.on = false), (p.q = false))); renderPads(); broadcast(); });

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
  else if (k === 'escape') { closeHold(); closeDeep(); }
});
window.addEventListener('resize', () => { if (S.view === 'play') layoutGrid(); if (S.view === 'seq') layoutLanes(); setView(S.view); });

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

/* ───────────── boot ───────────── */
function boot() {
  stages.play = new Stage($('#cv-play')); stages.seq = new Stage($('#cv-seq')); stages.perf = new Stage($('#cv-perf'));
  buildGrid(); buildEmitterButtons(); $$('.knob').forEach(makeKnob);
  buildLanes(); renderSong(); buildPads();
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
  if (v === 'perform' || params.has('pads')) { [0, 4, 9].forEach(k => { PADS[k].on = true; PADS[k].t0 = now() - k * 0.01; }); renderPads(); }
  if (params.has('d2')) $('#seg-out button[data-out="d2"]').classList.add('on'), $('#seg-out button[data-out="here"]').classList.remove('on'), $('#view-perform').classList.add('d2');
  if (params.has('deep')) setTimeout(() => openDeep(params.get('deep') === 'lane' ? 'lane' : params.get('deep') === 'emitter' ? 'emitter' : undefined, LANES[0]), 300);
  if (params.has('pop')) setTimeout(() => { const l = LANES[1]; openHold(l, 5, l.cells[5]); }, 600);
  // a ghost performer moving two emitters around the grid
  let t0 = now();
  const E = [EMITTERS[0], EMITTERS[1]];
  E.forEach(e => (e.on = true));
  setInterval(() => {
    const t = now() - t0;
    E[0].y = 0.5 + 0.38 * Math.sin(t * 0.7); E[0].col = 0; E[0].lastCol = 0;
    E[1].y = 0.5 + 0.3 * Math.sin(t * 0.43 + 1); E[1].col = 2;
    placeEmitters();
  }, 50);
}

boot();
})();
