// WebWorker: держит симуляцию (bevy_ecs -> WASM) и крутит фиксированный тик.
//
// Модель времени (§11 concept.md): dt тика постоянный, детерминизм не зависит от
// скорости. Множитель (0/1/5/10) = сколько сим-тиков в реальную секунду. Пауза = 0.
// Постройка (set_tile) применяется сразу при получении команды — работает и на паузе.

import init, { Sim } from './wasm/sp_sim.js';
import wasmUrl from './wasm/sp_sim_bg.wasm?url';

const BASE_TPS = 6; // сим-тиков в секунду на ×1
const SIM_DT_MS = 1000 / BASE_TPS;
const MAX_STEPS_PER_FRAME = 2000;

let sim = null;
let speed = 1; // 0 (пауза) | 1 | 5 | 10
let acc = 0;
let last = 0;
let mapDirty = false;

async function boot() {
  await init(wasmUrl);
  const yaml = await (await fetch('/rulesets/core.yaml')).text();
  sim = new Sim(yaml);
  postMessage({ type: 'ready', meta: sim.map_meta(), map: sim.base_map() });
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
  } else {
    acc = 0;
  }

  if (sim) {
    if (mapDirty) {
      postMessage({ type: 'map', map: sim.base_map() });
      mapDirty = false;
    }
    postMessage({ type: 'snapshot', snap: sim.snapshot() });
  }
  setTimeout(loop, 16);
}

onmessage = (e) => {
  const m = e.data;
  if (m.type === 'setSpeed') {
    speed = m.speed;
  } else if (m.type === 'build' && sim) {
    if (sim.set_tile(m.x, m.y, m.tile)) mapDirty = true;
  } else if (m.type === 'move' && sim) {
    sim.set_target(m.id, m.x, m.y);
  }
};

boot().catch((err) => postMessage({ type: 'error', message: String((err && err.stack) || err) }));
