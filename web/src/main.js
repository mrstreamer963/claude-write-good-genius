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
  study: 0xb08fde, // учёба: книжка над котом, сидящим за партой
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
const researchEl = document.getElementById('research');
const craftEl = document.getElementById('craft');
const noteEl = document.getElementById('note');

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
let researchRunning = false; // и тема тоже одна за раз (§12.26)
let craftRunning = false; // и заказ (§12.30)
let fame = 0; // известность: копится от вылазок, открывает доступ (§12.24)
const missionButtons = []; // кнопки запуска — их гасим, пока миссия идёт
const recruitButtons = []; // кнопки найма — гасим по известности и складу
const teachButtons = []; // кнопки обучения — живы, когда выбран ровно один кот
const topicButtons = []; // кнопки тем — гасим по технологиям, складу и допуску
const recipeButtons = []; // кнопки рецептов — гасим по технологии и мастерской
const tileButtons = []; // кнопки палитры, закрытые технологией (§12.27)

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
    // Учёба — потраченное котовремя, и это вся её цена (§12.18): не видно её —
    // игрок просто недосчитается рабочих лап.
    c.studyMark.visible = !!e.studying;
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
  renderResearchPanel(snap.research);
  renderCraftPanel(snap.crafting);
  syncRecruitButtons(snap.recruits);
  syncTopicButtons(snap.topics);
  syncRecipeButtons(snap.recipes);
  syncTileButtons(snap.techs);
  renderNotePanel(snap.notes);
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
  // Книжка — на месте «зззз»: спать и учиться одновременно нельзя, а состояния
  // читаются одинаково («кот занят собой, а не базой»).
  const studyMark = new Graphics();
  studyMark
    .rect(-TILE * 0.18, -TILE * 0.74, TILE * 0.36, TILE * 0.2)
    .fill(COLORS.study)
    .stroke({ color: 0x000000, width: 1 })
    .moveTo(0, -TILE * 0.74)
    .lineTo(0, -TILE * 0.54)
    .stroke({ color: 0x000000, width: 1 });
  studyMark.visible = false;
  c.addChild(body);
  c.addChild(stuckRing);
  c.addChild(load);
  c.addChild(sleepMark);
  c.addChild(studyMark);
  c.stuckRing = stuckRing;
  c.load = load;
  c.sleepMark = sleepMark;
  c.studyMark = studyMark;
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

  syncTeachButtons();

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
  if (e.studying) parts.push('<div class="cat-sub">учится</div>');
  // Надетое: снаряжение молча прибавляет отряду силы, и без этой строки игрок
  // не свяжет пропавший со склада комбинезон с выросшим прогнозом вылазки
  // (§12.29). Пустой комплект показываем тоже — иначе непонятно, что он бывает.
  const gear = e.gear ?? [];
  const force = gear.reduce((sum, i) => sum + ((meta.items ?? [])[i]?.force ?? 0), 0);
  parts.push(
    '<div class="cat-sub">' +
      (gear.length
        ? `надето: ${gear.map((i) => esc(itemLabel(i))).join(' · ')} (+${force} к силе)`
        : 'не экипирован') +
      '</div>',
  );
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

// Панель темы. Исследование идёт молча в дальней комнате, и без панели видно
// только кота, который зачем-то стоит в лаборатории (§12.26).
function renderResearchPanel(list) {
  const r = (list ?? [])[0];
  researchRunning = !!r;
  if (!r || !meta) {
    researchEl.hidden = true;
    return;
  }
  const def = (meta.research ?? [])[r.def];
  const pct = r.total > 0 ? Math.round((r.progress / r.total) * 100) : 0;
  const parts = [
    `<div class="cat-name">${esc(def?.label || def?.id || 'Тема')}</div>`,
    '<div class="cat-skill">' +
      `<div class="cat-row"><span>Изучено</span><b>${pct}%</b></div>` +
      `<div class="bar"><i style="width:${pct}%"></i></div>` +
      '</div>',
    // Пусто — исполнитель ещё не нашёлся: тема ждёт, а не идёт. Разница
    // важная, и в полоске её не видно.
    `<div class="cat-sub">${r.unit ? esc(r.unit) : 'ждёт исполнителя'}</div>`,
    '<button class="tool research-cancel"><span>Бросить</span></button>',
  ];
  researchEl.innerHTML = parts.join('');
  researchEl.hidden = false;
  researchEl
    .querySelector('.research-cancel')
    .addEventListener('click', () => worker.postMessage({ type: 'cancelResearch' }));
}

