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
  wound: 0xff5566, // ранение: крест над лежачим — тот же красный, что у стирания
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
const captiveEl = document.getElementById('captive');
const researchEl = document.getElementById('research');
const craftEl = document.getElementById('craft');
const dealEl = document.getElementById('deal');
const noteEl = document.getElementById('note');

// Кнопки внутри панелей вешаются **делегированием, один раз на контейнер**.
//
// Панели перерисовываются каждым снапшотом (~16 мс) целиком, через `innerHTML`:
// прогресс тикает, и разметка честно меняется. Значит узел кнопки живёт один
// кадр, и обработчик, повешенный на сам узел, почти никогда не срабатывает —
// `click` требует, чтобы `mousedown` и `mouseup` пришли в **один и тот же**
// элемент, а между ними панель успевает перерисоваться. Слушатель на
// контейнере это переживает: браузер шлёт `click` ближайшему общему предку.
function onPanelClick(el, selector, send) {
  el.addEventListener('click', (e) => {
    if (e.target.closest(selector)) send();
  });
}
onPanelClick(craftEl, '.craft-cancel', () => worker.postMessage({ type: 'cancelCraft' }));
onPanelClick(researchEl, '.research-cancel', () => worker.postMessage({ type: 'cancelResearch' }));
onPanelClick(missionEl, '.mission-cancel', () => worker.postMessage({ type: 'cancelMission' }));

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
let autoRest = true; // и сами бросают работу на исходе сил (§12.33)
// Выбор множественный: отряд на вылазку игрок набирает поимённо (§12.23), а
// один выбранный кот — это его частный случай. Панель показывает последнего.
let selectedUnits = [];
let dragFrom = null; // якорь рамки (клетка, где нажали), null = не тянем
let dragTo = null; // текущий угол рамки; переживает выход курсора за карту
// Кот, стоявший под началом рамки. Запоминается в момент нажатия, а не читается
// на отпускании: на ×10 кот успевает уйти за время клика, а игрок целился в
// того, кого видел.
let dragUnit = null;
// Кнопка «Курсор» — единственная, к которой обращаются извне тулбара: клик по
// коту любым инструментом возвращает игру в режим выбора, и подсветка обязана
// поехать вместе с режимом.
let cursorBtn = null;
let missionRunning = false; // миссия на POC одна за раз (§12.22)
let researchRunning = false; // и тема тоже одна за раз (§12.26)
let craftRunning = false; // и заказ (§12.30)
let fame = 0; // известность — только для показа: ворота считает ядро (§12.24)
// Кто сейчас выбыл по ранению (§12.37). Держим списком id, а не пересчитываем в
// обработчике клика: ранение приходит из ядра между кликами, и кнопка вылазки
// обязана погаснуть сама, не дожидаясь, пока игрок перевыберет отряд.
let wounded = new Set();
// Кто остался в плену (§12.40). По той же причине списком: пленный появляется
// в снапшоте, а не по клику, и кнопка «За своим» обязана зажечься сама.
let captives = [];
// Ворота вылазок в порядке палитры — их считает ядро (§12.24). Известность и
// «есть ли кого спасать» здесь не пересчитываются: второй экземпляр правила
// однажды разойдётся с фасадом, и кнопка нажмётся вхолостую.
let raids = [];
// Репутация по фракциям в порядке палитры (§12.43). Нужна не для ворот — их
// считает ядро, — а чтобы назвать отказ словом: «нужно 30, у вас −10».
let standing = [];
// Курсы и сделка (§12.44). Цену считает ядро тем же выражением, каким её
// спишет заказ, — здесь только показываем.
let money = 0;
let prices = [];
let hasPost = false;
let dealRunning = false;
const tradeButtons = []; // кнопки сделок — гасим, пока идёт другая
const missionButtons = []; // кнопки запуска — их гасим, пока миссия идёт
const recruitButtons = []; // кнопки найма — гасим по известности и складу
const teachButtons = []; // кнопки обучения — живы, когда выбран ровно один кот
const topicButtons = []; // кнопки тем — гасим по технологиям, складу и допуску
const recipeButtons = []; // кнопки рецептов — гасим по технологии и мастерской
const tileButtons = []; // кнопки палитры, закрытые технологией (§12.27)

// --- worker ---------------------------------------------------------------

const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

// Сохранение партии (§12.45). Слот один: снимок описывает мир целиком, а
// выбирать между слотами пока нечем — ни смерти, ни отката в игре нет.
const SAVE_KEY = 'sp.save.v1';
// Продолжаем ровно один раз за загрузку страницы: `ready` приходит и после
// самой загрузки снимка, и повторная попытка была бы бесконечной.
let triedResume = false;

worker.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'ready') {
    meta = m.meta;
    paletteColors = meta.palette.map((p) => hex(p.color));
    itemColors = (meta.items ?? []).map((i) => hex(i.color));
    buildToolbar();
    layout();
    drawMap(m.map);
    if (!triedResume) {
      triedResume = true;
      const saved = localStorage.getItem(SAVE_KEY);
      if (saved) worker.postMessage({ type: 'load', json: saved });
    }
  } else if (m.type === 'map') {
    drawMap(m.map);
  } else if (m.type === 'snapshot') {
    renderSnapshot(m.snap);
  } else if (m.type === 'saved') {
    if (m.auto) localStorage.setItem(SAVE_KEY, m.json);
    else download(`sp-save-${stamp()}.json`, m.json);
  } else if (m.type === 'traced') {
    download(`sp-trace-${stamp()}.txt`, m.text);
  } else if (m.type === 'loadFailed') {
    // Снимок не подошёл к текущим правилам. Держать его дальше незачем: он
    // не подойдёт и завтра, а базовая партия уже идёт.
    localStorage.removeItem(SAVE_KEY);
    showError(`Сохранение не загрузилось: ${m.message}. Партия начата заново.`);
  } else if (m.type === 'error') {
    showError(m.message);
  }
};

