//! Джобы: раздача чертежей котам и работа над ними.
//!
//! Чертёж — сущность `Blueprint`. Свободный (простаивающий) кот назначается на
//! ближайший достижимый чертёж, идёт к месту работы и «работает» `BUILD_TIME`
//! тиков, после чего тайл возводится (или сносится, если `tile < 0`).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::demolition::DemolitionFront;
use crate::map::{BaseMap, DIRS};
use crate::path::Reach;

/// Тиков работы, чтобы возвести один тайл.
pub(crate) const BUILD_TIME: i32 = 12;

/// Найти клетку, откуда кот выполнит чертёж (сам тайл, если проходим, либо
/// сосед), и её цену — число шагов от кота. Возвращает (клетка, шаги).
///
/// `bp_tile` — что планируется на клетке; для сноса (`< 0`) стоять на самой цели
/// нельзя. Цель сноса всегда проходима, иначе сносить было бы нечего, поэтому без
/// этого правила кот выбрал бы её первой и снёс пол под собой. В пустоту кот
/// попадает действием игрока, а не в порядке штатной работы (см. §12.10 concept.md).
///
/// Из подходящих клеток берём **ближайшую** к коту, кроме сноса: там порядок
/// задаёт берег (`front`), а не расстояние. Встав на клетку глубже цели, кот
/// работал бы «из-за» дыры и после сноса остался бы по дальнюю её сторону —
/// отрезанным от остатка собственной работы. Клетка на шаг ближе к берегу есть
/// у любой цели (это её родитель в волне) и по определению уйдёт позже неё.
fn build_spot(
    map: &BaseMap,
    reach: &Reach,
    bp: (i32, i32),
    bp_tile: i16,
    front: Option<&DemolitionFront>,
) -> Option<((i32, i32), i32)> {
    let mut spots = Vec::new();
    if bp_tile >= 0 && map.walkable(bp.0, bp.1) {
        spots.push(bp);
    }
    for (dx, dy) in DIRS {
        let n = (bp.0 + dx, bp.1 + dy);
        if map.walkable(n.0, n.1) {
            spots.push(n);
        }
    }
    let reachable = spots
        .into_iter()
        .filter_map(|s| reach.dist_at(s.0, s.1).map(|d| (s, d)));
    match front.filter(|_| bp_tile < 0) {
        Some(f) => reachable.min_by_key(|&((x, y), d)| (f.depth_at(x, y), d)),
        None => reachable.min_by_key(|&(_, d)| d),
    }
}

/// Назначает простаивающих котов (без задачи и без маршрута) на ближайшие
/// достижимые чертежи и отправляет их к месту постройки.
///
/// «Ближайший» — по числу шагов пути, а не по прямой, и выбирается жадно по
/// всем парам (кот, чертёж) сразу, а не в порядке чертежей (§12.14).
///
/// Кот с невыполнимым сейчас приказом тоже считается свободным. Иначе он бы
/// никогда не взялся строить — в том числе тот тайл, который открывает ему
/// выход из запертой комнаты. Приказ при этом не теряется: `retry_orders`
/// подхватит его, когда кот освободится от задачи.
///
/// Чертежи сноса дополнительно проходят через `DemolitionFront`: сносить можно
/// только самые глубокие клетки области, иначе снос отрезает доступ к остатку.
/// Порядок стройки не трогаем — построенный пол доступ не закрывает, а открывает.
pub(crate) fn assign_jobs(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut blueprints: Query<(Entity, &mut Blueprint)>,
    cats: Query<&Position, With<UnitId>>,
    free_cats: Query<(Entity, &Position), (With<UnitId>, Without<Assignment>, Without<Path>)>,
) {
    let doomed: Vec<(i32, i32)> = blueprints
        .iter()
        .filter(|(_, bp)| bp.tile < 0)
        .map(|(_, bp)| (bp.x, bp.y))
        .collect();
    let front = (!doomed.is_empty()).then(|| {
        let positions: Vec<(i32, i32)> = cats.iter().map(|p| (p.x, p.y)).collect();
        DemolitionFront::new(&map, &doomed, &positions)
    });

    // Чертежи, которые сейчас можно раздать.
    let mut open: Vec<(Entity, (i32, i32), i16)> = Vec::new();
    for (bp_e, mut bp) in &mut blueprints {
        // Снос не на фронте своей зоны ждёт: убрать эту клетку сейчас — значит
        // отрезать доступ к тем, что глубже.
        let waiting = bp.tile < 0 && front.as_ref().is_some_and(|f| !f.is_ready(bp.x, bp.y));
        if let Some(cat) = bp.assignee {
            // Игрок мог дорисовать область уже начатого сноса — тогда джоб
            // встаёт на паузу (прогресс сохраняется), а кот освобождается.
            if waiting {
                bp.assignee = None;
                commands
                    .entity(cat)
                    .remove::<(Assignment, Path, MoveCooldown)>();
            }
        } else if !waiting {
            open.push((bp_e, (bp.x, bp.y), bp.tile));
        }
    }

    if open.is_empty() {
        return;
    }

    // Достижимость считаем по разу на кота: одного обхода хватает, чтобы
    // сравнить между собой все доступные ему места работы.
    let (map, front) = (&*map, front.as_ref());
    let mut idle: Vec<(Entity, Reach)> = free_cats
        .iter()
        .map(|(e, p)| (e, Reach::all(map, (p.x, p.y))))
        .collect();

    // Жадно разбираем самые дешёвые пары (кот, чертёж). Раздача в порядке
    // самих чертежей гнала кота с только что законченной клетки через
    // полкарты — к следующему по счёту чертежу, мимо соседнего (§12.14).
    while !idle.is_empty() && !open.is_empty() {
        let chosen = idle
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, reach))| {
                open.iter()
                    .enumerate()
                    .filter_map(move |(oi, &(_, bp_xy, bp_tile))| {
                        build_spot(map, reach, bp_xy, bp_tile, front)
                            .map(|(spot, steps)| (steps, ci, oi, spot))
                    })
            })
            .min_by_key(|&(steps, ..)| steps);
        // Ни одна пара не сошлась — оставшиеся чертежи никому не достать.
        let Some((_, ci, oi, spot)) = chosen else {
            break;
        };

        let (cat_e, reach) = idle.remove(ci);
        let (bp_e, _, _) = open.remove(oi);
        let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
        if let Ok((_, mut bp)) = blueprints.get_mut(bp_e) {
            bp.assignee = Some(cat_e);
        }
        commands
            .entity(cat_e)
            .insert((Assignment(bp_e), Path { steps: path }, MoveCooldown(0)));
    }
}

/// Коты, добравшиеся до чертежа, «работают» до готовности; затем тайл возводится.
pub(crate) fn work_jobs(
    mut map: ResMut<BaseMap>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Assignment, Option<&Path>)>,
    mut blueprints: Query<&mut Blueprint>,
) {
    for (cat_e, pos, assign, path) in &cats {
        let Ok(mut bp) = blueprints.get_mut(assign.0) else {
            commands.entity(cat_e).remove::<Assignment>();
            continue;
        };

        let in_range = (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1;
        if in_range {
            bp.progress += 1;
            if bp.progress >= BUILD_TIME {
                map.set(bp.x, bp.y, bp.tile);
                commands.entity(assign.0).despawn();
                commands.entity(cat_e).remove::<Assignment>();
            }
        } else if path.is_none() {
            // Дошёл, но не в радиусе (маршрут оборвался) — освобождаем джоб.
            bp.assignee = None;
            commands.entity(cat_e).remove::<Assignment>();
        }
    }
}
