//! Исследование образцов: тема → работа в лаборатории → технология (§12.26).
//!
//! Тема — сущность `Research`, то есть **разметка работы**, как чертёж: игрок
//! говорит «изучаем это», а исполнителя берёт симуляция (§12.16). Новое здесь
//! ровно одно — **допуск**: тему берёт не любой свободный кот, а только тот,
//! кому хватает «Науки» (§12.18). Среди допущенных выбор прежний, по числу
//! шагов (§12.14): навык решает «можно ли», а не «кто лучше».
//!
//! Систем две, ровно как у стройки:
//!   * `assign_research` — раздатчик: сажает ближайшего допущенного кота за
//!     ближайшую к нему клетку лаборатории;
//!   * `work_research` — набивает очки работы и на выходе записывает технологию.
//!
//! Оплата темы (образцы со склада) снимается в момент заявки, в фасаде: это
//! решение игрока, а не работа котов, — там же, где платит найм (§12.24).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::WORK_RATE;
use crate::map::BaseMap;
use crate::path::Reach;
use crate::skills::{SKILL_SCIENCE, level_of};

/// Ближайшая к коту клетка лаборатории и её цена в шагах.
///
/// Работать кот будет **стоя на ней**, а не с соседней: лаборатория — комната,
/// а не стройплощадка, и правило соседства (§12.12) здесь ничего не спасает.
fn lab_spot(map: &BaseMap, tiles: &TileRules, reach: &Reach) -> Option<((i32, i32), i32)> {
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.is_lab(map.tile_at(x, y)))
        .filter_map(|(x, y)| reach.dist_at(x, y).map(|d| ((x, y), d)))
        .min_by_key(|&(_, d)| d)
}

/// Сажает за тему ближайшего свободного кота, которому хватает «Науки».
///
/// Раздатчик стоит **до стройки**: за тему уже заплачено образцами, и вставшая
/// наука — это потраченный ресурс, лежащий без движения, тогда как чертёж
/// подождёт (и почти всегда есть). Это же и есть специализация из §12.17:
/// котовремя конечно, и учёный не строит.
pub(crate) fn assign_research(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<ResearchRules>,
    skill_rules: Res<SkillRules>,
    mut commands: Commands,
    mut topics: Query<(Entity, &mut Research)>,
    free_cats: Query<
        (Entity, &Position, Option<&Skills>),
        (
            With<UnitId>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Crafting>,
            Without<Equipping>,
            Without<Squad>,
            Without<Path>,
        ),
    >,
) {
    let science = skill_rules.index_of(SKILL_SCIENCE);
    for (topic_e, mut topic) in &mut topics {
        if topic.assignee.is_some() {
            continue;
        }
        let Some(rule) = rules.0.get(topic.def) else {
            continue;
        };

        // Допуск отсекает исполнителей, расстояние выбирает из оставшихся.
        let chosen = free_cats
            .iter()
            .filter(|(_, _, skills)| {
                science.map_or(0, |s| level_of(&skill_rules, *skills, s)) >= rule.level
            })
            .filter_map(|(cat_e, pos, _)| {
                let reach = Reach::all(&map, (pos.x, pos.y));
                lab_spot(&map, &tiles, &reach).map(|(spot, steps)| (steps, cat_e, spot, reach))
            })
            .min_by_key(|&(steps, ..)| steps);
        let Some((_, cat_e, spot, reach)) = chosen else {
            continue; // некому взяться или до лаборатории не дойти
        };

        topic.assignee = Some(cat_e);
        topic.spot = Some(spot);
        let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
        commands.entity(cat_e).insert((
            Researching(topic_e),
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

/// Коты, добравшиеся до лаборатории, набивают очки темы; на выходе — технология.
///
/// Скорость — `WORK_RATE` плюс уровень «Науки», и сам навык при этом растёт:
/// здесь только маркер `Worked`, опыт начисляет `train_skills` (§12.17). Так
/// исследование становится вторым источником навыка после парты — тем самым
/// «мастерство из домена», ради которого парта и остановлена на пороге.
pub(crate) fn work_research(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<ResearchRules>,
    skill_rules: Res<SkillRules>,
    mut techs: ResMut<Techs>,
    mut commands: Commands,
    cats: Query<(
        Entity,
        &Position,
        &Researching,
        Option<&Path>,
        Option<&Skills>,
    )>,
    mut topics: Query<&mut Research>,
) {
    let science = skill_rules.index_of(SKILL_SCIENCE);
    for (cat_e, pos, task, path, skills) in &cats {
        let Ok(mut topic) = topics.get_mut(task.0) else {
            commands.entity(cat_e).remove::<Researching>();
            continue;
        };
        let Some(rule) = rules.0.get(topic.def).cloned() else {
            continue;
        };

        if !tiles.is_lab(map.tile_at(pos.x, pos.y)) {
            // Лабораторию могли снести, пока кот шёл, — тогда маршрут кончился
            // не там. Отпускаем тему: раздатчик подыщет другую комнату, а нет
            // её — тема просто ждёт, как ждёт чертёж без материала.
            if path.is_none() {
                topic.assignee = None;
                topic.spot = None;
                commands.entity(cat_e).remove::<Researching>();
            }
            continue;
        }

        let level = science.map_or(0, |s| level_of(&skill_rules, skills, s));
        topic.progress += WORK_RATE + level;
        if let Some(skill) = science {
            commands.entity(cat_e).insert(Worked(skill));
        }
        if topic.progress >= rule.work {
            // Технология только записывается: тратить её нельзя, как нельзя
            // тратить известность (§12.24). Ворота, которые закрываются,
            // читаются как поломка.
            if !techs.knows(&rule.id) {
                techs.0.push(rule.id.clone());
            }
            commands.entity(task.0).despawn();
            commands.entity(cat_e).remove::<Researching>();
        }
    }
}
