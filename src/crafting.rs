//! Производство: заказ → работа в мастерской → предмет под ноги (§12.30).
//!
//! Заказ — сущность `Craft`, то есть **разметка работы**, как чертёж и как тема:
//! игрок говорит «сделать столько-то», а исполнителя берёт симуляция (§12.16).
//! Новое здесь ровно одно — **повторяемость**: тема исчезает, сделанная один
//! раз, а рецепт крутится, пока не выйдет `left` штук. Очереди заказов нет,
//! счётчик и есть весь ответ на «сколько раз».
//!
//! Систем две, как у стройки и у науки:
//!   * `assign_craft` — раздатчик: ставит ближайшего свободного кота к ближайшей
//!     клетке мастерской. **Допуска по навыку нет** — рецепт открывает
//!     технология, а «Ремесло» только ускоряет (§12.14, §12.17);
//!   * `work_craft` — платит за штуку, набивает очки и вываливает готовое.
//!
//! **Платит склад за штуку, и в момент, когда за неё берутся** — здесь, а не в
//! фасаде: заказ на десять штук, оплаченный вперёд, заморозил бы склад под
//! работу, которая начнётся через полтысячи тиков (§12.30). Само правило
//! списания общее с наймом и наукой (`plan_spend`), чтобы две арифметики не
//! разошлись.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::{plan_spend, spill, storage_order};
use crate::jobs::WORK_RATE;
use crate::map::BaseMap;
use crate::path::Reach;
use crate::skills::{SKILL_CRAFT, level_of};

/// Ближайшая к коту клетка мастерской и её цена в шагах.
///
/// Работать кот будет **стоя на ней**, как в лаборатории: мастерская — комната,
/// а не стройплощадка, и правило соседства (§12.12) тут ничего не спасает.
fn shop_spot(map: &BaseMap, tiles: &TileRules, reach: &Reach) -> Option<((i32, i32), i32)> {
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.is_shop(map.tile_at(x, y)))
        .filter_map(|(x, y)| reach.dist_at(x, y).map(|d| ((x, y), d)))
        .min_by_key(|&(_, d)| d)
}

/// Ставит к заказу ближайшего свободного кота — если есть чем платить.
///
/// **Заказ без материала не раздаётся вовсе**, ровно как чертёж, на площадку
/// которого не завезли лом (§12.15): он не ошибка и не отказ, он ждёт. Проверка
/// идёт только для неоплаченной штуки — начатую бросать не за что.
///
/// Стоит после науки и до стройки: за тему уже заплачено образцами, а заказ
/// игрок разметил явно, тогда как чертёж на базе есть почти всегда (§12.26).
pub(crate) fn assign_craft(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<CraftRules>,
    mut commands: Commands,
    mut orders: Query<(Entity, &mut Craft)>,
    stacks: Query<(Entity, &Position, &Stack)>,
    free_cats: Query<
        (Entity, &Position),
        (
            With<UnitId>,
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
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    for (order_e, mut order) in &mut orders {
        if order.assignee.is_some() || order.left <= 0 {
            continue;
        }
        let Some(rule) = rules.0.get(order.def) else {
            continue;
        };
        if !order.paid {
            let piles = storage_order(
                &map,
                &tiles,
                stacks
                    .iter()
                    .map(|(e, p, s)| (e, (p.x, p.y), s.item, s.count)),
            );
            if plan_spend(&piles, &rule.cost).is_none() {
                continue; // склад пуст — заказ ждёт материала, а кот работает
            }
        }

        let chosen = free_cats
            .iter()
            .filter_map(|(cat_e, pos)| {
                let reach = Reach::all(&map, (pos.x, pos.y));
                shop_spot(&map, &tiles, &reach).map(|(spot, steps)| (steps, cat_e, spot, reach))
            })
            .min_by_key(|&(steps, ..)| steps);
        let Some((_, cat_e, spot, reach)) = chosen else {
            continue; // некому взяться или до мастерской не дойти
        };

        order.assignee = Some(cat_e);
        order.spot = Some(spot);
        let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Crafting(order_e), Path { steps: path }, MoveCooldown(0)));
    }
}

/// Мастера у верстака: списывают материал, набивают очки и вываливают готовое.
///
/// Готовая штука **ложится кучей под ноги**, а не на склад: работа кончается
/// там, где стоял работник, — то же правило, что у возврата от сноса (§12.11) и
/// у добычи с вылазки (§12.22). Разносит её обычная уборка (§12.16).
///
/// Скорость — `WORK_RATE` плюс уровень «Ремесла», и сам навык при этом растёт:
/// здесь только маркер `Worked`, опыт начисляет `train_skills` (§12.17).
pub(crate) fn work_craft(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<CraftRules>,
    skill_rules: Res<SkillRules>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Crafting, Option<&Path>, Option<&Skills>)>,
    mut orders: Query<&mut Craft>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    let craft = skill_rules.index_of(SKILL_CRAFT);
    for (cat_e, pos, task, path, skills) in &cats {
        let Ok(mut order) = orders.get_mut(task.0) else {
            commands.entity(cat_e).remove::<Crafting>();
            continue;
        };
        let Some(rule) = rules.0.get(order.def).cloned() else {
            continue;
        };

        if !tiles.is_shop(map.tile_at(pos.x, pos.y)) {
            // Мастерскую могли снести, пока кот шёл, — тогда маршрут кончился
            // не там. Отпускаем заказ: раздатчик подыщет другую комнату, а нет
            // её — заказ просто ждёт, как ждёт чертёж без материала.
            if path.is_none() {
                order.assignee = None;
                order.spot = None;
                commands.entity(cat_e).remove::<Crafting>();
            }
            continue;
        }

        if !order.paid {
            let piles = storage_order(
                &map,
                &tiles,
                stacks
                    .iter()
                    .map(|(e, p, s)| (e, (p.x, p.y), s.item, s.count)),
            );
            match plan_spend(&piles, &rule.cost) {
                Some(takes) => {
                    for (pile_e, taken) in takes {
                        if let Ok((_, _, mut stack)) = stacks.get_mut(pile_e) {
                            stack.count -= taken;
                            if stack.count <= 0 {
                                commands.entity(pile_e).despawn();
                            }
                        }
                    }
                    order.paid = true;
                }
                None => {
                    // Материал разобрали, пока мастер шёл. Отпускаем его на
                    // другую работу: стоять у верстака в ожидании — это ровно
                    // то, чего §12.15 избегает у чертежей.
                    order.assignee = None;
                    order.spot = None;
                    commands
                        .entity(cat_e)
                        .remove::<(Crafting, Path, MoveCooldown)>();
                    continue;
                }
            }
        }

        let level = craft.map_or(0, |s| level_of(&skill_rules, skills, s));
        order.progress += WORK_RATE + level;
        if let Some(skill) = craft {
            commands.entity(cat_e).insert(Worked(skill));
        }
        if order.progress < rule.work {
            continue;
        }

        for &(item, count) in &rule.gives {
            spill(&mut commands, &mut stacks, (pos.x, pos.y), item, count);
        }
        order.left -= 1;
        order.progress = 0;
        order.paid = false;
        // Штука готова — и это единственное место, где заказ кончается сам.
        if order.left <= 0 {
            commands.entity(task.0).despawn();
            commands.entity(cat_e).remove::<Crafting>();
        }
    }
}
