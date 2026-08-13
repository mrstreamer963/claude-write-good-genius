//! Производство: заказ → работа в мастерской → предмет под ноги (§12.30).
//!
//! Заказ — сущность `Craft`, то есть **разметка работы**, как чертёж и как тема:
//! игрок говорит «сделать столько-то», а исполнителя берёт симуляция (§12.16).
//! Новое здесь ровно одно — **повторяемость**: тема исчезает, сделанная один
//! раз, а рецепт крутится, пока не выйдет `left` штук. Очереди заказов нет,
//! счётчик и есть весь ответ на «сколько раз».
//!
//! Систем три, и первая из них — не про работу, а про повод для неё:
//!   * `plan_craft` — правило-порог: держит на базе не меньше заданного числа
//!     штук, заводя **обычный** заказ, когда просело (§12.64, §12.65);
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
use crate::hauling::{on_base_counts, plan_spend, spill, storage_order};
use crate::jobs::WORK_RATE;
use crate::map::BaseMap;
use crate::path::Reach;
use crate::skills::{SKILL_CRAFT, level_of};

/// Ближайшая к коту **свободная** клетка мастерской и её цена в шагах.
///
/// Работать кот будет **стоя на ней**, как в лаборатории: мастерская — комната,
/// а не стройплощадка, и правило соседства (§12.12) тут ничего не спасает.
///
/// `taken` — станки, за которыми уже сидят другие заказы (§12.55). Мастерская
/// это слот, и второй заказ на неё не сядет; при равном расстоянии выбираем
/// клетку по координате, а не по порядку обхода карты — он-то как раз
/// детерминирован, но ключ должен быть полным (§12.14).
fn shop_spot(
    map: &BaseMap,
    tiles: &TileRules,
    reach: &Reach,
    taken: &[(i32, i32)],
) -> Option<((i32, i32), i32)> {
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.is_shop(map.tile_at(x, y)))
        .filter(|xy| !taken.contains(xy))
        .filter_map(|(x, y)| reach.dist_at(x, y).map(|d| ((x, y), d)))
        .min_by_key(|&((x, y), d)| (d, y, x))
}

/// Сколько штук рецепта не хватает, чтобы выйти на порог: `min` меряется по
/// **каждому** предмету, который рецепт даёт, и берётся худший (§12.65).
///
/// У боевых рецептов выход один, и правило читается буквально — «держать пять
/// деталей». Набор из двух предметов пришёл бы к тому же: заказ повторяют, пока
/// хоть одного из выходов недостаёт.
fn pieces_needed(rule: &CraftRule, have: &[i32], owed: &[(usize, i32)], min: i32) -> i32 {
    rule.gives
        .iter()
        .filter(|&&(_, per)| per > 0)
        .map(|&(item, per)| {
            let promised = owed
                .iter()
                .find(|&&(i, _)| i == item)
                .map_or(0, |&(_, n)| n);
            let short = min - (have.get(item).copied().unwrap_or(0) - promised);
            // Вверх: половина детали порога не закрывает.
            short.div_euclid(per) + i32::from(short.rem_euclid(per) > 0)
        })
        .max()
        .unwrap_or(0)
        .max(0)
}

