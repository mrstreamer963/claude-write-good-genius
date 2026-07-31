//! Миссии: отряд уходит с базы и возвращается с добычей (§12.22 concept.md).
//!
//! Миссия — это **разметка работы**, как чертёж: игрок говорит «идём на свалку»,
//! а кого послать, решает симуляция (§12.16). Адресный выбор отряда придёт
//! вместе с авторасчётом исхода: пока миссия всегда успешна и добыча
//! фиксирована рулсетом, «кого послать» ни на что не влияет и выбирать нечего.
//!
//! Систем две, и они повторяют форму «раздатчик + работа», по которой устроены
//! стройка и перенос:
//!   * `assign_squad` — набирает недостающих бойцов из общего пула свободных
//!     котов и гонит их к шлюзу. Идёт вторым после отдыха: миссия — самая
//!     дорогая работа на базе, и подвоз лома не должен держать отряд.
//!   * `run_missions` — отправляет собравшийся отряд, крутит таймер и
//!     возвращает котов с добычей.
//!
//! Отдельного состояния «фаза миссии» нет, как его нет у `Haul` и `Rest`: где
//! отряд, видно по компонентам. `Squad` с маршрутом — кот идёт к шлюзу,
//! `Squad` без маршрута на шлюзе — ждёт остальных, `Away` — ушёл.
//!
//! **Добыча ложится кучей на шлюз**, а не в лапы котам: кот везёт один тип за
//! ходку (§12.21), а добыча бывает набором. Дальше её разносит обычная уборка
//! (§12.16) — и то, что миссия ничего не знает про склад, тут не упрощение,
//! а прямое следствие: возврат от сноса ложится под ноги ровно так же.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::spill;
use crate::map::BaseMap;
use crate::path::{Reach, find_path};

/// Все клетки-шлюзы карты, в порядке обхода: он фиксирован, значит выбор
/// шлюза детерминирован (§11).
fn gate_cells<'a>(map: &'a BaseMap, rules: &'a TileRules) -> impl Iterator<Item = (i32, i32)> + 'a {
    (0..map.height)
        .flat_map(move |y| (0..map.width).map(move |x| (x, y)))
        .filter(move |&(x, y)| rules.is_gate(map.tile_at(x, y)))
}