// Панель заказа. Показывает **текущую штуку**, а не весь заказ: работа и оплата
// идут поштучно, и «40% от пяти» игрок прочтёт неверно (§12.30).
function renderCraftPanel(list) {
  const c = (list ?? [])[0];
  craftRunning = !!c;
  if (!c || !meta) {
    craftEl.hidden = true;
    return;
  }
  const def = (meta.recipes ?? [])[c.def];
  const pct = c.total > 0 ? Math.round((c.progress / c.total) * 100) : 0;
  // Три разных «ничего не происходит», и путать их нельзя: некому взяться,
  // нечем платить или работа идёт.
  const state = c.unit ? esc(c.unit) : c.paid ? 'ждёт исполнителя' : 'ждёт материала';
  const parts = [
    `<div class="cat-name">${esc(def?.label || def?.id || 'Заказ')}</div>`,
    '<div class="cat-skill">' +
      `<div class="cat-row"><span>Штука</span><b>${pct}%</b></div>` +
      `<div class="bar"><i style="width:${pct}%"></i></div>` +
      `<div class="cat-sub">осталось ${c.left} шт</div>` +
      '</div>',
    `<div class="cat-sub">${state}</div>`,
    '<button class="tool craft-cancel"><span>Отменить</span></button>',
  ];
  craftEl.innerHTML = parts.join('');
  craftEl.hidden = false;
  craftEl
    .querySelector('.craft-cancel')
    .addEventListener('click', () => worker.postMessage({ type: 'cancelCraft' }));
}

// Записка (§4.6, §12.28). Что известно о будущем — решает ядро: пока детали не
// проступили, их в снапшоте просто нет, и показывать тут нечего. Прошедшее не
// стирается — записка заодно и журнал: видно, чем кончился каждый срок.
function renderNotePanel(list) {
  const notes = list ?? [];
  if (!notes.length || !meta) {
    noteEl.hidden = true;
    return;
  }
  const rows = notes.map((n) => {
    const when = n.done
      ? n.succeeded
        ? '<span class="good">успели</span>'
        : '<span class="warn">не успели</span>'
      : `через ${n.left}`;
    const parts = [
      `<div class="cat-row"><span>${esc(n.label)}</span><b>${when}</b></div>`,
      `<div class="cat-sub">${esc(n.detail || n.hint)}</div>`,
    ];
    // Требование показываем, только пока событие впереди: после срока важно уже
    // не «чего не хватало», а чем всё кончилось.
    if (!n.done && n.revealed && n.requires.length) {
      const needs = n.requires.map(techLabel).join(' · ');
      parts.push(
        n.ready
          ? `<div class="cat-sub good">готовы: ${esc(needs)}</div>`
          : `<div class="cat-sub warn">нужно: ${esc(needs)}</div>`,
      );
    }
    return `<div class="row${n.done ? ' past' : ''}">${parts.join('')}</div>`;
  });
  noteEl.innerHTML = `<div class="cat-name">Записка</div>${rows.join('')}`;
  noteEl.hidden = false;
}

