//! SP / Эсперы — ядро симуляции.
//!
//! bevy_ecs-мир крутится в WebWorker (WASM). Контент грузится из YAML-рулсета
//! (стиль OpenXcom Extended). Наружу отдаём:
//!   * `map_meta()`      — размеры и палитра тайлов (один раз, для рендера);
//!   * `map_version()`   — счётчик изменений карты (для отправки base_map по требованию);
//!   * `base_map()`      — текущее состояние тайлов базы;
//!   * `add_blueprint()` — поставить чертёж (задачу постройки); строят коты, по тикам;
//!   * `demolish()`      — отменить чертёж / снести готовый тайл (мгновенно);
//!   * `set_target()`    — приказ коту идти в тайл (движение по тикам, отменяет его задачу);
//!   * `tick()`          — один фиксированный шаг симуляции;
//!   * `snapshot()`      — рендерабельные сущности + чертежи (каждый кадр).
//!
//! Проходимость: ходить можно только по построенному полу (индекс тайла >= 0);
//! пустые клетки (-1) непроходимы. Путь ищется BFS по 4-связной сетке.
//!
//! Джобы постройки: чертёж — сущность `Blueprint`. Свободный (простаивающий) кот
//! назначается на ближайший достижимый чертёж, идёт к соседней проходимой клетке
//! и «работает» BUILD_TIME тиков, после чего тайл возводится.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;

/// Тиков между шагами юнита (при BASE_TPS=6 и периоде 1 — ~3 тайла/сек на ×1).
const MOVE_PERIOD: u8 = 1;
/// Тиков работы, чтобы возвести один тайл.
const BUILD_TIME: i32 = 12;

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

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

/// Оставшийся маршрут. Хранится «развёрнуто»: следующий шаг — последний элемент
/// (`pop()` = следующая клетка), первый элемент — конечная цель.
#[derive(Component)]
struct Path {
    steps: Vec<(i32, i32)>,
}

/// Обратный отсчёт тиков до следующего шага.
#[derive(Component)]
struct MoveCooldown(u8);

/// Кот назначен на джоб постройки (ссылка на сущность-чертёж).
#[derive(Component)]
struct Assignment(Entity);

/// Чертёж — запланированная постройка тайла.
#[derive(Component)]
struct Blueprint {
    x: i32,
    y: i32,
    tile: i16,
    progress: i32,
    assignee: Option<Entity>,
}

/// Тайловая карта базы. Значение ячейки — индекс в палитре тайлов, либо -1 (пусто).
#[derive(Resource)]
struct BaseMap {
    width: i32,
    height: i32,
    cells: Vec<i16>,
    version: u64,
}

impl BaseMap {
    fn empty(width: i32, height: i32) -> Self {
        BaseMap {
            width,
            height,
            cells: vec![-1; (width * height) as usize],
            version: 0,
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some((y * self.width + x) as usize)
        }
    }

    fn tile_at(&self, x: i32, y: i32) -> i16 {
        self.index(x, y).map_or(-1, |i| self.cells[i])
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
                self.version += 1;
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
    came[start_i] = start_i as i32;
    let mut queue = VecDeque::new();
    queue.push_back(start);

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
                return Some(path);
            }
            queue.push_back((nx, ny));
        }
    }
    None
}

/// Найти клетку, откуда можно строить чертёж (сам тайл, если проходим, либо сосед),
/// и маршрут кота до неё. Возвращает (клетка, маршрут).
fn path_to_build_spot(
    map: &BaseMap,
    from: (i32, i32),
    bp: (i32, i32),
) -> Option<((i32, i32), Vec<(i32, i32)>)> {
    let mut spots = Vec::new();
    if map.walkable(bp.0, bp.1) {
        spots.push(bp);
    }
    for (dx, dy) in DIRS {
        let n = (bp.0 + dx, bp.1 + dy);
        if map.walkable(n.0, n.1) {
            spots.push(n);
        }
    }
    for spot in spots {
        if spot == from {
            return Some((spot, Vec::new()));
        }
        if let Some(path) = find_path(map, from, spot) {
            return Some((spot, path));
        }
    }
    None
}

#[derive(Resource)]
struct SimTime {
    tick: u64,
}

// ------------------------------------------------------------------
// Системы (гоняются цепочкой каждый тик).
// ------------------------------------------------------------------

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

/// Назначает простаивающих котов (без задачи и без маршрута) на ближайшие
/// достижимые чертежи и отправляет их к месту постройки.
fn assign_jobs(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut blueprints: Query<(Entity, &mut Blueprint)>,
    free_cats: Query<(Entity, &Position), (With<UnitId>, Without<Assignment>, Without<Path>)>,
) {
    let mut free: Vec<(Entity, (i32, i32))> =
        free_cats.iter().map(|(e, p)| (e, (p.x, p.y))).collect();
    if free.is_empty() {
        return;
    }

    for (bp_e, mut bp) in &mut blueprints {
        if bp.assignee.is_some() {
            continue;
        }
        if free.is_empty() {
            break;
        }
        let mut chosen = None;
        for (i, (cat_e, cat_pos)) in free.iter().enumerate() {
            if let Some((_spot, path)) = path_to_build_spot(&map, *cat_pos, (bp.x, bp.y)) {
                chosen = Some((i, *cat_e, path));
                break;
            }
        }
        if let Some((i, cat_e, path)) = chosen {
            free.remove(i);
            bp.assignee = Some(cat_e);
            commands.entity(cat_e).insert((
                Assignment(bp_e),
                Path { steps: path },
                MoveCooldown(0),
            ));
        }
    }
}

