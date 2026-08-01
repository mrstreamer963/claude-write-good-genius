//! Перенос предметов: доставка материала на площадку, уборка пола и возврат при сносе.
//!
//! Перенос — **отдельный тип задачи**, а не фаза стройки (§12.15 concept.md):
//! носильщиком и строителем могут быть разные коты, а `jobs` знает про материал
//! ровно одно — хватает его на площадке или нет.
//!
//! Кот без груза идёт к куче, с грузом — к адресату; фазу задаёт наличие
//! `Carrying`, отдельного состояния нет. Груз сам не падает: пока кот его не
//! сдал, груз в лапах, и такой кот дешевле любого другого для новой доставки.
//!
//! Тип предмета лежит в самой куче и в грузе, поэтому раздача сводит их: коту с
//! грузом годится лишь площадка, которой нужен этот тип, а пустому — лишь куча
//! нужного типа. За ходку кот везёт один тип (§12.21).
//!
//! Адресатов два (`HaulTo`): площадка, которой не хватает материала, и склад —
//! клетка с ёмкостью. Уборка (`assign_tidy`) раздаётся **после** стройки и
//! сноса: она не должна отбирать котов у настоящей работы (§12.16).
//!
//! Объём лап (`Carry`, §12.17) режет только подъём: сколько кот уже несёт, он
//! донесёт. Остаток кучи честнее оставить лежать — раздатчик пришлёт за ним
//! следующую ходку.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::build_spot;
use crate::map::{BaseMap, DIRS};
use crate::path::Reach;

// --- доставка на площадку --------------------------------------------------

