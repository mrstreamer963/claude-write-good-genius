// Главный поток: PixiJS-рендер + ввод игрока. Логики нет — рисуем данные из воркера
// и шлём команды постройки.

import { Application, Container, Graphics } from 'pixi.js';

const TILE = 28;

const COLORS = {
  bg: 0x0e0f13,
  empty: 0x14161d, // непостроенная ячейка
  gridLine: 0x262c3a,
  unit: {
    cat_excellent: 0xe0c060,
    cat_helper: 0x8fb8de,
  },
  unitDefault: 0xcccccc,
};

const stageEl = document.getElementById('stage');
const tickEl = document.getElementById('tick');

const app = new Application();
await app.init({ background: COLORS.bg, antialias: true, resizeTo: stageEl });
stageEl.appendChild(app.canvas);

// Мир: тайлы -> юниты -> оверлей (подсветка ячейки под курсором).
const world = new Container();
const tileLayer = new Container();
const unitLayer = new Container();
const overlay = new Container();
world.addChild(tileLayer);
world.addChild(unitLayer);
world.addChild(overlay);
app.stage.addChild(world);

const hoverRect = new Graphics();
overlay.addChild(hoverRect);

app.stage.eventMode = 'static';
app.stage.hitArea = app.screen;

const units = new Map(); // id -> Container

let meta = null; // { width, height, palette: [{id,label,color}] }
let paletteColors = []; // number[]
let selected = 0; // индекс палитры, или -1 = стереть
let painting = false;

// --- worker ---------------------------------------------------------------

const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

worker.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'ready') {
    meta = m.meta;
    paletteColors = meta.palette.map((p) => hex(p.color));
    buildToolbar();
    layout();
    drawMap(m.map);
  } else if (m.type === 'map') {
    drawMap(m.map);
  } else if (m.type === 'snapshot') {
    renderSnapshot(m.snap);
  } else if (m.type === 'error') {
    showError(m.message);
  }
};

function hex(s) {
  return parseInt(s.replace('#', ''), 16);
}

// --- layout / render ------------------------------------------------------

function layout() {
  if (!meta) return;
  world.x = Math.max(8, Math.floor((app.screen.width - meta.width * TILE) / 2));
  world.y = Math.max(8, Math.floor((app.screen.height - meta.height * TILE) / 2));
}
app.renderer.on('resize', layout);

function drawMap(map) {
  const cells = map.cells;
  tileLayer.removeChildren();
  const g = new Graphics();
  for (let y = 0; y < meta.height; y++) {
    for (let x = 0; x < meta.width; x++) {
      const v = cells[y * meta.width + x];
      const color = v >= 0 ? paletteColors[v] : COLORS.empty;
      g.rect(x * TILE, y * TILE, TILE, TILE)
        .fill(color)
        .stroke({ color: COLORS.gridLine, width: 1 });
    }
  }
  tileLayer.addChild(g);
}

function renderSnapshot(snap) {
  tickEl.textContent = snap.tick;
  const seen = new Set();
  for (const e of snap.entities) {
    seen.add(e.id);
    const c = units.get(e.id) ?? createUnit(e);
    // TODO(§8b): интерполяция между тиками. Пока — снап к центру тайла.
    c.x = e.x * TILE + TILE / 2;
    c.y = e.y * TILE + TILE / 2;
  }
  for (const [id, c] of units) {
    if (!seen.has(id)) {
      c.destroy({ children: true });
      units.delete(id);
    }
  }
}

function createUnit(e) {
  const c = new Container();
  const body = new Graphics();
  body
    .circle(0, 0, TILE * 0.3)
    .fill(COLORS.unit[e.sprite] ?? COLORS.unitDefault)
    .stroke({ color: 0x000000, width: 2 });
  c.addChild(body);
  unitLayer.addChild(c);
  units.set(e.id, c);
  return c;
}

// --- ввод: постройка ------------------------------------------------------

function tileAt(global) {
  if (!meta) return null;
  const p = world.toLocal(global);
  const tx = Math.floor(p.x / TILE);
  const ty = Math.floor(p.y / TILE);
  if (tx < 0 || ty < 0 || tx >= meta.width || ty >= meta.height) return null;
  return { tx, ty };
}

function paint(global) {
  const t = tileAt(global);
  if (!t) return;
  worker.postMessage({ type: 'build', x: t.tx, y: t.ty, tile: selected });
}

function updateHover(global) {
  const t = tileAt(global);
  hoverRect.clear();
  if (!t) return;
  const col = selected >= 0 ? paletteColors[selected] : 0xff5566;
  hoverRect
    .rect(t.tx * TILE, t.ty * TILE, TILE, TILE)
    .fill({ color: col, alpha: 0.22 })
    .stroke({ color: col, width: 2, alpha: 0.9 });
}

app.stage.on('pointerdown', (e) => {
  painting = true;
  paint(e.global);
});
app.stage.on('pointermove', (e) => {
  updateHover(e.global);
  if (painting) paint(e.global);
});
app.stage.on('pointerup', () => (painting = false));
app.stage.on('pointerupoutside', () => (painting = false));

// --- палитра постройки ----------------------------------------------------

function buildToolbar() {
  const el = document.getElementById('toolbar');
  el.innerHTML = '<div class="tt">Постройка</div>';
  meta.palette.forEach((p, i) => {
    const b = document.createElement('button');
    b.className = 'tool';
    b.innerHTML = `<span class="sw" style="background:${p.color}"></span><span>${p.label || p.id}</span>`;
    b.addEventListener('click', () => selectTool(i, b));
    el.appendChild(b);
  });
  const er = document.createElement('button');
  er.className = 'tool';
  er.innerHTML = `<span class="sw sw-erase"></span><span>Стереть</span>`;
  er.addEventListener('click', () => selectTool(-1, er));
  el.appendChild(er);

  selectTool(0, el.querySelector('.tool'));
}

function selectTool(i, btn) {
  selected = i;
  for (const b of document.querySelectorAll('#toolbar .tool')) b.classList.remove('active');
  if (btn) btn.classList.add('active');
}

function showError(message) {
  const el = document.getElementById('error');
  el.hidden = false;
  el.textContent = 'Ошибка воркера: ' + message;
  console.error(message);
}

// --- скорость времени -----------------------------------------------------

function setSpeed(s) {
  worker.postMessage({ type: 'setSpeed', speed: s });
  for (const b of document.querySelectorAll('.speed')) {
    b.classList.toggle('active', Number(b.dataset.speed) === s);
  }
}
for (const b of document.querySelectorAll('.speed')) {
  b.addEventListener('click', () => setSpeed(Number(b.dataset.speed)));
}
setSpeed(1);
