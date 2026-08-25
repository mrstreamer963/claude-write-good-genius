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
use crate::crafting::spill_delivered;
use crate::hauling::spill;
use crate::jobs::WORK_RATE;
use crate::map::BaseMap;
use crate::path::Reach;
use crate::skills::{SKILL_SCIENCE, level_of};

/// Первая свободная клетка лаборатории по обходу карты; `None` — свободных нет.
///
/// Близнец `crafting::free_shop` (§12.96, §12.132): с §12.132 тема, как заказ и
/// сделка, рождается **в ячейке** и держит её до последнего очка работы. Обход
/// карты фиксирован, значит выбор детерминирован (§11).
///
/// Свободная функция, а не метод фасада, потому что зовут её двое —
/// `Sim::start_research` и снапшот. Правила-порога у науки нет и быть не может
/// (тема одноразова, «изучать до N» бессмысленно), так что третьего зова, как у
/// `free_shop`, здесь не появится.
///
/// Работает кот **стоя на клетке темы**, а не с соседней: лаборатория —
/// комната, а не стройплощадка, и правило соседства (§12.12) тут ни при чём.
pub(crate) fn free_lab(
    map: &BaseMap,
    tiles: &TileRules,
    taken: &[(i32, i32)],
) -> Option<(i32, i32)> {
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.is_lab(map.tile_at(x, y)))
        .find(|xy| !taken.contains(xy))
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
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
    free_cats: Query<
        (Entity, &UnitId, &Position, Option<&Skills>),
        (
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Crafting>,
            Without<Equipping>,
            Without<Eating>,
            Without<Healing>,
            Without<Treating>,
            Without<OnDuty>,
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    let science = skill_rules.index_of(SKILL_SCIENCE);
    for (topic_e, mut topic) in &mut topics {
        // Лабораторию снесли — тема уходит вместе с ней (§12.132), дословно как
        // заказ уходит со станком. Завезённый образец при этом **роняем кучей**
        // на клетку (§12.31, инвариант 8): материал не горит.
        if !tiles.is_lab(map.tile_at(topic.cell.0, topic.cell.1)) {
            spill_delivered(&mut commands, &mut stacks, topic.cell, &topic.delivered);
            if let Some(cat_e) = topic.assignee {
                commands
                    .entity(cat_e)
                    .remove::<(Researching, Path, MoveCooldown)>();
            }
            commands.entity(topic_e).despawn();
            continue;
        }
        if topic.assignee.is_some() {
            continue;
        }
        let Some(rule) = rules.0.get(topic.def) else {
            continue;
        };
        // Образец ещё везут — учёного не зовём (§12.133). Зеркало
        // `craft_supplied`: сперва носильщик, потом мастер (§12.15).
        if !topic_supplied(&rules, &topic) {
            continue;
        }

        // Допуск отсекает исполнителей, расстояние выбирает из оставшихся.
        // При равном расстоянии — по `id` кота, а не по порядку сущностей:
        // обход ECS зависит от истории вставок (§11), и та же пара котов после
        // загрузки сохранения решилась бы иначе.
        //
        // Идёт кот **на клетку своей темы**, а не на ближайшую лабораторию:
        // комнату выбрала сама тема (§12.132), и второй выбор здесь развёл бы
        // ячейку, которую тема держит, с той, где стоит учёный.
        let spot = topic.cell;
        let chosen = free_cats
            .iter()
            .filter(|(_, _, _, skills)| {
                science.map_or(0, |s| level_of(&skill_rules, *skills, s)) >= rule.level
            })
            .filter_map(|(cat_e, id, pos, _)| {
                let reach = Reach::all(&map, (pos.x, pos.y));
                reach
                    .dist_at(spot.0, spot.1)
                    .map(|steps| (steps, id.0.as_str(), cat_e, reach))
            })
            .min_by_key(|&(steps, id, ..)| (steps, id));
        let Some((_, _, cat_e, reach)) = chosen else {
            continue; // некому взяться или до лаборатории не дойти
        };

        topic.assignee = Some(cat_e);
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
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
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

        if (pos.x, pos.y) != topic.cell {
            // Кот ещё идёт — или его сбили с маршрута (сон, рана, приказ).
            // Тему при этом не трогаем: комната принадлежит ей (§12.132), а
            // снос уносит её целиком, и делает это раздатчик.
            if path.is_none() {
                topic.assignee = None;
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
            // Что вышло из образца, ложится **кучей на клетку темы** (§12.133,
            // инвариант 8) — как добыча на шлюз и готовое под ноги мастеру.
            // Дальше её разносит обычная уборка.
            for &(item, count) in &rule.gives {
                if count > 0 {
                    spill(&mut commands, &mut stacks, topic.cell, item, count);
                }
            }
            commands.entity(task.0).despawn();
            commands.entity(cat_e).remove::<Researching>();
        }
    }
}
