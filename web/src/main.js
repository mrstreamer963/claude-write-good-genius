// Главный поток: PixiJS-рендер + ввод игрока. Логики нет — рисуем данные из воркера
// и шлём команды (постройка тайлов, приказы движения).

import { Application, Container, Graphics, Text } from "pixi.js";

const TILE = 28;

const COLORS = {
  bg: 0x0e0f13,
  empty: 0x14161d, // непостроенная (непроходимая) ячейка
  gridLine: 0x262c3a,
  select: 0x6cf0a0, // выбор кота / метка цели / взведённая клетка
  cell: 0x9fb0ff, // осмотренная клетка: уголки вокруг того, о чём говорит панель
  erase: 0xff5566,
  // Хром режима стройки. Цвет самого тайла для этого не годится: тайлы тёмные
  // (пол — почти фон), и плашка с рамкой вокруг карты в нём не читались бы —
  // то есть режим снова стал бы тихим. Тайл показывается свотчем внутри плашки.
  build: 0x7fa6ff,
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

const stageEl = document.getElementById("stage");
const tickEl = document.getElementById("tick");
const scrapEl = document.getElementById("scrap");
const catEl = document.getElementById("cat");
const cellEl = document.getElementById("cell");
const missionEl = document.getElementById("mission");
const captiveEl = document.getElementById("captive");
const researchEl = document.getElementById("research");
const craftEl = document.getElementById("craft");
const dealEl = document.getElementById("deal");
const noteEl = document.getElementById("note");
const goalsEl = document.getElementById("goals");
const goalsToggleEl = document.getElementById("goals-toggle");
const finaleEl = document.getElementById("finale");
const toastsEl = document.getElementById("toasts");

// Кнопки внутри панелей вешаются **делегированием, один раз на контейнер**, и
// ловятся парой `mousedown`/`mouseup`, а не `click`.
//
// Панели перерисовываются каждым снапшотом (~16 мс) целиком, через `innerHTML`:
// прогресс тикает, и разметка честно меняется. Значит узел кнопки живёт один
// кадр, а человеческий клик длится сотню миллисекунд — между нажатием и
// отпусканием узел успевает смениться несколько раз.
//
// Делегирования на контейнер для этого **мало**, и это была вторая серия того
// же бага. Браузер шлёт `click` ближайшему общему предку нажатия и отпускания,
// то есть контейнеру, — но `event.target` у такого события это **контейнер**, а
// не кнопка, и проверка `target.closest('.mission-cancel')` не находит ничего.
// Обработчик молчал, кнопка «не работала».
//
// Поэтому нажатие и отпускание разбираются по отдельности: `mousedown` взводит
// (его цель — живой узел под курсором), `mouseup` спускает, если под курсором
// **свежий** узел той же кнопки. Семантика клика при этом цела: нажал на
// кнопке, увёл курсор, отпустил в стороне — ничего не произошло.
// Кнопок одного вида в панели может быть несколько (заказов теперь столько,
// сколько мастерских, §12.55), поэтому взводим не «нажали куда-то», а **чем**
// нажатое отличается от соседей: узлы живут один кадр, и сравнить их напрямую
// нельзя — сравниваем `data-def`. У панелей с единственной кнопкой он пуст, и
// поведение остаётся ровно прежним.
function onPanelClick(el, selector, send) {
  let armed = null;
  // Ключ строки: у отмен это заказ (`data-def`), у списка котов — сам кот
  // (`data-id`). Без него «нажал на одном, отпустил на другом» отдало бы
  // команду не тому: панель перерисовывается каждым кадром, и строки под
  // курсором успевают переехать.
  // `data-key` — для кнопок, у которых своего единственного ключа нет: в списке
  // отрядов (§12.66) один и тот же заказ стоит кнопкой у каждого узла, и по
  // `data-def` две такие кнопки поделили бы один взвод.
  const keyOf = (node) =>
    node?.dataset.key ?? node?.dataset.def ?? node?.dataset.id ?? "";
  el.addEventListener("mousedown", (e) => {
    const hit = e.target.closest(selector);
    armed = hit ? keyOf(hit) : null;
  });
  el.addEventListener("mouseup", (e) => {
    const hit = e.target.closest(selector);
    if (armed !== null && hit && keyOf(hit) === armed) send(hit);
    armed = null;
  });
}
// Отмена уходит по рецепту, а не по номеру строки: закрывшийся соседний заказ
// сдвинул бы номера под курсором игрока (§12.55).
onPanelClick(craftEl, ".craft-cancel", (node) =>
  sendAction({ type: "cancelCraft", recipe: Number(node.dataset.def) }),
);
// У заказа по порогу отменяют **правило**, а не заказ (§12.65): сам заказ ядро
// завело бы обратно тем же тиком.
onPanelClick(craftEl, ".keep-clear", (node) =>
  sendAction({ type: "setStock", recipe: Number(node.dataset.def), min: 0 }),
);
onPanelClick(researchEl, ".research-cancel", () =>
  sendAction({ type: "cancelResearch" }),
);
onPanelClick(missionEl, ".mission-cancel", (b) =>
  sendAction({ type: "cancelMission", mission: Number(b.dataset.def) }),
);
// Состав отряда узла (§12.61) и дежурство на связи (§12.60) — строки списка в
// панели клетки, и различаются они `data-id`, как отмены заказов различаются
// `data-def`: панель перерисовывается каждым кадром, и без ключа они поделили
// бы один взвод. `data-in` — «числится ли здесь»: одна кнопка на два состояния,
// потому что и вопрос один — про этого кота и этот узел.
onPanelClick(cellEl, ".crew-pick", (b) =>
  sendAction({
    type: b.dataset.in ? "dismiss" : "enlist",
    id: b.dataset.id,
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
onPanelClick(cellEl, ".crew-duty", (b) =>
  sendAction({
    type: b.dataset.in ? "unpostRelay" : "postRelay",
    id: b.dataset.id,
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);

const app = new Application();
await app.init({ background: COLORS.bg, antialias: true, resizeTo: stageEl });
stageEl.appendChild(app.canvas);

// Мир: тайлы -> лом -> чертежи -> юниты -> оверлей (подсветки).
const world = new Container();
const tileLayer = new Container();
const scrapLayer = new Container(); // кучи лома на полу
const bpLayer = new Container(); // чертежи (призраки будущих тайлов)
const dealLayer = new Container(); // контейнеры сделок в ячейках постов (§12.68)
const unitLayer = new Container();
const overlay = new Container();
world.addChild(tileLayer);
world.addChild(scrapLayer);
world.addChild(bpLayer);
world.addChild(dealLayer);
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
// Осмотренная клетка (§12.58): уголки, а не заливка, — заливкой рисуется
// `hoverRect`, и два одинаковых прямоугольника читались бы как один.
const cellMarker = new Graphics();
overlay.addChild(hoverRect);
overlay.addChild(orderMarker);
overlay.addChild(cellMarker);
overlay.addChild(selectionRings);

app.stage.eventMode = "static";
app.stage.hitArea = app.screen;

const units = new Map(); // id -> Container
const unitTiles = new Map(); // id -> { x, y } (в тайлах)
const orders = new Map(); // id -> { x, y } (заданная цель, для метки)

let meta = null; // { width, height, palette, items, skills, perks }
let paletteColors = []; // number[]
let itemColors = []; // number[] — цвет предмета по индексу палитры items
let mapCells = null; // Int-массив состояния карты
let mode = "cursor"; // 'cursor' | 'build' | 'store'
let buildTile = 0; // индекс палитры, или -1 = стереть (в режиме build)
let autoTidy = true; // коты сами свозят лом на склад (см. ядро, §12.16)
let autoRest = true; // и сами бросают работу на исходе сил (§12.33)
// Выбор множественный: отряд на вылазку игрок набирает поимённо (§12.23), а
// один выбранный кот — это его частный случай. Панель показывает последнего.
let selectedUnits = [];
// Клетка под последним кликом: о ней говорит панель, и она же взводит приказ —
// повторный клик по ней отправляет туда выбранных котов (§12.58).
let selectedCell = null; // { x, y } в тайлах
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
// Вылазок идёт столько, сколько узлов связи (§12.59). Оба числа считает ядро:
// `relayFree` — это ворота, и второй их экземпляр в JS однажды разойдётся с
// `launch`. `running` — заказы, по которым отряд уже вышел: двух вылазок по
// одному заказу не бывает, и гасить надо именно свою кнопку.
let relays = 0;
let relayFree = false;
// Узлы связи поимённо (§12.61). Состав отряда живёт на клетке рации, поэтому
// кнопка вылазки адресуется узлом, а не списком выделенных котов: с двумя узлами
// иначе не сказать, чей отряд идёт. Выделение котов при этом осталось осмотром и
// приказом — двух источников правды о составе быть не должно, и это ровно то,
// что §12.61 отверг.
//
// **Выбранного узла больше нет** (§12.66): раздел «Вылазки» показывает строку на
// каждый узел, и своя кнопка заказа стоит внутри строки. Узел, с которого уходят,
// — это та строка, в которой нажали, а не запомненный где-то в стороне клик.
let nodes = [];
// Идущие вылазки целиком: строке отряда надо сказать, чем занят **его** узел, а
// не только «занят». Тот же список читает и панель миссий.
let missionsOut = [];
let running = new Set();
let researchRunning = false; // и тема тоже одна за раз (§12.26)
// Мастерских может быть несколько, и заказов идёт столько же (§12.55).
// Нужен только для объяснения отказа: сами ворота считает ядро (`RecipeSnap.shop`).
let shops = 0;
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
// Имущество по типам: чем можно платить, что валяется и что забронировано
// (§12.53). Нужно не только шапке: отказ «на складе нечем заплатить» звучит
// издевательски, когда нужное лежит кучей в двух шагах, — и тогда подсказка
// обязана сказать, что делать.
let stock = [];
// Торговых постов может быть несколько, и сделок идёт столько же (§12.55).
// `postFree` считает ядро — это ворота, и второй их экземпляр в JS однажды
// покажет кнопку, которую фасад отклонит (§12.26).
let posts = 0;
let postFree = false;
// Открытые сделки — те же, что рисуются на карте и в панели. Кнопкам они нужны
// затем, что продать можно только непроданное: остаток считается по ним, а
// **не** по `stock.booked` (§12.50). `booked` — это бронь «в кучах», из неё уже
// вычтен груз в лапах, а `loose` его считает, — сложи их, и носильщик уйдёт в
// счёт дважды. Ровно на этом попались ворота самой заявки в ядре.
let openDeals = [];
// Зажат ли Shift: он удваивает не смысл кнопки, а её размер (пять штук против
// двадцати пяти), поэтому доступность и подпись обязаны следовать за клавишей.
// Молчащая кнопка читается как поломка — а «денег хватает на пять, но не на
// двадцать пять» без этого выглядело именно так.
let shiftHeld = false;
const tradeButtons = []; // кнопки сделок — гасим, когда все посты заняты (§12.55)
// Раздел вылазок — единственный, который перерисовывается целиком (§12.66):
// строка на отряд, а состав и занятость узла меняются каждым снапшотом. Массива
// живых кнопок у него поэтому нет — только контейнер.
let raidsEl = null;
const recruitButtons = []; // кнопки найма — гасим по известности и складу
const teachButtons = []; // кнопки обучения — живы, когда выбран ровно один кот
const topicButtons = []; // кнопки тем — гасим по технологиям, складу и допуску
const recipeButtons = []; // кнопки рецептов — гасим по технологии и мастерской
// Пороги автопроизводства в порядке палитры рецептов (§12.65). Число хранит
// ядро; здесь оно нужно кнопкам «−/+», чтобы знать, от чего отсчитывать.
let stocking = [];
const stockRows = []; // строки «держать N» — по строке на рецепт
const tileButtons = []; // кнопки палитры, закрытые технологией (§12.27)

// --- worker ---------------------------------------------------------------

const worker = new Worker(new URL("./worker.js", import.meta.url), {
  type: "module",
});

// Сохранение партии (§12.45). Слот один: снимок описывает мир целиком, а
// выбирать между слотами пока нечем — ни смерти, ни отката в игре нет.
const SAVE_KEY = "sp.save.v1";
// Продолжаем ровно один раз за загрузку страницы: `ready` приходит и после
// самой загрузки снимка, и повторная попытка была бы бесконечной.
let triedResume = false;

worker.onmessage = (e) => {
  const m = e.data;
  if (m.type === "ready") {
    meta = m.meta;
    paletteColors = meta.palette.map((p) => hex(p.color));
    itemColors = (meta.items ?? []).map((i) => hex(i.color));
    // Новая партия и поднятый снимок выделения не наследуют: панель клетки
    // говорила бы о старом мире, а взведённый приказ — о котах, которых нет.
    selectedCell = null;
    selectedUnits = [];
    // ...и цели тоже: у нового мира своя история взятого, и старая сделала бы
    // всё уже закрытое «только что закрытым» (см. `goalsDoneSeen`).
    goalsDoneSeen = null;
    buildToolbar();
    layout();
    drawMap(m.map);
    if (!triedResume) {
      triedResume = true;
      const saved = localStorage.getItem(SAVE_KEY);
      if (saved) worker.postMessage({ type: "load", json: saved });
    }
  } else if (m.type === "map") {
    drawMap(m.map);
  } else if (m.type === "snapshot") {
    renderSnapshot(m.snap);
  } else if (m.type === "saved") {
    if (m.auto) localStorage.setItem(SAVE_KEY, m.json);
    else download(`sp-save-${stamp()}.json`, m.json);
  } else if (m.type === "traced") {
    download(`sp-trace-${stamp()}.txt`, m.text);
  } else if (m.type === "loadFailed") {
    // Снимок не подошёл к текущим правилам. Держать его дальше незачем: он
    // не подойдёт и завтра, а базовая партия уже идёт.
    localStorage.removeItem(SAVE_KEY);
    showError(`Сохранение не загрузилось: ${m.message}. Партия начата заново.`);
  } else if (m.type === "error") {
    showError(m.message);
  }
};

// Уход со вкладки — момент, когда партию стоит дописать не дожидаясь таймера.
// Рефреш этим не покрыть: состояние держит воркер, а ответ асинхронный и до
// умирающей страницы уже не дойдёт — там работает короткий интервал автосейва.
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden")
    worker.postMessage({ type: "save", toSlot: true });
});

function stamp() {
  return new Date().toISOString().slice(0, 19).replaceAll(":", "-");
}

function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

function hex(s) {
  return parseInt(s.replace("#", ""), 16);
}

// --- layout / render ------------------------------------------------------

function layout() {
  if (!meta) return;
  world.x = Math.max(8, Math.floor((app.screen.width - meta.width * TILE) / 2));
  world.y = Math.max(
    8,
    Math.floor((app.screen.height - meta.height * TILE) / 2),
  );
}
app.renderer.on("resize", layout);

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
      g.circle(x + TILE / 2, y + TILE * 0.3, 2.5).fill({
        color: COLORS.select,
        alpha: 0.9,
      });
    }
  }
  scrapLayer.addChild(g);
}

// Сделка в ячейке торгового поста (§12.68). Словарь **один на обе стороны**:
// контур — «ячейка занята сделкой», заливка — «сколько уже здесь», цифра —
// «сколько тиков». Иначе покупка и продажа требовали бы двух значков, а игрок
// читал бы карту, только открыв панель.
//
// Покупка контейнер не наполняет: её заливка пустая до самого прибытия, а потом
// товар ложится обычной кучей и рисует его уже `drawScrap`. Продажа, наоборот,
// наполняется на глазах и уезжает целиком.
function drawDeals(list) {
  dealLayer.removeChildren();
  if (!list || !list.length) return;
  const g = new Graphics();
  for (const d of list) {
    const x = d.x * TILE;
    const y = d.y * TILE;
    const pad = TILE * 0.18;
    const side = TILE - pad * 2;
    const filled = d.buying ? 0 : d.count > 0 ? d.delivered / d.count : 0;
    if (filled > 0) {
      const h = side * Math.min(1, filled);
      g.rect(x + pad, y + pad + (side - h), side, h).fill({
        color: itemColor(d.item),
        alpha: 0.55,
      });
    }
    g.rect(x + pad, y + pad, side, side).stroke({
      width: 1.5,
      color: COLORS.select,
      alpha: 0.9,
    });
  }
  dealLayer.addChild(g);
  // Цифра — только когда срок реально идёт. У неполной продажи его нет вовсе
  // (§12.68), и ноль на карте читался бы как «вот-вот уедет».
  for (const d of list) {
    if (d.left <= 0) continue;
    const label = new Text({
      text: String(d.left),
      style: { fontFamily: "monospace", fontSize: 10, fill: COLORS.select },
    });
    label.anchor.set(0.5);
    label.x = d.x * TILE + TILE / 2;
    label.y = d.y * TILE + TILE / 2;
    dealLayer.addChild(label);
  }
}

function drawBlueprints(list) {
  bpLayer.removeChildren();
  if (!list || !list.length) return;
  const g = new Graphics();
  for (const b of list) {
    const x = b.x * TILE;
    const y = b.y * TILE;
    const isDemolish = b.tile < 0;
    const color = isDemolish
      ? COLORS.erase
      : (paletteColors[b.tile] ?? 0x888888);
    const supplied = b.delivered >= b.need;

    if (isDemolish) {
      // Снос: перечёркиваем существующий тайл, не пряча его под заливкой —
      // игрок должен видеть, что именно уйдёт.
      g.moveTo(x + 6, y + 6)
        .lineTo(x + TILE - 6, y + TILE - 6)
        .moveTo(x + TILE - 6, y + 6)
        .lineTo(x + 6, y + TILE - 6)
        .stroke({ color, width: 2, alpha: 0.9 });
      g.rect(x + 1, y + 1, TILE - 2, TILE - 2).stroke({
        color,
        width: 1,
        alpha: 0.6,
      });
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
      g.rect(x + 3, y + TILE - 6, TILE - 6, 3).fill({
        color: COLORS.scrap,
        alpha: 0.2,
      });
      if (m > 0) {
        g.rect(x + 3, y + TILE - 6, (TILE - 6) * m, 3).fill({
          color: COLORS.scrap,
          alpha: 0.95,
        });
      }
      continue;
    }
    // прогресс-бар работы
    const p = b.total > 0 ? Math.min(1, b.progress / b.total) : 0;
    if (p > 0) {
      g.rect(x + 3, y + TILE - 6, (TILE - 6) * p, 3).fill({
        color: COLORS.select,
        alpha: 0.95,
      });
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
  if (len <= 0) return "";
  const part = (tick % len) / len;
  const mins = Math.floor(part * 24 * 60);
  return `${String(Math.floor(mins / 60)).padStart(2, "0")}:${String(mins % 60).padStart(2, "0")}`;
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
  drawDeals(snap.deals);
  const seen = new Set();
  for (const e of snap.entities) {
    seen.add(e.id);
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
    const asleep = e.job === "rest" && !e.moving;
    const lying = e.job === "heal" && !e.moving;
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
    c.studyMark.visible = e.job === "study" && !e.moving;
    // Медик — только дошедший: лечит он с соседней клетки, и до неё ещё надо
    // добраться. Иначе пустой крест ехал бы через полбазы, обещая лечение.
    c.medicMark.visible = e.job === "treat" && !e.moving;
    unitTiles.set(e.id, { x: e.x, y: e.y });
  }
  // В шапке — только фишка и число: подписи распирают её, а цвет тот же, что
  // у куч на полу и у цены в палитре. Известность рядом: она не предмет, но
  // считается так же и решает, что базе вообще доступно.
  fame = snap.fame ?? 0;
  standing = snap.standing ?? [];
  money = snap.money ?? 0;
  prices = snap.prices ?? [];
  stock = snap.stock ?? [];
  openDeals = snap.deals ?? [];
  posts = snap.posts ?? 0;
  postFree = !!snap.post_free;
  shops = snap.shops ?? 0;
  relays = snap.relays ?? 0;
  relayFree = !!snap.relay_free;
  nodes = snap.nodes ?? [];
  scrapEl.innerHTML =
    (meta.items ?? [])
      .map((it, i) => {
        // Главное число — **чем можно платить**: склад минус бронь (§12.53).
        // Валяющееся приписано отдельно и приглушённо: оно у базы есть, но найм
        // и наука его не видят, и одно общее число ровно этим и обманывало.
        const st = (snap.stock ?? [])[i] ?? { stored: 0, loose: 0, booked: 0 };
        const free = Math.max(0, st.stored - st.booked);
        const name = esc(it.label || it.id);
        const hint = [
          `${name}: на складе ${st.stored}`,
          st.booked ? `забронировано ${st.booked} под сделку` : "",
          st.loose
            ? `валяется ${st.loose} — платить этим нельзя, пока не убрано`
            : "",
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          `<span class="stock" title="${hint}">` +
          `<i class="chip" style="background:${it.color}"></i><b>${free}</b>` +
          (st.loose ? `<u>+${st.loose}</u>` : "") +
          "</span>"
        );
      })
      .join(" ") +
    `<span class="fame" title="Известность">★<b>${fame}</b></span>` +
    // Репутация рядом с известностью, но врозь: та отвечает «насколько высоко»
    // и только копится, эта — «от кого» и ходит в обе стороны (§12.43). Знак
    // пишем всегда: «0» и «−0» читаются одинаково, а «+20» и «−20» — нет.
    (meta.factions ?? [])
      .map((f, i) => {
        const v = standing[i] ?? 0;
        const sign = v > 0 ? `+${v}` : `${v}`;
        return (
          `<span class="standing${v < 0 ? " bad" : ""}" title="${esc(f.label || f.id)}">` +
          `<i class="chip" style="background:${f.color}"></i><b>${sign}</b></span>`
        );
      })
      .join("") +
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
    snap.entities
      .filter((e) => e.health_max > 0 && e.health <= e.health_hurt)
      .map((e) => e.id),
  );
  captives = snap.entities.filter((e) => e.captive).map((e) => e.id);
  raids = snap.raids ?? [];
  missionsOut = snap.missions ?? [];

  updateSelectionOverlay();
  renderCatPanel(snap.entities);
  // После цикла по сущностям: панель читает `unitTiles`, и он обновлён выше.
  renderCellPanel(snap);
  renderCaptivePanel();
  renderMissionPanel(snap.missions);
  // После `renderMissionPanel`: он считает `running` — заказы, по которым отряд
  // уже вышел, — а строка отряда обязана гасить именно свою кнопку (§12.59).
  renderRaidsSection();
  renderResearchPanel(snap.research);
  renderCraftPanel(snap.crafting);
  renderDealPanel(snap.deals);
  syncRecruitButtons(snap.recruits);
  syncTopicButtons(snap.topics);
  syncRecipeButtons(snap.recipes);
  syncStockRows(snap.stocking, snap.recipes);
  syncTileButtons(snap.techs);
  renderNotePanel(snap.notes, snap.tick);
  renderGoalsPanel(snap.goals, snap.goals_required, snap);
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
  stuckRing
    .circle(0, 0, TILE * 0.52)
    .stroke({ color: COLORS.stuck, width: 2, alpha: 0.9 });
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

  drawCellMarker();
}

// Уголки вокруг осмотренной клетки (§12.58). Взведённая — тем же зелёным, что
// кольца выбора и метка цели: «сюда пойдут» на карте и в панели обязано быть
// одной краской, иначе второй шаг приказа приходится угадывать.
function drawCellMarker() {
  cellMarker.clear();
  if (!selectedCell) return;
  const x = selectedCell.x * TILE;
  const y = selectedCell.y * TILE;
  const armed = cellIsArmed();
  const arm = TILE * 0.3; // длина уголка
  const p = 1.5; // отступ внутрь, чтобы уголки не сливались с сеткой
  for (const [cx, cy, dx, dy] of [
    [x + p, y + p, 1, 1],
    [x + TILE - p, y + p, -1, 1],
    [x + p, y + TILE - p, 1, -1],
    [x + TILE - p, y + TILE - p, -1, -1],
  ]) {
    cellMarker
      .moveTo(cx + dx * arm, cy)
      .lineTo(cx, cy)
      .lineTo(cx, cy + dy * arm);
  }
  cellMarker.stroke({
    color: armed ? COLORS.select : COLORS.cell,
    width: 2,
    alpha: armed ? 0.95 : 0.75,
  });
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
    case "away":
      return "";
    case "heal":
      return going ? "идёт в лазарет" : "лежит: раны заживают";
    case "eat":
      return going ? "идёт есть" : "ест";
    // Приказ спящего не поднимает, пока включено «Беречь себя» (§12.51), — и
    // молчать об этом нельзя: игрок только что кликнул, а кот не двинулся.
    // Знание тут местное, из тех же `orders`, которыми рисуется метка цели:
    // ядру про «кому уже приказали» рассказывать нечего (§12.41).
    case "rest":
      if (going) return "идёт спать";
      return orders.has(e.id) ? "спит: приказ подождёт" : "спит";
    // Дремота — не сон: кот выспался до потолка места и добирает бодрость, пока
    // базе нечем его занять (§12.52). Поднимает его что угодно, и сказать это
    // надо прямо: иначе «спит» и «дремлет» игрок прочтёт одинаково и решит, что
    // приказ то работает, то нет.
    case "nap":
      return "дремлет: поднимется сразу";
    case "treat":
      return going ? "идёт к раненому" : "лечит раненого";
    case "equip":
      return going ? "идёт за снаряжением" : "снаряжается";
    case "squad":
      return going ? "идёт к шлюзу" : "ждёт отряд";
    case "haul":
      return e.carrying > 0
        ? `несёт ${itemLabel(e.carrying_item)}`
        : "идёт за грузом";
    case "research":
      return going ? "идёт в лабораторию" : "исследует";
    case "craft":
      return going ? "идёт в мастерскую" : "работает в мастерской";
    case "study":
      return going ? "идёт к парте" : "учится";
    case "relay":
      return going ? "идёт к рации" : "держит связь с отрядом";
    case "build":
      return going ? "идёт на площадку" : "строит";
    case "demolish":
      return going ? "идёт на снос" : "разбирает";
    // Приказ без маршрута — это и есть `stuck`, и он подписан отдельно.
    case "order":
      return going ? "идёт по приказу" : "";
    default:
      return "без дела";
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
    parts.push(
      `<div class="cat-sub">выбрано ${selectedUnits.length}: ${selectedUnits.map(esc).join(" · ")}</div>`,
    );
  }
  // Занятие — первой строкой и до всех шкал: «чем он вообще занят» игрок
  // спрашивает раньше, чем «сколько у него бодрости». Застрявший объясняется
  // здесь же: `stuck` — состояние легальное, но кот из него сам не выйдет.
  const job = e.stuck ? "не может дойти" : jobLabel(e);
  if (job)
    parts.push(`<div class="cat-job${e.stuck ? " stuck" : ""}">${job}</div>`);
  // Врождённое — до навыков: оно объясняет их пределы, а не наоборот (§12.42).
  // Опыт кот доберёт работой, а эти числа даны ему навсегда, и ровно поэтому
  // коты остаются разными после того, как бригада выработалась.
  const stats = (meta.stats ?? [])
    .map((st, i) => `${esc(st.label || st.id)} ${e.stats?.[i] ?? 0}`)
    .join(" · ");
  if (stats) parts.push(`<div class="cat-sub">${stats}</div>`);
  for (let i = 0; i < defs.length; i++) {
    const s = e.skills?.[i];
    if (!s) continue;
    // Нулевой навык, за который ещё не капнуло ни очка опыта, — это не факт о
    // коте, а пустая строка: доменов будет много, и список из них скрывает те,
    // что игроку действительно интересны. Появится опыт — появится и полоска.
    if (s.level === 0 && s.xp === 0) continue;
    const levels = defs[i].levels ?? [];
    const from = s.level > 0 ? levels[s.level - 1] : 0;
    // Врождённый предел — это не потолок навыка: полоска, вставшая на месте,
    // обязана назвать причину, иначе игрок прочтёт её как поломку (§12.42).
    const capped = s.cap > 0 && s.level >= s.cap;
    const born = capped && s.cap < levels.length;
    // next = 0 — навык на потолке: полоска полная, порога дальше нет.
    const pct =
      capped || s.next <= from
        ? 100
        : Math.round(((s.xp - from) / (s.next - from)) * 100);
    const note = born
      ? `предел: ${esc(statLabel(defs[i].stat))} ${e.stats?.[statIndex(defs[i].stat)] ?? 0}`
      : capped || s.next <= 0
        ? "потолок"
        : `${s.xp} / ${s.next}`;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${esc(defs[i].label || defs[i].id)}</span><b>${s.level}</b></div>` +
        `<div class="bar"><i class="${born ? "capped" : ""}" style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">${note}</div>` +
        "</div>",
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
      ? "на исходе сил: бросит работу"
      : e.energy <= e.energy_tired
        ? "устал: доработает и пойдёт спать"
        : "";
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Бодрость</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${spent ? "spent" : "rest"}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : "") +
        "</div>",
    );
  }
  // Сытость — вторая потребность (§12.36). Цена голода списывается с бодрости,
  // а не со шкалы рядом, поэтому «голоден» надо назвать словом: иначе игрок
  // видит, что коты всё время спят, и не связывает это с пустым складом.
  if (e.fed_max > 0) {
    const pct = Math.round((e.fed / e.fed_max) * 100);
    const starving = e.fed <= 0;
    const note = starving
      ? "голодает: бодрость горит вдвое"
      : e.fed <= e.fed_hungry
        ? "проголодался"
        : "";
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Сытость</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${starving ? "starving" : "fed"}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : "") +
        "</div>",
    );
  }
  // Здоровье — третья шкала (§12.37). Её роняет только провал вылазки, поэтому
  // просевшая полоска всегда означает «этот кот только что вернулся с плохой
  // вылазки», а порог надо назвать словом: ниже него кота не берут в отряд, и
  // без подписи игрок прочитает молчащую кнопку как поломку.
  if (e.health_max > 0) {
    const pct = Math.round((e.health / e.health_max) * 100);
    const hurt = e.health <= e.health_hurt;
    const note = hurt
      ? "ранен: не работает и в отряд не идёт"
      : e.health < e.health_max
        ? "царапины"
        : "";
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Здоровье</span><b>${pct}%</b></div>` +
        `<div class="bar"><i class="${hurt ? "hurt" : "health"}" style="width:${pct}%"></i></div>` +
        (note ? `<div class="cat-sub">${note}</div>` : "") +
        "</div>",
    );
  }
  // Пленный — тоже «нет на базе», но по таймеру он не вернётся: за ним надо
  // сходить. Разные слова здесь — это разные решения игрока (§12.40).
  if (e.captive)
    parts.push('<div class="cat-sub">в плену: нужна вылазка за своим</div>');
  else if (e.away) parts.push('<div class="cat-sub">на вылазке</div>');
  // Надетое: снаряжение молча прибавляет отряду силы, и без этой строки игрок
  // не свяжет пропавший со склада комбинезон с выросшим прогнозом вылазки
  // (§12.29). Пустой комплект показываем тоже — иначе непонятно, что он бывает.
  const gear = e.gear ?? [];
  const force = gear.reduce(
    (sum, i) => sum + ((meta.items ?? [])[i]?.force ?? 0),
    0,
  );
  parts.push(
    '<div class="cat-sub">' +
      (gear.length
        ? `надето: ${gear.map((i) => esc(itemLabel(i))).join(" · ")} (+${force} к силе)`
        : "не экипирован") +
      "</div>",
  );
  const held = e.carrying > 0 ? ` ${esc(itemLabel(e.carrying_item))}` : "";
  const paws =
    (e.carry_max > 0
      ? `лапы ${e.carrying}/${e.carry_max}`
      : `в лапах ${e.carrying}`) + held;
  const tags = (e.perks ?? []).map((id) => esc(perkLabel(id)));
  parts.push(`<div class="cat-sub">${[paws, ...tags].join(" · ")}</div>`);
  catEl.innerHTML = parts.join("");
  catEl.hidden = false;
}

// Роль клетки словами: чем она полезна сверх цвета. Свойства тайла — та самая
// схема «комната значит что-то сверх цвета» (§12.35), и все девять приезжают в
// `meta.palette` целиком, так что второго списка в ядре заводить не пришлось.
function tileRoles(def) {
  if (!def) return [];
  const roles = [];
  // Склада в списке нет намеренно: о нём говорит полоска «Занято N / C», и
  // тайл, который и назван «Склад», не должен трижды повторять это слово.
  if (def.rest > 0) roles.push("лежанка");
  if (def.heal > 0) roles.push("койка лазарета");
  if (def.gate) roles.push("шлюз: отсюда уходят на вылазку");
  if (def.teaches) roles.push(`парта: учит «${esc(skillLabel(def.teaches))}»`);
  if (def.lab) roles.push("лаборатория");
  if (def.shop) roles.push("мастерская");
  if (def.trade) roles.push("торговый пост");
  if (def.relay) roles.push("узел связи: держит одну вылазку");
  if (def.solid) roles.push("стеллаж: пройти можно, остаться нельзя");
  return roles;
}

// Раздел тулбара, к которому относится клетка (§12.55). Клик по мастерской
// раскрывает «Производство», по посту — рынок: игрок попадает откуда надо куда
// надо, а **управление остаётся в одном месте**.
//
// Кнопки на самой клетке при этом не появляется, и это не осторожность. Ядро не
// адресует работу месту: `shop_spot` и `lab_spot` берут ближайший **свободный**
// станок к тому коту, которого выбрала симуляция, а игрок размечает работу и не
// выбирает исполнителя (§12.16). Кнопка «заказать здесь» обещала бы адресность,
// которой нет, — и с двумя мастерскими врала бы через раз. Адресация понадобится
// тогда, когда станки перестанут быть взаимозаменяемыми (тиры по скорости или
// свои рецепты) — вот тогда и вернуться.
function cellSection(def) {
  if (!def) return null;
  if (def.shop) return "Производство";
  if (def.lab) return "Наука";
  if (def.teaches) return "Обучение";
  if (def.gate || def.relay) return "Вылазки";
  // У рынка раздел на фракцию, и какая из них «эта клетка» — неизвестно: пост
  // лицензия, а не прилавок (§12.44). Открываем первый — он же обычно и один.
  if (def.trade) {
    const fac = (meta.factions ?? [])[0];
    return fac ? `Рынок: ${fac.label || fac.id}` : null;
  }
  return null;
}

// Панель клетки (§12.58). Клетка была единственным, о чём игрок не мог спросить:
// кот объясняется карточкой, вылазка и заказ — своими панелями, а «что тут
// лежит», «сколько ещё влезет» и «почему тут ничего не строится» читалось только
// по трём чипсам на карте.
//
// Ядру она ничего не стоила: и кучи, и площадки, и свойства тайлов уже ехали в
// снапшоте — панель их только складывает. Считать что-нибудь сверх этого ей
// нельзя (§12.26): «чем можно платить» и «сколько влезет» — правила, и живут они
// в одном месте с `plan_spend` и `less_incoming`.
function renderCellPanel(snap) {
  if (!selectedCell || !meta || !mapCells) {
    cellEl.hidden = true;
    return;
  }
  const { x, y } = selectedCell;
  const tile = mapCells[y * meta.width + x];
  const def = tile >= 0 ? meta.palette[tile] : null;
  const name = def ? def.label || def.id : "Пустота";
  const parts = [
    `<div class="cat-name">${esc(name)} <span class="cell-at">${x}, ${y}</span></div>`,
  ];

  // Что случится по второму клику — до всего остального: приказ двухшаговый, и
  // невидимый второй шаг читается как «клик не сработал» (§4.4).
  if (cellIsArmed()) {
    parts.push(
      `<div class="cell-armed">ещё клик сюда — пойдут: ${selectedUnits.map(esc).join(" · ")}</div>`,
    );
  } else if (cellReleases()) {
    parts.push('<div class="cell-armed">ещё клик — снять выделение</div>');
  }

  if (!def) {
    parts.push('<div class="cat-sub">непроходима: коты её не пересекут</div>');
  } else {
    const roles = tileRoles(def);
    if (roles.length)
      parts.push(`<div class="cat-sub">${roles.join(" · ")}</div>`);
  }

  // Кучи на клетке — то, ради чего панель и заводилась. Разных типов на одной
  // клетке лежит сколько угодно, и сливаться они не должны (§12.21).
  const piles = (snap.stacks ?? []).filter((s) => s.x === x && s.y === y);
  const held = piles.reduce((sum, s) => sum + s.count, 0);
  // Ёмкость считает **штуки** независимо от типа (§12.21), поэтому сумма
  // законна. Но это «лежит столько», а не «свободно столько»: свободное место
  // ядро считает с поправкой на груз в пути (`less_incoming`), и обещать его
  // панель не вправе.
  if (def?.capacity > 0) {
    const pct = Math.min(100, Math.round((held / def.capacity) * 100));
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>Занято</span><b>${held} / ${def.capacity}</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        "</div>",
    );
  }
  if (piles.length) {
    const chips = piles
      .map(
        (s) =>
          `<i class="chip" style="background:${(meta.items ?? [])[s.item]?.color ?? "#c9a227"}"></i>` +
          `${esc(itemLabel(s.item))} ${s.count}`,
      )
      .join(" · ");
    parts.push(`<div class="cat-sub">${chips}</div>`);
    // Пометка «на склад» объясняет, почему за кучей кто-то придёт, — а при
    // включённой автоуборке помечено всё, что лежит вне склада.
    if (piles.some((s) => s.marked))
      parts.push('<div class="cat-sub">помечено на склад</div>');
  } else if (def) {
    parts.push('<div class="cat-sub">пусто</div>');
  }

  // Площадка — прямой ответ на «почему тут ничего не строится»: пока материал
  // не завезли, работа и не начнётся (§12.15).
  const bp = (snap.blueprints ?? []).find((b) => b.x === x && b.y === y);
  if (bp) {
    const what =
      bp.tile < 0
        ? "Снос"
        : `Стройка: ${esc(meta.palette[bp.tile]?.label || meta.palette[bp.tile]?.id || "?")}`;
    const supplied = bp.delivered >= bp.need;
    const pct = supplied
      ? bp.total > 0
        ? Math.min(100, Math.round((bp.progress / bp.total) * 100))
        : 0
      : bp.need > 0
        ? Math.round((bp.delivered / bp.need) * 100)
        : 100;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${what}</span><b>${pct}%</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">${supplied ? "материал на месте" : `завезено ${bp.delivered} из ${bp.need}`}</div>` +
        "</div>",
    );
  }

  // Что здесь идёт **сейчас**. Без этого функциональная клетка называла себя
  // («мастерская») и замолкала, а работа в ней шла молча — та же беда, из-за
  // которой заводили панели темы и заказа (§12.30, §12.41).
  for (const line of cellWork(snap, x, y, def)) {
    parts.push(`<div class="cat-sub">${line}</div>`);
  }

  // Кто стоит. Клетку коты делят на проходе (§12.32), а на паузе видно только
  // верхнего — из-за чего и разошлись показания в первом баге про лапы.
  const here = unitsAt(x, y);
  if (here.length)
    parts.push(
      `<div class="cat-sub">здесь: ${here.map(esc).join(" · ")}</div>`,
    );

  // Состав отряда и дежурство — списком всех котов базы, прямо здесь, у рации
  // (§12.60, §12.61). Раньше и то и другое собиралось «выдели кота на карте →
  // кликни узел → нажми кнопку»: три клика на кота, два разных выделения и
  // никакого способа перекинуть кота с узла на узел, не бегая по карте. Игрок
  // читал это как «вылазки сломались» — кнопка молчит, а как набрать состав, из
  // панели не видно. Список отвечает на «кого послать» там же, где спрашивают.
  //
  // Выделение котов на карте в наборе больше не участвует: осмотр остался
  // осмотром. Второго пути к тому же действию нет намеренно — два источника
  // правды в UI, и игрок не знает, какой сработал (тот же довод, по которому
  // §12.61 отверг «оба способа разом» в ядре).
  if (def?.relay) parts.push(...crewList(snap, x, y));

  cellEl.innerHTML = parts.join("");
  cellEl.hidden = false;
}

