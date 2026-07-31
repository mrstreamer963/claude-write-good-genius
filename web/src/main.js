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
  stuck: 0xff9a3c, // кот замурован / приказ невыполним
  scrap: 0xc9a227, // материал по умолчанию, если палитра предметов пуста
  rest: 0x7fd6b5, // сон: бодрость в панели и «зззз» над спящим котом
  unit: {
    cat_excellent: 0xe0c060,
    cat_helper: 0x8fb8de,
  },
  unitDefault: 0xcccccc,
};

const stageEl = document.getElementById('stage');
const tickEl = document.getElementById('tick');
const scrapEl = document.getElementById('scrap');
const catEl = document.getElementById('cat');
const missionEl = document.getElementById('mission');

const app = new Application();
await app.init({ background: COLORS.bg, antialias: true, resizeTo: stageEl });
stageEl.appendChild(app.canvas);

// Мир: тайлы -> лом -> чертежи -> юниты -> оверлей (подсветки).
const world = new Container();
const tileLayer = new Container();
const scrapLayer = new Container(); // кучи лома на полу
const bpLayer = new Container(); // чертежи (призраки будущих тайлов)
const unitLayer = new Container();
const overlay = new Container();
world.addChild(tileLayer);
world.addChild(scrapLayer);
world.addChild(bpLayer);
world.addChild(unitLayer);
world.addChild(overlay);
app.stage.addChild(world);

const hoverRect = new Graphics();
// Колец столько, сколько выбрано котов: под вылазку их набирают по несколько.
const selectionRings = new Container();
const orderMarker = new Graphics();
orderMarker
  .circle(0, 0, TILE * 0.16)
  .fill({ color: COLORS.select, alpha: 0.9 })
  .circle(0, 0, TILE * 0.34)
  .stroke({ color: COLORS.select, width: 2, alpha: 0.8 });
orderMarker.visible = false;
overlay.addChild(hoverRect);
overlay.addChild(orderMarker);
overlay.addChild(selectionRings);

app.stage.eventMode = 'static';
app.stage.hitArea = app.screen;

const units = new Map(); // id -> Container
const unitTiles = new Map(); // id -> { x, y } (в тайлах)
const orders = new Map(); // id -> { x, y } (заданная цель, для метки)

let meta = null; // { width, height, palette, items, skills, perks }
let paletteColors = []; // number[]
let itemColors = []; // number[] — цвет предмета по индексу палитры items
let mapCells = null; // Int-массив состояния карты
let mode = 'cursor'; // 'cursor' | 'build' | 'store'
let buildTile = 0; // индекс палитры, или -1 = стереть (в режиме build)
let autoTidy = true; // коты сами свозят лом на склад (см. ядро, §12.16)
// Выбор множественный: отряд на вылазку игрок набирает поимённо (§12.23), а
// один выбранный кот — это его частный случай. Панель показывает последнего.
let selectedUnits = [];
let dragFrom = null; // якорь рамки (клетка, где нажали), null = не тянем
let dragTo = null; // текущий угол рамки; переживает выход курсора за карту
let missionRunning = false; // миссия на POC одна за раз (§12.22)
let fame = 0; // известность: копится от вылазок, открывает доступ (§12.24)
const missionButtons = []; // кнопки запуска — их гасим, пока миссия идёт
const recruitButtons = []; // кнопки найма — гасим по известности и складу

// --- worker ---------------------------------------------------------------

const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

