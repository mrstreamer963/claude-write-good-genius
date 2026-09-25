// Звук (§12.253). Живёт целиком в виде: ядро о нём не знает, как не знает о
// сутках (§12.46) — звук только называет происходящее, а не влияет на него.
//
// Каркас один, звуки — записи реестра `SOUNDS`. У каждой есть синтез (WebAudio,
// без файлов) и необязательный `file`: положи `assets/sfx/<имя>.ogg`, допиши
// `file` — и запись зазвучит сэмплом, а пока он грузится или не нашёлся, звучит
// синтез. Логику вызовов это не трогает.
//
// ⚠️ `AudioContext` заводится **на первом жесте** игрока: до него браузер звук не
// пускает, и контекст, созданный раньше, висит в `suspended`. До жеста `play()`
// молча ничего не делает.

const PREFS_KEY = "sp-sound";
const BUSES = ["sfx", "ui", "ambient"];

let ctx = null;
let master = null;
const bus = {};
let noiseBuf = null;
let ambient = null;
let ambientDuck = 1;

const prefs = loadPrefs();

function loadPrefs() {
  // `on` — вкл/выкл по шине, отдельно от громкости: выключенный фон не должен
  // терять выставленный игроком уровень.
  const base = {
    mute: false,
    master: 0.7,
    sfx: 0.8,
    ui: 0.6,
    ambient: 0.35,
    on: { sfx: true, ui: true, ambient: true },
  };
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) {
      const got = JSON.parse(raw);
      return { ...base, ...got, on: { ...base.on, ...(got.on ?? {}) } };
    }
  } catch {
    // хранилище недоступно — живём на умолчаниях
  }
  return base;
}

function savePrefs() {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // не сохранилось — не беда, настройка живёт до перезагрузки
  }
}

function applyGains() {
  if (!ctx) return;
  const t = ctx.currentTime;
  master.gain.setTargetAtTime(prefs.mute ? 0 : prefs.master, t, 0.03);
  for (const b of BUSES) {
    const lvl = prefs.on[b] ? prefs[b] : 0;
    const v = b === "ambient" ? lvl * ambientDuck : lvl;
    bus[b].gain.setTargetAtTime(v, t, b === "ambient" ? 0.4 : 0.03);
  }
}

function unlock() {
  if (!ctx) {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return;
    ctx = new AC();
    master = ctx.createGain();
    master.connect(ctx.destination);
    for (const b of BUSES) {
      bus[b] = ctx.createGain();
      bus[b].connect(master);
    }
    noiseBuf = makeNoise();
    applyGains();
    startAmbient();
    loadFiles();
  }
  if (ctx.state === "suspended") ctx.resume();
}
for (const ev of ["pointerdown", "keydown"]) {
  window.addEventListener(ev, unlock, { capture: true });
}

// --- настройки ---------------------------------------------------------------

export function soundPrefs() {
  return { ...prefs, on: { ...prefs.on } };
}

/// Включить или выключить одну шину (`sfx`, `ui`, `ambient`).
export function setBusOn(which, on) {
  prefs.on[which] = on;
  savePrefs();
  applyGains();
}

export function setMuted(on) {
  prefs.mute = on;
  savePrefs();
  applyGains();
}

export function setVolume(which, v) {
  prefs[which] = Math.max(0, Math.min(1, v));
  savePrefs();
  applyGains();
}

/// Пауза приглушает гул базы, но не выключает: тишина читалась бы поломкой.
export function setPaused(paused) {
  ambientDuck = paused ? 0.35 : 1;
  applyGains();
}

// --- синтез -----------------------------------------------------------------

function makeNoise() {
  const len = ctx.sampleRate;
  const buf = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
  return buf;
}

function env(g, t, a, peak, decay) {
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(peak, t + a);
  g.gain.exponentialRampToValueAtTime(0.0001, t + a + decay);
}

function tone(dest, t, { type = "sine", f, f2, a = 0.004, peak = 0.3, decay = 0.15 }) {
  const o = ctx.createOscillator();
  const g = ctx.createGain();
  o.type = type;
  o.frequency.setValueAtTime(f, t);
  if (f2) o.frequency.exponentialRampToValueAtTime(f2, t + a + decay);
  env(g, t, a, peak, decay);
  o.connect(g).connect(dest);
  o.start(t);
  o.stop(t + a + decay + 0.05);
}