// Уход со вкладки — момент, когда партию стоит дописать не дожидаясь таймера.
// Рефреш этим не покрыть: состояние держит воркер, а ответ асинхронный и до
// умирающей страницы уже не дойдёт — там работает короткий интервал автосейва.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') worker.postMessage({ type: 'save', toSlot: true });
});

function stamp() {
  return new Date().toISOString().slice(0, 19).replaceAll(':', '-');
}

function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

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

// --- календарь (§12.46) ----------------------------------------------------
//
// Сутки — подача, а не механика: ядро о них не знает, тик остаётся
// единственными часами мира (§12.28). Число тиков в сутках приезжает в `meta`
// один раз, как палитры, и разворачивается здесь.

/// Номер суток, считая с первых. Ноль в `meta.day` = календаря нет.
function dayOf(tick) {
  const len = meta?.day ?? 0;
  return len > 0 ? Math.floor(tick / len) + 1 : 0;
}

/// Время внутри суток. Сутки растягиваются на 24 часа независимо от того,
/// сколько в них тиков, — иначе подпись пришлось бы менять вместе с балансом.
function clockOf(tick) {
  const len = meta?.day ?? 0;
  if (len <= 0) return '';
  const part = (tick % len) / len;
  const mins = Math.floor(part * 24 * 60);
  return `${String(Math.floor(mins / 60)).padStart(2, '0')}:${String(mins % 60).padStart(2, '0')}`;
}

/// «2 дня», «5 дней», «21 день» — без этого записка читается как машинный лог.
function days(n) {
  const tens = n % 100;
  const ones = n % 10;
  if (tens >= 11 && tens <= 14) return `${n} дней`;
  if (ones === 1) return `${n} день`;
  if (ones >= 2 && ones <= 4) return `${n} дня`;
  return `${n} дней`;
}

function renderSnapshot(snap) {
  // Сырой тик остаётся под наведением: он нужен для отладки и для сверки с
  // тестами, которые меряют время тиками и ничем другим мерить не могут.
  const day = dayOf(snap.tick);
  tickEl.textContent = day ? `${day}, ${clockOf(snap.tick)}` : snap.tick;
  tickEl.parentElement.title = `тик ${snap.tick}`;
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
    // «Дошёл и делает» против «ещё идёт»: маркер вешается только на первое —
    // кот в пути к лежанке не спит, а идёт (§12.41).
    const asleep = e.job === 'rest' && !e.moving;
    const lying = e.job === 'heal' && !e.moving;
    // Спящий кот пригашен: игрок должен видеть, почему тот не работает.
    // Лежачий раненый — тоже: причины разные, а следствие для базы одно (§12.37).
    c.alpha = asleep || lying ? 0.55 : 1;
    c.sleepMark.visible = asleep;
    // Крест — над раненым; он важнее «зззз», потому что лежачий кот выбыл не на
    // сотню тиков, а до конца лечения, и это единственная необратимая на вид
    // потеря, какая в игре есть.
    c.woundMark.visible = lying;
    // Учёба — потраченное котовремя, и это вся её цена (§12.18): не видно её —
    // игрок просто недосчитается рабочих лап.
    c.studyMark.visible = e.job === 'study' && !e.moving;
    // Медик — только дошедший: лечит он с соседней клетки, и до неё ещё надо
    // добраться. Иначе пустой крест ехал бы через полбазы, обещая лечение.
    c.medicMark.visible = e.job === 'treat' && !e.moving;
    unitTiles.set(e.id, { x: e.x, y: e.y });
  }
  // В шапке — только фишка и число: подписи распирают её, а цвет тот же, что
  // у куч на полу и у цены в палитре. Известность рядом: она не предмет, но
  // считается так же и решает, что базе вообще доступно.
  fame = snap.fame ?? 0;
  standing = snap.standing ?? [];
  money = snap.money ?? 0;
  prices = snap.prices ?? [];
  hasPost = !!snap.post;
  scrapEl.innerHTML =
    (meta.items ?? [])
      .map(
        (it, i) =>
          `<i class="chip" style="background:${it.color}" title="${esc(it.label || it.id)}"></i>` +
          `<b>${totals.get(i) ?? 0}</b>`,
      )
      .join(' ') +
    `<span class="fame" title="Известность">★<b>${fame}</b></span>` +
    // Репутация рядом с известностью, но врозь: та отвечает «насколько высоко»
    // и только копится, эта — «от кого» и ходит в обе стороны (§12.43). Знак
    // пишем всегда: «0» и «−0» читаются одинаково, а «+20» и «−20» — нет.
    (meta.factions ?? [])
      .map((f, i) => {
        const v = standing[i] ?? 0;
        const sign = v > 0 ? `+${v}` : `${v}`;
        return (
          `<span class="standing${v < 0 ? ' bad' : ''}" title="${esc(f.label || f.id)}">` +
          `<i class="chip" style="background:${f.color}"></i><b>${sign}</b></span>`
        );
      })
      .join('') +
    // Деньги — единственная величина, которая и копится, и тратится: это счёт,
    // а не ворота (§12.44). Потому и стоят отдельно от известности.
    `<span class="money" title="Котоденьги">¤<b>${money}</b></span>`;
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

  wounded = new Set(
    snap.entities.filter((e) => e.health_max > 0 && e.health <= e.health_hurt).map((e) => e.id),
  );
  captives = snap.entities.filter((e) => e.captive).map((e) => e.id);
  raids = snap.raids ?? [];
  syncMissionButtons();

  updateSelectionOverlay();
  renderCatPanel(snap.entities);
  renderCaptivePanel();
  renderMissionPanel(snap.missions);
  renderResearchPanel(snap.research);
  renderCraftPanel(snap.crafting);
  renderDealPanel(snap.deals);
  syncRecruitButtons(snap.recruits);
  syncTopicButtons(snap.topics);
  syncRecipeButtons(snap.recipes);
  syncTileButtons(snap.techs);
  renderNotePanel(snap.notes, snap.tick);
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
  // Красный крест — на месте «зззз» и книжки: три состояния «кот занят не
  // базой» читаются одинаково и не совмещаются (§12.37).
  const woundMark = new Graphics();
  woundMark
    .rect(-2, -TILE * 0.78, 4, TILE * 0.22)
    .fill(COLORS.wound)
    .rect(-TILE * 0.11, -TILE * 0.71, TILE * 0.22, 4)
    .fill(COLORS.wound);
  woundMark.visible = false;
  // Тот же крест, но пустой — над медиком: лечение шло всегда, а видно его не
  // было, и база выглядела так, будто раны затягиваются сами (§12.41).
  // Контур против заливки — это «лечит» против «лечится»: одна картинка на
  // двоих читается как пара, а не как два разных состояния.
  const medicMark = new Graphics();
  medicMark
    .rect(-2, -TILE * 0.78, 4, TILE * 0.22)
    .rect(-TILE * 0.11, -TILE * 0.71, TILE * 0.22, 4)
    .stroke({ color: COLORS.wound, width: 1.5 });
  medicMark.visible = false;
  c.addChild(body);
  c.addChild(stuckRing);
  c.addChild(load);
  c.addChild(sleepMark);
  c.addChild(studyMark);
  c.addChild(woundMark);
  c.addChild(medicMark);
  c.stuckRing = stuckRing;
  c.load = load;
  c.sleepMark = sleepMark;
  c.studyMark = studyMark;
  c.woundMark = woundMark;
  c.medicMark = medicMark;
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