worker.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'ready') {
    meta = m.meta;
    paletteColors = meta.palette.map((p) => hex(p.color));
    itemColors = (meta.items ?? []).map((i) => hex(i.color));
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

// Кучи лома на полу. Точное количество — в шапке; здесь только «сколько
// примерно», чтобы куча читалась одним взглядом и не спорила с тайлом под ней.
// Цвет предмета по индексу палитры; палитры может не быть (схема без items).
function itemColor(item) {
  return itemColors[item] ?? COLORS.scrap;
}

function drawScrap(list) {
  scrapLayer.removeChildren();
  if (!list || !list.length) return;
  const g = new Graphics();
  for (const s of list) {
    const x = s.x * TILE;
    const y = s.y * TILE;
    const chips = s.count >= 15 ? 3 : s.count >= 5 ? 2 : 1;
    for (let i = 0; i < chips; i++) {
      const w = TILE * 0.4 - i * 4;
      g.rect(x + (TILE - w) / 2, y + TILE * 0.62 - i * 4, w, 3).fill({
        color: itemColor(s.item),
        alpha: 0.95,
      });
    }
    // Помечена «на склад» — за ней придёт свободный кот. При автоуборке помечено
    // всё, что лежит вне склада, так что метка заодно показывает, что режим включён.
    if (s.marked) {
      g.circle(x + TILE / 2, y + TILE * 0.3, 2.5).fill({ color: COLORS.select, alpha: 0.9 });
    }
  }
  scrapLayer.addChild(g);
}

function drawBlueprints(list) {
  bpLayer.removeChildren();
  if (!list || !list.length) return;
  const g = new Graphics();
  for (const b of list) {
    const x = b.x * TILE;
    const y = b.y * TILE;
    const isDemolish = b.tile < 0;
    const color = isDemolish ? COLORS.erase : (paletteColors[b.tile] ?? 0x888888);
    const supplied = b.delivered >= b.need;

    if (isDemolish) {
      // Снос: перечёркиваем существующий тайл, не пряча его под заливкой —
      // игрок должен видеть, что именно уйдёт.
      g.moveTo(x + 6, y + 6)
        .lineTo(x + TILE - 6, y + TILE - 6)
        .moveTo(x + TILE - 6, y + 6)
        .lineTo(x + 6, y + TILE - 6)
        .stroke({ color, width: 2, alpha: 0.9 });
      g.rect(x + 1, y + 1, TILE - 2, TILE - 2).stroke({ color, width: 1, alpha: 0.6 });
    } else {
      // Постройка: призрачная заливка будущего тайла + рамка. Пока лом не
      // завезли, площадка бледная — работа туда ещё не назначена.
      g.rect(x + 1, y + 1, TILE - 2, TILE - 2)
        .fill({ color, alpha: supplied ? 0.28 : 0.1 })
        .stroke({ color, width: 1, alpha: supplied ? 0.85 : 0.35 });
    }

    if (!supplied) {
      // Полоса подвоза материала — на месте полосы работы: пока она не полна,
      // работа и не начнётся.
      const m = b.need > 0 ? b.delivered / b.need : 1;
      g.rect(x + 3, y + TILE - 6, TILE - 6, 3).fill({ color: COLORS.scrap, alpha: 0.2 });
      if (m > 0) {
        g.rect(x + 3, y + TILE - 6, (TILE - 6) * m, 3).fill({ color: COLORS.scrap, alpha: 0.95 });
      }
      continue;
    }
    // прогресс-бар работы
    const p = b.total > 0 ? Math.min(1, b.progress / b.total) : 0;
    if (p > 0) {
      g.rect(x + 3, y + TILE - 6, (TILE - 6) * p, 3).fill({ color: COLORS.select, alpha: 0.95 });
    }
  }
  bpLayer.addChild(g);
}

function renderSnapshot(snap) {
  tickEl.textContent = snap.tick;
  drawScrap(snap.stacks);
  drawBlueprints(snap.blueprints);
  // Всё добро мира по типам: и лежащее, и уже поднятое — иначе счётчик
  // проседает, пока кот несёт груз, и это читается как потеря материала.
  const totals = new Map();
  const add = (item, n) => totals.set(item, (totals.get(item) ?? 0) + n);
  for (const s of snap.stacks) add(s.item, s.count);
  const seen = new Set();
  for (const e of snap.entities) {
    seen.add(e.id);
    if (e.carrying > 0) add(e.carrying_item, e.carrying);
    const c = units.get(e.id) ?? createUnit(e);
    // Ушедшего на вылазку на карте нет: его позиция — это шлюз, с которого он
    // ушёл, и она ничего не говорит о том, где кот на самом деле (§12.22).
    // Из `unitTiles` он тоже выпадает — иначе клик по шлюзу выбирал бы призрака.
    c.visible = !e.away;
    if (e.away) {
      unitTiles.delete(e.id);
      continue;
    }
    // TODO(§8b): интерполяция между тиками. Пока — снап к центру тайла.
    c.x = e.x * TILE + TILE / 2;
    c.y = e.y * TILE + TILE / 2;
    c.stuckRing.visible = !!e.stuck;
    c.load.visible = e.carrying > 0;
    if (e.carrying > 0) c.load.tint = itemColor(e.carrying_item);
    // Спящий кот пригашен: игрок должен видеть, почему тот не работает.
    c.alpha = e.sleeping ? 0.55 : 1;
    c.sleepMark.visible = !!e.sleeping;
    unitTiles.set(e.id, { x: e.x, y: e.y });
  }
  // В шапке — только фишка и число: подписи распирают её, а цвет тот же, что
  // у куч на полу и у цены в палитре. Известность рядом: она не предмет, но
  // считается так же и решает, что базе вообще доступно.
  fame = snap.fame ?? 0;
  scrapEl.innerHTML =
    (meta.items ?? [])
      .map(
        (it, i) =>
          `<i class="chip" style="background:${it.color}" title="${esc(it.label || it.id)}"></i>` +
          `<b>${totals.get(i) ?? 0}</b>`,
      )
      .join(' ') + `<span class="fame" title="Известность">★<b>${fame}</b></span>`;
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
  renderCatPanel(snap.entities);
  renderMissionPanel(snap.missions);
  syncRecruitButtons(snap.recruits);
}

function createUnit(e) {
  const c = new Container();
  const body = new Graphics();
  body
    .circle(0, 0, TILE * 0.3)
    .fill(COLORS.unit[e.sprite] ?? COLORS.unitDefault)
    .stroke({ color: 0x000000, width: 2 });
  // Кольцо «кот застрял» — шире кольца выбора, чтобы читались вместе.
  const stuckRing = new Graphics();
  stuckRing.circle(0, 0, TILE * 0.52).stroke({ color: COLORS.stuck, width: 2, alpha: 0.9 });
  stuckRing.visible = false;
  // Груз лома — брусок над котом, той же краской, что и кучи на полу.
  const load = new Graphics();
  load
    .rect(-TILE * 0.16, -TILE * 0.5, TILE * 0.32, 4)
    .fill(0xffffff)
    .stroke({ color: 0x000000, width: 1 });
  load.visible = false;
  // «Зззз» — три пузырька над спящим, выше бруска груза: спать можно и с ломом.
  const sleepMark = new Graphics();
  sleepMark
    .circle(-4, -TILE * 0.62, 1.5)
    .fill(COLORS.rest)
    .circle(1, -TILE * 0.7, 2)
    .fill(COLORS.rest)
    .circle(7, -TILE * 0.78, 2.5)
    .fill(COLORS.rest);
  sleepMark.visible = false;
  c.addChild(body);
  c.addChild(stuckRing);
  c.addChild(load);
  c.addChild(sleepMark);
  c.stuckRing = stuckRing;
  c.load = load;
  c.sleepMark = sleepMark;
  unitLayer.addChild(c);
  units.set(e.id, c);
  return c;
}

function updateSelectionOverlay() {
  // Выбранный кот мог уйти на вылазку — с карты он при этом исчезает, но из
  // выбора не выпадает: вернётся, и кольцо снова зажжётся.
  selectionRings.removeChildren();
  for (const id of selectedUnits) {
    const at = unitTiles.get(id);
    if (!at) continue;
    const ring = new Graphics();
    ring.circle(0, 0, TILE * 0.44).stroke({ color: COLORS.select, width: 2 });
    ring.x = at.x * TILE + TILE / 2;
    ring.y = at.y * TILE + TILE / 2;
    selectionRings.addChild(ring);
  }

  const last = selectedUnits[selectedUnits.length - 1];
  const so = last ? orders.get(last) : null;
  if (so) {
    orderMarker.visible = true;
    orderMarker.x = so.x * TILE + TILE / 2;
    orderMarker.y = so.y * TILE + TILE / 2;
  } else {
    orderMarker.visible = false;
  }
}

// Панель выбранного кота. Навык растёт молча, и это единственное место, где
// рост виден игроку (§12.17): уровень, полоска до следующего, лапы и перки.
function renderCatPanel(entities) {
  const last = selectedUnits[selectedUnits.length - 1];
  const e = last ? entities.find((u) => u.id === last) : null;
  if (!e || !meta) {
    catEl.hidden = true;
    return;
  }
  const defs = meta.skills ?? [];
  const parts = [`<div class="cat-name">${esc(e.id)}</div>`];
  if (selectedUnits.length > 1) {
    parts.push(`<div class="cat-sub">выбрано ${selectedUnits.length}: ${selectedUnits.map(esc).join(' · ')}</div>`);
  }
  for (let i = 0; i < defs.length; i++) {
    const s = e.skills?.[i];
    if (!s) continue;
    const levels = defs[i].levels ?? [];
    const from = s.level > 0 ? levels[s.level - 1] : 0;
    // next = 0 — навык на потолке: полоска полная, порога дальше нет.
    const pct = s.next > from ? Math.round(((s.xp - from) / (s.next - from)) * 100) : 100;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${esc(defs[i].label || defs[i].id)}</span><b>${s.level}</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">${s.next > 0 ? `${s.xp} / ${s.next}` : 'потолок'}</div>` +
        '</div>',
    );
  }
  if (e.energy_max > 0) {
    const pct = Math.round((e.energy / e.energy_max) * 100);
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Бодрость</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="rest" style="width:${pct}%"></i></div>` +
        (e.sleeping ? '<div class="cat-sub">спит</div>' : '') +
        '</div>',
    );
  }
  if (e.away) parts.push('<div class="cat-sub">на вылазке</div>');
  const held = e.carrying > 0 ? ` ${esc(itemLabel(e.carrying_item))}` : '';
  const paws =
    (e.carry_max > 0 ? `лапы ${e.carrying}/${e.carry_max}` : `в лапах ${e.carrying}`) + held;
  const tags = (e.perks ?? []).map((id) => esc(perkLabel(id)));
  parts.push(`<div class="cat-sub">${[paws, ...tags].join(' · ')}</div>`);
  catEl.innerHTML = parts.join('');
  catEl.hidden = false;
}

// Панель миссии. Пока отряд собирается, показываем состав: игрок не выбирает,
// кого послать (§12.22), — значит должен хотя бы видеть, кого выбрала за него
// симуляция и почему база вдруг перестала строить.
function renderMissionPanel(list) {
  const m = (list ?? [])[0];
  missionRunning = !!m;
  syncMissionButtons();
  if (!m || !meta) {
    missionEl.hidden = true;
    return;
  }
  const def = (meta.missions ?? [])[m.def];
  const parts = [`<div class="cat-name">${esc(def?.label || def?.id || 'Вылазка')}</div>`];
  if (m.away) {
    const pct = m.total > 0 ? Math.round(((m.total - m.left) / m.total) * 100) : 0;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>В пути</span><b>${pct}%</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        '</div>',
    );
  } else {
    parts.push('<div class="cat-sub">Собираются у шлюза</div>');
  }
  parts.push(`<div class="cat-sub">${m.squad.map(esc).join(' · ') || '—'}</div>`);
  // Прогноз исхода: его считает ядро тем же выражением, которым исход
  // посчитается на возвращении (§12.23). Пока отряд на базе — это ещё и
  // предупреждение: увидел «провал», успел отозвать.
  if (m.danger > 0) {
    const verdict = m.failed ? 'провал' : `добыча ${m.share}%`;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Сила / сложность</span><b>${m.strength} / ${m.danger}</b></div>` +
        `<div class="bar"><i class="${m.failed ? 'fail' : ''}" style="width:${m.failed ? 100 : m.share}%"></i></div>` +
        `<div class="cat-sub">${verdict}</div>` +
        '</div>',
    );
  }
  // Отозвать можно только тех, кто ещё на базе: ушедший отряд симуляции уже
  // не подчиняется — вылазка считается разом по возвращении.
  if (!m.away) {
    parts.push('<button class="tool mission-cancel"><span>Отозвать</span></button>');
  }
  missionEl.innerHTML = parts.join('');
  missionEl.hidden = false;
  const cancel = missionEl.querySelector('.mission-cancel');
  if (cancel) cancel.addEventListener('click', () => worker.postMessage({ type: 'cancelMission' }));
}

