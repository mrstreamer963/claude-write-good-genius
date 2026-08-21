// Главный поток: PixiJS-рендер + ввод игрока. Логики нет — рисуем данные из воркера
// и шлём команды (постройка тайлов, приказы движения).

import { Application, Container, Graphics, Text } from "pixi.js";
import { ITEM_GLYPHS, TILE_GLYPHS } from "./glyphs.js";

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

// --- глифы (§12.109) -------------------------------------------------------
//
// Предмет и роль клетки рисуются значком, а не цветным квадратиком. Квадратик
// был честной краской, но пустой формой: `▪60 ▪6 ▪0 ▪0 ▪6 ▪0` в шапке — это
// шесть абстракций, которые игрок обязан выучить прежде, чем прочтёт хоть одно
// число. Краска при этом никуда не делась: глиф красится тем же цветом из
// рулсета (`currentColor`), то есть словарь не заменён, а дополнен формой.
//
// Граница у этого одна и держать её надо твёрдо: **глиф заменяет
// существительное, но никогда — причину**. Отказ кнопки, срок, доля и вилка
// состава остаются словами (§12.53, §12.71): «готовы идти 0, а нужно 1»
// пиктограммой не сказать, не соврав.
//
// Контуры лежат в `glyphs.js` одним списком на HTML и на карту — второй набор
// однажды разъедется, и паёк в шапке перестанет быть пайком в лапах у кота.

/// Вставляет `<symbol>`-спрайт в документ. Зовётся один раз при старте: панели
/// перерисовываются каждым кадром через `innerHTML`, и `<use href="#...">` —
/// единственная форма, при которой контур не пересобирается по сотне раз в
/// секунду. Прячем не через `hidden` (его перебивает `display` из вёрстки —
/// на этом уже попадались), а собственным `display: none`.
function installGlyphSprite() {
  const sym = (prefix, table) =>
    Object.entries(table)
      .map(
        ([id, d]) =>
          `<symbol id="g-${prefix}-${id}" viewBox="0 0 24 24">` +
          `<path d="${d}"/></symbol>`,
      )
      .join("");
  document.body.insertAdjacentHTML(
    "afterbegin",
    '<svg id="glyph-sprite" style="display:none" aria-hidden="true">' +
      sym("item", ITEM_GLYPHS) +
      sym("tile", TILE_GLYPHS) +
      "</svg>",
  );
}
installGlyphSprite();

/// Разметка одного глифа. `color` идёт инлайном, потому что краска у предмета
/// приезжает из рулсета, а не из темы: контур залит `currentColor`.
function glyphHtml(sym, color, cls = "") {
  return (
    `<svg class="glyph ${cls}" style="color:${color}" aria-hidden="true">` +
    `<use href="#${sym}"/></svg>`
  );
}

/// Глиф предмета по индексу палитры `items:`.
///
/// Рулсет — контент, и предмет в нём заводится без спроса у вида: у своего
/// глифа для него нет, и падать на этом нельзя. Такой предмет остаётся цветным
/// квадратиком — ровно тем, чем были все шестеро до §12.109.
function itemGlyph(i, cls = "") {
  const it = (meta?.items ?? [])[i];
  if (!it) return "";
  return ITEM_GLYPHS[it.id]
    ? glyphHtml(`g-item-${it.id}`, it.color, cls)
    : `<i class="chip" style="background:${it.color}"></i>`;
}

/// Глиф тайла по индексу палитры `tiles:`. У «Пола» его нет — это фон, а не
/// роль, — и пустая строка здесь законный ответ, а не промах.
function tileGlyphHtml(i, cls = "") {
  const t = (meta?.palette ?? [])[i];
  if (!t || !TILE_GLYPHS[t.id]) return "";
  return glyphHtml(`g-tile-${t.id}`, t.color, cls);
}

