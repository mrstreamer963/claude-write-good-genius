// Подача карты штаба (§12.305): местность под участками и пятно заражения вокруг
// очага. Только картинка — ни клетки, ни скорости, ни маршруты от неё не зависят,
// поэтому случайность здесь законна, а сид постоянный: окрестности базы не должны
// меняться от того, что игрок закрыл и открыл окно.

const SEED = 9173;
const SCALE = 1;

// Детерминированный генератор (mulberry32): пятно и река каждый раз те же.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function makeNoise(seed) {
  const rand = rng(seed);
  const table = Float32Array.from({ length: 65536 }, rand);
  const at = (x, y) => table[((x & 255) << 8) | (y & 255)];
  const smooth = (t) => t * t * (3 - 2 * t);
  const value = (x, y) => {
    const ix = Math.floor(x);
    const iy = Math.floor(y);
    const fx = smooth(x - ix);
    const fy = smooth(y - iy);
    const top = at(ix, iy) * (1 - fx) + at(ix + 1, iy) * fx;
    const low = at(ix, iy + 1) * (1 - fx) + at(ix + 1, iy + 1) * fx;
    return top * (1 - fy) + low * fy;
  };
  return (x, y) => {
    let sum = 0;
    let amp = 0.5;
    let f = 1;
    for (let o = 0; o < 4; o++) {
      sum += value(x * f, y * f) * amp;
      amp *= 0.5;
      f *= 2;
    }
    return sum / 0.9375;
  };
}

const LOW = [36, 46, 46];
const HIGH = [78, 80, 64];
const BANDS = 11;

// Рельеф — горизонтали по шуму, вода — лента по его же ложбинам. Сетку масштаба
// рисует сама карта (§12.301). Шум на миллион пикселей стоит десятки миллисекунд,
// поэтому текстура печётся один раз, а в SVG едет `<image>`:
// фильтр `feTurbulence` пересчитывался бы на каждом кадре, где едет значок отряда.
const baked = new Map();

export function terrainTexture(w, h) {
  const key = `${w}x${h}`;
  if (baked.has(key)) return baked.get(key);
  const cw = Math.ceil(w * SCALE);
  const ch = Math.ceil(h * SCALE);
  const canvas = document.createElement("canvas");
  canvas.width = cw;
  canvas.height = ch;
  const ctx = canvas.getContext("2d");
  const noise = makeNoise(SEED);
  const heights = new Float32Array(cw * ch);
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const u = x / SCALE / 170;
      const v = y / SCALE / 170;
      // Сдвиг координат шумом гнёт горизонтали: без него они читаются пятнами.
      const warp = noise(u + 7.3, v + 2.1) * 1.2;
      heights[y * cw + x] = noise(u + warp, v - warp);
    }
  }
  const img = ctx.createImageData(cw, ch);
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const i = y * cw + x;
      const t = Math.min(1, Math.max(0, (heights[i] - 0.22) / 0.56));
      const band = Math.floor(t * BANDS);
      const k = band / (BANDS - 1);
      const right = heights[y * cw + Math.min(x + 1, cw - 1)];
      const down = heights[Math.min(y + 1, ch - 1) * cw + x];
      const near = (n) => Math.floor(Math.min(1, Math.max(0, (n - 0.22) / 0.56)) * BANDS);
      const edge = near(right) !== band || near(down) !== band;
      const lift = edge ? 13 : 0;
      const o = i * 4;
      img.data[o] = LOW[0] + (HIGH[0] - LOW[0]) * k + lift;
      img.data[o + 1] = LOW[1] + (HIGH[1] - LOW[1]) * k + lift;
      img.data[o + 2] = LOW[2] + (HIGH[2] - LOW[2]) * k + lift;
      img.data[o + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  ctx.scale(SCALE, SCALE);

  // Река: идёт через полотно, виляя по тому же шуму.
  const trace = [];
  let ry = h * 0.72;
  for (let x = -10; x <= w + 10; x += 8) {
    ry += (noise(x / 90 + 3.1, 5.7) - 0.5) * 16 - 0.9;
    trace.push([x, ry]);
  }
  const stroke = (color, width) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.lineJoin = "round";
    ctx.beginPath();
    trace.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.stroke();
  };
  stroke("rgba(70, 120, 135, 0.13)", 11);
  stroke("rgba(95, 150, 165, 0.32)", 2.5);

  // Виньетка уводит край: участки стоят с запасом, и смотреть туда незачем.
  const glow = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.35, w / 2, h / 2, Math.max(w, h) * 0.72);
  glow.addColorStop(0, "rgba(10, 12, 12, 0)");
  glow.addColorStop(1, "rgba(10, 12, 12, 0.6)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, w, h);

  const url = canvas.toDataURL("image/png");
  baked.set(key, url);
  return url;
}

// Контур пятна заражения: неровный круг, у каждого участка свой и постоянный.
// Размер — только подача ступени, а не радиус чего-либо в мире.
export function blightBlob(site, radius) {
  const rand = rng(SEED + site * 101);
  const n = 18;
  const pts = Array.from({ length: n }, (_, k) => {
    const a = (k / n) * Math.PI * 2;
    const r = radius * (0.84 + rand() * 0.28);
    return [Math.cos(a) * r, Math.sin(a) * r];
  });
  // Сглаживаем через середины сторон: квадратичная кривая на каждую вершину.
  const mid = (p, q) => [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2];
  const start = mid(pts[n - 1], pts[0]);
  let d = `M${start[0].toFixed(1)} ${start[1].toFixed(1)}`;
  pts.forEach((p, k) => {
    const e = mid(p, pts[(k + 1) % n]);
    d += `Q${p[0].toFixed(1)} ${p[1].toFixed(1)} ${e[0].toFixed(1)} ${e[1].toFixed(1)}`;
  });
  return `${d}Z`;
}