// Список котов узла связи: кто в его отряде, кто свободен, кто числится на
// другом узле (§12.61). Строка — переключатель, как чертёж в тулбаре: клик по
// своему вычёркивает, клик по чужому или свободному зачисляет сюда.
//
// Перенос с чужого узла — это тот же `enlist`, а не пара «вычеркни там →
// зачисли здесь»: ядро снимает прежнюю приписку молча, и «переназначить
// туда-сюда» стоит один клик. Отказывать было бы хуже — игрок не видит, где кот
// числился раньше, и кнопка молчала бы без объяснения.
//
// Порядок — три группы, внутри каждой по `id`: свои, свободные, чужие. По `id`,
// а не по обходу снапшота, ровно по той же причине, по какой сортирует
// `roster_of`, — список игрок читает глазами, и он не должен переставляться сам
// собой от кадра к кадру.
function crewList(snap, x, y) {
  const node = nodeAt(x, y);
  const here = (e) => e.crew_x === x && e.crew_y === y;
  // Ушедших нет на базе (§12.40): в отряд их не зачислить, а пленных объясняет
  // своя панель. Показывать их строкой значило бы предлагать невозможное.
  const cats = (snap.entities ?? []).filter((e) => !e.away);
  const rank = (e) => (here(e) ? 0 : e.crew_x < 0 ? 1 : 2);
  cats.sort((a, b) => rank(a) - rank(b) || (a.id < b.id ? -1 : 1));

  const rows = cats.map((e) => {
    const mine = here(e);
    // Раненого ядро в отряд не пустит (§12.37), и молчащая строка читалась бы
    // как поломка: причину называем словом, как её называет кнопка вылазки.
    const hurt = wounded.has(e.id);
    // Пока узел ведёт вылазку, состав уже в поле и не переигрывается: для этого
    // есть отзыв (`cancel_mission`), а не правка списка.
    const off = hurt || !!node?.busy;
    const where = mine || e.crew_x < 0 ? "" : ` · узел ${e.crew_x},${e.crew_y}`;
    const note = hurt ? "ранен" : jobLabel(e) || "";
    const pick =
      `<button class="tool crew-pick${mine ? " on" : ""}" data-id="${esc(e.id)}"` +
      ` data-x="${x}" data-y="${y}"${mine ? ' data-in="1"' : ""}${off ? " disabled" : ""}>` +
      `<span class="crew-id">${esc(e.id)}${where}</span>` +
      (note ? `<i class="crew-note">${esc(note)}</i>` : "") +
      "</button>";
    // Дежурство — вторая кнопка той же строки: бонус отряду даёт та же клетка,
    // и разводить эти два решения по разным местам панели незачем. На узле без
    // `comms` дежурить незачем, и кнопки там нет вовсе (§12.60).
    if (!node?.comms) return `<div class="crew-row">${pick}</div>`;
    const on = e.post_x === x && e.post_y === y;
    const duty =
      `<button class="tool crew-duty${on ? " on" : ""}" data-id="${esc(e.id)}"` +
      ` data-x="${x}" data-y="${y}"${on ? ' data-in="1"' : ""}` +
      ` title="${on ? "Снять приписку к рации" : "Приписать к рации: сядет на связь, как освободится"}">📻</button>`;
    return `<div class="crew-row">${pick}${duty}</div>`;
  });

  return [
    `<div class="cat-sub">Отряд узла · ${node?.crew.length ?? 0}</div>`,
    `<div class="crew-list">${rows.join("")}</div>`,
  ];
}

