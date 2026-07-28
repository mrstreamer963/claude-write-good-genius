//! SP / Эсперы — ядро симуляции.
//!
//! bevy_ecs-мир крутится в WebWorker (WASM). Контент грузится из YAML-рулсета
//! (стиль OpenXcom Extended). Наружу отдаём:
//!   * `map_meta()`  — размеры и палитра тайлов (один раз, для рендера);
//!   * `base_map()`  — текущее состояние тайлов базы (при изменениях);
//!   * `set_tile()`  — команда постройки/сноса (мгновенно, работает и на паузе);
//!   * `set_target()`— приказ коту идти в тайл (движение идёт по тикам);
//!   * `tick()`      — один фиксированный шаг симуляции;
//!   * `snapshot()`  — состояние рендерабельных сущностей (каждый кадр).
//!
//! Проходимость: ходить можно только по построенному полу (индекс тайла >= 0);
//! пустые клетки (-1) непроходимы. Путь ищется BFS по 4-связной сетке.
//!
//! Детерминизм: единственный источник случайности — `Rng` в ресурсе (пока не используется
//! для движения; юниты ходят строго по приказам).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;

/// Тиков между шагами юнита (при BASE_TPS=6 и периоде 1 — ~3 тайла/сек на ×1).
const MOVE_PERIOD: u8 = 1;

// ------------------------------------------------------------------
// Рулсет (YAML) — data-driven контент.
// ------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Ruleset {
    grid: GridDef,
    #[serde(default)]
    tiles: Vec<TileDef>,
    #[serde(default)]
    build: Vec<BuildRect>,
    #[serde(default)]
    units: Vec<UnitDef>,
}

#[derive(Debug, Deserialize, Clone)]
struct GridDef {
    width: i32,
    height: i32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
struct TileDef {
    id: String,
    #[serde(default)]
    label: String,
    color: String,
}

#[derive(Debug, Deserialize, Clone)]
struct BuildRect {
    tile: String,
    /// [x, y, w, h] в тайлах
    rect: [i32; 4],
}

#[derive(Debug, Deserialize, Clone)]
struct UnitDef {
    id: String,
    sprite: String,
    /// [x, y] в тайлах
    pos: [i32; 2],
}

// ------------------------------------------------------------------
// Компоненты и ресурсы.
// ------------------------------------------------------------------

#[derive(Component)]
struct Position {
    x: i32,
    y: i32,
}

#[derive(Component)]
struct Renderable {
    sprite: String,
}

#[derive(Component)]
struct UnitId(String);

/// Куда юнит идёт (конечная цель приказа).
#[derive(Component)]
struct Target {
    x: i32,
    y: i32,
}

/// Оставшийся маршрут. Хранится «развёрнуто»: следующий шаг — последний элемент
/// (`pop()` = следующая клетка), первый элемент — конечная цель.
#[derive(Component)]
struct Path {
    steps: Vec<(i32, i32)>,
}

/// Обратный отсчёт тиков до следующего шага.
#[derive(Component)]
struct MoveCooldown(u8);

/// Тайловая карта базы. Значение ячейки — индекс в палитре тайлов, либо -1 (пусто).
#[derive(Resource)]
struct BaseMap {
    width: i32,
    height: i32,
    cells: Vec<i16>,
}

impl BaseMap {
    fn empty(width: i32, height: i32) -> Self {
        BaseMap {
            width,
            height,
            cells: vec![-1; (width * height) as usize],
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some((y * self.width + x) as usize)
        }
    }

    /// Проходима ли клетка (построен ли пол).
    fn walkable(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.cells[i] >= 0)
    }

    /// Поставить тайл (`tile` — индекс палитры, `-1` = снести). Вернёт true при изменении.
    fn set(&mut self, x: i32, y: i32, tile: i16) -> bool {
        match self.index(x, y) {
            Some(i) if self.cells[i] != tile => {
                self.cells[i] = tile;
                true
            }
            _ => false,
        }
    }

