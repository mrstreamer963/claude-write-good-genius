// Главный поток: PixiJS-рендер + ввод игрока. Логики нет — рисуем данные из воркера
// и шлём команды (постройка тайлов, приказы движения).

import { Application, Container, Graphics } from 'pixi.js';

const TILE = 28;

const COLORS = {
  bg: 0x0e0f13,
  empty: 0x14161d, // непостроенная (непроходимая) ячейка
  gridLine: 0x262c3a,
  select: 0x6cf0a0, // выбор кота / метка цели
  erase: 0xff5566,
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

// Мир: тайлы -> юниты -> оверлей (подсветки).
const world = new Container();
const tileLayer = new Container();
const bpLayer = new Container(); // чертежи (призраки будущих тайлов)
const unitLayer = new Container();
const overlay = new Container();
world.addChild(tileLayer);
world.addChild(bpLayer);
world.addChild(unitLayer);
world.addChild(overlay);
app.stage.addChild(world);

const hoverRect = new Graphics();
const selectionRing = new Graphics();
selectionRing.circle(0, 0, TILE * 0.44).stroke({ color: COLORS.select, width: 2 });
selectionRing.visible = false;
const orderMarker = new Graphics();
orderMarker
  .circle(0, 0, TILE * 0.16)
  .fill({ color: COLORS.select, alpha: 0.9 })
  .circle(0, 0, TILE * 0.34)
  .stroke({ color: COLORS.select, width: 2, alpha: 0.8 });
orderMarker.visible = false;
overlay.addChild(hoverRect);
overlay.addChild(orderMarker);
overlay.addChild(selectionRing);

app.stage.eventMode = 'static';
app.stage.hitArea = app.screen;

const units = new Map(); // id -> Container
const unitTiles = new Map(); // id -> { x, y } (в тайлах)
const orders = new Map(); // id -> { x, y } (заданная цель, для метки)

let meta = null; // { width, height, palette: [{id,label,color}] }
let paletteColors = []; // number[]
let mapCells = null; // Int-массив состояния карты
let mode = 'cursor'; // 'cursor' | 'build'
let buildTile = 0; // индекс палитры, или -1 = стереть (в режиме build)
let selectedUnit = null;
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
  mapCells = map.cells;
  tileLayer.removeChildren();
  const g = new Graphics();
  for (let y = 0; y < meta.height; y++) {
    for (let x = 0; x < meta.width; x++) {
      const v = mapCells[y * meta.width + x];
      const color = v >= 0 ? paletteColors[v] : COLORS.empty;
      g.rect(x * TILE, y * TILE, TILE, TILE)
        .fill(color)
        .stroke({ color: COLORS.gridLine, width: 1 });
    }
  }
  tileLayer.addChild(g);
}

function drawBlueprints(list) {
  bpLayer.removeChildren();
  if (!list || !list.length) return;
  const g = new Graphics();
  for (const b of list) {
    const color = paletteColors[b.tile] ?? 0x888888;
    const x = b.x * TILE;
    const y = b.y * TILE;
    // призрачная заливка + пунктирная рамка
    g.rect(x + 1, y + 1, TILE - 2, TILE - 2)
      .fill({ color, alpha: 0.28 })
      .stroke({ color, width: 1, alpha: 0.85 });
    // прогресс-бар постройки
    const p = b.total > 0 ? Math.min(1, b.progress / b.total) : 0;
    if (p > 0) {
      g.rect(x + 3, y + TILE - 6, (TILE - 6) * p, 3).fill({ color: COLORS.select, alpha: 0.95 });
    }
  }
  bpLayer.addChild(g);
}