// Что происходит в этой клетке прямо сейчас — строками, в порядке её свойств.
//
// Станок и лаборатория опознаются **по координатам заказа**, а не по «идёт ли
// вообще заказ»: мастерских теперь несколько (§12.55), и «здесь делают деталь»
// на пустом соседнем верстаке было бы враньём ровно того сорта, каким врала
// шапка до §12.53.
function cellWork(snap, x, y, def) {
  if (!def) return [];
  const out = [];
  if (def.shop) {
    const order = (snap.crafting ?? []).find((c) => c.x === x && c.y === y);
    if (order) {
      const name = esc(recipeLabel(order.def));
      const who = order.unit
        ? `, работает ${esc(order.unit)}`
        : ", мастер идёт";
      out.push(`делают: ${name}, осталось ${order.left} шт${who}`);
    } else {
      out.push("станок свободен — заказы в разделе «Производство»");
    }
  }
  if (def.lab) {
    const topic = (snap.research ?? [])[0];
    out.push(
      topic
        ? `тема: ${esc(topicLabel(topic.def))}${topic.unit ? `, работает ${esc(topic.unit)}` : ""}`
        : "тем нет — их берут в разделе «Наука»",
    );
  }
  if (def.gate) {
    const m = (snap.missions ?? []).find((v) => v.x === x && v.y === y);
    if (m)
      out.push(
        m.away
          ? `отряд в поле, вернётся через ${m.left}`
          : "здесь собирается отряд",
      );
  }
  if (def.trade) {
    // §12.68: пост — **место**, а не лицензия, и за ним по-прежнему никто не
    // работает. Сказать надо и то, и другое: иначе игрок либо ждёт у поста
    // кота, либо не понимает, почему ячейка занята.
    const busy = (snap.deals ?? []).length;
    out.push("торговая ячейка: за ней не работают, к ней возят");
    out.push(`сделок идёт ${busy} из ${posts} — по одной на ячейку`);
    // Что лежит в контейнере — ровно то, ради чего клик по ячейке и нужен.
    // Сделать с этим ничего нельзя: товар уже продан и ждёт отгрузки.
    const here = (snap.deals ?? []).find((d) => d.x === x && d.y === y);
    if (here) {
      const item = (meta.items ?? [])[here.item];
      const name = esc(item?.label || item?.id || "товар");
      out.push(
        here.buying
          ? `ждёт поставку: ${name} ${here.count} шт, ${here.left} тиков`
          : `в контейнере: ${name} ${here.delivered} из ${here.count}` +
              (here.left > 0 ? ` · уедет через ${here.left}` : " · набирают"),
      );
    } else if ((snap.stacks ?? []).some((s) => s.x === x && s.y === y)) {
      // Привезённое занимает ячейку, пока его не увезут (§12.68). Без этой
      // строки затор выглядит поломкой, а не работой, которую надо доделать.
      out.push("ячейку занимает привезённое — пока не вывезут, пост занят");
    }
  }
  if (def.relay) {
    // §12.59 дал узлу слот, §12.60 — смысл сидеть за ним. Говорим и то, и
    // другое: сколько вылазок держат узлы вообще и что происходит на **этой**
    // рации сейчас, — иначе игрок ждёт у неё кота и решает, что всё сломалось.
    const busy = (snap.missions ?? []).length;
    out.push(`вылазок идёт ${busy} из ${relays} — по одной на узел`);
    // Состав живёт на клетке и переживает вылазку (§12.61): сказать его надо
    // здесь же, иначе кнопка вылазки берёт отряд ниоткуда.
    const node = nodeAt(x, y);
    out.push(
      node?.crew.length
        ? `отряд: ${node.crew.map(esc).join(" · ")}`
        : "отряд не набран: отметьте котов в списке ниже",
    );
    out.push("вылазки отсюда — строкой этого отряда в разделе «Вылазки»");
    const raid = (snap.missions ?? []).find(
      (v) => v.node_x === x && v.node_y === y,
    );
    if (!raid) {
      out.push("свободен: отряд отсюда не выходил");
    } else {
      const label = missionLabel(raid.def);
      out.push(
        raid.away
          ? `ведёт «${esc(label)}» · вернутся через ${raid.left}`
          : `закреплён за «${esc(label)}» — отряд ещё собирается`,
      );
      if (raid.away) {
        const on = (snap.entities ?? []).find(
          (e) => e.job === "relay" && !e.moving && e.x === x && e.y === y,
        );
        out.push(
          on
            ? `на связи ${esc(on.id)} · +${raid.comms} к силе отряда`
            : "связи нет: за рацией никто не сидит",
        );
      }
    }
  }
  return out;
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
    `<div class="cat-sub">${captives.map(esc).join(" · ")}</div>` +
    '<div class="cat-sub">Сам не вернётся — нужна вылазка за своим</div>';
  captiveEl.hidden = false;
}