function itemLabel(item) {
  const def = (meta.items ?? [])[item];
  return def?.label || def?.id || '?';
}

function perkLabel(id) {
  const def = (meta.perks ?? []).find((p) => p.id === id);
  return def?.label || id;
}

function esc(s) {
  return String(s).replace(
    /[&<>"]/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c],
  );
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

// режим постройки: игрок тянет рамку, отпускание применяет её целиком
function rectOf(a, b) {
  return {
    x: Math.min(a.tx, b.tx),
    y: Math.min(a.ty, b.ty),
    w: Math.abs(a.tx - b.tx) + 1,
    h: Math.abs(a.ty - b.ty) + 1,
  };
}

// Один жест — одно сообщение: решение по рамке принимает ядро, а не рендер.
// Здесь оно принято быть и не может — списки чертежей и куч у нас из последнего
// снапшота, а на ×10 он отстаёт от симуляции на несколько тиков.
function applyDrag() {
  if (!dragFrom || !dragTo) return;
  const rect = rectOf(dragFrom, dragTo);
  if (mode === 'store') worker.postMessage({ type: 'store', ...rect });
  else worker.postMessage({ type: 'build', ...rect, tile: buildTile });
}

// `global` — где отпустили кнопку: подсветка сразу возвращается к одной клетке
// под курсором. Без этого рамка висела бы на экране до следующего движения мыши
// и читалась как «что-то ещё выделено».
function endDrag(apply, global) {
  if (dragFrom && apply) applyDrag();
  dragFrom = null;
  dragTo = null;
  hoverRect.clear();
  if (global) updateHover(global);
}

// режим курсора: выбрать кота (Shift — добавить в отряд) / приказать идти
function command(global, add) {
  const t = tileAt(global);
  if (!t) return;
  const hit = unitAt(t.tx, t.ty);
  if (hit) {
    if (!add) selectedUnits = [hit];
    else if (selectedUnits.includes(hit)) selectedUnits = selectedUnits.filter((u) => u !== hit);
    else selectedUnits.push(hit);
    updateSelectionOverlay();
    return;
  }
  if (!isWalkable(t.tx, t.ty)) return;
  // Приказ уходит каждому выбранному: коты друг друга не блокируют, и толпа
  // на одной клетке — законное состояние (см. `set_target` в ядре).
  for (const id of selectedUnits) {
    worker.postMessage({ type: 'move', id, x: t.tx, y: t.ty });
    orders.set(id, { x: t.tx, y: t.ty });
  }
  updateSelectionOverlay();
}

function updateHover(global) {
  const t = tileAt(global);
  hoverRect.clear();
  // Во время протяжки показываем всю рамку — даже если курсор ушёл за карту.
  const r = dragFrom ? rectOf(dragFrom, dragTo) : t && { x: t.tx, y: t.ty, w: 1, h: 1 };
  if (!r) return;
  const col =
    mode === 'store'
      ? COLORS.scrap
      : mode === 'build'
        ? buildTile >= 0
          ? paletteColors[buildTile]
          : COLORS.erase
        : COLORS.select;
  hoverRect
    .rect(r.x * TILE, r.y * TILE, r.w * TILE, r.h * TILE)
    .fill({ color: col, alpha: 0.16 })
    .stroke({ color: col, width: 2, alpha: 0.9 });
}

app.stage.on('pointerdown', (e) => {
  if (mode === 'cursor') {
    command(e.global, e.shiftKey);
    return;
  }
  const t = tileAt(e.global);
  if (!t) return;
  dragFrom = t;
  dragTo = t;
  updateHover(e.global);
});
app.stage.on('pointermove', (e) => {
  const t = tileAt(e.global);
  if (dragFrom && t) dragTo = t;
  updateHover(e.global);
});
app.stage.on('pointerup', (e) => endDrag(true, e.global));
// Курсор ушёл со сцены — применяем последнюю рамку в пределах карты: бросать
// уже нарисованное выделение обиднее, чем применить его на клетку меньше.
app.stage.on('pointerupoutside', (e) => endDrag(true, e.global));

// Escape — отмена начатой протяжки: единственный способ передумать, не отпуская
// кнопку. Уже применённую рамку отменяет ластик (или повторный ластик).
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && dragFrom) endDrag(false);
});

