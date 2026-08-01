//! Тесты ядра. Гоняют ту же цепочку систем (`build_schedule`), что и боевая
//! симуляция, поэтому проверяют реальные взаимодействия, а не их копию.
//!
//! Баги здесь живут во взаимодействии систем и ECS-фильтров, а не в отдельных
//! функциях, поэтому механику покрываем прогоном полной цепочки (`tick_n`),
//! а не юнит-тестом функции. Мир собирается из ASCII-схем (`sim_from`), минуя
//! YAML; общие хелперы — в этом файле, сами тесты разложены по механикам.

mod crafting;
mod crowd;
mod demolition;
mod fame;
mod food;
mod gear;
mod hauling;
mod items;
mod jobs;
mod missions;
mod needs;
mod orders;
mod paths;
mod research;
mod skills;
mod study;
mod terrain;
mod tidying;
mod timeline;
mod voids;

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::{BUILD_WORK, WORK_RATE};
use crate::map::BaseMap;
use crate::movement::{Busy, is_stuck};
use std::collections::BTreeMap;

use crate::ruleset::TileDef;
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
    world.insert_resource(SkillRules::default());
    world.insert_resource(NeedRules::default());
    world.insert_resource(FoodRules::default());
    world.insert_resource(MissionRules::default());
    world.insert_resource(RecruitRules::default());
    world.insert_resource(ResearchRules::default());
    world.insert_resource(CraftRules::default());
    world.insert_resource(Techs::default());
    world.insert_resource(TimelineRules::default());
    world.insert_resource(Chronicle::default());
    world.insert_resource(Fame::default());
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
            gate: false,
            teaches: String::new(),
            lab: false,
            shop: false,
            solid: false,
            tech: String::new(),
        }],
        items: Vec::new(),
        skills: Vec::new(),
        perks: Vec::new(),
        missions: Vec::new(),
        recruits: Vec::new(),
        research: Vec::new(),
        recipes: Vec::new(),
        timeline: Vec::new(),
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
            Option<&Study>,
            Option<&Researching>,
            Option<&Crafting>,
            Option<&Equipping>,
            Option<&Eating>,
            Option<&Squad>,
            Option<&Away>,
        )>();
        let map = self.world.resource::<BaseMap>();
        q.iter(&self.world)
            .find(|(id, ..)| id.0 == unit)
            .map(|(_, p, o, path, a, h, r, st, re, cr, eq, ea, s, away)| {
                is_stuck(
                    map,
                    p,
                    Busy::of(o, path, a, h, r, st, re, cr, eq, ea, s, away),
                )
            })
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
    /// Парта такому домену не учит (`taught: 0`), как и «Стройка» в рулсете, —
    /// обучение включают отдельно (`set_taught`).
    fn set_skill(&mut self, id: &str, levels: &[i32]) -> usize {
        let mut rules = self.world.resource_mut::<SkillRules>();
        rules.0.push(SkillRule {
            id: id.to_string(),
            levels: levels.to_vec(),
            taught: 0,
        });
        rules.0.len() - 1
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

    /// Сделать тайл шлюзом: отсюда отряд уходит на миссию и сюда возвращается.
    fn set_gate(&mut self, tile: i16, on: bool) {
        self.tile_rule(tile, |r| r.gate = on);
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
            perks: Vec::new(),
        });
        rules.0.len() - 1
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