// Панель миссии. Пока отряд собирается, показываем состав: игрок не выбирает,
// кого послать (§12.22), — значит должен хотя бы видеть, кого выбрала за него
// симуляция и почему база вдруг перестала строить.
function renderMissionPanel(list) {
  const raidsOut = list ?? [];
  // Идущие вылазки — по заказу, а не по счёту: кнопка гасится именно у своей
  // (§12.59), двух вылазок по одному заказу не бывает.
  running = new Set(raidsOut.map((m) => m.def));
  if (!raidsOut.length || !meta) {
    missionEl.hidden = true;
    return;
  }
  const parts = [];
  // Сколько слотов занято — там же, где видно сами вылазки. Число узлов считает
  // ядро (§12.59): «вылазок меньше, чем узлов» вторым экземпляром в JS однажды
  // разойдётся с `launch`.
  if (relays > 1) {
    parts.push(
      `<div class="cat-name">Вылазки · ${raidsOut.length} из ${relays}</div>`,
    );
  }
  for (const m of raidsOut) parts.push(missionCard(m));
  missionEl.innerHTML = parts.join("");
  missionEl.hidden = false;
}

// Одна карточка вылазки. Их столько, сколько узлов связи (§12.59), поэтому у
// кнопки отмены обязателен `data-def`: `onPanelClick` различает одинаковые
// кнопки только по нему, а без него все отмены поделят один пустой ключ.
function missionCard(m) {
  const def = (meta.missions ?? [])[m.def];
  const parts = [
    `<div class="cat-name">${esc(def?.label || def?.id || "Вылазка")}</div>`,
  ];
  if (m.away) {
    const pct =
      m.total > 0 ? Math.round(((m.total - m.left) / m.total) * 100) : 0;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>В пути</span><b>${pct}%</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        "</div>",
    );
  } else {
    // Спящего бойца заявка не поднимает, пока включено «Беречь себя» (§12.51),
    // а сбор ничем не ограничен по времени: «собираются у шлюза» под отрядом,
    // который никуда не идёт, читается как поломка.
    const gathering = m.resting
      ? "Ждут, пока выспится боец"
      : "Собираются у шлюза";
    parts.push(`<div class="cat-sub">${gathering}</div>`);
  }
  parts.push(
    `<div class="cat-sub">${m.squad.map(esc).join(" · ") || "—"}</div>`,
  );
  // Связь (§12.60). Число — **накопленное**, то есть что будет, если связь
  // оборвётся прямо сейчас: она копится за тик, а не меряется одним замером,
  // и прогноз честно растёт вместе с ней. Говорим и то, держат ли её сейчас, —
  // иначе просевший на возвращении бонус выглядел бы необъяснимым.
  if (m.away) {
    const link = m.manned
      ? "связь держат"
      : m.comms > 0
        ? "связь оборвалась"
        : "связи нет";
    const gain = m.comms > 0 ? ` · +${m.comms} к силе` : "";
    parts.push(`<div class="cat-sub">${link}${gain}</div>`);
  }
  // Прогноз исхода: его считает ядро тем же выражением, которым исход
  // посчитается на возвращении (§12.23). Пока отряд на базе — это ещё и
  // предупреждение: увидел «провал», успел отозвать.
  if (m.danger > 0) {
    // Раны считаются той же долей, что и добыча (§12.37), поэтому цену провала
    // можно назвать здесь же — из того самого числа, которым ядро её посчитает.
    const harm = Math.round(((def?.harm ?? 0) * (100 - m.share)) / 100);
    const wounds = harm > 0 ? `, раны ${harm}` : "";
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
        `<div class="bar"><i class="${m.failed ? "fail" : ""}" style="width:${m.failed ? 100 : m.share}%"></i></div>` +
        `<div class="cat-sub">${verdict}</div>` +
        "</div>",
    );
  }
  // Цена решения — рядом с прогнозом добычи и тем же числом, которым ядро её
  // посчитает на возвращении (§12.43). Это главное, что панель обязана сказать
  // до клика: закрывшиеся ворота честны ровно постольку, поскольку игрок видел,
  // чем платит. У провала здесь ноль — и это тоже новость.
  if (m.patron >= 0 || m.against >= 0) {
    const name = (f) => esc((meta.factions ?? [])[f]?.label || "—");
    const moves = [];
    if (m.patron >= 0) moves.push(`${name(m.patron)} +${m.standing}`);
    if (m.against >= 0) moves.push(`${name(m.against)} −${m.standing}`);
    parts.push(
      '<div class="cat-skill">' +
        '<div class="cat-row"><span>Репутация</span></div>' +
        `<div class="cat-sub">${moves.join(" · ")}</div>` +
        "</div>",
    );
  }
  // Отозвать можно только тех, кто ещё на базе: ушедший отряд симуляции уже
  // не подчиняется — вылазка считается разом по возвращении.
  if (!m.away) {
    parts.push(
      `<button class="tool mission-cancel" data-def="${m.def}"><span>Отозвать</span></button>`,
    );
  }
  return `<div class="cat-skill">${parts.join("")}</div>`;
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
    `<div class="cat-name">${esc(def?.label || def?.id || "Тема")}</div>`,
    '<div class="cat-skill">' +
      `<div class="cat-row"><span>Изучено</span><b>${pct}%</b></div>` +
      `<div class="bar"><i style="width:${pct}%"></i></div>` +
      "</div>",
    // Пусто — исполнитель ещё не нашёлся: тема ждёт, а не идёт. Разница
    // важная, и в полоске её не видно.
    `<div class="cat-sub">${r.unit ? esc(r.unit) : "ждёт исполнителя"}</div>`,
    '<button class="tool research-cancel"><span>Бросить</span></button>',
  ];
  researchEl.innerHTML = parts.join("");
  researchEl.hidden = false;
}

// Панель заказов. Полоска у каждого — про **текущую штуку**, а не про весь
// заказ: работа и оплата идут поштучно, и «40% от пяти» игрок прочтёт неверно
// (§12.30).
//
// Заказов теперь столько, сколько мастерских (§12.55), поэтому это список.
// Отмена уходит **по рецепту**, а не по номеру строки: список приезжает
// отсортированным по рецепту, но номер строки поедет, как только соседний заказ
// закроется, — а игрок к этому моменту уже целился в кнопку.
function renderCraftPanel(list) {
  const orders = list ?? [];

  if (!orders.length || !meta) {
    craftEl.hidden = true;
    return;
  }
  const parts = [
    `<div class="cat-name">Заказы${orders.length > 1 ? ` · ${orders.length}` : ""}</div>`,
  ];
  for (const c of orders) {
    const def = (meta.recipes ?? [])[c.def];
    const pct = c.total > 0 ? Math.round((c.progress / c.total) * 100) : 0;
    // Три разных «ничего не происходит», и путать их нельзя: некому взяться,
    // нечем платить или работа идёт.
    const state = c.unit
      ? esc(c.unit)
      : c.paid
        ? "ждёт исполнителя"
        : "ждёт материала";
    // Заказ правила отменяют **снятием порога**, а не «Отменить»: правило завело
    // бы его обратно тем же тиком, и кнопка читалась бы как поломка (§12.65).
    const button = c.auto
      ? `<button class="tool keep-clear" data-def="${c.def}"><span>Снять порог</span></button>`
      : `<button class="tool craft-cancel" data-def="${c.def}"><span>Отменить</span></button>`;
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${esc(def?.label || def?.id || "Заказ")}</span><b>${pct}%</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">осталось ${c.left} шт · ${state}${c.auto ? " · по порогу" : ""}</div>` +
        button +
        "</div>",
    );
  }
  craftEl.innerHTML = parts.join("");
  craftEl.hidden = false;
}