/// Набирает отряд и гонит его к шлюзу.
///
/// Набор **постепенный**: кто освободился, тот и в отряде. Брать всех разом
/// нельзя — на базе с бесконечной работой коты свободны поодиночке и по одному
/// тику, и отряд не собрался бы никогда.
///
/// Исполнителей выбирает симуляция, и выбирает **ближайших** к шлюзу — то же
/// правило, что у чертежей (§12.14). Шлюз выбирается один раз на миссию: тот,
/// до которого суммарно ближе всего идти отряду нужного размера.
pub(crate) fn assign_squad(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<MissionRules>,
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    crew: Query<(Entity, &Squad, &Position, Option<&Path>, Option<&Away>)>,
    free_cats: Query<
        (Entity, &Position),
        (
            With<UnitId>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Squad>,
            Without<Path>,
        ),
    >,
) {
    if missions.is_empty() {
        return; // обход на кота недёшев, а без миссий он никому не нужен
    }
    let map = &*map;
    let mut idle: Vec<(Entity, Reach)> = free_cats
        .iter()
        .map(|(e, p)| (e, Reach::all(map, (p.x, p.y))))
        .collect();

    for (mission_e, mut mission) in &mut missions {
        let Some(rule) = rules.0.get(mission.def) else {
            continue; // запись контента мимо палитры миссий
        };

        // Кто уже в отряде и где он. Пустой маршрут считается пройденным:
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
        // Отряд ушёл — набирать больше нечего, и шлюз больше не пересматривается:
        // вернутся коты туда, откуда ушли, даже если гараж успели снести.
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
            mission.gate = pick_gate(map, &tiles, rule.squad, &squad, &idle);
        }
        let Some(gate) = mission.gate else {
            continue; // шлюза нет или до него не добраться нужным числом котов
        };

        // Добираем недостающих — ближайшими к шлюзу.
        let mut need = rule.squad.saturating_sub(squad.len());
        while need > 0 {
            let nearest = idle
                .iter()
                .enumerate()
                .filter_map(|(i, (_, reach))| reach.dist_at(gate.0, gate.1).map(|d| (d, i)))
                .min_by_key(|&(d, _)| d);
            let Some((_, i)) = nearest else {
                break; // до шлюза никому не дойти — ждём следующего тика
            };
            let (cat_e, reach) = idle.remove(i);
            let path = reach.path_to(gate.0, gate.1).unwrap_or_default();
            commands.entity(cat_e).insert((
                Squad(mission_e),
                Path { steps: path },
                MoveCooldown(0),
            ));
            need -= 1;
        }

        // Боец стоит не на шлюзе и никуда не идёт: шлюз сменился, или кота
        // выбросило из ямы (`escape_voids`). Маршрут перепрокладываем — это
        // тот же случай, что `retry_orders` у приказа игрока.
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
/// Клетки, до которых не набирается нужное число котов, отбрасываются: иначе
/// отряд ушёл бы собираться к шлюзу, отрезанному от половины базы. Ничьи
/// разрешает порядок обхода карты, то есть детерминированно.
fn pick_gate(
    map: &BaseMap,
    tiles: &TileRules,
    size: usize,
    squad: &[(Entity, (i32, i32), bool)],
    idle: &[(Entity, Reach)],
) -> Option<(i32, i32)> {
    let reaches: Vec<Reach> = squad
        .iter()
        .map(|&(_, at, _)| Reach::all(map, at))
        .collect();
    gate_cells(map, tiles)
        .filter_map(|(x, y)| {
            let mut steps: Vec<i32> = reaches
                .iter()
                .chain(idle.iter().map(|(_, r)| r))
                .filter_map(|r| r.dist_at(x, y))
                .collect();
            if steps.len() < size {
                return None;
            }
            steps.sort_unstable();
            Some((steps[..size].iter().sum::<i32>(), (x, y)))
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
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    crew: Query<(Entity, &Squad, &Position, Option<&Path>, Option<&Away>)>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    for (mission_e, mut mission) in &mut missions {
        let Some(rule) = rules.0.get(mission.def) else {
            continue;
        };
        let Some(gate) = mission.gate else {
            continue; // отряд ещё не начали набирать
        };

        let squad: Vec<(Entity, (i32, i32), bool, bool)> = crew
            .iter()
            .filter(|(_, s, ..)| s.0 == mission_e)
            .map(|(e, _, p, path, away)| {
                let walking = path.is_some_and(|p| !p.steps.is_empty());
                (e, (p.x, p.y), walking, away.is_some())
            })
            .collect();

        // Отряд в поле. База о нём ничего не знает: ни усталости, ни маршрутов —
        // авторасчёт исхода придёт отдельным шагом, пока миссия просто идёт.
        if squad.iter().any(|&(.., away)| away) {
            mission.left -= 1;
            if mission.left > 0 {
                continue;
            }
            for &(cat_e, ..) in &squad {
                commands.entity(cat_e).remove::<(Away, Squad)>();
            }
            // Добыча ложится кучей на шлюз — ровно как возврат от сноса ложится
            // под ноги сносильщику. Развозит её обычная уборка (§12.16).
            for &(item, count) in &rule.loot {
                spill(&mut commands, &mut stacks, gate, item, count);
            }
            commands.entity(mission_e).despawn();
            continue;
        }

        // Уходим, только когда отряд в полном составе стоит на шлюзе: недобор
        // здесь — это не «пошли вдвоём вместо троих», а «ещё идут».
        let ready = squad.len() == rule.squad
            && squad
                .iter()
                .all(|&(_, at, walking, _)| at == gate && !walking);
        if ready {
            for &(cat_e, ..) in &squad {
                commands.entity(cat_e).insert(Away);
            }
        }
    }
}