/// Правило-порог: держать на базе не меньше `Stocking` штук того, что даёт
/// рецепт (§12.65).
///
/// **Ставит ту же разметку, что и игрок**, — обычный `Craft`, только с пометкой
/// `auto`. Ни новой задачи у кота, ни второго способа делать предметы: правило
/// заменяет повторный клик, а не механику (§12.64).
///
/// Считается **всё добро базы за вычетом обещанного покупателю**, а не склад:
/// готовое ложится кучей под ноги мастеру (§12.30), и склад узнаёт о нём только
/// после уборки — порог по складу штамповал бы лишнее всё время, пока носильщик
/// идёт. Платит при этом по-прежнему склад (`plan_spend` в `work_craft`):
/// «сколько есть» и «чем заплатить» — разные вопросы (§12.24, §12.53).
///
/// **Пересчитывается каждый тик**, и отсюда вся отмена: порог, поднятый
/// игроком, дотягивает `left`; порог, снятый в ноль, снимает заказ сам. Руками
/// мир при этом не трогает никто — `Sim::set_stock` только пишет число.
/// Единственная оговорка — **начатую штуку правило не отбирает**: материал за
/// неё уже списан, и отменить её значило бы сжечь его молча (§12.26).
///
/// Чужого не трогает: ручной заказ на тот же рецепт правило не ведёт и второго
/// рядом не заводит — на одну разметку один источник (§12.64).
pub(crate) fn plan_craft(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<CraftRules>,
    techs: Res<Techs>,
    stocking: Res<Stocking>,
    mut commands: Commands,
    mut orders: Query<(Entity, &mut Craft)>,
    stacks: Query<&Stack>,
    loads: Query<&Carrying>,
    deals: Query<&Deal>,
) {
    // Дешёвый выход для мира без правил (все тесты чужих механик и начало
    // партии). **Заказы правила он обязан учитывать**: снятый порог — это ноль,
    // и по одним только порогам система вышла бы отсюда, не убрав за собой тот
    // самый заказ, который игрок и отменял.
    if stocking.0.iter().all(|&min| min <= 0) && !orders.iter().any(|(_, o)| o.auto) {
        return;
    }
    let have = on_base_counts(stacks.iter(), loads.iter());
    // **`owed`, а не `booked`**: `have` считает и лапы, а из брони они вычтены
    // (§12.50). Со скидкой порог засчитывал бы своим то, что кот уже несёт
    // покупателю, и не дозаказывал взамен проданного.
    let owed = crate::trade::owed(deals.iter());

    // Станок — слот заказа (§12.55). Отдельной проверки правилу не нужно: оно
    // заводит ту же разметку, у которой ограничение уже есть, — просто молчит,
    // когда свободного станка нет, как молчит кнопка у игрока.
    let shops = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.is_shop(map.tile_at(x, y)))
        .count();
    let mut running = orders.iter().count();

    // Обход по индексу рецепта, а не по сущностям: порядок обхода ECS зависит
    // от истории вставок (§11), а палитра — нет.
    for def in 0..stocking.0.len() {
        let min = stocking.min_of(def);
        let Some(rule) = rules.0.get(def) else {
            continue;
        };
        // Рецепт, закрытый технологией, не существует (§12.27) — порог на нём
        // ждёт науки, как ждал бы кнопки.
        if !techs.covers(&rule.requires) {
            continue;
        }
        let want = if min > 0 {
            pieces_needed(rule, &have, &owed, min)
        } else {
            0
        };

        match orders.iter_mut().find(|(_, o)| o.def == def) {
            Some((order_e, mut order)) => {
                if !order.auto {
                    continue; // ручной заказ ведёт игрок
                }
                // Оплаченная штука доделывается в любом случае: материал за неё
                // уже списан.
                let left = want.max(i32::from(order.paid));
                if left <= 0 {
                    if let Some(cat_e) = order.assignee {
                        commands
                            .entity(cat_e)
                            .remove::<(Crafting, Path, MoveCooldown)>();
                    }
                    commands.entity(order_e).despawn();
                    running -= 1;
                } else if order.left != left {
                    order.left = left;
                }
            }
            None => {
                if want <= 0 || running >= shops {
                    continue;
                }
                // Заказ сразу на всю недостачу, а не поштучно: N заказов по
                // штуке заняли бы станки по очереди и мигали бы в панели.
                commands.spawn(Craft {
                    def,
                    left: want,
                    progress: 0,
                    paid: false,
                    assignee: None,
                    spot: None,
                    auto: true,
                });
                running += 1;
            }
        }
    }
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
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
    free_cats: Query<
        (Entity, &UnitId, &Position),
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
    // Забронированное под продажу заказу не принадлежит (§12.50).
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());

    // Станки, за которыми уже сидят другие заказы (§12.55). Занятость держит сам
    // `Craft::spot`, ровно как лежанку держит `Rest::spot`: снимается вместе с
    // задачей, отдельного реестра нет и заводить его не надо.
    let mut taken: Vec<(i32, i32)> = orders.iter().filter_map(|(_, o)| o.spot).collect();

    // Заказы, которые сейчас можно раздать. Собираем списком, а не раздаём на
    // ходу, потому что заказов теперь несколько: `commands` отложены до конца
    // тика, и на втором витке цикла тот же кот и тот же станок выглядели бы
    // свободными (§12.55).
    let mut open: Vec<(Entity, usize)> = Vec::new();
    for (order_e, order) in orders.iter() {
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
            if plan_spend(&piles, &rule.cost, &booked).is_none() {
                continue; // склад пуст — заказ ждёт материала, а кот работает
            }
        }
        open.push((order_e, order.def));
    }
    if open.is_empty() {
        return;
    }
    // Порядок обхода ECS зависит от истории вставок (§11), а жадный выбор ниже
    // при равенстве берёт первый заказ. Сортируем по рецепту: двух заказов на
    // один рецепт не бывает по построению, значит ключ полный.
    open.sort_unstable_by_key(|&(_, def)| def);

    // Достижимость считаем по разу на кота, коты — по `id` (§12.14, §11).
    let mut idle: Vec<(&str, Entity, Reach)> = free_cats
        .iter()
        .map(|(e, id, p)| (id.0.as_str(), e, Reach::all(&map, (p.x, p.y))))
        .collect();
    idle.sort_unstable_by_key(|&(id, ..)| id);

    // Пара выбирается только между котом и станком: **какой именно заказ он
    // возьмёт, не влияет ни на что** — станки одинаковы, и любой кот делает
    // любой рецепт с одной скоростью (навык домена общий, §12.17). Поэтому
    // заказ берём первый по рецепту, а минимизируем расстояние до верстака.
    while !idle.is_empty() && !open.is_empty() {
        let chosen = idle
            .iter()
            .enumerate()
            .filter_map(|(ci, (_, _, reach))| {
                shop_spot(&map, &tiles, reach, &taken).map(|(spot, steps)| (steps, ci, spot))
            })
            .min_by_key(|&(steps, ci, _)| (steps, ci));
        // Ни один кот не дошёл до свободного станка — остальные заказы ждут.
        let Some((_, ci, spot)) = chosen else {
            break;
        };

        let (_, cat_e, reach) = idle.remove(ci);
        let (order_e, _) = open.remove(0);
        taken.push(spot);
        if let Ok((_, mut order)) = orders.get_mut(order_e) {
            order.assignee = Some(cat_e);
            order.spot = Some(spot);
        }
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
    mut crafted: ResMut<Crafted>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Crafting, Option<&Path>, Option<&Skills>)>,
    mut orders: Query<&mut Craft>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
) {
    let craft = skill_rules.index_of(SKILL_CRAFT);
    // Забронированное под продажу заказу не принадлежит (§12.50).
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());
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
            match plan_spend(&piles, &rule.cost, &booked) {
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
        // Журнал совершённого (§12.58): здесь и только здесь штука становится
        // сделанной. Цель на аптечку смотрит сюда, а не на склад, — иначе её
        // закрыла бы аптечка, купленная на рынке или поднятая на вылазке.
        if !crafted.0.contains(&order.def) {
            crafted.0.push(order.def);
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