// --- тулбар ---------------------------------------------------------------

// Цена тайла: по цветной фишке на каждый нужный предмет. Порядок — как в
// палитре предметов, чтобы он совпадал со счётчиками в шапке (в самой цене
// он алфавитный: в рулсете это отображение).
function costChips(cost) {
  // serde-wasm-bindgen отдаёт YAML-отображение настоящим `Map`, а не объектом:
  // цена приходит как `Map { "scrap" => 1 }`.
  const entries = cost instanceof Map ? [...cost.entries()] : Object.entries(cost ?? {});
  if (!entries.length) return '';
  const chips = (meta.items ?? [])
    .map((it) => {
      const found = entries.find(([id]) => id === it.id);
      return found ? `<i class="chip" style="background:${it.color}"></i>${found[1]}` : '';
    })
    .filter(Boolean)
    .join(' ');
  return `<span class="cost">${chips}</span>`;
}

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
    // Цена набором — рядом с образцом: что и сколько завезти на клетку.
    const cost = costChips(p.cost);
    const b = mkTool(
      `<span class="sw" style="background:${p.color}"></span><span>${p.label || p.id}</span>${cost}`,
      () => selectBuild(i, b),
    );
    el.appendChild(b);
  });

  const er = mkTool('<span class="sw sw-erase"></span><span>Стереть</span>', () =>
    selectBuild(-1, er),
  );
  el.appendChild(er);

  const tl = document.createElement('div');
  tl.className = 'tt';
  tl.textContent = 'Лом';
  el.appendChild(tl);

  // Разметка уборки рамкой: повторный жест по помеченному снимает пометку.
  // Кот не выбирается — задачу возьмёт любой свободный.
  const st = mkTool('<span class="sw sw-scrap"></span><span>На склад</span>', () => selectStore(st));
  el.appendChild(st);

  // Автоуборка — не режим ввода, а правило симуляции, поэтому кнопка не входит
  // в общую группу инструментов и своей подсветкой их не сбивает.
  const auto = mkTool('<span class="sw sw-scrap"></span><span>Убирать сам</span>', () => {
    autoTidy = !autoTidy;
    auto.classList.toggle('on', autoTidy);
    worker.postMessage({ type: 'setAutoTidy', on: autoTidy });
  });
  auto.classList.add('toggle', 'on');
  el.appendChild(auto);

  // Вылазки. Не режим ввода: клик — это сразу заявка, отряд наберётся сам
  // (§12.22). Поэтому кнопки не входят в общую подсветку инструментов.
  const missions = meta.missions ?? [];
  if (missions.length) {
    const tm = document.createElement('div');
    tm.className = 'tt';
    tm.textContent = 'Вылазки';
    el.appendChild(tm);

    missionButtons.length = 0;
    missions.forEach((m, i) => {
      // На кнопке — добыча теми же фишками, что и цена тайла: это одна и та же
      // валюта. Отряд берётся из выделения: кого послать, решает игрок.
      const b = mkTool(
        `<span class="sw sw-gate"></span><span>${esc(m.label || m.id)}</span>${costChips(m.loot)}`,
        () => worker.postMessage({ type: 'launch', mission: i, units: [...selectedUnits] }),
      );
      b.classList.add('toggle');
      b.dataset.squad = m.squad;
      b.dataset.requires = m.requires ?? 0;
      b.dataset.hint = `${m.squad} кота · ${m.ticks} тиков · сложность ${m.danger ?? 0}`;
      missionButtons.push(b);
      el.appendChild(b);
    });
    syncMissionButtons();
  }

  // Найм. Кандидаты уникальны (§4.2): каждый приходит один раз, известность
  // открывает, а платит склад — цена теми же фишками, что и у тайлов (§12.24).
  const recruits = meta.recruits ?? [];
  if (recruits.length) {
    const tr = document.createElement('div');
    tr.className = 'tt';
    tr.textContent = 'Найм';
    el.appendChild(tr);

    recruitButtons.length = 0;
    recruits.forEach((r, i) => {
      const b = mkTool(
        `<span class="sw sw-hire"></span><span>${esc(r.label || r.id)}</span>${costChips(r.cost)}`,
        () => worker.postMessage({ type: 'hire', recruit: i }),
      );
      b.classList.add('toggle');
      b.dataset.requires = r.requires ?? 0;
      recruitButtons.push(b);
      el.appendChild(b);
    });
  }

  selectCursor(cursorBtn); // режим по умолчанию
}

