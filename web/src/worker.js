// WebWorker: держит симуляцию (bevy_ecs -> WASM) и крутит фиксированный тик.
//
// Модель времени (§11 concept.md): dt тика постоянный, детерминизм не зависит от
// скорости. Множитель (0/1/5/10) = сколько сим-тиков в реальную секунду. Пауза = 0.
// Постройка теперь через чертежи: игрок тянет рамку, коты строят по тикам.
// Карту шлём при росте её версии (правки игрока + завершённые постройки).

import init, { Sim } from './wasm/sp_sim.js';
import wasmUrl from './wasm/sp_sim_bg.wasm?url';

const BASE_TPS = 6; // сим-тиков в секунду на ×1
const SIM_DT_MS = 1000 / BASE_TPS;
const MAX_STEPS_PER_FRAME = 2000;
// Автосохранение (§12.45) — по двум часам сразу, что наступит раньше.
//
// Тики меряют потерю в игровом времени и не зависят от скорости. Но на паузе
// они не идут вовсе, а игрок при этом размечает стройку, шлёт приказы и
// заказывает сделки — то есть меняет ровно то, что лежит в снимке. Поэтому
// вторые часы настенные.
//
// В §11 это не дыра: снимок снимается по `&World` и мир не трогает, поэтому
// момент фотографирования ни на что в симуляции повлиять не может. Настенное
// время решает «когда», а не «что».
const AUTOSAVE_EVERY = 120; // сим-тиков (20 с на ×1)
const AUTOSAVE_EVERY_MS = 20000; // и не реже этого по реальным часам

// Команды, которые мир не меняют: после них сохранять нечего.
const READ_ONLY = new Set(['setSpeed', 'save', 'trace']);

let sim = null;
let speed = 1; // 0 (пауза) | 1 | 5 | 10
let acc = 0;
let last = 0;
let lastMapVersion = -1;
// Текст рулсета нужен и после старта: из него собирается и новая партия, и
// загруженная — правил в снимке нет и быть не должно (§12.45).
let yamlText = null;
let sinceSave = 0;
let lastSaveAt = 0;
// Менялся ли мир с последнего снимка. Без этого вкладка, забытая на паузе,
// переписывала бы одно и то же каждые двадцать секунд до конца времён.
let dirty = false;

function announce() {
  lastMapVersion = sim.map_version();
  sinceSave = 0;
  lastSaveAt = performance.now();
  dirty = false;
  postMessage({ type: 'ready', meta: sim.map_meta(), map: sim.base_map() });
}

async function boot() {
  await init(wasmUrl);
  yamlText = await (await fetch('/rulesets/core.yaml')).text();
  sim = new Sim(yamlText);
  announce();
  last = performance.now();
  loop();
}

function loop() {
  const now = performance.now();
  let dt = now - last;
  last = now;
  if (dt > 250) dt = 250;

  if (sim && speed > 0) {
    acc += dt * speed;
    let steps = 0;
    while (acc >= SIM_DT_MS && steps < MAX_STEPS_PER_FRAME) {
      sim.tick();
      acc -= SIM_DT_MS;
      steps++;
    }
    sinceSave += steps;
    if (steps > 0) dirty = true;
  } else {
    acc = 0;
  }

  // Снимок кладёт в localStorage главный поток: у воркера его нет вовсе.
  // Проверка снаружи ветки скорости — иначе на паузе она не выполнялась бы
  // никогда, а размеченное игроком терялось бы целиком.
  if (sim && dirty && (sinceSave >= AUTOSAVE_EVERY || now - lastSaveAt >= AUTOSAVE_EVERY_MS)) {
    sinceSave = 0;
    lastSaveAt = now;
    dirty = false;
    postMessage({ type: 'saved', json: sim.save(), auto: true });
  }

  if (sim) {
    const v = sim.map_version();
    if (v !== lastMapVersion) {
      postMessage({ type: 'map', map: sim.base_map() });
      lastMapVersion = v;
    }
    postMessage({ type: 'snapshot', snap: sim.snapshot() });
  }
  setTimeout(loop, 16);
}

