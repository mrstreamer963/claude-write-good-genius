//! Перенос лома: доставка материала на площадку, уборка пола и возврат при сносе.
//!
//! Перенос — **отдельный тип задачи**, а не фаза стройки (§12.15 concept.md):
//! носильщиком и строителем могут быть разные коты, а `jobs` знает про материал
//! ровно одно — хватает его на площадке или нет.
//!
//! Кот без груза идёт к куче, с грузом — к адресату; фазу задаёт наличие
//! `Carrying`, отдельного состояния нет. Груз сам не падает: пока кот его не
//! сдал, лом на руках, и такой кот дешевле любого другого для новой доставки.
//!
//! Адресатов два (`HaulTo`): площадка, которой не хватает материала, и склад —
//! клетка с ёмкостью. Уборка (`assign_tidy`) раздаётся **после** стройки и
//! сноса: она не должна отбирать котов у настоящей работы (§12.16).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::build_spot;
use crate::map::{BaseMap, DIRS};
use crate::path::Reach;

// --- доставка на площадку --------------------------------------------------

/// Назначает свободных котов на доставку лома к чертежам, которым его не хватает.
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
            Without<Path>,
        ),
    >,
) {
    // Площадки, которым не хватает лома и к которым сейчас никто не едет.
    let mut needy: Vec<(Entity, (i32, i32), i16)> = blueprints
        .iter()
        .filter(|(_, bp)| bp.hauler.is_none() && bp.delivered < rules.cost_of(bp.tile))
        .map(|(e, bp)| (e, (bp.x, bp.y), bp.tile))
        .collect();
    if needy.is_empty() {
        return;
    }

    let map = &*map;
    let mut idle: Vec<(Entity, bool, Reach)> = free_cats
        .iter()
        .map(|(e, p, load)| (e, load.is_some(), Reach::all(map, (p.x, p.y))))
        .collect();

    // Обходы от куч нужны только пустым котам — гружёный идёт сразу на площадку.
    let piles: Vec<((i32, i32), Reach)> = if idle.iter().any(|(_, loaded, _)| !loaded) {
        stacks
            .iter()
            .filter(|(_, s)| s.count > 0)
            .map(|(p, _)| ((p.x, p.y), Reach::all(map, (p.x, p.y))))
            .collect()
    } else {
        Vec::new()
    };
    let piles: &[((i32, i32), Reach)] = &piles;

    while !idle.is_empty() && !needy.is_empty() {
        // Куда идти первым шагом: гружёный — на площадку, пустой — к куче.
        let chosen = idle
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, loaded, reach))| {
                needy
                    .iter()
                    .enumerate()
                    .filter_map(move |(ni, &(_, bp_xy, bp_tile))| {
                        if *loaded {
                            let (spot, steps) = build_spot(map, reach, bp_xy, bp_tile, None)?;
                            return Some((steps, ci, ni, spot));
                        }
                        piles
                            .iter()
                            .filter_map(|(pile, from_pile)| {
                                let to_pile = reach.dist_at(pile.0, pile.1)?;
                                let (_, rest) = build_spot(map, from_pile, bp_xy, bp_tile, None)?;
                                Some((to_pile + rest, ci, ni, *pile))
                            })
                            .min_by_key(|&(steps, ..)| steps)
                    })
            })
            .min_by_key(|&(steps, ..)| steps);
        // Ни одна пара не сошлась: лома нет вовсе или до него не дойти. Коты
        // остаются свободными — `assign_jobs` найдёт им бесплатную работу.
        let Some((_, ci, ni, goal)) = chosen else {
            break;
        };

        let (cat_e, _, reach) = idle.remove(ci);
        let (bp_e, _, _) = needy.remove(ni);
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

