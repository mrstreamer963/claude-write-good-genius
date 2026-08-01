//! WASM-интерфейс: всё, что видит воркер.
//!
//! Наружу отдаём:
//!   * `map_meta()`      — размеры и палитра тайлов (один раз, для рендера);
//!   * `map_version()`   — счётчик изменений карты (для отправки base_map по требованию);
//!   * `base_map()`      — текущее состояние тайлов базы;
//!   * `add_blueprint()` — поставить чертёж (задачу); выполняют коты, по тикам;
//!   * `plan_demolish()` — ластик: отменить чертёж либо запланировать снос тайла;
//!   * `*_rect()`        — те же инструменты на рамку; решение — на всю рамку сразу;
//!   * `mark_to_store_rect()` — пометить кучи «на склад» (или снять пометку);
//!   * `set_auto_tidy()` — убирать ли лом с пола без приказа;
//!   * `set_auto_rest()` — бросает ли кот работу на критической бодрости;
//!   * `launch()`        — отправить названных котов на вылазку;
//!   * `cancel_mission()`— распустить отряд, пока он ещё не ушёл с базы;
//!   * `hire()`          — нанять кандидата: известность открывает, склад платит;
//!   * `teach()`         — отправить кота за парту: обучение адресно (§12.18);
//!   * `start_research()`— взяться за тему: склад платит образцами, сядет допущенный;
//!   * `cancel_research()` — бросить тему (образцы не возвращаются);
//!   * `start_craft()`   — заказать штуки по рецепту: склад платит за каждую;
//!   * `cancel_craft()`  — отменить заказ (материал начатой штуки не вернётся);
//!   * `demolish()`      — мгновенный снос без котов (тесты/отладка);
//!   * `set_target()`    — приказ коту идти в тайл (движение по тикам, отменяет его задачу);
//!   * `tick()`          — один фиксированный шаг симуляции;
//!   * `snapshot()`      — сущности, чертежи и кучи лома (каждый кадр).

use bevy_ecs::prelude::*;
use wasm_bindgen::prelude::*;

use crate::components::*;
use crate::hauling::plan_spend;
use crate::jobs::BUILD_WORK;
use crate::map::{BaseMap, rect_cells};
use crate::missions::{outcome, pick_gate};
use crate::movement::{Busy, is_stuck};
use crate::path::find_path;
use crate::ruleset::{
    EventDef, ItemDef, MissionDef, PerkDef, RecipeDef, RecruitDef, ResearchDef, Ruleset, SkillDef,
    TileDef,
};
use crate::schedule::build_schedule;
use crate::skills::{SKILL_RAID, SKILL_SCIENCE, level_of, nearest_desk};
use crate::snapshot::{
    BaseMapDto, BlueprintSnap, CraftSnap, EntitySnap, MapMeta, MissionSnap, NoteSnap, RecipeSnap,
    RecruitSnap, ResearchSnap, SkillSnap, Snapshot, StackSnap, TopicSnap,
};
use crate::timeline::{ready_for, revealed};

#[wasm_bindgen]
pub struct Sim {
    pub(crate) world: World,
    pub(crate) schedule: Schedule,
    pub(crate) palette: Vec<TileDef>,
    pub(crate) items: Vec<ItemDef>,
    pub(crate) skills: Vec<SkillDef>,
    pub(crate) perks: Vec<PerkDef>,
    pub(crate) missions: Vec<MissionDef>,
    pub(crate) recruits: Vec<RecruitDef>,
    pub(crate) research: Vec<ResearchDef>,
    pub(crate) recipes: Vec<RecipeDef>,
    pub(crate) timeline: Vec<EventDef>,
    pub(crate) width: i32,
    pub(crate) height: i32,
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

    /// Снять чертёж с клетки, освободить строителя с носильщиком и **вернуть
    /// завезённое** (§12.31).
    ///
    /// Материал ложится кучей на клетку самой площадки — тем же правилом, что
    /// возврат от сноса и добыча с вылазки: вещь появляется там, где была
    /// работа. Площадка обычно ещё пустота, и куча оказывается в яме — оттуда её
    /// штатно сдвинет `settle_stacks` (§12.15), отдельного случая не нужно.
    ///
    /// Груз **в лапах** носильщика при этом остаётся у него: ношу посреди базы
    /// не бросают, он донесёт её следующей доставкой (§12.16).
    fn cancel_blueprint(&mut self, x: i32, y: i32) -> bool {
        let Some(e) = self.blueprint_at(x, y) else {
            return false;
        };
        let bp = self.world.get::<Blueprint>(e);
        let (assignee, hauler) = (bp.and_then(|b| b.assignee), bp.and_then(|b| b.hauler));
        let delivered: Vec<(usize, i32)> = bp.map(|b| b.delivered.clone()).unwrap_or_default();
        for (item, count) in delivered {
            self.drop_stack(x, y, item, count);
        }
        if let Some(cat) = assignee {
            self.world
                .entity_mut(cat)
                .remove::<(Assignment, Path, MoveCooldown)>();
        }
        if let Some(cat) = hauler {
            self.world
                .entity_mut(cat)
                .remove::<(Haul, Path, MoveCooldown)>();
        }
        self.world.entity_mut(e).despawn();
        true
    }

    /// Положить кучу на клетку, слив с уже лежащей там кучей того же типа.
    ///
    /// Фасадный двойник `hauling::spill`: тот работает в системе (`Commands` +
    /// `Query`), этот — по миру. Правило одно: кучи одного типа на клетке
    /// сливаются, разных — лежат рядом (§12.21).
    fn drop_stack(&mut self, x: i32, y: i32, item: usize, count: i32) {
        if count <= 0 {
            return;
        }
        let mut q = self.world.query::<(Entity, &Position, &Stack)>();
        let found = q
            .iter(&self.world)
            .find(|(_, p, s)| (p.x, p.y) == (x, y) && s.item == item)
            .map(|(e, ..)| e);
        match found.and_then(|e| self.world.get_mut::<Stack>(e)) {
            Some(mut stack) => stack.count += count,
            None => {
                self.world.spawn((Position { x, y }, Stack { item, count }));
            }
        }
    }

    /// Миссия, которая сейчас идёт (её на POC не больше одной).
    fn mission(&mut self) -> Option<Entity> {
        let mut q = self.world.query_filtered::<Entity, With<Mission>>();
        q.iter(&self.world).next()
    }

    /// Отряд миссии: кто в нём и ушёл ли он уже с базы.
    fn crew_of(&mut self, mission: Entity) -> Vec<(Entity, bool)> {
        let mut q = self.world.query::<(Entity, &Squad, Option<&Away>)>();
        q.iter(&self.world)
            .filter(|(_, squad, _)| squad.0 == mission)
            .map(|(e, _, away)| (e, away.is_some()))
            .collect()
    }