function noise(dest, t, { filter = "bandpass", f, q = 1, f2, a = 0.003, peak = 0.3, decay = 0.1 }) {
  const s = ctx.createBufferSource();
  s.buffer = noiseBuf;
  const fl = ctx.createBiquadFilter();
  fl.type = filter;
  fl.frequency.setValueAtTime(f, t);
  if (f2) fl.frequency.exponentialRampToValueAtTime(f2, t + a + decay);
  fl.Q.value = q;
  const g = ctx.createGain();
  env(g, t, a, peak, decay);
  s.connect(fl).connect(g).connect(dest);
  s.start(t, Math.random() * 0.5);
  s.stop(t + a + decay + 0.05);
}

// Разброс высоты ±5 %: серия ударов одним тоном звучит автоматом.
const vary = (f) => f * (0.95 + Math.random() * 0.1);

const SOUNDS = {
  // Молоток: деревянный тук и короткий металлический щелчок.
  hammer: {
    bus: "sfx", max: 4, gap: 60,
    synth(d, t) {
      tone(d, t, { type: "triangle", f: vary(220), f2: 90, peak: 0.35, decay: 0.09 });
      noise(d, t, { f: vary(3200), q: 4, peak: 0.18, decay: 0.03 });
    },
  },
  // Лом: хруст — шум пониже и подлиннее, с опусканием фильтра.
  crowbar: {
    bus: "sfx", max: 4, gap: 60,
    synth(d, t) {
      noise(d, t, { f: vary(1400), f2: 400, q: 2, peak: 0.35, decay: 0.16 });
      tone(d, t, { type: "square", f: vary(140), f2: 60, peak: 0.08, decay: 0.07 });
    },
  },
  // Пыль достроенной клетки.
  puff: {
    bus: "sfx", max: 3, gap: 80,
    synth(d, t) {
      noise(d, t, { filter: "lowpass", f: 1800, f2: 300, a: 0.02, peak: 0.3, decay: 0.35 });
    },
  },
  click: {
    bus: "ui", max: 2, gap: 30,
    synth(d, t) {
      tone(d, t, { type: "square", f: 1800, peak: 0.08, a: 0.001, decay: 0.025 });
    },
  },
  // Отказ: кнопка погашена (`.off`) — глуше и ниже.
  deny: {
    bus: "ui", max: 1, gap: 80,
    synth(d, t) {
      tone(d, t, { type: "triangle", f: 180, f2: 140, peak: 0.15, decay: 0.08 });
    },
  },
  open: {
    bus: "ui", max: 1, gap: 60,
    synth(d, t) {
      tone(d, t, { type: "sine", f: 440, f2: 880, peak: 0.12, decay: 0.09 });
    },
  },
  close: {
    bus: "ui", max: 1, gap: 60,
    synth(d, t) {
      tone(d, t, { type: "sine", f: 760, f2: 380, peak: 0.1, decay: 0.08 });
    },
  },
  // Разметка чертежа: мягкий «чпок».
  blueprint: {
    bus: "ui", max: 2, gap: 40,
    synth(d, t) {
      tone(d, t, { type: "sine", f: 520, f2: 700, peak: 0.12, decay: 0.06 });
    },
  },
  news: {
    bus: "ui", max: 1, gap: 250,
    synth(d, t) {
      tone(d, t, { f: 988, peak: 0.12, decay: 0.25 });
      tone(d, t + 0.09, { f: 1319, peak: 0.12, decay: 0.35 });
    },
  },
  squad: {
    bus: "ui", max: 1, gap: 400,
    synth(d, t) {
      [523, 659, 784].forEach((f, i) =>
        tone(d, t + i * 0.1, { type: "triangle", f, peak: 0.14, decay: 0.3 }),
      );
    },
  },
  goal: {
    bus: "ui", max: 1, gap: 400,
    synth(d, t) {
      for (const f of [523, 659, 784, 1047]) {
        tone(d, t, { type: "triangle", f, a: 0.02, peak: 0.09, decay: 1.2 });
      }
    },
  },
  // Фанфара ачивки (§12.255): короткий подъём до октавы и тянущийся аккорд —
  // крупнее `goal`, потому что закрыта связка условий, а не одно.
  victory: {
    bus: "ui", max: 1, gap: 1500,
    synth(d, t) {
      [392, 523, 659].forEach((f, i) =>
        tone(d, t + i * 0.11, { type: "square", f, peak: 0.06, decay: 0.14 }),
      );
      const at = t + 0.36;
      for (const f of [523, 659, 784, 1047]) {
        tone(d, at, { type: "triangle", f, a: 0.01, peak: 0.1, decay: 1.6 });
      }
      tone(d, at, { type: "square", f: 1047, f2: 1060, peak: 0.03, decay: 1.2 });
      noise(d, at, { f: 6000, q: 0.7, peak: 0.05, decay: 0.5 });
    },
  },
};

