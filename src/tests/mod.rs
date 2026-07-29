//! Тесты ядра. Гоняют ту же цепочку систем (`build_schedule`), что и боевая
//! симуляция, поэтому проверяют реальные взаимодействия, а не их копию.
//!
//! Баги здесь живут во взаимодействии систем и ECS-фильтров, а не в отдельных
//! функциях, поэтому механику покрываем прогоном полной цепочки (`tick_n`),
//! а не юнит-тестом функции. Мир собирается из ASCII-схем (`sim_from`), минуя
//! YAML; общие хелперы — в этом файле, сами тесты разложены по механикам.

mod demolition;
mod jobs;
mod orders;
mod paths;
mod voids;

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::BaseMap;
use crate::movement::is_stuck;
use crate::ruleset::TileDef;
use crate::schedule::build_schedule;
use crate::sim::Sim;

/// Строки ASCII-схемы из файла `src/test_maps/*.map` (через `include_str!`
/// на вызывающей стороне) — для схем, которые `rustfmt` иначе схлопывает
/// в одну строку и делает нечитаемыми.
fn rows_from(text: &'static str) -> Vec<&'static str> {
    text.lines().filter(|l| !l.is_empty()).collect()
}

/// Собирает `Sim` из ASCII-схемы, минуя YAML: `#` — пустота (непроходимо),
/// `.` — пол, любая другая буква — пол с котом под этим id.
fn sim_from(rows: &[&str]) -> Sim {
    let height = rows.len() as i32;
    let width = rows[0].len() as i32;
    let mut map = BaseMap::empty(width, height);
    let mut world = World::new();

    for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len() as i32, width, "строки схемы разной длины");
        for (x, ch) in row.chars().enumerate() {
            let (x, y) = (x as i32, y as i32);
            if ch == '#' {
                continue;
            }
            map.set(x, y, 0);
            if ch != '.' {
                world.spawn((
                    UnitId(ch.to_string()),
                    Renderable {
                        sprite: "cat".to_string(),
                    },
                    Position { x, y },
                ));
            }
        }
    }

    world.insert_resource(map);
    world.insert_resource(SimTime { tick: 0 });
    Sim {
        world,
        schedule: build_schedule(),
        palette: vec![TileDef {
            id: "floor".to_string(),
            label: "Пол".to_string(),
            color: "#000000".to_string(),
        }],
        width,
        height,
    }
}

impl Sim {
    fn tick_n(&mut self, n: usize) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn pos_of(&mut self, unit: &str) -> (i32, i32) {
        let mut q = self.world.query::<(&UnitId, &Position)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, p)| (p.x, p.y))
            .expect("кот не найден")
    }

    fn stuck_of(&mut self, unit: &str) -> bool {
        let mut q = self.world.query::<(
            &UnitId,
            &Position,
            Option<&Order>,
            Option<&Path>,
            Option<&Assignment>,
        )>();
        let map = self.world.resource::<BaseMap>();
        q.iter(&self.world)
            .find(|(id, ..)| id.0 == unit)
            .map(|(_, p, o, path, a)| is_stuck(map, p, o, path, a))
            .expect("кот не найден")
    }

    /// `tried_version` приказа, если приказ ещё висит.
    fn order_tried_version(&mut self, unit: &str) -> Option<u64> {
        let mut q = self.world.query::<(&UnitId, Option<&Order>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .and_then(|(_, o)| o.map(|o| o.tried_version))
    }

    fn has_assignment(&mut self, unit: &str) -> bool {
        let mut q = self.world.query::<(&UnitId, Option<&Assignment>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, a)| a.is_some())
            .unwrap_or(false)
    }

    fn has_path(&mut self, unit: &str) -> bool {
        let mut q = self.world.query::<(&UnitId, Option<&Path>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, p)| p.is_some())
            .unwrap_or(false)
    }

    fn tile(&self, x: i32, y: i32) -> i16 {
        self.world.resource::<BaseMap>().tile_at(x, y)
    }

    fn map_ver(&self) -> u64 {
        self.world.resource::<BaseMap>().version
    }

    /// Изменить тайл в обход чертежей — для проверки реакции на смену карты.
    fn force_tile(&mut self, x: i32, y: i32, tile: i16) {
        self.world.resource_mut::<BaseMap>().set(x, y, tile);
    }

    /// Сколько клеток пола осталось в прямоугольнике.
    fn floors_left(&self, rect: [i32; 4]) -> i32 {
        let [x, y, w, h] = rect;
        let map = self.world.resource::<BaseMap>();
        (0..h)
            .flat_map(|dy| (0..w).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| map.tile_at(x + dx, y + dy) >= 0)
            .count() as i32
    }
}