const stageEl = document.getElementById("stage");
const tickEl = document.getElementById("tick");
const scrapEl = document.getElementById("scrap");
// Фишки склада в шапке — дверь в окно «Склад» (§12.100): числа уже стоят здесь,
// а окно это их подробность. Отдельной кнопки для этого не заводим — она стояла
// бы вплотную к тому, что и так про склад.
scrapEl.style.cursor = "pointer";
scrapEl.title = "Открыть склад";
scrapEl.addEventListener("click", () => openStockWindow());
const catEl = document.getElementById("cat");
const cellEl = document.getElementById("cell");
// ⚠️ Панель клетки перерисовывается каждым снапшотом целиком, поэтому её
// единственная кнопка идёт делегированием и парой `mousedown`/`mouseup`
// (§12.80, §12.95): узел кнопки живёт один кадр, а клик человека — сотню
// миллисекунд. Регистрируется один раз, здесь, а не при отрисовке панели.
onPanelClick(cellEl, ".store-mark", (b) =>
  sendAction({
    type: "store",
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
    w: 1,
    h: 1,
  }),
);
const missionEl = document.getElementById("mission");
const captiveEl = document.getElementById("captive");
const researchEl = document.getElementById("research");
const craftEl = document.getElementById("craft");
const dealEl = document.getElementById("deal");
const tapeEl = document.getElementById("tickers");
const noteEl = document.getElementById("note");
const goalsEl = document.getElementById("goals");
const goalsToggleEl = document.getElementById("goals-toggle");
const finaleEl = document.getElementById("finale");
const raidWinEl = document.getElementById("raidwin");
const stockWinEl = document.getElementById("stockwin");
const toastsEl = document.getElementById("toasts");
const liveTipEl = document.getElementById("livetip");

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
// Отмена уходит **по клетке** и живёт в панели этой самой клетки (§12.96):
// заказов на один рецепт теперь бывает несколько, и три строки «Деталь» в общем
// списке не различить, — а ячейка различает их полностью. Вторая кнопка панели
// клетки после «на склад» (§12.95), и по тому же правилу: кнопка стоит там, что
// она меняет.
// Быстрая сделка из ленты (§12.100). Панель перерисовывается целиком каждым
// снапшотом, поэтому кнопка ловится делегированием парой `mousedown`/`mouseup`
// (§12.57), а не своим `addEventListener` на узле-однодневке. Ключ — «сторона
// сделки + предмет»: у одной строки две кнопки, и по `data-item` они поделили
// бы одну.
onPanelClick(tapeEl, ".tick-deal", (node) =>
  sendAction({
    type: "trade",
    faction: Number(node.dataset.faction),
    item: Number(node.dataset.item),
    // Размер берётся тем же выражением, что и подпись на кнопке: обещать пять,
    // а слать двадцать пять — это кнопка, которая врёт (§12.90).
    count: dealSize(shiftHeld),
    buying: !!node.dataset.buying,
  }),
);
// Снять тикер прямо из ленты (§12.100): закрепил игрок в окне, а надоел он ему
// здесь, и гонять его за этим обратно в окно — дорога в один конец. Сторона в
// команде не нужна, ключ у тикера — предмет, но фасад её принимает: снятие
// ворот не спрашивает.
onPanelClick(tapeEl, ".tick-off", (node) =>
  sendAction({
    type: "setTicker",
    item: Number(node.dataset.item),
    faction: 0,
    on: false,
  }),
);
onPanelClick(cellEl, ".craft-cancel", (node) =>
  sendAction({
    type: "cancelCraft",
    x: Number(node.dataset.x),
    y: Number(node.dataset.y),
  }),
);
onPanelClick(researchEl, ".research-cancel", () =>
  sendAction({ type: "cancelResearch" }),
);
onPanelClick(missionEl, ".mission-cancel", (b) =>
  sendAction({ type: "cancelMission", mission: Number(b.dataset.def) }),
);
// В панели клетки команд нет ни одной (§12.80). Состав отряда (§12.61) и
// дежурство на связи (§12.60) правились строками прямо в ней, и панель рации
// выросла в самый плотный экран игры: кнопка штаба, список всех котов базы по
// две кнопки на строку — и всё это в колонке шириной с тулбар, где ни исхода,
// ни причины отказа не помещается. Решение о вылазке принимают там, где видно
// его цену, — в штабе; панель клетки осталась осмотром, как и остальные.
//
// Штаб вылазок (§12.71). Строки списка различаются `data-id`, как отмены
// заказов различаются `data-def`: окно перерисовывается каждым снапшотом
// целиком — иначе прогноз не менялся бы от щелчка по коту, ради чего окно и
// заведено, — и без ключа две строки поделили бы один взвод. `data-in` —
// «числится ли здесь»: одна кнопка на два состояния, потому что и вопрос один —
// про этого кота и этот узел.
onPanelClick(raidWinEl, ".crew-pick", (b) =>
  sendAction({
    type: b.dataset.in ? "dismiss" : "enlist",
    id: b.dataset.id,
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
onPanelClick(raidWinEl, ".crew-duty", (b) =>
  sendAction({
    type: b.dataset.in ? "unpostRelay" : "postRelay",
    id: b.dataset.id,
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
onPanelClick(raidWinEl, ".raid-go", (b) =>
  sendAction({
    type: "launch",
    mission: Number(b.dataset.def),
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
// Отзыв из штаба — та же команда, что и в панели вылазки: по `def`, а не по
// номеру строки (§12.59). Свой обработчик нужен потому, что делегирование
// привязано к контейнеру, а окно — отдельный контейнер от панелей.
onPanelClick(raidWinEl, ".mission-cancel", (b) =>
  sendAction({ type: "cancelMission", mission: Number(b.dataset.def) }),
);
onPanelClick(raidWinEl, ".raid-auto", (b) =>
  sendAction({
    type: "setAutoRaid",
    mission: Number(b.dataset.def),
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
// Пауза правила (§12.77). Мир она меняет — значит `sendAction`. `data-on` уже
// несёт то, что надо отправить: пусто у «приостановить», «1» у «возобновить».
onPanelClick(raidWinEl, ".raid-pause", (b) =>
  sendAction({
    type: "setAutoRaidOn",
    on: !!b.dataset.on,
    x: Number(b.dataset.x),
    y: Number(b.dataset.y),
  }),
);
// Переключение узла внутри окна — осмотр, а не команда: `sendAction` тут не при
// чём. Отряды листаются на месте, чтобы «перекинуть кота с узла на узел» не
// требовало закрывать окно и искать вторую рацию на карте.
onPanelClick(raidWinEl, ".raidwin-tab", (b) => {
  raidWinAt = { x: Number(b.dataset.x), y: Number(b.dataset.y) };
  renderRaidWindow();
});
onPanelClick(raidWinEl, ".raidwin-close", () => closeRaidWindow());
// Клик по затемнению — тот же выход, что и Escape: окно модальное, и промах
// мимо него это почти всегда «хватит».
raidWinEl.addEventListener("mousedown", (e) => {
  if (e.target === raidWinEl) closeRaidWindow();
});

const app = new Application();
// `resolution` + `autoDensity` — иначе на retina канвас рисуется в половину
// экранного разрешения: backing store совпадает с CSS-размером, и каждый пиксель
// карты растягивается вдвое самим дисплеем. По умолчанию Pixi берёт `1`, то есть
// вся карта была мыльной на любом экране с dpr > 1. `autoDensity` при этом
// держит CSS-размер канваса прежним — растёт только буфер.
await app.init({
  background: COLORS.bg,
  antialias: true,
  resizeTo: stageEl,
  resolution: window.devicePixelRatio || 1,
  autoDensity: true,
});
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
// Во сколько раз карта растянута под размер окна (`layout`). Читают двое:
// само центрирование и текст внутри `world` — тот растеризуется в момент
// создания и без поправки на масштаб мылится.
let worldScale = 1;
let paletteColors = []; // number[]
let itemColors = []; // number[] — цвет предмета по индексу палитры items
let mapCells = null; // Int-массив состояния карты
let mode = "cursor"; // 'cursor' | 'build'
let buildTile = 0; // индекс палитры, или -1 = стереть (в режиме build)
let autoTidy = true; // коты сами свозят лом на склад (см. ядро, §12.16)
let tidyBtn = null; // кнопка «Убирать сам»: подсветку ей ставит снимок (§12.96)
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
// Вылазок идёт столько, сколько узлов связи (§12.59). Число считает ядро
// (`relay_nodes`), а JS его только подписывает: второй экземпляр этого счёта
// однажды разойдётся с `launch`.
//
// Рядом с ним ехали **ворота** — «свободен ли хоть один узел». С §12.61 их
// больше нет ни здесь, ни в снимке: узел адресуется поимённо, и вопрос стал
// поузловым («занят ли **этот**», `NodeSnap.busy`). Глобальный ответ на него
// уже не отвечает, и заводить его обратно незачем.
let relays = 0;
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
// Какой узел открыт в штабе вылазок, или `null` — штаб закрыт (§12.71). Это не
// «выбранный узел» из §12.66: тулбарная строка по-прежнему адресуется собой, а
// здесь помнится ровно то, что игрок сейчас читает в модальном окне.
let raidWinAt = null;
// Разметка окна с прошлого кадра: заменять её, когда она не изменилась, значит
// каждым кадром отматывать прокрутку в ноль (§12.71).
let raidWinHtml = "";
// Последний снапшот целиком: штаб перерисовывается каждым кадром и читает из
// него состав базы (`entities`), как это делает панель клетки.
let lastSnap = null;
// Идущие вылазки целиком: строке отряда надо сказать, чем занят **его** узел, а
// не только «занят». Тот же список читает и панель миссий.
let missionsOut = [];
// Заказы, по которым отряд уже вышел: двух вылазок по одному заказу не бывает
// (§12.59), и гасить надо именно свою кнопку.
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
// Сколько влезает в контейнер ячейки, которую займёт следующая сделка (§12.90);
// ноль — предела нет. Считает ядро тем же выражением, что и `trade`: разойдись
// они, и Shift обещал бы объём, который фасад отклоняет молча.
let postLot = 0;
// Открыта ли автоматика такого рода (§12.93): ворота названы технологиями в
// рулсете, а изучены они или нет — считает ядро. Ярлыки самих тем приезжают
// один раз в `meta.auto_gates`, потому что имя темы это контент.
let autoOpen = { sales: true, crafting: true, raids: true };

// Отказ ворот автоматики словом (§12.53): «нужна какая-то наука» не говорит
// ничего, поэтому называем тему по имени. `null` — ворота открыты.
function autoGateHint(kind) {
  if (autoOpen[kind]) return null;
  const name = (meta?.auto_gates ?? {})[kind];
  return name ? `Нужно исследование «${name}»` : "Нужно исследование";
}
// Зажат ли Shift: он удваивает не смысл кнопки, а её размер (пять штук против
// двадцати пяти), поэтому доступность и подпись обязаны следовать за клавишей.
// Молчащая кнопка читается как поломка — а «денег хватает на пять, но не на
// двадцать пять» без этого выглядело именно так.
let shiftHeld = false;
const tradeButtons = []; // кнопки сделок — гасим, когда все посты заняты (§12.55)
// Правила автопродажи (§12.87, §12.88): **по одному на предмет**, покупатель —
// поле правила. Хранит их ядро и везёт отдельным списком, а не полем в строке
// курса: строк у предмета столько, сколько сторон им торгует, а правило одно.
let sales = [];
// Закладки игрока (§12.100). Обе живут в ядре и переживают загрузку партии:
// зеркало в JS уже врало после загрузки у тумблера «Убирать сам» (§12.96).
// Избранное — только предметы (это порядок строк в окне), тикеры — предмет и
// сторона (это лента на главном экране и кнопки сделки в ней).
let favorites = [];
let tickers = [];
// Кого выберет следующий клик у предмета, на котором правила ещё нет. Это не
// зеркало правила (его нет), а заготовка выбора: как только порог поставят,
// сторона уедет в ядро и читаться будет уже оттуда (§12.53).
const picked = new Map();
// Раздел вылазок — единственный, который перерисовывается целиком (§12.66):
// строка на отряд, а состав и занятость узла меняются каждым снапшотом. Массива
// живых кнопок у него поэтому нет — только контейнер.
let raidsEl = null;
const recruitButtons = []; // кнопки найма — гасим по известности и складу
const teachButtons = []; // кнопки обучения — живы, когда выбран ровно один кот
// Парты по доменам: сколько всего и сколько свободно (§12.84). Считает ядро —
// «свободна» значит «её не держит ничей `Study`», а занятость знает только оно.
// Нужны здесь ровно за тем же, чем `standing`: назвать отказ словом.
let desks = [];
const topicButtons = []; // кнопки тем — гасим по технологиям, складу и допуску
// Пороги автопроизводства в порядке палитры рецептов (§12.65). Число хранит
// ядро; здесь оно нужно строке окна «Склад» — и подписи, и полю правки, которое
// открывается **с него** (§12.100, §12.105, §12.108).
let stocking = [];
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
    goalsOpen = true; // ...и свёрнутость панели: она про прошлый мир, а не про этот
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

// Карта вписывается в отведённое ей место **масштабом контейнера**, а не
// пересчётом `TILE`. Довод не в лени: `TILE` ходит по полусотне мест — по
// позициям тайлов, по радиусам котов, по отступам меток, — и половина из них
// нарисована один раз при создании узла (`createUnit`, `orderMarker`). Сделай
// `TILE` переменной — и всё это пришлось бы пересобирать на каждом ресайзе.
// `world.scale` растит их разом и бесплатно.
//
// Ввод от этого не страдает: `tileAt` считает клетку через `world.toLocal()`,
// который масштаб уже учитывает. Второго места, знающего про масштаб, нет.
//
// До этого карта 24×16 занимала треть отведённой площади и стояла крошечным
// пятном посреди пустоты: `TILE` подобран под самый маленький экран, а растёт
// окно, а не тайл.
function layout() {
  if (!meta) return;
  const w = meta.width * TILE;
  const h = meta.height * TILE;
  // Свободная полоса, а не вся сцена: `#stage` растянут во всё окно, а тулбар и
  // правая колонка стоят `position: fixed` **поверх** него (на этом держится
  // подсветка режима по периметру карты). Впиши карту в `app.screen` — и её
  // края уедут под панели.
  //
  // Ширины меряем у самих узлов, а не переписываем сюда числами из CSS: колонки
  // там правятся, а разъехавшийся дубль дал бы карту, наполовину скрытую под
  // панелью, — то есть ровно то, что чинится этим кодом.
  //
  // По вертикали полосу не режем намеренно: тулбар растёт и сжимается, когда
  // игрок раскрывает разделы, и карта прыгала бы под курсором на каждом клике.
  const stageBox = stageEl.getBoundingClientRect();
  const barBox = document.getElementById("toolbar")?.getBoundingClientRect();
  const sideBox = document.getElementById("side")?.getBoundingClientRect();
  const padLeft = barBox ? Math.max(8, barBox.right - stageBox.left + 12) : 8;
  const padRight = sideBox ? Math.max(8, stageBox.right - sideBox.left + 12) : 8;
  const availW = Math.max(TILE, app.screen.width - padLeft - padRight);
  const availH = Math.max(TILE, app.screen.height - 16);
  // Не мельчим ниже единицы: `TILE` подобран под самый маленький экран, и на нём
  // карта просто уезжает под панели, как уезжала всегда.
  worldScale = Math.max(1, Math.min(availW / w, availH / h));
  world.scale.set(worldScale);
  world.x = padLeft + Math.max(0, Math.floor((availW - w * worldScale) / 2));
  world.y = 8 + Math.max(0, Math.floor((availH - h * worldScale) / 2));
}
app.renderer.on("resize", layout);

// Контур глифа тайла — **один `GraphicsContext` на тип**, а не на клетку.
// Складов на базе десятки, и пересобирать один и тот же контур под каждый — это
// разбор SVG в цикле по всей карте. Pixi умеет делить контекст между узлами
// (`new Graphics(ctx)`), чем мы и пользуемся.
//
// Цвет запечён в сам контекст, потому что он свойство типа, а не клетки.
const tileGlyphCache = new Map();
function tileGlyphContext(i) {
  if (tileGlyphCache.has(i)) return tileGlyphCache.get(i);
  const def = (meta?.palette ?? [])[i];
  const d = def && TILE_GLYPHS[def.id];
  let ctx = null;
  if (d) {
    // Рулсет — контент, и кривой контур в нём не должен ронять карту: тайл
    // просто останется без значка, как «Пол», у которого его нет и так.
    try {
      ctx = new Graphics().svg(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">' +
          `<path fill="#ffffff" d="${d}"/></svg>`,
      ).context;
    } catch (err) {
      console.warn(`глиф тайла ${def.id} не разобрался:`, err);
    }
  }
  tileGlyphCache.set(i, ctx);
  return ctx;
}

// То же для предметов: глиф груза в лапах у кота. Контекст на тип, а не на
// кота, — носильщиков бывает десяток, а типов шесть.
const itemGlyphCache = new Map();
function itemGlyphContext(i) {
  if (itemGlyphCache.has(i)) return itemGlyphCache.get(i);
  const def = (meta?.items ?? [])[i];
  const d = def && ITEM_GLYPHS[def.id];
  let ctx = null;
  if (d) {
    try {
      ctx = new Graphics().svg(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">' +
          `<path fill="${def.color}" d="${d}"/></svg>`,
      ).context;
    } catch (err) {
      console.warn(`глиф предмета ${def.id} не разобрался:`, err);
    }
  }
  itemGlyphCache.set(i, ctx);
  return ctx;
}

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

  // Вторым проходом — что клетка делает (§12.109). Заливка отвечает «какая это
  // комната», глиф — «зачем она». До него роль тайла жила только в цвете, то
  // есть в том, что игрок обязан выучить: склад от лаборатории отличался
  // оттенком синего.
  //
  // Проход второй, а не в том же цикле, намеренно: заливка и сетка — одна
  // `Graphics` на всю карту, а глифы делят контексты по типам, и мешать их в
  // одном узле нельзя. Дорого это не выходит — `drawMap` зовётся только когда
  // выросла `map_version`, а не каждый кадр (инвариант 3).
  const size = TILE * 0.62;
  for (let y = 0; y < meta.height; y++) {
    for (let x = 0; x < meta.width; x++) {
      const v = mapCells[y * meta.width + x];
      if (v < 0) continue;
      const ctx = tileGlyphContext(v);
      if (!ctx) continue;
      const node = new Graphics(ctx);
      node.scale.set(size / 24);
      node.x = x * TILE + (TILE - size) / 2;
      node.y = y * TILE + (TILE - size) / 2;
      // Приглушённо: глиф — подпись к клетке, а не её содержимое. Кучи лома,
      // чертежи и коты рисуются поверх и обязаны читаться первыми.
      node.alpha = 0.3;
      tileLayer.addChild(node);
    }
  }
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
      // Текст — единственное, что внутри `world` не векторное: он печётся в
      // текстуру один раз, и растянутый `world.scale` мылит его. Печём сразу в
      // том разрешении, в каком он окажется на экране. Пересоздаётся он каждым
      // кадром, так что смену масштаба подхватывает сам.
      resolution: app.renderer.resolution * worldScale,
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
  lastSnap = snap;
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
    // Разворот: куда шагнул, туда и смотрит, — и **остаётся** смотреть, встав.
    // Прошлая клетка лежит в `unitTiles` до перезаписи ниже, второго источника
    // для этого не нужно. Зеркалим силуэт, а не весь узел: метки состояний —
    // соседние дети контейнера, и они бы уехали вместе с ним.
    const was = unitTiles.get(e.id);
    if (was && was.x !== e.x) c.body.scale.x = e.x > was.x ? 1 : -1;
    // Силуэт пересобираем только на смене экипировки: `Graphics` строится
    // командами рисования, и безусловный `clear()` каждые 16 мс — это разбор
    // фигуры шестьдесят раз в секунду на каждого кота.
    const geared = (e.gear ?? []).length > 0;
    if (geared !== c.geared) {
      c.geared = geared;
      c.body.clear();
      drawCat(c.body, c.fur, geared);
    }
    c.load.visible = e.carrying > 0;
    // Глиф груза — **новый узел** на смене типа, а не подмена контекста у
    // старого: `Graphics` принимает общий контекст только конструктором
    // (`new Graphics(ctx)`), а присваивание `.context` живому узлу молча
    // ничего не рисует — кот носил пустой тёмный кружок. Смена типа редка
    // (ходка целиком идёт одним предметом, инвариант 12), так что пересоздание
    // здесь ничего не стоит.
    if (e.carrying > 0 && e.carrying_item !== c.loadItem) {
      c.loadItem = e.carrying_item;
      c.loadGlyph?.destroy();
      c.loadGlyph = null;
      const ctx = itemGlyphContext(e.carrying_item);
      if (ctx) {
        const size = TILE * 0.34;
        const gl = new Graphics(ctx);
        gl.scale.set(size / 24);
        gl.x = -size / 2;
        gl.y = -TILE * 0.52 - size / 2;
        c.load.addChild(gl);
        c.loadGlyph = gl;
      }
    }
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
  sales = snap.sales ?? [];
  favorites = snap.favorites ?? [];
  tickers = snap.tickers ?? [];
  stock = snap.stock ?? [];
  desks = snap.desks ?? [];
  posts = snap.posts ?? 0;
  autoOpen = {
    sales: !!snap.auto_sales,
    crafting: !!snap.auto_crafting,
    raids: !!snap.auto_raids,
  };
  // Правило живёт в ядре и сохраняется в снимке, поэтому вид его **читает**,
  // а не помнит (§12.96): от него зависит, есть ли у кучи кнопка «на склад».
  autoTidy = snap.auto_tidy ?? true;
  if (tidyBtn) tidyBtn.classList.toggle("on", autoTidy);
  postFree = !!snap.post_free;
  postLot = snap.post_lot ?? 0;
  shops = snap.shops ?? 0;
  relays = snap.relays ?? 0;
  nodes = snap.nodes ?? [];
  scrapEl.innerHTML =
    (meta.items ?? [])
      .map((it, i) => {
        // Главное число — **учтённое**: склад минус бронь (§12.53). С §12.69 у
        // него один смысл на всё, что база делает наружу, — им и платят, и
        // торгуют. Валяющееся приписано отдельно и приглушённо: оно у базы есть,
        // но годится только на стройку внутри, и одно общее число ровно этим и
        // обманывало.
        const st = (snap.stock ?? [])[i] ?? { stored: 0, loose: 0, booked: 0 };
        const free = Math.max(0, st.stored - st.booked);
        const name = esc(it.label || it.id);
        const hint = [
          `${name}: на складе ${st.stored}`,
          st.booked ? `забронировано ${st.booked} под сделку` : "",
          st.loose
            ? `валяется ${st.loose} — годится на стройку, но платить и ` +
              `продавать этим нельзя, пока не убрано`
            : "",
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          `<span class="stock" title="${hint}">` +
          `${itemGlyph(i)}<b>${free}</b>` +
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
  // Приписка к парте меняется без участия игрока — кот доучился, и кнопка
  // обязана перестать быть «Снять с учёбы» тем же кадром (§12.84).
  syncTeachButtons();
  // После цикла по сущностям: панель читает `unitTiles`, и он обновлён выше.
  renderCellPanel(snap);
  renderCaptivePanel();
  renderMissionPanel(snap.missions);
  // После `renderMissionPanel`: он считает `running` — заказы, по которым отряд
  // уже вышел, — а строка отряда обязана гасить именно свою кнопку (§12.59).
  renderRaidsSection();
  // Штаб — после `renderMissionPanel` по той же причине: он читает `running`.
  // Перерисовывается он каждым кадром намеренно (§12.71): цена решения меняется
  // от щелчка по коту, и увидеть это игрок должен сразу, а не после закрытия.
  renderRaidWindow();
  renderResearchPanel(snap.research);
  renderCraftPanel(snap.crafting);
  renderTickers();
  renderTradePanel(snap.deals);
  syncRecruitButtons(snap.recruits);
  syncTopicButtons(snap.topics);
  syncStockWindow();
  syncTileButtons(snap.techs);
  renderNotePanel(snap.notes, snap.tick);
  renderGoalsPanel(snap.goals, snap.goals_required, snap);
}

// Силуэт кота (§12.109): уши, корпус, хвост — и жилет, когда кот экипирован.
//
// До этого кот был кругом, и три кота на базе различались **только заливкой**.
// Круг честно отвечал на «здесь кто-то есть» и молчал обо всём остальном: куда
// он идёт, надето ли на нём хоть что-нибудь. Ради этого приходилось кликать, то
// есть карта не отвечала на вопросы, ради которых на неё и смотрят.
//
// Рисуется примитивами, а не спрайтом, по трём причинам: масштабируется вместе
// с `world.scale` без второго набора картинок под каждый dpr, перекрашивается
// той же палитрой `COLORS.unit`, что и раньше (цветовой словарь не тронут), и
// не заводит загрузку ассетов ради фигурки в 24 px. Фотореализм из `mockup/`
// сюда не влезает вовсе — он ушёл в портреты панелей, где для него есть место.
//
// Порядок слоёв значим: хвост уходит **за** корпус, поэтому рисуется первым.
// Обводка — не `stroke` по общему контуру (он обвёл бы и границы ушей о
// корпус, превратив силуэт в чертёж), а тёмная копия шире фигуры под ней.
function drawCat(g, fur, geared) {
  const r = TILE * 0.3; // тот же радиус, что был у круга: метки над котом не съехали
  const dark = 0x0b0d12;

  // Хвост: тёмный потолще, поверх — цветной потоньше. Он же единственное, по
  // чему видно разворот у стоящего кота.
  for (const [w, col] of [
    [r * 0.5, dark],
    [r * 0.3, fur],
  ]) {
    g.moveTo(r * 0.5, r * 0.85)
      .quadraticCurveTo(r * 2.05, r * 1.5, r * 1.85, r * 0.05)
      .stroke({ color: col, width: w, cap: "round" });
  }

  // Корпус и уши — одной фигурой в двух размерах: тёмная подложка и мех.
  for (const [k, col] of [
    [1.16, dark],
    [1.0, fur],
  ]) {
    for (const sx of [-1, 1]) {
      g.moveTo(sx * r * 0.72 * k, -r * 0.4 * k)
        .lineTo(sx * r * 0.46 * k, -r * 1.3 * k)
        .lineTo(sx * r * 0.04 * k, -r * 0.62 * k)
        .closePath()
        .fill(col);
    }
    g.circle(0, 0, r * k).fill(col);
  }

  // Жилет — только на экипированном (§12.34). Это и есть ответ на «коты в
  // плащах и перчатках», который влезает в тайл: не текстура, а различимая
  // деталь силуэта. Гол кот или нет, теперь видно с карты, а не только из
  // карточки, — а зависит от этого сила отряда на вылазке.
  if (geared) {
    g.roundRect(-r * 0.66, -r * 0.12, r * 1.32, r * 0.86, r * 0.22)
      .fill({ color: dark, alpha: 0.85 });
    g.rect(-r * 0.66, r * 0.12, r * 1.32, r * 0.16).fill({
      color: 0xd6b26a,
      alpha: 0.9,
    });
  }
}

function createUnit(e) {
  const c = new Container();
  const body = new Graphics();
  drawCat(body, COLORS.unit[e.sprite] ?? COLORS.unitDefault, false);
  // Кольцо «кот застрял» — шире кольца выбора, чтобы читались вместе.
  const stuckRing = new Graphics();
  stuckRing
    .circle(0, 0, TILE * 0.52)
    .stroke({ color: COLORS.stuck, width: 2, alpha: 0.9 });
  stuckRing.visible = false;
  // Груз — глиф того, что кот несёт (§12.109). Был белый брусок, крашенный в
  // цвет предмета: он говорил «что-то несёт», но не «что». Теперь это тот же
  // значок, что стоит в шапке и в ценах, — ради чего единый словарь и заводился.
  // Тёмный кружок под ним нужен: паёк и лом кот носит над клеткой любого цвета.
  const load = new Container();
  const loadDisc = new Graphics();
  loadDisc.circle(0, -TILE * 0.52, TILE * 0.23).fill({
    color: 0x0b0d12,
    alpha: 0.85,
  });
  load.addChild(loadDisc);
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
  c.loadGlyph = null;
  c.body = body;
  c.fur = COLORS.unit[e.sprite] ?? COLORS.unitDefault;
  // Что уже нарисовано, помним на самом узле: силуэт пересобирается только
  // когда кот оделся или разделся, а не каждым кадром (тот же довод, что у
  // `syncTeachButtons`, §12.84 — безусловная перерисовка каждые 16 мс).
  c.geared = false;
  c.loadItem = -1;
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
  // Без слова «парта» — оно уже в заголовке панели; роль отвечает только на
  // «чему тут учат» (§12.80: строка, повторяющая соседа по экрану, лишняя).
  if (def.teaches) roles.push(`учит «${esc(skillLabel(def.teaches))}»`);
  if (def.lab) roles.push("лаборатория");
  if (def.shop) roles.push("мастерская");
  // Поста в списке нет (§12.81): «торговый пост» — это заголовок панели слово в
  // слово, то есть строка, которая не добавляет к имени ничего. Правило «за
  // ячейкой не работают, к ней возят» ушло туда же: оно одинаково у всех постов
  // навсегда, а панель говорит о том, что в **этой** ячейке сейчас.
  // Узла в списке нет по той же причине, что и склада (§12.80): тайл назван
  // «Узел связи», а «держит одну вылазку» — правило, которое ниже повторяла
  // строка про слоты. Что делает **эта** рация, говорит её собственная строка.
  if (def.solid) roles.push("стеллаж: пройти можно, остаться нельзя");
  return roles;
}

// Раздел тулбара, к которому относится клетка (§12.55). Клик по парте
// раскрывает «Обучение», по рации — «Вылазки»: игрок попадает откуда надо куда
// надо, а **управление остаётся в одном месте**.
//
// Пост, склад и — с §12.105 — мастерская сюда не входят вовсе, и это **не**
// «ведут в окно» (§12.101). Раздел тулбара раскрывается **рядом** с панелью
// клетки, а модальное окно накрывает и карту, и саму панель — то есть стирает
// ровно тот ответ, за которым игрок кликал. Клик по клетке отвечает «что
// здесь», и точка; склад открывают кнопкой в тулбаре и фишками в шапке.
//
// Мастерская потеряла свой раздел не потому, что он уехал в окно, а потому, что
// его больше нет: заказ живёт в строке предмета (§12.105). Клетка от этого не
// онемела — `craftCell` в панели говорит, что за заказ тут стоит, докуда дошёл,
// кто за ним и как его отменить. Это ровно тот же исход, что у склада и поста
// после §12.101: на «что здесь» отвечает панель, а не раздел.
//
// До §12.100 пост открывал рынок **первой** фракции — наугад, потому что пост
// это лицензия, а не прилавок, и «чья эта клетка» ответа не имеет. Разделов
// рынка больше нет, так что открывать по посту нечего, и это правильно: что в
// **этой** ячейке лежит и докуда дошло, говорит `dealCell` в панели.
//
// Кнопки «заказать здесь» на самой клетке при этом не появилось и с §12.105, и
// это не осторожность. Ядро не адресует работу месту: ячейку заказу выбирает
// `spare_shop_cell`, а игрок размечает работу и не выбирает исполнителя
// (§12.16). Такая кнопка обещала бы адресность, которой нет, — и с двумя
// мастерскими врала бы через раз. Адресация понадобится тогда, когда станки
// перестанут быть взаимозаменяемыми (тиры по скорости или свои рецепты) — вот
// тогда и вернуться.
function cellSection(def) {
  if (!def) return null;
  if (def.lab) return "Наука";
  if (def.teaches) return "Обучение";
  if (def.gate || def.relay) return "Вылазки";
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
  // невидимый второй шаг читается как «клик не сработал» (§4.4). На клетке с
  // ролью он значит не «пойдут», а саму роль (§12.85): обещать «пойдут» там, где
  // кот сядет учиться, — это то же молчание, только вслух.
  // ...но не тогда, когда второй клик уже ничего не изменит: кот приписан к
  // этой самой парте или к этой самой рации. Зелёная рамка это обещание
  // перемены, и стоять она обязана только там, где перемена будет (§12.85).
  // Про уже приписанного панель говорит строкой ниже — тускло, как факт.
  if (cellIsArmed() && !alreadyHere(def, x, y)) {
    const who = selectedUnits.map(esc).join(" · ");
    parts.push(
      `<div class="cell-armed">ещё клик сюда — ${
        deskWelcomes(def, x, y)
          ? `сядут учиться «${esc(skillLabel(def.teaches))}»: ${who}`
          : def?.relay && selectedUnits.length === 1
            ? `${who} сядет на связь, когда отсюда уйдёт отряд`
            : `пойдут: ${who}`
      }</div>`,
    );
  } else if (cellReleases()) {
    parts.push('<div class="cell-armed">ещё клик — снять выделение</div>');
  }

  // Приписанный, которого сейчас здесь нет, — единственное, чего по клетке не
  // видно: он придёт сам, и звать его второй раз не надо. Домен в строке не
  // повторяется (он выше, в роли), имя — тоже не повторяется, потому что
  // стоящего здесь кота называет «здесь: …» (§12.80).
  // Молчать вместо обещания нельзя: пропавшая рамка читается как «клик не
  // сработал» ровно так же, как её отсутствие на первом шаге (§4.4). Поэтому
  // про уже посланного панель говорит фактом — тускло, как про приписанного.
  if (selectedUnits.length === 1) {
    const cat = (lastSnap?.entities ?? []).find(
      (e) => e.id === selectedUnits[0],
    );
    if (
      cat &&
      cat.order_x === x &&
      cat.order_y === y &&
      !(cat.x === x && cat.y === y)
    ) {
      parts.push(`<div class="cat-sub">${esc(cat.id)} уже идёт сюда</div>`);
    }
  }

  if (def?.teaches && selectedUnits.length === 1) {
    const i = (meta.skills ?? []).findIndex((s) => s.id === def.teaches);
    const cat = (lastSnap?.entities ?? []).find(
      (e) => e.id === selectedUnits[0],
    );
    if (cat && cat.study === i && !(cat.x === x && cat.y === y)) {
      parts.push(`<div class="cat-sub">${esc(cat.id)} придёт сюда сам</div>`);
    }
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
          `${itemGlyph(s.item)}` +
          `${esc(itemLabel(s.item))} ${s.count}`,
      )
      .join(" · ");
    parts.push(`<div class="cat-sub">${chips}</div>`);
    // Пометка «на склад» объясняет, почему за кучей кто-то придёт, и с §12.95
    // она же единственное решение об этой клетке — поэтому стоит здесь
    // кнопкой, а не строкой: разметка рамкой была режимом ввода, который
    // держал на прицеле каждый клик по карте ради жеста в одну клетку.
    //
    // Но кнопка появляется, только когда решение и правда за игроком (§12.96).
    // Решает за него ядро в двух случаях, и оба сводятся к `mark_loose_scrap`:
    // на складской клетке пометка снимается **всегда** (лежащее в складе уже
    // дома), а вне склада при «Убирать сам» — ставится всегда. Кнопка там
    // отработала бы ровно один тик и вернулась обратно: не отказ, а качели,
    // которые игрок прочтёт как поломку. Остаётся факт строкой.
    const marked = piles.some((s) => s.marked);
    const auto = def?.capacity > 0 || autoTidy;
    if (auto) {
      if (marked) parts.push('<div class="cat-sub">помечено на склад</div>');
    } else {
      parts.push(
        `<button class="tool store-mark${marked ? " on" : ""}" data-key="store@${x}, ${y}"` +
          ` data-x="${x}" data-y="${y}">` +
          (marked ? "✓ помечено на склад" : "Пометить на склад") +
          "</button>",
      );
    }
  } else if (def?.capacity > 0) {
    // «Пусто» пишется только там, где что-то могло бы лежать (§12.80): на полу,
    // за партой и у рации куч не бывает вовсе, и отсутствие того, чего тут не
    // держат, — это строка ни о чём. У склада и стеллажа она осмысленна: рядом
    // стоит полоска «Занято N / C», и ноль в ней это состояние, а не пустота.
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

  // Сделка в ячейке поста — карточкой, а не строкой: у неё полоска (§12.82).
  // Стоит рядом с площадкой и по той же причине — обе отвечают на «докуда
  // дошло», и обе меряют это работой котов, а не тиками.
  if (def?.trade) parts.push(...dealCell(snap, x, y));

  // Заказ в ячейке станка — тем же порядком (§12.96). До него станок отвечал
  // одной строкой в `cellWork`, потому что заказ ячейке не принадлежал и
  // отменялся не здесь.
  if (def?.shop) parts.push(...craftCell(snap, x, y));

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

  cellEl.innerHTML = parts.join("");
  cellEl.hidden = false;
}

// Список котов узла связи: кто в его отряде, кто свободен, кто числится на
// другом узле (§12.61). Строка — переключатель, как чертёж в тулбаре: клик по
// своему вычёркивает, клик по чужому или свободному зачисляет сюда.
//
// Живёт список **только в штабе** (§12.80): рядом с ним стоит прогноз, который
// от него и зависит, — а в панели рации он был вторым таким же списком в
// колонке, где ни исход, ни причина отказа не помещаются.
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
  // Номер чужого отряда — тот же, что на вкладке штаба и в строке тулбара:
  // порядок `nodes` (§12.66); свой и ничей дают ноль. Координаты рации («узел
  // 3,11») называли ту же клетку, но не тот отряд, который игрок только что
  // листал вкладками, — и сопоставлять их приходилось глазами по карте.
  //
  // Отряд без своей рации (её снесли, пока приписка цела) — это ноль, то есть
  // «свободен»: отряда, из которого кота забирают, больше нет, и обещать его
  // строкой значило бы отправлять игрока искать несуществующую вкладку.
  const squadNo = (e) =>
    here(e)
      ? 0
      : nodes.findIndex((o) => o.x === e.crew_x && o.y === e.crew_y) + 1;
  const rank = (e) => (here(e) ? 0 : squadNo(e) ? 2 : 1);
  // Внутри группы — по полезности в поле, а не по алфавиту (§12.71): вопрос,
  // который игрок задаёт этому списку, ровно один — «кого взять», — и отвечать
  // на него порядком дешевле, чем заставлять сравнивать глазами. Ничья
  // разрешается ступенью проводника, потом именем: перестановки от кадра к
  // кадру недопустимы, а сила меняется редко (надел комбинезон, вырос уровень).
  cats.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (b.raid_force ?? 0) - (a.raid_force ?? 0) ||
      (b.guide_step ?? 0) - (a.guide_step ?? 0) ||
      (a.id < b.id ? -1 : 1),
  );
  // Чья реакция сейчас ведёт отряд: проводник — это **максимум** по готовым
  // (§12.70), поэтому «станет проводником» — сравнение с ним. Само правило
  // «проводник считается по реакции» живёт в ядре (`guide_step`), здесь только
  // максимум по уже приехавшим числам.
  const led = Math.max(
    0,
    ...cats
      .filter((e) => (node?.ready ?? []).includes(e.id))
      .map((e) => e.guide_step ?? 0),
  );

  // Кто из зачисленных пойдёт **сейчас**: список считает ядро (§12.70) — оно же
  // решает, кого `launch` возьмёт. Второй экземпляр этого правила в JS («спит,
  // значит не идёт») однажды разойдётся с ядром на первом же новом состоянии.
  const ready = new Set(node?.ready ?? []);
  const rows = cats.map((e) => {
    const mine = here(e);
    // Группа кота — та же четвёрка, что и `rank`, но с учётом готовности: идут,
    // зачислены и не идут, свободные, чужие. Считается здесь, чтобы порядок и
    // заголовки брались из одного места: разойдись они — кот оказался бы под
    // чужой подписью, а это хуже, чем список без подписей вовсе.
    const no = squadNo(e);
    const band = mine ? (ready.has(e.id) ? 0 : 1) : no ? 3 : 2;
    // Раненого ядро в отряд не пустит (§12.37), и молчащая строка читалась бы
    // как поломка: причину называем словом, как её называет кнопка вылазки.
    const hurt = wounded.has(e.id);
    // Пока узел ведёт вылазку, состав уже в поле и не переигрывается: для этого
    // есть отзыв (`cancel_mission`), а не правка списка.
    const off = hurt || !!node?.busy;
    const where = no ? ` · Отряд ${no}` : "";
    const note = hurt ? "ранен" : jobLabel(e) || "";
    // Зачислен, но сейчас не пойдёт: спит, ранен, ещё не вернулся. Отряд из
    // троих с силой 0 читается как поломка, хотя это ровно правило §12.70 —
    // берут **готовых**, а не всех. Поэтому такой кот помечен рамкой, а его
    // занятие перестаёт быть серой припиской: оно и есть причина, и стоит там
    // же, где игрок её ищет — на самом коте, а не в подписи под шапкой.
    //
    // Считается это только для своих: у чужого кота «спит» — не отказ, а просто
    // чем он занят, и красить его значило бы обещать, что после пробуждения он
    // куда-то пойдёт.
    const idle = band === 1;
    // Подсказка отвечает на вопрос своей группы. Своим — почему кот не пойдёт;
    // чужим — что клик их **заберёт**: перенос стоит один клик и молча снимает
    // прежнюю приписку (см. шапку функции), и это ровно то, что игрок делает не
    // глядя, набирая отряд из общего списка.
    const tip = idle
      ? `Сейчас не пойдёт: ${note || "занят"}`
      : no
        ? `Сейчас в Отряде ${no}: зачислить сюда — значит забрать оттуда`
        : "";
    const pick =
      `<button class="tool crew-pick${mine ? " on" : ""}${idle ? " idle" : ""}" data-id="${esc(e.id)}"` +
      ` data-x="${x}" data-y="${y}"${mine ? ' data-in="1"' : ""}${off ? " disabled" : ""}` +
      `${tip ? ` title="${esc(tip)}"` : ""}` +
      ">" +
      `<span class="crew-id">${esc(e.id)}${where ? `<i class="crew-at">${where}</i>` : ""}</span>` +
      fieldLine(e, led, node) +
      (note
        ? `<i class="crew-note${idle ? " bad" : ""}">${esc(note)}</i>`
        : "") +
      "</button>";
    // Дежурство — вторая кнопка той же строки: бонус отряду даёт та же клетка,
    // и разводить эти два решения по разным местам панели незачем. На узле без
    // `comms` дежурить незачем, и кнопки там нет вовсе (§12.60).
    if (!node?.comms)
      return { band, html: `<div class="crew-row">${pick}</div>` };
    const on = e.post_x === x && e.post_y === y;
    const duty =
      `<button class="tool crew-duty${on ? " on" : ""}" data-id="${esc(e.id)}"` +
      ` data-x="${x}" data-y="${y}"${on ? ' data-in="1"' : ""}` +
      ` title="${on ? "Снять приписку к рации" : "Приписать к рации: сядет на связь, как освободится"}">📻</button>`;
    return { band, html: `<div class="crew-row">${pick}${duty}</div>` };
  });

  // Четыре группы с подписями. Вопрос «почему сила 0, когда в отряде трое»
  // рамка на коте закрывает наполовину: она объясняет одного кота, а список
  // по-прежнему выглядит одним отрядом. Подписи отвечают на него сразу:
  // идут — вот эти, зачислены и не идут — вот эти, остальные тут ни при чём.
  //
  // «Прочие коты базы» одной группой были ловушкой: свободный кот и кот из
  // соседнего отряда стояли вперемешку под общей подписью, отличаясь только
  // серой припиской, — и набор третьего отряда молча разбирал два первых.
  // Поэтому чужие отделены своей группой и стоят **последними**: своё лежит
  // ближе, чужое дальше, а забрать оттуда по-прежнему можно одним кликом.
  //
  // Пока узел в поле, деления на «идут / не идут» не существует: состав уже
  // ушёл, ядро никого не готовит, и обе первые группы говорили бы о решении,
  // которого сейчас не принимают. Поэтому свои в это время идут одной группой.
  const heads = node?.busy
    ? ["Отряд узла", "Отряд узла", "Свободные коты", "В других отрядах"]
    : ["Идут", "В отряде, но не пойдут", "Свободные коты", "В других отрядах"];
  const groups = [];
  for (let band = 0; band < heads.length; band++) {
    const group = rows.filter((r) => r.band === band);
    // Пустая группа не пишется вовсе: «В отряде, но не пойдут · 0» — это
    // заголовок над пустотой, то есть шум ровно там, где всё в порядке.
    if (!group.length) continue;
    // Слитые группы (узел в поле) идут одним списком, а не двумя под одинаковой
    // подписью: разрыв между ними читался бы как деление, которого нет.
    const last = groups.at(-1);
    if (last?.head === heads[band]) last.rows.push(...group);
    else groups.push({ head: heads[band], rows: [...group] });
  }
  return groups.flatMap((g) => [
    `<div class="cat-sub crew-head">${g.head} · ${g.rows.length}</div>`,
    `<div class="crew-list">${g.rows.map((r) => r.html).join("")}</div>`,
  ]);
}

// Каков кот в поле — одной строкой под именем (§12.71). Строка широкая и живёт
// в окне: в тулбарной колонке она сжала бы имя до многоточия — одна из причин,
// по которым набор состава из панели рации ушёл целиком (§12.80).
//
// Три числа, и каждое отвечает на свой вопрос (§12.70). Вклад в силу — «сколько
// он добавит»: сила складывается, поэтому «+2» это точное обещание, а не намёк.
// Проводник — «с ним не пропадём»: он **максимум** по отряду, значит важно не
// само число, а побьёт ли оно нынешнее, — так и пишем. Выносливость — «как
// часто он в форме»: на исход вылазки она не влияет вовсе, и потому идёт
// последней и приглушённо.
//
// Все три приезжают из ядра (`raid_force`, `guide_step`, `stat_steps`): сила
// отряда считается одним выражением на прогноз и на уход (инвариант 14), а
// какой параметр делает проводника — знание ядра, а не JS.
function fieldLine(e, led, node) {
  const parts = [
    `<b title="${esc(forceSplit(e))}">+${e.raid_force ?? 0}</b> сила`,
  ];
  // Проводник помечается в двух видах, и оба нужны: кто ведёт **сейчас** (это
  // говорит ядро — `node.guide`, там же разрешается ничья) и кто поведёт, если
  // его взять. Второе и есть ответ на «кого добавить»: опасность делится на
  // ступень лучшего, так что кот с реакцией выше нынешней меняет исход всей
  // бригады, а не только свою долю (§12.70).
  //
  // Сама ступень приезжает из ядра (`guide_step`): какой параметр ведёт отряд —
  // правило, и второй его экземпляр в JS однажды разойдётся с рулсетом.
  //
  // Пишем не саму реакцию, а **что она даёт** — процент, на который срежется
  // сложность (`guide_cut`, выведен из самой `raid_danger`). Ни сырое значение,
  // ни ступень для этого не годятся: ступеней у «Реакции» две (5 и 9), поэтому
  // «7» рядом с «5» обещает разницу, которой нет, а «2/4» читается как дробь,
  // которую хочется сократить или дозаполнить. Обе записи зовут сравнивать то,
  // чего игра не сравнивает.
  //
  // Само значение реакции при этом **показывается всегда** — и когда оно ничего
  // не даёт. Без него «сложность не изменит» выглядит утверждением о пустоте:
  // реакция у кота есть, число видно в его карточке, а строка молчит, будто
  // параметра нет вовсе. Выделяем его, только когда оно работает: подсвеченное
  // число значит «вот из-за чего вычет», обычное — «есть, но в дело не идёт».
  // Возражение против сырого числа (7 работает как 5) этим и снимается — оно
  // стоит не вместо следствия, а рядом с ним, и следствие его объясняет.
  const gi = meta.guide_stat ?? -1;
  const leads = node?.guide === e.id;
  if (gi >= 0) {
    const st = (meta.stats ?? [])[gi];
    parts.push(
      `<span class="stat${leads ? " on" : ""}">${esc(st?.label || st?.id || "")} ` +
        `${e.stats?.[gi] ?? 0}</span>`,
    );
  }
  const cut = e.guide_cut ?? 0;
  if (leads) {
    parts.push(`<u>ведёт</u> — сложность <b class="cut">−${cut} %</b>`);
  } else if ((e.guide_step ?? 0) > led) {
    // Именно это и есть ответ на «кого добавить»: у отряда уже есть какой-то
    // вычет, и взятый кот его не складывает, а **заменяет** собой.
    const now = ledCut(node);
    parts.push(
      `<u>станет проводником</u> — сложность <b class="cut">−${cut} %</b>` +
        (now > 0 ? ` вместо −${now} %` : ""),
    );
  }
  // Третьего случая в строке нет намеренно, и это не то же самое, что молчание
  // до §12.71 (§12.71). Тогда строка не говорила вообще ничего, и «нет пометки»
  // читалось как «нет данных». Теперь реакция показана всегда, и невыделенное
  // число само и есть ответ: параметр у кота есть, в дело не идёт. Приписка
  // «сложность не изменит» повторяла бы это словом — то есть писала бы «нет»
  // там, где ничего и не обещано.
  // Больше в строке ничего нет, и это решение, а не экономия места (§12.71).
  // Выносливость на исход вылазки не влияет вовсе, а «часто ли этот кот в
  // форме» видно занятием в той же строке. Сырые значения параметров живут в
  // карточке кота (§12.42) — там они отвечают на другой вопрос, про потолки
  // навыков, и там число уместно.
  return `<i class="crew-field">${parts.join(" · ")}</i>`;
}

// Из чего сложилась сила кота в поле. Сумма «+4» на вопрос «кого одеть, а кого
// брать, чтобы рос» не отвечает: опытный боец без комбинезона и новичок в
// комбинезоне выглядят одинаково. Слагаемые приезжают из ядра (`raid_skill`,
// `gear_force`), и сама сила там же из них и складывается, — здесь только
// подпись. Единица за самого кота не считается, а **выводится** остатком: своё
// «1» в JS было бы вторым экземпляром правила силы (инвариант 14).
function forceSplit(e) {
  const skill = e.raid_skill ?? 0;
  const gear = e.gear_force ?? 0;
  const own = (e.raid_force ?? 0) - skill - gear;
  const si = meta.raid_skill ?? -1;
  const label = (meta.skills ?? [])[si]?.label || "Вылазка";
  // Нулевые слагаемые не пишем: подсказка отвечает на «из чего эта сила», а
  // «снаряжение 0» — это не слагаемое, а его отсутствие. Сам кот остаётся
  // всегда: он единственный, кто не бывает нулём, и без него у новичка
  // подсказка оказалась бы пустой.
  const named = [
    `сам ${own}`,
    ...(skill > 0 ? [`${label} ${skill}`] : []),
    ...(gear > 0 ? [`снаряжение ${gear}`] : []),
  ];
  return named.join(" · ");
}

// Какой вычет сложности у отряда сейчас — берём у того, кто ведёт. Считать его
// из ступени нельзя: `2 + ступень` в JS это второй экземпляр `raid_danger`.
function ledCut(node) {
  const who = (lastSnap?.entities ?? []).find((e) => e.id === node?.guide);
  return who?.guide_cut ?? 0;
}

// Что происходит в этой клетке прямо сейчас — строками, в порядке её свойств.
//
// Станок сюда больше не входит: с §12.96 заказ принадлежит ячейке, и отвечает
// за неё карточка `craftCell` — там же, где стоит его отмена. Остальное
// опознаётся **по координатам**, а не по «идёт ли вообще работа»: комнат одного
// вида несколько, и «здесь делают деталь» на пустом соседнем верстаке было бы
// враньём ровно того сорта, каким врала шапка до §12.53.
function cellWork(snap, x, y, def) {
  if (!def) return [];
  const out = [];
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
  // Ячейка поста говорит не строкой, а карточкой, и живёт та в `renderCellPanel`
  // рядом с площадкой (§12.82): у неё полоска наполнения, а `cellWork` умеет
  // отдавать только строки.
  //
  // Узел без своего `node` — это рация, снесённая между кадрами: говорить о
  // несуществующем отряде нечего, и панель молчит.
  const node = def.relay ? nodeAt(x, y) : null;
  if (node) {
    // §12.59 дал узлу слот, §12.60 — смысл сидеть за ним. Панель узла — сводка
    // и ни одной кнопки (§12.80): отряд здесь называется, а набирается в штабе.
    //
    // Строк ровно три, и каждая про **эту** рацию: кто в отряде, чем узел занят
    // и есть ли связь. Общего счётчика слотов («вылазок идёт 2 из 2») тут нет:
    // он не про клетку, а своя строка узла отвечает на тот же вопрос точнее.
    const raid = (snap.missions ?? []).find(
      (v) => v.node_x === x && v.node_y === y,
    );
    // Номер отряда — порядок `nodes`, тот же, что на строке тулбара и на
    // вкладке штаба (§12.66). Здесь он главное слово панели: решение
    // принимается не тут, и сказать надо, какую строку открывать.
    const n = nodes.indexOf(node) + 1;
    // Состав живёт на клетке и переживает вылазку (§12.61). Кто из него не
    // уйдёт прямо сейчас — тусклым, ровно как в строке тулбара (§12.70):
    // «в отряде трое, а сила ноль» без этой пометки читается поломкой.
    //
    // Указатель в тулбар приписан сюда же и только к пустому отряду (§12.80):
    // это единственное состояние, когда идти отсюда куда-то надо. У набранного
    // он повторял бы то, что клик по клетке уже сделал, — раскрыл «Вылазки».
    out.push(
      node.crew.length
        ? `Отряд ${n}: ${node.crew
            .map((id) =>
              (node.ready ?? []).includes(id)
                ? esc(id)
                : `<span class="dim">${esc(id)}</span>`,
            )
            .join(" · ")}`
        : `Отряд ${n}: пусто — собрать в «Вылазках»`,
    );
    if (!raid) {
      out.push("свободен");
    } else {
      const label = missionLabel(raid.def);
      out.push(
        raid.away
          ? `«${esc(label)}» · вернутся через ${raid.left}`
          : `«${esc(label)}» — отряд ещё собирается`,
      );
    }
    // Дежурный на связи (§12.60). На узле без `comms` дежурить незачем, и там о
    // связи молчим вовсе. Прибавка пишется, только когда она есть: «+0 к силе
    // отряда» у пустой рации — это цифра, объясняющая отсутствие цифры.
    if (node.comms) {
      const on = (snap.entities ?? []).find(
        (e) => e.job === "relay" && !e.moving && e.x === x && e.y === y,
      );
      const gain =
        raid?.away && raid.comms > 0 ? ` · +${raid.comms} к силе` : "";
      out.push(on ? `на связи ${esc(on.id)}${gain}` : `связи нет${gain}`);
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
  // Проводник помечается прямо в составе, а не отдельной строкой: «ведёт sp2»
  // под списком, где sp2 уже стоит, — это то же имя дважды. Подчёркивание —
  // та же пометка, которой ведущего отмечает штаб (`unitRaidLine`).
  parts.push(
    `<div class="cat-sub">${
      m.squad
        .map((n) =>
          n === m.guide
            ? `<u title="ведёт отряд: режет сложность">${esc(n)}</u>`
            : esc(n),
        )
        .join(" · ") || "—"
    }</div>`,
  );
  // Связь (§12.60). Число — **накопленное**, то есть что будет, если связь
  // оборвётся прямо сейчас: она копится за тик, а не меряется одним замером,
  // и прогноз честно растёт вместе с ней. Говорим и то, держат ли её сейчас, —
  // иначе просевший на возвращении бонус выглядел бы необъяснимым.
  if (m.away) {
    // Дежурного зовут по имени: «связь держат» не говорит, кого игрок потеряет
    // из работы и кого нельзя трогать. Ищем его так же, как панель клетки узла
    // (`relayLines`) — по занятию на клетке узла этой вылазки.
    const on = (lastSnap?.entities ?? []).find(
      (e) =>
        e.job === "relay" && !e.moving && e.x === m.node_x && e.y === m.node_y,
    );
    const link = m.manned
      ? `на связи ${on ? esc(on.id) : "дежурный"}`
      : m.comms > 0
        ? "связь оборвалась"
        : "связи нет: за рацией никто не сидит";
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
        // Опасность едет сюда уже урезанной проводником (§12.70), и объяснять
        // её панель не пытается: исходную сложность игрок сравнивал в штабе —
        // до ухода, когда состав ещё выбирался, — а у ушедшей вылазки состав
        // заморожен (§12.22). Кто режет, помечено в самом составе выше.
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

// Панель заказов — **сводка, а не пульт** (§12.96). Кнопок в ней нет: заказов на
// один рецепт бывает несколько, и три строки «Деталь» подряд ничем не
// различаются — целиться было бы не во что. Решение про конкретный заказ живёт
// у его ячейки (`craftCell`), где написано, что за станок и докуда он дошёл;
// порог снимается там, где задавался, — строкой «делать до N» в окне «Склад»,
// и ноль там и есть снятие (§12.65, §12.100).
//
// Полоска у каждого — про **текущую штуку**, а не про весь заказ: работа и
// оплата идут поштучно, и «40% от пяти» игрок прочтёт неверно (§12.30).
// Живая подсказка (§12.107). Нативный `title` браузер рисует **один раз**, в
// момент показа: запись в атрибут открытую подсказку не перерисовывает, и та,
// что висит над числом, тикающим каждый кадр, показывает вчерашний день (у
// сводки заказов это видно глазом — в подсказке «осталось 15», а строкой выше
// уже «×10»). Поэтому у живых чисел подсказка своя: узел переписывается, пока
// висит.
//
// Заводится она **по элементу**, а не по месту вызова: текст ставит тот же код,
// что рисует строку, и обновляется он тем же кадром. Нативный `title` у такого
// элемента ставить нельзя — иначе подсказок будет две.
const liveTips = new WeakMap();
let liveTipFor = null;

function liveTitle(el, text) {
  const prev = liveTips.get(el);
  if (prev === text) return;
  liveTips.set(el, text);
  if (liveTipFor === el) showLiveTip(el);
}

// Подсказку держит один узел на всё приложение: окно «Склад» пересобирается при
// каждом открытии, и слушатель на каждом элементе копился бы вместе с ним.
function bindLiveTip(el) {
  el.addEventListener("mouseenter", () => showLiveTip(el));
  el.addEventListener("mousemove", moveLiveTip);
  el.addEventListener("mouseleave", hideLiveTip);
}

function showLiveTip(el) {
  const text = liveTips.get(el) ?? "";
  // Пустой текст и спрятанный элемент — не «подсказка без слов», а её
  // отсутствие: заказов нет, и говорить не о чем.
  if (!text || el.hidden) {
    hideLiveTip();
    return;
  }
  liveTipFor = el;
  if (liveTipEl.textContent !== text) liveTipEl.textContent = text;
  liveTipEl.hidden = false;
}

function moveLiveTip(e) {
  if (liveTipEl.hidden) return;
  // Ставим по курсору и зажимаем в окно: подсказка у правого края таблицы
  // иначе уезжает за экран, а прокрутки у неё нет.
  const box = liveTipEl.getBoundingClientRect();
  const vw = document.documentElement.clientWidth;
  const vh = document.documentElement.clientHeight;
  const x = Math.min(e.clientX + 14, vw - box.width - 8);
  const y = Math.min(e.clientY + 18, vh - box.height - 8);
  liveTipEl.style.left = `${Math.max(8, x)}px`;
  liveTipEl.style.top = `${Math.max(8, y)}px`;
}

function hideLiveTip() {
  liveTipFor = null;
  liveTipEl.hidden = true;
}

// Чем занят заказ — **одной формулировкой на оба места**: панель «Заказы» и
// сводка в окне «Склад» (§12.107) говорят про одно состояние, и два описания
// одного состояния разойдутся на первой же правке. Возвращает чистый текст:
// панель его экранирует сама, подсказке экранирование не нужно.
//
// Три разных «ничего не происходит», и путать их нельзя: некому взяться,
// материал ещё едет или работа идёт. С §12.102 второе стало счётным —
// материал везут ногами, и «сколько уже привезли» видно, как у площадки.
function craftStateText(c) {
  if (c.unit) return c.unit;
  return c.supplied ? "ждёт исполнителя" : `везут ${c.delivered} из ${c.need}`;
}

// Список приезжает отсортированным по клетке, как сделки (§12.81): рецепт
// заказы больше не различает, а закрывшийся сосед переставлял бы строки.
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
    const state = esc(craftStateText(c));
    parts.push(
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${esc(def?.label || def?.id || "Заказ")}</span><b>${pct}%</b></div>` +
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
        `<div class="cat-sub">осталось ${c.left} шт · ${state}${c.auto ? " · по порогу" : ""}</div>` +
        "</div>",
    );
  }
  craftEl.innerHTML = parts.join("");
  craftEl.hidden = false;
}

// Сделки, схлопнутые в группы (§12.81). Сделок идёт столько, сколько ячеек
// (§12.55), а ячеек к середине партии десятки — и панель, где карточка на
// каждую, перестаёт быть панелью: двадцать восемь карточек по четыре строки это
// не сводка, а список, который не читают. При этом отличались они **только
// координатами ячейки**, то есть тем, по чему решать нечего: отмены у сделки
// нет намеренно (§12.44), и «какая именно ячейка» — вопрос к ячейке, а не к
// панели.
//
// Сделка в ячейке поста — вторая половина границы, заведённой §12.81 (§12.82).
// Панель сделок говорит про **пачку**: сколько всего, на сколько денег и сколько
// контейнеров в какой фазе, — и про отдельную сделку нарочно молчит, потому что
// в пачке их двадцать восемь. Значит подробность обязана быть где-то одна, и
// это ячейка: сделка у неё ровно одна, места хватает, а спрашивают о ней, ткнув
// в контейнер на карте.
//
// Отсюда и полоска: она мерит **наполнение контейнера** (§12.68), то есть
// ходки котов, — ровно то же, что полоска площадки, и потому карточка стоит с
// ней рядом и выглядит так же. У покупки такой работы нет (товар везёт
// продавец), поэтому там полоски нет вовсе, а не пустая на ноль процентов.
//
// Курс пишется **зафиксированный**, а не сегодняшний, и той же формой, что в
// карточке пачки: рассчитаются именно по нему (§12.44), и две записи одного
// числа не должны выглядеть по-разному.
// Заказ в ячейке станка — карточкой, как сделка в ячейке поста (§12.96): у него
// есть полоска, а в отличие от сделки — ещё и отмена.
//
// **Кнопка живёт здесь, а не в панели заказов.** Заказов на один рецепт теперь
// бывает несколько, и три строки «Деталь» подряд не различить — целиться было
// бы не во что; ячейка же различает их полностью. Это то же правило, по
// которому §12.95 перенесла «на склад» в панель той клетки, которую метит.
//
// §12.80 при этом не нарушен: он запрещает панели клетки решения, **чью цену
// она не может показать**, — а тут вся цена написана прямо над кнопкой: что
// делают, сколько осталось и докуда дошла оплаченная штука.
function craftCell(snap, x, y) {
  const c = (snap.crafting ?? []).find((v) => v.x === x && v.y === y);
  if (!c) {
    return [
      '<div class="cat-sub">станок свободен — заказы в окне «Склад»</div>',
    ];
  }
  const pct = c.total > 0 ? Math.round((c.progress / c.total) * 100) : 0;
  // Три разных «ничего не происходит», и путать их нельзя: некому взяться,
  // материал ещё едет или работа идёт (§12.102).
  const state = c.unit
    ? `работает ${esc(c.unit)}`
    : c.supplied
      ? "ждёт исполнителя"
      : `материал везут: ${c.delivered} из ${c.need}`;
  // Заказ правила отсюда не отменяют (§12.65): правило завело бы его обратно
  // тем же тиком, и кнопка читалась бы как поломка. Отменяют его снятием
  // порога — там же, где порог и задавали.
  const foot = c.auto
    ? '<div class="cat-sub">по порогу — снимается в окне «Склад»</div>'
    : `<button class="tool craft-cancel" data-key="craft@${x}, ${y}" ` +
      `data-x="${x}" data-y="${y}"><span>Отменить</span></button>`;
  return [
    '<div class="cat-skill">' +
      `<div class="cat-row"><span>Заказ: ${esc(recipeLabel(c.def))}</span><b>${pct}%</b></div>` +
      `<div class="bar"><i style="width:${pct}%"></i></div>` +
      `<div class="cat-sub">осталось ${c.left} шт · ${state}</div>` +
      foot +
      "</div>",
  ];
}

function dealCell(snap, x, y) {
  const d = (snap.deals ?? []).find((v) => v.x === x && v.y === y);
  if (!d) {
    // Привезённое занимает ячейку, пока его не увезут (§12.68). Без этой строки
    // затор выглядит поломкой, а не работой, которую надо доделать.
    const jam = (snap.stacks ?? []).some((s) => s.x === x && s.y === y);
    return [
      `<div class="cat-sub">${
        jam ? "занята привезённым — пока не вывезут, пост занят" : "свободна"
      }</div>`,
    ];
  }
  const item = (meta.items ?? [])[d.item];
  const who = (meta.factions ?? [])[d.faction];
  const name = esc(item?.label || item?.id || "товар");
  const head = `${d.buying ? "Покупка" : "Продажа"}: ${name}`;
  const price =
    `${esc(who?.label || "—")} · ${d.count} шт по ${d.unit} = ` +
    `${d.unit * d.count}¤`;
  if (d.buying) {
    return [
      '<div class="cat-skill">' +
        `<div class="cat-row"><span>${head}</span></div>` +
        `<div class="cat-sub">${price}</div>` +
        `<div class="cat-sub">в пути · приедет через ${d.left}</div>` +
        "</div>",
    ];
  }
  // У отгруженного контейнер полон по определению (иначе таймер бы не пошёл,
  // §12.68), так что сотня здесь — не округление, а состояние.
  const pct = d.count > 0 ? Math.round((d.delivered / d.count) * 100) : 0;
  const state =
    d.left > 0
      ? `отгружено · расчёт через ${d.left}`
      : `в контейнере ${d.delivered} из ${d.count} · набирают`;
  return [
    '<div class="cat-skill">' +
      `<div class="cat-row"><span>${head}</span><b>${pct}%</b></div>` +
      `<div class="cat-sub">${price}</div>` +
      `<div class="bar"><i style="width:${pct}%"></i></div>` +
      `<div class="cat-sub">${state}</div>` +
      "</div>",
  ];
}

// Ключ группы — **сторона и фракция**, и больше ничего (§12.83). Плашка сделок
// это сводка «с кем и на сколько», а не список заказов: заказов там до двадцати
// восьми, и любое деление мельче фракции возвращает стену. Сторона из ключа не
// уходит никогда — деньги входящие и исходящие в одну строку не складываются.
//
// Отсюда следует, что́ карточка вправе писать: **только складываемое**. Курс,
// размер заказа и таймер из неё ушли (§12.81), а «штуки» ушли вместе с ними: у
// разных товаров они не суммируются. Складываются ровно две вещи — контейнеры и
// котоденьги, и обе в карточке есть. Ярусом выше, у тикера, ключ уже предмет —
// там штуки законны, потому что складывать нечего (§12.100).
//
// Всё, что перестало помещаться, спрашивают у ячейки (§12.82): там сделка одна,
// и у неё есть и курс, и точный срок, и своя полоска.
//
// Порядок групп — порядок `deals`, а тот с §12.81 отсортирован ядром по клетке:
// обход ECS зависит от истории вставок, и закрывшаяся сделка переставляла бы
// карточки под курсором.
function dealGroups(deals) {
  const groups = new Map();
  for (const d of deals) {
    const key = `${d.buying}|${d.faction}`;
    let g = groups.get(key);
    if (!g) {
      g = {
        buying: d.buying,
        faction: d.faction,
        items: new Set(),
        deals: 0,
        money: 0,
        earned: 0,
        shipped: 0,
        filling: 0,
      };
      groups.set(key, g);
    }
    g.items.add(d.item);
    g.deals += 1;
    g.money += d.unit * d.count;
    // Докуда дошла пачка — считаем **деньгами**, а не штуками: штука образца и
    // штука лома в одной полоске весят поровну только по недосмотру. В группе
    // из одного товара это ровно та же дробь, что и по штукам, так что обычный
    // случай ничего не замечает.
    g.earned += d.unit * d.delivered;
    // У продажи таймер идёт с той минуты, когда контейнер набит целиком
    // (§12.68), поэтому `left > 0` и значит «отгружено». Заодно у такой сделки
    // `delivered == count`, и полоска складывается сама собой.
    if (d.left > 0) g.shipped += 1;
    else g.filling += 1;
  }
  return [...groups.values()];
}

// Панель «Торговля» (§12.100) — два яруса, и вопросы у них разные.
//
// **Сверху — тикеры**: строка на предмет, который игрок сам вынес на главный
// экран. Ключ здесь предмет, а не фракция, и это снимает ограничение §12.83, а
// не нарушает его: штуки убрали из карточки потому, что она группировала по
// фракции и складывала лом с образцами. Строка отвечает «почём это сейчас», и по
// ней же торгуют в один клик. Склад и действующее правило в неё не идут: лента
// стоит на главном экране ради курса, а «сколько у меня» и «сбывать сверх N» —
// это вопросы к окну «Склад», где они и правятся.
//
// **Снизу — плашки сделок**, ровно те, что были до §12.100: группа на сторону и
// фракцию, и в ней только складываемое (§12.83). Они **не про тикеры**, а про
// всё, что едет: сделка по незакреплённому товару обязана быть видна, иначе
// игрок узнаёт о ней, только открыв окно.
//
// Подробностей про **одну** сделку здесь нет намеренно (§12.82): ни курса, ни
// срока, ни «расчёт через N» — это вопрос к ячейке поста, где сделка одна.
//
// Кнопки сделки в тикере законны по доводу §12.75 («Снять» и «Отозвать» у
// вылазок): это повторение уже принятого решения, а цена написана в той же
// строке. Там же и «×» — снять тикер: закрепил игрок здесь, снимать его,
// открывая окно, было бы дорогой в один конец.
// Лента тикеров — **своя панель и самая верхняя** в правой колонке. С панелью
// «Торговля» она разошлась не по смыслу, а по поведению: панели ниже (заказы,
// сделки, миссия) появляются и пропадают сами, и лента, стоявшая под ними,
// уезжала из-под курсора между двумя кликами — «Продать ×25» второй раз подряд
// нажать было нельзя. Наверху над ней не всплывает ничто, поэтому кнопка стоит
// на месте. Порядок панелей здесь — не украшение, а условие того, что по кнопке
// можно попасть.
function renderTickers() {
  syncTradeButtons();
  if (!tickers.length || !meta) {
    tapeEl.hidden = true;
    return;
  }
  const parts = ['<div class="cat-name">Торговля</div>'];
  const qty = dealSize(shiftHeld);
  // То же правило, что в окне (§12.100): нет поста — кнопок сделки нет вовсе, а
  // причина написана красным **один раз**, а не в каждой строке. Тикеры при этом
  // остаются: курс читать по-прежнему можно, а пост игрок отстроит.
  if (!posts) parts.push('<div class="warewin-warn">Нет «Торгового поста»</div>');

  for (const t of tickers) {
    const it = (meta.items ?? [])[t.item];
    if (!it) continue;
    const fac = (meta.factions ?? [])[t.faction];
    const q = quoteOf(t.faction, t.item);

    // Состояние кнопок считает `tradeState` — то же место, что и в окне
    // (§12.100): второй экземпляр этой арифметики однажды покажет живую кнопку,
    // которую фасад отклонит.
    const acts = [true, false]
      .map((buying) => {
        const s = tradeState(t.faction, t.item, buying, qty);
        return (
          `<button class="tool tick-deal${s?.ready ? " on" : " off"}" ` +
          `data-key="${buying ? "buy" : "sell"}@${t.item}" ` +
          `data-item="${t.item}" data-faction="${t.faction}" ` +
          `data-buying="${buying ? "1" : ""}" title="${esc(s?.title ?? "")}">` +
          `<span>${buying ? "Купить" : "Продать"}</span>` +
          `<b class="qty">×${qty}</b></button>`
        );
      })
      .join("");

    parts.push(
      '<div class="cat-skill tick-row">' +
        // Три яруса, а не один ряд: колонка узкая и фиксированной ширины, а
        // «Комбинезон · Синдикат» плюс два курса плюс «×» в неё не влезают
        // никогда. В одну строку они переносились по словам, и строка ленты
        // расползалась на четыре высоты, обрезая кнопки сделки по краю окна.
        '<div class="cat-row"><span class="tick-name">' +
        `${itemGlyph(t.item)}` +
        `${esc(it.label || it.id)} · ${esc(fac?.label || fac?.id || "—")}</span>` +
        // Снять тикер — там же, где он работает. Ключ свой, иначе `onPanelClick`
        // спутал бы его с кнопками сделки той же строки.
        `<button class="tool tick-off" data-key="off@${t.item}" ` +
        `data-item="${t.item}" title="Убрать из ленты">×</button>` +
        "</div>" +
        `<div class="tick-rate">${rateText(q, true)}` +
        `<span class="ware-sep">·</span>${rateText(q, false)}</div>` +
        (posts ? `<div class="cat-act">${acts}</div>` : "") +
        "</div>",
    );
  }

  tapeEl.innerHTML = parts.join("");
  tapeEl.hidden = false;
}

// Панель «Сделки» — то, что едет: группа на сторону и фракцию, и в ней только
// складываемое (§12.83). Она **не про тикеры**: сделка по незакреплённому
// товару обязана быть видна, иначе игрок узнаёт о ней, только открыв окно.
function renderTradePanel(list) {
  const deals = list ?? [];
  if (!deals.length || !meta) {
    dealEl.hidden = true;
    return;
  }
  const parts = [];

  for (const g of dealGroups(deals)) {
    const who = (meta.factions ?? [])[g.faction];
    // Товары — по индексу палитры, а не по порядку встречи: иначе строка
    // переставлялась бы от того, какую сделку закрыли раньше.
    const what = [...g.items]
      .sort((a, b) => a - b)
      .map((i) => esc(itemLabel(i)))
      .join(" · ");
    // «×5 сделок» стоит у заголовка: это ответ на «куда делись пять ячеек» —
    // счёт слотов и счёт карточек разошлись намеренно (§12.81).
    const many = g.deals > 1 ? ` · ×${g.deals}` : "";
    // Заголовок — только сторона сделки и счёт: у карточки он играет ту же роль,
    // что «Логово рейдеров» у вылазки, — чем это вообще является. **Кто именно
    // покупает — свойство сделки**, а не её название, и стоит оно строкой ниже,
    // среди прочих свойств: с двумя-тремя карточками заголовок из трёх частей
    // читался длиннее, чем всё, что под ним.
    const rows = [
      `<div class="cat-name">${g.buying ? "Покупка" : "Продажа"}${many}</div>`,
      `<div class="cat-sub">${esc(who?.label || "—")} · ${what}</div>`,
    ];
    if (g.buying) {
      // Покупке нечего показывать, кроме того, что она едет: контейнер набивает
      // продавец, а не коты, и делать с этим нечего. Денег тут тоже нет — за
      // покупку списали сразу, в момент заказа (§12.44), и «на 340¤» рядом со
      // строкой продажи прочиталось бы как второй доход.
      rows.push('<div class="cat-sub">в пути</div>');
    } else {
      // Полоска мерит, докуда дошёл **доход**: у отгруженного контейнера
      // `delivered == count` (иначе таймер бы не пошёл), так что сумма и есть
      // «сколько из ожидаемого уже свезли на посты».
      const pct = g.money > 0 ? Math.round((g.earned / g.money) * 100) : 0;
      // Считаем контейнеры, а не тики. Числа при одной сделке не пишем: «×1» в
      // заголовке нет по той же причине, и «отгружено 1» читалось бы как счёт
      // чего-то другого.
      const state = [];
      if (g.shipped)
        state.push(g.deals > 1 ? `отгружено ${g.shipped}` : "отгружено");
      if (g.filling)
        state.push(g.deals > 1 ? `набирают ${g.filling}` : "набирают");
      // Ожидаемый доход — то, ради чего панель и открывают. Пишется он всегда,
      // в том числе у целиком отгруженной пачки: деньги приходят разом по
      // истечении срока (§12.68), и до тех пор не заплачено ничего.
      state.push(`доход ${g.money}¤`);
      rows.push(
        `<div class="bar"><i style="width:${pct}%"></i></div>` +
          `<div class="cat-sub">${state.join(" · ")}</div>`,
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

  // Оба перехода считаются по одному множеству — тому, что было в прошлом кадре.
  const doneNow = new Set(goals.filter((g) => g.done).map((g) => g.def));
  const first = goalsDoneSeen === null;
  // Мир, поднятый уже пройденным (обновление страницы, загрузка снимка), панель
  // тоже не разворачивает: перехода к полноте на первом кадре не случается —
  // значит, свернуть её финалу будет уже нечем.
  if (first && done >= required) goalsOpen = false;
  const fresh = first
    ? []
    : goals.filter((g) => g.done && !goalsDoneSeen.has(g.def));
  // Финал — по **переходу** к полноте, а не по факту полноты: иначе он всплывал
  // бы каждым кадром после закрытия.
  const finale = !first && goalsDoneSeen.size < required && done >= required;

  if (finale) {
    // Список целей своё отслужил: всё в нём закрыто, и финал уже перечислил это
    // разом. Сворачиваем панель — но кнопку оставляем, и открыть её обратно
    // никто не мешает.
    goalsOpen = false;
    showFinale(goals, snap);
  }
  // Уведомления **и о скрытых тоже**: взятая скрытая цель — это ровно тот момент,
  // ради которого её прятали, и промолчать о нём значит спрятать её насовсем.
  // А вот вместе с финалом их не показываем: модал уже перечисляет всё разом, и
  // семь всплывающих поверх него — это шум, а не сведения.
  if (!finale) fresh.forEach((g) => showGoalToast(g));

  goalsDoneSeen = doneNow;
  // В самом конце: до сюда `goalsOpen` мог свернуться и финалом, и первым кадром
  // уже пройденного мира.
  goalsEl.hidden = !goalsOpen;
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
// Сядет ли выбранный кот за эту парту по второму клику (§12.85). Условия те же,
// что у `teach_at` в ядре, — но здесь они только выбирают слово: ошибись эта
// проверка, и хуже обещания «пойдут» не станет, а команду всё равно решает ядро.
// Приписан ли выбранный кот **к этой самой клетке** (§12.85): за эту парту или
// на эту рацию. Второй клик тогда ничего не меняет, и обещать перемену нечем.
function alreadyHere(def, x, y) {
  if (selectedUnits.length !== 1) return false;
  const cat = (lastSnap?.entities ?? []).find((e) => e.id === selectedUnits[0]);
  if (!cat) return false;
  // Кот уже послан сюда приказом (§12.86): второй клик повторит тот же приказ,
  // то есть не изменит ничего. Проверка стоит **до** ролей, потому что верна
  // для любой клетки — у пола других причин «уже здесь» нет вовсе.
  if (cat.order_x === x && cat.order_y === y) return true;
  if (def?.relay) return cat.post_x === x && cat.post_y === y;
  if (!def?.teaches) return false;
  const i = (meta.skills ?? []).findIndex((s) => s.id === def.teaches);
  // У парты приписка к **домену**, а не к клетке (§12.84), поэтому «уже здесь»
  // значит «уже учится этому»: кот, идущий к соседней парте того же домена,
  // тоже никуда не денется. Позицию сюда добавлять нельзя — на дороге к парте
  // рамка снова загорелась бы, а это ровно тот случай, на котором её и
  // поймали: она обещает перемену коту, который и так идёт.
  return cat.study === i;
}

function deskWelcomes(def, x, y) {
  if (!def?.teaches || selectedUnits.length !== 1) return false;
  const i = (meta.skills ?? []).findIndex((s) => s.id === def.teaches);
  const cat = (lastSnap?.entities ?? []).find((e) => e.id === selectedUnits[0]);
  const skill = cat?.skills?.[i];
  if (!skill || skill.xp >= skill.desk) return false;
  // Занятую парту ядро отклонит, и клик останется приказом: обещать за неё
  // учёбу — это соврать ровно там, где игрок и так удивится (§12.20).
  return !(lastSnap?.entities ?? []).some(
    (e) => e.id !== cat.id && e.job === "study" && e.x === x && e.y === y,
  );
}

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
  worker.postMessage({ type: "build", ...rect, tile: buildTile });
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
  const def = tile >= 0 ? meta.palette[tile] : null;
  const section = def ? cellSection(def) : null;
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
  stageEl.classList.remove("mode-cursor", "mode-build", "mode-erase");
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
// Набор состава и приписка связиста (§12.60, §12.61) — тоже исключение, но с
// §12.80 по другой причине: обе уходят **из штаба**, а он модальное окно поверх
// карты. Клетку под ним игрок не разглядывает и не выбирал заново — снимать её
// на каждый щелчок по коту значит молча стирать выделение, к которому он
// вернётся, закрыв окно.
const KEEPS_CELL = new Set([
  "move",
  "postRelay",
  "unpostRelay",
  "enlist",
  "dismiss",
  // Пометка «на склад» (§12.95) и отмена заказа (§12.96) — команды, кнопки
  // которых стоят в самой панели клетки: снять выделение значило бы закрыть
  // панель тем же кликом, которым её нажали. У пометки это сделало бы «снять
  // пометку» недостижимой, у заказа — спрятало бы ответ («станок свободен»).
  "store",
  "cancelCraft",
]);

function sendAction(msg) {
  worker.postMessage(msg);
  if (KEEPS_CELL.has(msg.type)) return;
  selectedCell = null;
  updateSelectionOverlay();
}

// Shift держат, а не нажимают: кнопки сделок и рецептов обязаны перерисоваться
// на зажатие и на отпускание. `blur` здесь не перестраховка — отпустят клавишу
// в другом окне, и кнопка навсегда останется «×25».
function setShift(on) {
  if (shiftHeld === on) return;
  shiftHeld = on;
  syncTradeButtons();
  syncCraftSize();
}

window.addEventListener("keydown", (e) => {
  // ⚠️ Пока печатают в поле, клавиши игры молчат (§12.92). Пробел ставит паузу,
  // а цифры переключают скорость — без этой строки набор «100» в пороге уводил
  // бы игру в ×1. Проверка общая, а не про конкретное поле: следующее заведут
  // через полгода и об этом не вспомнят.
  if (e.target instanceof HTMLInputElement) return;
  if (e.key === "Shift") setShift(true);
  if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.code === "Escape" || e.key === "Escape") {
    // Модальные окна закрываются первыми: они поверх всего, и пока открыто
    // такое окно, «отменить» может значить только «закрыть его» (§12.71).
    if (stockWinOpen) {
      closeStockWindow();
      return;
    }
    if (raidWinAt) {
      closeRaidWindow();
      return;
    }
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
    if (selectedCell || selectedUnits.length) {
      clearSelection();
      return;
    }
    // Снимать нечего — сворачиваем раздел тулбара. Последняя ступень
    // намеренно: раздел ничего не держит на прицеле и никого не выделяет,
    // он лишь занимает место, — а Escape отменяет по одному за нажатие,
    // начиная с самого навязчивого.
    if (openSection !== null) openOnly(null);
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

// Набранное в поле порога, но ещё не отданное ядру, досылаем: игрок ушёл из
// окна, ждать продолжения нечего, а потерять его настройку — хуже, чем
// применить её на мгновение раньше (§12.89). Слушатель один на всё приложение:
// у самих полей жизнь короче окна (окно «Склад» пересобирается на каждом
// открытии), и слушатель на каждом из них копился бы с ними.
window.addEventListener("blur", () => endNumEdit(true));
// Уход со вкладки — тот же случай: воркер в этот момент пишет автосейв (§12.45),
// и набранное обязано попасть в него, а не пропасть вместе со вкладкой. На
// `blur` окна тут полагаться нельзя: браузер не обязан гасить фокус поля,
// уводя вкладку в фон.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) endNumEdit(true);
});

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
    .map((it, i) => {
      const found = entries.find(([id]) => id === it.id);
      return found ? `${itemGlyph(i)}${found[1]}` : "";
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
//
// Начинаем со свёрнутого (`null` — законное состояние, а не «раздел
// потерялся»): первый экран партии — это карта и записка, а раскрытый раздел
// стоит одного клика, и этот клик и есть решение «хочу строить».
let openSection = null;

// Разделы-инструменты: они переключают режим разметки, поэтому в самом режиме
// остаются живыми (§12.62). Всё остальное в тулбаре и в правых панелях — это
// «управлять», а не «размечать», и на время разметки глушится: рука уже на
// карте, и клик мимо инструмента почти всегда промах, а не намерение.
// «Лом» здесь вместе с «Постройкой» намеренно: рамка на склад — та же разметка,
// и гонять игрока через курсор между двумя рамками было бы налогом на ровном
// месте.
const TOOL_SECTIONS = new Set(["Постройка"]);

function mkSection(el, title) {
  const sec = document.createElement("div");
  if (!TOOL_SECTIONS.has(title)) sec.classList.add("gated");
  const head = document.createElement("button");
  head.className = "sec-head";
  head.innerHTML = `<span>${esc(title)}</span><span class="chev">›</span>`;
  // Повторный клик по открытому разделу его закрывает: заголовок — это
  // переключатель, а не только «открыть». «Свёрнуто всё» и так законное
  // состояние (см. `openOnly`), просто попасть в него мышью было нельзя.
  head.addEventListener("click", () =>
    openOnly(openSection === title ? null : title),
  );
  const body = document.createElement("div");
  body.className = "sec-body";
  sec.appendChild(head);
  sec.appendChild(body);
  el.appendChild(sec);
  sections.push({ title, head, body });
  return body;
}

// `title === null` значит «свёрнуто всё» — состояние стартового экрана и
// законный выбор игрока, а не «раздел потерялся».
//
// Времени раздел не касается: разметка идёт при любом темпе (§12.86).
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

  // Склад — вне разделов, как и «Курсор», но по другой причине: это не режим, а
  // дверь в окно (§12.100). Раздел из одной кнопки был бы лишним кликом —
  // склад на базе один, выбирать в списке нечего.
  const ware = mkTool(
    '<span class="sw sw-scrap"></span><span>Склад</span>',
    () => openStockWindow(),
  );
  ware.title = "Что есть на базе, почём его берут и какие правила это двигают";
  el.appendChild(ware);

  const build = mkSection(el, "Постройка");

  tileButtons.length = 0;
  meta.palette.forEach((p, i) => {
    // Цена набором — рядом с образцом: что и сколько завезти на клетку.
    const cost = costChips(p.cost);
    // Образец тайла — глиф в цвете тайла, а не голая заливка (§12.109). Обе
    // вещи в одном месте: краска говорит, каким игрок увидит тайл на карте,
    // глиф — что клетка делает. Тот же глиф стоит и на самой клетке, поэтому
    // палитра и карта читаются одним словарём.
    //
    // Пол глифа не имеет, и подменять его на этот случай нечем — остаётся
    // заливка, как было у всех до §12.109.
    const glyph = TILE_GLYPHS[p.id]
      ? glyphHtml(`g-tile-${p.id}`, p.color, "sw-glyph")
      : `<span class="sw" style="background:${p.color}"></span>`;
    const b = mkTool(
      `${glyph}<span>${p.label || p.id}</span>${cost}`,
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

  // Правила симуляции — не режимы ввода, а тумблеры поведения котов, поэтому
  // они живут отдельно от инструментов и своей подсветкой их не сбивают.
  const rules = mkSection(el, "Правила");

  // Подсветку кнопки ведёт снимок, а не сам клик (§12.96): правда о правиле
  // живёт в ядре и переживает загрузку партии, а зеркало в виде — нет.
  const auto = mkTool(
    '<span class="sw sw-scrap"></span><span>Убирать сам</span>',
    () => worker.postMessage({ type: "setAutoTidy", on: !autoTidy }),
  );
  auto.classList.add("toggle", "on");
  auto.title = "Коты свозят лом на склад без разметки";
  rules.appendChild(auto);
  tidyBtn = auto;

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
    // Отправку заказом раздел больше не делает (§12.75): она вся в штабе, где
    // рядом стоят состав, прогноз и причина отказа. Здесь остаются только две
    // команды по уже принятому решению — снять правило и отозвать отряд.
    //
    // Снятие правила автовылазки (§12.67). Мир оно меняет — значит `sendAction`.
    // `data-def` уже несёт то, что надо отправить: `-1` у снятия.
    onPanelClick(raidsEl, ".raid-auto", (b) =>
      sendAction({
        type: "setAutoRaid",
        mission: Number(b.dataset.def),
        x: Number(b.dataset.x),
        y: Number(b.dataset.y),
      }),
    );
    // Пауза правила (§12.77): то же решение по уже принятому решению, что и
    // «Снять», только обратимое. `data-on` несёт направление переключения.
    onPanelClick(raidsEl, ".raid-pause", (b) =>
      sendAction({
        type: "setAutoRaidOn",
        on: !!b.dataset.on,
        x: Number(b.dataset.x),
        y: Number(b.dataset.y),
      }),
    );
    // Строка отряда открывает штаб (§12.71): состав и прогноз стоят там рядом,
    // и менять первый, глядя на второй, можно не закрывая окна. Это осмотр, а
    // не команда: `sendAction` тут не при чём — он бы ещё и гасил выделение.
    onPanelClick(raidsEl, ".raid-crew", (b) =>
      openRaidWindow(Number(b.dataset.x), Number(b.dataset.y)),
    );
    // Отзыв отряда — по заказу, а не по номеру строки (§12.59), ровно как в
    // панели миссии: порядок обхода сущностей ECS недетерминирован.
    onPanelClick(raidsEl, ".mission-cancel", (b) =>
      sendAction({ type: "cancelMission", mission: Number(b.dataset.def) }),
    );
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

  // Раздела «Производство» здесь нет с §12.105: заказ переехал в строку предмета
  // окна «Склад», к своему порогу «делать до N». Правило и разовый заказ — одно
  // решение с двух сторон, и цена у них общая; разложенные по разным экранам,
  // они заставляли игрока держать в голове то, что можно показать рядом.

  // Обучение. Кнопка адресная, а не разметка работы: игрок отправляет за парту
  // конкретного кота, и это решение о его судьбе (§12.18). Домены без `taught`
  // сюда не попадают — «Стройке» парта не нужна.
  const taught = (meta.skills ?? [])
    .map((s, i) => ({ s, i }))
    .filter(({ s }) => (s.taught ?? 0) > 0);
  if (taught.length) {
    const school = mkSection(el, "Обучение");

    teachButtons.length = 0;
    for (const { s, i } of taught) {
      // Одна кнопка на домен, а не две: «Учить» и «Снять» — это одно решение в
      // двух состояниях, и вторая кнопка стояла бы мёртвой в девяти случаях из
      // десяти. Что именно она сейчас делает, решает `syncTeachButtons` — он же
      // и пишет на ней слово.
      const b = mkTool("", () => {
        if (selectedUnits.length !== 1) return;
        if (b.classList.contains("off")) return; // отказ уже написан в подсказке
        const id = selectedUnits[0];
        sendAction(
          b.dataset.on === "1"
            ? { type: "unteach", id }
            : { type: "teach", id, skill: s.id },
        );
      });
      b.classList.add("toggle");
      b.dataset.skill = s.id;
      b.dataset.i = i;
      b.dataset.label = s.label || s.id;
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

  // Раскрыт тот раздел, что был открыт до перестройки; на первом кадре не
  // раскрыт никакой.
  // `null` — это выбор игрока «всё свёрнуто», а не «раздел потерялся», поэтому
  // подстановка палитры его не касается.
  openOnly(
    openSection === null || sections.some((s) => s.title === openSection)
      ? openSection
      : "Постройка",
  );
  selectCursor(cursorBtn); // режим по умолчанию
}

/// Курс фракции по предмету — из снапшота, где его посчитало ядро тем же
/// выражением, каким его посчитает заказ (§12.44). Второй арифметики цены в JS
/// быть не должно.
function quoteOf(faction, item) {
  return prices.find((p) => p.faction === faction && p.item === item);
}

// Размер сделки: клик — пять штук, Shift — **полный контейнер** (§12.90).
//
// Верхнее число перестало быть константой: контейнер это свойство поста, и
// когда наука даст пост побольше, Shift вырастет вместе с ним сам. Ноль от
// ядра значит «предела нет» (синтетические миры и база без постов) — там
// остаётся прежняя двадцатка с хвостиком.
//
// Клик тоже прижимается к контейнеру: обещать пять штук там, где влезает три,
// значит показывать кнопку, которую фасад отклонит (§12.53).
// Сколько штук уйдёт за клик по рецепту: клик — штука, Shift — пять (§12.30).
// Отдельной функцией по той же причине, по которой ею стал `dealSize`: число
// читают двое — обработчик клика и подпись «×N» на кнопке, — и разойтись им
// нельзя, иначе кнопка обещает не то, что отправляет.
function craftSize(shift) {
  return shift ? 5 : 1;
}

// Подпись «×N» у кнопок заказа в окне «Склад» (§12.105): сколько уйдёт **прямо
// сейчас**, с учётом зажатого Shift. Отдельной функцией, потому что от снапшота
// не зависит вовсе — только от клавиши, — и звать её надо на каждое нажатие и
// отпускание, а не раз в кадр. Обход `wareRows` пуст, пока окно закрыто, и
// звать её оттуда всё равно безопасно.
//
// ⚠️ Меняем текст **существующего** узла и только при изменении (§12.84):
// безусловный `innerHTML` кнопки пересоздавал бы её детей каждые 16 мс, и клик
// человека, длящийся сотни миллисекунд, не доходил бы вовсе — `mousedown` попал
// бы в один `<b>`, а `mouseup` в другой, уже несуществующий.
function syncCraftSize() {
  const qty = `×${craftSize(shiftHeld)}`;
  for (const r of wareRows) {
    for (const k of r.keeps) {
      const size = k.make.querySelector(".qty");
      if (!size) continue;
      if (size.textContent !== qty) size.textContent = qty;
      size.classList.toggle("big", shiftHeld);
    }
  }
}

function dealSize(shift) {
  const lot = postLot > 0 ? postLot : 25;
  return shift ? lot : Math.min(5, lot);
}

// Сколько предмета база вправе выставить на продажу — **то же самое, чем она
// платит** (§12.69): склад минус бронь. Учтённым база распоряжается наружу,
// неучтённое (пол, лапы, ячейки постов) годится только на стройку внутри.
//
// Отсюда и то, что считать здесь нечего: это ровно главное число из шапки, и
// кнопка гаснет тогда же, когда игрок видит в шапке ноль. До §12.69 продажа
// брала откуда угодно, и «сколько можно продать» было третьим смыслом слова
// «есть» — не выводимым ни из одного числа на экране.
function sellableOf(item) {
  const st = stock[item] ?? { stored: 0, booked: 0 };
  return st.stored - st.booked;
}

// Доступность сделки: пост считает ядро, деньги и товар — арифметика над уже
// названными им числами. Причину отказа называем словом: молчащая кнопка
// читается как поломка.
//
// Размер сделки следует за Shift, а не узнаётся в момент клика: кнопка,
// одинаково горящая на пять и на двадцать пять, врёт ровно тогда, когда хватает
// на первое и не хватает на второе.
// Можно ли эту сделку прямо сейчас — и если нет, то почему словом.
//
// **Одно место на оба экрана** (§12.100): кнопки стоят и в окне «Склад», и в
// ленте тикеров, а лента перерисовывается целиком каждым снапшотом, то есть
// своими узлами в `tradeButtons` числиться не может. Разведи эту арифметику по
// двум местам — и один экран однажды покажет живую кнопку, которую фасад
// отклонит (§12.26, §12.53). Пост считает ядро, деньги и товар — арифметика над
// уже названными им числами.
//
// Размер сделки следует за Shift, а не узнаётся в момент клика: кнопка,
// одинаково горящая на пять и на двадцать пять, врёт ровно тогда, когда хватает
// на первое и не хватает на второе.
function tradeState(fi, ii, buying, qty) {
  const q = quoteOf(fi, ii);
  if (!q) return null;
  const unit = buying ? q.buy : q.sell;
  const total = unit * qty;
  const broke = buying && money < total;
  const free = buying ? 0 : sellableOf(ii);
  const empty = !buying && free < qty;
  // Расписание видно вперёд — это и есть разница между планированием и
  // караулом с секундомером (§12.40). В строку оно не идёт (§12.100): там
  // стоит дельта к прошлой фазе, а прогноз распирал бы её.
  const next = buying ? q.next_buy : q.next_sell;
  const ahead =
    q.next_in > 0 && next !== unit ? ` · через ${q.next_in} станет ${next}¤` : "";
  // Правило автопродажи живёт своей строкой (§12.88), и кнопка «Продать»
  // обязана про него сказать: иначе игрок, у которого излишек уходит сам,
  // ищет причину не там, где принимал решение (§12.64).
  const rule = buying ? null : saleOf(ii);
  const auto = rule
    ? ` · излишек сверх ${rule.keep} уходит «${
        (meta.factions ?? [])[rule.faction]?.label ?? "?"
      }» сам`
    : "";
  const title = !posts
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
              shiftHeld ? "" : ` · Shift — полный контейнер (${dealSize(true)})`
            }${ahead}${auto}`;
  return { q, unit, ready: postFree && !broke && !empty, title };
}

// Кнопки сделок в окне «Склад»: состояние на них наводит `tradeState`, а сам
// список узлов живёт, пока окно открыто (§12.100).
function syncTradeButtons() {
  const qty = dealSize(shiftHeld);
  for (const b of tradeButtons) {
    const fi = Number(b.dataset.faction);
    const ii = Number(b.dataset.item);
    const buying = !!b.dataset.buying;
    const st = tradeState(fi, ii, buying, qty);
    if (!st) continue;
    // ⚠️ Гасим **классом**, а не `disabled` (§12.53, §12.71): по выключенному
    // элементу браузер не шлёт событий мыши, и причина отказа, которую только
    // что посчитал `tradeState`, не показалась бы **никогда**. До §12.100 эти
    // кнопки стояли в тулбаре и гасились `disabled` — то есть молчали ровно
    // тогда, когда игроку и надо было объяснить, почему сделка не идёт.
    b.classList.toggle("off", !st.ready);
    b.classList.toggle("on", st.ready);
    const size = b.querySelector(".qty");
    if (size) {
      size.textContent = `×${qty}`;
      size.classList.toggle("big", shiftHeld);
    }
    b.title = st.title;
  }
}

// Курс словом: число и то, куда оно шагнуло с прошлой фазы (§12.100).
//
// Дельта, а не прогноз вперёд: строка одна, и «через 240 станет 7¤» её
// распирает. Прогноз при этом не потерян — он в подсказке кнопки (`tradeState`),
// там же, где жил и до §12.100.
function rateText(q, buying) {
  if (!q) return "—";
  const now = buying ? q.buy : q.sell;
  const was = buying ? q.prev_buy : q.prev_sell;
  // `next_in === 0` — расписание не меняется вовсе: сравнивать не с чем, и
  // стрелка обещала бы движение, которого не будет.
  if (!q.next_in || was === now) return `${now}¤`;
  const up = now > was;
  return `${now}¤ <i class="delta ${up ? "up" : "down"}">${up ? "▲+" : "▼−"}${Math.abs(now - was)}</i>`;
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

// Раздел, в котором не осталось ни одной живой кнопки, прячется целиком (§12.94):
// пустой заголовок обещает выбор, которого нет.
function syncSectionRows(title, buttons) {
  const sec = sections.find((s) => s.title === title);
  if (!sec) return;
  sec.head.parentElement.hidden = buttons.every((b) => b.hidden);
}

// Доступность кандидата считает ядро (известность + содержимое склада), здесь
// только показываем: дублировать правило в JS значит однажды показать кнопку,
// которую ядро отклонит.
function syncRecruitButtons(list) {
  recruitButtons.forEach((b, i) => {
    const r = (list ?? [])[i];
    if (!r) return;
    // Нанятый кандидат уникален и второй раз не придёт (§4.2, §12.94), поэтому строка
    // его — не выбор, а память. Убираем совсем: список найма про тех, кого
    // ещё можно позвать.
    b.hidden = !!r.hired;
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
  syncSectionRows("Найм", recruitButtons);
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
    // Технологии не забываются (§12.18), значит изученная тема — это уже не решение (§12.94).
    // Прячем её, как и тему, запертую другими технологиями: список
    // показывает то, за что можно взяться сейчас. Остальные причины отказа
    // (склад, допуск, лаборатория) кнопку не прячут — они про «пока нечем»,
    // а не про «пока нельзя выбрать».
    b.hidden = !!t.known || !t.unlocked;
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
  syncSectionRows("Наука", topicButtons);
}


// Число порога, которое **правится на месте** (§12.92): клик по нему — и это
// уже поле ввода, Enter применяет, Escape отменяет. С §12.108 это единственный
// способ его задать: кнопок шага рядом больше нет.
//
// Порог — число, которое у игрока уже в голове («держать пятьсот»), а не
// величина, которую нащупывают: кнопками пятьсот набиралось тремя секундами
// удержания, и промах на семь штук стоил семи кликов. Правка на месте оставляет
// правило там же, где стоит команда (§12.64), и не утолщает строку.
//
// Ею же закрыт и §12.89: **промежуточных значений больше нет вовсе**. Пока
// набирают «1000», ядро не видит ни «1», ни «10» — а видело бы, и на «сверх 1»
// автопродажа успевала выставить весь склад сделкой, которую не отменить
// (§12.44). «Набор устоялся» перестало быть догадкой по таймеру и стало
// нажатием Enter.
let numEdit = null;

function mkKeepLabel(key, read, write) {
  const label = document.createElement("span");
  label.className = "keep-val";
  label.title = "Клик — ввести число";
  label.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault(); // иначе начинается выделение текста, а не правка
    startNumEdit(key, label, read, write);
  });
  return label;
}

// Правится ли сейчас эта строка. Спрашивают подписи: пока игрок печатает, его
// текст затирать снапшотом нельзя — а подписи зовутся каждым кадром (§12.84).
function numEditing(key) {
  return !!numEdit && numEdit.key === key;
}

// Число, которое игрок печатает в этой строке **прямо сейчас**; `null` — поля
// нет или в нём мусор. Спрашивает это смена стороны у порога сбыта: правило
// переезжает к другому покупателю вместе с набранным, а снапшот про набранное
// ещё не знает (§12.88).
function numPending(key) {
  if (!numEditing(key)) return null;
  const v = Number.parseInt(numEdit.input.value.trim(), 10);
  return Number.isFinite(v) && v >= 0 ? v : null;
}

function startNumEdit(key, label, read, write) {
  if (numEdit) endNumEdit(true);
  const start = read();

  const input = document.createElement("input");
  input.className = "keep-input";
  input.type = "text";
  input.inputMode = "numeric";
  input.value = String(start);
  label.textContent = "";
  // Свой пунктир подпись на время правки прячет: у поля есть собственная рамка,
  // и две черты под одним числом читаются как артефакт вёрстки.
  label.classList.add("editing");
  label.appendChild(input);
  numEdit = { key, label, input, write };

  // ⚠️ Клавиши игры висят на окне: пробел ставит паузу, цифры переключают
  // скорость. Без этого набор «100» уводил бы игру в ×1 — молча и в чужом
  // месте. Гасим их здесь, у самого поля; на окне стоит вторая, общая защита.
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") endNumEdit(true);
    else if (e.key === "Escape") endNumEdit(false);
  });
  input.addEventListener("blur", () => endNumEdit(true));

  input.focus();
  input.select();
}

// Закончить правку: `commit` — применить набранное, иначе бросить.
//
// Мусор и пустая строка равны отмене: «сколько-то» — это не число, а ноль игрок
// вводит явно (ноль снимает правило, §12.87).
function endNumEdit(commit) {
  if (!numEdit) return;
  const { input, label, write } = numEdit;
  const raw = input.value.trim();
  // Снимаем правку до записи: подпись вернёт ближайший снапшот, а `blur` от
  // `remove()` не должен зайти сюда второй раз.
  numEdit = null;
  input.remove();
  label.classList.remove("editing");
  label.textContent = "";
  if (!commit) return;
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value) || value < 0) return;
  write(value);
}

// Действующее правило по предмету — из снапшота (§12.88). Правило одно, и
// покупатель приезжает вместе с числом: второго источника ни того, ни другого
// в JS быть не должно.
function saleOf(item) {
  return sales.find((s) => s.item === item);
}

// Почему рецепт не заказать. «Мастерской нет» и «все станки заняты» — разные
// новости: первую чинят стройкой первой мастерской, вторую — второй, и
// молчащая кнопка не сказала бы ни того, ни другого (§4.4, §12.55).
function shopsBusyHint() {
  return shops > 0
    ? `Все мастерские заняты: заказов ${shops} из ${shops}. Постройте ещё ` +
        "или отмените заказ — кнопка в панели самого станка"
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
// Кнопка обучения: что она сейчас делает и почему не делает ничего (§12.84).
//
// Гасим классом, а не `disabled`: по выключенному элементу браузер не шлёт
// событий мыши, и подсказка с причиной не показывается **никогда** (§12.71).
// Ровно на этом кнопка «Учить» и стояла — молчала на все пять причин отказа.
//
// Причины считает ядро и везёт числами: `desk` у навыка кота (докуда доводит
// парта именно его) и `desks` по домену (сколько парт есть и сколько свободно).
// Вывести их в JS нельзя — оба про то, чего в виде нет: врождённый предел и
// занятость парты чужой задачей.
function syncTeachButtons() {
  const cat =
    selectedUnits.length === 1
      ? (lastSnap?.entities ?? []).find((e) => e.id === selectedUnits[0])
      : null;
  for (const b of teachButtons) {
    const i = +b.dataset.i;
    const name = esc(b.dataset.label);
    const skill = cat?.skills?.[i];
    const desk = desks[i] ?? { total: 0, free: 0 };
    const on = cat?.study === i;
    // Порядок причин — от «про этого кота» к «про базу»: сперва то, что
    // решается выбором другого кота, потом то, что решается стройкой.
    let why = null;
    if (!cat) why = "Выберите одного кота";
    else if (cat.away) why = `${cat.id} не на базе`;
    else if (!on && skill && skill.xp >= skill.desk)
      why = `парта уже ничему не научит: ${cat.id} на её потолке`;
    else if (!on && desk.total === 0)
      why = `парты для «${b.dataset.label}» нет`;
    else if (!on && desk.free === 0) why = "все парты этого домена заняты";

    b.dataset.on = on ? "1" : "";
    // ⚠️ Только при изменении. `syncTeachButtons` зовётся каждым снапшотом, то
    // есть ~60 раз в секунду, и безусловный `innerHTML` пересоздавал бы детей
    // кнопки каждые 16 мс. Клик человека длится сотни миллисекунд: `mousedown`
    // приходится на один `<span>`, `mouseup` — на другой, уже несуществующий, и
    // `click` браузер не выдаёт вовсе. Кнопка выглядит живой и не работает —
    // ровно та же ловушка, из-за которой панели ходят через `onPanelClick`,
    // только здесь узел кнопки жив, а мрут его дети.
    const html =
      `<span class="sw sw-study"></span>` +
      `<span>${on ? `Снять с учёбы: ${name}` : `Учить: ${name}`}</span>`;
    if (b.innerHTML !== html) b.innerHTML = html;
    b.classList.toggle("off", !!why);
    b.classList.toggle("on", !why);
    b.title =
      why ??
      (on
        ? `${cat.id} учится: после сна и еды вернётся за парту сам`
        : `${cat.id} — ${b.dataset.hint}`);
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

// Вилка состава заказа: `squad` в рулсете — либо число (минимум = предел), либо
// пара `[минимум, предел]` (§12.70). Одно место чтения на весь вид: разобрать
// эту форму в двух местах — значит однажды показать «нужно 2», когда нужно 5.
function squadBounds(def) {
  const s = def?.squad;
  if (Array.isArray(s)) return [s[0] ?? 0, s[1] ?? s[0] ?? 0];
  return [s ?? 0, s ?? 0];
}

// Раздел «Вылазки» целиком: строка на каждый узел связи (§12.66). Отряд живёт
// на клетке рации и переживает вылазку (§12.61), поэтому список отрядов — это
// список узлов.
//
// Заказов в разделе больше нет (§12.75). Отправка живёт **только** в штабе
// (§12.71): там рядом стоят состав, прогноз и написанная словом причина отказа,
// а здесь у заказа не было ничего, кроме имени и добычи, — то есть решение
// принималось вслепую, и восемь строк на отряд делали список отрядов
// нечитаемым. Раздел остался сводкой: кто есть, чем занят — и две команды по
// уже принятому решению (снять правило, отозвать отряд), которым штаб не нужен.
function renderRaidsSection() {
  if (!raidsEl || !meta) return;
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
            ? node.crew
                .map((id) =>
                  // Кто не уйдёт прямо сейчас — тусклым: цена решения обязана
                  // читаться со строки, не открывая ничего (§12.70).
                  (node.ready ?? []).includes(id)
                    ? esc(id)
                    : `<span class="dim">${esc(id)}</span>`,
                )
                .join(" · ")
            : "состав не набран — щёлкните строку отряда и отметьте котов"
        }</div>`,
      ];
      // Правило автовылазки — про весь отряд, а не про одну кнопку, поэтому
      // стоит строкой над заказами и видно его всегда: и когда отряд дома, и
      // когда он уже в поле по этому самому правилу (§12.67). Иначе снять его
      // во время вылазки было бы нечем, а следующая ушла бы «сама».
      if (node.auto >= 0) rows.push(ruleRow(node, at));
      if (raid) {
        const label = esc(missionLabel(raid.def));
        rows.push(
          `<div class="cat-sub">${
            raid.away
              ? `в поле: «${label}» · вернутся через ${raid.left}`
              : `собирается: «${label}»`
          }</div>`,
        );
        // Отозвать можно только тех, кто ещё на базе: ушедший отряд симуляции
        // уже не подчиняется — вылазка считается разом по возвращении.
        if (!raid.away) {
          rows.push(
            `<button class="tool mission-cancel" data-key="stop@${at}" data-def="${raid.def}">` +
              "<span>Отозвать</span></button>",
          );
        }
      } else {
        // Свободный отряд: куда его послать, решается в штабе — говорим это
        // словом, иначе пустая строка читается как «отправить нечем».
        rows.push(
          '<div class="cat-sub">свободен — отправка в штабе, щёлкните строку отряда</div>',
        );
      }
      return `<div class="raid-node">${rows.join("")}</div>`;
    })
    .join("");
}

// Что прячут, а не гасят. Общее правило по-прежнему обратное — закрытая вылазка
// видна, потому что лестница ответственности это то, к чему игрок идёт (§4.4), —
// но исключений из него два, и оба про строки, которые никуда не зовут.
//
// **Известность не доросла** (§12.79). Прятать по ней честно ровно потому, что
// известность **только растёт** (инвариант 15): спрятанное однажды появится и
// больше не пропадёт, а пропадающая строка читалась бы как поломка. По
// репутации не прячем никогда: она знаковая и умеет падать (§12.43), поэтому
// заказ, закрытый только доверием, остаётся в списке погашенным — доверие игрок
// возвращает сам, и заказ обязан ждать его на месте.
//
// **«За своим»** (§12.71): не ступень лестницы, а ответ на событие. Пока плена
// не случилось, звать она никуда не зовёт. Появится пленный — появится и
// строка, и это само по себе новость.
//
// Оба вопроса задаём ядру (`RaidSnap`), а не считаем здесь: это те же ворота,
// которыми `launch_at` примет или отклонит заявку (§12.24, §12.40).
function hiddenRaid(i) {
  const def = (meta.missions ?? [])[i];
  const gates = raids[i];
  if (!gates?.unlocked) return true;
  return !!def?.rescue && !(gates.possible ?? true);
}

// Открыт ли заказ сам по себе — без отряда и без узла (§12.78). Те же трое
// ворот, что стоят в `launch_at`, и берутся они из ядра (`RaidSnap`), а не
// считаются здесь: строка правила автовылазки обязана гаснуть ровно тогда,
// когда фасад перестаёт принимать заявку.
function raidOpen(i) {
  const gates = raids[i];
  return !!gates?.unlocked && !!gates?.welcome && (gates?.possible ?? true);
}

// Пойдёт ли этот отряд по этому заказу — и если нет, то почему словом. Ворота
// считает ядро (§12.24, §12.43, §12.59): те же проверки стоят в `launch`, и
// второй их экземпляр здесь однажды разойдётся с фасадом. Здесь только причина
// отказа: молчащая кнопка читается как поломка.
function raidGate(i, node) {
  const def = (meta.missions ?? [])[i] ?? {};
  const [need, most] = squadBounds(def);
  // Цена решения — до нажатия (§12.70): сколько лап уйдёт, столько и срок, и
  // доля. Оба числа считает ядро (`node.spans`, `node.dangers`) теми же
  // выражениями, какими они посчитаются на уходе, — второй экземпляр обещал бы
  // игроку не то, что он получит.
  const paws = node.ready?.length ?? 0;
  const span = node.spans?.[i];
  const danger = node.dangers?.[i] ?? def.danger ?? 0;
  const cut =
    danger !== (def.danger ?? 0)
      ? ` (было ${def.danger}${node.guide ? `, ведёт ${node.guide}` : ""})`
      : "";
  // Срок пустому отряду не обещаем и здесь (§12.71): на нуле лап работа на месте
  // не делится, и ядро честно возвращает одну дорогу — а для игрока это лучший
  // случай, который не наступит никогда.
  const hint =
    `идут ${paws} из ${node.crew.length}` +
    (span == null || paws === 0 ? "" : ` · ${span} тиков`) +
    ` · сложность ${danger}${cut}` +
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
  // Раненый и пленный вылазку больше не отменяют — они просто остаются дома
  // (§12.70), и ядро их уже вычло из `ready`. Называем их словом всё равно:
  // «идут трое из пяти» без объяснения читается как поломка.
  const home = node.crew.filter((id) => !(node.ready ?? []).includes(id));
  // Двух вылазок по одному заказу не бывает (§12.59), и это другая новость, чем
  // «этот узел занят»: там ждать своего отряда, здесь — брать другой заказ.
  const taken = running.has(i);
  const ready =
    !taken && known && welcome && !nobody && paws >= need && paws <= most;
  // Закрытая вылазка видна и объясняется словом: лестница ответственности —
  // это то, к чему игрок идёт (§4.4). Исключения перечислены у `hiddenRaid`, и
  // до карточки они не доходят вовсе — ветку «нужна известность» здесь держим
  // защитной: ворота у обеих функций одни (`RaidSnap`), и молчащая карточка при
  // расхождении хуже лишней строки.
  const title = taken
    ? "Эта вылазка уже идёт другим отрядом"
    : !known
      ? `${hint} · нужна известность ${def.requires ?? 0}`
      : distrust
        ? `${hint} · ${distrust}`
        : nobody
          ? `${hint} · все дома, спасать некого`
          : paws < need
            ? `${hint} · готовых ${paws}, нужно ${need}` +
              (home.length ? ` · дома остаются: ${home.join(", ")}` : "")
            : paws > most
              ? `${hint} · в отряде ${paws}, а больше ${most} этот заказ не уводит`
              : home.length
                ? `${hint} · дома остаются: ${home.join(", ")}`
                : hint;
  // Причина отказа отдельной строкой от подсказки: тулбар склеивает их в один
  // `title`, а штаб (§12.71) печатает причину словом под заказом — там она и
  // должна читаться, не наводя мышь.
  const reason = taken
    ? "эту вылазку уже ведёт другой отряд"
    : !known
      ? `нужна известность ${def.requires ?? 0}, у базы ${fame}`
      : distrust
        ? distrust
        : nobody
          ? "все дома — спасать некого"
          : paws < need
            ? `готовы идти ${paws}, а нужно ${need}`
            : paws > most
              ? `в отряде ${paws}, а больше ${most} этот заказ не уводит`
              : null;
  return {
    ready,
    title,
    reason,
    paws,
    need,
    most,
    span,
    danger,
    base: def.danger ?? 0,
    guide: node.guide ?? "",
    home,
    share: node.shares?.[i] ?? 0,
    failed: !!node.fails?.[i],
  };
}

// --- штаб вылазок (§12.71) -------------------------------------------------
//
// Одно окно на все вылазки: слева состав отряда, справа заказы. Раньше это были
// два разных места экрана — состав правился в панели рации, а исход показывался
// в тулбаре шириной 190px, — и связи между ними игрок не видел вовсе: добыча
// уезжала за правый край, а причина, по которой заказ закрыт, жила в нативной
// подсказке на `disabled`-кнопке, то есть не показывалась никогда (браузер не
// шлёт по ним события мыши).
//
// Прогноз здесь **весь из ядра** (`node.spans`, `dangers`, `force`, `shares`,
// `fails`): щелчок по коту уходит командой `enlist`/`dismiss`, ядро пересчитывает
// узел, и следующий же снапшот приносит новые числа. Второго экземпляра формулы
// в JS нет и быть не может (инвариант 14) — именно поэтому «добавил кота, и вот
// что изменилось» получается само.
// Срок словом. Тик остаётся единственными часами мира (§12.28, §12.46), но
// «60 тиков» игрок ни с чем не соотносит: рядом переводим в те же сутки, что
// показывают часы в шапке. Календарь — подача, а не механика, и живёт он здесь,
// на стороне вида.
// «2 кота» против «5 котов»: делитель в формуле срока стоит рядом с числом, и
// без склонения запись читается как машинный вывод (§12.71).
function pawsWord(n) {
  const ten = n % 10;
  const hundred = n % 100;
  const word =
    ten === 1 && hundred !== 11
      ? "кот"
      : ten >= 2 && ten <= 4 && (hundred < 12 || hundred > 14)
        ? "кота"
        : "котов";
  return `${n} ${word}`;
}

function spanText(span) {
  const len = meta?.day ?? 0;
  if (len <= 0) return `${span} тиков`;
  const days = span / len;
  const when =
    days >= 1
      ? `${days.toFixed(1)} дн`
      : `${Math.max(1, Math.round(days * 24))} ч`;
  return `${span} тиков (≈${when})`;
}

// Доля от полной шкалы кота. Максимум спрашиваем у самого кота, а не пишем
// числом рядом с рулсетом: второй экземпляр баланса в JS однажды разойдётся с
// YAML, и игрок увидит «−40 %» там, где отнимут половину.
function scaleText(value, field) {
  const max = Math.max(
    0,
    ...(lastSnap?.entities ?? []).map((e) => e[field] ?? 0),
  );
  return max > 0 ? `${Math.round((value / max) * 100)} %` : `${value}`;
}

// --- окно «Склад» (§12.100) -------------------------------------------------
//
// Всё имущество базы, курсы всех сторон и оба порога — одной таблицей: строка
// на предмет. До §12.100 это жило в пяти местах (фишка в шапке и её подсказка,
// «Рынок: <фракция>» по разделу на каждую, «Сбыт», «держать N» в
// «Производстве»), и ни одно не отвечало на вопрос целиком.
//
// ⚠️ **Строится один раз при открытии и дальше синхронизируется на месте.**
// Идиома тулбара, а не панели, и с соседним модалом (`#raidwin`, §12.71) она
// **противоположная**: тот перерисовывается целиком каждым снапшотом. Здесь так
// нельзя — в окне живут поля порогов, а их убивает ровно перерисовка: §12.84
// (пересозданные дети кнопки съедают клик), §12.87 (удержание считает от числа,
// снятого на нажатии), §12.89 (набор досылается, когда устоялся), §12.92
// (подпись на время правки становится `<input>`). Числа здесь меняются каждым
// кадром, так что спасительное сравнение HTML из штаба не сработало бы ни разу.
let stockWinOpen = false;
// Строка красных предупреждений в шапке окна: чего базе не хватает, чтобы
// показанное вообще работало (§12.100).
let warnEl = null;
let busyEl = null;
// Строки предметов, пока окно открыто. Массив живёт от открытия до закрытия:
// узлы в нём переиспользуются, и на этом держится вся работа порогов.
const wareRows = [];

function openStockWindow() {
  if (stockWinOpen) return;
  stockWinOpen = true;
  buildStockWindow();
  syncStockWindow();
  stockWinEl.hidden = false;
}

function closeStockWindow() {
  if (!stockWinOpen) return;
  // Уход из окна — конец набора (§12.89): открытую правку досылаем, иначе
  // число не доедет до ядра и не попадёт в автосейв.
  endNumEdit(true);
  stockWinOpen = false;
  wareRows.length = 0;
  tradeButtons.length = 0;
  stockWinEl.hidden = true;
  stockWinEl.innerHTML = "";
  // Узел, над которым висела подсказка, сейчас исчезнет: `mouseleave` по
  // удалённому элементу браузер не шлёт, и подсказка осталась бы висеть поверх
  // карты навсегда.
  hideLiveTip();
}

// Кто сторона этой строки: **одна на всю строку**, и ей адресованы все её
// решения — и разовая сделка, и порог автопродажи, и тикер (§12.100).
//
// Порядок источников не случаен: сперва то, что игрок уже решил и что хранит
// ядро, потом заготовка выбора, потом первый торгующий. «Продавать тому, кто в
// этот тик даёт больше» ядру посчитать нетрудно, и ровно поэтому нельзя
// (§12.87): правило обыгрывало бы игрока на его же расписании.
function sideOf(item, sides) {
  return (
    saleOf(item)?.faction ??
    tickers.find((t) => t.item === item)?.faction ??
    picked.get(item) ??
    sides[0]
  );
}

// Стороны, которые вообще торгуют этим предметом. Из палитры, а не из снапшота:
// окно может открыться раньше первого курса, а прайс — контент (см. `costChips`,
// `prices` приезжает `Map`).
function sidesOf(item) {
  const id = (meta.items ?? [])[item]?.id;
  return (meta.factions ?? [])
    .map((fac, fi) => ({ fac, fi }))
    .filter(({ fac }) => {
      const list =
        fac.prices instanceof Map
          ? [...fac.prices.keys()]
          : Object.keys(fac.prices ?? {});
      return list.includes(id);
    })
    .map(({ fi }) => fi);
}

// Рецепты, которые дают этот предмет. Обычно один; двух хватает, чтобы порог
// нельзя было повесить на предмет, — на этом и стоит §12.65.
function recipesGiving(item) {
  const id = (meta.items ?? [])[item]?.id;
  return (meta.recipes ?? [])
    .map((r, i) => ({ r, i }))
    .filter(({ r }) => {
      const gives =
        r.gives instanceof Map
          ? [...r.gives.keys()]
          : Object.keys(r.gives ?? {});
      return gives.includes(id);
    });
}

function buildStockWindow() {
  wareRows.length = 0;
  tradeButtons.length = 0;
  stockWinEl.innerHTML = "";

  const box = document.createElement("div");
  box.className = "warewin-box";
  const top = document.createElement("div");
  top.className = "warewin-top";
  top.innerHTML = '<div class="warewin-title">Склад</div>';

  // Что делается прямо сейчас (§12.107) — **в одну линию с заголовком**: оранжевым
  // называется само окно, а серым рядом идёт то, что в нём происходит. Своей
  // строкой сводка занимала место, отвечая на вопрос, который чаще всего звучит
  // «ничего» (заказов нет — строки нет вовсе).
  //
  // Живёт она в окне, а не в панели «Заказы», потому что панель накрыта самим
  // модалом — то же столкновение, из-за которого §12.101 запретил открывать
  // окна кликом по карте. Без неё клик по «Произвести» не отвечает ничем:
  // запас в строке вырастет через сотни тиков.
  busyEl = document.createElement("div");
  busyEl.className = "warewin-busy";
  busyEl.hidden = true;
  // Подсказка у неё **живая** (`liveTitle`, а не `title`): числа в ней тикают
  // каждым кадром, а нативную подсказку браузер рисует один раз при показе.
  bindLiveTip(busyEl);
  top.appendChild(busyEl);

  const close = mkTool("Закрыть", () => closeStockWindow());
  close.className = "tool warewin-close";
  top.appendChild(close);
  box.appendChild(top);

  // Предупреждения — одной строкой на всё окно, а не по строке на предмет:
  // «нет торгового поста» шесть раз подряд это не шесть новостей (§12.80).
  warnEl = document.createElement("div");
  warnEl.className = "warewin-warn";
  box.appendChild(warnEl);

  const list = document.createElement("div");
  list.className = "warewin-list";
  box.appendChild(list);
  stockWinEl.appendChild(box);

  (meta.items ?? []).forEach((it, item) => {
    const sides = sidesOf(item);
    const row = document.createElement("div");
    row.className = "ware-row";

    // Избранное — первым в строке: это про саму строку и её место в списке, и
    // глаз ищет его в одном столбце, а не в конце строк разной длины. Ворот у
    // него нет вовсе — закрепить можно и то, чем не торгует никто (§12.100).
    const fav = mkTool("★", () =>
      sendAction({ type: "setFavorite", item, on: !favorites.includes(item) }),
    );
    fav.classList.add("toggle", "ware-fav");
    row.appendChild(fav);

    const name = document.createElement("div");
    name.className = "ware-name";
    name.innerHTML =
      `<span class="sw" style="background:${it.color}"></span>` +
      `<span>${esc(it.label || it.id)}</span>`;
    row.appendChild(name);

    // Числа — идиома шапки: главное и `+22` серым, без подписей. Расклад на три
    // числа §12.53 живёт в подсказке, где он и стоит сегодня.
    const num = document.createElement("div");
    num.className = "ware-num";
    row.appendChild(num);

    // Правила строки: сперва «делать до» (по рецепту, §12.65), потом «сбывать
    // сверх» (по предмету, §12.88).
    const rules = document.createElement("div");
    rules.className = "ware-rules";
    row.appendChild(rules);

    const keeps = [];
    const many = recipesGiving(item).length > 1;
    for (const { r, i: def } of recipesGiving(item)) {
      const line = document.createElement("div");
      line.className = "keep";
      const key = `stock:${def}`;
      const read = () => stocking[def] ?? 0;
      const write = (min) => sendAction({ type: "setStock", recipe: def, min });
      const label = mkKeepLabel(key, read, write);
      // Кнопка заказа — в конце строки своего порога (§12.105): «делать до N» и
      // разовый заказ это одно решение с двух сторон, и цена у них общая. Она же
      // и держит цену фишками — теми же, что у тайлов: полоска порога умеет
      // пропасть (нет технологии автоматики, нет мастерской), а кнопка остаётся,
      // и уехавшая с полоской цена исчезала бы вместе с правилом.
      //
      // Что рецепт **даёт**, здесь не пишем: строка и есть этот предмет, а
      // повторять то, подо чем стоишь, нельзя (§12.80). В тулбаре кнопка была
      // сама по себе, и «что выходит» ей приходилось называть.
      const make = mkTool(
        `<span>Произвести</span><b class="qty">×${craftSize(false)}</b>`,
        (e) =>
          sendAction({
            type: "craft",
            recipe: def,
            count: craftSize(e.shiftKey),
          }),
      );
      make.classList.add("toggle", "ware-make");
      // Цена — **под** кнопкой, а не внутри неё: фишек у рецепта бывает одна, а
      // бывает три, и вшитая в подпись цена делала кнопки разной ширины —
      // столбец переставал читаться столбцом (тот же довод, по которому
      // `.ware-rules .keep` вообще сетка).
      //
      // Стоит она **в пустой клетке следующей строки** — той, что напротив
      // «сбывать сверх»: своей строкой цена растила бы каждую строку рецепта на
      // свою высоту, а колонка заказа у порога сбыта пуста всегда. Держит это
      // не сетка (строки порога — отдельные гриды, общих колонок у них нет), а
      // выкладка поверх (`.ware-craft.under`), и включает её `syncStockWindow`,
      // только если следующая строка видима: у последней строки предмета этого
      // места нет, и цена там встаёт обычным потоком.
      const craft = document.createElement("div");
      craft.className = "ware-craft";
      const cost = document.createElement("div");
      cost.className = "ware-cost";
      cost.innerHTML = costChips(r.cost);
      craft.append(make, cost);
      line.append(label, craft);
      // **Имя рецепта — только когда рецептов больше одного** (§12.100): при
      // единственном оно повторяет то, подо чем стоит, а повторяющая соседа
      // строка лишняя (§12.80).
      if (many) {
        const who = document.createElement("span");
        who.className = "ware-recipe";
        who.textContent = r.label || r.id;
        line.appendChild(who);
      }
      rules.appendChild(line);
      keeps.push({ line, label, make, craft, key, def });
    }

    let sale = null;
    if (sides.length) {
      const line = document.createElement("div");
      line.className = "keep";
      const key = `sale:${item}`;
      const read = () => saleOf(item)?.keep ?? 0;
      const write = (keep) =>
        sendAction({
          type: "setSale",
          faction: sideOf(item, sides),
          item,
          keep,
        });
      const label = mkKeepLabel(key, read, write);
      line.append(label);
      rules.appendChild(line);
      sale = { line, label, key };
    }

    // Торговый блок. Тикер стоит здесь, а не рядом с избранным: у него в данных
    // **сторона**, и переключатель обязан стоять при своём поле — два чекбокса
    // подряд различались бы только глифом (§12.100).
    const trade = document.createElement("div");
    trade.className = "ware-trade";
    row.appendChild(trade);

    const tick = mkTool("◉", () => {
      if (tick.classList.contains("off")) return; // причина уже в подсказке
      sendAction({
        type: "setTicker",
        item,
        faction: sideOf(item, sides),
        on: !tickers.some((t) => t.item === item),
      });
    });
    tick.classList.add("toggle", "ware-tick");
    trade.appendChild(tick);

    let side = null;
    let rate = null;
    let buy = null;
    let sell = null;
    let none = null;
    if (sides.length) {
      // Сторона перебирается кликом по кругу — идиома «Сбыта» (§12.88). Колонка
      // на фракцию растёт вбок и умирает на четвёртой; список сторон не растёт.
      side = mkTool("", () => {
        if (sides.length < 2) return;
        const now = sideOf(item, sides);
        const next = sides[(sides.indexOf(now) + 1) % sides.length];
        picked.set(item, next);
        // Сторона у строки одна, и ей адресованы все её решения: смена стороны
        // переписывает и правило автопродажи, и тикер — сразу, а не «когда-
        // нибудь». Иначе игрок видит одну сторону, а торгуют с другой (§12.88).
        // Набранное, но ещё не отданное ядру, едет к новому покупателю вместе
        // с правилом — и **не** уезжает к старому: `endNumEdit(false)` гасит
        // правку, не записывая её.
        const typed = numPending(`sale:${item}`);
        endNumEdit(false);
        const keep = typed ?? saleOf(item)?.keep ?? 0;
        if (keep > 0)
          sendAction({ type: "setSale", faction: next, item, keep });
        if (tickers.some((t) => t.item === item))
          sendAction({ type: "setTicker", item, faction: next, on: true });
        syncStockWindow();
      });
      side.classList.add("toggle", "ware-side");
      // Клик по стороне не гасит открытое поле фокусом: `blur` пришёл бы раньше
      // `click` и записал набранное **прежнему** покупателю, а строка тут же
      // прыгнула бы обратно на него (сторона берётся из действующего правила).
      side.addEventListener("mousedown", (e) => e.preventDefault());
      trade.appendChild(side);

      rate = document.createElement("span");
      rate.className = "ware-rate";
      trade.appendChild(rate);

      for (const buying of [true, false]) {
        const b = mkTool(
          `<span>${buying ? "Купить" : "Продать"}</span><b class="qty">×5</b>`,
          (ev) => {
            // Клик — пять штук, Shift — полный контейнер (§12.90).
            const count = dealSize(ev.shiftKey);
            sendAction({
              type: "trade",
              faction: sideOf(item, sides),
              item,
              count,
              buying,
            });
          },
        );
        b.classList.add("toggle");
        b.dataset.item = item;
        b.dataset.buying = buying ? "1" : "";
        trade.appendChild(b);
        tradeButtons.push(b);
        if (buying) buy = b;
        else sell = b;
      }
    } else {
      none = document.createElement("span");
      none.className = "ware-none";
      none.textContent = "никто не берёт";
      trade.appendChild(none);
    }

    list.appendChild(row);
    wareRows.push({
      item,
      row,
      fav,
      num,
      keeps,
      sale,
      tick,
      side,
      rate,
      buy,
      sell,
      none,
      sides,
      it,
    });
  });
}

// Сводка «что сейчас делается» в шапке окна (§12.107). Заказы едут в снимке
// целиком (`crafting`), здесь их только складывают по рецепту — та же
// арифметика по снимку, что `dealGroups` делает по сделкам, а не второй
// экземпляр правила.
//
// Отвечает она и на «сколько добавил мой клик»: снимок уходит в главный поток
// каждым кадром **независимо от паузы**, а заказ заводит фасад в момент
// команды, — число прыгает сразу, даже на ⏸.
function syncStockBusy() {
  const orders = lastSnap?.crafting ?? [];
  // По индексу рецепта в палитре, а не по порядку в снимке: тот отсортирован
  // по клетке (§12.96), и закрывшийся заказ переставлял бы соседей под
  // курсором — тот же довод, по которому ядро сортирует `deals`.
  const byDef = new Map();
  for (const c of orders) {
    const g = byDef.get(c.def) ?? { left: 0, list: [] };
    g.left += c.left;
    g.list.push(c);
    byDef.set(c.def, g);
  }
  const defs = [...byDef.keys()].sort((a, b) => a - b);

  const parts = [];
  const tips = [];
  for (const def of defs) {
    const g = byDef.get(def);
    if (g.left <= 0) continue;
    const name = craftLabel(def);
    parts.push(`${name} ×${g.left}`);
    for (const c of g.list) {
      const pct = c.total > 0 ? Math.round((c.progress / c.total) * 100) : 0;
      tips.push(
        `${name}: осталось ${c.left} шт · ${craftStateText(c)} · ${pct} %` +
          (c.auto ? " · по порогу" : ""),
      );
    }
  }

  const text = parts.length ? `В работе: ${parts.join(" · ")}` : "";
  if (busyEl.textContent !== text) busyEl.textContent = text;
  liveTitle(busyEl, tips.join("\n"));
  busyEl.hidden = !parts.length;
}

// Как назвать заказ в сводке — **тем же правилом, что и строка предмета**
// (§12.100): имя рецепта пишется, только когда рецептов на предмет больше
// одного, иначе оно повторяет то, подо чем стоит. Предмет у рецепта берём
// первый по палитре: строка окна — это предмет, и сводка обязана указывать на
// ту же строку.
function craftLabel(def) {
  const r = (meta.recipes ?? [])[def];
  const gives =
    r?.gives instanceof Map
      ? [...r.gives.keys()]
      : Object.keys(r?.gives ?? {});
  const item = (meta.items ?? []).findIndex((it) => gives.includes(it.id));
  if (item < 0) return r?.label || r?.id || "Заказ";
  return recipesGiving(item).length > 1
    ? r?.label || r?.id || "Заказ"
    : (meta.items ?? [])[item].label || (meta.items ?? [])[item].id;
}

function syncStockWindow() {
  if (!stockWinOpen || !meta) return;
  const recipeSnaps = lastSnap?.recipes ?? [];

  // **Три состояния, а не два** (§12.100). Технологии нет — не показываем
  // ничего: правило ещё не механика игры, и полоска «делать до —» обещала бы
  // то, чего в мире нет (§12.94). Технология есть, а ячейки нет — показываем
  // красным, **почему**: это чинится стройкой, то есть решением, которое игрок
  // может принять прямо сейчас. И только когда есть оба — обычная строка.
  //
  // Порядок важен: сперва спрашиваем технологию, потом ячейку. Наоборот —
  // и база без мастерской звала бы строить её, ещё не зная рецептов.
  const canTrade = posts > 0;
  const canCraft = shops > 0;
  const warns = [];
  if (!canTrade) warns.push("Торговля недоступна: нет «Торгового поста»");
  // Про мастерскую говорим, только если рецепты уже открыты: иначе это
  // предупреждение про механику, которой у игрока ещё нет.
  const anyRecipe = recipeSnaps.some((r) => r?.unlocked);
  if (!canCraft && anyRecipe) warns.push("Производство недоступно: нет «Мастерской»");
  const warnHtml = warns.map((w) => `<div>${w}</div>`).join("");
  if (warnEl.innerHTML !== warnHtml) warnEl.innerHTML = warnHtml;
  warnEl.hidden = !warns.length;

  syncStockBusy();

  for (const r of wareRows) {
    const st = stock[r.item] ?? {
      stored: 0,
      loose: 0,
      booked: 0,
      on_base: 0,
    };
    const free = Math.max(0, st.stored - st.booked);
    const name = esc(r.it.label || r.it.id);

    // Числа и их расклад — ровно то же, что в шапке (§12.53): второй формы
    // записи тех же трёх чисел в игре быть не должно.
    const nums =
      `<b>${free}</b>` + (st.loose ? `<u>+${st.loose}</u>` : "");
    if (r.num.innerHTML !== nums) r.num.innerHTML = nums;
    r.num.title = [
      `${name}: на складе ${st.stored}`,
      st.booked ? `забронировано ${st.booked} под сделку` : "",
      st.loose
        ? `валяется ${st.loose} — годится на стройку, но платить и ` +
          `продавать этим нельзя, пока не убрано`
        : "",
    ]
      .filter(Boolean)
      .join(" · ");

    const isFav = favorites.includes(r.item);
    r.fav.classList.toggle("on", isFav);
    r.fav.title = isFav
      ? "Убрать из избранного"
      : "В избранное: строка встанет наверх списка";

    // Производство (§12.65, §12.105). Число, которое меряет порог, — **в
    // подсказке**, а не в строке: две почти одинаковые цифры рядом читаются как
    // ошибка вёрстки чаще, чем как разница запасов (§12.100).
    //
    // ⚠️ **Видимость кнопки и видимость порога расходятся, и это главное здесь.**
    // Строку целиком прячет только отсутствие технологии рецепта: механики ещё
    // нет, и обещать её нечем (§12.94). Два других условия — про правило и
    // только про него: без технологии автоматики (§12.93) правила нет, а руками
    // заказать можно всегда; без мастерской правило не сработает ни разу, но
    // «нет мастерской» — это ровно то решение, к которому игрок целится, и оно
    // остаётся на месте с причиной словом (§12.94). Поэтому `hidden` висит на
    // трёх элементах порога, а не на строке: свяжи их снова, и кнопка пропадала
    // бы у базы, которой всего-то и нужно построить станок.
    const craftGate = autoGateHint("crafting");
    for (const k of r.keeps) {
      stocking[k.def] = (lastSnap?.stocking ?? [])[k.def] ?? 0;
      const min = stocking[k.def];
      const rs = recipeSnaps[k.def] ?? {};
      const open = rs.unlocked ?? false;
      if (!numEditing(k.key)) {
        k.label.textContent = min > 0 ? `делать до ${min}` : "делать до —";
      }
      k.line.classList.toggle("on", min > 0 && !craftGate);
      k.line.hidden = !open;
      // Нет станка — полоски порога нет, а причина написана красным в шапке
      // окна: порог без мастерской не сработает ни разу (§12.100).
      k.label.hidden = !!craftGate || !canCraft;
      // Есть ли под строкой рецепта место для цены: клетка колонки заказа у
      // следующей видимой строки (обычно «сбывать сверх») пуста. Нет её —
      // цена встаёт обычным потоком и строку растит.
      // Занятой она бывает у второго рецепта того же предмета (§12.100): там в
      // колонке заказа стоит его кнопка, и цена легла бы поверх неё.
      let next = k.line.nextElementSibling;
      while (next && next.hidden) next = next.nextElementSibling;
      k.craft.classList.toggle(
        "under",
        !!next && !next.querySelector(".ware-craft"),
      );
      k.line.title =
        craftGate ??
        (min > 0
          ? `Коты сами делают, пока на базе меньше ${min} шт. Сейчас на базе ` +
            `${st.on_base} — это склад вместе с полом и лапами, за вычетом ` +
            `обещанного покупателю. Клик по числу — ввести другое`
          : `Держать запас: коты будут делать сами, когда просядет. Считается ` +
            `не склад, а вся база — сейчас на ней ${st.on_base}`);

      // Доступность рецепта считает ядро (технологии, мастерская, склад), здесь
      // только показываем. `shop` значит «есть куда поставить»: ячейка станка —
      // свободная или отбираемая у правила-порога (§12.97), — **или** уже
      // размеченный заказ на этот рецепт, тогда клик добавит штук (§12.96). Своя
      // работа не запирает себя же: все станки заняты деталями — деталей закажут
      // ещё. Считает всё это ядро (`spare_shop_cell`), второго экземпляра здесь
      // быть не должно.
      //
      // Пустой склад кнопку **не гасит**: заказ без материала ядро примет, он
      // будет ждать — как чертёж без лома (§12.30). Поэтому «нечем платить»
      // живёт в подсказке, а не в `disabled`.
      //
      // ⚠️ Гасим **классом**, а не `disabled` (§12.53, §12.71): по выключенному
      // элементу браузер не шлёт событий мыши, и подсказка не показывалась бы
      // **никогда** — а с §12.96 «все станки заняты» стало частым ответом,
      // потому что один рецепт умеет занять их все. Ровно на этих граблях уже
      // молчала кнопка «Учить» (§12.84).
      const ready = open && rs.shop;
      k.make.classList.toggle("off", !ready);
      k.make.classList.toggle("on", ready && rs.affordable);
      k.make.title = !open
        ? "Нужна технология"
        : !rs.shop
          ? shopsBusyHint()
          : rs.affordable
            ? "Произвести: клик — штука, Shift — пять"
            : `На складе нет материала, заказ будет ждать: ${payHint(
                (meta.recipes ?? [])[k.def]?.cost,
              )}`;
    }

    const who = r.sides.length ? sideOf(r.item, r.sides) : null;
    const fac = who === null ? null : (meta.factions ?? [])[who];

    // Порог автопродажи (§12.87, §12.88).
    if (r.sale) {
      const saleGate = autoGateHint("sales");
      const keep = saleOf(r.item)?.keep ?? 0;
      if (!numEditing(r.sale.key)) {
        r.sale.label.textContent =
          keep > 0 ? `сбывать сверх ${keep}` : "сбывать сверх —";
      }
      r.sale.line.classList.toggle("on", keep > 0 && !saleGate);
      r.sale.line.classList.toggle("off", !!saleGate);
      // Без поста прятать так же, как порог производства без станка: правило
      // стоит, но не сработает, а причина — в шапке окна.
      r.sale.line.hidden = !!saleGate || !canTrade;
      r.sale.line.title =
        saleGate ??
        (keep > 0
          ? `Коты продают «${fac?.label ?? "?"}» всё, что на складе сверх ` +
            `${keep} шт. (лежащее на полу не в счёт — сперва уберут).${
              posts ? "" : " Но торгового поста ещё нет."
            } Клик по числу — ввести другое`
          : "Продавать излишек: всё, что на складе сверх порога, коты отнесут " +
            "на пост сами");
    }

    // Тикер (§12.100). Сторона обязательна — без неё торговать нечем, — поэтому
    // у неторгуемого предмета кнопка гаснет классом, а не пропадает: дырка в
    // столбце читается как поломка (§12.53).
    // Весь торговый блок — тикер, сторона, курс и кнопки — прячется вместе с
    // постом: торговать без него нельзя ничем, и шесть строк мёртвых кнопок
    // говорят меньше, чем одна красная строка наверху.
    for (const node of [r.tick, r.side, r.rate, r.buy, r.sell, r.none])
      if (node) node.hidden = !canTrade;

    const onTicker = tickers.some((t) => t.item === r.item);
    r.tick.classList.toggle("on", onTicker);
    r.tick.classList.toggle("off", !r.sides.length);
    r.tick.title = !r.sides.length
      ? `«${name}» не берёт никто — торговать по нему нечем`
      : onTicker
        ? "Убрать из ленты на главном экране"
        : `В ленту: строка встанет на главный экран, и по ней можно будет ` +
          `торговать с «${fac?.label ?? "?"}» в один клик`;

    if (r.side) {
      const one = r.sides.length < 2;
      const label = esc(fac?.label || fac?.id || "—");
      if (r.side.textContent !== label) r.side.textContent = label;
      r.side.classList.toggle("off", one);
      r.side.title = one
        ? `Этот товар берёт только «${fac?.label ?? "?"}» — выбирать не из кого`
        : `Торгуем с «${fac?.label ?? "?"}». Клик — другая сторона; порог и ` +
          `тикер переезжают вместе с ней`;
    }

    // Сторона у кнопок сделки — **не своя, а строкина**: её выбирают кликом, и
    // приколоченная в `dataset` при сборке она осталась бы от первой стороны.
    // `syncTradeButtons` читает именно `dataset`, поэтому обновляем перед ним.
    if (who !== null) {
      for (const b of [r.buy, r.sell]) if (b) b.dataset.faction = who;
    }

    if (r.rate) {
      const q = quoteOf(who, r.item);
      const html = q
        ? `${rateText(q, true)}<span class="ware-sep">·</span>${rateText(q, false)}`
        : "—";
      if (r.rate.innerHTML !== html) r.rate.innerHTML = html;
      r.rate.title = q
        ? `Покупка ${q.buy}¤ · продажа ${q.sell}¤ за штуку` +
          (q.next_in > 0
            ? ` · через ${q.next_in} станет ${q.next_buy}¤ / ${q.next_sell}¤`
            : "")
        : "";
    }
  }

  // Подпись «×N» у кнопок заказа: она зависит только от зажатого Shift, но
  // кадром её тоже надо освежить — окно могли открыть с уже зажатой клавишей.
  syncCraftSize();

  // Состояние кнопок сделки — общим `syncTradeButtons`: считает его `tradeState`,
  // одно место на окно и на ленту (§12.100).
  syncTradeButtons();

  orderWareRows();
}

// Избранные — наверх, остальные по палитре. Переставляем **узлы**, а не
// разметку: строка переживает переезд вместе со слушателями, набором в поле и
// удержанием кнопки, а `innerHTML` убил бы всё это (§12.84).
function orderWareRows() {
  const list = stockWinEl.querySelector(".warewin-list");
  if (!list) return;
  const want = [...wareRows].sort((a, b) => {
    const fa = favorites.includes(a.item) ? 0 : 1;
    const fb = favorites.includes(b.item) ? 0 : 1;
    return fa - fb || a.item - b.item;
  });
  const now = [...list.children];
  if (want.every((r, i) => now[i] === r.row)) return;
  for (const r of want) list.appendChild(r.row);
}

function openRaidWindow(x, y) {
  raidWinAt = { x, y };
  renderRaidWindow();
}

function closeRaidWindow() {
  raidWinAt = null;
  raidWinHtml = "";
  raidWinEl.hidden = true;
  raidWinEl.innerHTML = "";
}

function renderRaidWindow() {
  if (!raidWinAt || !meta) return;
  // Рацию могли снести, пока окно открыто: узел пропал — закрываемся, а не
  // показываем отряд, которого больше нет.
  const node = nodeAt(raidWinAt.x, raidWinAt.y);
  if (!node || !lastSnap) {
    closeRaidWindow();
    return;
  }
  const n = nodes.indexOf(node);
  const defs = meta.missions ?? [];
  const raid = missionsOut.find(
    (m) => m.node_x === node.x && m.node_y === node.y,
  );

  // Вкладки узлов: перекинуть кота с отряда на отряд — это одно решение, и
  // закрывать ради него окно незачем. При единственном узле вкладок нет.
  const tabs =
    nodes.length > 1
      ? '<div class="raidwin-tabs">' +
        nodes
          .map(
            (o, i) =>
              `<button class="tool raidwin-tab${o === node ? " on" : ""}"` +
              ` data-key="tab@${o.x},${o.y}" data-x="${o.x}" data-y="${o.y}">` +
              `Отряд ${i + 1}${o.busy ? " · в поле" : ""}</button>`,
          )
          .join("") +
        "</div>"
      : "";

  // Шапка отряда: сила, состав и проводник — те самые числа, которые крутит
  // список слева. Сила без заказа ничего не значит, поэтому рядом с ней сразу
  // стоит, из чего она сложилась.
  const paws = node.ready?.length ?? 0;
  const parts = node.ready.map(
    (id, i) => `${esc(id)}&nbsp;+${node.forces?.[i] ?? 0}`,
  );
  // Пока отряд в поле, узел прогноза не считает: `ready` — это «кто готов идти
  // **сейчас**», а ушедшие все `away`, и шапка честно показала бы «сила 0, идут
  // 0 из 3». Честно — и бесполезно: игрок смотрит сюда, чтобы узнать, чем
  // кончится идущая вылазка, а не сколько лап осталось дома. Поэтому у ушедшего
  // отряда шапку заполняет сама вылазка (`MissionSnap`) — теми же числами,
  // какими её считает панель справа, из того же `outcome` (инвариант 14).
  const summary = raid?.away
    ? '<div class="raidwin-sum">' +
      `<div class="raidwin-force"><b>${raid.strength}</b><span>сила отряда</span></div>` +
      '<div class="raidwin-sumtext">' +
      `<div>в поле · вернутся через ${raid.left}${
        raid.guide ? ` · ведёт ${esc(raid.guide)}` : " · проводника нет"
      }</div>` +
      `<div class="cat-sub">${
        [...raid.squad.map(esc), ...commsPart(raid, node)].join(" · ") ||
        "в отряде никого"
      }</div>` +
      "</div></div>"
    : '<div class="raidwin-sum">' +
      `<div class="raidwin-force"><b>${node.force ?? 0}</b><span>сила отряда</span></div>` +
      '<div class="raidwin-sumtext">' +
      `<div>идут ${paws} из ${node.crew.length}${
        node.guide ? ` · ведёт ${esc(node.guide)}` : " · проводника нет"
      }</div>` +
      `<div class="cat-sub">${parts.join(" · ") || "в отряде никого"}</div>` +
      "</div></div>";

  const left =
    '<div class="raidwin-col raidwin-crew">' +
    '<div class="raidwin-h">Кто идёт</div>' +
    summary +
    crewList(lastSnap, node.x, node.y).join("") +
    "</div>";

  // Правило узла — строкой **над** списком, а не первой карточкой в нём.
  // Порядок заказов это лестница сложности (индекс в палитре = ступень), и
  // вытащенная наверх карточка ломает и её чтение, и позиционную память —
  // причём ровно та карточка, про которую уже всё решено. Со строкой видно то
  // же самое, а список цел. У занятого узла её не дублируем: там правило стоит
  // кнопкой в самой карточке (`busyCard`).
  const rule =
    !raid && node.auto >= 0 ? ruleRow(node, `${node.x},${node.y}`) : "";

  const cards = raid
    ? busyCard(raid, node)
    : defs.map((_, i) => (hiddenRaid(i) ? "" : raidCard(i, node))).join("");
  // Пустая колонка читается как поломка окна, а не как «пока нечего» (§12.79):
  // с сокрытием по известности список впервые в принципе умеет опустеть. В
  // боевом рулсете этого не бывает — первая ступень открыта всем (`requires: 0`),
  // и это стережёт `the_shipped_ruleset_*`, — но синтетический контент такое
  // допускает, и молчать здесь нельзя.
  const nothing =
    '<div class="cat-sub">заказов пока нет — первый откроет известность</div>';

  const right =
    '<div class="raidwin-col raidwin-jobs">' +
    '<div class="raidwin-h">Заказы</div>' +
    rule +
    (cards || nothing) +
    "</div>";

  const html =
    '<div class="raidwin-box">' +
    '<div class="raidwin-top">' +
    `<div class="raidwin-title">Отряд ${n + 1}<span class="cell-at">рация ${node.x}, ${node.y}</span></div>` +
    '<button class="tool raidwin-close" data-key="close">Закрыть</button>' +
    "</div>" +
    tabs +
    `<div class="raidwin-body">${left}${right}</div>` +
    "</div>";

  // Окно перерисовывается каждым снапшотом, а `innerHTML` заменяет узлы вместе
  // с их `scrollTop` — то есть шестьдесят раз в секунду отматывает прокрутку в
  // ноль. Полоса при этом видна и таскается мышью, а колесо «не работает»:
  // прокрутка происходит и тут же откатывается. Ровно этим списки в панелях
  // отличаются от списка в окне — те короткие и не прокручиваются вовсе.
  //
  // Лечим двумя мерами, и нужны обе. Первая: не трогаем разметку, пока она не
  // изменилась, — большую часть кадров она совпадает дословно. Вторая: когда
  // изменилась (у идущей вылазки тикает таймер), возвращаем прокрутку по месту.
  if (html === raidWinHtml) return;
  const tops = [...raidWinEl.querySelectorAll(".raidwin-col")].map(
    (c) => c.scrollTop,
  );
  raidWinHtml = html;
  raidWinEl.innerHTML = html;
  raidWinEl.hidden = false;
  raidWinEl
    .querySelectorAll(".raidwin-col")
    .forEach((c, i) => (c.scrollTop = tops[i] ?? 0));
}

// Одна карточка заказа: что получим, чем рискуем и почему нельзя. Всё, что
// зависит от состава, идёт из `raidGate`, то есть из ядра, — а всё, что зависит
// только от заказа (добыча, раны, репутация), из палитры.
function raidCard(i, node) {
  const def = (meta.missions ?? [])[i] ?? {};
  const g = raidGate(i, node);
  const at = `${node.x}, ${node.y}`;
  const on = node.auto === i && node.auto_on;
  // Усыплённое правило на этом же заказе — не «нет правила»: заказ узел помнит,
  // и тумблер обязан будить его, а не заводить заново (§12.77).
  const paused = node.auto === i && !node.auto_on;
  const rows = [];

  // Прогноз — первым и крупно: это ответ на «стоит ли». Провал называем словом,
  // а не нулём: «добыча 0 %» и «отряд не вернётся с добычей, да ещё и ранят» —
  // разные новости (§12.37).
  //
  // Но у пустого отряда прогноза нет вовсе, и «провал» на всех заказах разом —
  // это не предсказание, а арифметика нуля: игрок ещё никого не выбрал, а игра
  // уже сообщает ему исход (§12.71). Красным по всему списку это к тому же
  // читается как поломка, а не как «начните отсюда». Говорим, чего не хватает,
  // — ровно как о причине отказа под кнопкой.
  const empty = g.paws === 0;
  const verdict = empty
    ? '<span class="cat-sub">отряд не набран</span>'
    : g.failed
      ? '<span class="bad">провал</span>'
      : g.share >= 100
        ? '<span class="good">вся добыча</span>'
        : `<span class="warn">добыча ${g.share} %</span>`;
  // Проводник режет сложность делением (§12.70), и без «было» урезанное число
  // выглядит взятым с потолка. Но стоять это обязано **в той же строке**, что и
  // само сравнение: «сила 1 против сложности 1» и «сложность 2 → 1» двумя
  // строками читаются как два разных факта, хотя это один — откуда, куда и
  // против чего (§12.71).
  const cut =
    g.danger !== g.base ? ` (было ${g.base} — ведёт ${esc(g.guide)})` : "";
  rows.push(
    `<div class="raidwin-verdict">${verdict}` +
      `<i>${
        empty
          ? `сложность ${g.danger}`
          : `сила ${node.force ?? 0} против сложности ${g.danger}${cut}`
      }</i></div>`,
  );

  const facts = [];
  // Срок — вилкой, а не одним числом, и это ответ на «сколько это займёт» ещё
  // до набора отряда (§12.71). Считается он как «дорога + работа на лапу»,
  // поэтому у одних заказов состав его двигает, а у других — нет вовсе, и
  // разница между «Свалкой» и «Сопровождением каравана» именно в этом. Одно
  // число этого не показывало: пустому отряду ядро возвращает одну дорогу, то
  // есть лучший случай, который не наступит никогда.
  //
  // Границы берём из ядра (`span_slow`/`span_fast`, тот же `duration`), а не
  // делим работу на лапы в JS.
  // Срок показывается **формулой, а не итогом**, и это отвечает сразу на три
  // вопроса вместо одного (§12.71): сколько ждать, ускорит ли отряд и почему.
  //
  // Слагаемых обязательно два. «Срок 240 = 420 / 2 кота» было бы короче, но
  // неверно: делится только работа, дорога фиксирована (§12.70), и по такой
  // записи игрок ждал бы впятером 84 вместо 132. Разложение снимает и нужду в
  // гипотетическом «отрядом из 5 — 132»: из формулы видно и что рычаг есть, и
  // что отдача от него убывает, — а настоящее число меняется на глазах, как
  // только берёшь кота, ради чего окно и перерисовывается каждым кадром.
  //
  // Заказ без работы на месте — это формула без второго слагаемого, и «от
  // состава не зависит» в нём видно из самой записи.
  const slow = raids[i]?.span_slow;
  const work = def.work ?? 0;
  const road = `дорога ${def.travel ?? 0}`;
  if (slow != null) {
    if (work === 0) {
      facts.push(`срок ${spanText(slow)} = ${road}, и только`);
    } else if (empty) {
      // Отряда нет — считать не на кого, поэтому показываем медленный край и
      // помечаем его «до»: число верное (столько выйдет у минимального
      // состава), но без предлога оно читается как обещание, а это потолок.
      facts.push(
        `срок до ${spanText(slow)} = ${road} + работа ${work} / ${pawsWord(g.need)}`,
      );
    } else {
      facts.push(
        `срок ${spanText(g.span)} = ${road} + работа ${work} / ${pawsWord(g.paws)}`,
      );
    }
  }
  facts.push(
    g.need === g.most ? `нужно котов ${g.need}` : `котов ${g.need}—${g.most}`,
  );
  // Бодрость и здоровье укрупнены вдесятеро ради ступени «Выносливости»
  // (§12.70), и сырые «−12000» не значат для игрока ничего. Долей от полной
  // шкалы — значат: шкалы он видит полосками в карточке кота. Максимум берётся
  // из самого кота, а не из второго экземпляра рулсета в JS.
  if (def.toll) facts.push(`бодрости −${scaleText(def.toll, "energy_max")}`);
  if (def.harm) {
    facts.push(`раны при провале ${scaleText(def.harm, "health_max")}`);
  }
  if (def.fame) facts.push(`известность +${def.fame}`);
  rows.push(`<div class="cat-sub">${facts.join(" · ")}</div>`);

  // Добыча целиком, а не обрезанная краем колонки, — ровно то, чего не было
  // видно в тулбаре. Рядом с ней доля: полную получают не всегда.
  if (def.rescue) {
    rows.push('<div class="cat-sub">возвращает пленного, а не добычу</div>');
  } else {
    const loot = costChips(def.loot);
    if (loot) {
      rows.push(
        `<div class="raidwin-loot">добыча ${loot}` +
          (g.share > 0 && g.share < 100 ? ` <i>× ${g.share} %</i>` : "") +
          "</div>",
      );
    }
  }

  // Заказчик и пострадавший: репутация — единственная знаковая шкала, и цену
  // выбора стороны игрок обязан видеть до нажатия (§12.43).
  const fname = (id) => {
    const f = (meta.factions ?? []).find((v) => v.id === id);
    return esc(f?.label || id);
  };
  if (def.patron || def.against) {
    const moves = [];
    if (def.patron) moves.push(`${fname(def.patron)} +${def.standing ?? 0}`);
    if (def.against) moves.push(`${fname(def.against)} −${def.standing ?? 0}`);
    rows.push(`<div class="cat-sub">репутация: ${moves.join(" · ")}</div>`);
  }

  // Тот же заказ мог уже стоять правилом у соседнего отряда (§12.67). Молчать
  // об этом нельзя: игрок ставит второе такое же правило, а потом видит, что
  // на заказ уходят по очереди двое, и читает это как случайность. Зовём отряд
  // номером — тем же, что и в списке котов из чужих отрядов (§12.73).
  const alsoAuto = nodes
    .map((o, k) => (o !== node && o.auto === i ? k + 1 : 0))
    .filter(Boolean);
  if (alsoAuto.length) {
    rows.push(
      `<div class="cat-sub">уже ходит сам: ${alsoAuto
        .map((k) => `Отряд ${k}`)
        .join(", ")}</div>`,
    );
  }

  // Причина отказа — строкой, а не в нативной подсказке: на `disabled`-кнопке
  // её не увидеть вовсе, и закрытый заказ читался как поломка.
  if (g.reason) rows.push(`<div class="raidwin-why">${esc(g.reason)}</div>`);
  else if (g.home.length) {
    rows.push(
      `<div class="cat-sub">дома остаются: ${g.home.map(esc).join(", ")}</div>`,
    );
  }

  const go =
    `<button class="tool raid-go${g.ready ? " on" : ""}"` +
    `${g.ready ? "" : " disabled"}` +
    ` data-key="go${i}@${at}" data-def="${i}" data-x="${node.x}" data-y="${node.y}">` +
    (g.ready ? "Отправить" : "Нельзя") +
    "</button>";
  // Ворота автоматики (§12.93, §12.94): без технологии правило не поставить, и причина
  // называется словом — по `.off` браузер события мыши шлёт, в отличие от
  // `disabled` (§12.71).
  const autoGate = autoGateHint("raids");
  // Тумблер доступен и у закрытого заказа: правило ждёт ворот, как порог
  // производства ждёт материала, и поставить его заранее — это план (§12.67).
  // А вот до самой технологии тумблера нет вовсе: автоматики в игре ещё не
  // существует, и кнопка обещала бы механику, о которой игрок не знает.
  const auto = autoGate
    ? ""
    : paused
      ? `<button class="tool raid-pause" data-key="auto${i}@${at}" data-on="1"` +
        ` data-x="${node.x}" data-y="${node.y}"` +
        ' title="Правило стоит на паузе — вернуть отряд на этот круг">' +
        "↻ неактивно</button>"
      : `<button class="tool raid-auto${on ? " on" : ""}"` +
        ` data-key="auto${i}@${at}" data-def="${on ? -1 : i}"` +
        ` data-x="${node.x}" data-y="${node.y}"` +
        ` title="${on ? "Отряд ходит сюда сам — снять правило" : "Ходить сюда самому, как только отряд готов"}">` +
        (on ? "↻ автовылазка" : "↻") +
        "</button>";

  return (
    `<div class="raidwin-card${g.ready ? "" : " off"}">` +
    `<div class="raidwin-name">${esc(def.label || def.id || "Вылазка")}</div>` +
    rows.join("") +
    `<div class="raidwin-act">${go}${auto}</div>` +
    "</div>"
  );
}

// Связь — слагаемое силы отряда наравне с котами (§12.60), и в составе она
// стоит рядом с ними по одной причине: без неё сила растёт «сама собой». Она
// копится за каждый тик дежурства, поэтому число в шапке ползёт вверх, пока
// игрок смотрит на карту, — и без подписи это выглядит поломкой, а не
// дежурным, который сел к рации. Дежурного зовём по имени: он не в отряде, но
// вклад его в отряде, и его же нельзя трогать, пока идёт вылазка.
//
// Дежурного ищем так же, как панель клетки узла (`relayLines`), — по занятию на
// клетке этого узла: в снимке миссии есть только `manned`, «держат или нет».
function commsPart(raid, node) {
  if (!(raid.comms > 0)) return [];
  const on = (lastSnap?.entities ?? []).find(
    (e) => e.job === "relay" && !e.moving && e.x === node.x && e.y === node.y,
  );
  // Оборвавшаяся связь своё уже накопленное не теряет (§12.60), поэтому число
  // остаётся, а меняется только приписка: «+4» без дежурного больше не растёт.
  const who = raid.manned
    ? on
      ? `: ${esc(on.id)}`
      : ""
    : " — оборвалась, больше не растёт";
  return [`связь +${raid.comms}${who}`];
}

// Карточка узла, у которого уже есть вылазка. До этого здесь стояла строка
// «Пока он не вернётся, новую отсюда не отправить» и отсылка в панель справа —
// то есть окно, открытое ради решений об отряде, на время вылазки переставало
// показывать сам отряд и не давало ни одного решения о нём. Здесь их два, и оба
// касаются именно этого узла: отозвать ещё не ушедших и снять повтор.
function busyCard(raid, node) {
  const at = `${node.x},${node.y}`;
  const label = esc(missionLabel(raid.def));
  const rows = [`<div class="raidwin-name">${label}</div>`];
  // Прогноз идущей вылазки — из тех же полей снимка, что и в панели справа
  // (`outcome`, инвариант 14). Пока отряд собирается, это ещё и предупреждение:
  // увидел «провал» — успел отозвать, и кнопка отзыва стоит тут же.
  const verdict = raid.failed
    ? '<span class="bad">провал</span>'
    : raid.rescue
      ? '<span class="good">выносят своих</span>'
      : raid.share >= 100
        ? '<span class="good">вся добыча</span>'
        : `<span class="warn">добыча ${raid.share} %</span>`;
  rows.push(
    `<div class="raidwin-verdict">${verdict}` +
      `<i>сила ${raid.strength} против сложности ${raid.danger}</i></div>`,
  );
  rows.push(
    `<div class="cat-sub">${
      raid.away
        ? `в поле · вернутся через ${raid.left} из ${raid.total}`
        : raid.resting
          ? "ждут, пока выспится боец — срок пойдёт с ухода"
          : "собираются у шлюза — срок пойдёт с ухода"
    }</div>`,
  );
  // Отзыв — только пока отряд на базе, и это правило ядра (§12.22): ушедший
  // отряд не отзывается вовсе, `cancel_mission` его и не примет. Поэтому у
  // ушедшего кнопки здесь нет — не «погашена», а нет: гасить нечего, решение
  // кончилось в момент ухода.
  const act = [];
  if (!raid.away) {
    act.push(
      `<button class="tool mission-cancel" data-key="drop@${at}"` +
        ` data-def="${raid.def}">Отозвать</button>`,
    );
  }
  // Повтор снимается прямо отсюда — иначе на всю вылазку его нечем снять, и
  // следующая уходит «сама» (§12.67). Правило про узел, а не про заказ, поэтому
  // показываем его и когда повторяется не эта вылазка, а другая, — но заказ в
  // кнопке называем **только в этом случае**: карточка одна и уже озаглавлена
  // своим именем, и повторять его в кнопке значит писать его дважды подряд.
  if (node.auto >= 0) {
    const same = node.auto === raid.def;
    const name = `: «${esc(missionLabel(node.auto))}»`;
    act.push(
      `<button class="tool raid-pause${node.auto_on ? " on" : ""}"` +
        ` data-key="pause@${at}" data-on="${node.auto_on ? "" : "1"}"` +
        ` data-x="${node.x}" data-y="${node.y}"` +
        ` title="${
          node.auto_on
            ? "Отряд ходит сюда сам — приостановить правило"
            : "Правило на паузе — вернуть отряд на этот круг"
        }">` +
        (!raidOpen(node.auto)
          ? "↻ недоступно"
          : node.auto_on
            ? "↻ автовылазка"
            : "↻ неактивно") +
        (same ? "" : name) +
        "</button>",
      `<button class="tool raid-auto" data-key="off@${at}" data-def="-1"` +
        ` data-x="${node.x}" data-y="${node.y}"` +
        ' title="Забыть правило совсем">Снять</button>',
    );
  }
  if (act.length) rows.push(`<div class="raidwin-act">${act.join("")}</div>`);
  return `<div class="raidwin-card">${rows.join("")}</div>`;
}

/// Строка правила автовылазки — одна на раздел «Вылазки» и на штаб (§12.77).
///
/// Правило у узла одно, а решений по нему два: «пока не ходи» и «забудь». Пауза
/// стоит первой, потому что она обратима: игрок прерывает круг ради разового
/// дела на базе — разгрести привезённое — и возвращается к тому же заказу, не
/// выбирая его среди карточек второй раз. «Снять» остаётся рядом: правило,
/// которое нечем стереть, читалось бы как навязанное.
function ruleRow(node, at) {
  const label = esc(missionLabel(node.auto));
  // Правило живо, а заказ закрылся: репутация упала, известность ещё не дошла,
  // спасать стало некого. Отряд молча стоит, а строка до §12.78 говорила
  // «автовылазка» — это ровно та тишина, которую игрок читает как поломку.
  // Ворота спрашиваем у ядра (`raids[def]`, §12.24, §12.43), причину словом
  // здесь не пишем: она уже написана в штабе, у самой карточки заказа, и
  // второй её экземпляр в тулбаре однажды разойдётся с первым.
  // Закрытость заказа от паузы не зависит: усыплённое правило будят, чтобы
  // отряд пошёл, — и «Возобновить», после которого ничего не происходит,
  // читается как поломка ровно так же. Поэтому и слово одно на оба случая:
  // «недоступно» вытесняет и «автовылазка», и «неактивно». Разница между
  // жёлтым «неактивно» и красным «неактивно» — это разница, которую игрок
  // читает как оттенок, а не как новость.
  const blocked = !raidOpen(node.auto);
  const state = blocked
    ? "недоступно"
    : node.auto_on
      ? "автовылазка"
      : "неактивно";
  return (
    `<div class="raid-rule${node.auto_on ? "" : " off"}${
      blocked ? " blocked" : ""
    }">` +
    `<span${
      blocked
        ? ' title="Заказ сейчас недоступен — почему, написано в штабе вылазок"'
        : ""
    }>${state}: «${label}»</span>` +
    `<button class="tool raid-pause${node.auto_on ? " on" : ""}"` +
    ` data-key="pause@${at}" data-on="${node.auto_on ? "" : "1"}"` +
    ` data-x="${node.x}" data-y="${node.y}"` +
    ` title="${
      node.auto_on
        ? "Приостановить: заказ узел запомнит, но отряд по нему не уйдёт"
        : "Вернуть отряд на этот круг"
    }">` +
    (node.auto_on ? "Пауза" : "Возобновить") +
    "</button>" +
    `<button class="tool raid-auto" data-key="off@${at}" data-def="-1"` +
    ` data-x="${node.x}" data-y="${node.y}"` +
    ' title="Забыть правило совсем">Снять</button>' +
    "</div>"
  );
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

// Времени выбор инструмента не касается: паузу ставит только игрок (§12.86).
function selectBuild(i, btn) {
  mode = "build";
  buildTile = i;
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
  // Темп ничего в тулбаре не трогает: ни раскрытый раздел, ни выбранный
  // инструмент (§12.86). «СТРОЙКА: Пол» поверх идущего ×10 — законное
  // состояние: разметка это команда на будущее, а не действие по живой карте, и
  // ждать паузы ради неё игрока никто не заставляет.
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
// Партия начинается с паузы: обновление страницы — единственный способ её
// продолжить (§12.45), и мир, разъезжающийся, пока игрок ещё читает экран, —
// это потерянные тики, о которых он не просил. Снимает её только игрок —
// пробелом или кнопкой темпа; сама она не уходит ни от раздела, ни от
// инструмента (§12.86). `lastSpeed` при этом уже ×1, так что пробел пускает
// время туда, где оно и было бы.
setSpeed(0);