    fn fill_rect(&mut self, rect: [i32; 4], tile: i16) {
        let [x, y, w, h] = rect;
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, tile);
            }
        }
    }
}

/// BFS по проходимым клеткам. Возвращает маршрут в «развёрнутом» виде:
/// `[goal, .., first_step]` (без стартовой клетки), либо None если пути нет.
fn find_path(map: &BaseMap, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    if start == goal {
        return Some(Vec::new());
    }
    let (w, h) = (map.width, map.height);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let start_i = idx(start.0, start.1);

    let mut came: Vec<i32> = vec![-1; (w * h) as usize];
    came[start_i] = start_i as i32; // помечаем старт посещённым
    let mut queue = VecDeque::new();
    queue.push_back(start);

    const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    while let Some((cx, cy)) = queue.pop_front() {
        for (dx, dy) in DIRS {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let ni = idx(nx, ny);
            if came[ni] != -1 || !map.walkable(nx, ny) {
                continue;
            }
            came[ni] = idx(cx, cy) as i32;
            if (nx, ny) == goal {
                let mut path = Vec::new();
                let mut cur = ni;
                while cur != start_i {
                    path.push(((cur as i32) % w, (cur as i32) / w));
                    cur = came[cur] as usize;
                }
                return Some(path); // [goal, .., first_step]
            }
            queue.push_back((nx, ny));
        }
    }
    None
}

#[derive(Resource)]
struct SimTime {
    tick: u64,
}

#[derive(Resource)]
struct Rng {
    #[allow(dead_code)]
    state: u64,
}

// ------------------------------------------------------------------
// Системы.
// ------------------------------------------------------------------

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

/// Двигает юнитов по маршруту. Если следующая клетка перестала быть проходимой
/// (игрок снёс пол), маршрут пересчитывается; если пути нет — юнит останавливается.
fn move_units(map: Res<BaseMap>, mut q: Query<(&mut Position, &mut Path, &mut MoveCooldown)>) {
    for (mut pos, mut path, mut cd) in &mut q {
        if path.steps.is_empty() {
            continue; // прибыл / приказа нет
        }
        if cd.0 > 0 {
            cd.0 -= 1;
            continue;
        }

        let next = *path.steps.last().unwrap();
        if !map.walkable(next.0, next.1) {
            let goal = *path.steps.first().unwrap();
            match find_path(&map, (pos.x, pos.y), goal) {
                Some(p) => path.steps = p,
                None => path.steps.clear(),
            }
            cd.0 = MOVE_PERIOD;
            continue;
        }

        path.steps.pop();
        pos.x = next.0;
        pos.y = next.1;
        cd.0 = MOVE_PERIOD;
    }
}

// ------------------------------------------------------------------
// DTO наружу (worker -> main), сериализуются serde-wasm-bindgen.
// ------------------------------------------------------------------

#[derive(Serialize)]
struct MapMeta {
    width: i32,
    height: i32,
    palette: Vec<TileDef>,
}

#[derive(Serialize)]
struct BaseMapDto<'a> {
    width: i32,
    height: i32,
    cells: &'a [i16],
}

#[derive(Serialize)]
struct Snapshot {
    tick: u64,
    entities: Vec<EntitySnap>,
}

#[derive(Serialize)]
struct EntitySnap {
    id: String,
    sprite: String,
    x: i32,
    y: i32,
}

// ------------------------------------------------------------------
// WASM-интерфейс.
// ------------------------------------------------------------------

#[wasm_bindgen]
pub struct Sim {
    world: World,
    schedule: Schedule,
    palette: Vec<TileDef>,
    width: i32,
    height: i32,
}