function techLabel(id) {
  const def = (meta.research ?? []).find((r) => r.id === id);
  return def?.label || id;
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

// Клавиши. Escape — отмена начатой протяжки: единственный способ передумать, не
// отпуская кнопку (уже применённую рамку отменяет ластик).
//
// Время — на пробеле и цифрах: рука игрока и так на мыши, а темп меняют чаще
// всего остального. Пробел **переключает** паузу и возвращает тот темп, на
// котором остановились, — «пауза» и «×1» это разные вещи, и терять ×10 из-за
// секундной остановки обидно.
// Клавишу узнаём по `code`, а не по `key`: `code` — это физическая кнопка, и он
// одинаков в любой раскладке. Цифровой ряд и цифровой блок — одно и то же.
const SPEED_KEYS = {
  Digit1: 1,
  Numpad1: 1,
  Digit2: 5,
  Numpad2: 5,
  Digit3: 10,
  Numpad3: 10,
};

window.addEventListener('keydown', (e) => {
  if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.code === 'Escape' || e.key === 'Escape') {
    if (dragFrom) endDrag(false);
    return;
  }
  if (e.code === 'Space' || e.key === ' ') {
    // Иначе пробел «нажимает» кнопку в фокусе — а это может оказаться вылазка
    // или найм: клавиша одна, а цена ошибки разная.
    e.preventDefault();
    setSpeed(speed > 0 ? 0 : lastSpeed);
    return;
  }
  const speedKey = SPEED_KEYS[e.code] ?? SPEED_KEYS['Digit' + e.key];
  if (speedKey) setSpeed(speedKey);
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

// Разделы тулбара: раскрыт ровно один. Инструментов стало на четыре механики
// больше, чем помещается в экран, и список уезжал под записку — а листать
// скроллом то, чем пользуешься каждые пять секунд, хуже, чем один клик.
const sections = [];
// Какой раздел открыт, помним между перестройками: иначе после каждого
// возвращения к палитре её пришлось бы раскрывать заново.
let openSection = 'Постройка';

function mkSection(el, title) {
  const sec = document.createElement('div');
  const head = document.createElement('button');
  head.className = 'sec-head';
  head.innerHTML = `<span>${esc(title)}</span><span class="chev">›</span>`;
  head.addEventListener('click', () => openOnly(title));
  const body = document.createElement('div');
  body.className = 'sec-body';
  sec.appendChild(head);
  sec.appendChild(body);
  el.appendChild(sec);
  sections.push({ title, head, body });
  return body;
}

function openOnly(title) {
  openSection = title;
  for (const s of sections) {
    const on = s.title === title;
    s.head.classList.toggle('active', on);
    s.body.hidden = !on;
  }
}

function buildToolbar() {
  const el = document.getElementById('toolbar');
  el.innerHTML = '';
  sections.length = 0;

  // Курсор — вне разделов: это не инструмент в ряду прочих, а состояние «ничего
  // не размечаю», и оно нужно из любого раздела.
  const cursorBtn = mkTool('<span class="sw sw-cursor"></span><span>Курсор</span>', () =>
    selectCursor(cursorBtn),
  );
  el.appendChild(cursorBtn);

  const build = mkSection(el, 'Постройка');

  tileButtons.length = 0;
  meta.palette.forEach((p, i) => {
    // Цена набором — рядом с образцом: что и сколько завезти на клетку.
    const cost = costChips(p.cost);
    const b = mkTool(
      `<span class="sw" style="background:${p.color}"></span><span>${p.label || p.id}</span>${cost}`,
      () => selectBuild(i, b),
    );
    // Закрытый технологией тайл виден, но не размечается: невидимая цель не
    // тянет, а ядро такую разметку всё равно отклонит (§12.27, §4.4).
    if (p.tech) tileButtons.push({ btn: b, tech: p.tech });
    build.appendChild(b);
  });

  const er = mkTool('<span class="sw sw-erase"></span><span>Стереть</span>', () =>
    selectBuild(-1, er),
  );
  build.appendChild(er);

  const scrap = mkSection(el, 'Лом');

  // Разметка уборки рамкой: повторный жест по помеченному снимает пометку.
  // Кот не выбирается — задачу возьмёт любой свободный.
  const st = mkTool('<span class="sw sw-scrap"></span><span>На склад</span>', () => selectStore(st));
  scrap.appendChild(st);

  // Автоуборка — не режим ввода, а правило симуляции, поэтому кнопка не входит
  // в общую группу инструментов и своей подсветкой их не сбивает.
  const auto = mkTool('<span class="sw sw-scrap"></span><span>Убирать сам</span>', () => {
    autoTidy = !autoTidy;
    auto.classList.toggle('on', autoTidy);
    worker.postMessage({ type: 'setAutoTidy', on: autoTidy });
  });
  auto.classList.add('toggle', 'on');
  scrap.appendChild(auto);

  // Вылазки. Не режим ввода: клик — это сразу заявка, отряд наберётся сам
  // (§12.22). Поэтому кнопки не входят в общую подсветку инструментов.
  const missions = meta.missions ?? [];
  if (missions.length) {
    const raids = mkSection(el, 'Вылазки');

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
      raids.appendChild(b);
    });
    syncMissionButtons();
  }

  // Наука. Тема — разметка работы, как чертёж: кота не выбираем (§12.26).
  // Цена теми же фишками, что у тайлов и найма: образцы — обычный предмет.
  const topics = meta.research ?? [];
  if (topics.length) {
    const science = mkSection(el, 'Наука');

    topicButtons.length = 0;
    topics.forEach((r, i) => {
      const b = mkTool(
        `<span class="sw sw-lab"></span><span>${esc(r.label || r.id)}</span>${costChips(r.cost)}`,
        () => worker.postMessage({ type: 'research', topic: i }),
      );
      b.classList.add('toggle');
      b.dataset.level = r.level ?? 0;
      topicButtons.push(b);
      science.appendChild(b);
    });
  }

  // Производство. Заказ — разметка работы, как чертёж, но со счётчиком штук:
  // клик заказывает одну, Shift — пять (§12.30). Кота не выбираем.
  const recipes = meta.recipes ?? [];
  if (recipes.length) {
    const shop = mkSection(el, 'Производство');

    recipeButtons.length = 0;
    recipes.forEach((r, i) => {
      // На кнопке — что выходит, и следом цена: те же фишки, что у тайлов.
      const b = mkTool(
        `<span class="sw sw-shop"></span><span>${esc(r.label || r.id)}</span>` +
          `${costChips(r.gives)}<span class="of">←</span>${costChips(r.cost)}`,
        (e) => worker.postMessage({ type: 'craft', recipe: i, count: e.shiftKey ? 5 : 1 }),
      );
      b.classList.add('toggle');
      recipeButtons.push(b);
      shop.appendChild(b);
    });
  }

  // Обучение. Кнопка адресная, а не разметка работы: игрок отправляет за парту
  // конкретного кота, и это решение о его судьбе (§12.18). Домены без `taught`
  // сюда не попадают — «Стройке» парта не нужна.
  const taught = (meta.skills ?? []).filter((s) => (s.taught ?? 0) > 0);
  if (taught.length) {
    const school = mkSection(el, 'Обучение');

    teachButtons.length = 0;
    for (const s of taught) {
      const b = mkTool(
        `<span class="sw sw-study"></span><span>Учить: ${esc(s.label || s.id)}</span>`,
        () => {
          if (selectedUnits.length === 1) {
            worker.postMessage({ type: 'teach', id: selectedUnits[0], skill: s.id });
          }
        },
      );
      b.classList.add('toggle');
      b.dataset.skill = s.id;
      b.dataset.hint = `до ${s.taught}-го уровня, дальше только практика`;
      teachButtons.push(b);
      school.appendChild(b);
    }
    syncTeachButtons();
  }

  // Найм. Кандидаты уникальны (§4.2): каждый приходит один раз, известность
  // открывает, а платит склад — цена теми же фишками, что и у тайлов (§12.24).
  const recruits = meta.recruits ?? [];
  if (recruits.length) {
    const hire = mkSection(el, 'Найм');

    recruitButtons.length = 0;
    recruits.forEach((r, i) => {
      const b = mkTool(
        `<span class="sw sw-hire"></span><span>${esc(r.label || r.id)}</span>${costChips(r.cost)}`,
        () => worker.postMessage({ type: 'hire', recruit: i }),
      );
      b.classList.add('toggle');
      b.dataset.requires = r.requires ?? 0;
      recruitButtons.push(b);
      hire.appendChild(b);
    });
  }

  // Раскрыт тот раздел, что был открыт до перестройки; на первом кадре это
  // палитра — с неё игра и начинается.
  openOnly(sections.some((s) => s.title === openSection) ? openSection : 'Постройка');
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
// Доступность темы считает ядро (технологии, склад, допуск, лаборатория), здесь
// только показываем — и объясняем, чего не хватает: молчащая кнопка читается как
// сломанная, а закрытая цель, наоборот, тянет (§4.4).
function syncTopicButtons(list) {
  topicButtons.forEach((b, i) => {
    const t = (list ?? [])[i];
    if (!t) return;
    const ready = !t.known && t.unlocked && t.affordable && t.staffed && t.lab && !researchRunning;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    b.title = t.known
      ? 'Уже изучено'
      : !t.unlocked
        ? 'Нужны предыдущие технологии'
        : !t.lab
          ? 'Нет лаборатории'
          : !t.staffed
            ? `Нужен кот с «Наукой» ${b.dataset.level} уровня`
            : !t.affordable
              ? 'На складе нет образцов'
              : researchRunning
                ? 'Тема уже изучается'
                : 'Взяться за тему';
  });
}

// Доступность рецепта считает ядро (технологии, мастерская, склад), здесь
// только показываем. Пустой склад кнопку **не гасит**: заказ без материала ядро
// примет, он будет ждать — как чертёж без лома (§12.30). Поэтому «нечем платить»
// живёт в подсказке, а не в `disabled`.
function syncRecipeButtons(list) {
  recipeButtons.forEach((b, i) => {
    const r = (list ?? [])[i];
    if (!r) return;
    const ready = r.unlocked && r.shop && !craftRunning;
    b.disabled = !ready;
    b.classList.toggle('on', ready && r.affordable);
    b.title = !r.unlocked
      ? 'Нужна технология'
      : !r.shop
        ? 'Нет мастерской'
        : craftRunning
          ? 'Заказ уже в работе'
          : r.affordable
            ? 'Заказать: клик — штука, Shift — пять'
            : 'На складе нет материала — заказ будет ждать';
  });
}

// Палитра, закрытая технологией: кнопка видна и объясняет, чем открывается.
// Название темы берём из палитры тем — второго списка технологий не заводим.
function syncTileButtons(techs) {
  const known = techs ?? [];
  for (const { btn, tech } of tileButtons) {
    const open = known.includes(tech);
    btn.disabled = !open;
    const def = (meta.research ?? []).find((r) => r.id === tech);
    btn.title = open ? '' : `Откроет тема «${def?.label || tech}»`;
  }
}

// Учат по одному: обучение адресно, и «учить троих разом» — это уже не решение
// о судьбе кота, а разметка работы, которой обучение как раз не является.
function syncTeachButtons() {
  const ready = selectedUnits.length === 1;
  for (const b of teachButtons) {
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    b.title = ready ? `${selectedUnits[0]} — ${b.dataset.hint}` : 'Выберите одного кота';
  }
}

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

// Подсказка: длинная и закрывает карту, поэтому её можно убрать одной кнопкой.
// Состояние нигде не хранится — на POC лишняя persistent-настройка дороже, чем
// один клик после перезагрузки.
const hintEl = document.getElementById('hint');
const hintToggle = document.getElementById('hint-toggle');
hintToggle.classList.add('active');
hintToggle.addEventListener('click', () => {
  hintEl.hidden = !hintEl.hidden;
  hintToggle.classList.toggle('active', !hintEl.hidden);
});

// --- скорость времени -----------------------------------------------------

// Текущий темп и тот, к которому возвращает пробел: пауза — это не «скорость 0
// навсегда», а снятая на минуту рука с руля, и вернуть игрок хочет тот темп,
// на котором остановился.
let speed = 1;
let lastSpeed = 1;

function setSpeed(s) {
  // Мусор до воркера не доходит: скорость приходит из разметки и с клавиш, и
  // одна опечатка в `data-speed` иначе тихо замораживает симуляцию.
  if (!Number.isFinite(s) || s < 0) return;
  if (s > 0) lastSpeed = s;
  speed = s;
  worker.postMessage({ type: 'setSpeed', speed: s });
  for (const b of document.querySelectorAll('.speed')) {
    b.classList.toggle('active', Number(b.dataset.speed) === s);
  }
}
// Только кнопки с самой скоростью: без фильтра сюда попадала соседняя «?», и
// клик по ней слал в воркер `Number(undefined)` — то есть останавливал время.
for (const b of document.querySelectorAll('.speed[data-speed]')) {
  b.addEventListener('click', () => setSpeed(Number(b.dataset.speed)));
}
setSpeed(1);