/// Назначает свободных котов на доставку материала к чертежам, которым его не хватает.
///
/// Стоимость назначения — длина всего маршрута: `кот → куча → площадка`. Вторая
/// нога считается честным обходом от кучи, а не расстоянием от кота: иначе при
/// двух кучах кот брал бы ближнюю к себе, даже если из неё до стройки вдвое
/// дальше. Коту с грузом нога до кучи не нужна — он и оказывается дешевле всех.
///
/// Раздача жадная, как и в `assign_jobs` (§12.14): каждый раз берём самую
/// дешёвую пару (кот, чертёж) из оставшихся.
///
/// Чертежи сноса сюда не попадают: снос ничего не стоит, он материал возвращает.
pub(crate) fn assign_hauls(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    mut blueprints: Query<(Entity, &mut Blueprint)>,
    stacks: Query<(&Position, &Stack)>,
    free_cats: Query<
        (Entity, &Position, Option<&Carrying>),
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
            Without<Squad>,
            Without<Path>,
        ),
    >,
) {
    // Площадки, которым чего-то не хватает и к которым сейчас никто не едет.
    let mut needy: Vec<(Entity, (i32, i32), i16, Vec<(usize, i32)>)> = blueprints
        .iter()
        .filter(|(_, bp)| bp.hauler.is_none())
        .filter_map(|(e, bp)| {
            let miss = missing(&rules, bp);
            (!miss.is_empty()).then(|| (e, (bp.x, bp.y), bp.tile, miss))
        })
        .collect();
    if needy.is_empty() {
        return;
    }

    let map = &*map;
    let mut idle: Vec<(Entity, Option<usize>, Reach)> = free_cats
        .iter()
        .map(|(e, p, load)| (e, load.map(|l| l.item), Reach::all(map, (p.x, p.y))))
        .collect();

    // Обходы от куч нужны только пустым котам — гружёный идёт сразу на площадку.
    let piles: Vec<((i32, i32), usize, Reach)> = if idle.iter().any(|(_, load, _)| load.is_none()) {
        stacks
            .iter()
            .filter(|(_, s)| s.count > 0)
            .map(|(p, s)| ((p.x, p.y), s.item, Reach::all(map, (p.x, p.y))))
            .collect()
    } else {
        Vec::new()
    };
    let piles: &[((i32, i32), usize, Reach)] = &piles;

    while !idle.is_empty() && !needy.is_empty() {
        // Куда идти первым шагом: гружёный — на площадку, пустой — к куче. Тип
        // связывает обоих: гружёный годится только той площадке, которой нужен
        // его груз, пустой идёт лишь к куче нужного типа (§12.21).
        let chosen = idle
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, loaded, reach))| {
                needy
                    .iter()
                    .enumerate()
                    .filter_map(move |(ni, (_, bp_xy, bp_tile, miss))| {
                        let wanted = |item: usize| miss.iter().any(|&(i, _)| i == item);
                        if let Some(item) = *loaded {
                            if !wanted(item) {
                                return None;
                            }
                            let (spot, steps) = build_spot(map, reach, *bp_xy, *bp_tile, None)?;
                            return Some((steps, ci, ni, spot));
                        }
                        piles
                            .iter()
                            .filter(|&&(_, item, _)| wanted(item))
                            .filter_map(|(pile, _, from_pile)| {
                                let to_pile = reach.dist_at(pile.0, pile.1)?;
                                let (_, rest) = build_spot(map, from_pile, *bp_xy, *bp_tile, None)?;
                                Some((to_pile + rest, ci, ni, *pile))
                            })
                            .min_by_key(|&(steps, ..)| steps)
                    })
            })
            .min_by_key(|&(steps, ..)| steps);
        // Ни одна пара не сошлась: нужного нет вовсе или до него не дойти. Коты
        // остаются свободными — `assign_jobs` найдёт им бесплатную работу.
        let Some((_, ci, ni, goal)) = chosen else {
            break;
        };

        let (cat_e, _, reach) = idle.remove(ci);
        let (bp_e, ..) = needy.remove(ni);
        let path = reach.path_to(goal.0, goal.1).unwrap_or_default();
        if let Ok((_, mut bp)) = blueprints.get_mut(bp_e) {
            bp.hauler = Some(cat_e);
        }
        commands.entity(cat_e).insert((
            Haul(HaulTo::Site(bp_e)),
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

// --- уборка ----------------------------------------------------------------

/// Держит пометки уборки в согласии с картой.
///
/// При включённой автоуборке помечает всё, что валяется вне склада. Снимает
/// пометку с куч **на складе** в любом режиме: без этого доставленный лом
/// сразу становился бы задачей на самого себя и коты гоняли бы его по кругу.
pub(crate) fn mark_loose_scrap(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    auto: Res<AutoTidy>,
    mut commands: Commands,
    stacks: Query<(Entity, &Position, Option<&ToStore>), With<Stack>>,
) {
    for (e, pos, mark) in &stacks {
        let in_store = rules.capacity_of(map.tile_at(pos.x, pos.y)) > 0;
        match (in_store, mark.is_some()) {
            (true, true) => {
                commands.entity(e).remove::<ToStore>();
            }
            (false, false) if auto.0 => {
                commands.entity(e).insert(ToStore::default());
            }
            _ => {}
        }
    }
}

/// Раздаёт уборку: гружёные коты несут груз на склад, пустые идут за помеченными
/// кучами.
///
/// Стоит **после** `assign_jobs` в цепочке — уборка не отбирает котов у стройки
/// и сноса. Здесь, в отличие от `assign_hauls`, вторая нога маршрута (куча →
/// склад) не оптимизируется: склад обычно одна комната, а платить пришлось бы
/// обходом на каждую кучу каждый тик, пока на полу есть мусор.
pub(crate) fn assign_tidy(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    mut marks: Query<(Entity, &Position, &mut ToStore)>,
    stacks: Query<(&Position, &Stack)>,
    free_cats: Query<
        (Entity, &Position, Option<&Carrying>),
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
            Without<Squad>,
            Without<Path>,
        ),
    >,
) {
    if free_cats.is_empty() {
        return;
    }
    let map = &*map;
    let stock = stock_grid(map, stacks.iter().map(|(p, s)| ((p.x, p.y), s.count)));
    // Склада нет или он забит — уборки не будет вовсе. Без этой проверки коты
    // ходили бы к кучам и возвращались ни с чем, тик за тиком.
    if !any_store_room(map, &rules, &stock) {
        return;
    }

    // Гружёные коты: каждому — ближайший склад со свободным местом. Драться за
    // клетку им незачем, ёмкость проверяется ещё раз при сдаче.
    let mut empty: Vec<(Entity, Reach)> = Vec::new();
    for (cat_e, pos, load) in &free_cats {
        let reach = Reach::all(map, (pos.x, pos.y));
        if load.is_none() {
            empty.push((cat_e, reach));
            continue;
        }
        if let Some((cell, _)) = nearest_store(map, &rules, &reach, &stock) {
            let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
            commands.entity(cat_e).insert((
                Haul(HaulTo::Store(None)),
                Path { steps: path },
                MoveCooldown(0),
            ));
        }
    }

    // Пустые коты: жадно разбираем пары (кот, помеченная куча) от ближней.
    let mut open: Vec<(Entity, (i32, i32))> = marks
        .iter()
        .filter(|(_, _, mark)| mark.hauler.is_none())
        .map(|(e, p, _)| (e, (p.x, p.y)))
        .collect();

    while !empty.is_empty() && !open.is_empty() {
        let chosen = empty
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, reach))| {
                open.iter().enumerate().filter_map(move |(oi, &(_, xy))| {
                    reach.dist_at(xy.0, xy.1).map(|d| (d, ci, oi, xy))
                })
            })
            .min_by_key(|&(steps, ..)| steps);
        let Some((_, ci, oi, goal)) = chosen else {
            break;
        };

        let (cat_e, reach) = empty.remove(ci);
        let (pile_e, _) = open.remove(oi);
        let path = reach.path_to(goal.0, goal.1).unwrap_or_default();
        if let Ok((_, _, mut mark)) = marks.get_mut(pile_e) {
            mark.hauler = Some(cat_e);
        }
        commands.entity(cat_e).insert((
            Haul(HaulTo::Store(Some(pile_e))),
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

// --- работа носильщика -----------------------------------------------------

/// Носильщики, добравшиеся до цели: набирают лом на куче и сдают его адресату.
///
/// Отпустить задачу (снять `Haul`, обнулить claim) — штатный исход, а не
/// ошибка: кучу могли разобрать раньше, склад — заполниться, маршрут —
/// оборваться. Раздатчик на следующем тике попробует снова, груз при этом
/// остаётся на коте.
pub(crate) fn work_hauls(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    cats: Query<(
        Entity,
        &Position,
        &Haul,
        Option<&Carrying>,
        Option<&Path>,
        Option<&Carry>,
    )>,
    mut blueprints: Query<&mut Blueprint>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
    mut marks: Query<&mut ToStore>,
) {
    for (cat_e, pos, haul, load, path, carry) in &cats {
        match haul.0 {
            HaulTo::Site(bp_e) => {
                // Чертёж отменили, пока кот был в пути.
                let Ok(mut bp) = blueprints.get_mut(bp_e) else {
                    commands.entity(cat_e).remove::<Haul>();
                    continue;
                };
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let miss = missing(&rules, &bp);

                let Some(load) = load else {
                    // Пришёл к куче: берём тот тип, которого площадке не хватает,
                    // и ровно столько, сколько не хватает и влезает в лапы. Какая
                    // именно куча под ногами — решается здесь, а не при раздаче.
                    let taken =
                        take_needed(&mut commands, &mut stacks, (pos.x, pos.y), &miss, carry);
                    let Some((item, taken)) = taken else {
                        bp.hauler = None;
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    let reach = Reach::all(&map, (pos.x, pos.y));
                    match build_spot(&map, &reach, (bp.x, bp.y), bp.tile, None) {
                        Some((spot, _)) => {
                            let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
                            commands.entity(cat_e).insert((
                                Carrying { item, count: taken },
                                Path { steps: path },
                                MoveCooldown(0),
                            ));
                        }
                        // Площадка стала недостижима, пока кот шёл за грузом:
                        // груз при нём, задача отпущена.
                        None => {
                            bp.hauler = None;
                            commands
                                .entity(cat_e)
                                .insert(Carrying { item, count: taken })
                                .remove::<Haul>();
                        }
                    }
                    continue;
                };

                // Пришёл на площадку — сдаём груз (излишек уносит с собой).
                if (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1 {
                    let need = miss
                        .iter()
                        .find(|&&(i, _)| i == load.item)
                        .map_or(0, |&(_, n)| n);
                    let given = load.count.min(need);
                    add_delivered(&mut bp.delivered, load.item, given);
                    keep_rest(&mut commands, cat_e, load.item, load.count - given);
                }
                bp.hauler = None;
                commands.entity(cat_e).remove::<Haul>();
            }

            HaulTo::Store(pile_e) => {
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let Some(load) = load else {
                    // Куда нести, решается **до** подъёма: клетка склада могла
                    // заполниться, пока кот шёл. Складов нет или все полны —
                    // куча остаётся лежать на виду, а не переезжает в лапы,
                    // откуда игрок её уже не достанет.
                    let reach = Reach::all(&map, (pos.x, pos.y));
                    let stock =
                        stock_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                    let Some((cell, _)) = nearest_store(&map, &rules, &reach, &stock) else {
                        release_claim(&mut marks, pile_e);
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    // Берём ровно столько, сколько влезет в адресат и в лапы:
                    // остаток честнее оставить на полу, чем таскать по базе.
                    // Берём именно ту кучу, за которой шли: на клетке могут
                    // лежать разные типы, а помечена была одна (§12.21).
                    let free = portion(carry, free_space(&map, &rules, &stock, cell));
                    let taken =
                        take_from_pile(&mut commands, &mut stacks, pile_e, (pos.x, pos.y), free);
                    let Some((item, taken)) = taken else {
                        release_claim(&mut marks, pile_e);
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };
                    let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
                    commands.entity(cat_e).insert((
                        Carrying { item, count: taken },
                        Path { steps: path },
                        MoveCooldown(0),
                    ));
                    continue;
                };

                // Пришёл на склад — сдаём, сколько влезло.
                let stock = stock_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                let free = free_space(&map, &rules, &stock, (pos.x, pos.y));
                if free > 0 {
                    let given = load.count.min(free);
                    spill(&mut commands, &mut stacks, (pos.x, pos.y), load.item, given);
                    keep_rest(&mut commands, cat_e, load.item, load.count - given);
                }
                release_claim(&mut marks, pile_e);
                commands.entity(cat_e).remove::<Haul>();
            }
        }
    }
}

/// Сколько кот унесёт за эту ходку: `Carry` — предел лап, его отсутствие (и
/// ноль) читается как «без предела», по правилу нулей у тайлов (§12.17).
fn portion(carry: Option<&Carry>, want: i32) -> i32 {
    match carry {
        Some(&Carry(cap)) if cap > 0 => want.min(cap),
        _ => want,
    }
}

/// Оставить коту остаток груза; пусто — снять `Carrying`.
fn keep_rest(commands: &mut Commands, cat: Entity, item: usize, left: i32) {
    if left > 0 {
        commands.entity(cat).insert(Carrying { item, count: left });
    } else {
        commands.entity(cat).remove::<Carrying>();
    }
}

/// Снять с кучи претензию носильщика (сама пометка остаётся). Кучи может уже не
/// быть — её и подняли; это штатный конец задачи, а не ошибка.
pub(crate) fn release_claim(marks: &mut Query<&mut ToStore>, pile: Option<Entity>) {
    if let Some(Ok(mut mark)) = pile.map(|e| marks.get_mut(e)) {
        mark.hauler = None;
    }
}

/// Взять из конкретной кучи, если она ещё есть и лежит под котом. Кучи может
/// уже не быть — её разобрали раньше; это штатный конец задачи, а не ошибка.
fn take_from_pile(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    pile: Option<Entity>,
    at: (i32, i32),
    want: i32,
) -> Option<(usize, i32)> {
    let (stack_e, pos, mut stack) = pile.and_then(|e| stacks.get_mut(e).ok())?;
    if (pos.x, pos.y) != at {
        return None;
    }
    let taken = want.min(stack.count);
    if taken <= 0 {
        return None;
    }
    stack.count -= taken;
    let item = stack.item;
    if stack.count <= 0 {
        commands.entity(stack_e).despawn();
    }
    Some((item, taken))
}

/// Взять из-под ног тот тип, которого не хватает площадке, — сколько не хватает
/// и сколько влезает в лапы. Перебор идёт по недостаче, а не по кучам: порядок
/// цены детерминирован (§12.21), порядок сущностей — нет.
fn take_needed(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    at: (i32, i32),
    miss: &[(usize, i32)],
    carry: Option<&Carry>,
) -> Option<(usize, i32)> {
    for &(item, need) in miss {
        let found = stacks
            .iter()
            .find(|(_, p, s)| (p.x, p.y) == at && s.item == item && s.count > 0)
            .map(|(e, ..)| e);
        if let Some(taken) = take_from_pile(commands, stacks, found, at, portion(carry, need)) {
            return Some(taken);
        }
    }
    None
}

// --- склад и кучи ----------------------------------------------------------

/// Кучи, лежащие **на складе**, в порядке обхода карты.
///
/// Порядок задан явно, а не порядком сущностей: обход ECS зависит от истории
/// вставок, а любой недетерминизм ломает и тесты, и модель времени (§11).
/// Лежащее на полу сюда не попадает — платит склад, а не то, что валяется
/// (§12.24).
pub(crate) fn storage_order<I>(
    map: &BaseMap,
    rules: &TileRules,
    piles: I,
) -> Vec<(Entity, usize, i32)>
where
    I: IntoIterator<Item = (Entity, (i32, i32), usize, i32)>,
{
    let mut shelved: Vec<(i32, i32, Entity, usize, i32)> = piles
        .into_iter()
        .filter(|&(_, (x, y), _, count)| count > 0 && rules.capacity_of(map.tile_at(x, y)) > 0)
        .map(|(e, (x, y), item, count)| (y, x, e, item, count))
        .collect();
    shelved.sort_unstable_by_key(|&(y, x, ..)| (y, x));
    shelved
        .into_iter()
        .map(|(_, _, e, i, n)| (e, i, n))
        .collect()
}

/// Сколько снять с каждой кучи, чтобы покрыть набор; `None` — не покрывается.
///
/// **Либо весь набор, либо ничего**: половинчатая оплата оставила бы игрока и
/// без предметов, и без покупки. Правило живёт здесь одно на всех, кто платит
/// складом, — найм и науку (фасад) и производство (система): две арифметики
/// списания однажды разойдутся на порядке обхода куч (§12.30).
pub(crate) fn plan_spend(
    piles: &[(Entity, usize, i32)],
    cost: &[(usize, i32)],
) -> Option<Vec<(Entity, i32)>> {
    let mut takes: Vec<(Entity, i32)> = Vec::new();
    for &(item, need) in cost {
        let mut left = need;
        for &(pile_e, pile_item, count) in piles {
            if left <= 0 {
                break;
            }
            if pile_item != item {
                continue;
            }
            let taken = left.min(count);
            left -= taken;
            takes.push((pile_e, taken));
        }
        if left > 0 {
            return None; // на складе не хватает — не снимаем ничего
        }
    }
    Some(takes)
}

/// Сколько всего добра лежит на каждой клетке — сеткой, чтобы не искать
/// линейно. Тип здесь не важен: склад типо-агностичен, ёмкость считает штуки
/// (§12.21).
fn stock_grid(map: &BaseMap, piles: impl Iterator<Item = ((i32, i32), i32)>) -> Vec<i32> {
    let mut grid = vec![0; (map.width * map.height) as usize];
    for ((x, y), count) in piles {
        if let Some(i) = map.index(x, y) {
            grid[i] += count;
        }
    }
    grid
}

/// Сколько ещё влезет на клетку. Склад — это тайл с ёмкостью, отдельного
/// слоя зон нет (§12.16); у обычного пола ёмкость нулевая.
fn free_space(map: &BaseMap, rules: &TileRules, stock: &[i32], at: (i32, i32)) -> i32 {
    map.index(at.0, at.1)
        .map_or(0, |i| rules.capacity_of(map.cells[i]) - stock[i])
}

/// Ближайшая клетка склада, куда влезет хоть сколько-то лома, и путь до неё
/// в шагах.
fn nearest_store(
    map: &BaseMap,
    rules: &TileRules,
    reach: &Reach,
    stock: &[i32],
) -> Option<((i32, i32), i32)> {
    let mut best: Option<((i32, i32), i32)> = None;
    for y in 0..map.height {
        for x in 0..map.width {
            if free_space(map, rules, stock, (x, y)) <= 0 {
                continue;
            }
            let Some(d) = reach.dist_at(x, y) else {
                continue;
            };
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some(((x, y), d));
            }
        }
    }
    best
}

/// Есть ли на базе хоть одно место под груз. Дешёвая проверка без обхода: без
/// неё коты ходили бы к кучам и возвращались ни с чем, пока склада нет.
fn any_store_room(map: &BaseMap, rules: &TileRules, stock: &[i32]) -> bool {
    (0..stock.len()).any(|i| rules.capacity_of(map.cells[i]) - stock[i] > 0)
}

/// Приводит кучи в порядок: снимает их с пустоты и сливает те, что оказались
/// на одной клетке.
///
/// **Пустота.** Кот сносит клетку, стоя на шаг ближе к берегу, возврат ложится
/// под него — и следующая волна убирает эту клетку вместе с кучей. Сдвиг на
/// соседний пол — тот же ответ, что и у кота в яме (§12.10), и по той же
/// причине: пустоту не пересечь, значит куча в ней недостижима. Разница в том,
/// что кот виден флагом `stuck`, а «лом провалился в дыру» читался бы как
/// молчаливая пропажа материала. Шаг ровно один: куча возникает только под
/// котом, а он стоит на полу. Соседа нет — куча остаётся в яме, обратимо.
///
/// **Двойники.** `spill` не видит куч, заспавненных в этом же тике через
/// `Commands`, поэтому два кота, сдавшие одинаковый груз на одну клетку, создают
/// две кучи — сливаем; живут двойники не дольше тика. Кучи **разных** типов на
/// одной клетке двойниками не считаются и остаются лежать рядом (§12.21).
pub(crate) fn settle_stacks(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut stacks: Query<(Entity, &mut Position, &mut Stack, Option<&ToStore>)>,
) {
    // Первая куча своего типа на клетке остаётся, остальные вливаются в неё.
    let mut keepers: Vec<(((i32, i32), usize), Entity, bool)> = Vec::new();
    let mut moves: Vec<(Entity, (i32, i32))> = Vec::new();
    let mut merges: Vec<(Entity, Entity, i32, bool)> = Vec::new();

    for (e, pos, stack, mark) in stacks.iter() {
        let Some(home) = home_of(&map, (pos.x, pos.y)) else {
            continue; // замуровано вместе с ямой — обратимо
        };
        let home = (home, stack.item);
        match keepers.iter().find(|(cell, ..)| *cell == home) {
            // Пометка переживает слияние: иначе жест игрока молча пропадал бы.
            Some(&(_, keeper, kept_mark)) => {
                merges.push((e, keeper, stack.count, mark.is_some() && !kept_mark))
            }
            None => {
                keepers.push((home, e, mark.is_some()));
                if (pos.x, pos.y) != home.0 {
                    moves.push((e, home.0));
                }
            }
        }
    }

    for (e, to) in moves {
        if let Ok((_, mut pos, ..)) = stacks.get_mut(e) {
            pos.x = to.0;
            pos.y = to.1;
        }
    }
    for (e, keeper, count, pass_mark) in merges {
        if let Ok((_, _, mut stack, _)) = stacks.get_mut(keeper) {
            stack.count += count;
        }
        if pass_mark {
            commands.entity(keeper).insert(ToStore::default());
        }
        commands.entity(e).despawn();
    }
}

/// Клетка, на которой куче место: своя, если проходима, иначе первый проходимый
/// сосед. Порядок `DIRS` фиксирован, значит выбор детерминирован (§11).
fn home_of(map: &BaseMap, at: (i32, i32)) -> Option<(i32, i32)> {
    if map.walkable(at.0, at.1) {
        return Some(at);
    }
    DIRS.iter()
        .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
        .find(|&(nx, ny)| map.walkable(nx, ny))
}

/// Положить лом на клетку, слив с уже лежащей там кучей.
pub(crate) fn spill(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    at: (i32, i32),
    item: usize,
    count: i32,
) {
    if let Some((_, _, mut stack)) = stacks
        .iter_mut()
        .find(|(_, p, s)| (p.x, p.y) == at && s.item == item)
    {
        stack.count += count;
        return;
    }
    commands.spawn((Position { x: at.0, y: at.1 }, Stack { item, count }));
}