    /// Снять миссию и освободить весь её отряд. Зовётся и по кнопке «Отозвать»,
    /// и когда игрок уводит бойца приказом: состав выбран поимённо, заменить
    /// выбывшего некем, а молча зависшая вылазка хуже честного роспуска.
    fn disband(&mut self, mission: Entity) {
        for (cat_e, _) in self.crew_of(mission) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Squad, Path, MoveCooldown)>();
        }
        self.world.entity_mut(mission).despawn();
    }

    /// Что лежит на складе: кучи на клетках с ёмкостью, в порядке обхода карты.
    ///
    /// Порядок задан явно, а не порядком сущностей: обход ECS зависит от истории
    /// вставок, а любой недетерминизм ломает и тесты, и модель времени (§11).
    fn storage_piles(&mut self) -> Vec<(Entity, usize, i32)> {
        let mut q = self.world.query::<(Entity, &Position, &Stack)>();
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        let mut piles: Vec<(i32, i32, Entity, usize, i32)> = q
            .iter(&self.world)
            .filter(|(_, p, _)| rules.capacity_of(map.tile_at(p.x, p.y)) > 0)
            .map(|(e, p, s)| (p.y, p.x, e, s.item, s.count))
            .collect();
        piles.sort_unstable_by_key(|&(y, x, ..)| (y, x));
        piles.into_iter().map(|(_, _, e, i, n)| (e, i, n)).collect()
    }

    /// Сколько предмета лежит на складе — им и платят (§12.24).
    fn in_storage(&mut self, item: usize) -> i32 {
        self.storage_piles()
            .iter()
            .filter(|&&(_, i, _)| i == item)
            .map(|&(_, _, n)| n)
            .sum()
    }

    /// Хватает ли на складе на весь набор.
    fn storage_covers(&mut self, cost: &[(usize, i32)]) -> bool {
        cost.iter()
            .all(|&(item, need)| self.in_storage(item) >= need)
    }

    /// Списать набор со склада. Либо снимается всё, либо ничего: половинчатая
    /// оплата оставила бы игрока и без предметов, и без кота.
    ///
    /// Сам расчёт живёт в `plan_spend` и общий с производством, которое платит
    /// из системы (§12.30): две арифметики списания однажды разошлись бы на
    /// порядке обхода куч.
    fn spend_from_storage(&mut self, cost: &[(usize, i32)]) -> bool {
        let piles = self.storage_piles();
        let Some(takes) = plan_spend(&piles, cost) else {
            return false;
        };
        for (pile_e, taken) in takes {
            let count = self.world.get::<Stack>(pile_e).map_or(0, |s| s.count);
            if taken >= count {
                self.world.entity_mut(pile_e).despawn();
            } else if let Some(mut stack) = self.world.get_mut::<Stack>(pile_e) {
                stack.count -= taken;
            }
        }
        true
    }

    /// Снять с кота текущую задачу, освободив всё, что она держала.
    ///
    /// Одно место на всех, кто задачу отбирает — приказ игрока и заявка на
    /// вылазку. Забыть освободить чертёж можно ровно однажды, и площадка после
    /// этого навсегда останется за котом, который давно занят другим (§12.15).
    fn release_task(&mut self, cat: Entity) {
        if let Some(bp_e) = self.world.get::<Assignment>(cat).map(|a| a.0) {
            if let Some(mut bp) = self.world.get_mut::<Blueprint>(bp_e) {
                bp.assignee = None;
            }
        }
        match self.world.get::<Haul>(cat).map(|h| h.0) {
            Some(HaulTo::Site(bp_e)) => {
                if let Some(mut bp) = self.world.get_mut::<Blueprint>(bp_e) {
                    bp.hauler = None;
                }
            }
            Some(HaulTo::Store(pile)) => {
                if let Some(mut mark) = pile.and_then(|e| self.world.get_mut::<ToStore>(e)) {
                    mark.hauler = None;
                }
            }
            None => {}
        }
        if let Some(topic_e) = self.world.get::<Researching>(cat).map(|r| r.0) {
            if let Some(mut topic) = self.world.get_mut::<Research>(topic_e) {
                topic.assignee = None;
                topic.spot = None;
            }
        }
        if let Some(order_e) = self.world.get::<Crafting>(cat).map(|c| c.0) {
            if let Some(mut order) = self.world.get_mut::<Craft>(order_e) {
                order.assignee = None;
                order.spot = None;
            }
        }
        // Груз кот при этом не бросает: донесёт, когда снова возьмётся за
        // доставку (§12.15). Сон и учёба снимаются — это осознанные действия
        // (§12.20, §12.18); парту при этом отпускает сам снятый `Study`.
        self.world.entity_mut(cat).remove::<(
            Assignment,
            Haul,
            Rest,
            Study,
            Researching,
            Crafting,
            Equipping,
            Eating,
            Path,
            MoveCooldown,
        )>();
    }

    /// Открыта ли постройка этого тайла: технология изучена или не нужна.
    fn tech_allows(&self, tile: i16) -> bool {
        let rules = self.world.resource::<TileRules>();
        match rules.tech_of(tile) {
            Some(tech) => self.world.resource::<Techs>().knows(tech),
            None => true,
        }
    }

    /// Тема, которую сейчас изучают (на POC не больше одной).
    fn research(&mut self) -> Option<Entity> {
        let mut q = self.world.query_filtered::<Entity, With<Research>>();
        q.iter(&self.world).next()
    }

    /// Есть ли на базе клетка лаборатории — без неё работать негде.
    fn has_lab(&mut self) -> bool {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .any(|(x, y)| rules.is_lab(map.tile_at(x, y)))
    }

    /// Есть ли на базе кот с таким уровнем «Науки» — это допуск, а не скорость
    /// (§12.18): без него тему не возьмёт никто и никогда.
    fn has_scientist(&mut self, level: i32) -> bool {
        let Some(science) = self.world.resource::<SkillRules>().index_of(SKILL_SCIENCE) else {
            return level <= 0; // домена нет вовсе — только тема без допуска
        };
        let mut q = self.world.query_filtered::<Option<&Skills>, With<UnitId>>();
        let rules = self.world.resource::<SkillRules>();
        q.iter(&self.world)
            .any(|skills| level_of(rules, skills, science) >= level)
    }

    /// Заказ, который сейчас в работе (на POC не больше одного).
    fn order(&mut self) -> Option<Entity> {
        let mut q = self.world.query_filtered::<Entity, With<Craft>>();
        q.iter(&self.world).next()
    }

    /// Есть ли на базе клетка мастерской — без неё работать негде.
    fn has_shop(&mut self) -> bool {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .any(|(x, y)| rules.is_shop(map.tile_at(x, y)))
    }

    /// Клетки, занятые учениками: и та, за которой сидят, и та, к которой идут.
    fn taken_desks(&mut self) -> Vec<(i32, i32)> {
        let mut q = self.world.query::<&Study>();
        q.iter(&self.world).map(|s| s.spot).collect()
    }
}