/// Раздаёт уборку: гружёные коты несут лом на склад, пустые идут за помеченными
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
            Without<Path>,
        ),
    >,
) {
    if free_cats.is_empty() {
        return;
    }
    let map = &*map;
    let scrap = scrap_grid(map, stacks.iter().map(|(p, s)| ((p.x, p.y), s.count)));
    // Склада нет или он забит — уборки не будет вовсе. Без этой проверки коты
    // ходили бы к кучам и возвращались ни с чем, тик за тиком.
    if !any_store_room(map, &rules, &scrap) {
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
        if let Some((cell, _)) = nearest_store(map, &rules, &reach, &scrap) {
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
    cats: Query<(Entity, &Position, &Haul, Option<&Carrying>, Option<&Path>)>,
    mut blueprints: Query<&mut Blueprint>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
    mut marks: Query<&mut ToStore>,
) {
    for (cat_e, pos, haul, load, path) in &cats {
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
                let need = (rules.cost_of(bp.tile) - bp.delivered).max(0);

                let Some(load) = load else {
                    // Пришёл к куче: берём ровно столько, сколько не хватает.
                    let taken = take_from_pile(&mut commands, &mut stacks, (pos.x, pos.y), need);
                    if taken <= 0 {
                        bp.hauler = None;
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    }

                    let reach = Reach::all(&map, (pos.x, pos.y));
                    match build_spot(&map, &reach, (bp.x, bp.y), bp.tile, None) {
                        Some((spot, _)) => {
                            let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
                            commands.entity(cat_e).insert((
                                Carrying(taken),
                                Path { steps: path },
                                MoveCooldown(0),
                            ));
                        }
                        // Площадка стала недостижима, пока кот шёл за ломом:
                        // груз при нём, задача отпущена.
                        None => {
                            bp.hauler = None;
                            commands
                                .entity(cat_e)
                                .insert(Carrying(taken))
                                .remove::<Haul>();
                        }
                    }
                    continue;
                };

                // Пришёл на площадку — сдаём груз (излишек уносит с собой).
                if (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1 {
                    let given = load.0.min(need);
                    bp.delivered += given;
                    keep_rest(&mut commands, cat_e, load.0 - given);
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
                    let scrap =
                        scrap_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                    let Some((cell, _)) = nearest_store(&map, &rules, &reach, &scrap) else {
                        release_claim(&mut marks, pile_e);
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    // Берём ровно столько, сколько влезет в адресат: остаток
                    // честнее оставить на полу, чем таскать по базе.
                    let free = free_space(&map, &rules, &scrap, cell);
                    let taken = take_from_pile(&mut commands, &mut stacks, (pos.x, pos.y), free);
                    if taken <= 0 {
                        release_claim(&mut marks, pile_e);
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    }
                    let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
                    commands.entity(cat_e).insert((
                        Carrying(taken),
                        Path { steps: path },
                        MoveCooldown(0),
                    ));
                    continue;
                };

                // Пришёл на склад — сдаём, сколько влезло.
                let scrap = scrap_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                let free = free_space(&map, &rules, &scrap, (pos.x, pos.y));
                if free > 0 {
                    let given = load.0.min(free);
                    spill(&mut commands, &mut stacks, (pos.x, pos.y), given);
                    keep_rest(&mut commands, cat_e, load.0 - given);
                }
                release_claim(&mut marks, pile_e);
                commands.entity(cat_e).remove::<Haul>();
            }
        }
    }
}

/// Оставить коту остаток груза; пусто — снять `Carrying`.
fn keep_rest(commands: &mut Commands, cat: Entity, left: i32) {
    if left > 0 {
        commands.entity(cat).insert(Carrying(left));
    } else {
        commands.entity(cat).remove::<Carrying>();
    }
}

/// Снять с кучи претензию носильщика (сама пометка остаётся). Кучи может уже не
/// быть — её и подняли; это штатный конец задачи, а не ошибка.
fn release_claim(marks: &mut Query<&mut ToStore>, pile: Option<Entity>) {
    if let Some(Ok(mut mark)) = pile.map(|e| marks.get_mut(e)) {
        mark.hauler = None;
    }
}

/// Взять с кучи под ногами не больше `want`; вернёт сколько взято.
fn take_from_pile(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    at: (i32, i32),
    want: i32,
) -> i32 {
    stacks
        .iter_mut()
        .find(|(_, p, _)| (p.x, p.y) == at)
        .map(|(stack_e, _, mut stack)| {
            let taken = want.min(stack.count);
            stack.count -= taken;
            if stack.count <= 0 {
                commands.entity(stack_e).despawn();
            }
            taken
        })
        .unwrap_or(0)
}

// --- склад и кучи ----------------------------------------------------------

/// Сколько лома лежит на каждой клетке — сеткой, чтобы не искать линейно.
fn scrap_grid(map: &BaseMap, piles: impl Iterator<Item = ((i32, i32), i32)>) -> Vec<i32> {
    let mut grid = vec![0; (map.width * map.height) as usize];
    for ((x, y), count) in piles {
        if let Some(i) = map.index(x, y) {
            grid[i] += count;
        }
    }
    grid
}

/// Сколько лома ещё влезет на клетку. Склад — это тайл с ёмкостью, отдельного
/// слоя зон нет (§12.16); у обычного пола ёмкость нулевая.
fn free_space(map: &BaseMap, rules: &TileRules, scrap: &[i32], at: (i32, i32)) -> i32 {
    map.index(at.0, at.1)
        .map_or(0, |i| rules.capacity_of(map.cells[i]) - scrap[i])
}

/// Ближайшая клетка склада, куда влезет хоть сколько-то лома, и путь до неё
/// в шагах.
fn nearest_store(
    map: &BaseMap,
    rules: &TileRules,
    reach: &Reach,
    scrap: &[i32],
) -> Option<((i32, i32), i32)> {
    let mut best: Option<((i32, i32), i32)> = None;
    for y in 0..map.height {
        for x in 0..map.width {
            if free_space(map, rules, scrap, (x, y)) <= 0 {
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

/// Есть ли на базе хоть одно место под лом. Дешёвая проверка без обхода: без
/// неё коты ходили бы к кучам и возвращались ни с чем, пока склада нет.
fn any_store_room(map: &BaseMap, rules: &TileRules, scrap: &[i32]) -> bool {
    (0..scrap.len()).any(|i| rules.capacity_of(map.cells[i]) - scrap[i] > 0)
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
/// `Commands`, поэтому два кота, сдавшие груз на одну пустую клетку, создают
/// две кучи. Носильщик берёт лом с той клетки, где стоит, и вторую бы просто не
/// заметил — сливаем. Живут двойники не дольше тика.
pub(crate) fn settle_stacks(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut stacks: Query<(Entity, &mut Position, &mut Stack, Option<&ToStore>)>,
) {
    // Первая куча на клетке остаётся, остальные вливаются в неё.
    let mut keepers: Vec<((i32, i32), Entity, bool)> = Vec::new();
    let mut moves: Vec<(Entity, (i32, i32))> = Vec::new();
    let mut merges: Vec<(Entity, Entity, i32, bool)> = Vec::new();

    for (e, pos, stack, mark) in stacks.iter() {
        let Some(home) = home_of(&map, (pos.x, pos.y)) else {
            continue; // замуровано вместе с ямой — обратимо
        };
        match keepers.iter().find(|(cell, ..)| *cell == home) {
            // Пометка переживает слияние: иначе жест игрока молча пропадал бы.
            Some(&(_, keeper, kept_mark)) => {
                merges.push((e, keeper, stack.count, mark.is_some() && !kept_mark))
            }
            None => {
                keepers.push((home, e, mark.is_some()));
                if (pos.x, pos.y) != home {
                    moves.push((e, home));
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
    count: i32,
) {
    if let Some((_, _, mut stack)) = stacks.iter_mut().find(|(_, p, _)| (p.x, p.y) == at) {
        stack.count += count;
        return;
    }
    commands.spawn((Position { x: at.0, y: at.1 }, Stack { count }));
}