onmessage = (e) => {
  const m = e.data;
  // Любая команда, кроме читающих, делает мир несохранённым — в том числе на
  // паузе, где тиков нет, а разметка есть.
  if (!READ_ONLY.has(m.type)) dirty = true;
  if (m.type === 'setSpeed') {
    speed = m.speed;
  } else if (m.type === 'build' && sim) {
    // Один жест игрока = одна рамка = один вызов. Ластик не сносит мгновенно:
    // сначала снимает чертежи в рамке, и только если снимать было нечего —
    // планирует снос, который выполняют коты (см. `plan_demolish_rect` в ядре).
    if (m.tile >= 0) sim.add_blueprint_rect(m.x, m.y, m.w, m.h, m.tile);
    else sim.plan_demolish_rect(m.x, m.y, m.w, m.h);
  } else if (m.type === 'store' && sim) {
    // Разметка уборки: кота не выбираем — как и чертёж, задачу возьмёт любой
    // свободный. Повторный жест по помеченному снимает пометку.
    sim.mark_to_store_rect(m.x, m.y, m.w, m.h);
  } else if (m.type === 'setAutoTidy' && sim) {
    sim.set_auto_tidy(m.on);
  } else if (m.type === 'setAutoRest' && sim) {
    // Второй порог усталости: бросает ли кот работу, когда бодрость на исходе
    // (см. `set_auto_rest` в ядре, §12.33). Выключение никого не будит.
    sim.set_auto_rest(m.on);
  } else if (m.type === 'move' && sim) {
    sim.set_target(m.id, m.x, m.y);
  } else if (m.type === 'launch' && sim) {
    // Отряд игрок выбирает поимённо — единственная работа, где исполнителя не
    // раздаёт симуляция: от состава зависит исход (см. `launch` в ядре, §12.23).
    // Неполную заявку ядро отклоняет само.
    sim.launch(m.mission, m.units);
  } else if (m.type === 'cancelMission' && sim) {
    sim.cancel_mission();
  } else if (m.type === 'research' && sim) {
    // Тема — разметка работы: кота не выбираем, за неё сядет ближайший, кому
    // хватает «Науки» (см. `start_research` в ядре, §12.26). Платит склад.
    sim.start_research(m.topic);
  } else if (m.type === 'cancelResearch' && sim) {
    sim.cancel_research();
  } else if (m.type === 'craft' && sim) {
    // Заказ — разметка работы со счётчиком штук: кота не выбираем, к верстаку
    // встанет ближайший свободный (см. `start_craft` в ядре, §12.30). Склад
    // платит за каждую штуку отдельно, поэтому пустой склад заявку не отменяет —
    // заказ ждёт материала.
    sim.start_craft(m.recipe, m.count);
  } else if (m.type === 'cancelCraft' && sim) {
    sim.cancel_craft();
  } else if (m.type === 'teach' && sim) {
    // Обучение адресно: игрок решает судьбу конкретного кота, а не размечает
    // работу базе (см. `teach` в ядре, §12.18).
    sim.teach(m.id, m.skill);
  } else if (m.type === 'trade' && sim) {
    // Сделка с внешним миром (см. `trade` в ядре, §12.44). Курс фиксируется
    // здесь и сейчас, покупка платится сразу, а товар едет `lead` тиков.
    // Команды отмены нет намеренно: деньги ушли, как уходят образцы за тему.
    sim.trade(m.faction, m.item, m.count, m.buying);
  } else if (m.type === 'hire' && sim) {
    // Известность открывает кандидата, платит склад (см. `hire` в ядре, §12.24).
    sim.hire(m.recruit);
  } else if (m.type === 'save' && sim) {
    // Один запрос, два адресата: `toSlot` — это уход со вкладки (пишем в
    // хранилище), без него — кнопка «Сохранить в файл».
    if (m.toSlot) {
      sinceSave = 0;
      lastSaveAt = performance.now();
      dirty = false;
    }
    postMessage({ type: 'saved', json: sim.save(), auto: !!m.toSlot });
  } else if (m.type === 'load' && yamlText) {
    // Снимок собирается в мир из **того же** рулсета: правил в нём нет (§12.45).
    // Отказ — это чужая версия формата или чужой рулсет, и он именно отказ:
    // молча загруженная чужая партия была бы тихо другим миром.
    try {
      sim = Sim.load(yamlText, m.json);
      announce();
    } catch (err) {
      postMessage({ type: 'loadFailed', message: String((err && err.message) || err) });
    }
  } else if (m.type === 'newGame' && yamlText) {
    sim = new Sim(yamlText);
    announce();
  } else if (m.type === 'trace' && sim) {
    postMessage({ type: 'traced', text: sim.trace() });
  }
};

boot().catch((err) => postMessage({ type: 'error', message: String((err && err.stack) || err) }));