/// Собрать кота по правилам рулсета.
///
/// Одно место на стартовую тройку и на любого новичка с найма (§12.24): иначе
/// нанятый однажды приедет с лапами другого размера или без бодрости, и понять
/// это можно будет только по странному поведению.
///
/// Перк превращается в числа здесь, один раз: расти ему всё равно некуда
/// (§12.17). Опыт — стартовый багаж кандидата, у стартовой тройки он пуст.
fn spawn_cat(
    world: &mut World,
    id: &str,
    sprite: &str,
    at: (i32, i32),
    perks: &[String],
    skills: &[(usize, i32)],
) -> Entity {
    let carry = world.resource::<UnitRules>().carry;
    let energy_max = world.resource::<NeedRules>().max;
    let fed_max = world.resource::<FoodRules>().max;
    let caps: Vec<(usize, i32)> = {
        let rules = world.resource::<SkillRules>();
        skills.iter().map(|&(s, _)| (s, rules.xp_cap(s))).collect()
    };

    let hauler = perks.iter().any(|p| p == PERK_HAULER);
    let mut cat = world.spawn((
        UnitId(id.to_string()),
        Renderable {
            sprite: sprite.to_string(),
        },
        Position { x: at.0, y: at.1 },
        Perks(perks.to_vec()),
    ));
    if carry > 0 {
        cat.insert(Carry(carry * if hauler { 2 } else { 1 }));
    }
    if energy_max > 0 {
        cat.insert(Energy(energy_max));
    }
    // Новичок приходит сытым, как и бодрым: голодать он начнёт на общих
    // основаниях, а не с порога (§12.36).
    if fed_max > 0 {
        cat.insert(Fed(fed_max));
    }
    if !skills.is_empty() {
        let mut xp = Skills::default();
        for (&(skill, amount), &(_, cap)) in skills.iter().zip(&caps) {
            xp.add_xp(skill, amount, cap.max(amount));
        }
        cat.insert(xp);
    }
    cat.id()
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
        let item_index = |id: &str| rs.items.iter().position(|i| i.id == id);

        let mut map = BaseMap::empty(w, h);
        for b in &rs.build {
            if let Some(idx) = tile_index(&b.tile) {
                map.fill_rect(b.rect, idx);
            }
        }

        let mut world = World::new();
        world.insert_resource(map);
        world.insert_resource(SimTime { tick: 0 });
        // Навыки заводим до тайлов: парта ссылается на домен по имени, а в
        // правилах остаётся его индекс — как цена ссылается на предмет.
        let skill_index = |id: &str| rs.skills.iter().position(|s| s.id == id);
        world.insert_resource(SkillRules(
            rs.skills
                .iter()
                .map(|s| SkillRule {
                    id: s.id.clone(),
                    levels: s.levels.clone(),
                    taught: s.taught,
                })
                .collect(),
        ));
        // Цена из рулсета — имена предметов; в правилах остаются индексы палитры.
        // Порядок пар задан `BTreeMap` (по имени), то есть детерминирован (§12.21).
        world.insert_resource(TileRules(
            rs.tiles
                .iter()
                .map(|t| TileRule {
                    cost: t
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    capacity: t.capacity,
                    rest: t.rest,
                    gate: t.gate,
                    teaches: skill_index(&t.teaches),
                    lab: t.lab,
                    shop: t.shop,
                    solid: t.solid,
                    tech: t.tech.clone(),
                })
                .collect(),
        ));
        world.insert_resource(ResearchRules(
            rs.research
                .iter()
                .map(|r| ResearchRule {
                    id: r.id.clone(),
                    level: r.level,
                    work: r.work,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    requires: r.requires.clone(),
                })
                .collect(),
        ));
        world.insert_resource(CraftRules(
            rs.recipes
                .iter()
                .map(|r| CraftRule {
                    work: r.work,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    gives: r
                        .gives
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    requires: r.requires.clone(),
                })
                .collect(),
        ));
        world.insert_resource(Techs::default());
        world.insert_resource(TimelineRules(
            rs.timeline
                .iter()
                .map(|e| EventRule {
                    at: e.at,
                    reveal: e.reveal,
                    requires: e.requires.clone(),
                    gift: e
                        .gift
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    fame: e.fame,
                    toll: e.toll,
                })
                .collect(),
        ));
        world.insert_resource(Chronicle::default());
        world.insert_resource(MissionRules(
            rs.missions
                .iter()
                .map(|m| MissionRule {
                    squad: m.squad,
                    ticks: m.ticks,
                    danger: m.danger,
                    toll: m.toll,
                    loot: m
                        .loot
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    fame: m.fame,
                    requires: m.requires,
                })
                .collect(),
        ));
        world.insert_resource(RecruitRules(
            rs.recruits
                .iter()
                .map(|r| RecruitRule {
                    id: r.id.clone(),
                    sprite: r.sprite.clone(),
                    requires: r.requires,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    skills: r
                        .skills
                        .iter()
                        .filter_map(|(id, &xp)| skill_index(id).map(|i| (i, xp)))
                        .collect(),
                    perks: r.perks.clone(),
                })
                .collect(),
        ));
        world.insert_resource(Fame::default());
        // Снаряжение — свойство предмета, а шаблон ссылается на предметы по
        // имени: в правилах, как и в цене тайла, остаются индексы палитры.
        world.insert_resource(ItemRules(
            rs.items
                .iter()
                .map(|i| ItemRule {
                    force: i.force,
                    nutrition: i.nutrition,
                })
                .collect(),
        ));
        world.insert_resource(LoadoutRules(
            rs.loadout.iter().filter_map(|id| item_index(id)).collect(),
        ));
        world.insert_resource(UnitRules { carry: rs.carry });
        world.insert_resource(AutoTidy(true));
        world.insert_resource(AutoRest(true));
        world.insert_resource(NeedRules {
            max: rs.energy.max,
            tired: rs.energy.tired,
            critical: rs.energy.critical,
            floor: rs.energy.floor,
        });
        world.insert_resource(FoodRules {
            max: rs.food.max,
            hungry: rs.food.hungry,
            starve: rs.food.starve,
        });

        // Стартовый запас. Стартовая застройка (`build`) при этом бесплатна —
        // это уже существующая база, а не работа котов.
        for s in &rs.stock {
            let Some(item) = item_index(&s.item) else {
                continue; // предмета нет в палитре — запись контента мимо
            };
            world.spawn((
                Position {
                    x: s.at[0],
                    y: s.at[1],
                },
                Stack {
                    item,
                    count: s.count,
                },
            ));
        }

        for u in &rs.units {
            spawn_cat(
                &mut world,
                &u.id,
                &u.sprite,
                (u.pos[0], u.pos[1]),
                &u.perks,
                &[],
            );
        }

        let schedule = build_schedule();
        Ok(Sim {
            world,
            schedule,
            palette: rs.tiles,
            items: rs.items,
            skills: rs.skills,
            perks: rs.perks,
            missions: rs.missions,
            recruits: rs.recruits,
            research: rs.research,
            recipes: rs.recipes,
            timeline: rs.timeline,
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
            items: self.items.clone(),
            skills: self.skills.clone(),
            perks: self.perks.clone(),
            missions: self.missions.clone(),
            recruits: self.recruits.clone(),
            research: self.research.clone(),
            recipes: self.recipes.clone(),
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

    /// Поставить чертёж (джоб) `tile` на клетку (x, y). Выполняют коты, по тикам.
    /// `tile = -1` — чертёж сноса: кот придёт на соседнюю клетку и уберёт пол.
    /// Вернёт true, если чертёж добавлен/обновлён.
    pub fn add_blueprint(&mut self, x: i32, y: i32, tile: i32) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let t = tile as i16;
        // Технология — ворота палитры (§12.27). Проверяем в момент разметки, а
        // не в момент работы: чертёж, который никто никогда не возьмёт, игрок
        // прочтёт как поломку, а не как «сперва изучите». У сноса ворот нет —
        // разбирать можно что угодно и всегда.
        if !self.tech_allows(t) {
            return false;
        }
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
            delivered: Vec::new(),
            hauler: None,
        });
        true
    }

    /// Поставить чертежи `tile` на весь прямоугольник — один жест рамкой.
    /// Вернёт true, если хоть одна клетка изменилась.
    pub fn add_blueprint_rect(&mut self, x: i32, y: i32, w: i32, h: i32, tile: i32) -> bool {
        let mut changed = false;
        for (cx, cy) in rect_cells(x, y, w, h) {
            changed |= self.add_blueprint(cx, cy, tile);
        }
        changed
    }

    /// Ластик игрока: отменить чертёж, либо запланировать снос построенного тайла.
    ///
    /// Отмена плана мгновенна — строить ещё не начинали, и большая часть «ой, не
    /// туда» приходится именно на неё. Снос готового тайла идёт через ту же
    /// очередь, что и стройка: чертёж с `tile = -1`, кот приходит на соседнюю
    /// клетку и работает. Повторный ластик по клетке с чертежом сноса отменяет
    /// его. Вернёт true, если что-то изменилось.
    pub fn plan_demolish(&mut self, x: i32, y: i32) -> bool {
        self.plan_demolish_rect(x, y, 1, 1)
    }

    /// Ластик по прямоугольнику. Решение принимается на всю рамку сразу: сперва
    /// снимаем чертежи, и только если снимать было нечего — планируем снос пола.
    ///
    /// Потайловый переключатель на рамке дал бы кашу: там, где часть клеток уже
    /// запланирована, один жест половину снял бы, а половину поставил, и результат
    /// зависел бы от того, что игрок делал раньше. Порядок «сначала отмена» ещё и
    /// безопаснее — отмена ничего не разрушает.
    pub fn plan_demolish_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        let cells: Vec<(i32, i32)> = rect_cells(x, y, w, h).collect();
        let mut cancelled = false;
        for &(cx, cy) in &cells {
            cancelled |= self.cancel_blueprint(cx, cy);
        }
        if cancelled {
            return true;
        }
        let mut planned = false;
        for &(cx, cy) in &cells {
            planned |= self.add_blueprint(cx, cy, -1);
        }
        planned
    }

    /// Убирать ли лом с пола без приказа игрока.
    ///
    /// Выключение снимает все пометки и разворачивает котов, которые шли за
    /// кучей: иначе переключатель выглядел бы сломанным — коты продолжали бы
    /// разбирать помеченное. Кот, уже несущий груз, свою ходку доводит до
    /// конца: бросать лом посреди базы хуже, чем донести (§12.16).
    pub fn set_auto_tidy(&mut self, on: bool) {
        self.world.resource_mut::<AutoTidy>().0 = on;
        if on {
            return;
        }

        let mut marked = self.world.query_filtered::<Entity, With<ToStore>>();
        for e in marked.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(e).remove::<ToStore>();
        }

        let mut q = self.world.query::<(Entity, &Haul, Option<&Carrying>)>();
        let going: Vec<Entity> = q
            .iter(&self.world)
            .filter(|(_, haul, load)| matches!(haul.0, HaulTo::Store(_)) && load.is_none())
            .map(|(e, ..)| e)
            .collect();
        for e in going {
            self.world
                .entity_mut(e)
                .remove::<(Haul, Path, MoveCooldown)>();
        }
    }

    /// Бросает ли кот начатое, когда бодрость ушла ниже критического порога
    /// (§12.33). Выключено — коты работают до нуля и валятся где стоят, как до
    /// второго порога.
    ///
    /// В отличие от `set_auto_tidy`, выключение **никого не будит**: уже
    /// спящий кот — это состояние, а не пометка игрока, и снимать её значило бы
    /// поднимать котов на нуле бодрости, чтобы они тут же упали снова.
    pub fn set_auto_rest(&mut self, on: bool) {
        self.world.resource_mut::<AutoRest>().0 = on;
    }

    /// Пометить кучи под рамкой «на склад» — или снять пометку.
    ///
    /// Решение принимается на всю рамку сразу, по правилу ластика (§12.13):
    /// есть под рамкой хоть одна помеченная куча — снимаем все пометки, нет —
    /// помечаем все. Кота выбирать не нужно: как и чертёж, это разметка работы,
    /// а возьмёт её любой свободный.
    ///
    /// Вернёт true, если что-то изменилось.
    pub fn mark_to_store_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        let cells: Vec<(i32, i32)> = rect_cells(x, y, w, h).collect();
        let mut q = self
            .world
            .query::<(Entity, &Position, Option<&ToStore>, &Stack)>();
        let under: Vec<(Entity, bool)> = q
            .iter(&self.world)
            .filter(|(_, p, ..)| cells.contains(&(p.x, p.y)))
            .map(|(e, _, mark, _)| (e, mark.is_some()))
            .collect();
        if under.is_empty() {
            return false;
        }

        let unmark = under.iter().any(|&(_, marked)| marked);
        for (e, marked) in under {
            match (unmark, marked) {
                (true, true) => {
                    self.world.entity_mut(e).remove::<ToStore>();
                }
                (false, false) => {
                    self.world.entity_mut(e).insert(ToStore::default());
                }
                _ => {}
            }
        }
        true
    }

    /// Отправить названных котов на миссию `def` (индекс в палитре `map_meta`).
    ///
    /// **Отряд выбирает игрок, поимённо** — единственная работа, где исполнитель
    /// не раздаётся симуляцией (§12.23). Причина одна: от состава зависит исход.
    /// Требуется ровно `squad` котов: недобор — это не «пойдут вдвоём вместо
    /// троих», а неполная заявка, и молча дополнять её симуляция не станет.
    ///
    /// Заявка снимает с выбранных текущие задачи — как приказ игрока (§12.15):
    /// решение отправить кота в поле весомее начатой им стройки.
    ///
    /// Миссия одна за раз: на POC с тремя котами вторая осталась бы без людей,
    /// а очередь вылазок — механика, которую не на чем проверить.
    /// Вернёт false, если миссии нет, одна уже идёт, состав не тот или до
    /// общего шлюза дойдут не все.
    pub fn launch(&mut self, def: usize, units: Vec<String>) -> bool {
        let Some(rule) = self.world.resource::<MissionRules>().0.get(def).cloned() else {
            return false;
        };
        if self.mission().is_some() || units.len() != rule.squad {
            return false;
        }
        // Известность — ворота: за дело, о котором ещё не слышали, не берутся,
        // сколько бы сильным ни был отряд (§12.24).
        if self.world.resource::<Fame>().0 < rule.requires {
            return false;
        }

        // Ушедших в списке быть не может — их нет на базе; дубликаты отсекаем
        // сравнением длины, иначе «три раза excellent» сошло бы за отряд.
        let mut crew: Vec<(Entity, (i32, i32))> = Vec::new();
        {
            let mut q = self
                .world
                .query::<(Entity, &UnitId, &Position, Option<&Away>)>();
            for id in &units {
                let found = q
                    .iter(&self.world)
                    .find(|(_, u, _, away)| u.0 == *id && away.is_none())
                    .map(|(e, _, p, _)| (e, (p.x, p.y)));
                match found {
                    Some(cat) if !crew.iter().any(|&(e, _)| e == cat.0) => crew.push(cat),
                    _ => return false,
                }
            }
        }

        let at: Vec<(i32, i32)> = crew.iter().map(|&(_, p)| p).collect();
        let Some(gate) = pick_gate(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            &at,
        ) else {
            return false; // шлюза нет или до общего не добраться всем разом
        };

        let mission_e = self.world.spawn(Mission {
            def,
            gate: Some(gate),
            left: rule.ticks,
        });
        let mission_e = mission_e.id();
        for (cat_e, from) in crew {
            self.release_task(cat_e);
            let path = find_path(self.world.resource::<BaseMap>(), from, gate).unwrap_or_default();
            self.world.entity_mut(cat_e).insert((
                Squad(mission_e),
                Path { steps: path },
                MoveCooldown(0),
            ));
        }
        true
    }

    /// Распустить отряд и снять миссию.
    ///
    /// Работает, только пока отряд на базе: ушедших не отзывают — что там
    /// происходит, симуляция не знает, вылазка считается разом по возвращении.
    /// Вернёт false, если миссии нет или отряд уже ушёл.
    pub fn cancel_mission(&mut self) -> bool {
        let Some(mission_e) = self.mission() else {
            return false;
        };
        if self.crew_of(mission_e).iter().any(|&(_, away)| away) {
            return false;
        }
        self.disband(mission_e);
        true
    }

    /// Нанять кандидата `def` (индекс в палитре из `map_meta`).
    ///
    /// **Известность открывает, платят предметами** (§12.24). Одна шкала и
    /// ворота, и валюта была бы ловушкой: наняв кота, игрок обнаружил бы, что
    /// у него закрылась вылазка, — и прочитал бы это как поломку.
    ///
    /// Платит **склад**: то, что валяется на полу, ещё не сосчитано, а склад —
    /// учтённое имущество базы. Это третий смысл клетки с `capacity` после
    /// ёмкости и места назначения, и первая причина убираться не из аккуратности.
    ///
    /// Новичок появляется у шлюза: гараж — точка контакта базы с миром (§12.22).
    /// Вернёт false, если кандидата нет, известности не хватает, он уже нанят,
    /// на складе нет цены или на базе нет шлюза.
    pub fn hire(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<RecruitRules>().0.get(def).cloned() else {
            return false;
        };
        if self.world.resource::<Fame>().0 < rule.requires {
            return false;
        }
        // Нанят ли — спрашиваем у самого кота: список нанятых был бы вторым
        // источником правды, который однажды разойдётся с миром.
        let mut units = self.world.query::<&UnitId>();
        if units.iter(&self.world).any(|u| u.0 == rule.id) {
            return false;
        }

        // Отряда нет, поэтому «ближайший» вырождается в первый по обходу карты —
        // детерминированно, и этого достаточно: новичок просто приходит.
        let Some(gate) = pick_gate(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            &[],
        ) else {
            return false; // шлюза нет — новичку неоткуда взяться
        };
        if !self.spend_from_storage(&rule.cost) {
            return false;
        }
        spawn_cat(
            &mut self.world,
            &rule.id,
            &rule.sprite,
            gate,
            &rule.perks,
            &rule.skills,
        );
        true
    }

    /// Взяться за тему `def` (индекс в палитре из `map_meta`).
    ///
    /// **Тема — разметка работы, а исполнителя берёт симуляция** (§12.16): игрок
    /// решает, что изучать, а сядет за это ближайший кот, которому хватает
    /// «Науки». Уровень — допуск (§12.18), а не скорость: без него не медленнее,
    /// а никак.
    ///
    /// Платят **образцами со склада**, разом при заявке, — как за найм (§12.24):
    /// работа котов начнётся потом, а решение принято сейчас.
    ///
    /// Тема одна за раз: на POC второй некого посадить, а очередь тем — механика,
    /// которую не на чем проверить. Вернёт false, если темы нет, она уже изучена
    /// или идёт, не хватает предыдущих технологий, на базе нет лаборатории,
    /// некому взяться или на складе нечем заплатить.
    pub fn start_research(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<ResearchRules>().0.get(def).cloned() else {
            return false;
        };
        if self.research().is_some() {
            return false;
        }
        let techs = self.world.resource::<Techs>();
        // Уже изученное не изучают снова, а без предыдущих технологий темы
        // просто не существует — это дерево из §4.3.
        if techs.knows(&rule.id) || !techs.covers(&rule.requires) {
            return false;
        }
        if !self.has_lab() {
            return false;
        }
        // Некому взяться — отказываем **до** оплаты: тема, за которую заплачено
        // образцами и которую никто не возьмёт, читается как потерянный ресурс,
        // а не как «подождите, пока кто-нибудь доучится».
        if !self.has_scientist(rule.level) {
            return false;
        }
        if !self.spend_from_storage(&rule.cost) {
            return false;
        }
        self.world.spawn(Research {
            def,
            progress: 0,
            assignee: None,
            spot: None,
        });
        true
    }

    /// Бросить тему и освободить исполнителя.
    ///
    /// Образцы при этом **не возвращаются**: их уже разобрали на опыты — та же
    /// цена поспешной разметки, что и у отменённого чертежа с завезённым ломом.
    /// Вернёт false, если изучать нечего.
    pub fn cancel_research(&mut self) -> bool {
        let Some(topic_e) = self.research() else {
            return false;
        };
        if let Some(cat_e) = self.world.get::<Research>(topic_e).and_then(|t| t.assignee) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Researching, Path, MoveCooldown)>();
        }
        self.world.entity_mut(topic_e).despawn();
        true
    }

    /// Заказать `count` штук по рецепту `def` (индекс в палитре из `map_meta`).
    ///
    /// **Заказ — разметка работы, как чертёж** (§12.16): игрок решает, что и
    /// сколько сделать, а к верстаку встанет ближайший свободный кот. Допуска по
    /// навыку здесь нет — рецепт открывает технология, «Ремесло» лишь ускоряет
    /// (§12.30).
    ///
    /// **Вперёд не платят.** Материал списывается за штуку и в тот момент, когда
    /// за неё берутся: заказ на десять штук, оплаченный сразу, заморозил бы склад
    /// под работу, которая начнётся через полтысячи тиков. Поэтому и пустой склад
    /// заявку не отклоняет — заказ ждёт материала, как чертёж ждёт лом (§12.15).
    ///
    /// Заказ один за раз, как вылазка и тема. Вернёт false, если рецепта нет,
    /// счётчик неположителен, заказ уже идёт, не хватает технологий или на базе
    /// нет мастерской.
    pub fn start_craft(&mut self, def: usize, count: i32) -> bool {
        let Some(rule) = self.world.resource::<CraftRules>().0.get(def).cloned() else {
            return false;
        };
        if count <= 0 || self.order().is_some() {
            return false;
        }
        if !self.world.resource::<Techs>().covers(&rule.requires) {
            return false;
        }
        if !self.has_shop() {
            return false;
        }
        self.world.spawn(Craft {
            def,
            left: count,
            progress: 0,
            paid: false,
            assignee: None,
            spot: None,
        });
        true
    }

    /// Отменить заказ и освободить мастера.
    ///
    /// Материал уже начатой штуки **не возвращается** — та же цена поспешной
    /// разметки, что у брошенной темы и у отменённого чертежа с завезённым ломом
    /// (§12.26). Неоплаченные штуки не стоили ничего и просто исчезают.
    /// Вернёт false, если заказывать нечего.
    pub fn cancel_craft(&mut self) -> bool {
        let Some(order_e) = self.order() else {
            return false;
        };
        if let Some(cat_e) = self.world.get::<Craft>(order_e).and_then(|o| o.assignee) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Crafting, Path, MoveCooldown)>();
        }
        self.world.entity_mut(order_e).despawn();
        true
    }

    /// Отправить кота `unit_id` учиться домену `skill_id`.
    ///
    /// **Обучение адресно** (§12.18) — как приказ «иди туда» и как заявка на
    /// вылазку: правило §12.16 «игрок размечает работу, исполнителя берёт
    /// симуляция» здесь не нарушается, а получает вторую границу. Обучение —
    /// не работа над базой, а решение о судьбе конкретного кота: это он
    /// перестанет строить на ближайшие пару сотен тиков.
    ///
    /// Команда снимает с кота текущую задачу и распускает его вылазку — ровно
    /// как приказ игрока (§12.23). Вернёт false, если кота нет, он в поле,
    /// домену не учат, коту уже нечему учиться за партой или парты нет.
    pub fn teach(&mut self, unit_id: &str, skill_id: &str) -> bool {
        let rules = self.world.resource::<SkillRules>();
        let Some(skill) = rules.index_of(skill_id) else {
            return false;
        };
        // Ноль — домену не учат вовсе («Стройка»): парта ему не поможет.
        let cap = rules.taught_cap(skill);
        if cap <= 0 {
            return false;
        }

        let mut found = None;
        {
            let mut q = self
                .world
                .query::<(Entity, &UnitId, &Position, Option<&Skills>, Option<&Away>)>();
            for (e, id, p, skills, away) in q.iter(&self.world) {
                if id.0 == unit_id && away.is_none() {
                    found = Some((e, (p.x, p.y), skills.map_or(0, |s| s.xp_of(skill))));
                    break;
                }
            }
        }
        // Кота нет на базе — учить некого: его позиция это шлюз, с которого он
        // ушёл (§12.22).
        let Some((cat_e, from, xp)) = found else {
            return false;
        };
        // Парта — вход в домен, а не тренажёр: доученного она не берёт.
        if xp >= cap {
            return false;
        }

        let taken = self.taken_desks();
        let Some(spot) = nearest_desk(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            skill,
            from,
            &taken,
        ) else {
            return false; // парт нет, все заняты или до них не добраться
        };

        self.release_task(cat_e);
        if let Some(mission_e) = self.world.get::<Squad>(cat_e).map(|s| s.0) {
            self.disband(mission_e);
        }
        // Старый приказ «иди туда» снимается: обучение — такое же адресное
        // распоряжение этим котом, и два противоречащих друг другу висеть не
        // должны. Иначе кот, доучившись, ушёл бы «доисполнять» забытый приказ.
        let path = find_path(self.world.resource::<BaseMap>(), from, spot).unwrap_or_default();
        self.world.entity_mut(cat_e).remove::<Order>().insert((
            Study { skill, spot },
            Path { steps: path },
            MoveCooldown(0),
        ));
        true
    }

    /// Мгновенно снести тайл вместе с чертежом, без участия котов.
    ///
    /// Ластик игрока ходит через `plan_demolish`; этот путь оставлен для тестов
    /// и отладки. Вернёт true при изменении.
    pub fn demolish(&mut self, x: i32, y: i32) -> bool {
        let mut changed = self.cancel_blueprint(x, y);
        if self.world.resource_mut::<BaseMap>().set(x, y, -1) {
            changed = true;
        }
        changed
    }

    /// Приказ коту `unit_id` идти в тайл (x, y). Отменяет его джоб постройки, если был.
    /// Приказ сохраняется, даже если путь пока не найден — маршрут будет
    /// перепроложен автоматически, как только карта изменится (см. `retry_orders`).
    /// Вернёт true, если приказ принят (цель проходима), false — если цель не тайл-пол
    /// или юнит не найден.
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
        // Кота нет на базе — приказывать некому: его позиция это шлюз, с
        // которого он ушёл, а сам он вернётся туда же (§12.22).
        if self.world.get::<Away>(entity).is_some() {
            return false;
        }

        if !self.world.resource::<BaseMap>().walkable(x, y) {
            return false;
        }
        let map_version = self.world.resource::<BaseMap>().version;
        let path = find_path(self.world.resource::<BaseMap>(), (sx, sy), (x, y));

        // Приказ забирает кота у любой задачи — стройки, переноса, сна (§12.15,
        // §12.20). Груз он при этом не бросает: донесёт, когда снова возьмётся
        // за доставку.
        self.release_task(entity);

        // А вылазку приказ **распускает целиком**: состав выбран поимённо,
        // заменить выбывшего некем, и отряд, который никогда не соберётся,
        // хуже честного роспуска (§12.23). Ушедшего это не касается — такого
        // кота нет в мире базы, и приказ ему отклонён выше.
        if let Some(mission_e) = self.world.get::<Squad>(entity).map(|s| s.0) {
            self.disband(mission_e);
        }

        // Приказ сохраняется даже без пути прямо сейчас — `retry_orders`
        // перепроложит маршрут при следующем изменении карты (например, после
        // постройки коридора, открывающего доступ к цели).
        self.world.entity_mut(entity).insert(Order {
            x,
            y,
            tried_version: map_version,
        });
        match path {
            Some(p) => {
                self.world
                    .entity_mut(entity)
                    .insert((Path { steps: p }, MoveCooldown(0)));
            }
            None => {
                self.world
                    .entity_mut(entity)
                    .remove::<(Path, MoveCooldown)>();
            }
        }
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
            // Задачи кота собраны вложенным кортежем: их ровно столько, сколько
            // берёт `Busy::of`, и растут они вместе — а плоский запрос упёрся бы
            // в предел арности `QueryData`.
            let mut q = self.world.query::<(
                &UnitId,
                &Renderable,
                &Position,
                Option<&Carrying>,
                Option<&Carry>,
                Option<&Skills>,
                Option<&Perks>,
                Option<&Energy>,
                Option<&Fed>,
                Option<&Gear>,
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
                    Option<&Squad>,
                    Option<&Away>,
                ),
            )>();
            let map = self.world.resource::<BaseMap>();
            let rules = self.world.resource::<SkillRules>();
            let needs = self.world.resource::<NeedRules>();
            let food = self.world.resource::<FoodRules>();
            for (id, r, p, load, carry, skills, perks, energy, fed, gear, tasks) in
                q.iter(&self.world)
            {
                let (
                    order,
                    path,
                    assignment,
                    haul,
                    rest,
                    study,
                    researching,
                    crafting,
                    equipping,
                    eating,
                    squad,
                    away,
                ) = tasks;
                let busy = Busy::of(
                    order,
                    path,
                    assignment,
                    haul,
                    rest,
                    study,
                    researching,
                    crafting,
                    equipping,
                    eating,
                    squad,
                    away,
                );
                entities.push(EntitySnap {
                    id: id.0.clone(),
                    sprite: r.sprite.clone(),
                    x: p.x,
                    y: p.y,
                    stuck: is_stuck(map, p, busy),
                    away: away.is_some(),
                    energy: energy.map_or(0, |e| e.0),
                    energy_max: needs.max,
                    fed: fed.map_or(0, |f| f.0),
                    fed_max: food.max,
                    // Порог голода уходит наружу вместе со шкалой: без него
                    // панель не отличит «наелся минуту назад» от «уже идёт
                    // есть», а второй экземпляр числа в JS однажды разойдётся
                    // с рулсетом (§12.26 — считает ядро, а не интерфейс).
                    fed_hungry: food.hungry,
                    // Спит, а не идёт спать: маршрут ещё есть — значит в пути.
                    sleeping: rest.is_some() && path.is_none(),
                    // Учится, а не идёт к парте — по тому же признаку.
                    studying: study.is_some() && path.is_none(),
                    carrying: load.map_or(0, |c| c.count),
                    carrying_item: load.map_or(-1, |c| c.item as i32),
                    carry_max: carry.map_or(0, |c| c.0),
                    skills: (0..rules.0.len())
                        .map(|i| {
                            let xp = skills.map_or(0, |s| s.xp_of(i));
                            SkillSnap {
                                level: rules.level(i, xp),
                                xp,
                                next: rules.next_threshold(i, xp),
                            }
                        })
                        .collect(),
                    perks: perks.map(|p| p.0.clone()).unwrap_or_default(),
                    // Надетое видно в панели кота: снаряжение молча прибавляет
                    // отряду силы, и без этого игрок не свяжет пропавший со
                    // склада комбинезон с выросшим прогнозом вылазки (§12.29).
                    gear: gear.map(|g| g.0.clone()).unwrap_or_default(),
                });
            }
        }

        let mut blueprints = Vec::new();
        {
            let mut q = self.world.query::<&Blueprint>();
            let rules = self.world.resource::<TileRules>();
            for bp in q.iter(&self.world) {
                blueprints.push(BlueprintSnap {
                    x: bp.x,
                    y: bp.y,
                    tile: bp.tile,
                    progress: bp.progress,
                    total: BUILD_WORK,
                    // Полоска подвоза показывает набор целиком: сколько всего
                    // штук нужно и сколько уже лежит на площадке.
                    need: rules.cost_of(bp.tile).iter().map(|&(_, n)| n).sum(),
                    delivered: bp.delivered.iter().map(|&(_, n)| n).sum(),
                });
            }
        }

        let mut stacks = Vec::new();
        {
            let mut q = self.world.query::<(&Position, &Stack, Option<&ToStore>)>();
            for (p, s, mark) in q.iter(&self.world) {
                stacks.push(StackSnap {
                    x: p.x,
                    y: p.y,
                    item: s.item,
                    count: s.count,
                    marked: mark.is_some(),
                });
            }
        }

        let mut missions = Vec::new();
        {
            let raid = self.world.resource::<SkillRules>().index_of(SKILL_RAID);
            let mut crew = self.world.query::<(
                &UnitId,
                &Squad,
                Option<&Away>,
                Option<&Skills>,
                Option<&Gear>,
            )>();
            let skill_rules = self.world.resource::<SkillRules>();
            let items = self.world.resource::<ItemRules>();
            // Вклад кота в силу отряда считается ровно как в `run_missions`:
            // сам он стоит единицу, уровень «Вылазки» — сверху, надетое — ещё
            // сверху. Прогноз и результат обязаны быть одним выражением (§12.23).
            let members: Vec<(Entity, String, bool, i32)> = crew
                .iter(&self.world)
                .map(|(id, squad, away, skills, gear)| {
                    let force = 1
                        + raid.map_or(0, |s| level_of(skill_rules, skills, s))
                        + items.force_of_gear(gear);
                    (squad.0, id.0.clone(), away.is_some(), force)
                })
                .collect();
            let mut q = self.world.query::<(Entity, &Mission)>();
            let rules = self.world.resource::<MissionRules>();
            for (e, m) in q.iter(&self.world) {
                let rule = rules.0.get(m.def);
                let mine = || members.iter().filter(move |&&(owner, ..)| owner == e);
                let danger = rule.map_or(0, |r| r.danger);
                let out = outcome(danger, mine().map(|&(.., force)| force).sum());
                missions.push(MissionSnap {
                    def: m.def,
                    x: m.gate.map_or(-1, |(x, _)| x),
                    y: m.gate.map_or(-1, |(_, y)| y),
                    left: m.left,
                    total: rule.map_or(0, |r| r.ticks),
                    squad: mine().map(|(_, id, ..)| id.clone()).collect(),
                    size: rule.map_or(0, |r| r.squad),
                    away: mine().any(|&(_, _, away, _)| away),
                    strength: out.strength,
                    danger,
                    share: out.share,
                    failed: out.failed,
                });
            }
        }

        let fame = self.world.resource::<Fame>().0;
        let mut recruits = Vec::new();
        {
            let rules = self.world.resource::<RecruitRules>().0.clone();
            let hired: Vec<String> = {
                let mut q = self.world.query::<&UnitId>();
                q.iter(&self.world).map(|u| u.0.clone()).collect()
            };
            for rule in &rules {
                recruits.push(RecruitSnap {
                    hired: hired.contains(&rule.id),
                    unlocked: fame >= rule.requires,
                    affordable: self.storage_covers(&rule.cost),
                });
            }
        }

        let mut research = Vec::new();
        {
            let mut q = self.world.query::<&Research>();
            let names: Vec<(Entity, String)> = {
                let mut cats = self.world.query::<(Entity, &UnitId)>();
                cats.iter(&self.world)
                    .map(|(e, u)| (e, u.0.clone()))
                    .collect()
            };
            let rules = self.world.resource::<ResearchRules>();
            for topic in q.iter(&self.world) {
                research.push(ResearchSnap {
                    def: topic.def,
                    progress: topic.progress,
                    total: rules.0.get(topic.def).map_or(0, |r| r.work),
                    unit: topic
                        .assignee
                        .and_then(|e| names.iter().find(|&&(cat, _)| cat == e))
                        .map(|(_, id)| id.clone())
                        .unwrap_or_default(),
                });
            }
        }

        let techs = self.world.resource::<Techs>().0.clone();
        let has_lab = self.has_lab();
        let mut topics = Vec::new();
        {
            let rules = self.world.resource::<ResearchRules>().0.clone();
            for rule in &rules {
                let known = self.world.resource::<Techs>().knows(&rule.id);
                let unlocked = self.world.resource::<Techs>().covers(&rule.requires);
                topics.push(TopicSnap {
                    known,
                    unlocked,
                    affordable: self.storage_covers(&rule.cost),
                    staffed: self.has_scientist(rule.level),
                    lab: has_lab,
                });
            }
        }

        let mut crafting = Vec::new();
        {
            let mut q = self.world.query::<&Craft>();
            let names: Vec<(Entity, String)> = {
                let mut cats = self.world.query::<(Entity, &UnitId)>();
                cats.iter(&self.world)
                    .map(|(e, u)| (e, u.0.clone()))
                    .collect()
            };
            let rules = self.world.resource::<CraftRules>();
            for order in q.iter(&self.world) {
                crafting.push(CraftSnap {
                    def: order.def,
                    left: order.left,
                    progress: order.progress,
                    total: rules.0.get(order.def).map_or(0, |r| r.work),
                    paid: order.paid,
                    unit: order
                        .assignee
                        .and_then(|e| names.iter().find(|&&(cat, _)| cat == e))
                        .map(|(_, id)| id.clone())
                        .unwrap_or_default(),
                });
            }
        }

        let has_shop = self.has_shop();
        let mut recipes = Vec::new();
        {
            let rules = self.world.resource::<CraftRules>().0.clone();
            for rule in &rules {
                recipes.push(RecipeSnap {
                    unlocked: self.world.resource::<Techs>().covers(&rule.requires),
                    // Подсказка, а не запрет: заказ без материала ядро примет,
                    // он просто будет ждать склада (§12.30).
                    affordable: self.storage_covers(&rule.cost),
                    shop: has_shop,
                });
            }
        }

        // Записка. Что игрок знает о будущем — решает ядро: до `reveal` детали
        // в снапшот просто не кладутся (§4.6, §12.28).
        let mut notes = Vec::new();
        {
            let rules = self.world.resource::<TimelineRules>().0.clone();
            for (def, rule) in rules.iter().enumerate() {
                let happened = self.world.resource::<Chronicle>().happened(def);
                let open = revealed(rule, tick);
                let ready = open && ready_for(rule, self.world.resource::<Techs>());
                notes.push(NoteSnap {
                    def,
                    label: self.timeline[def].label.clone(),
                    at: rule.at,
                    left: rule.at as i64 - tick as i64,
                    hint: self.timeline[def].hint.clone(),
                    revealed: open,
                    detail: if open {
                        self.timeline[def].detail.clone()
                    } else {
                        String::new()
                    },
                    requires: if open {
                        rule.requires.clone()
                    } else {
                        Vec::new()
                    },
                    ready,
                    done: happened.is_some(),
                    succeeded: happened.is_some_and(|h| h.ready),
                });
            }
        }

        serde_wasm_bindgen::to_value(&Snapshot {
            tick,
            entities,
            blueprints,
            stacks,
            missions,
            fame,
            recruits,
            research,
            topics,
            crafting,
            recipes,
            techs,
            notes,
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
