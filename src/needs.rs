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
pub(crate) fn collapse_exhausted(
    mut commands: Commands,
    cats: Query<
        (Entity, &Energy, Option<&Assignment>, Option<&Haul>),
        (With<UnitId>, Without<Rest>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut marks: Query<&mut ToStore>,
) {
    for (cat_e, energy, assignment, haul) in &cats {
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
        // Груз кот не роняет: донесёт, когда выспится (§12.15).
        commands
            .entity(cat_e)
            .insert(Rest)
            .remove::<(Assignment, Haul, Path, MoveCooldown)>();
    }
}

/// Отправляет уставших свободных котов к ближайшей лежанке.
///
/// Раздаётся **первым** из всех работ: порядок раздатчиков и есть приоритет
/// (§12.15), иначе уставшего кота тут же уводит подвоз материала.
///
/// Лежанки не резервируются — по той же причине, что и кучи лома (§12.15):
/// на POC лишняя ходка дешевле механики резервов, а спать вдвоём на одной
/// клетке коты друг другу не мешают (проходимость зависит только от тайлов).
///
/// Лежанки нет или до неё не дойти — кот продолжает работать до нуля бодрости,
/// и тогда его подберёт `collapse_exhausted`. Это и есть цена базы без зоны
/// отдыха: не запрет строить, а медленный сон на голом полу.
pub(crate) fn assign_rest(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut commands: Commands,
    free_cats: Query<
        (Entity, &Position, &Energy),
        (
            With<UnitId>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
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

    let beds: Vec<(i32, i32)> = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| rules.rest_of(map.tile_at(x, y)) > 0)
        .collect();
    if beds.is_empty() {
        return;
    }

    for (cat_e, from) in tired {
        let reach = Reach::all(&map, from);
        let nearest = beds
            .iter()
            .filter_map(|&(x, y)| reach.dist_at(x, y).map(|d| ((x, y), d)))
            .min_by_key(|&(_, d)| d);
        let Some((cell, _)) = nearest else {
            continue; // до лежанок не добраться — работаем до упора
        };
        let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Rest, Path { steps: path }, MoveCooldown(0)));
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
pub(crate) fn tire(mut cats: Query<&mut Energy, (With<UnitId>, Without<Rest>)>) {
    for mut energy in &mut cats {
        energy.0 = (energy.0 - TIRE_PER_TICK).max(0);
    }
}
