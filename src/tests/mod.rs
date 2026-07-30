//! Тесты ядра. Гоняют ту же цепочку систем (`build_schedule`), что и боевая
//! симуляция, поэтому проверяют реальные взаимодействия, а не их копию.
//!
//! Баги здесь живут во взаимодействии систем и ECS-фильтров, а не в отдельных
//! функциях, поэтому механику покрываем прогоном полной цепочки (`tick_n`),
//! а не юнит-тестом функции. Мир собирается из ASCII-схем (`sim_from`), минуя
//! YAML; общие хелперы — в этом файле, сами тесты разложены по механикам.

mod demolition;
mod hauling;
mod items;
mod jobs;
mod needs;
mod orders;
mod paths;
mod skills;
mod tidying;
mod voids;

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::{BUILD_WORK, WORK_RATE};
use crate::map::BaseMap;
use crate::movement::is_stuck;
use std::collections::BTreeMap;

use crate::ruleset::TileDef;
use crate::schedule::build_schedule;
use crate::sim::Sim;

/// Тиков на один тайл при нулевом навыке. Работа считается в очках (§12.17),
/// но тесты меряют время тиками, поэтому пересчёт держим здесь.
const BUILD_TICKS: usize = (BUILD_WORK / WORK_RATE) as usize;

/// Строки ASCII-схемы из файла `src/test_maps/*.map` (через `include_str!`
/// на вызывающей стороне) — для схем, которые `rustfmt` иначе схлопывает
/// в одну строку и делает нечитаемыми.
fn rows_from(text: &'static str) -> Vec<&'static str> {
    text.lines().filter(|l| !l.is_empty()).collect()
}

/// Собирает `Sim` из ASCII-схемы, минуя YAML: `#` — пустота (непроходимо),
/// `.` — пол, любая другая буква — пол с котом под этим id.
///
/// Тайл схемы бесплатен и не склад (`TileRules` = нули): цена — это контент
/// рулсета, и тесты механик, не связанных с материалом, не должны о нём знать.
/// Тесты переноса задают свойства явно (`set_cost`, `set_capacity`).
///
/// Навыков в мире тоже нет (`SkillRules` пуст), и лапы у котов без предела:
/// работа идёт с базовой скоростью, а тесты навыков включают их сами
/// (`set_skill`, `set_carry`) — §12.17.
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
    world.insert_resource(TileRules(vec![TileRule::default()]));
    world.insert_resource(AutoTidy(true));
    world.insert_resource(SkillRules::default());
    world.insert_resource(NeedRules::default());
    Sim {
        world,
        schedule: build_schedule(),
        palette: vec![TileDef {
            id: "floor".to_string(),
            label: "Пол".to_string(),
            color: "#000000".to_string(),
            cost: BTreeMap::new(),
            capacity: 0,
            rest: 0,
        }],
        items: Vec::new(),
        skills: Vec::new(),
        perks: Vec::new(),
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
            Option<&Haul>,
            Option<&Rest>,
        )>();
        let map = self.world.resource::<BaseMap>();
        q.iter(&self.world)
            .find(|(id, ..)| id.0 == unit)
            .map(|(_, p, o, path, a, h, r)| is_stuck(map, p, o, path, a, h, r))
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

    /// Назначить цену тайлу палитры в предмете 0 — включает материал в тесте.
    /// Тесты, которым нужны разные типы, зовут `set_cost_items`.
    fn set_cost(&mut self, tile: i16, cost: i32) {
        self.set_cost_items(tile, &[(0, cost)]);
    }

    /// Цена набором: `(предмет, сколько)`. Нулевые позиции отбрасываются, как и
    /// в рулсете, где отсутствие записи значит «не нужен».
    fn set_cost_items(&mut self, tile: i16, cost: &[(usize, i32)]) {
        let cost: Vec<(usize, i32)> = cost.iter().copied().filter(|&(_, n)| n > 0).collect();
        self.tile_rule(tile, |r| r.cost = cost);
    }

    /// Сделать тайл складом с такой ёмкостью (0 = обычный пол).
    fn set_capacity(&mut self, tile: i16, capacity: i32) {
        self.tile_rule(tile, |r| r.capacity = capacity);
    }

    fn tile_rule(&mut self, tile: i16, edit: impl FnOnce(&mut TileRule)) {
        let mut rules = self.world.resource_mut::<TileRules>();
        let slot = tile as usize;
        if rules.0.len() <= slot {
            rules.0.resize(slot + 1, TileRule::default());
        }
        edit(&mut rules.0[slot]);
    }

    /// Всё добро мира: и в кучах, и в лапах, и уже завезённое на площадки.
    /// Величина сохраняется — на этом держатся проверки «ничего не пропало».
    fn scrap_total(&mut self) -> i32 {
        let mut piles = self.world.query::<&Stack>();
        let mut loads = self.world.query::<&Carrying>();
        let mut sites = self.world.query::<&Blueprint>();
        piles.iter(&self.world).map(|s| s.count).sum::<i32>()
            + loads.iter(&self.world).map(|c| c.count).sum::<i32>()
            + sites
                .iter(&self.world)
                .flat_map(|bp| bp.delivered.iter().map(|&(_, n)| n))
                .sum::<i32>()
    }

    /// Положить кучу предмета 0 на клетку.
    fn put_scrap(&mut self, x: i32, y: i32, count: i32) {
        self.put_item(x, y, 0, count);
    }

    fn put_item(&mut self, x: i32, y: i32, item: usize, count: i32) {
        self.world.spawn((Position { x, y }, Stack { item, count }));
    }

    /// Сколько всего добра лежит на клетке, всех типов разом.
    fn scrap_at(&mut self, x: i32, y: i32) -> i32 {
        let mut q = self.world.query::<(&Position, &Stack)>();
        q.iter(&self.world)
            .filter(|(p, _)| (p.x, p.y) == (x, y))
            .map(|(_, s)| s.count)
            .sum()
    }

    /// Сколько предмета данного типа лежит по всему миру, во всех кучах.
    fn item_total(&mut self, item: usize) -> i32 {
        let mut q = self.world.query::<&Stack>();
        q.iter(&self.world)
            .filter(|s| s.item == item)
            .map(|s| s.count)
            .sum()
    }

    /// Сколько предмета данного типа лежит на клетке.
    fn item_at(&mut self, x: i32, y: i32, item: usize) -> i32 {
        let mut q = self.world.query::<(&Position, &Stack)>();
        q.iter(&self.world)
            .filter(|(p, s)| (p.x, p.y) == (x, y) && s.item == item)
            .map(|(_, s)| s.count)
            .sum()
    }

    /// Весь ли лом лежит на проходимых клетках.
    fn scrap_is_on_floor(&mut self) -> bool {
        let mut q = self.world.query::<(&Position, &Stack)>();
        let map = self.world.resource::<BaseMap>();
        q.iter(&self.world).all(|(p, _)| map.walkable(p.x, p.y))
    }

    /// Весь ли лом убран на склад — то есть лежит на клетках с ёмкостью.
    fn scrap_is_in_storage(&mut self) -> bool {
        let mut q = self.world.query::<(&Position, &Stack)>();
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        q.iter(&self.world)
            .all(|(p, _)| rules.capacity_of(map.tile_at(p.x, p.y)) > 0)
    }

    /// Сколько кот несёт в лапах (любого типа).
    fn carrying_of(&mut self, unit: &str) -> i32 {
        let mut q = self.world.query::<(&UnitId, Option<&Carrying>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .and_then(|(_, c)| c.map(|c| c.count))
            .unwrap_or(0)
    }

    /// Что именно кот несёт; `None` — лапы пусты.
    fn carrying_item_of(&mut self, unit: &str) -> Option<usize> {
        let mut q = self.world.query::<(&UnitId, Option<&Carrying>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .and_then(|(_, c)| c.map(|c| c.item))
    }

    /// Сколько всего завезли на площадку; `None` — чертежа на клетке нет.
    fn delivered_at(&mut self, x: i32, y: i32) -> Option<i32> {
        let mut q = self.world.query::<&Blueprint>();
        q.iter(&self.world)
            .find(|bp| (bp.x, bp.y) == (x, y))
            .map(|bp| bp.delivered.iter().map(|&(_, n)| n).sum())
    }

    /// Сколько предмета данного типа завезли на площадку.
    fn delivered_item_at(&mut self, x: i32, y: i32, item: usize) -> i32 {
        let mut q = self.world.query::<&Blueprint>();
        q.iter(&self.world)
            .find(|bp| (bp.x, bp.y) == (x, y))
            .map_or(0, |bp| delivered_of(&bp.delivered, item))
    }

    fn has_haul(&mut self, unit: &str) -> bool {
        let mut q = self.world.query::<(&UnitId, Option<&Haul>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, h)| h.is_some())
            .unwrap_or(false)
    }

    fn entity_of(&mut self, unit: &str) -> Entity {
        let mut q = self.world.query::<(Entity, &UnitId)>();
        q.iter(&self.world)
            .find(|(_, id)| id.0 == unit)
            .map(|(e, _)| e)
            .expect("кот не найден")
    }

    /// Завести навык в мире теста: пороги уровней. Вернёт индекс домена.
    fn set_skill(&mut self, id: &str, levels: &[i32]) -> usize {
        let mut rules = self.world.resource_mut::<SkillRules>();
        rules.0.push(SkillRule {
            id: id.to_string(),
            levels: levels.to_vec(),
        });
        rules.0.len() - 1
    }

    /// Выдать коту опыт — стартовый навык без отработки тиков.
    fn set_xp(&mut self, unit: &str, skill: usize, xp: i32) {
        let cat = self.entity_of(unit);
        let mut skills = self
            .world
            .entity_mut(cat)
            .take::<Skills>()
            .unwrap_or_default();
        skills.add_xp(skill, xp, xp);
        self.world.entity_mut(cat).insert(skills);
    }

    fn xp_of(&mut self, unit: &str, skill: usize) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Skills>(cat).map_or(0, |s| s.xp_of(skill))
    }

    fn level_of(&mut self, unit: &str, skill: usize) -> i32 {
        let xp = self.xp_of(unit, skill);
        self.world.resource::<SkillRules>().level(skill, xp)
    }

    /// Ограничить лапы кота: сколько лома он берёт за ходку.
    fn set_carry(&mut self, unit: &str, cap: i32) {
        let cat = self.entity_of(unit);
        self.world.entity_mut(cat).insert(Carry(cap));
    }

    /// Предел лап кота; 0 — предела нет.
    fn carry_max_of(&mut self, unit: &str) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Carry>(cat).map_or(0, |c| c.0)
    }

    /// Индекс домена по имени — для тестов на боевом рулсете.
    fn skill_index(&self, id: &str) -> Option<usize> {
        self.world.resource::<SkillRules>().index_of(id)
    }

    /// Включить усталость: потолок бодрости, порог «пора спать» и скорость
    /// восстановления вне лежанки. Всем котам выдаётся полная бодрость.
    fn set_needs(&mut self, max: i32, tired: i32, floor: i32) {
        self.world.insert_resource(NeedRules { max, tired, floor });
        let mut q = self.world.query_filtered::<Entity, With<UnitId>>();
        for cat in q.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(cat).insert(Energy(max));
        }
    }

    /// Сделать тайл лежанкой: сколько бодрости он возвращает за тик.
    fn set_rest(&mut self, tile: i16, rate: i32) {
        self.tile_rule(tile, |r| r.rest = rate);
    }

    fn set_energy(&mut self, unit: &str, value: i32) {
        let cat = self.entity_of(unit);
        self.world.entity_mut(cat).insert(Energy(value));
    }

    fn energy_of(&mut self, unit: &str) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Energy>(cat).map_or(0, |e| e.0)
    }

    /// Кот занят отдыхом — спит или идёт к лежанке.
    fn is_resting(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Rest>(cat).is_some()
    }

    /// Лежанка, которую кот занял; `None` — не отдыхает или упал где стоял.
    fn rest_spot_of(&mut self, unit: &str) -> Option<(i32, i32)> {
        let cat = self.entity_of(unit);
        self.world.get::<Rest>(cat).and_then(|r| r.spot)
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