/// Двигает юнитов по маршруту; на прибытии снимает компоненты движения.
fn move_units(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Position, &mut Path, &mut MoveCooldown)>,
) {
    for (e, mut pos, mut path, mut cd) in &mut q {
        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
            continue;
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

        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
        }
    }
}

/// Коты, добравшиеся до чертежа, «работают» до готовности; затем тайл возводится.
fn work_jobs(
    mut map: ResMut<BaseMap>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Assignment, Option<&Path>)>,
    mut blueprints: Query<&mut Blueprint>,
) {
    for (cat_e, pos, assign, path) in &cats {
        let Ok(mut bp) = blueprints.get_mut(assign.0) else {
            commands.entity(cat_e).remove::<Assignment>();
            continue;
        };

        let in_range = (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1;
        if in_range {
            bp.progress += 1;
            if bp.progress >= BUILD_TIME {
                map.set(bp.x, bp.y, bp.tile);
                commands.entity(assign.0).despawn();
                commands.entity(cat_e).remove::<Assignment>();
            }
        } else if path.is_none() {
            // Дошёл, но не в радиусе (маршрут оборвался) — освобождаем джоб.
            bp.assignee = None;
            commands.entity(cat_e).remove::<Assignment>();
        }
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
    blueprints: Vec<BlueprintSnap>,
}

#[derive(Serialize)]
struct EntitySnap {
    id: String,
    sprite: String,
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct BlueprintSnap {
    x: i32,
    y: i32,
    tile: i16,
    progress: i32,
    total: i32,
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

impl Sim {
    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    /// Сущность-чертёж на клетке (x, y), если есть.
    fn blueprint_at(&mut self, x: i32, y: i32) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &Blueprint)>();
        for (e, bp) in q.iter(&self.world) {
            if bp.x == x && bp.y == y {
                return Some(e);
            }
        }
        None
    }
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
        schedule.add_systems((advance_time, assign_jobs, move_units, work_jobs).chain());

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

    /// Счётчик изменений карты (воркер шлёт base_map, когда он вырос).
    pub fn map_version(&self) -> f64 {
        self.world.resource::<BaseMap>().version as f64
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

    /// Поставить чертёж (джоб постройки) `tile` на клетку (x, y). Строят коты, по тикам.
    /// Вернёт true, если чертёж добавлен/обновлён.
    pub fn add_blueprint(&mut self, x: i32, y: i32, tile: i32) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let t = tile as i16;
        if self.world.resource::<BaseMap>().tile_at(x, y) == t {
            return false; // уже построено этим тайлом
        }
        if let Some(e) = self.blueprint_at(x, y) {
            let mut bp = self.world.get_mut::<Blueprint>(e).unwrap();
            if bp.tile != t {
                bp.tile = t;
                bp.progress = 0;
            }
            return true;
        }
        self.world.spawn(Blueprint {
            x,
            y,
            tile: t,
            progress: 0,
            assignee: None,
        });
        true
    }

    /// Отменить чертёж на клетке / снести готовый тайл (мгновенно). Вернёт true при изменении.
    pub fn demolish(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        if let Some(e) = self.blueprint_at(x, y) {
            if let Some(cat) = self.world.get::<Blueprint>(e).and_then(|bp| bp.assignee) {
                self.world
                    .entity_mut(cat)
                    .remove::<(Assignment, Path, MoveCooldown)>();
            }
            self.world.entity_mut(e).despawn();
            changed = true;
        }
        if self.world.resource_mut::<BaseMap>().set(x, y, -1) {
            changed = true;
        }
        changed
    }

    /// Приказ коту `unit_id` идти в тайл (x, y). Отменяет его джоб постройки, если был.
    /// Вернёт true, если приказ принят (цель проходима и достижима).
    pub fn set_target(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
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

        // Снять текущую задачу постройки (освободить чертёж).
        if let Some(bp_e) = self.world.get::<Assignment>(entity).map(|a| a.0) {
            if let Some(mut bp) = self.world.get_mut::<Blueprint>(bp_e) {
                bp.assignee = None;
            }
            self.world.entity_mut(entity).remove::<Assignment>();
        }

        self.world.entity_mut(entity).insert((
            Path { steps: path },
            MoveCooldown(0),
        ));
        true
    }

    /// Один фиксированный шаг симуляции.
    pub fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }

    /// Рендерабельные сущности + чертежи (для PixiJS).
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        let tick = self.world.resource::<SimTime>().tick;

        let mut entities = Vec::new();
        {
            let mut q = self.world.query::<(&UnitId, &Renderable, &Position)>();
            for (id, r, p) in q.iter(&self.world) {
                entities.push(EntitySnap {
                    id: id.0.clone(),
                    sprite: r.sprite.clone(),
                    x: p.x,
                    y: p.y,
                });
            }
        }

        let mut blueprints = Vec::new();
        {
            let mut q = self.world.query::<&Blueprint>();
            for bp in q.iter(&self.world) {
                blueprints.push(BlueprintSnap {
                    x: bp.x,
                    y: bp.y,
                    tile: bp.tile,
                    progress: bp.progress,
                    total: BUILD_TIME,
                });
            }
        }

        serde_wasm_bindgen::to_value(&Snapshot {
            tick,
            entities,
            blueprints,
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