// Сэмплы: запись с `file` грузится один раз после первого жеста.
const buffers = new Map();
function loadFiles() {
  for (const [name, s] of Object.entries(SOUNDS)) {
    if (!s.file) continue;
    fetch(`/sfx/${s.file}`)
      .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject()))
      .then((b) => ctx.decodeAudioData(b))
      .then((buf) => buffers.set(name, buf))
      .catch(() => {}); // нет файла — остаётся синтез
  }
}

const lastAt = new Map();
const playing = new Map();

/// Сыграть звук по имени. `pan` — от −1 (слева) до 1 (справа).
export function play(name, { pan = 0, gain = 1 } = {}) {
  if (!ctx || ctx.state !== "running" || prefs.mute) return;
  const s = SOUNDS[name];
  if (!s || !prefs.on[s.bus]) return;
  const now = performance.now();
  if (now - (lastAt.get(name) ?? -1e9) < (s.gap ?? 0)) return;
  if ((playing.get(name) ?? 0) >= (s.max ?? 8)) return;
  lastAt.set(name, now);
  playing.set(name, (playing.get(name) ?? 0) + 1);
  setTimeout(() => playing.set(name, playing.get(name) - 1), 300);

  let dest = bus[s.bus];
  const g = ctx.createGain();
  g.gain.value = gain;
  g.connect(dest);
  dest = g;
  if (pan && ctx.createStereoPanner) {
    const p = ctx.createStereoPanner();
    p.pan.value = Math.max(-1, Math.min(1, pan));
    p.connect(g);
    dest = p;
  }
  const t = ctx.currentTime + 0.005;
  const buf = buffers.get(name);
  if (buf) {
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.playbackRate.value = 0.95 + Math.random() * 0.1;
    src.connect(dest);
    src.start(t);
  } else {
    s.synth(dest, t);
  }
  setTimeout(() => g.disconnect(), 2500);
}

// --- фон ---------------------------------------------------------------------
//
// Гул базы: два расстроенных низких тона и шум через lowpass, фильтр медленно
// дышит. Тихо: это подложка, а не музыка.
function startAmbient() {
  if (ambient) return;
  const out = ctx.createGain();
  out.gain.value = 0.12;
  out.connect(bus.ambient);
  const lp = ctx.createBiquadFilter();
  lp.type = "lowpass";
  lp.frequency.value = 320;
  lp.connect(out);
  for (const f of [55, 55.7, 82.5]) {
    const o = ctx.createOscillator();
    o.type = "sawtooth";
    o.frequency.value = f;
    const g = ctx.createGain();
    g.gain.value = f > 60 ? 0.12 : 0.25;
    o.connect(g).connect(lp);
    o.start();
  }
  const n = ctx.createBufferSource();
  n.buffer = noiseBuf;
  n.loop = true;
  const ng = ctx.createGain();
  ng.gain.value = 0.15;
  n.connect(ng).connect(lp);
  n.start();
  const lfo = ctx.createOscillator();
  lfo.frequency.value = 0.07;
  const depth = ctx.createGain();
  depth.gain.value = 120;
  lfo.connect(depth).connect(lp.frequency);
  lfo.start();
  ambient = out;
}