// Чем кот занят — словами. Ключ считает ядро (`Busy::job`), текст живёт здесь,
// как подписи тайлов и навыков: симуляция знает задачи, а не язык (§12.41).
//
// У каждой задачи две формулировки — «идёт» и «делает», и различает их маршрут.
// Без этого панель врала бы ровно в тот момент, когда игрок в неё смотрит:
// «лечит раненого» о коте, который только вышел с другого конца базы.
function jobLabel(e) {
  const going = !!e.moving;
  switch (e.job) {
    // Ушедшего с базы объясняют отдельные строки: «на вылазке» и «в плену» —
    // разные решения игрока, а не одно состояние (§12.40).
    case 'away':
      return '';
    case 'heal':
      return going ? 'идёт в лазарет' : 'лежит: раны заживают';
    case 'eat':
      return going ? 'идёт есть' : 'ест';
    // Приказ спящего не поднимает, пока включено «Беречь себя» (§12.51), — и
    // молчать об этом нельзя: игрок только что кликнул, а кот не двинулся.
    // Знание тут местное, из тех же `orders`, которыми рисуется метка цели:
    // ядру про «кому уже приказали» рассказывать нечего (§12.41).
    case 'rest':
      if (going) return 'идёт спать';
      return orders.has(e.id) ? 'спит: приказ подождёт' : 'спит';
    case 'treat':
      return going ? 'идёт к раненому' : 'лечит раненого';
    case 'equip':
      return going ? 'идёт за снаряжением' : 'снаряжается';
    case 'squad':
      return going ? 'идёт к шлюзу' : 'ждёт отряд';
    case 'haul':
      return e.carrying > 0 ? `несёт ${itemLabel(e.carrying_item)}` : 'идёт за грузом';
    case 'research':
      return going ? 'идёт в лабораторию' : 'исследует';
    case 'craft':
      return going ? 'идёт в мастерскую' : 'работает в мастерской';
    case 'study':
      return going ? 'идёт к парте' : 'учится';
    case 'build':
      return going ? 'идёт на площадку' : 'строит';
    case 'demolish':
      return going ? 'идёт на снос' : 'разбирает';
    // Приказ без маршрута — это и есть `stuck`, и он подписан отдельно.
    case 'order':
      return going ? 'идёт по приказу' : '';
    default:
      return 'без дела';
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
  // Занятие — первой строкой и до всех шкал: «чем он вообще занят» игрок
  // спрашивает раньше, чем «сколько у него бодрости». Застрявший объясняется
  // здесь же: `stuck` — состояние легальное, но кот из него сам не выйдет.
  const job = e.stuck ? 'не может дойти' : jobLabel(e);
  if (job) parts.push(`<div class="cat-job${e.stuck ? ' stuck' : ''}">${job}</div>`);
  // Врождённое — до навыков: оно объясняет их пределы, а не наоборот (§12.42).
  // Опыт кот доберёт работой, а эти числа даны ему навсегда, и ровно поэтому
  // коты остаются разными после того, как бригада выработалась.
  const stats = (meta.stats ?? [])
    .map((st, i) => `${esc(st.label || st.id)} ${e.stats?.[i] ?? 0}`)
    .join(' · ');
  if (stats) parts.push(`<div class="cat-sub">${stats}</div>`);
  for (let i = 0; i < defs.length; i++) {
    const s = e.skills?.[i];
    if (!s) continue;
    const levels = defs[i].levels ?? [];
    const from = s.level > 0 ? levels[s.level - 1] : 0;
    // Врождённый предел — это не потолок навыка: полоска, вставшая на месте,
    // обязана назвать причину, иначе игрок прочтёт её как поломку (§12.42).
    const capped = s.cap > 0 && s.level >= s.cap;
    const born = capped && s.cap < levels.length;
    // next = 0 — навык на потолке: полоска полная, порога дальше нет.
    const pct = capped || s.next <= from ? 100 : Math.round(((s.xp - from) / (s.next - from)) * 100);
    const note = born
      ? `предел: ${esc(statLabel(defs[i].stat))} ${e.stats?.[statIndex(defs[i].stat)] ?? 0}`
      : capped || s.next <= 0
        ? 'потолок'
        : `${s.xp} / ${s.next}`;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${esc(defs[i].label || defs[i].id)}</span><b>${s.level}</b></div>` +
        `<div class="bar"><i class="${born ? 'capped' : ''}" style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">${note}</div>` +
        '</div>',
    );
  }
  // У бодрости два порога, и они забирают кота по-разному (§12.33): выше
  // `tired` он работает, ниже — уходит спать освободившись, ниже `critical` —
  // бросает начатое. Оба надо назвать словами: без них полоска на 30 % и
  // полоска на 10 % выглядят одинаково, а кот с них ведёт себя по-разному.
  if (e.energy_max > 0) {
    const pct = Math.round((e.energy / e.energy_max) * 100);
    const spent = e.energy_critical > 0 && e.energy <= e.energy_critical;
    const note = spent
      ? 'на исходе сил: бросит работу'
      : e.energy <= e.energy_tired
        ? 'устал: доработает и пойдёт спать'
        : '';
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Бодрость</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${spent ? 'spent' : 'rest'}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : '') +
        '</div>',
    );
  }
  // Сытость — вторая потребность (§12.36). Цена голода списывается с бодрости,
  // а не со шкалы рядом, поэтому «голоден» надо назвать словом: иначе игрок
  // видит, что коты всё время спят, и не связывает это с пустым складом.
  if (e.fed_max > 0) {
    const pct = Math.round((e.fed / e.fed_max) * 100);
    const starving = e.fed <= 0;
    const note = starving ? 'голодает: бодрость горит вдвое' : e.fed <= e.fed_hungry ? 'проголодался' : '';
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Сытость</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${starving ? 'starving' : 'fed'}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : '') +
        '</div>',
    );
  }
  // Здоровье — третья шкала (§12.37). Её роняет только провал вылазки, поэтому
  // просевшая полоска всегда означает «этот кот только что вернулся с плохой
  // вылазки», а порог надо назвать словом: ниже него кота не берут в отряд, и
  // без подписи игрок прочитает молчащую кнопку как поломку.
  if (e.health_max > 0) {
    const pct = Math.round((e.health / e.health_max) * 100);
    const hurt = e.health <= e.health_hurt;
    const note = hurt ? 'ранен: не работает и в отряд не идёт' : e.health < e.health_max ? 'царапины' : '';
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Здоровье</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${hurt ? 'hurt' : 'health'}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : '') +
        '</div>',
    );
  }
  // Пленный — тоже «нет на базе», но по таймеру он не вернётся: за ним надо
  // сходить. Разные слова здесь — это разные решения игрока (§12.40).
  if (e.captive) parts.push('<div class="cat-sub">в плену: нужна вылазка за своим</div>');
  else if (e.away) parts.push('<div class="cat-sub">на вылазке</div>');
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

