// Подача карты штаба (§12.305, §12.310): местность под участками и пятно заражения вокруг
// очага. Только картинка — ни клетки, ни скорости, ни маршруты от неё не зависят,
// поэтому случайность здесь законна, а сид постоянный: окрестности базы не должны
// меняться от того, что игрок закрыл и открыл окно.

const SEED = 9173;
// Текстура мельче поля: карта размыта не видна, а печь её вдвое дешевле.
const SCALE = 0.7;

// Детерминированный генератор (mulberry32): пятно и вода каждый раз те же.
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

function makeNoise(seed, octaves = 4) {
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
    for (let o = 0; o < octaves; o++) {
      sum += value(x * f, y * f) * amp;
      amp *= 0.5;
      f *= 2;
    }
    return sum / (1 - 0.5 ** octaves);
  };
}

// Опорные цвета земли: тёмные и холодные, чтобы значки и подписи стояли поверх
// (§12.310). Низина, лес и нагорье смешиваются плавно: ступени читались пятнами.
const LOWLAND = [9, 15, 18];
const MEADOW = [15, 24, 27];
const FOREST = [9, 21, 20];
const UPLAND = [27, 33, 36];
const WATER = [70, 120, 140];
const BANDS = 14;

const clamp01 = (t) => Math.min(1, Math.max(0, t));
const ramp = (a, b, t) => {
  const k = clamp01((t - a) / (b - a));
  return k * k * (3 - 2 * k);
};

// Рельеф, лес и водная сеть по шуму; сетку масштаба рисует сама карта (§12.301).
// Шум на миллион пикселей стоит сотни миллисекунд, поэтому текстура печётся один
// раз, а в SVG едет `<image>`: фильтр `feTurbulence` пересчитывался бы на каждом
// кадре, где едет значок отряда.
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
  // Русла — по гладкому шуму: мелкие октавы рвали их в зубчатые штрихи.
  const soft = makeNoise(SEED, 2);
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
  const band = (n) => Math.floor(clamp01((n - 0.22) / 0.56) * BANDS);
  const img = ctx.createImageData(cw, ch);
  const c = [0, 0, 0];
  const mix = (to, k) => {
    for (let n = 0; n < 3; n++) c[n] += (to[n] - c[n]) * k;
  };
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const i = y * cw + x;
      const height = heights[i];
      const u = x / SCALE;
      const v = y / SCALE;
      c[0] = LOWLAND[0];
      c[1] = LOWLAND[1];
      c[2] = LOWLAND[2];
      mix(MEADOW, ramp(0.3, 0.55, height));
      mix(FOREST, ramp(0.5, 0.62, soft(u / 62 + 31.7, v / 62 + 11.3)) * 0.9);
      mix(UPLAND, ramp(0.6, 0.76, height));

      // Вода — гребень шума: линия, где он пересекает середину, сама виляет и
      // ветвится. Второй шум рвёт её на притоки, иначе сеть замыкается в кольца.
      const flow = soft(u / 190 + 41.3 + height * 0.5, v / 190 + 17.9 - height * 0.5);
      const reach = ramp(0.3, 0.42, soft(u / 300 + 3.7, v / 300 + 59.1));
      const off = Math.abs(flow - 0.5);
      mix(WATER, (1 - ramp(0.0, 0.02, off)) * 0.06 * reach); // свечение берега
      mix(WATER, (1 - ramp(0.002, 0.005, off)) * 0.28 * reach); // само русло

      const right = heights[y * cw + Math.min(x + 1, cw - 1)];
      const down = heights[Math.min(y + 1, ch - 1) * cw + x];
      const b = band(height);
      const lift = band(right) !== b || band(down) !== b ? 4 : 0;
      const grain = (soft(u / 5 + 90.1, v / 5 + 70.3) - 0.5) * 6;
      const o = i * 4;
      img.data[o] = c[0] + lift + grain;
      img.data[o + 1] = c[1] + lift + grain;
      img.data[o + 2] = c[2] + lift + grain;
      img.data[o + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  ctx.scale(SCALE, SCALE);

  // Виньетка уводит край: участки стоят с запасом, и смотреть туда незачем.
  const glow = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.35, w / 2, h / 2, Math.max(w, h) * 0.72);
  glow.addColorStop(0, "rgba(6, 12, 14, 0)");
  glow.addColorStop(1, "rgba(6, 12, 14, 0.65)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, w, h);

  // JPEG: зернистый PNG такого размера весит мегабайты, а альфа здесь не нужна.
  const url = canvas.toDataURL("image/jpeg", 0.88);
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