// Сделка (§12.44). Показываем **зафиксированный** курс, а не сегодняшний:
// рассчитаются именно по нему, а расписание за это время могло уйти — в этом и
// весь риск торговли. Кнопки «Отменить» здесь нет намеренно: деньги за покупку
// уже ушли, и возврат превратил бы сделку в бесплатный опцион.
function renderDealPanel(list) {
  const deals = list ?? [];
  syncTradeButtons();
  if (!deals.length || !meta) {
    dealEl.hidden = true;
    return;
  }
  // Сколько окон занято из скольких: постов теперь может быть несколько, и
  // «почему кнопка не жмётся» игрок должен читать здесь, а не гадать (§12.55).
  const head = posts > 1 ? `Сделки · ${deals.length} из ${posts}` : "Сделка";
  const parts = [`<div class="cat-name">${head}</div>`];
  for (const d of deals) {
    const item = (meta.items ?? [])[d.item];
    const who = (meta.factions ?? [])[d.faction];
    const name = esc(item?.label || item?.id || "товар");
    const rows = [
      `<div class="cat-row"><span>${d.buying ? "Покупка" : "Продажа"}: ${name}</span></div>`,
      `<div class="cat-sub">${esc(who?.label || "—")} · ${d.count} шт по ${d.unit} = ${d.unit * d.count}¤</div>`,
    ];
    if (d.buying) {
      rows.push(
        `<div class="cat-sub">в пути ${d.left} — приедет в ячейку ${d.x},${d.y}</div>`,
      );
    } else if (d.left > 0) {
      // Контейнер набит и уехал: срок пошёл, деньги придут разом по отгрузке
      // (§12.68). До этого момента не заплачено ничего.
      rows.push(
        `<div class="cat-sub">отгружено · расчёт через ${d.left}</div>`,
      );
    } else {
      // Пока набирают, срока нет вовсе: его меряют ходки котов, а не тики.
      // Показываем сделанное — и **не** показываем денег: их ещё нет.
      const pct = d.count > 0 ? Math.round((d.delivered / d.count) * 100) : 0;
      rows.push(
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
          `<div class="cat-sub">в контейнере ${d.delivered} из ${d.count} · ячейка ${d.x},${d.y}</div>`,
      );
    }
    parts.push(`<div class="cat-skill">${rows.join("")}</div>`);
  }
  dealEl.innerHTML = parts.join("");
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
          ? "сегодня"
          : until === 1
            ? "завтра"
            : `через ${days(until)}`;
    const stamp = today ? `день ${dayOf(n.at)} — ` : "";
    const parts = [
      `<div class="cat-row"><span>${esc(n.label)}</span><b>${when}</b></div>`,
      `<div class="cat-sub">${stamp}${esc(n.detail || n.hint)}</div>`,
    ];
    // Требование показываем, только пока событие впереди: после срока важно уже
    // не «чего не хватало», а чем всё кончилось.
    if (!n.done && n.revealed && n.requires.length) {
      const needs = n.requires.map(techLabel).join(" · ");
      parts.push(
        n.ready
          ? `<div class="cat-sub good">готовы: ${esc(needs)}</div>`
          : `<div class="cat-sub warn">нужно: ${esc(needs)}</div>`,
      );
    }
    return `<div class="row${n.done ? " past" : ""}">${parts.join("")}</div>`;
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
        (today ? `день ${dayOf(last)} — ` : "") +
        `дальше дат нет: база живёт вслепую</div>` +
        `</div>`,
    );
  }

  noteEl.innerHTML = `<div class="cat-name">Записка</div>${rows.join("")}`;
  noteEl.hidden = false;
}

// --- цели партии (§12.58) ---------------------------------------------------
//
// Панель открыта с начала: цель партии — это первое, что игрок должен увидеть,
// иначе он опять в песочнице без запроса. Прячется кнопкой в шапке, счёт на
// которой виден всегда.
//
// Ни ярлыков, ни подписей в снапшоте нет — только индексы: тексты приезжают
// один раз в `meta.goals`, как рецепты и темы. Скрытой невзятой цели в снапшоте
// нет вовсе, и это решает **ядро**: прятать её здесь значило бы объявить её в
// devtools (§12.28).
let goalsOpen = true;
// Что было закрыто в прошлом кадре — множеством, а не счётчиком: по нему
// ловятся **оба** перехода, и полнота набора (финал), и каждая отдельная цель
// (уведомление). Два разных «что было в прошлом кадре» однажды разъехались бы.
//
// `null` — кадра **этого мира** ещё не было. Отсюда же и то, что своего флага
// «уже показано» ни у финала, ни у уведомлений нет: первый снапшот мира только
// засевает множество, перехода на нём не случается, и не всплывает ничего.
// Игрока не поздравляют повторно с тем, что он сделал вчера.
//
// Считать «первым кадром» первый кадр **страницы** оказалось мало: воркер
// поднимает новую партию сразу, а снимок приезжает следом (см. `ready`), и её
// пустыми целями засевался счёт — на фоне которого всё взятое в снимке
// выглядело только что сделанным. Поэтому сброс висит на `ready`: мир сменился —
// прошлого кадра нет.
let goalsDoneSeen = null;

goalsToggleEl.addEventListener("click", () => {
  goalsOpen = !goalsOpen;
  goalsEl.hidden = !goalsOpen;
});

function goalDef(def) {
  return (meta.goals ?? [])[def] ?? {};
}

function renderGoalsPanel(goals, required, snap) {
  if (!goals?.length) {
    goalsToggleEl.hidden = true;
    goalsEl.hidden = true;
    return;
  }
  goalsToggleEl.hidden = false;

  // В счёт идут только обязательные: взятая скрытая раздула бы знаменатель, и
  // «8 / 7» игрок прочтёт как поломку.
  const done = goals.filter((g) => g.done && !g.hidden).length;
  goalsToggleEl.textContent = `цели ${done}/${required}`;
  goalsToggleEl.classList.toggle("done", done >= required);

  const row = (g) => {
    const def = goalDef(g.def);
    const label = esc(def.label || def.id || "?");
    if (g.done) {
      const when = dayOf(g.at) ? `день ${dayOf(g.at)}` : "✓";
      return `<div class="row past"><div class="cat-row"><span>${label}</span><b>${when}</b></div></div>`;
    }
    // Счётчик показываем только там, где есть что мерить: у двоичной цели
    // «0 / 1» — это шум, а не сведения.
    const meter = g.need > 1 ? `<b>${g.have} / ${g.need}</b>` : "";
    return (
      `<div class="row"><div class="cat-row"><span>${label}</span>${meter}</div>` +
      `<div class="cat-sub">${esc(def.hint || "")}</div></div>`
    );
  };

  const open = goals.filter((g) => !g.hidden);
  const extra = goals.filter((g) => g.hidden); // сюда попадают только взятые
  const rows = open.map(row);
  if (extra.length) {
    rows.push(
      '<div class="cat-sub goals-extra">сверх того</div>',
      ...extra.map(row),
    );
  }
  goalsEl.innerHTML = `<div class="cat-name">Цели</div>${rows.join("")}`;
  goalsEl.hidden = !goalsOpen;

  // Оба перехода считаются по одному множеству — тому, что было в прошлом кадре.
  const doneNow = new Set(goals.filter((g) => g.done).map((g) => g.def));
  const first = goalsDoneSeen === null;
  const fresh = first
    ? []
    : goals.filter((g) => g.done && !goalsDoneSeen.has(g.def));
  // Финал — по **переходу** к полноте, а не по факту полноты: иначе он всплывал
  // бы каждым кадром после закрытия.
  const finale = !first && goalsDoneSeen.size < required && done >= required;

  if (finale) showFinale(goals, snap);
  // Уведомления **и о скрытых тоже**: взятая скрытая цель — это ровно тот момент,
  // ради которого её прятали, и промолчать о нём значит спрятать её насовсем.
  // А вот вместе с финалом их не показываем: модал уже перечисляет всё разом, и
  // семь всплывающих поверх него — это шум, а не сведения.
  if (!finale) fresh.forEach((g) => showGoalToast(g));

  goalsDoneSeen = doneNow;
}

// --- уведомление о взятой цели ----------------------------------------------
//
// Живёт **в реальных секундах, а не в тиках**, и это главное в нём: на ×10 тик
// длится 16 мс, и отмеренное тиками уведомление мигнуло бы и пропало. Ядру
// wall-clock запрещён (§11: любой недетерминизм ломает и тесты, и модель
// времени), но здесь вид — ему часы мира не указ, и на паузе уведомление точно
// так же честно досчитает своё и уйдёт.
const TOAST_MS = 7000;

function showGoalToast(goal) {
  const def = goalDef(goal.def);
  const node = document.createElement("div");
  node.className = "toast";
  node.innerHTML =
    `<div class="toast-kind">${goal.hidden ? "скрытая цель" : "цель закрыта"}</div>` +
    `<div class="toast-label">${esc(def.label || def.id || "?")}</div>`;

  // Уходит либо само, либо по клику — но убирается **одним** путём: иначе клик
  // по уже угасающему уведомлению снимал бы его дважды.
  let done = false;
  const close = () => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    node.classList.add("leaving");
    // Ждём конца перехода, а не таймером на ту же длительность: второй таймер
    // разъехался бы с CSS при первой же правке анимации.
    node.addEventListener("transitionend", () => node.remove(), { once: true });
  };
  const timer = setTimeout(close, TOAST_MS);
  node.addEventListener("click", close);

  toastsEl.appendChild(node);
  // Класс появления вешаем следующим кадром: навешанный сразу, он совпал бы с
  // вставкой узла, и браузер не увидел бы перехода — уведомление возникало бы
  // рывком.
  requestAnimationFrame(() => node.classList.add("shown"));
}

/// Единственный модальный экран в игре — и он **не кончает партию**.
///
/// Закрыл и играешь дальше: §10 отказывает MVP в финальном акте Воланда, а тут
/// кончилось обучение, как у записки кончилось предзнание, а не мир (§12.46).
function showFinale(goals, snap) {
  const cats = snap.entities.filter((e) => e.unit).length;
  const scrap = snap.stock?.[0];
  const stored = scrap ? scrap.stored + scrap.loose : 0;
  const day = dayOf(snap.tick);
  const lines = goals
    .filter((g) => !g.hidden)
    .map((g) => {
      const def = goalDef(g.def);
      const when = dayOf(g.at) ? `день ${dayOf(g.at)}` : "✓";
      return `<div class="cat-row"><span>${esc(def.label || def.id)}</span><b>${when}</b></div>`;
    })
    .join("");
  finaleEl.innerHTML =
    `<div class="finale-box"><div class="finale-title">База состоялась</div>` +
    `<div class="cat-sub">всё, что было задумано, база умеет</div>` +
    `<div class="finale-list">${lines}</div>` +
    `<div class="cat-sub">` +
    (day ? `день ${day} · ` : "") +
    `${cats} кот(ов) · ${stored} лома</div>` +
    `<div class="cat-sub">дальше целей нет — база живёт как хочет</div>` +
    `<button class="finale-close">Играть дальше</button></div>`;
  finaleEl.hidden = false;
  // Модал ставит время на паузу: итог читают, а не догоняют глазами на ×10.
  // Своего «запомненного темпа» не заводим — `lastSpeed` уже значит ровно это
  // («тот темп, к которому возвращает пробел»), и `setSpeed(0)` его не затирает.
  // Второй такой памяти хватило бы, чтобы однажды разойтись с пробелом.
  //
  // Игрок, поставивший паузу сам, сюда не попадёт: цели отмечает `check_goals`,
  // а он тикает вместе с миром — на паузе закрыться нечему.
  setSpeed(0);
}

// Модал живёт вне потока панелей и не перерисовывается каждым кадром, поэтому
// обычный `click` тут уместен — узел под курсором не сменится (ср. §12.55).
finaleEl.addEventListener("click", (e) => {
  if (!e.target.closest(".finale-close") && e.target !== finaleEl) return;
  finaleEl.hidden = true;
  // Возвращаем тот темп, на котором игрока застал финал, — как это делает пробел.
  setSpeed(lastSpeed);
});

// Рецепт и тема — по индексу палитры, как предмет: их `def` в снапшоте это
// номер записи, а не имя (в отличие от технологии).
function recipeLabel(def) {
  const d = (meta.recipes ?? [])[def];
  return d?.label || d?.id || "?";
}

// Название вылазки по индексу палитры — панели узла и миссии говорят о ней
// одним и тем же словом.
function missionLabel(def) {
  const d = (meta.missions ?? [])[def];
  return d?.label || d?.id || "вылазка";
}

function topicLabel(def) {
  const d = (meta.research ?? [])[def];
  return d?.label || d?.id || "?";
}

function techLabel(id) {
  const def = (meta.research ?? []).find((r) => r.id === id);
  return def?.label || id;
}

function itemLabel(item) {
  const def = (meta.items ?? [])[item];
  return def?.label || def?.id || "?";
}

function perkLabel(id) {
  const def = (meta.perks ?? []).find((p) => p.id === id);
  return def?.label || id;
}

// Парта хранит `id` навыка, а не его номер (§12.18) — отсюда поиск по имени, а
// не индексация, как у перков и технологий.
function skillLabel(id) {
  const def = (meta.skills ?? []).find((s) => s.id === id);
  return def?.label || id;
}

// Врождённые параметры кандидата словами: «Ум 4 · Реакция 9 · Выносливость 6».
// serde-wasm-bindgen отдаёт отображение из YAML настоящим `Map`, как и цену.
function statsHint(stats) {
  const entries =
    stats instanceof Map ? [...stats.entries()] : Object.entries(stats ?? {});
  if (!entries.length) return "";
  return (meta.stats ?? [])
    .map((st) => {
      const found = entries.find(([id]) => id === st.id);
      return found ? `${st.label || st.id} ${found[1]}` : "";
    })
    .filter(Boolean)
    .join(" · ");
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
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c],
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

// Все коты на клетке — для панели. `unitAt` берёт первого и остаётся как есть:
// он в горячем пути `updateHover`, на каждом движении мыши. Порядок `unitTiles`
// идёт из порядка сущностей ECS и для показа недетерминирован, поэтому сортируем
// по имени: список, который сам себя перетасовывает, читается как мельтешение.
function unitsAt(tx, ty) {
  const found = [];
  for (const [id, ut] of unitTiles)
    if (ut.x === tx && ut.y === ty) found.push(id);
  return found.sort();
}

// Узел связи в этой клетке — или `null`. Список приходит из ядра (§12.61):
// второй экземпляр правила «где узлы» в JS однажды разойдётся с картой.
function nodeAt(tx, ty) {
  return (nodes ?? []).find((n) => n.x === tx && n.y === ty) ?? null;
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
  if (mode === "store") worker.postMessage({ type: "store", ...rect });
  else worker.postMessage({ type: "build", ...rect, tile: buildTile });
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
      // Тот же первый шаг, что и у клика курсором (§12.58): клетка показана, мир
      // не тронут. Клетку выставляем **до** выбора кота — перерисовку оверлея
      // зовёт `selectUnit`, и она должна увидеть уже обе половины выделения.
      selectedCell = { x: rect.x, y: rect.y };
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
  else if (selectedUnits.includes(id))
    selectedUnits = selectedUnits.filter((u) => u !== id);
  else selectedUnits.push(id);
  updateSelectionOverlay();
}

