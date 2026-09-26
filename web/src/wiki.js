// Справочник: лор игры, разложенный по записям палитр (§12.204).
//
// До него весь сеттинг — фракции, история мира, что такое эспер и зачем коту
// кольцо — жил в `ideas.md` и до игрока не доходил **никак**: в самой игре
// нарративом были одна строка `hint` и одна `detail` у события записки. Игрок
// строил стеллаж, не зная, чей это стеллаж и почему на базе вообще есть лом.
//
// ⚠️ **Тексты живут вне рулсета, и это не вкусовщина** (§12.204). Отпечаток
// партии — FNV по всему YAML байт в байт (`save::fingerprint`), значит правка
// запятой в описании стеллажа стирала бы игроку автосейв. Лор правят часто и
// мелко, поэтому он лежит в `assets/wiki/*.md`, которые Vite раздаёт тем же
// `publicDir`, что и рулсеты с портретами, а ядро о них не знает вовсе.
//
// ⚠️ **Ключ статьи — `вид:id`, а не индекс палитры.** Индексы ходят по
// протоколу и лежат в снимках, но вставка записи в середину `items:` сдвинула
// бы все статьи разом и молча: «Стеллаж» рассказывал бы про лазарет. `id` же
// в рулсете и есть то, что не меняется (на этом стоит и `tech` у тайла, и
// `requires` у рецепта).

// Разделы справочника. Список зашит здесь, а не собирается манифестом: файлов
// десяток, меняются они раз в полгода, и лишний круг запросов ради этого —
// плата ни за что.
const FILES = [
  "lore",
  "tiles",
  "items",
  "research",
  "recipes",
  "missions",
  "recruits",
  "factions",
  "rules",
  "skills",
];

// Ключ → { title, body }. Пусто, пока не загрузились: справочник не механика,
// и игра обязана работать без него (файл не доехал — значков «i» просто нет).
const articles = new Map();

let loaded = false;

/// Статьи, временно убранные из справочника. Текст лежит в `assets/wiki` и
/// стережётся `src/tests/wiki.rs` как прежде — прячет его только вид. Сейчас здесь
/// записка (механика жива, а как изложить её в лоре, ещё не решено) и кольцо. Вернуть —
/// убрать ключ из списка.
const HIDDEN_ARTICLES = new Set(["lore:note", "lore:ring"]);

/// Загрузить справочник. Зовётся один раз при старте страницы, а не на `ready`:
/// тексты принадлежат игре, а не партии, и новая партия их не меняет.
export async function loadWiki() {
  if (loaded) return;
  loaded = true;
  // ⚠️ Путь **через `BASE_URL`**, а не от корня: продакшн-сборка живёт под
  // `/cats-of-base/`, и абсолютный `/wiki/…` там даёт 404 — молча, потому что
  // отсутствие статей это законное состояние. Дев-сервер стоит в корне и такую
  // ошибку не показывает вовсе; так же грузит рулсет `worker.js`.
  const base = import.meta.env.BASE_URL;
  const texts = await Promise.all(
    FILES.map((name) =>
      fetch(`${base}wiki/${name}.md`)
        .then((r) => (r.ok ? r.text() : ""))
        // Раздела нет или сеть молчит — справочник просто беднее на раздел.
        // Ронять игру из-за текста нельзя: это подача, а не механика (§12.46).
        .catch(() => ""),
    ),
  );
  for (const text of texts) parseFile(text);
  for (const key of HIDDEN_ARTICLES) articles.delete(key);
}

// `---понято---` или `---понято tech_id---`: во втором случае полная версия
// открывается изучением технологии, а не пониманием предмета.
const UNDERSTOOD = /^---понято(?:\s+([a-z0-9_]+))?---$/;

/// Разобрать файл раздела. Статья начинается со строки `## ключ | Заголовок`;
/// заголовок необязателен — без него его даст палитра (`label` записи).
function parseFile(text) {
  let key = null;
  let title = "";
  let lines = [];
  const flush = () => {
    if (!key) return;
    // Маркер `---понято---` делит статью на две версии: выше — что база видит
    // снаружи, пока вещь не понята (§12.131), ниже — полная. Без маркера
    // версия одна, и `teaser` пуст.
    const cut = lines.findIndex((l) => UNDERSTOOD.test(l.trim()));
    const teaser = cut < 0 ? "" : lines.slice(0, cut).join("\n").trim();
    const tech = cut < 0 ? "" : (UNDERSTOOD.exec(lines[cut].trim())[1] ?? "");
    const body = lines.slice(cut + 1).join("\n").trim();
    articles.set(key, { title, teaser, body, tech });
  };
  for (const raw of text.split(/\r?\n/)) {
    const head = /^##\s+([a-z]+:[a-z0-9_]+)\s*(?:\|\s*(.*))?$/i.exec(raw);
    if (head) {
      flush();
      key = head[1].toLowerCase();
      title = (head[2] ?? "").trim();
      lines = [];
      continue;
    }
    if (key) lines.push(raw);
  }
  flush();
}