#[wasm_bindgen]
impl Sim {
    /// Создать симуляцию из текста YAML-рулсета.
    #[wasm_bindgen(constructor)]
    pub fn new(ruleset_yaml: &str) -> Result<Sim, JsValue> {
        console_error_panic_hook::set_once();

        let rs: Ruleset = serde_yaml::from_str(ruleset_yaml)
            .map_err(|e| JsValue::from_str(&format!("ruleset parse error: {e}")))?;

        let (w, h) = (rs.grid.width, rs.grid.height);
        let tile_index = |id: &str| rs.tiles.iter().position(|t| t.id == id).map(|i| i as i16);

        let mut map = BaseMap::empty(w, h);
        for b in &rs.build {
            if let Some(idx) = tile_index(&b.tile) {
                map.fill_rect(b.rect, idx);
            }
        }

        let mut world = World::new();
        world.insert_resource(map);
        world.insert_resource(SimTime { tick: 0 });
        world.insert_resource(Rng {
            state: 0x9E37_79B9_7F4A_7C15,
        });

        for u in &rs.units {
            world.spawn((
                UnitId(u.id.clone()),
                Renderable {
                    sprite: u.sprite.clone(),
                },
                Position {
                    x: u.pos[0],
                    y: u.pos[1],
                },
            ));
        }

        let mut schedule = Schedule::default();
        schedule.add_systems((advance_time, move_units));

        Ok(Sim {
            world,
            schedule,
            palette: rs.tiles,
            width: w,
            height: h,
        })
    }

    /// Размеры и палитра тайлов — отдаём один раз для настройки рендера.
    pub fn map_meta(&self) -> Result<JsValue, JsValue> {
        let meta = MapMeta {
            width: self.width,
            height: self.height,
            palette: self.palette.clone(),
        };
        serde_wasm_bindgen::to_value(&meta).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Текущее состояние тайлов базы.
    pub fn base_map(&self) -> Result<JsValue, JsValue> {
        let map = self.world.resource::<BaseMap>();
        let dto = BaseMapDto {
            width: map.width,
            height: map.height,
            cells: &map.cells,
        };
        serde_wasm_bindgen::to_value(&dto).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Постройка/снос от игрока. `tile` — индекс палитры или -1 (снести).
    /// Мгновенно, независимо от тика (строить можно и на паузе). Вернёт true при изменении.
    pub fn set_tile(&mut self, x: i32, y: i32, tile: i32) -> bool {
        self.world.resource_mut::<BaseMap>().set(x, y, tile as i16)
    }

    /// Приказ коту `unit_id` идти в тайл (x, y). Само движение идёт по тикам.
    /// Вернёт true, если приказ принят (цель проходима и достижима).
    pub fn set_target(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        // Найти сущность и её позицию.
        let mut found = None;
        {
            let mut q = self.world.query::<(Entity, &UnitId, &Position)>();
            for (e, id, p) in q.iter(&self.world) {
                if id.0 == unit_id {
                    found = Some((e, p.x, p.y));
                    break;
                }
            }
        }
        let Some((entity, sx, sy)) = found else {
            return false;
        };

        // Проложить маршрут (границы заимствования ресурса — в блоке).
        let path = {
            let map = self.world.resource::<BaseMap>();
            if !map.walkable(x, y) {
                return false;
            }
            match find_path(map, (sx, sy), (x, y)) {
                Some(p) => p,
                None => return false,
            }
        };

        self.world.entity_mut(entity).insert((
            Target { x, y },
            Path { steps: path },
            MoveCooldown(0),
        ));
        true
    }

    /// Один фиксированный шаг симуляции.
    pub fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }

    /// Состояние рендерабельных сущностей (для PixiJS).
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        let tick = self.world.resource::<SimTime>().tick;

        let mut entities = Vec::new();
        let mut q = self.world.query::<(&UnitId, &Renderable, &Position)>();
        for (id, r, p) in q.iter(&self.world) {
            entities.push(EntitySnap {
                id: id.0.clone(),
                sprite: r.sprite.clone(),
                x: p.x,
                y: p.y,
            });
        }

        serde_wasm_bindgen::to_value(&Snapshot { tick, entities })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