// Режим курсора. Приказ здесь **двухшаговый** (§12.58): первый клик по клетке
// только показывает, что на ней (и выбирает кота, если тот там стоит), а
// отправляет туда котов повторный клик по **той же** клетке.
//
// Так дороже ровно то, чем пользуются редко. Приказ в этой игре — намеренная
// оговорка к единственному правилу ввода (§12.16: «игрок размечает работу,
// исполнителя выбирает симуляция»), и в петле игры его нет вовсе: уборка,
// снаряжение, сон и дремота на котах, остальное — рамки и кнопки. А осмотр
// клетки нужен постоянно, и он теперь ничего не стоит и ничем не грозит.
//
// Таймера двойного клика нет намеренно: «повторный» здесь значит «клетка уже
// выбрана», и передумать можно сколько угодно долго — как и не спешить.
function command(global, add) {
  const t = tileAt(global);
  if (!t) return;
  const same =
    selectedCell && selectedCell.x === t.tx && selectedCell.y === t.ty;
  // Панель следует за кликом всегда, даже когда тот же клик отдаёт приказ:
  // иначе клетку под котом не осмотреть вовсе — клик по ней выбирает кота, и
  // получается замкнутый круг.
  selectedCell = { x: t.tx, y: t.ty };
  revealSection(t.tx, t.ty);
  const hit = unitAt(t.tx, t.ty);
  // Shift — чистый набор отряда, обеими руками: приказа он не даёт никогда, и
  // клетку под собой не взводит. Иначе шаг «набрал троих» пришлось бы считать
  // первым кликом, и четвёртый Shift-клик угонял бы отряд.
  if (add) {
    if (hit) selectUnit(hit, true);
    else updateSelectionOverlay();
    return;
  }
  if (!same) {
    if (hit) selectUnit(hit);
    else updateSelectionOverlay();
    return;
  }
  if (cellReleases()) {
    selectedUnits = [];
  } else if (isWalkable(t.tx, t.ty)) {
    // Приказ уходит каждому выбранному: коты друг друга не блокируют, и толпа
    // на одной клетке — законное состояние (см. `set_target` в ядре).
    for (const id of selectedUnits) {
      worker.postMessage({ type: "move", id, x: t.tx, y: t.ty });
      orders.set(id, { x: t.tx, y: t.ty });
    }
  }
  updateSelectionOverlay();
}

// Клик по функциональной клетке раскрывает её раздел тулбара (§12.55). Это
// **только вид**: ни одной команды отсюда не уходит, и решение по-прежнему
// принимается в одном месте — просто игрок туда попадает, ткнув в комнату.
//
// Открываем на первом клике, а не на втором: осмотр и есть «покажи, что тут», а
// второй клик занят приказом.
function revealSection(tx, ty) {
  if (!mapCells || !meta) return;
  const tile = mapCells[ty * meta.width + tx];
  const section = tile >= 0 ? cellSection(meta.palette[tile]) : null;
  // Раздела нет — раскрывать нечего, и уже открытый не трогаем: захлопывать
  // палитру на каждый клик по полу значит отбирать у игрока инструмент.
  if (section && sections.some((s) => s.title === section)) openOnly(section);
}

// Что сделает повторный клик по выбранной клетке. Обе половины считаются одним
// местом на троих читателей — сам клик, уголки на карте и строка в панели: то,
// что игрок видит обещанным, и то, что он получает, обязано быть одним
// выражением, иначе обещание однажды разойдётся с делом (§4.4).
//
// Отпустит — если единственный выбранный кот стоит на этой самой клетке: слать
// его туда, где он уже есть, нечего.
function cellReleases() {
  if (!selectedCell || selectedUnits.length !== 1) return false;
  return unitAt(selectedCell.x, selectedCell.y) === selectedUnits[0];
}

// Взведена — если по клетке есть кого отправить и есть куда идти.
function cellIsArmed() {
  if (!selectedCell || !selectedUnits.length) return false;
  return isWalkable(selectedCell.x, selectedCell.y) && !cellReleases();
}

function cssColor(n) {
  return `#${n.toString(16).padStart(6, "0")}`;
}

// Как выглядит текущий режим ввода: подпись, цвет хрома (плашка и рамка вокруг
// карты) и `tint` — цвет самого размечаемого тайла. Единственное место, где это
// решается: хром, курсор и подсветка клетки обязаны говорить одно и то же,
// иначе игрок сверяет два разных сигнала.
//
// Хром и `tint` разведены намеренно: на карте рамка обязана быть цветом того,
// что игрок ставит (это ответ на «что именно»), а плашка — заметной (ответ на
// «я в режиме»), и тёмный пол в этой роли не работает.
function modeChrome() {
  if (mode === "store") {
    return { key: "store", label: "НА СКЛАД", color: COLORS.scrap, tint: null };
  }
  if (mode === "build") {
    if (buildTile < 0) {
      return { key: "erase", label: "СНОС", color: COLORS.erase, tint: null };
    }
    const p = meta?.palette?.[buildTile];
    return {
      key: "build",
      label: `СТРОЙКА: ${p ? p.label || p.id : buildTile}`,
      color: COLORS.build,
      tint: paletteColors[buildTile] ?? COLORS.build,
    };
  }
  return { key: "cursor", label: "", color: COLORS.select, tint: null };
}

// Красит окно под режим: плашка над картой, обводка самого поля и курсор. Режим
// липкий (после мазка не сбрасывается), поэтому «я всё ещё в стройке» обязано
// читаться до клика — периферийным зрением и там, куда игрок смотрит.
function applyModeChrome() {
  const m = modeChrome();
  stageEl.classList.remove(
    "mode-cursor",
    "mode-build",
    "mode-erase",
    "mode-store",
  );
  stageEl.classList.add(`mode-${m.key}`);
  stageEl.style.setProperty("--mode-color", cssColor(m.color));
  // Разметка модальна: пока она идёт, «управлять» нечем. Признак висит на
  // `body`, а не на тулбаре, потому что правые панели ему не родня, а гасить их
  // надо тем же переключателем. Панели перерисовываются каждым снапшотом
  // целиком, поэтому запрет обязан жить в CSS: любой список «что отключить»
  // пришлось бы восстанавливать каждый кадр.
  document.body.classList.toggle("marking", m.key !== "cursor");
  const banner = document.getElementById("mode-banner");
  if (!banner) return;
  // Свотч тайла — тем же приёмом, что и в кнопках тулбара: подпись говорит, что
  // ставим, а квадратик — каким это будет на карте.
  banner.innerHTML = m.tint
    ? `<span class="sw" style="background:${cssColor(m.tint)}"></span>${m.label}`
    : m.label;
}

function updateHover(global) {
  const t = tileAt(global);
  hoverRect.clear();
  // Во время протяжки показываем всю рамку — даже если курсор ушёл за карту.
  const r = dragFrom
    ? rectOf(dragFrom, dragTo)
    : t && { x: t.tx, y: t.ty, w: 1, h: 1 };
  if (!r) return;
  // Одна клетка с котом под ней подсвечивается как выбор, а не как разметка:
  // отпустив кнопку здесь, игрок выберет кота, и цвет обязан сказать это до
  // клика, а не после.
  const overUnit =
    r.w === 1 && r.h === 1 && (dragFrom ? dragUnit : t && unitAt(t.tx, t.ty));
  const m = modeChrome();
  const col = overUnit ? COLORS.select : (m.tint ?? m.color);
  hoverRect
    .rect(r.x * TILE, r.y * TILE, r.w * TILE, r.h * TILE)
    .fill({ color: col, alpha: 0.28 })
    .stroke({ color: col, width: 3, alpha: 0.9 });
}