/// Есть ли статья. По этому и только по этому появляется значок «i»: кнопка,
/// открывающая пустоту, — это отказ без причины (§12.53), а мёртвых значков в
/// проекте нет ни одного.
///
/// ⚠️ **И статья открыта** (§12.251): справочник знает не больше базы. Ворота
/// считает вид по снимку (`setArticleGate` из `main.js`) — теми же полями, по
/// которым сама вещь появляется на экране; до первого снимка ворот нет.
export function hasArticle(key) {
  return articles.has(key) && gate(key);
}

let gate = () => true;
let resolve = (key) => key;

/// Поставить переадресацию: `(ключ) → ключ статьи, которая его заменяет`.
/// Тема «Стеллаж» ведёт в постройку «Стеллаж» (§12.251): два одноимённых
/// ответа на один вопрос читаются дублем.
export function setArticleAlias(fn) {
  resolve = fn;
}

/// Куда на самом деле ведёт ключ — с учётом переадресации.
export function articleTarget(key) {
  return resolve(key);
}
let teased = () => false;

/// Есть ли у статьи версия «до понимания» (маркер `---понято---`).
export function hasTeaser(key) {
  return !!articles.get(key)?.teaser;
}

/// Технология, открывающая полную версию статьи (`---понято tech---`); "" — нет.
export function teaserTech(key) {
  return articles.get(key)?.tech ?? "";
}

/// Показывать ли сейчас короткую версию: `(ключ) → вещь ещё не понята`.
/// Ответ считает вид по снимку, как и ворота.
export function setArticleTeased(fn) {
  teased = fn;
}

/// Текст статьи в той версии, какая открыта сейчас.
function bodyOf(key) {
  const art = articles.get(key);
  if (!art) return "";
  return art.teaser && teased(key) ? art.teaser : art.body;
}

/// Версия статьи как метка: по её смене вид объявляет «Запись дополнена».
export function articleStage(key) {
  return hasTeaser(key) && teased(key) ? "teaser" : "full";
}

/// Поставить ворота статей: `(ключ) → открыта ли`.
export function setArticleGate(fn) {
  gate = fn;
}

export function articleTitle(key) {
  return articles.get(key)?.title ?? "";
}

/// Ключи всех статей — по ним строится оглавление окна.
export function articleKeys() {
  return [...articles.keys()].filter(gate);
}

const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ESC[c]);
}

/// Разметка статьи → HTML. Свой минимальный набор, а не библиотека markdown:
/// нужны четыре вещи (абзац, выделение, цитата, ссылка), а тащить парсер на
/// сорок килобайт ради них — это вес, который платит каждый игрок.
///
/// ⚠️ **Экранируем первыми, размечаем потом.** Тексты свои, но правило это про
/// дисциплину, а не про недоверие: однажды в статью попадёт имя фракции из
/// рулсета, и порядок окажется единственным, что стоит между ним и разметкой.
///
/// Ссылка — `[[вид:id]]` или `[[вид:id|слово]]`. Ведёт она внутрь справочника
/// и наружу не умеет: внешних ссылок в игре нет и заводить их нельзя — окно
/// браузера поверх партии это выход из игры, а не переход по справочнику.
export function renderArticle(key, nameOf) {
  const art = articles.get(key);
  if (!art) return "";
  const link = (m, target, word) => {
    const t = resolve(target.toLowerCase());
    const label = word || nameOf(t) || t;
    // Битая ссылка остаётся словом, а не кнопкой в никуда: статью могли ещё не
    // написать, и «нажал — ничего» хуже, чем просто текст.
    return hasArticle(t)
      ? `<button class="wiki-link" data-key="wiki${t}" data-go="${esc(t)}">${esc(label)}</button>`
      : esc(label);
  };
  return bodyOf(key)
    .split(/\n\s*\n/)
    .map((block) => {
      const quote = block.startsWith("> ");
      const text = esc(quote ? block.replace(/^>\s?/gm, "") : block)
        .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
        .replace(/\*(.+?)\*/g, "<i>$1</i>")
        .replace(/\[\[([a-z]+:[a-z0-9_]+)(?:\|(.+?))?\]\]/gi, link)
        // Одиночный перевод строки — **пробел**, а не разрыв: файл переносят
        // по 80 знаков ради чтения глазами в редакторе, и уехавшая в игру
        // раскладка исходника рвала бы абзац посреди фразы. Жёсткий перенос
        // просят двумя пробелами в конце строки — как в markdown, чтобы
        // тот, кто правит текст, не учил ещё один диалект.
        .replace(/ {2,}\n/g, "<br>")
        .replace(/\n/g, " ");
      return quote
        ? `<blockquote class="wiki-quote">${text}</blockquote>`
        : `<p>${text}</p>`;
    })
    .join("");
}

/// Ссылки, которые статья ставит сама, — по ним строится «Смотри также».
/// Считаются **перекличкой с текстом**, а не вторым списком в файле: разойдись
/// они, и подвал обещал бы статью, которой в тексте нет (идиома `opensOf`).
export function articleLinks(key) {
  const body = bodyOf(key);
  const out = [];
  for (const m of body.matchAll(/\[\[([a-z]+:[a-z0-9_]+)(?:\|.+?)?\]\]/gi)) {
    const t = resolve(m[1].toLowerCase());
    if (hasArticle(t) && !out.includes(t)) out.push(t);
  }
  return out;
}
