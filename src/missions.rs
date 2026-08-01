//! Вылазки: отряд уходит с базы и возвращается с добычей (§12.22, §12.23
//! concept.md).
//!
//! **Отряд выбирает игрок, поимённо** (§12.23) — единственная работа, где
//! исполнитель не раздаётся симуляцией. Причина ровно одна: от состава зависит
//! исход, а всё остальное на базе одинаково выполнимо любым котом (§12.16).
//!
//! Систем две:
//!   * `gather_squad` — держит выбранный отряд идущим к шлюзу, переживая
//!     изменения карты. Состав не трогает: заменить выбывшего некем.
//!   * `run_missions` — отправляет собравшийся отряд, крутит таймер, считает
//!     исход и возвращает котов с добычей.
//!
//! Отдельного состояния «фаза миссии» нет, как его нет у `Haul` и `Rest`: где
//! отряд, видно по компонентам. `Squad` с маршрутом — кот идёт к шлюзу,
//! `Squad` без маршрута на шлюзе — ждёт остальных, `Away` — ушёл.
//!
//! **Добыча ложится кучей на шлюз**, а не в лапы котам: кот везёт один тип за
//! ходку (§12.21), а добыча бывает набором. Дальше её разносит обычная уборка
//! (§12.16) — и то, что миссия ничего не знает про склад, тут не упрощение,
//! а прямое следствие: возврат от сноса ложится под ноги ровно так же.
//!
//! **Исход детерминирован** (`outcome`): сила отряда против сложности вылазки,
//! без броска кубика. Плата за вылазку — бодрость: та же валюта котовремени, в
//! которой измеряется и сама отправка отряда (§12.23).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::spill;
use crate::map::BaseMap;
use crate::path::{Reach, find_path};
use crate::skills::{SKILL_RAID, level_of};

/// Все клетки-шлюзы карты, в порядке обхода: он фиксирован, значит выбор
/// шлюза детерминирован (§11).
fn gate_cells<'a>(map: &'a BaseMap, rules: &'a TileRules) -> impl Iterator<Item = (i32, i32)> + 'a {
    (0..map.height)
        .flat_map(move |y| (0..map.width).map(move |x| (x, y)))
        .filter(move |&(x, y)| rules.is_gate(map.tile_at(x, y)))
}

/// Чем кончится вылазка. Считается и на возвращении, и каждый кадр для панели:
/// прогноз и результат обязаны быть одним и тем же выражением, иначе игрок
/// увидит одно, а получит другое.
#[derive(Clone, Copy)]
pub(crate) struct Outcome {
    pub(crate) strength: i32,
    /// Какая доля добычи достанется, в процентах (0..=100).
    pub(crate) share: i32,
    /// Провал: силы не хватило даже вполовину.
    pub(crate) failed: bool,
}

/// Исход по силе отряда и сложности вылазки — **без броска кубика** (§12.23).
///
/// Детерминизм здесь не формальность: он единственное, что делает выбор отряда
/// читаемым. С кубиком игрок не отличил бы «выбрал слабый отряд» от «не
/// повезло», а вылазок за сеанс столько, что вероятность так и не проступит.
///
/// Хватило силы — вся добыча; не хватило — её доля; вдвое меньше нужного —
/// провал: ни добычи, ни сил. Нулевая сложность удаётся всегда, по общему
/// правилу нулей в рулсете (цена тайла, ёмкость склада, потолок бодрости).
pub(crate) fn outcome(danger: i32, strength: i32) -> Outcome {
    if danger <= 0 {
        return Outcome {
            strength,
            share: 100,
            failed: false,
        };
    }
    let failed = strength * 2 < danger;
    Outcome {
        strength,
        share: if failed {
            0
        } else {
            (strength * 100 / danger).min(100)
        },
        failed,
    }
}

