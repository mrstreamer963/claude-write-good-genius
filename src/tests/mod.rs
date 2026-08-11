//! Тесты ядра. Гоняют ту же цепочку систем (`build_schedule`), что и боевая
//! симуляция, поэтому проверяют реальные взаимодействия, а не их копию.
//!
//! Баги здесь живут во взаимодействии систем и ECS-фильтров, а не в отдельных
//! функциях, поэтому механику покрываем прогоном полной цепочки (`tick_n`),
//! а не юнит-тестом функции. Мир собирается из ASCII-схем (`sim_from`), минуя
//! YAML; общие хелперы — в этом файле, сами тесты разложены по механикам.

mod captivity;
mod crafting;
mod crowd;
mod demolition;
mod factions;
mod fame;
mod food;
mod gear;
mod goals;
mod hauling;
mod health;
mod items;
mod jobs;
mod missions;
mod needs;
mod orders;
mod panel;
mod paths;
mod relay;
mod research;
mod save;
mod skills;
mod stats;
mod study;
mod terrain;
mod tidying;
mod timeline;
mod trade;
mod voids;

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::{BUILD_WORK, WORK_RATE};
use crate::map::BaseMap;
use crate::movement::{Busy, is_stuck};
use std::collections::BTreeMap;

use crate::ruleset::{ItemDef, TileDef};
use crate::schedule::build_schedule;
use crate::sim::Sim;
use crate::timeline::{ready_for, revealed};

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
    world.insert_resource(AutoRest(true));
    world.insert_resource(Trace::default());
    world.insert_resource(SkillRules::default());
    world.insert_resource(NeedRules::default());
    world.insert_resource(FoodRules::default());
    world.insert_resource(HealthRules::default());
    world.insert_resource(MissionRules::default());
    world.insert_resource(RecruitRules::default());
    world.insert_resource(ResearchRules::default());
    world.insert_resource(CraftRules::default());
    world.insert_resource(Techs::default());
    world.insert_resource(TimelineRules::default());
    world.insert_resource(Chronicle::default());
    world.insert_resource(Fame::default());
    // Фракций в схеме нет, как нет вылазок, навыков и голода: репутация — это
    // контент рулсета, и тесты чужих механик о ней не знают (§12.43). Заводит
    // её `set_faction`.
    world.insert_resource(FactionRules::default());
    world.insert_resource(Standing::default());
    // Денег у базы нет и торговать не с кем: внешний рынок — тоже контент
    // рулсета (§12.44). Заводят его `set_trade_post` и `set_prices`.
    world.insert_resource(Money::default());
    // Целей у схемы нет, как нет вылазок и рынка: цель — контент рулсета
    // (§12.58). Заводят её `set_goal` и `set_hidden_goal`. Три журнала при этом
    // заводятся пустыми всегда: они не контент, а память мира о совершённом, и
    // писать в них системы обязаны независимо от того, смотрит ли на них цель.
    world.insert_resource(GoalRules::default());
    world.insert_resource(Goals::default());
    world.insert_resource(Raids::default());
    world.insert_resource(Crafted::default());
    world.insert_resource(Earned::default());
    world.insert_resource(ItemRules::default());
    world.insert_resource(LoadoutRules::default());
    world.insert_resource(UnitRules::default());
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
            wake: 0,
            heal: 0,
            gate: false,
            teaches: String::new(),
            lab: false,
            shop: false,
            solid: false,
            trade: false,
            relay: false,
            comms: 0,
            tech: String::new(),
        }],
        items: Vec::new(),
        skills: Vec::new(),
        stats: Vec::new(),
        perks: Vec::new(),
        factions: Vec::new(),
        missions: Vec::new(),
        recruits: Vec::new(),
        research: Vec::new(),
        recipes: Vec::new(),
        timeline: Vec::new(),
        goals: Vec::new(),
        width,
        height,
        // Календаря в схеме нет: сутки — контент рулсета (§12.46), а тесты
        // механик о нём не знают. Ноль читается как «календаря нет».
        day: 0,
        // Схема собрана мимо YAML, рулсета за ней нет — отпечатывать нечего.
        // Тесты сохранения работают на боевом `core.yaml`, где он настоящий.
        ruleset: 0,
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
        // Задачи собраны вложенным кортежем: их ровно столько, сколько берёт
        // `Busy::of`, а плоский запрос упёрся бы в предел арности `QueryData`.
        let mut q = self.world.query::<(
            &UnitId,
            &Position,
            (
                Option<&Order>,
                Option<&Path>,
                Option<&Assignment>,
                Option<&Haul>,
                Option<&Rest>,
                Option<&Study>,
                Option<&Researching>,
                Option<&Crafting>,
                Option<&Equipping>,
                Option<&Eating>,
                Option<&Healing>,
                Option<&Treating>,
                Option<&Squad>,
                Option<&OnDuty>,
                Option<&Away>,
            ),
        )>();
        let map = self.world.resource::<BaseMap>();
        q.iter(&self.world)
            .find(|(id, ..)| id.0 == unit)
            .map(|(_, p, tasks)| {
                let (o, path, a, h, r, st, re, cr, eq, ea, he, tr, s, du, away) = tasks;
                // Дремота на `stuck` не влияет: она только подписывает занятие
                // и задачей не является (§12.52).
                is_stuck(
                    map,
                    p,
                    Busy::of(
                        o, path, a, h, r, st, re, cr, eq, ea, he, tr, s, du, away, false,
                    ),
                )
            })
            .expect("кот не найден")
    }

    /// Чем кот занят и в пути ли он — ровно то, что уходит в панель (§12.41).
    /// Тот же вложенный кортеж и по той же причине, что у `stuck_of`.
    fn job_of(&mut self, unit: &str) -> (&'static str, bool) {
        let mut q = self.world.query::<(
            &UnitId,
            &Position,
            (
                Option<&Order>,
                Option<&Path>,
                Option<&Assignment>,
                Option<&Haul>,
                Option<&Rest>,
                Option<&Study>,
                Option<&Researching>,
                Option<&Crafting>,
                Option<&Equipping>,
                Option<&Eating>,
                Option<&Healing>,
                Option<&Treating>,
                Option<&Squad>,
                Option<&OnDuty>,
                Option<&Away>,
            ),
        )>();
        let map = self.world.resource::<BaseMap>();
        let tiles = self.world.resource::<TileRules>();
        q.iter(&self.world)
            .find(|(id, ..)| id.0 == unit)
            .map(|(_, p, tasks)| {
                let (o, path, a, h, r, st, re, cr, eq, ea, he, tr, s, du, away) = tasks;
                // Дремота считается ровно так же, как в снапшоте: место для сна
                // под лапами (§12.52).
                let bed = tiles.rest_of(map.tile_at(p.x, p.y)) > 0;
                let busy = Busy::of(
                    o, path, a, h, r, st, re, cr, eq, ea, he, tr, s, du, away, bed,
                );
                (busy.job, busy.moving)
            })
            .expect("кот не найден")
    }

    /// `tried_version` приказа, если приказ ещё висит.
    /// Отметка провалившейся попытки: `None` — приказа нет либо последняя
    /// попытка удалась (маршрут проложен).
    fn order_tried_version(&mut self, unit: &str) -> Option<u64> {
        let mut q = self.world.query::<(&UnitId, Option<&Order>)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .and_then(|(_, o)| o.and_then(|o| o.tried_version))
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

    /// Индексы заставленных тайлов палитры — для контентных проверок (§12.35).
    fn solid_tiles(&self) -> Vec<i16> {
        let rules = self.world.resource::<TileRules>();
        (0..rules.0.len())
            .map(|i| i as i16)
            .filter(|&t| rules.is_solid(t))
            .collect()
    }

    /// Ёмкость тайла палитры.
    fn capacity_of(&self, tile: i16) -> i32 {
        self.world.resource::<TileRules>().capacity_of(tile)
    }

    /// Заставить тайл доверху: пройти можно, остаться нельзя (§12.35).
    fn set_solid(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.solid = on);
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
    /// Завести в палитре предметов `n` типов. Имущество раскладывается по
    /// палитре (§12.53), а в схеме `sim_from` она пуста — как пуст и рулсет:
    /// типы предметов это контент, и тесты чужих механик о них не знают.
    fn set_items(&mut self, n: usize) {
        while self.items.len() < n {
            let i = self.items.len();
            self.items.push(ItemDef {
                id: format!("item{i}"),
                label: String::new(),
                color: "#000000".to_string(),
                force: 0,
                mends: 0,
                nutrition: 0,
            });
        }
    }

    /// Имущество базы по типу так, как его видит шапка: `(склад, валяется,
    /// забронировано)` — ровно то, что уходит в снапшот (§12.53).
    fn stock_of(&mut self, item: usize) -> (i32, i32, i32) {
        self.set_items(item + 1);
        let stock = self.stock();
        let s = &stock[item];
        (s.stored, s.loose, s.booked)
    }

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

    /// Куда кот нацелился за грузом: клетка кучи и тип (§12.49). `None` — идёт
    /// с грузом, идёт на склад или не несёт вовсе.
    fn haul_aim_of(&mut self, unit: &str) -> Option<((i32, i32), usize)> {
        let mut q = self.world.query::<(&UnitId, &Haul)>();
        let aim = q
            .iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .and_then(|(_, h)| h.aim)?;
        let pos = self.world.get::<Position>(aim.pile)?;
        Some(((pos.x, pos.y), aim.item))
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
    /// Парта такому домену не учит (`taught: 0`), как и «Стройка» в рулсете, —
    /// обучение включают отдельно (`set_taught`).
    fn set_skill(&mut self, id: &str, levels: &[i32]) -> usize {
        let mut rules = self.world.resource_mut::<SkillRules>();
        rules.0.push(SkillRule {
            id: id.to_string(),
            levels: levels.to_vec(),
            taught: 0,
            // Предела по параметру у домена нет, пока его не задали явно:
            // тесты чужих механик о врождённом знать не должны (§12.42).
            stat: None,
            demands: Vec::new(),
        });
        rules.0.len() - 1
    }

    /// Ограничить домен врождённым параметром: каким и сколько его нужно на
    /// 2-й, 3-й, … уровень (§12.42). Первый уровень не закрыт никогда.
    fn set_demands(&mut self, skill: usize, stat: usize, demands: &[i32]) {
        let mut rules = self.world.resource_mut::<SkillRules>();
        if let Some(rule) = rules.0.get_mut(skill) {
            rule.stat = Some(stat);
            rule.demands = demands.to_vec();
        }
    }

    /// Выдать коту врождённый параметр. Палитры параметров в синтетическом мире
    /// нет — индекс задаёт сам тест, как и индекс домена.
    fn set_stat(&mut self, unit: &str, stat: usize, value: i32) {
        let cat = self.entity_of(unit);
        let mut stats = self
            .world
            .entity_mut(cat)
            .take::<Stats>()
            .unwrap_or_default();
        stats.set(stat, value);
        self.world.entity_mut(cat).insert(stats);
    }

    /// Значение врождённого параметра кота; 0 — параметра у него нет.
    fn stat_of(&mut self, unit: &str, stat: usize) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Stats>(cat).map_or(0, |s| s.value_of(stat))
    }

    /// Докуда кот вырастет в домене — предел по врождённому параметру (§12.42).
    fn level_cap_of(&mut self, unit: &str, skill: usize) -> i32 {
        let cat = self.entity_of(unit);
        let stats = self.world.get::<Stats>(cat);
        crate::skills::level_cap_of(self.world.resource::<SkillRules>(), stats, skill)
    }

    /// До какого уровня доводит домен парта; 0 — домену не учат (§12.18).
    fn set_taught(&mut self, skill: usize, taught: i32) {
        let mut rules = self.world.resource_mut::<SkillRules>();
        if let Some(rule) = rules.0.get_mut(skill) {
            rule.taught = taught;
        }
    }

    /// Сделать тайл партой: какому домену он учит.
    fn set_teaches(&mut self, tile: i16, skill: usize) {
        self.tile_rule(tile, |r| r.teaches = Some(skill));
    }

    /// Закрыть постройку тайла технологией (пустая строка — открыт всегда).
    fn set_tile_tech(&mut self, tile: i16, tech: &str) {
        self.tile_rule(tile, |r| r.tech = tech.to_string());
    }

    /// Сделать тайл лабораторией: в ней идёт работа над темой.
    fn set_lab(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.lab = on);
    }

    /// Сделать тайл мастерской: в ней идёт работа над заказом (§12.30).
    fn set_shop(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.shop = on);
    }

    /// Завести рецепт: объём работы на штуку, цена штуки, что выходит и какие
    /// технологии нужны. Вернёт его индекс — им же зовётся `start_craft`.
    fn set_recipe(
        &mut self,
        work: i32,
        cost: &[(usize, i32)],
        gives: &[(usize, i32)],
        requires: &[&str],
    ) -> usize {
        let mut rules = self.world.resource_mut::<CraftRules>();
        rules.0.push(CraftRule {
            work,
            cost: cost.to_vec(),
            gives: gives.to_vec(),
            requires: requires.iter().map(|t| t.to_string()).collect(),
        });
        rules.0.len() - 1
    }

    /// Сколько штук осталось в заказе; `None` — заказа нет.
    fn craft_left(&mut self) -> Option<i32> {
        let mut q = self.world.query::<&Craft>();
        q.iter(&self.world).next().map(|o| o.left)
    }

    /// Очки работы текущей штуки; `None` — заказа нет.
    fn craft_progress(&mut self) -> Option<i32> {
        let mut q = self.world.query::<&Craft>();
        q.iter(&self.world).next().map(|o| o.progress)
    }

    /// Кто стоит у верстака; `None` — исполнителя нет (или нет заказа).
    fn crafter(&mut self) -> Option<String> {
        let mut q = self.world.query::<&Craft>();
        let assignee = q.iter(&self.world).next().and_then(|o| o.assignee)?;
        self.world.get::<UnitId>(assignee).map(|u| u.0.clone())
    }

    fn is_crafting(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Crafting>(cat).is_some()
    }

    /// Сколько заказов размечено. Их столько, сколько мастерских (§12.55).
    fn orders_count(&mut self) -> usize {
        let mut q = self.world.query::<&Craft>();
        q.iter(&self.world).count()
    }

    /// Сколько штук осталось у заказа на этот рецепт; `None` — заказа нет.
    ///
    /// По рецепту, а не «первый попавшийся»: заказов теперь несколько, а порядок
    /// обхода ECS недетерминирован (§11). Двух заказов на один рецепт не бывает,
    /// поэтому рецепт их и различает.
    fn craft_left_of(&mut self, def: usize) -> Option<i32> {
        let mut q = self.world.query::<&Craft>();
        q.iter(&self.world).find(|o| o.def == def).map(|o| o.left)
    }

    /// Станок, за которым идёт этот заказ; `None` — исполнителя ещё нет.
    fn craft_spot_of(&mut self, def: usize) -> Option<(i32, i32)> {
        let mut q = self.world.query::<&Craft>();
        q.iter(&self.world)
            .find(|o| o.def == def)
            .and_then(|o| o.spot)
    }

    /// Кто стоит у верстака по этому рецепту; `None` — исполнителя нет.
    fn crafter_of(&mut self, def: usize) -> Option<String> {
        let mut q = self.world.query::<&Craft>();
        let assignee = q.iter(&self.world).find(|o| o.def == def)?.assignee?;
        self.world.get::<UnitId>(assignee).map(|u| u.0.clone())
    }

    /// Завести тему исследования: допуск по «Науке», объём работы, цена и
    /// нужные технологии. Вернёт её индекс — им же зовётся `start_research`.
    fn set_topic(
        &mut self,
        id: &str,
        level: i32,
        work: i32,
        cost: &[(usize, i32)],
        requires: &[&str],
    ) -> usize {
        let mut rules = self.world.resource_mut::<ResearchRules>();
        rules.0.push(ResearchRule {
            id: id.to_string(),
            level,
            work,
            cost: cost.to_vec(),
            requires: requires.iter().map(|t| t.to_string()).collect(),
        });
        rules.0.len() - 1
    }

    /// Выключить таймлайн в мире боевого рулсета.
    ///
    /// Мир по расписанию — единственное, что происходит без участия игрока
    /// (§12.28), и на длинных прогонах он меняет и запасы, и известность.
    /// Тестам чужих механик это шум, ровно как пустые `MissionRules` в
    /// синтетической схеме; сами события покрывает `tests/timeline.rs`.
    fn without_timeline(&mut self) {
        self.world.resource_mut::<TimelineRules>().0.clear();
    }

    /// Завести событие таймлайна: дата, требуемые технологии, подарок за
    /// готовность и плата за неготовность. Вернёт его индекс.
    fn set_event(
        &mut self,
        at: u64,
        requires: &[&str],
        gift: &[(usize, i32)],
        fame: i32,
        toll: i32,
    ) -> usize {
        let mut rules = self.world.resource_mut::<TimelineRules>();
        rules.0.push(EventRule {
            at,
            reveal: 0,
            requires: requires.iter().map(|t| t.to_string()).collect(),
            gift: gift.to_vec(),
            fame,
            toll,
        });
        rules.0.len() - 1
    }

    /// За сколько тиков до срока у события проступают детали.
    fn set_reveal(&mut self, event: usize, reveal: u64) {
        let mut rules = self.world.resource_mut::<TimelineRules>();
        if let Some(rule) = rules.0.get_mut(event) {
            rule.reveal = reveal;
        }
    }

    /// Выдать технологию напрямую — как `set_fame` для известности.
    fn set_tech(&mut self, tech: &str) {
        self.world.resource_mut::<Techs>().0.push(tech.to_string());
    }

    /// Проступили ли детали события — то же, что уходит в снапшот (§12.28).
    fn note_revealed(&self, event: usize) -> bool {
        let tick = self.world.resource::<SimTime>().tick;
        self.world
            .resource::<TimelineRules>()
            .0
            .get(event)
            .is_some_and(|r| revealed(r, tick))
    }

    /// Требования события — **как их видит игрок**: до раскрытия пусто.
    fn note_requires(&self, event: usize) -> Vec<String> {
        let rules = self.world.resource::<TimelineRules>();
        match rules.0.get(event) {
            Some(rule) if self.note_revealed(event) => rule.requires.clone(),
            _ => Vec::new(),
        }
    }

    /// Успевает ли база к сроку — тоже деталь, и до раскрытия её не видно.
    fn note_ready(&self, event: usize) -> bool {
        let rules = self.world.resource::<TimelineRules>();
        match rules.0.get(event) {
            Some(rule) if self.note_revealed(event) => {
                ready_for(rule, self.world.resource::<Techs>())
            }
            _ => false,
        }
    }

    /// Случилось ли событие и была ли база к нему готова; `None` — ещё нет.
    fn happened(&self, event: usize) -> Option<bool> {
        self.world
            .resource::<Chronicle>()
            .happened(event)
            .map(|h| h.ready)
    }

    /// Изучена ли технология.
    fn knows_tech(&self, tech: &str) -> bool {
        self.world.resource::<Techs>().knows(tech)
    }

    /// Очки работы, набитые по теме; `None` — темы нет.
    fn research_progress(&mut self) -> Option<i32> {
        let mut q = self.world.query::<&Research>();
        q.iter(&self.world).next().map(|t| t.progress)
    }

    /// Кот занят темой — идёт в лабораторию или уже работает.
    fn is_researching(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Researching>(cat).is_some()
    }

    /// Кто сейчас за темой; `None` — исполнителя нет или темы нет.
    fn researcher(&mut self) -> Option<String> {
        let mut q = self.world.query::<&Research>();
        let assignee = q.iter(&self.world).next().and_then(|t| t.assignee)?;
        self.world.get::<UnitId>(assignee).map(|u| u.0.clone())
    }

    /// Кот учится — сидит за партой или идёт к ней.
    fn is_studying(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Study>(cat).is_some()
    }

    /// Парта, которую занял ученик; `None` — кот не учится.
    fn desk_of(&mut self, unit: &str) -> Option<(i32, i32)> {
        let cat = self.entity_of(unit);
        self.world.get::<Study>(cat).map(|s| s.spot)
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
    /// Включить усталость. Критического порога тут нет намеренно: он меняет
    /// поведение занятых котов, и тесты чужих механик о нём знать не должны —
    /// его включает `set_critical` (§12.33).
    fn set_needs(&mut self, max: i32, tired: i32, floor: i32) {
        self.world.insert_resource(NeedRules {
            max,
            tired,
            critical: 0,
            floor,
            // Потолков сна тут нет: сон везде до полной, как было до §12.52, —
            // их включают тесты дремоты через `set_wake` и `set_floor_wake`.
            floor_wake: 0,
        });
        let mut q = self.world.query_filtered::<Entity, With<UnitId>>();
        for cat in q.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(cat).insert(Energy(max));
        }
    }

    /// Порог, ниже которого кот бросает начатое и уходит спать; ноль выключает.
    fn set_critical(&mut self, value: i32) {
        self.world.resource_mut::<NeedRules>().critical = value;
    }

    /// Пороги усталости из правил: `(tired, critical)`.
    fn thresholds(&self) -> (i32, i32) {
        let needs = self.world.resource::<NeedRules>();
        (needs.tired, needs.critical)
    }

    /// Сделать тайл лежанкой: сколько бодрости он возвращает за тик.
    fn set_rest(&mut self, tile: i16, rate: i32) {
        self.tile_rule(tile, |r| r.rest = rate);
    }

    /// Потолок сна на тайле: докуда здесь высыпаются (§12.52). Ноль — до полной.
    fn set_wake(&mut self, tile: i16, ceiling: i32) {
        self.tile_rule(tile, |r| r.wake = ceiling);
    }

    /// Потолок сна там, где лежанки нет; ноль — до полной (§12.52).
    fn set_floor_wake(&mut self, ceiling: i32) {
        self.world.resource_mut::<NeedRules>().floor_wake = ceiling;
    }

    /// Потолки сна из правил: `(пол, максимум)` — для тестов на боевом рулсете.
    fn ceilings(&self) -> (i32, i32) {
        let needs = self.world.resource::<NeedRules>();
        (needs.floor_wake, needs.max)
    }

    /// Место для сна как его видит рулсет: `(ставка, потолок)` у тайла (§12.52).
    fn bed_of(&self, tile: i16) -> (i32, i32) {
        let rules = self.world.resource::<TileRules>();
        (rules.rest_of(tile), rules.wake_of(tile))
    }

    /// Индекс тайла по `id` — для тестов на боевом рулсете, где палитру задаёт
    /// контент, а не схема.
    fn tile_index(&self, id: &str) -> Option<i16> {
        self.palette
            .iter()
            .position(|t| t.id == id)
            .map(|i| i as i16)
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

    /// Включить голод: потолок сытости, порог «пора есть» и во сколько раз
    /// быстрее горит бодрость на пустой желудок (§12.36). Всем котам выдаётся
    /// полная сытость. Отдельно от `set_needs`: голод и усталость включаются
    /// порознь, иначе тесты усталости начали бы зависеть от наличия еды.
    fn set_food(&mut self, max: i32, hungry: i32, starve: i32) {
        self.world.insert_resource(FoodRules {
            max,
            hungry,
            starve,
        });
        let mut q = self.world.query_filtered::<Entity, With<UnitId>>();
        for cat in q.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(cat).insert(Fed(max));
        }
    }

    /// Сделать предмет едой: сколько сытости даёт одна штука. В схеме `sim_from`
    /// предметы несъедобны, как тайл бесплатен, — еда это контент рулсета.
    fn set_nutrition(&mut self, item: usize, nutrition: i32) {
        let mut rules = self.world.resource_mut::<ItemRules>();
        if rules.0.len() <= item {
            rules.0.resize(item + 1, ItemRule::default());
        }
        rules.0[item].nutrition = nutrition;
    }

    fn set_fed(&mut self, unit: &str, value: i32) {
        let cat = self.entity_of(unit);
        self.world.entity_mut(cat).insert(Fed(value));
    }

    fn fed_of(&mut self, unit: &str) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Fed>(cat).map_or(0, |f| f.0)
    }

    /// Кот идёт к куче с едой (§12.36).
    fn is_eating(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Eating>(cat).is_some()
    }

    /// Включить ранения: потолок здоровья, порог выбывания и заживление вне
    /// лазарета (§12.37). Всем котам выдаётся полное здоровье. Отдельно от
    /// `set_needs` и `set_food`: три шкалы включаются порознь, иначе тесты
    /// усталости начали бы зависеть от наличия лазарета.
    fn set_health_rules(&mut self, max: i32, hurt: i32, mend: i32) {
        self.world.insert_resource(HealthRules { max, hurt, mend });
        let mut q = self.world.query_filtered::<Entity, With<UnitId>>();
        for cat in q.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(cat).insert(Health(max));
        }
    }

    fn set_health(&mut self, unit: &str, value: i32) {
        let cat = self.entity_of(unit);
        self.world.entity_mut(cat).insert(Health(value));
    }

    fn health_of(&mut self, unit: &str) -> i32 {
        let cat = self.entity_of(unit);
        self.world.get::<Health>(cat).map_or(0, |h| h.0)
    }

    /// Сделать тайл койкой лазарета: сколько здоровья он возвращает за тик.
    fn set_heal(&mut self, tile: i16, rate: i32) {
        self.tile_rule(tile, |r| r.heal = rate);
    }

    /// Кот выбыл — лежит или идёт в лазарет.
    fn is_healing(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Healing>(cat).is_some()
    }

    /// Койка, которую занял раненый; `None` — лёг где стоял или здоров.
    fn ward_of(&mut self, unit: &str) -> Option<(i32, i32)> {
        let cat = self.entity_of(unit);
        self.world.get::<Healing>(cat).and_then(|h| h.spot)
    }

    /// Кто лечит этого кота; `None` — медика нет или кот здоров.
    fn medic_of(&mut self, unit: &str) -> Option<String> {
        let cat = self.entity_of(unit);
        let medic = self.world.get::<Healing>(cat)?.medic?;
        self.world.get::<UnitId>(medic).map(|u| u.0.clone())
    }

    /// Кот лечит кого-то — идёт к раненому или уже стоит у койки.
    fn is_treating(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Treating>(cat).is_some()
    }

    /// Дописать миссии урон: во сколько здоровья обходится полный провал.
    fn set_mission_harm(&mut self, mission: usize, harm: i32) {
        let mut rules = self.world.resource_mut::<MissionRules>();
        if let Some(rule) = rules.0.get_mut(mission) {
            rule.harm = harm;
        }
    }

    /// Сделать тайл шлюзом: отсюда отряд уходит на миссию и сюда возвращается.
    fn set_gate(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.gate = on);
    }

    /// Сделать тайл узлом связи: каждая такая клетка — слот вылазки (§12.59).
    ///
    /// В схеме `sim_from` узлов нет, как нет ни шлюза, ни мастерской: ноль узлов
    /// значит ноль вылазок, и `launch` отклонит заявку. Тесты вылазок вешают
    /// узел **на ту же клетку, что и шлюз**, — она одна, значит слот один, то
    /// есть ровно то поведение, которое было до §12.59.
    fn set_relay(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.relay = on);
    }

    /// Сила связи у тайла — то же число, которое читает `duty_gain`.
    fn comms_of_tile(&self, tile: i16) -> i32 {
        self.world.resource::<TileRules>().comms_of(tile)
    }

    /// Сколько силы даёт полное дежурство на этом тайле (§12.60). В схеме
    /// `sim_from` это ноль, как ноль у навыков и рынка: узел даёт слот, но
    /// дежурить на нём незачем, и раздатчик к нему никого не зовёт.
    fn set_comms(&mut self, tile: i16, power: i32) {
        self.tile_rule(tile, |r| r.comms = power);
    }

    /// Выключить связь на боевом рулсете: механика в этом тесте — шум.
    ///
    /// Тот же приём, что `without_timeline` (§12.28): мир по расписанию и
    /// дежурный у рации мешают мерить чужую механику, и глушить их надо явно, а
    /// не подгонять под них контент.
    fn without_comms(&mut self) {
        let mut rules = self.world.resource_mut::<TileRules>();
        for rule in rules.0.iter_mut() {
            rule.comms = 0;
        }
    }

    /// На каком узле дежурит кот, если дежурит.
    fn duty_of(&mut self, unit: &str) -> Option<(i32, i32)> {
        let mut q = self.world.query::<(&UnitId, &OnDuty)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, d)| d.spot)
    }

    /// К какому узлу кот приписан игроком (§12.60).
    fn post_of(&mut self, unit: &str) -> Option<(i32, i32)> {
        let mut q = self.world.query::<(&UnitId, &Posted)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, p)| p.spot)
    }

    /// В отряде какого узла кот числится (§12.61).
    fn enlisted_at(&mut self, unit: &str) -> Option<(i32, i32)> {
        let mut q = self.world.query::<(&UnitId, &Enlisted)>();
        q.iter(&self.world)
            .find(|(id, _)| id.0 == unit)
            .map(|(_, c)| c.spot)
    }

    /// Постоянный состав отряда этого узла — ровно то, что возьмёт
    /// `launch_node` и что увидит в панели игрок (§12.61).
    fn roster_at(&mut self, x: i32, y: i32) -> Vec<String> {
        let mut q = self.world.query::<(&UnitId, &Enlisted)>();
        let mut crew: Vec<String> = q
            .iter(&self.world)
            .filter(|(_, c)| c.spot == (x, y))
            .map(|(id, _)| id.0.clone())
            .collect();
        crew.sort_unstable();
        crew
    }

    /// Набежавшая связь у вылазки по этому заказу (§12.60).
    fn covered_of(&mut self, def: usize) -> Option<i32> {
        let mut q = self.world.query::<&Mission>();
        q.iter(&self.world)
            .find(|m| m.def == def)
            .map(|m| m.covered)
    }

    /// Сколько узлов связи построено — потолок одновременных вылазок (§12.59).
    fn relay_count(&mut self) -> usize {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .filter(|&(x, y)| rules.is_relay_node(map.tile_at(x, y)))
            .count()
    }

    /// Сколько вылазок идёт прямо сейчас — занятых слотов (§12.59).
    fn raid_count(&mut self) -> usize {
        let mut q = self.world.query_filtered::<Entity, With<Mission>>();
        q.iter(&self.world).count()
    }

    /// Идёт ли вылазка по этому заказу и сколько ей осталось.
    fn raid_left(&mut self, def: usize) -> Option<i32> {
        let mut q = self.world.query::<&Mission>();
        q.iter(&self.world).find(|m| m.def == def).map(|m| m.left)
    }

    /// Сделать предмет снаряжением: сколько силы он даёт отряду (§12.29).
    /// В схеме `sim_from` предметы бессильны, как тайл бесплатен: снаряжение —
    /// контент рулсета, и тесты чужих механик о нём не знают.
    fn set_force(&mut self, item: usize, force: i32) {
        let mut rules = self.world.resource_mut::<ItemRules>();
        if rules.0.len() <= item {
            rules.0.resize(item + 1, ItemRule::default());
        }
        rules.0[item].force = force;
    }

    /// Сделать предмет аптечкой: сколько заживления за тик он добавляет
    /// израсходованным (§12.47). Ноль — обычный предмет.
    fn set_mends(&mut self, item: usize, mends: i32) {
        let mut rules = self.world.resource_mut::<ItemRules>();
        if rules.0.len() <= item {
            rules.0.resize(item + 1, ItemRule::default());
        }
        rules.0[item].mends = mends;
    }

    /// Задать шаблон снаряжения: что коты носят. Один на всех (§12.29).
    fn set_loadout(&mut self, items: &[usize]) {
        self.world.resource_mut::<LoadoutRules>().0 = items.to_vec();
    }

    /// Убрать кучу с клетки целиком — как если бы её унесли, пока кот шёл.
    fn take_item(&mut self, x: i32, y: i32, item: usize) {
        let mut q = self.world.query::<(Entity, &Position, &Stack)>();
        let piles: Vec<Entity> = q
            .iter(&self.world)
            .filter(|(_, p, s)| (p.x, p.y) == (x, y) && s.item == item)
            .map(|(e, ..)| e)
            .collect();
        for pile in piles {
            self.world.entity_mut(pile).despawn();
        }
    }

    /// Кот идёт за вещью из шаблона (§12.34).
    fn is_equipping(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Equipping>(cat).is_some()
    }

    /// Что надето на коте, в порядке шаблона.
    fn gear_of(&mut self, unit: &str) -> Vec<usize> {
        let cat = self.entity_of(unit);
        self.world
            .get::<Gear>(cat)
            .map(|g| g.0.clone())
            .unwrap_or_default()
    }

    /// Завести миссию в мире теста: размер отряда, длительность и добыча.
    /// Она **безопасна и бесплатна** (`danger`/`toll` в нулях), как бесплатен
    /// тайл в `sim_from`: тесты сбора отряда об исходе ничего не знают.
    /// Вернёт её индекс — им же зовётся `launch`.
    fn set_mission(&mut self, squad: usize, ticks: i32, loot: &[(usize, i32)]) -> usize {
        self.set_risky_mission(squad, ticks, 0, 0, loot)
    }

    /// Миссия со сложностью и платой — для тестов исхода (§12.23).
    fn set_risky_mission(
        &mut self,
        squad: usize,
        ticks: i32,
        danger: i32,
        toll: i32,
        loot: &[(usize, i32)],
    ) -> usize {
        let mut rules = self.world.resource_mut::<MissionRules>();
        rules.0.push(MissionRule {
            squad,
            ticks,
            danger,
            toll,
            loot: loot.to_vec(),
            ..MissionRule::default()
        });
        rules.0.len() - 1
    }

    /// Вылазка за своим (§12.40): за добычей не ходит, зато возвращает пленных.
    /// В схеме `sim_from` такой миссии нет, как нет и вылазок вообще, — значит
    /// и плена в чужих тестах не случается.
    fn set_rescue_mission(&mut self, squad: usize, ticks: i32, danger: i32) -> usize {
        let m = self.set_risky_mission(squad, ticks, danger, 0, &[]);
        let mut rules = self.world.resource_mut::<MissionRules>();
        rules.0[m].rescue = true;
        m
    }

    /// Кот остался в плену: провал не смог унести его домой (§12.40).
    fn is_captive(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Captive>(cat).is_some()
    }

    /// Вылазки за своим из рулсета: индекс, отряд, сложность, порог известности.
    fn rescue_missions(&self) -> Vec<(usize, usize, i32, i32)> {
        self.world
            .resource::<MissionRules>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, r)| r.rescue)
            .map(|(i, r)| (i, r.squad, r.danger, r.requires))
            .collect()
    }

    /// Сколько всего котов у базы — включая тех, кого на базе сейчас нет.
    fn unit_count(&mut self) -> usize {
        let mut q = self.world.query::<&UnitId>();
        q.iter(&self.world).count()
    }

    /// Дописать миссии известность: сколько даёт и сколько требует (§12.24).
    fn set_mission_fame(&mut self, mission: usize, gives: i32, requires: i32) {
        let mut rules = self.world.resource_mut::<MissionRules>();
        if let Some(rule) = rules.0.get_mut(mission) {
            rule.fame = gives;
            rule.requires = requires;
        }
    }

    fn fame(&self) -> i32 {
        self.world.resource::<Fame>().0
    }

    fn money(&self) -> i32 {
        self.world.resource::<Money>().0
    }

    fn set_money(&mut self, value: i32) {
        self.world.resource_mut::<Money>().0 = value;
    }

    /// Сделать тайл торговым постом: без него база не торгует (§12.44).
    fn set_trade_post(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.trade = on);
    }

    /// Завести фракцию в мире теста. Вернёт её индекс — им же адресуются
    /// `patron`, `against` и `needs` (§12.43).
    fn set_faction(&mut self, span: i32) -> usize {
        let mut rules = self.world.resource_mut::<FactionRules>();
        rules.0.push(FactionRule {
            span,
            ..FactionRule::default()
        });
        rules.0.len() - 1
    }

    /// Приписать вылазке заказчика и пострадавшего (§12.43). `None` — ничей:
    /// так выглядит нейтральная вылазка, и в схеме `sim_from` все они такие.
    fn set_mission_factions(
        &mut self,
        mission: usize,
        patron: Option<usize>,
        against: Option<usize>,
        standing: i32,
    ) {
        let mut rules = self.world.resource_mut::<MissionRules>();
        if let Some(rule) = rules.0.get_mut(mission) {
            rule.patron = patron;
            rule.against = against;
            rule.standing = standing;
        }
    }

    /// Открыть фракции внешний рынок (§12.44): задержка поставки, наценка на
    /// покупку, вес репутации в курсе и длина фазы расписания.
    fn set_market(&mut self, faction: usize, lead: i32, spread: i32, favor: i32, period: u64) {
        let mut rules = self.world.resource_mut::<FactionRules>();
        if let Some(rule) = rules.0.get_mut(faction) {
            rule.lead = lead;
            rule.spread = spread;
            rule.favor = favor;
            rule.period = period;
        }
    }

    /// Расписание базового курса по предмету: фазы идут по кругу.
    fn set_prices(&mut self, faction: usize, item: usize, phases: &[i32]) {
        let mut rules = self.world.resource_mut::<FactionRules>();
        if let Some(rule) = rules.0.get_mut(faction) {
            rule.prices.retain(|&(i, _)| i != item);
            rule.prices.push((item, phases.to_vec()));
            rule.prices.sort_unstable_by_key(|&(i, _)| i);
        }
    }

    /// Сколько предметов в палитре рулсета.
    fn item_count(&self) -> usize {
        self.world.resource::<ItemRules>().0.len()
    }

    /// Длина фазы расписания — общая у всех фракций боевого рулсета; берём
    /// первую ненулевую, чтобы шагать по фазам в тестах.
    fn market_period(&self) -> u64 {
        self.world
            .resource::<FactionRules>()
            .0
            .iter()
            .map(|f| f.period)
            .find(|&p| p > 0)
            .unwrap_or(1)
    }

    /// Задержка поставки у фракции.
    fn market_lead(&self, faction: usize) -> i32 {
        self.world
            .resource::<FactionRules>()
            .0
            .get(faction)
            .map_or(0, |f| f.lead)
    }

    /// Индексы торговых постов палитры — для контентных проверок (§12.44).
    fn trade_post_tiles(&self) -> Vec<i16> {
        let rules = self.world.resource::<TileRules>();
        (0..rules.0.len())
            .map(|i| i as i16)
            .filter(|&t| rules.is_trade_post(t))
            .collect()
    }

    /// Технология, запирающая тайл; `None` — доступен сразу.
    fn tile_tech(&self, tile: i16) -> Option<String> {
        self.world
            .resource::<TileRules>()
            .tech_of(tile)
            .map(|t| t.to_string())
    }

    /// Перемотать часы: расписание курсов — чистая функция тика, поэтому фазу
    /// в тесте можно выбрать, а не отсчитывать её тиками (§12.44).
    fn set_tick(&mut self, tick: u64) {
        self.world.resource_mut::<SimTime>().tick = tick;
    }

    /// Сделка в работе: `(покупаем ли, штук, курс, тиков до поставки, донесено)`.
    fn deal_of(&mut self) -> Option<(bool, i32, i32, i32, i32)> {
        let mut q = self.world.query::<&Deal>();
        q.iter(&self.world)
            .next()
            .map(|d| (d.buying, d.count, d.unit, d.left, d.delivered))
    }

    /// Курс так, как его видит и панель, и фасад, — одно выражение (§12.44).
    fn quote(&self, faction: usize, item: usize, buying: bool) -> Option<i32> {
        crate::trade::quote(
            self.world.resource::<FactionRules>(),
            self.world.resource::<Standing>(),
            faction,
            item,
            self.world.resource::<SimTime>().tick,
            buying,
        )
    }

    /// Пол доверия у вылазки: ниже него заказчик её не даёт (§12.43).
    fn set_mission_needs(&mut self, mission: usize, needs: &[(usize, i32)]) {
        let mut rules = self.world.resource_mut::<MissionRules>();
        if let Some(rule) = rules.0.get_mut(mission) {
            rule.needs = needs.to_vec();
        }
    }

    /// Пределы репутации по фракциям рулсета, в порядке палитры.
    fn faction_spans(&self) -> Vec<i32> {
        self.world
            .resource::<FactionRules>()
            .0
            .iter()
            .map(|f| f.span)
            .collect()
    }

    /// Стороны и ворота вылазок рулсета: `(заказчик, пострадавший, репутация за
    /// успех, пол доверия, вылазка ли за своим, порог известности)`.
    #[allow(clippy::type_complexity)]
    fn mission_sides(
        &self,
    ) -> Vec<(
        Option<usize>,
        Option<usize>,
        i32,
        Vec<(usize, i32)>,
        bool,
        i32,
    )> {
        self.world
            .resource::<MissionRules>()
            .0
            .iter()
            .map(|r| {
                (
                    r.patron,
                    r.against,
                    r.standing,
                    r.needs.clone(),
                    r.rescue,
                    r.requires,
                )
            })
            .collect()
    }

    /// Пол доверия у кандидатов рулсета, в порядке палитры.
    fn recruit_needs(&self) -> Vec<Vec<(usize, i32)>> {
        self.world
            .resource::<RecruitRules>()
            .0
            .iter()
            .map(|r| r.needs.clone())
            .collect()
    }

    /// Репутация базы у фракции: копится не она, а сторона.
    fn standing(&self, faction: usize) -> i32 {
        self.world.resource::<Standing>().value_of(faction)
    }

    fn set_standing(&mut self, faction: usize, value: i32) {
        let span = self.world.resource::<FactionRules>().span_of(faction);
        let now = self.standing(faction);
        self.world
            .resource_mut::<Standing>()
            .add(faction, value - now, span);
    }

    fn set_fame(&mut self, value: i32) {
        self.world.resource_mut::<Fame>().0 = value;
    }

    /// Завести кандидата на найм. Вернёт его индекс — им же зовётся `hire`.
    fn set_recruit(
        &mut self,
        id: &str,
        requires: i32,
        cost: &[(usize, i32)],
        skills: &[(usize, i32)],
    ) -> usize {
        let mut rules = self.world.resource_mut::<RecruitRules>();
        rules.0.push(RecruitRule {
            id: id.to_string(),
            sprite: "cat".to_string(),
            requires,
            cost: cost.to_vec(),
            skills: skills.to_vec(),
            // Врождённого у кандидата в синтетическом мире нет: параметры
            // включают тесты §12.42 своим `set_recruit_stats`.
            stats: Vec::new(),
            perks: Vec::new(),
            // Фракций в синтетическом мире нет — пол доверия ставит
            // `set_recruit_needs` из тестов §12.43.
            needs: Vec::new(),
        });
        rules.0.len() - 1
    }

    /// Пол доверия у кандидата: `(фракция, сколько)` (§12.43).
    fn set_recruit_needs(&mut self, recruit: usize, needs: &[(usize, i32)]) {
        let mut rules = self.world.resource_mut::<RecruitRules>();
        if let Some(rule) = rules.0.get_mut(recruit) {
            rule.needs = needs.to_vec();
        }
    }

    /// Выдать кандидату врождённые параметры: `(параметр, сколько)` (§12.42).
    fn set_recruit_stats(&mut self, recruit: usize, stats: &[(usize, i32)]) {
        let mut rules = self.world.resource_mut::<RecruitRules>();
        if let Some(rule) = rules.0.get_mut(recruit) {
            rule.stats = stats.to_vec();
        }
    }

    /// Есть ли такой кот на базе.
    fn has_unit(&mut self, unit: &str) -> bool {
        let mut q = self.world.query::<&UnitId>();
        q.iter(&self.world).any(|u| u.0 == unit)
    }

    /// Кот записан в отряд — идёт к шлюзу, ждёт на нём или уже ушёл.
    fn in_squad(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Squad>(cat).is_some()
    }

    /// Кота нет на базе: отряд ушёл на вылазку.
    fn is_away(&mut self, unit: &str) -> bool {
        let cat = self.entity_of(unit);
        self.world.get::<Away>(cat).is_some()
    }

    /// Тиков до возвращения отряда; `None` — миссии нет.
    fn mission_left(&mut self) -> Option<i32> {
        let mut q = self.world.query::<&Mission>();
        q.iter(&self.world).next().map(|m| m.left)
    }

    /// Выбранный шлюз миссии; `None` — миссии нет или отряд не начали набирать.
    fn mission_gate(&mut self) -> Option<(i32, i32)> {
        let mut q = self.world.query::<&Mission>();
        q.iter(&self.world).next().and_then(|m| m.gate)
    }

    /// Узел, который держит слот вылазки по этому заказу (§12.59, §12.61).
    /// Адресуется по заказу, а не по номеру: порядок обхода ECS недетерминирован.
    fn mission_node(&mut self, def: usize) -> Option<(i32, i32)> {
        let mut q = self.world.query::<&Mission>();
        q.iter(&self.world).find(|m| m.def == def).map(|m| m.node)
    }

    /// Завести цель партии; вернёт её индекс (§12.58).
    fn set_goal(&mut self, test: GoalTest) -> usize {
        let mut rules = self.world.resource_mut::<GoalRules>();
        rules.0.push(GoalRule {
            test,
            hidden: false,
        });
        rules.0.len() - 1
    }

    /// Скрытая цель: в счёт не идёт и наружу не уходит, пока не взята.
    fn set_hidden_goal(&mut self, test: GoalTest) -> usize {
        let mut rules = self.world.resource_mut::<GoalRules>();
        rules.0.push(GoalRule { test, hidden: true });
        rules.0.len() - 1
    }

    /// Взята ли цель.
    fn goal_taken(&self, def: usize) -> bool {
        self.world.resource::<Goals>().taken(def).is_some()
    }

    /// Тик, на котором цель взяли; `None` — ещё не взята.
    fn goal_at(&self, def: usize) -> Option<u64> {
        self.world.resource::<Goals>().taken(def).map(|t| t.at)
    }

    /// Сколько целей взято всего, считая скрытые.
    fn goals_taken(&self) -> usize {
        self.world.resource::<Goals>().0.len()
    }

    /// Счётчик цели **так, как его видит панель** — тем же путём, каким он
    /// уходит в снапшот. Отдельной арифметики в тесте нет намеренно: она бы
    /// проверяла себя, а не то, что показано игроку (§12.58).
    fn goal_progress(&mut self, def: usize) -> Option<(i32, i32)> {
        self.goals()
            .iter()
            .find(|g| g.def == def)
            .map(|g| (g.have, g.need))
    }

    /// Уходит ли цель наружу вообще: скрытая и невзятая — нет.
    fn goal_is_visible(&mut self, def: usize) -> bool {
        self.goals().iter().any(|g| g.def == def)
    }

    /// Сколько целей идёт в счёт «Цели 3/7».
    fn goals_required(&self) -> usize {
        self.world.resource::<GoalRules>().required()
    }

    /// Правила целей так, как их собрал рулсет: `(скрытая, условие)`.
    /// Нужны сторожу на боевом контенте (§12.58).
    fn goal_specs(&self) -> Vec<(bool, GoalTest)> {
        self.world
            .resource::<GoalRules>()
            .0
            .iter()
            .map(|g| (g.hidden, g.test.clone()))
            .collect()
    }

    /// Сколько заработано продажей за партию — журнал, а не текущие деньги.
    fn earned(&self) -> i32 {
        self.world.resource::<Earned>().0
    }

    /// Журнал успешных вылазок: индексы миссий.
    fn raids_done(&self) -> Vec<usize> {
        self.world.resource::<Raids>().0.clone()
    }

    /// Журнал производства: индексы рецептов, по которым сделана хоть штука.
    fn crafted_done(&self) -> Vec<usize> {
        self.world.resource::<Crafted>().0.clone()
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