// Доступность кандидата считает ядро (известность + содержимое склада), здесь
// только показываем: дублировать правило в JS значит однажды показать кнопку,
// которую ядро отклонит.
function syncRecruitButtons(list) {
  recruitButtons.forEach((b, i) => {
    const r = (list ?? [])[i];
    if (!r) return;
    const ready = !r.hired && r.unlocked && r.affordable;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    b.title = r.hired
      ? 'Уже на базе'
      : !r.unlocked
        ? `Откликнется при известности ${b.dataset.requires}`
        : !r.affordable
          ? 'На складе нечем заплатить'
          : 'Нанять';
  });
}

// Кнопка живая, только когда выделено ровно столько котов, сколько уходит:
// ядро неполную заявку отклоняет молча (§12.23), а молчащая кнопка читается
// как сломанная. Заодно подсказка объясняет, чего не хватает.
function syncMissionButtons() {
  for (const b of missionButtons) {
    const need = Number(b.dataset.squad);
    const requires = Number(b.dataset.requires);
    const known = fame >= requires;
    const ready = !missionRunning && known && selectedUnits.length === need;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    // Закрытые вылазки видны, а не спрятаны: лестница ответственности — это то,
    // к чему игрок идёт, и невидимая цель не тянет (§4.4).
    b.title = missionRunning
      ? 'Вылазка уже идёт'
      : !known
        ? `${b.dataset.hint} · нужна известность ${requires}`
        : `${b.dataset.hint} · выбрано ${selectedUnits.length} из ${need}`;
  }
}

function mkTool(html, onClick) {
  const b = document.createElement('button');
  b.className = 'tool';
  b.innerHTML = html;
  b.addEventListener('click', onClick);
  return b;
}

function activate(btn) {
  for (const b of document.querySelectorAll('#toolbar .tool:not(.toggle)')) {
    b.classList.remove('active');
  }
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
function selectStore(btn) {
  mode = 'store';
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