/// Держит выбранный игроком отряд идущим к шлюзу.
///
/// Состав здесь не трогается: его назначил `Sim::launch` в момент заявки, и
/// заменить выбывшего некем — это выбор игрока, а не раздача работы (§12.23).
/// Система нужна ровно для того, чтобы маршрут переживал изменения карты:
/// шлюз снесли, кота выбросило из ямы, боец проснулся после истощения.
/// Тот же случай, что `retry_orders` у приказа игрока.
pub(crate) fn gather_squad(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    crew: Query<
        (Entity, &Squad, &Position, Option<&Path>, Option<&Away>),
        // Спящего не трогаем: маршрут разбудил бы его, а истощение — не повод
        // гнать кота дальше. Проснётся — эта же система его и подберёт. Идущего
        // за снаряжением — тем более: одетым он уйдёт сильнее (§12.29, §12.34).
        (Without<Rest>, Without<Equipping>),
    >,
) {
    if missions.is_empty() {
        return;
    }
    let map = &*map;

    for (mission_e, mut mission) in &mut missions {
        // Кто в отряде и где он. Пустой маршрут считается пройденным:
        // `move_units` снимет его только следующим тиком.
        let mut left_base = false;
        let squad: Vec<(Entity, (i32, i32), bool)> = crew
            .iter()
            .filter(|(_, s, ..)| s.0 == mission_e)
            .inspect(|(.., away)| left_base |= away.is_some())
            .map(|(e, _, p, path, _)| {
                let walking = path.is_some_and(|p| !p.steps.is_empty());
                (e, (p.x, p.y), walking)
            })
            .collect();
        // Отряд ушёл — шлюз больше не пересматривается: вернутся коты туда,
        // откуда ушли, даже если гараж успели снести.
        if left_base {
            continue;
        }

        // Шлюз мог уйти под ластиком, пока отряд собирался, — выбираем заново.
        if mission
            .gate
            .is_some_and(|(x, y)| !tiles.is_gate(map.tile_at(x, y)))
        {
            mission.gate = None;
        }
        if mission.gate.is_none() {
            let at: Vec<(i32, i32)> = squad.iter().map(|&(_, at, _)| at).collect();
            mission.gate = pick_gate(map, &tiles, &at);
        }
        let Some(gate) = mission.gate else {
            continue; // шлюза нет или до него не добраться всем разом
        };

        for &(cat_e, at, walking) in &squad {
            if walking || at == gate {
                continue;
            }
            if let Some(steps) = find_path(map, at, gate) {
                commands
                    .entity(cat_e)
                    .insert((Path { steps }, MoveCooldown(0)));
            }
        }
    }
}

/// Шлюз, к которому отряду суммарно ближе всего идти.
///
/// Клетки, до которых дойдут не все, отбрасываются: состав фиксирован, и шлюз,
/// отрезанный от одного из бойцов, значит вылазку, которая никогда не тронется.
/// Ничьи разрешает порядок обхода карты, то есть детерминированно.
pub(crate) fn pick_gate(map: &BaseMap, tiles: &TileRules, at: &[(i32, i32)]) -> Option<(i32, i32)> {
    let reaches: Vec<Reach> = at.iter().map(|&p| Reach::all(map, p)).collect();
    gate_cells(map, tiles)
        .filter_map(|(x, y)| {
            let mut total = 0;
            for r in &reaches {
                total += r.dist_at(x, y)?;
            }
            Some((total, (x, y)))
        })
        .min_by_key(|&(total, _)| total)
        .map(|(_, cell)| cell)
}