// Панель плена. Кота на карте нет, кликнуть по нему нельзя, и без этой панели
// он просто пропал бы — а пропажа обязана быть объяснимой (§12.40). Ушедший
// отряд объясняет себя панелью миссии и вернётся по таймеру; пленный не
// вернётся никогда, пока за ним не сходят, и сказать об этом больше некому.
function renderCaptivePanel() {
  if (!captives.length) {
    captiveEl.hidden = true;
    return;
  }
  captiveEl.innerHTML =
    '<div class="cat-name">В плену</div>' +
    `<div class="cat-sub">${captives.map(esc).join(' · ')}</div>` +
    '<div class="cat-sub">Сам не вернётся — нужна вылазка за своим</div>';
  captiveEl.hidden = false;
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
    // Спящего бойца заявка не поднимает, пока включено «Беречь себя» (§12.51),
    // а сбор ничем не ограничен по времени: «собираются у шлюза» под отрядом,
    // который никуда не идёт, читается как поломка.
    const gathering = m.resting ? 'Ждут, пока выспится боец' : 'Собираются у шлюза';
    parts.push(`<div class="cat-sub">${gathering}</div>`);
  }
  parts.push(`<div class="cat-sub">${m.squad.map(esc).join(' · ') || '—'}</div>`);
  // Прогноз исхода: его считает ядро тем же выражением, которым исход
  // посчитается на возвращении (§12.23). Пока отряд на базе — это ещё и
  // предупреждение: увидел «провал», успел отозвать.
  if (m.danger > 0) {
    // Раны считаются той же долей, что и добыча (§12.37), поэтому цену провала
    // можно назвать здесь же — из того самого числа, которым ядро её посчитает.
    const harm = Math.round(((def?.harm ?? 0) * (100 - m.share)) / 100);
    const wounds = harm > 0 ? `, раны ${harm}` : '';
    // У вылазки за своим доля тоже считается, но говорить о ней процентами
    // значило бы обещать половину кота: её исход — «вынесут или нет» (§12.40).
    const verdict = m.failed
      ? `провал${wounds}`
      : m.rescue
        ? `выносят своих${wounds}`
        : `добыча ${m.share}%${wounds}`;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Сила / сложность</span><b>${m.strength} / ${m.danger}</b></div>` +
        `<div class="bar"><i class="${m.failed ? 'fail' : ''}" style="width:${m.failed ? 100 : m.share}%"></i></div>` +
        `<div class="cat-sub">${verdict}</div>` +
        '</div>',
    );
  }
  // Цена решения — рядом с прогнозом добычи и тем же числом, которым ядро её
  // посчитает на возвращении (§12.43). Это главное, что панель обязана сказать
  // до клика: закрывшиеся ворота честны ровно постольку, поскольку игрок видел,
  // чем платит. У провала здесь ноль — и это тоже новость.
  if (m.patron >= 0 || m.against >= 0) {
    const name = (f) => esc((meta.factions ?? [])[f]?.label || '—');
    const moves = [];
    if (m.patron >= 0) moves.push(`${name(m.patron)} +${m.standing}`);
    if (m.against >= 0) moves.push(`${name(m.against)} −${m.standing}`);
    parts.push(
      '<div class="cat-skill">' +
        '<div class="cat-row"><span>Репутация</span></div>' +
        `<div class="cat-sub">${moves.join(' · ')}</div>` +
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
}

// Сделка (§12.44). Показываем **зафиксированный** курс, а не сегодняшний:
// рассчитаются именно по нему, а расписание за это время могло уйти — в этом и
// весь риск торговли. Кнопки «Отменить» здесь нет намеренно: деньги за покупку
// уже ушли, и возврат превратил бы сделку в бесплатный опцион.
function renderDealPanel(list) {
  const d = (list ?? [])[0];
  dealRunning = !!d;
  syncTradeButtons();
  if (!d || !meta) {
    dealEl.hidden = true;
    return;
  }
  const item = (meta.items ?? [])[d.item];
  const who = (meta.factions ?? [])[d.faction];
  const name = esc(item?.label || item?.id || 'товар');
  const parts = [
    `<div class="cat-name">${d.buying ? 'Покупка' : 'Продажа'}: ${name}</div>`,
    `<div class="cat-sub">${esc(who?.label || '—')} · ${d.count} шт по ${d.unit} = ${d.unit * d.count}¤</div>`,
  ];
  if (d.buying) {
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>В пути</span><b>${d.left}</b></div>` +
        '<div class="cat-sub">приедет в гараж</div>' +
        '</div>',
    );
  } else {
    // У продажи «срок» — это ходки котов, и мерить его тиками нечем: показываем
    // сделанное, а не оставшееся время.
    const pct = d.count > 0 ? Math.round((d.delivered / d.count) * 100) : 0;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Отнесли</span><b>${d.delivered} из ${d.count}</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">получено ${d.unit * d.delivered}¤</div>` +
        '</div>',
    );
  }
  dealEl.innerHTML = parts.join('');
  dealEl.hidden = false;
}

