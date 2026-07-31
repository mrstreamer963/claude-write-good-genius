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
//!   * `launch()`        — отправить отряд на миссию; отряд наберётся сам;
//!   * `cancel_mission()`— распустить отряд, пока он ещё не ушёл с базы;
//!   * `demolish()`      — мгновенный снос без котов (тесты/отладка);
//!   * `set_target()`    — приказ коту идти в тайл (движение по тикам, отменяет его задачу);
//!   * `tick()`          — один фиксированный шаг симуляции;
//!   * `snapshot()`      — сущности, чертежи и кучи лома (каждый кадр).

use bevy_ecs::prelude::*;
use wasm_bindgen::prelude::*;

use crate::components::*;
use crate::jobs::BUILD_WORK;
use crate::map::{BaseMap, rect_cells};
use crate::missions::{outcome, pick_gate};
use crate::movement::{Busy, is_stuck};
use crate::path::find_path;
use crate::ruleset::{ItemDef, MissionDef, PerkDef, Ruleset, SkillDef, TileDef};
use crate::schedule::build_schedule;
use crate::skills::{SKILL_RAID, level_of};
use crate::snapshot::{
    BaseMapDto, BlueprintSnap, EntitySnap, MapMeta, MissionSnap, SkillSnap, Snapshot, StackSnap,
};

#[wasm_bindgen]
pub struct Sim {
    pub(crate) world: World,
    pub(crate) schedule: Schedule,
    pub(crate) palette: Vec<TileDef>,
    pub(crate) items: Vec<ItemDef>,
    pub(crate) skills: Vec<SkillDef>,
    pub(crate) perks: Vec<PerkDef>,
    pub(crate) missions: Vec<MissionDef>,
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

    /// Снять чертёж с клетки и освободить и строителя, и носильщика.
    /// Отмена плана мгновенна и бесплатна — строить ещё не начинали.
    ///
    /// Уже завезённый лом при отмене не пропадает и на пол не сыплется: он
    /// остаётся на руках у носильщика, если тот не успел его сдать, а сданный
    /// списывается вместе с чертежом — на POC это цена поспешной разметки.
    fn cancel_blueprint(&mut self, x: i32, y: i32) -> bool {
        let Some(e) = self.blueprint_at(x, y) else {
            return false;
        };
        let bp = self.world.get::<Blueprint>(e);
        let (assignee, hauler) = (bp.and_then(|b| b.assignee), bp.and_then(|b| b.hauler));
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
        // Груз кот при этом не бросает: донесёт, когда снова возьмётся за
        // доставку (§12.15). Сон снимается — это осознанное действие (§12.20).
        self.world
            .entity_mut(cat)
            .remove::<(Assignment, Haul, Rest, Path, MoveCooldown)>();
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
                })
                .collect(),
        ));
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
                })
                .collect(),
        ));
        world.insert_resource(AutoTidy(true));
        world.insert_resource(NeedRules {
            max: rs.energy.max,
            tired: rs.energy.tired,
            floor: rs.energy.floor,
        });
        world.insert_resource(SkillRules(
            rs.skills
                .iter()
                .map(|s| SkillRule {
                    id: s.id.clone(),
                    levels: s.levels.clone(),
                })
                .collect(),
        ));

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

        // Перк — статичный тег из рулсета; в числа он превращается один раз,
        // здесь: расти ему всё равно некуда (§12.17).
        for u in &rs.units {
            let hauler = u.perks.iter().any(|p| p == PERK_HAULER);
            let mut cat = world.spawn((
                UnitId(u.id.clone()),
                Renderable {
                    sprite: u.sprite.clone(),
                },
                Position {
                    x: u.pos[0],
                    y: u.pos[1],
                },
                Perks(u.perks.clone()),
            ));
            if rs.carry > 0 {
                cat.insert(Carry(rs.carry * if hauler { 2 } else { 1 }));
            }
            if rs.energy.max > 0 {
                cat.insert(Energy(rs.energy.max));
            }
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
            let mut q = self.world.query::<(
                &UnitId,
                &Renderable,
                &Position,
                Option<&Order>,
                Option<&Path>,
                Option<&Assignment>,
                Option<&Haul>,
                Option<&Carrying>,
                Option<&Carry>,
                Option<&Skills>,
                Option<&Perks>,
                Option<&Energy>,
                Option<&Rest>,
                Option<&Squad>,
                Option<&Away>,
            )>();
            let map = self.world.resource::<BaseMap>();
            let rules = self.world.resource::<SkillRules>();
            let needs = self.world.resource::<NeedRules>();
            for (
                id,
                r,
                p,
                order,
                path,
                assignment,
                haul,
                load,
                carry,
                skills,
                perks,
                energy,
                rest,
                squad,
                away,
            ) in q.iter(&self.world)
            {
                let busy = Busy::of(order, path, assignment, haul, rest, squad, away);
                entities.push(EntitySnap {
                    id: id.0.clone(),
                    sprite: r.sprite.clone(),
                    x: p.x,
                    y: p.y,
                    stuck: is_stuck(map, p, busy),
                    away: away.is_some(),
                    energy: energy.map_or(0, |e| e.0),
                    energy_max: needs.max,
                    // Спит, а не идёт спать: маршрут ещё есть — значит в пути.
                    sleeping: rest.is_some() && path.is_none(),
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
            let mut crew = self
                .world
                .query::<(&UnitId, &Squad, Option<&Away>, Option<&Skills>)>();
            let skill_rules = self.world.resource::<SkillRules>();
            // Вклад кота в силу отряда считается ровно как в `run_missions`:
            // сам он стоит единицу, уровень «Вылазки» — сверху.
            let members: Vec<(Entity, String, bool, i32)> = crew
                .iter(&self.world)
                .map(|(id, squad, away, skills)| {
                    let force = 1 + raid.map_or(0, |s| level_of(skill_rules, skills, s));
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

        serde_wasm_bindgen::to_value(&Snapshot {
            tick,
            entities,
            blueprints,
            stacks,
            missions,
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