/// Отправляет собравшийся отряд, крутит таймер и возвращает котов с добычей.
///
/// Стоит после `move_units`: кот, шагнувший на шлюз в этом тике, засчитывается
/// сразу, — и до `settle_stacks`, чтобы добыча, вывалившаяся в свежую яму,
/// съехала на пол тем же тиком (§12.15).
pub(crate) fn run_missions(
    rules: Res<MissionRules>,
    skill_rules: Res<SkillRules>,
    items: Res<ItemRules>,
    mut fame: ResMut<Fame>,
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    mut crew: Query<(
        Entity,
        &Squad,
        &Position,
        Option<&Path>,
        Option<&Away>,
        Option<&Skills>,
        Option<&mut Energy>,
        Option<&Gear>,
    )>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    let raid = skill_rules.index_of(SKILL_RAID);
    for (mission_e, mut mission) in &mut missions {
        let Some(rule) = rules.0.get(mission.def) else {
            continue;
        };
        let Some(gate) = mission.gate else {
            continue; // шлюз ещё не выбран
        };

        let squad: Vec<(Entity, (i32, i32), bool, bool, i32)> = crew
            .iter()
            .filter(|(_, s, ..)| s.0 == mission_e)
            .map(|(e, _, p, path, away, skills, _, gear)| {
                let walking = path.is_some_and(|p| !p.steps.is_empty());
                // Вклад кота в силу отряда: сам он стоит единицу, навык —
                // сверху, надетое — ещё сверху. Нулевой навык поэтому не значит
                // «бесполезен», а снаряжение — второе слагаемое, которое растёт
                // не от навыка и не упирается в его потолок (§12.29).
                let force = 1
                    + raid.map_or(0, |s| level_of(&skill_rules, skills, s))
                    + items.force_of_gear(gear);
                (e, (p.x, p.y), walking, away.is_some(), force)
            })
            .collect();

        // Отряд в поле. База о нём ничего не знает: ни усталости, ни маршрутов —
        // вылазка считается разом по возвращении, а не симулируется (§12.22).
        if squad.iter().any(|&(.., away, _)| away) {
            // Опыт капает за тик в поле, как и на любой другой работе (§12.17):
            // начисляет его `train_skills` в конце цепочки, здесь только маркер.
            if let Some(skill) = raid {
                for &(cat_e, ..) in &squad {
                    commands.entity(cat_e).insert(Worked(skill));
                }
            }
            mission.left -= 1;
            if mission.left > 0 {
                continue;
            }

            // Исход считаем по силе **на возвращении**: за вылазку навык вырос,
            // и отнимать этот рост у самой вылазки было бы странно.
            let force = squad.iter().map(|&(.., f)| f).sum();
            let out = outcome(rule.danger, force);
            for &(cat_e, ..) in &squad {
                commands.entity(cat_e).remove::<(Away, Squad)>();
                // Плата за вылазку — котовремя: та же валюта, в которой
                // измеряется и сама отправка отряда. Провал забирает всё, и
                // коты валятся у шлюза — `collapse_exhausted` подберёт их.
                if let Ok((.., Some(mut energy), _)) = crew.get_mut(cat_e) {
                    let toll = if out.failed { energy.0 } else { rule.toll };
                    energy.0 = (energy.0 - toll).max(0);
                }
                // Провал ломает снаряжение: до него он стоил только бодрости, а
                // она восстанавливается бесплатно — то есть заведомо провальная
                // вылазка была способом качать «Вылазку» за одно лишь время
                // (§12.29). Успех не изнашивает: износ за каждый выход
                // превратил бы петлю «добыча → сила» в оброк. Комплект наберётся
                // заново, как только на складе снова будет из чего.
                if out.failed {
                    commands.entity(cat_e).remove::<Gear>();
                }
            }
            // Добыча ложится кучей на шлюз — ровно как возврат от сноса ложится
            // под ноги сносильщику. Развозит её обычная уборка (§12.16).
            for &(item, count) in &rule.loot {
                let got = count * out.share / 100;
                if got > 0 {
                    spill(&mut commands, &mut stacks, gate, item, got);
                }
            }
            // Известность идёт той же долей, что и добыча: слухи расходятся по
            // сделанному, а не по задуманному. Провал не приносит ничего — но и
            // не отнимает: ворота, которые закрываются, читаются как поломка
            // (§12.24).
            fame.0 += rule.fame * out.share / 100;
            commands.entity(mission_e).despawn();
            continue;
        }

        // Уходим, только когда отряд в полном составе стоит на шлюзе: недобор
        // здесь — это не «пошли вдвоём вместо троих», а «ещё идут».
        let ready = squad.len() == rule.squad
            && squad
                .iter()
                .all(|&(_, at, walking, ..)| at == gate && !walking);
        if ready {
            for &(cat_e, ..) in &squad {
                commands.entity(cat_e).insert(Away);
            }
        }
    }
}
