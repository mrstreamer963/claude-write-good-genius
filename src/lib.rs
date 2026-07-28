//! SP / Эсперы — ядро симуляции.
//!
//! bevy_ecs-мир крутится в WebWorker (WASM). Контент грузится из YAML-рулсета
//! (стиль OpenXcom Extended). Наружу отдаём:
//!   * `map_meta()` — размеры и палитра тайлов (один раз, для рендера);
//!   * `base_map()` — текущее состояние тайлов базы (при изменениях);
//!   * `set_tile()` — команда постройки/сноса от игрока (работает и на паузе);
//!   * `tick()`     — один фиксированный шаг симуляции;
//!   * `snapshot()` — состояние рендерабельных сущностей (каждый кадр).
//!
//! Детерминизм: единственный источник случайности — `Rng` в ресурсе (xorshift).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

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

/// Маркер: сущность бродит по базе (заглушка поведения для каркаса).
#[derive(Component)]
struct Wander;

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

    /// Поставить тайл (`tile` — индекс палитры, `-1` = снести). Вернёт true, если что-то изменилось.
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

#[derive(Resource)]
struct SimTime {
    tick: u64,
}

#[derive(Resource)]
struct Rng {
    state: u64,
}

impl Rng {
    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn step(&mut self) -> (i32, i32) {
        match self.next_u64() % 5 {
            0 => (1, 0),
            1 => (-1, 0),
            2 => (0, 1),
            3 => (0, -1),
            _ => (0, 0),
        }
    }
}

// ------------------------------------------------------------------
// Системы.
// ------------------------------------------------------------------

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

fn wander(mut rng: ResMut<Rng>, map: Res<BaseMap>, mut q: Query<&mut Position, With<Wander>>) {
    for mut pos in &mut q {
        let (dx, dy) = rng.step();
        pos.x = (pos.x + dx).clamp(0, map.width - 1);
        pos.y = (pos.y + dy).clamp(0, map.height - 1);
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

        // Индекс палитры по id тайла (для разбора начальной застройки).
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
                Wander,
            ));
        }

        let mut schedule = Schedule::default();
        schedule.add_systems((advance_time, wander));

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

    /// Команда постройки/сноса от игрока. `tile` — индекс палитры или -1 (снести).
    /// Работает независимо от тика — строить можно и на паузе. Вернёт true при изменении.
    pub fn set_tile(&mut self, x: i32, y: i32, tile: i32) -> bool {
        self.world.resource_mut::<BaseMap>().set(x, y, tile as i16)
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