function renderSnapshot(snap) {
  tickEl.textContent = snap.tick;
  drawBlueprints(snap.blueprints);
  const seen = new Set();
  for (const e of snap.entities) {
    seen.add(e.id);
    const c = units.get(e.id) ?? createUnit(e);
    // TODO(§8b): интерполяция между тиками. Пока — снап к центру тайла.
    c.x = e.x * TILE + TILE / 2;
    c.y = e.y * TILE + TILE / 2;
    unitTiles.set(e.id, { x: e.x, y: e.y });
  }
  for (const [id, c] of units) {
    if (!seen.has(id)) {
      c.destroy({ children: true });
      units.delete(id);
      unitTiles.delete(id);
    }
  }

  // Снять метку цели, если кот дошёл.
  for (const [id, o] of orders) {
    const ut = unitTiles.get(id);
    if (ut && ut.x === o.x && ut.y === o.y) orders.delete(id);
  }

  updateSelectionOverlay();
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

function updateSelectionOverlay() {
  const su = selectedUnit ? unitTiles.get(selectedUnit) : null;
  if (su) {
    selectionRing.visible = true;
    selectionRing.x = su.x * TILE + TILE / 2;
    selectionRing.y = su.y * TILE + TILE / 2;
  } else {
    selectionRing.visible = false;
  }

  const so = selectedUnit ? orders.get(selectedUnit) : null;
  if (so) {
    orderMarker.visible = true;
    orderMarker.x = so.x * TILE + TILE / 2;
    orderMarker.y = so.y * TILE + TILE / 2;
  } else {
    orderMarker.visible = false;
  }
}

// --- ввод -----------------------------------------------------------------

function tileAt(global) {
  if (!meta) return null;
  const p = world.toLocal(global);
  const tx = Math.floor(p.x / TILE);
  const ty = Math.floor(p.y / TILE);
  if (tx < 0 || ty < 0 || tx >= meta.width || ty >= meta.height) return null;
  return { tx, ty };
}

function isWalkable(tx, ty) {
  return mapCells && mapCells[ty * meta.width + tx] >= 0;
}

function unitAt(tx, ty) {
  for (const [id, ut] of unitTiles) if (ut.x === tx && ut.y === ty) return id;
  return null;
}

// режим постройки
function paint(global) {
  const t = tileAt(global);
  if (!t) return;
  worker.postMessage({ type: 'build', x: t.tx, y: t.ty, tile: buildTile });
}

// режим курсора: выбрать кота / приказать идти
function command(global) {
  const t = tileAt(global);
  if (!t) return;
  const hit = unitAt(t.tx, t.ty);
  if (hit) {
    selectedUnit = hit;
    updateSelectionOverlay();
    return;
  }
  if (selectedUnit && isWalkable(t.tx, t.ty)) {
    worker.postMessage({ type: 'move', id: selectedUnit, x: t.tx, y: t.ty });
    orders.set(selectedUnit, { x: t.tx, y: t.ty });
    updateSelectionOverlay();
  }
}

function updateHover(global) {
  const t = tileAt(global);
  hoverRect.clear();
  if (!t) return;
  const col = mode === 'build' ? (buildTile >= 0 ? paletteColors[buildTile] : COLORS.erase) : COLORS.select;
  hoverRect
    .rect(t.tx * TILE, t.ty * TILE, TILE, TILE)
    .fill({ color: col, alpha: 0.16 })
    .stroke({ color: col, width: 2, alpha: 0.9 });
}

app.stage.on('pointerdown', (e) => {
  if (mode === 'build') {
    painting = true;
    paint(e.global);
  } else {
    command(e.global);
  }
});
app.stage.on('pointermove', (e) => {
  updateHover(e.global);
  if (painting) paint(e.global);
});
app.stage.on('pointerup', () => (painting = false));
app.stage.on('pointerupoutside', () => (painting = false));

// --- тулбар ---------------------------------------------------------------

function buildToolbar() {
  const el = document.getElementById('toolbar');
  el.innerHTML = '';

  const cursorBtn = mkTool('<span class="sw sw-cursor"></span><span>Курсор</span>', () =>
    selectCursor(cursorBtn),
  );
  el.appendChild(cursorBtn);

  const tt = document.createElement('div');
  tt.className = 'tt';
  tt.textContent = 'Постройка';
  el.appendChild(tt);

  meta.palette.forEach((p, i) => {
    const b = mkTool(
      `<span class="sw" style="background:${p.color}"></span><span>${p.label || p.id}</span>`,
      () => selectBuild(i, b),
    );
    el.appendChild(b);
  });

  const er = mkTool('<span class="sw sw-erase"></span><span>Стереть</span>', () => selectBuild(-1, er));
  el.appendChild(er);

  selectCursor(cursorBtn); // режим по умолчанию
}

function mkTool(html, onClick) {
  const b = document.createElement('button');
  b.className = 'tool';
  b.innerHTML = html;
  b.addEventListener('click', onClick);
  return b;
}

function activate(btn) {
  for (const b of document.querySelectorAll('#toolbar .tool')) b.classList.remove('active');
  if (btn) btn.classList.add('active');
}

function selectCursor(btn) {
  mode = 'cursor';
  activate(btn);
}
function selectBuild(i, btn) {
  mode = 'build';
  buildTile = i;
  activate(btn);
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
