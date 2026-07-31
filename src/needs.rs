//! Потребности: усталость и отдых (§12.20 concept.md).
//!
//! Бодрость тратится по очку за тик бодрствования и возвращается сном. Отдых —
//! **задача общего слоя** (`Rest`), а не множитель скорости: скорость уже
//! принадлежит навыку (§12.17), а задача видна в поведении — кот встал и ушёл
//! спать. Это первая задача, которую кот назначает себе сам; все четыре
//! остальные приходят от разметки игрока.
//!
//! Правил два, и у каждого своя работа:
//!   * `assign_rest` — уставший **и свободный** кот идёт к лежанке. Начатое дело
//!     он при этом не бросает, как не бросает его и по приказу игрока (§12.15).
//!   * `collapse_exhausted` — на нуле бодрости кот валится там, где стоит, и
//!     задачу отпускает. Без этого при бесконечной работе он не поспал бы
//!     никогда, а лежанки остались бы украшением.
//!
//! Фаза отдыха отдельно не хранится: `Rest` с маршрутом — идёт спать, `Rest`
//! без маршрута — уже спит (так же устроен `Haul`).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::release_claim;
use crate::map::BaseMap;
use crate::path::Reach;

/// Бодрости тратится за тик бодрствования.
const TIRE_PER_TICK: i32 = 1;

/// Кот на нуле бодрости засыпает где стоит, отпустив задачу.
///
/// Чертёж и пометку кучи надо освободить явно: иначе площадка останется
/// «занятой» спящим и её больше никто не возьмёт.
///
/// А вот из **отряда** истощённый не выпадает: состав выбрал игрок, заменить
/// бойца некем, и вылазка просто ждёт, пока тот выспится (§12.23). Это и есть
/// разница с приказом игрока — тот распускает отряд, потому что это решение,
/// а не случившееся. Ушедшего с базы истощение не берёт: вне базы он не устаёт.
pub(crate) fn collapse_exhausted(
    mut commands: Commands,
    cats: Query<
        (
            Entity,
            &Energy,
            Option<&Assignment>,
            Option<&Haul>,
            Option<&Researching>,
        ),
        (With<UnitId>, Without<Rest>, Without<Away>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut marks: Query<&mut ToStore>,
    mut topics: Query<&mut Research>,
) {
    for (cat_e, energy, assignment, haul, researching) in &cats {
        if energy.0 > 0 {
            continue;
        }
        if let Some(bp_e) = assignment.map(|a| a.0) {
            if let Ok(mut bp) = blueprints.get_mut(bp_e) {
                bp.assignee = None;
            }
        }
        match haul.map(|h| h.0) {
            Some(HaulTo::Site(bp_e)) => {
                if let Ok(mut bp) = blueprints.get_mut(bp_e) {
                    bp.hauler = None;
                }
            }
            Some(HaulTo::Store(pile)) => release_claim(&mut marks, pile),
            None => {}
        }
        if let Some(mut topic) = researching.and_then(|r| topics.get_mut(r.0).ok()) {
            topic.assignee = None;
            topic.spot = None;
        }
        // Груз кот не роняет: донесёт, когда выспится (§12.15). Лежанку он
        // при этом не занимает — падает где стоит, а не идёт к месту. А вот
        // парту отпускает: занятость держит сам `Study`, и уснувший ученик
        // держал бы её вечно (§12.18).
        commands
            .entity(cat_e)
            .insert(Rest { spot: None })
            .remove::<(Assignment, Haul, Study, Researching, Path, MoveCooldown)>();
    }
}

/// Отправляет уставших свободных котов к ближайшей лежанке.
///
/// Раздаётся **первым** из всех работ: порядок раздатчиков и есть приоритет
/// (§12.15), иначе уставшего кота тут же уводит подвоз материала.
///
/// **Лежанка занята, пока на неё идут или на ней спят.** Делить её нельзя:
/// место для сна — ресурс, и если его можно занимать вдвоём, число лежанок
/// перестаёт на что-либо влиять (§12.20). Занятость не хранится отдельным
/// claim — её держит сам `Rest` спящего кота, поэтому она снимается вместе с
/// задачей: просыпанием, приказом игрока, чем угодно.
///
/// Свободной лежанки нет или до неё не дойти — кот продолжает работать до нуля
/// бодрости, и тогда его подберёт `collapse_exhausted`. Это и есть цена базы
/// с недостроенной зоной отдыха: не запрет работать, а медленный сон на полу.
pub(crate) fn assign_rest(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut commands: Commands,
    resting: Query<(&Position, &Rest)>,
    free_cats: Query<
        (Entity, &Position, &Energy),
        (
            With<UnitId>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Squad>,
            Without<Path>,
        ),
    >,
) {
    let tired: Vec<(Entity, (i32, i32))> = free_cats
        .iter()
        .filter(|(_, _, energy)| energy.0 <= needs.tired)
        .map(|(e, pos, _)| (e, (pos.x, pos.y)))
        .collect();
    if tired.is_empty() {
        return;
    }

    // Занято и то, к чему идут, и то, на чём лежат: кот, свалившийся прямо на
    // лежанку, места в `Rest` не держит, но занимает его собой.
    let mut taken: Vec<(i32, i32)> = Vec::new();
    for (pos, rest) in &resting {
        taken.extend(rest.spot);
        if rules.rest_of(map.tile_at(pos.x, pos.y)) > 0 {
            taken.push((pos.x, pos.y));
        }
    }

    let mut free: Vec<(i32, i32)> = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| rules.rest_of(map.tile_at(x, y)) > 0)
        .filter(|cell| !taken.contains(cell))
        .collect();

    for (cat_e, from) in tired {
        if free.is_empty() {
            return; // свободных лежанок не осталось — работаем до упора
        }
        let reach = Reach::all(&map, from);
        let nearest = free
            .iter()
            .enumerate()
            .filter_map(|(i, &(x, y))| reach.dist_at(x, y).map(|d| (i, (x, y), d)))
            .min_by_key(|&(_, _, d)| d);
        let Some((i, cell, _)) = nearest else {
            continue; // до свободных лежанок не добраться
        };
        free.remove(i);
        let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
        commands.entity(cat_e).insert((
            Rest { spot: Some(cell) },
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

/// Спящие коты восстанавливают бодрость и просыпаются на полной.
///
/// Скорость даёт клетка под котом: лежанка — свою, всё остальное — общий
/// `floor` из рулсета. Ноль не берём никогда: спать можно где угодно, вопрос
/// только в том, сколько это займёт, — иначе кот, уснувший на голом полу
/// рулсета без `floor`, не проснулся бы вовсе.
pub(crate) fn sleep(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut commands: Commands,
    mut cats: Query<(Entity, &Position, &mut Energy, Option<&Path>), With<Rest>>,
) {
    for (cat_e, pos, mut energy, path) in &mut cats {
        if path.is_some() {
            continue; // ещё идёт к лежанке
        }
        let rate = rules
            .rest_of(map.tile_at(pos.x, pos.y))
            .max(needs.floor)
            .max(1);
        energy.0 = (energy.0 + rate).min(needs.max);
        if energy.0 >= needs.max {
            commands.entity(cat_e).remove::<Rest>();
        }
    }
}

/// Бодрствование стоит бодрости — одинаково за работу, ходьбу и простой.
/// Кроме тех, кто на миссии: вне базы усталость не считается вовсе (§12.22).
pub(crate) fn tire(mut cats: Query<&mut Energy, (With<UnitId>, Without<Rest>, Without<Away>)>) {
    for mut energy in &mut cats {
        energy.0 = (energy.0 - TIRE_PER_TICK).max(0);
    }
}