app.stage.on("pointerdown", (e) => {
  if (mode === "cursor") {
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
app.stage.on("pointermove", (e) => {
  const t = tileAt(e.global);
  if (dragFrom && t) dragTo = t;
  updateHover(e.global);
});
app.stage.on("pointerup", (e) => endDrag(true, e.global));
// Курсор ушёл со сцены — применяем последнюю рамку в пределах карты: бросать
// уже нарисованное выделение обиднее, чем применить его на клетку меньше.
app.stage.on("pointerupoutside", (e) => endDrag(true, e.global));

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

// Снять выделение целиком: и клетку, и котов.
function clearSelection() {
  selectedCell = null;
  selectedUnits = [];
  updateSelectionOverlay();
}

// Команда, отданная мимо карты, — это «я закончил с этой клеткой»: отряд ушёл,
// сделка заказана, кот нанят — подсветка клетки и её панель уже про прошлое.
//
// Снимается **только клетка и только на действии**, меняющем мир. Две границы,
// и обе намеренные. Переключить раздел тулбара или заглянуть в «Правила» — это
// осмотр, такой же, как сам выбор клетки, и терять от него выделение обидно;
// поэтому правило живёт в отправке команды, а не в общем слушателе документа.
// А выбор котов переживает действие: он дороже клика по карте (Shift-набор
// отряда) и нужен подряд — поучить, отправить, снарядить. Снимает его целиком
// только Escape.
//
// Приказ котам (`move`) сюда не входит: он уходит с карты, и клетка, в которую
// пошли, — ровно то, что игрок сейчас и разглядывает.
// Приписка связиста (§12.60) — тоже исключение, и по той же причине, что
// приказ: команда **про эту самую клетку**, и её результат игрок разглядывает
// здесь же. Спрятать панель под кнопкой, которую только что нажали, — это
// «кнопка не сработала» в чистом виде: подтверждения не видно.
const KEEPS_CELL = new Set([
  "move",
  "postRelay",
  "unpostRelay",
  "enlist",
  "dismiss",
]);

function sendAction(msg) {
  worker.postMessage(msg);
  if (KEEPS_CELL.has(msg.type)) return;
  selectedCell = null;
  updateSelectionOverlay();
}

// Shift держат, а не нажимают: кнопки сделок обязаны перерисоваться на зажатие
// и на отпускание. `blur` здесь не перестраховка — отпустят клавишу в другом
// окне, и кнопка навсегда останется «×25».
function setShift(on) {
  if (shiftHeld === on) return;
  shiftHeld = on;
  syncTradeButtons();
}

window.addEventListener("keydown", (e) => {
  if (e.key === "Shift") setShift(true);
  if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.code === "Escape" || e.key === "Escape") {
    if (dragFrom) {
      endDrag(false);
      return;
    }
    // Дальше Escape отменяет ровно одно — и начинает с самого навязчивого.
    // Режим разметки липкий (§12.62) и держит на прицеле каждый клик по карте,
    // поэтому выходит первым; выделение подождёт второго нажатия.
    if (mode !== "cursor") {
      selectCursor(cursorBtn);
      return;
    }
    // Режима нет — снимаем выделение целиком: и клетку, и котов. Клика,
    // который снимал бы выбор с пустого места, в двухшаговой модели нет
    // (§12.58): любой клик по карте что-нибудь да выбирает.
    clearSelection();
    return;
  }
  if (e.code === "Space" || e.key === " ") {
    // Иначе пробел «нажимает» кнопку в фокусе — а это может оказаться вылазка
    // или найм: клавиша одна, а цена ошибки разная.
    e.preventDefault();
    setSpeed(speed > 0 ? 0 : lastSpeed);
    return;
  }
  const speedKey = SPEED_KEYS[e.code] ?? SPEED_KEYS["Digit" + e.key];
  if (speedKey) setSpeed(speedKey);
});

window.addEventListener("keyup", (e) => {
  if (e.key === "Shift") setShift(false);
});
window.addEventListener("blur", () => setShift(false));

// --- тулбар ---------------------------------------------------------------

// Цена тайла: по цветной фишке на каждый нужный предмет. Порядок — как в
// палитре предметов, чтобы он совпадал со счётчиками в шапке (в самой цене
// он алфавитный: в рулсете это отображение).
function costChips(cost) {
  // serde-wasm-bindgen отдаёт YAML-отображение настоящим `Map`, а не объектом:
  // цена приходит как `Map { "scrap" => 1 }`.
  const entries =
    cost instanceof Map ? [...cost.entries()] : Object.entries(cost ?? {});
  if (!entries.length) return "";
  const chips = (meta.items ?? [])
    .map((it) => {
      const found = entries.find(([id]) => id === it.id);
      return found
        ? `<i class="chip" style="background:${it.color}"></i>${found[1]}`
        : "";
    })
    .filter(Boolean)
    .join(" ");
  return `<span class="cost">${chips}</span>`;
}

// Разделы тулбара: раскрыт ровно один. Инструментов стало на четыре механики
// больше, чем помещается в экран, и список уезжал под записку — а листать
// скроллом то, чем пользуешься каждые пять секунд, хуже, чем один клик.
const sections = [];
// Какой раздел открыт, помним между перестройками: иначе после каждого
// возвращения к палитре её пришлось бы раскрывать заново.
let openSection = "Постройка";

// Разделы-инструменты: они переключают режим разметки, поэтому в самом режиме
// остаются живыми (§12.62). Всё остальное в тулбаре и в правых панелях — это
// «управлять», а не «размечать», и на время разметки глушится: рука уже на
// карте, и клик мимо инструмента почти всегда промах, а не намерение.
// «Лом» здесь вместе с «Постройкой» намеренно: рамка на склад — та же разметка,
// и гонять игрока через курсор между двумя рамками было бы налогом на ровном
// месте.
const TOOL_SECTIONS = new Set(["Постройка", "Лом"]);

function mkSection(el, title) {
  const sec = document.createElement("div");
  if (!TOOL_SECTIONS.has(title)) sec.classList.add("gated");
  const head = document.createElement("button");
  head.className = "sec-head";
  head.innerHTML = `<span>${esc(title)}</span><span class="chev">›</span>`;
  head.addEventListener("click", () => openOnly(title));
  const body = document.createElement("div");
  body.className = "sec-body";
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
    s.head.classList.toggle("active", on);
    s.body.hidden = !on;
  }
}

function buildToolbar() {
  const el = document.getElementById("toolbar");
  el.innerHTML = "";
  sections.length = 0;

  // Курсор — вне разделов: это не инструмент в ряду прочих, а состояние «ничего
  // не размечаю», и оно нужно из любого раздела.
  cursorBtn = mkTool(
    '<span class="sw sw-cursor"></span><span>Курсор</span>',
    () => selectCursor(cursorBtn),
  );
  el.appendChild(cursorBtn);

  const build = mkSection(el, "Постройка");

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

  const er = mkTool(
    '<span class="sw sw-erase"></span><span>Стереть</span>',
    () => selectBuild(-1, er),
  );
  build.appendChild(er);

  const scrap = mkSection(el, "Лом");

  // Разметка уборки рамкой: повторный жест по помеченному снимает пометку.
  // Кот не выбирается — задачу возьмёт любой свободный.
  const st = mkTool(
    '<span class="sw sw-scrap"></span><span>На склад</span>',
    () => selectStore(st),
  );
  scrap.appendChild(st);

  // Правила симуляции — не режимы ввода, а тумблеры поведения котов, поэтому
  // они живут отдельно от инструментов и своей подсветкой их не сбивают.
  const rules = mkSection(el, "Правила");

  const auto = mkTool(
    '<span class="sw sw-scrap"></span><span>Убирать сам</span>',
    () => {
      autoTidy = !autoTidy;
      auto.classList.toggle("on", autoTidy);
      worker.postMessage({ type: "setAutoTidy", on: autoTidy });
    },
  );
  auto.classList.add("toggle", "on");
  auto.title = "Коты свозят лом на склад без разметки";
  rules.appendChild(auto);

  // Второй порог усталости (§12.33). Выключено — коты доработают до нуля и
  // свалятся где стоят: это осознанный выбор игрока гнать базу до упора.
  const care = mkTool(
    '<span class="sw sw-rest"></span><span>Беречь себя</span>',
    () => {
      autoRest = !autoRest;
      care.classList.toggle("on", autoRest);
      worker.postMessage({ type: "setAutoRest", on: autoRest });
    },
  );
  care.classList.add("toggle", "on");
  care.title = "На исходе сил кот бросает работу и уходит спать";
  rules.appendChild(care);

  // Вылазки. Не режим ввода: клик — это сразу заявка (§12.22). Поэтому кнопки
  // не входят в общую подсветку инструментов.
  //
  // Раздел — **список отрядов, а не список заказов** (§12.66): строка на узел
  // связи, а кнопки заказов стоят внутри своей строки. Перерисовывается он
  // целиком каждым снапшотом (состав, занятость и таймер меняются), поэтому
  // кнопки внутри ловятся делегированием парой `mousedown`/`mouseup`, как в
  // панелях (§12.57), а не своим `addEventListener` на узле-однодневке.
  if ((meta.missions ?? []).length) {
    raidsEl = mkSection(el, "Вылазки");
    onPanelClick(raidsEl, ".raid-go", (b) =>
      sendAction({
        type: "launch",
        mission: Number(b.dataset.def),
        x: Number(b.dataset.x),
        y: Number(b.dataset.y),
      }),
    );
    // Правило автовылазки (§12.67). Мир оно меняет — значит `sendAction`, как и
    // сама заявка: тумблер и кнопка заказа это одно решение, разово или всякий
    // раз. `data-def` уже несёт то, что надо отправить: `-1` у снятия.
    onPanelClick(raidsEl, ".raid-auto", (b) =>
      sendAction({
        type: "setAutoRaid",
        mission: Number(b.dataset.def),
        x: Number(b.dataset.x),
        y: Number(b.dataset.y),
      }),
    );
    // Состав набирается в панели самой рации (§12.61) — строка отряда только
    // приводит туда. Это осмотр, а не команда: `sendAction` тут не при чём, он
    // бы ещё и гасил выделение клетки, которую мы как раз выбираем.
    onPanelClick(raidsEl, ".raid-crew", (b) => {
      selectedCell = { x: Number(b.dataset.x), y: Number(b.dataset.y) };
      updateSelectionOverlay();
    });
    renderRaidsSection();
  }

  // Наука. Тема — разметка работы, как чертёж: кота не выбираем (§12.26).
  // Цена теми же фишками, что у тайлов и найма: образцы — обычный предмет.
  const topics = meta.research ?? [];
  if (topics.length) {
    const science = mkSection(el, "Наука");

    topicButtons.length = 0;
    topics.forEach((r, i) => {
      const b = mkTool(
        `<span class="sw sw-lab"></span><span>${esc(r.label || r.id)}</span>${costChips(r.cost)}`,
        () => sendAction({ type: "research", topic: i }),
      );
      b.classList.add("toggle");
      b.dataset.level = r.level ?? 0;
      topicButtons.push(b);
      science.appendChild(b);
    });
  }

  // Производство. Заказ — разметка работы, как чертёж, но со счётчиком штук:
  // клик заказывает одну, Shift — пять (§12.30). Кота не выбираем.
  const recipes = meta.recipes ?? [];
  if (recipes.length) {
    const shop = mkSection(el, "Производство");

    recipeButtons.length = 0;
    stockRows.length = 0;
    recipes.forEach((r, i) => {
      // На кнопке — что выходит, и следом цена: те же фишки, что у тайлов.
      const b = mkTool(
        `<span class="sw sw-shop"></span><span>${esc(r.label || r.id)}</span>` +
          `${costChips(r.gives)}<span class="of">←</span>${costChips(r.cost)}`,
        (e) =>
          sendAction({ type: "craft", recipe: i, count: e.shiftKey ? 5 : 1 }),
      );
      b.classList.add("toggle");
      recipeButtons.push(b);
      shop.appendChild(b);

      // Порог автопроизводства — **правило рядом с командой** (§12.64): «держать
      // N» вместо «заказать ещё раз». Живёт здесь, а не в отдельном экране
      // настроек: иначе игрок ищет причину происходящего не там, где решал.
      const row = document.createElement("div");
      row.className = "keep";
      const minus = mkStep("−", (e) => bumpStock(i, e.shiftKey ? -5 : -1));
      const label = document.createElement("span");
      label.className = "keep-val";
      const plus = mkStep("+", (e) => bumpStock(i, e.shiftKey ? 5 : 1));
      row.append(minus, label, plus);
      shop.appendChild(row);
      stockRows.push({ row, label, minus, plus });
    });
  }

  // Обучение. Кнопка адресная, а не разметка работы: игрок отправляет за парту
  // конкретного кота, и это решение о его судьбе (§12.18). Домены без `taught`
  // сюда не попадают — «Стройке» парта не нужна.
  const taught = (meta.skills ?? []).filter((s) => (s.taught ?? 0) > 0);
  if (taught.length) {
    const school = mkSection(el, "Обучение");

    teachButtons.length = 0;
    for (const s of taught) {
      const b = mkTool(
        `<span class="sw sw-study"></span><span>Учить: ${esc(s.label || s.id)}</span>`,
        () => {
          if (selectedUnits.length === 1) {
            sendAction({ type: "teach", id: selectedUnits[0], skill: s.id });
          }
        },
      );
      b.classList.add("toggle");
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
    const list =
      fac.prices instanceof Map
        ? [...fac.prices.keys()]
        : Object.keys(fac.prices ?? {});
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
            `<span>${buying ? "Купить" : "Продать"} ${esc(it.label || it.id)}</span>` +
            '<b class="rate">—</b><b class="qty">×5</b>',
          (ev) => {
            // Клик — пять штук, Shift — двадцать пять: тот же идиом, что у
            // заказа в мастерской, только товар возят мешками.
            const count = ev.shiftKey ? 25 : 5;
            sendAction({ type: "trade", faction: fi, item: ii, count, buying });
          },
        );
        b.classList.add("toggle");
        b.dataset.faction = fi;
        b.dataset.item = ii;
        b.dataset.buying = buying ? "1" : "";
        tradeButtons.push(b);
        sec.appendChild(b);
      }
    }
  });

  // Найм. Кандидаты уникальны (§4.2): каждый приходит один раз, известность
  // открывает, а платит склад — цена теми же фишками, что и у тайлов (§12.24).
  const recruits = meta.recruits ?? [];
  if (recruits.length) {
    const hire = mkSection(el, "Найм");

    recruitButtons.length = 0;
    recruits.forEach((r, i) => {
      const b = mkTool(
        `<span class="sw sw-hire"></span><span>${esc(r.label || r.id)}</span>${costChips(r.cost)}`,
        () => sendAction({ type: "hire", recruit: i }),
      );
      b.classList.add("toggle");
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
  const game = mkSection(el, "Партия");

  const fresh = mkTool(
    '<span class="sw sw-cursor"></span><span>Новая партия</span>',
    () => {
      // Спрашиваем: действие разрушительное и необратимое — автосохранение
      // затрёт старую партию через десяток секунд.
      if (!confirm("Начать новую партию? Текущая будет потеряна.")) return;
      localStorage.removeItem(SAVE_KEY);
      worker.postMessage({ type: "newGame" });
      // Темп сбрасывается вместе с базой: на ×10 первые сутки пролетают, пока
      // игрок читает записку, а на паузе новая партия выглядит сломанной.
      setSpeed(1);
    },
  );
  fresh.title = "Сбросить базу к началу";
  game.appendChild(fresh);

  const dump = mkTool(
    '<span class="sw sw-scrap"></span><span>Сохранить в файл</span>',
    () => worker.postMessage({ type: "save" }),
  );
  dump.title = "Скачать снимок партии";
  game.appendChild(dump);

  const picker = document.createElement("input");
  picker.type = "file";
  picker.accept = ".json,application/json";
  picker.hidden = true;
  picker.addEventListener("change", async () => {
    const file = picker.files?.[0];
    if (!file) return;
    worker.postMessage({ type: "load", json: await file.text() });
    picker.value = ""; // иначе тот же файл второй раз не выберется
  });
  const restore = mkTool(
    '<span class="sw sw-scrap"></span><span>Загрузить файл</span>',
    () => picker.click(),
  );
  restore.title = "Открыть снимок партии";
  game.appendChild(restore);
  game.appendChild(picker);

  const trace = mkTool(
    '<span class="sw sw-hire"></span><span>Скачать трейс</span>',
    () => worker.postMessage({ type: "trace" }),
  );
  trace.title = "Журнал команд: как партия пришла в это состояние";
  game.appendChild(trace);

  // Раскрыт тот раздел, что был открыт до перестройки; на первом кадре это
  // палитра — с неё игра и начинается.
  openOnly(
    sections.some((s) => s.title === openSection) ? openSection : "Постройка",
  );
  selectCursor(cursorBtn); // режим по умолчанию
}

/// Курс фракции по предмету — из снапшота, где его посчитало ядро тем же
/// выражением, каким его посчитает заказ (§12.44). Второй арифметики цены в JS
/// быть не должно.
function quoteOf(faction, item) {
  return prices.find((p) => p.faction === faction && p.item === item);
}

// Сколько предмета база вправе выставить на продажу: всё её добро за вычетом
// того, что уже должны открытые сделки (§12.50).
//
// Считается **по сделкам**, а не по `stock.booked`, и это не вкус. `loose`
// включает груз в лапах, а `booked` его вычитает: из куч эти штуки уже взяты, и
// для тех, кто берёт из куч, скидка верна. Здесь же складываются кучи **и**
// лапы, поэтому носильщик прошёл бы в счёт дважды — раз как «есть на базе», раз
// как «уже не обещано». Ровно эта ошибка жила в воротах самой заявки и давала
// продать то, что кот несёт покупателю.
function sellableOf(item) {
  const st = stock[item] ?? { stored: 0, loose: 0 };
  const owed = openDeals.reduce(
    (n, d) =>
      d.buying || d.item !== item ? n : n + Math.max(0, d.count - d.delivered),
    0,
  );
  return st.stored + st.loose - owed;
}

// Доступность сделки: пост считает ядро, деньги и товар — арифметика над уже
// названными им числами. Причину отказа называем словом: молчащая кнопка
// читается как поломка.
//
// Размер сделки следует за Shift, а не узнаётся в момент клика: кнопка,
// одинаково горящая на пять и на двадцать пять, врёт ровно тогда, когда хватает
// на первое и не хватает на второе.
function syncTradeButtons() {
  const qty = shiftHeld ? 25 : 5;
  for (const b of tradeButtons) {
    const fi = Number(b.dataset.faction);
    const ii = Number(b.dataset.item);
    const buying = !!b.dataset.buying;
    const q = quoteOf(fi, ii);
    if (!q) continue;
    const unit = buying ? q.buy : q.sell;
    const total = unit * qty;
    const broke = buying && money < total;
    const free = buying ? 0 : sellableOf(ii);
    const empty = !buying && free < qty;
    const ready = postFree && !broke && !empty;
    b.disabled = !ready;
    b.classList.toggle("on", ready);
    const rate = b.querySelector(".rate");
    if (rate) rate.textContent = `${unit}¤`;
    const size = b.querySelector(".qty");
    if (size) {
      size.textContent = `×${qty}`;
      size.classList.toggle("big", shiftHeld);
    }
    // Расписание видно вперёд — это и есть разница между планированием и
    // караулом с секундомером (§12.40).
    const next = buying ? q.next_buy : q.next_sell;
    const ahead =
      q.next_in > 0 && next !== unit
        ? ` · через ${q.next_in} станет ${next}¤`
        : "";
    b.title = !posts
      ? "Нужен «Торговый пост»"
      : !postFree
        ? // Ячейка занята либо сделкой, либо непойманным привозом (§12.68):
          // второй случай игрок чинит сам, и сказать об этом надо здесь.
          `Свободных ячеек нет: разгрузите пост или постройте ещё один`
        : broke
          ? `Нужно ${total}¤ за ${qty}, у вас ${money}¤`
          : empty
            ? // Названное число — это «свободно», а не «есть»: под открытой
              // сделкой товар базе уже не принадлежит (§12.50), и без этой
              // оговорки отказ спорит с счётчиком в шапке.
              `Свободно к продаже ${Math.max(0, free)}, нужно ${qty}`
            : `${unit}¤ за штуку · ${qty} шт. = ${total}¤${
                shiftHeld ? "" : " · Shift — двадцать пять"
              }${ahead}`;
  }
}

// Чего не хватает складу на этот набор — и что с этим делать (§12.53).
//
// «На складе нечем заплатить» звучит издевательски, когда нужное лежит кучей в
// двух шагах: платит склад (§12.24), а игрок видит базу целиком. Поэтому отказ
// называет и то, сколько есть, и то, сколько валяется мимо склада. Числа —
// из снапшота, считает их ядро.
function payHint(cost) {
  const entries =
    cost instanceof Map ? [...cost.entries()] : Object.entries(cost ?? {});
  const items = meta.items ?? [];
  const short = [];
  for (const [id, need] of entries) {
    const i = items.findIndex((it) => it.id === id);
    const st = stock[i] ?? { stored: 0, loose: 0, booked: 0 };
    const free = Math.max(0, st.stored - st.booked);
    if (free >= need) continue;
    const tail = st.loose
      ? `, ещё ${st.loose} валяется — уберите на склад`
      : "";
    short.push(`${items[i]?.label || id} ${free} из ${need}${tail}`);
  }
  return short.join(" · ");
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
    b.classList.toggle("on", ready);
    // Своего присылают тем, кому доверяют (§12.43), и репутацией за него не
    // платят — платит склад. Поэтому причин отказа три и они разные.
    const distrust = r.welcome
      ? null
      : trustGap((meta.recruits ?? [])[i]?.needs);
    const why = r.hired
      ? "Уже на базе"
      : !r.unlocked
        ? `Откликнется при известности ${b.dataset.requires}`
        : distrust
          ? distrust
          : !r.affordable
            ? `На складе нечем заплатить: ${payHint((meta.recruits ?? [])[i]?.cost)}`
            : "Нанять";
    // Параметры называем и у закрытого кандидата: к нему идут заранее, и
    // «зачем мне этот кот» игрок спрашивает до того, как накопит.
    b.title = [b.dataset.hint, why].filter(Boolean).join(" · ");
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
    const ready =
      !t.known &&
      t.unlocked &&
      t.affordable &&
      t.staffed &&
      t.lab &&
      !researchRunning;
    b.disabled = !ready;
    b.classList.toggle("on", ready);
    b.title = t.known
      ? "Уже изучено"
      : !t.unlocked
        ? "Нужны предыдущие технологии"
        : !t.lab
          ? "Нет лаборатории"
          : !t.staffed
            ? `Нужен кот с «Наукой» ${b.dataset.level} уровня`
            : !t.affordable
              ? `На складе нет образцов: ${payHint((meta.research ?? [])[i]?.cost)}`
              : researchRunning
                ? "Тема уже изучается"
                : "Взяться за тему";
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
    // `shop` теперь значит «есть куда поставить»: свободный станок **или** уже
    // размеченный заказ на этот рецепт — тогда клик добавит штук (§12.55).
    // Общего «заказ уже в работе» больше нет: заказов столько, сколько
    // мастерских, и вторая мастерская — это вторая работа, а не декорация.
    const ready = r.unlocked && r.shop;
    b.disabled = !ready;
    b.classList.toggle("on", ready && r.affordable);
    b.title = !r.unlocked
      ? "Нужна технология"
      : !r.shop
        ? shopsBusyHint()
        : r.affordable
          ? "Заказать: клик — штука, Shift — пять"
          : `На складе нет материала, заказ будет ждать: ${payHint((meta.recipes ?? [])[i]?.cost)}`;
  });
}