// Записка (§4.6, §12.28). Что известно о будущем — решает ядро: пока детали не
// проступили, их в снапшоте просто нет, и показывать тут нечего. Прошедшее не
// стирается — записка заодно и журнал: видно, чем кончился каждый срок.
function renderNotePanel(list, tick = 0) {
  const notes = list ?? [];
  if (!notes.length || !meta) {
    noteEl.hidden = true;
    return;
  }
  const today = dayOf(tick);
  const rows = notes.map((n) => {
    // Срок в днях, а не в тиках: «через 5780» не переводится в решение, а
    // «послезавтра» переводится. Считаем по номеру суток, а не делением
    // остатка, — иначе событие в конце дня показывалось бы «сегодня» до
    // самого утра.
    const until = dayOf(n.at) - today;
    const when = n.done
      ? n.succeeded
        ? '<span class="good">успели</span>'
        : '<span class="warn">не успели</span>'
      : !today
        ? `через ${n.left}`
        : until <= 0
          ? 'сегодня'
          : until === 1
            ? 'завтра'
            : `через ${days(until)}`;
    const stamp = today ? `день ${dayOf(n.at)} — ` : '';
    const parts = [
      `<div class="cat-row"><span>${esc(n.label)}</span><b>${when}</b></div>`,
      `<div class="cat-sub">${stamp}${esc(n.detail || n.hint)}</div>`,
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

  // Записка кончилась — но кончился не мир, а предзнание (§4.6, §12.46).
  // Итог договаривает сама записка, потому что она же и журнал; модального
  // экрана нет намеренно: песочница не прерывается (§10), играть можно дальше.
  if (notes.every((n) => n.done)) {
    const kept = notes.filter((n) => n.succeeded).length;
    const last = Math.max(...notes.map((n) => n.at));
    rows.push(
      `<div class="row past">` +
        `<div class="cat-row"><span>Записка кончилась</span>` +
        `<b>${kept} из ${notes.length}</b></div>` +
        `<div class="cat-sub">` +
        (today ? `день ${dayOf(last)} — ` : '') +
        `дальше дат нет: база живёт вслепую</div>` +
        `</div>`,
    );
  }

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

// Врождённые параметры кандидата словами: «Ум 4 · Реакция 9 · Выносливость 6».
// serde-wasm-bindgen отдаёт отображение из YAML настоящим `Map`, как и цену.
function statsHint(stats) {
  const entries = stats instanceof Map ? [...stats.entries()] : Object.entries(stats ?? {});
  if (!entries.length) return '';
  return (meta.stats ?? [])
    .map((st) => {
      const found = entries.find(([id]) => id === st.id);
      return found ? `${st.label || st.id} ${found[1]}` : '';
    })
    .filter(Boolean)
    .join(' · ');
}

function statIndex(id) {
  return (meta.stats ?? []).findIndex((s) => s.id === id);
}

function statLabel(id) {
  const def = (meta.stats ?? []).find((s) => s.id === id);
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
  if (dragFrom && apply) {
    // Клик по коту — это «покажи мне его», а не «застрой клетку под ним»:
    // возвращаемся в курсор и выбираем кота. Отличает клик от разметки размер
    // жеста: одна клетка — клик, две и больше — рамка, и застроить (или снести)
    // клетку под котом по-прежнему можно, протянув через неё.
    const rect = rectOf(dragFrom, dragTo);
    if (dragUnit && rect.w === 1 && rect.h === 1) {
      selectCursor(cursorBtn);
      selectUnit(dragUnit);
    } else {
      applyDrag();
    }
  }
  dragFrom = null;
  dragTo = null;
  dragUnit = null;
  hoverRect.clear();
  if (global) updateHover(global);
}

// Выбрать кота; `add` (Shift) — добавить в отряд или убрать из него.
function selectUnit(id, add) {
  if (!add) selectedUnits = [id];
  else if (selectedUnits.includes(id)) selectedUnits = selectedUnits.filter((u) => u !== id);
  else selectedUnits.push(id);
  updateSelectionOverlay();
}

// режим курсора: выбрать кота (Shift — добавить в отряд) / приказать идти
function command(global, add) {
  const t = tileAt(global);
  if (!t) return;
  const hit = unitAt(t.tx, t.ty);
  if (hit) {
    selectUnit(hit, add);
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
  // Одна клетка с котом под ней подсвечивается как выбор, а не как разметка:
  // отпустив кнопку здесь, игрок выберет кота, и цвет обязан сказать это до
  // клика, а не после.
  const overUnit = r.w === 1 && r.h === 1 && (dragFrom ? dragUnit : t && unitAt(t.tx, t.ty));
  const col =
    overUnit || mode === 'cursor'
      ? COLORS.select
      : mode === 'store'
        ? COLORS.scrap
        : buildTile >= 0
          ? paletteColors[buildTile]
          : COLORS.erase;
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
  dragUnit = unitAt(t.tx, t.ty);
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
  cursorBtn = mkTool('<span class="sw sw-cursor"></span><span>Курсор</span>', () =>
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

  // Правила симуляции — не режимы ввода, а тумблеры поведения котов, поэтому
  // они живут отдельно от инструментов и своей подсветкой их не сбивают.
  const rules = mkSection(el, 'Правила');

  const auto = mkTool('<span class="sw sw-scrap"></span><span>Убирать сам</span>', () => {
    autoTidy = !autoTidy;
    auto.classList.toggle('on', autoTidy);
    worker.postMessage({ type: 'setAutoTidy', on: autoTidy });
  });
  auto.classList.add('toggle', 'on');
  auto.title = 'Коты свозят лом на склад без разметки';
  rules.appendChild(auto);

  // Второй порог усталости (§12.33). Выключено — коты доработают до нуля и
  // свалятся где стоят: это осознанный выбор игрока гнать базу до упора.
  const care = mkTool('<span class="sw sw-rest"></span><span>Беречь себя</span>', () => {
    autoRest = !autoRest;
    care.classList.toggle('on', autoRest);
    worker.postMessage({ type: 'setAutoRest', on: autoRest });
  });
  care.classList.add('toggle', 'on');
  care.title = 'На исходе сил кот бросает работу и уходит спать';
  rules.appendChild(care);

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
      // Вылазка за своим доступна, только пока есть кого спасать: это решает
      // ядро, а кнопка обязана показывать то же самое (§12.40).
      b.dataset.rescue = m.rescue ? '1' : '';
      b.dataset.hint =
        `${m.squad} кота · ${m.ticks} тиков · сложность ${m.danger ?? 0}` +
        (m.harm ? ` · раны при провале ${m.harm}` : '');
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

  // Торговля (§12.44). Раздел на фракцию: у каждой свой прайс, свой темп и своя
  // наценка, и это второе лицо развилки §12.43 — сторону выбирают уже не только
  // по заказам. Кнопок по две на предмет: купить и продать, чтобы направление
  // не пряталось за модификатором.
  tradeButtons.length = 0;
  (meta.factions ?? []).forEach((fac, fi) => {
    // Чем фракция торгует, видно из палитры, а не из снапшота: тулбар строится
    // один раз по `ready`, когда курсов ещё нет. `prices` приезжает `Map`, а не
    // объектом, — та же идиома, что у цены и добычи (см. `costChips`).
    const list = fac.prices instanceof Map ? [...fac.prices.keys()] : Object.keys(fac.prices ?? {});
    const traded = (meta.items ?? [])
      .map((it, ii) => ({ it, ii }))
      .filter(({ it }) => list.includes(it.id));
    if (!traded.length) return;
    // «Рынок», а не «Торговля»: в одну строку заголовка помещается только он, а
    // перенос делал бы эти два раздела вдвое выше всех остальных.
    const sec = mkSection(el, `Рынок: ${fac.label || fac.id}`);
    for (const { it, ii } of traded) {
      for (const buying of [true, false]) {
        const b = mkTool(
          `<span class="sw" style="background:${it.color}"></span>` +
            `<span>${buying ? 'Купить' : 'Продать'} ${esc(it.label || it.id)}</span>` +
            '<b class="rate">—</b>',
          (ev) => {
            // Клик — пять штук, Shift — двадцать пять: тот же идиом, что у
            // заказа в мастерской, только товар возят мешками.
            const count = ev.shiftKey ? 25 : 5;
            worker.postMessage({ type: 'trade', faction: fi, item: ii, count, buying });
          },
        );
        b.classList.add('toggle');
        b.dataset.faction = fi;
        b.dataset.item = ii;
        b.dataset.buying = buying ? '1' : '';
        tradeButtons.push(b);
        sec.appendChild(b);
      }
    }
  });

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
      // Врождённое кандидата — это и есть то, ради чего на него смотрят
      // (§12.42): опыт база доберёт работой, а предел даётся раз и навсегда.
      b.dataset.hint = statsHint(r.stats);
      recruitButtons.push(b);
      hire.appendChild(b);
    });
  }

  // Партия (§12.45). Автосохранение идёт само и молча, поэтому здесь только
  // то, что игрок решает сам: начать заново, унести партию файлом, принести
  // обратно и снять трейс.
  const game = mkSection(el, 'Партия');

  const fresh = mkTool('<span class="sw sw-cursor"></span><span>Новая партия</span>', () => {
    // Спрашиваем: действие разрушительное и необратимое — автосохранение
    // затрёт старую партию через десяток секунд.
    if (!confirm('Начать новую партию? Текущая будет потеряна.')) return;
    localStorage.removeItem(SAVE_KEY);
    worker.postMessage({ type: 'newGame' });
    // Темп сбрасывается вместе с базой: на ×10 первые сутки пролетают, пока
    // игрок читает записку, а на паузе новая партия выглядит сломанной.
    setSpeed(1);
  });
  fresh.title = 'Сбросить базу к началу';
  game.appendChild(fresh);

  const dump = mkTool('<span class="sw sw-scrap"></span><span>Сохранить в файл</span>', () =>
    worker.postMessage({ type: 'save' }),
  );
  dump.title = 'Скачать снимок партии';
  game.appendChild(dump);

  const picker = document.createElement('input');
  picker.type = 'file';
  picker.accept = '.json,application/json';
  picker.hidden = true;
  picker.addEventListener('change', async () => {
    const file = picker.files?.[0];
    if (!file) return;
    worker.postMessage({ type: 'load', json: await file.text() });
    picker.value = ''; // иначе тот же файл второй раз не выберется
  });
  const restore = mkTool('<span class="sw sw-scrap"></span><span>Загрузить файл</span>', () =>
    picker.click(),
  );
  restore.title = 'Открыть снимок партии';
  game.appendChild(restore);
  game.appendChild(picker);

  const trace = mkTool('<span class="sw sw-hire"></span><span>Скачать трейс</span>', () =>
    worker.postMessage({ type: 'trace' }),
  );
  trace.title = 'Журнал команд: как партия пришла в это состояние';
  game.appendChild(trace);

  // Раскрыт тот раздел, что был открыт до перестройки; на первом кадре это
  // палитра — с неё игра и начинается.
  openOnly(sections.some((s) => s.title === openSection) ? openSection : 'Постройка');
  selectCursor(cursorBtn); // режим по умолчанию
}

/// Курс фракции по предмету — из снапшота, где его посчитало ядро тем же
/// выражением, каким его посчитает заказ (§12.44). Второй арифметики цены в JS
/// быть не должно.
function quoteOf(faction, item) {
  return prices.find((p) => p.faction === faction && p.item === item);
}

// Доступность сделки: пост считает ядро, деньги — умножение уже названной им
// цены. Причину отказа называем словом: молчащая кнопка читается как поломка.
function syncTradeButtons() {
  for (const b of tradeButtons) {
    const fi = Number(b.dataset.faction);
    const ii = Number(b.dataset.item);
    const buying = !!b.dataset.buying;
    const q = quoteOf(fi, ii);
    if (!q) continue;
    const unit = buying ? q.buy : q.sell;
    const total = unit * 5;
    const broke = buying && money < total;
    const ready = hasPost && !dealRunning && !broke;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    const rate = b.querySelector('.rate');
    if (rate) rate.textContent = `${unit}¤`;
    // Расписание видно вперёд — это и есть разница между планированием и
    // караулом с секундомером (§12.40).
    const next = buying ? q.next_buy : q.next_sell;
    const ahead =
      q.next_in > 0 && next !== unit ? ` · через ${q.next_in} станет ${next}¤` : '';
    b.title = !hasPost
      ? 'Нужен «Торговый пост»'
      : dealRunning
        ? 'Сделка уже идёт'
        : broke
          ? `Нужно ${total}¤ за пять, у вас ${money}¤`
          : `${unit}¤ за штуку · клик — пять (${total}¤), Shift — двадцать пять${ahead}`;
  }
}

// Доступность кандидата считает ядро (известность + содержимое склада), здесь
// только показываем: дублировать правило в JS значит однажды показать кнопку,
// которую ядро отклонит.
function syncRecruitButtons(list) {
  recruitButtons.forEach((b, i) => {
    const r = (list ?? [])[i];
    if (!r) return;
    const ready = !r.hired && r.unlocked && r.welcome && r.affordable;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    // Своего присылают тем, кому доверяют (§12.43), и репутацией за него не
    // платят — платит склад. Поэтому причин отказа три и они разные.
    const distrust = r.welcome ? null : trustGap((meta.recruits ?? [])[i]?.needs);
    const why = r.hired
      ? 'Уже на базе'
      : !r.unlocked
        ? `Откликнется при известности ${b.dataset.requires}`
        : distrust
          ? distrust
          : !r.affordable
            ? 'На складе нечем заплатить'
            : 'Нанять';
    // Параметры называем и у закрытого кандидата: к нему идут заранее, и
    // «зачем мне этот кот» игрок спрашивает до того, как накопит.
    b.title = [b.dataset.hint, why].filter(Boolean).join(' · ');
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

/// Кто из фракций не доверяет базе настолько, чтобы дать этот заказ, — и
/// насколько не хватает. Само «дают или нет» решает ядро (`welcome`); здесь
/// только слова для игрока: молчащая кнопка читается как поломка (§12.24).
function trustGap(needs) {
  const factions = meta.factions ?? [];
  // `BTreeMap` из рулсета приезжает сюда как `Map`, а не как объект — тот же
  // случай, что у цены и добычи (см. `costChips`). `Object.entries` на нём молча
  // вернул бы пусто, и отказ остался бы без причины.
  const entries = needs instanceof Map ? [...needs.entries()] : Object.entries(needs ?? {});
  for (const [id, want] of entries) {
    const i = factions.findIndex((f) => f.id === id);
    if (i < 0) continue;
    const have = standing[i] ?? 0;
    if (have < want) {
      return `${factions[i].label || id} вам не доверяет: нужно ${want}, у вас ${have}`;
    }
  }
  return null;
}

function syncMissionButtons() {
  missionButtons.forEach((b, i) => {
    const need = Number(b.dataset.squad);
    const requires = Number(b.dataset.requires);
    // Открыта ли вылазка и есть ли у неё цель, решает ядро (§12.24): те же две
    // проверки стоят в `launch`, и считать их здесь во второй раз значит однажды
    // разойтись с фасадом. До первого снапшота ворот ещё нет — считаем закрытыми.
    const gates = raids[i];
    const known = !!gates?.unlocked;
    // За своим идут, только пока есть за кем: у вылазки с `rescue` нет ни
    // добычи, ни цели, если все дома.
    const nobody = !(gates?.possible ?? true);
    // Заказчик с базой не разговаривает (§12.43). Отдельно от известности:
    // «не дорос» и «эти вас не жалуют» — разные новости, и вторая обратима.
    const welcome = !!gates?.welcome;
    const distrust = welcome ? null : trustGap((meta.missions ?? [])[i]?.needs);
    // Раненого ядро в отряд не пустит (§12.37), и молчащая кнопка читалась бы
    // как поломка: причину называем словом, как и нехватку известности.
    const hurt = selectedUnits.filter((id) => wounded.has(id));
    // Пленный остаётся выбранным — на карте его нет, снять выделение игроку
    // нечем. Ядро такую заявку отклонит (его нет на базе), а молчащая кнопка
    // читается как поломка — причину называем словом, как и ранение.
    const gone = selectedUnits.filter((id) => captives.includes(id));
    const ready =
      !missionRunning &&
      known &&
      welcome &&
      !hurt.length &&
      !gone.length &&
      !nobody &&
      selectedUnits.length === need;
    b.disabled = !ready;
    b.classList.toggle('on', ready);
    // Закрытые вылазки видны, а не спрятаны: лестница ответственности — это то,
    // к чему игрок идёт, и невидимая цель не тянет (§4.4).
    b.title = missionRunning
      ? 'Вылазка уже идёт'
      : !known
        ? `${b.dataset.hint} · нужна известность ${requires}`
        : distrust
          ? `${b.dataset.hint} · ${distrust}`
          : nobody
            ? `${b.dataset.hint} · все дома, спасать некого`
            : gone.length
              ? `${b.dataset.hint} · в плену: ${gone.join(', ')}`
              : hurt.length
                ? `${b.dataset.hint} · ранен: ${hurt.join(', ')}`
                : `${b.dataset.hint} · выбрано ${selectedUnits.length} из ${need}`;
  });
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