// Кнопка шага у порога. Живёт в тулбаре, а не в панели, поэтому слушатель
// вешается прямо на узел: перерисовка целиком (§12.57) грозит только правым
// панелям, а тулбар собирается один раз.
function mkStep(sign, onClick) {
  const b = document.createElement("button");
  b.className = "tool step";
  b.textContent = sign;
  b.addEventListener("click", onClick);
  return b;
}

// Сдвинуть порог. Отсчитываем от **числа из снапшота**, а не от своего
// счётчика: правило живёт в ядре, и второй его экземпляр здесь разошёлся бы с
// ним при первой же загрузке партии (§12.53).
function bumpStock(recipe, delta) {
  const min = Math.max(0, (stocking[recipe] ?? 0) + delta);
  if (min === (stocking[recipe] ?? 0)) return;
  sendAction({ type: "setStock", recipe, min });
}

// Порог показываем словом, а не пустым нулём: «держать —» читается как «правила
// нет», а «0» — как «держать ноль», то есть как настройку, которой игрок не
// делал. Рецепт, закрытый технологией, порога не принимает — ядро его всё равно
// не исполнит (§12.65).
function syncStockRows(list, recipes) {
  stockRows.forEach(({ row, label, minus, plus }, i) => {
    const min = (list ?? [])[i] ?? 0;
    const open = ((recipes ?? [])[i] ?? {}).unlocked ?? false;
    stocking[i] = min;
    label.textContent = min > 0 ? `держать ${min}` : "держать —";
    row.classList.toggle("on", min > 0);
    row.hidden = !open;
    minus.disabled = !open || min <= 0;
    plus.disabled = !open;
    row.title =
      min > 0
        ? `Коты сами делают, пока на базе меньше ${min} шт. Клик — на штуку, Shift — на пять`
        : "Держать запас: коты будут делать сами, когда просядет";
  });
}

// Почему рецепт не заказать. «Мастерской нет» и «все станки заняты» — разные
// новости: первую чинят стройкой первой мастерской, вторую — второй, и
// молчащая кнопка не сказала бы ни того, ни другого (§4.4, §12.55).
function shopsBusyHint() {
  return shops > 0
    ? `Все мастерские заняты: заказов ${shops} из ${shops}. Постройте ещё`
    : "Нет мастерской";
}

// Палитра, закрытая технологией: кнопка видна и объясняет, чем открывается.
// Название темы берём из палитры тем — второго списка технологий не заводим.
function syncTileButtons(techs) {
  const known = techs ?? [];
  for (const { btn, tech } of tileButtons) {
    const open = known.includes(tech);
    btn.disabled = !open;
    const def = (meta.research ?? []).find((r) => r.id === tech);
    btn.title = open ? "" : `Откроет тема «${def?.label || tech}»`;
  }
}

// Учат по одному: обучение адресно, и «учить троих разом» — это уже не решение
// о судьбе кота, а разметка работы, которой обучение как раз не является.
function syncTeachButtons() {
  const ready = selectedUnits.length === 1;
  for (const b of teachButtons) {
    b.disabled = !ready;
    b.classList.toggle("on", ready);
    b.title = ready
      ? `${selectedUnits[0]} — ${b.dataset.hint}`
      : "Выберите одного кота";
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
  const entries =
    needs instanceof Map ? [...needs.entries()] : Object.entries(needs ?? {});
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

// Раздел «Вылазки» целиком: строка на каждый узел связи (§12.66). Отряд живёт
// на клетке рации и переживает вылазку (§12.61), поэтому список отрядов — это
// список узлов, а заказы стоят кнопками внутри своей строки: вопрос «чей отряд
// идёт» отвечается тем, в какой строке нажали, и запоминать выбранный узел
// больше не нужно.
function renderRaidsSection() {
  if (!raidsEl || !meta) return;
  const defs = meta.missions ?? [];
  if (!nodes.length) {
    // Ноль узлов — ноль вылазок, строго как ноль мастерских это ноль заказов
    // (§12.59). Говорим это словом: пустой раздел читается как поломка.
    raidsEl.innerHTML =
      '<div class="cat-sub">Узлов связи нет — постройте рацию, она держит отряд и одну вылазку</div>';
    return;
  }
  raidsEl.innerHTML = nodes
    .map((node, n) => {
      // Номер отряда — по порядку узлов из ядра, а он row-major по карте
      // (`relay_cells`) и потому детерминирован. Своего имени у узла нет
      // намеренно: это лишнее поле в снимке ради подписи (§12.59).
      const at = `${node.x}, ${node.y}`;
      const raid = missionsOut.find(
        (m) => m.node_x === node.x && m.node_y === node.y,
      );
      const rows = [
        `<button class="tool raid-crew" data-key="crew@${at}" data-x="${node.x}" data-y="${node.y}">` +
          `<span class="sw sw-gate"></span><span>Отряд ${n + 1}</span>` +
          `<span class="cell-at">${at}</span></button>`,
        `<div class="cat-sub">${
          node.crew.length
            ? node.crew.map(esc).join(" · ")
            : "состав не набран — откройте рацию и отметьте котов"
        }</div>`,
      ];
      // Правило автовылазки — про весь отряд, а не про одну кнопку, поэтому
      // стоит строкой над заказами и видно его всегда: и когда отряд дома, и
      // когда он уже в поле по этому самому правилу (§12.67). Иначе снять его
      // во время вылазки было бы нечем, а следующая ушла бы «сама».
      if (node.auto >= 0) {
        rows.push(
          '<div class="raid-rule">' +
            `<span>ходит сама: «${esc(missionLabel(node.auto))}»</span>` +
            `<button class="tool raid-auto" data-key="off@${at}" data-def="-1"` +
            ` data-x="${node.x}" data-y="${node.y}">Снять</button>` +
            "</div>",
        );
      }
      if (raid) {
        // Занятый узел кнопок не показывает вовсе: вторая вылазка отсюда всё
        // равно невозможна, а восемь погашенных кнопок под каждым отрядом —
        // это шум, из-за которого список отрядов перестаёт читаться.
        const label = esc(missionLabel(raid.def));
        rows.push(
          `<div class="cat-sub">${
            raid.away
              ? `в поле: «${label}» · вернутся через ${raid.left}`
              : `собирается: «${label}»`
          }</div>`,
        );
      } else {
        for (let i = 0; i < defs.length; i++) {
          const g = raidGate(i, node);
          const on = node.auto === i;
          // Заказ и «ходить сюда самому» — про одно и то же решение, поэтому
          // стоят одной строкой: клик отправляет разово, тумблер повторяет клик
          // каждый раз, когда отряд снова готов (§12.67). Тумблер доступен и у
          // закрытой вылазки: правило ждёт ворот, как порог ждёт материала, и
          // поставить его заранее — это план, а не ошибка.
          rows.push(
            '<div class="raid-line">' +
              `<button class="tool toggle raid-go${g.ready ? " on" : ""}"` +
              `${g.ready ? "" : " disabled"}` +
              ` data-key="${i}@${at}" data-def="${i}" data-x="${node.x}" data-y="${node.y}"` +
              ` title="${esc(g.title)}">` +
              `<span class="sw sw-gate"></span><span>${esc(defs[i].label || defs[i].id)}</span>` +
              `${costChips(defs[i].loot)}</button>` +
              `<button class="tool raid-auto${on ? " on" : ""}"` +
              ` data-key="auto${i}@${at}" data-def="${on ? -1 : i}"` +
              ` data-x="${node.x}" data-y="${node.y}"` +
              ` title="${on ? "Отряд ходит сюда сам — снять правило" : "Ходить сюда самому, как только отряд готов"}">↻</button>` +
              "</div>",
          );
        }
      }
      return `<div class="raid-node">${rows.join("")}</div>`;
    })
    .join("");
}

// Пойдёт ли этот отряд по этому заказу — и если нет, то почему словом. Ворота
// считает ядро (§12.24, §12.43, §12.59): те же проверки стоят в `launch`, и
// второй их экземпляр здесь однажды разойдётся с фасадом. Здесь только причина
// отказа: молчащая кнопка читается как поломка.
function raidGate(i, node) {
  const def = (meta.missions ?? [])[i] ?? {};
  const need = def.squad ?? 0;
  const hint =
    `${need} кота · ${def.ticks} тиков · сложность ${def.danger ?? 0}` +
    (def.harm ? ` · раны при провале ${def.harm}` : "");
  // До первого снапшота ворот ещё нет — считаем закрытыми.
  const gates = raids[i];
  const known = !!gates?.unlocked;
  // За своим идут, только пока есть за кем: у вылазки с `rescue` нет ни добычи,
  // ни цели, если все дома.
  const nobody = !(gates?.possible ?? true);
  // Заказчик с базой не разговаривает (§12.43). Отдельно от известности: «не
  // дорос» и «эти вас не жалуют» — разные новости, и вторая обратима.
  const welcome = !!gates?.welcome;
  const distrust = welcome ? null : trustGap(def.needs);
  // Раненого и пленного ядро в отряд не пустит (§12.37, §12.40) — причину
  // называем словом, как и нехватку известности.
  const hurt = node.crew.filter((id) => wounded.has(id));
  const gone = node.crew.filter((id) => captives.includes(id));
  // Двух вылазок по одному заказу не бывает (§12.59), и это другая новость, чем
  // «этот узел занят»: там ждать своего отряда, здесь — брать другой заказ.
  const taken = running.has(i);
  const ready =
    !taken &&
    known &&
    welcome &&
    !hurt.length &&
    !gone.length &&
    !nobody &&
    node.crew.length === need;
  // Закрытые вылазки видны, а не спрятаны: лестница ответственности — это то, к
  // чему игрок идёт, и невидимая цель не тянет (§4.4).
  const title = taken
    ? "Эта вылазка уже идёт другим отрядом"
    : !known
      ? `${hint} · нужна известность ${def.requires ?? 0}`
      : distrust
        ? `${hint} · ${distrust}`
        : nobody
          ? `${hint} · все дома, спасать некого`
          : gone.length
            ? `${hint} · в плену: ${gone.join(", ")}`
            : hurt.length
              ? `${hint} · ранен: ${hurt.join(", ")}`
              : node.crew.length !== need
                ? `${hint} · в отряде ${node.crew.length} из ${need} — состав набирается в панели рации`
                : hint;
  return { ready, title };
}

function mkTool(html, onClick) {
  const b = document.createElement("button");
  b.className = "tool";
  b.innerHTML = html;
  b.addEventListener("click", onClick);
  return b;
}

function activate(btn) {
  for (const b of document.querySelectorAll("#toolbar .tool:not(.toggle)")) {
    b.classList.remove("active");
  }
  if (btn) btn.classList.add("active");
}

function selectCursor(btn) {
  mode = "cursor";
  activate(btn);
  applyModeChrome();
}
function selectBuild(i, btn) {
  mode = "build";
  buildTile = i;
  activate(btn);
  applyModeChrome();
}
function selectStore(btn) {
  mode = "store";
  activate(btn);
  applyModeChrome();
}

function showError(message) {
  const el = document.getElementById("error");
  el.hidden = false;
  el.textContent = "Ошибка воркера: " + message;
  console.error(message);
}

// Подсказка: длинная и закрывает карту, поэтому по умолчанию свёрнута и
// разворачивается кнопкой. Раскрытой она была, пока была единственным местом,
// где написано управление, — но выросла вместе с механиками и стала закрывать
// то, что объясняет; а перезагрузка (единственный способ продолжить партию)
// возвращала её каждый раз поверх карты. Состояние нигде не хранится: на POC
// лишняя persistent-настройка дороже, чем один клик.
const hintEl = document.getElementById("hint");
const hintToggle = document.getElementById("hint-toggle");
hintEl.hidden = true;
hintToggle.addEventListener("click", () => {
  hintEl.hidden = !hintEl.hidden;
  hintToggle.classList.toggle("active", !hintEl.hidden);
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
  worker.postMessage({ type: "setSpeed", speed: s });
  for (const b of document.querySelectorAll(".speed")) {
    b.classList.toggle("active", Number(b.dataset.speed) === s);
  }
}
// Только кнопки с самой скоростью: без фильтра сюда попадала соседняя «?», и
// клик по ней слал в воркер `Number(undefined)` — то есть останавливал время.
for (const b of document.querySelectorAll(".speed[data-speed]")) {
  b.addEventListener("click", () => setSpeed(Number(b.dataset.speed)));
}
setSpeed(1);
