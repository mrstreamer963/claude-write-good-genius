//! Перемещение котов: шаги по маршруту, повтор приказов, выход из пустоты.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::{BaseMap, DIRS};
use crate::path::find_path;

/// Тиков между шагами юнита (при BASE_TPS=6 и периоде 1 — ~3 тайла/сек на ×1).
const MOVE_PERIOD: u8 = 1;

/// Двигает юнитов по маршруту; на прибытии снимает компоненты движения.
pub(crate) fn move_units(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Position, &mut Path, &mut MoveCooldown)>,
) {
    for (e, mut pos, mut path, mut cd) in &mut q {
        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
            continue;
        }
        if cd.0 > 0 {
            cd.0 -= 1;
            continue;
        }

        let next = *path.steps.last().unwrap();
        if !map.walkable(next.0, next.1) {
            let goal = *path.steps.first().unwrap();
            match find_path(&map, (pos.x, pos.y), goal) {
                Some(p) => path.steps = p,
                None => path.steps.clear(),
            }
            cd.0 = MOVE_PERIOD;
            continue;
        }

        path.steps.pop();
        pos.x = next.0;
        pos.y = next.1;
        cd.0 = MOVE_PERIOD;

        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
        }
    }
}

/// Пытается проложить маршрут котам с активным приказом (`Order`), у которых
/// сейчас нет пути — например, приказ был отдан до постройки коридора.
/// Снимает `Order`, когда цель достигнута.
///
/// Коты за работой — стройкой (`Assignment`), переносом (`Haul`), сном (`Rest`)
/// учёбой (`Study`), наукой (`Researching`) или в отряде (`Squad`) —
/// пропускаются: приказ не должен срывать кота с
/// начатой задачи. Он подхватится сам, как только задача снимется.
pub(crate) fn retry_orders(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut q: Query<
        (Entity, &Position, &mut Order),
        (
            Without<Path>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Crafting>,
            Without<Squad>,
        ),
    >,
) {
    for (e, pos, mut order) in &mut q {
        if (pos.x, pos.y) == (order.x, order.y) {
            commands.entity(e).remove::<Order>();
            continue;
        }
        // Карта не менялась с прошлой попытки — результат будет тот же.
        if order.tried_version == map.version {
            continue;
        }
        order.tried_version = map.version;
        if let Some(path) = find_path(&map, (pos.x, pos.y), (order.x, order.y)) {
            commands
                .entity(e)
                .insert((Path { steps: path }, MoveCooldown(0)));
        }
    }
}

/// Выводит кота со снесённой клетки на соседний пол обычным шагом.
///
/// Снос под котом разрешён — это штатная механика (дырки в перекрытиях, см.
/// `ideas.md`), а не ошибка (§12.10 concept.md).
///
/// Пересечь пустоту нельзя (`move_units` шагает только на проходимое), поэтому
/// «выход» — это всегда шаг на соседа; искать дальше смысла нет. Коты с
/// маршрутом или задачей выбираются сами: `find_path` не требует проходимости
/// стартовой клетки. Остаётся простаивающий кот — им и занимается эта система.
/// Если проходимых соседей нет, кот остаётся в пустоте (честное «замурован»)
/// и помечается флагом `stuck` в снапшоте. Состояние обратимо: игрок ставит пол
/// рядом, и кот выбирается — в том числе построив этот пол сам, изнутри.
pub(crate) fn escape_voids(
    map: Res<BaseMap>,
    mut commands: Commands,
    q: Query<(Entity, &Position), (With<UnitId>, Without<Path>, Without<Away>)>,
) {
    for (e, pos) in &q {
        if map.walkable(pos.x, pos.y) {
            continue;
        }
        // Порядок DIRS фиксирован, значит выбор соседа детерминирован (§11).
        if let Some(step) = DIRS
            .iter()
            .map(|(dx, dy)| (pos.x + dx, pos.y + dy))
            .find(|(nx, ny)| map.walkable(*nx, *ny))
        {
            commands
                .entity(e)
                .insert((Path { steps: vec![step] }, MoveCooldown(0)));
        }
    }
}

/// Разводит котов, оставшихся в одной клетке: пройти сквозь можно, встать
/// вместе — нет (§12.32).
///
/// Проходимость считается по тайлам, и кот в неё не входит: сделай его
/// препятствием — и маршруты начнут зависеть от того, кто где стоит в этот тик,
/// а двое встречных в коридоре шириной в клетку встанут намертво. Поэтому
/// правило касается только **остановки**, и разбирается оно после факта — тем
/// же приёмом, что `escape_voids` выводит кота из ямы, а `settle_stacks`
/// сдвигает кучу из пустоты. Иначе «клетка занята» пришлось бы вписать в
/// каждый из семи раздатчиков и однажды забыть в восьмом.
///
/// Остаётся в клетке занятый делом, при равенстве — первый по `id`; остальные
/// делают шаг на свободного соседа в фиксированном порядке `DIRS` (§11). Отойти
/// некуда — стоят вместе: это легальное состояние, как `stuck`.
///
/// **Отряд не трогаем**: сбор у шлюза по определению сводит котов в одну точку,
/// и `run_missions` ждёт, пока все встанут именно на неё (§12.22). Ушедших с
/// базы — тем более: их позиция это шлюз, а не место, где они есть.
pub(crate) fn spread_units(
    map: Res<BaseMap>,
    mut commands: Commands,
    all: Query<(&Position, Option<&Path>), (With<UnitId>, Without<Away>)>,
    stopped: Query<
        (
            Entity,
            &UnitId,
            &Position,
            Option<&Assignment>,
            Option<&Haul>,
            Option<&Rest>,
            Option<&Study>,
            Option<&Researching>,
            Option<&Crafting>,
        ),
        (With<UnitId>, Without<Path>, Without<Away>, Without<Squad>),
    >,
) {
    // Занято то, где кто-то **стоит**: идущий сквозь не мешает. Отряд у шлюза
    // сюда входит — вставать под него незачем, хоть он и не расходится сам.
    let mut blocked: Vec<(i32, i32)> = all
        .iter()
        .filter(|(_, path)| path.is_none())
        .map(|(p, _)| (p.x, p.y))
        .collect();

    // Кто с кем стоит. Порядок задан явно: занятый делом остаётся, дальше по
    // `id`, — иначе «кто отойдёт» зависело бы от истории вставок в ECS (§12.24).
    let mut standing: Vec<(bool, &str, Entity, (i32, i32))> = stopped
        .iter()
        .map(|(e, id, pos, job, haul, rest, study, research, craft)| {
            let at_work = job.is_some()
                || haul.is_some()
                || rest.is_some()
                || study.is_some()
                || research.is_some()
                || craft.is_some();
            (at_work, id.0.as_str(), e, (pos.x, pos.y))
        })
        .collect();
    standing.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));

    let mut kept: Vec<(i32, i32)> = Vec::new();
    for (_, _, cat_e, at) in standing {
        if !kept.contains(&at) {
            kept.push(at); // первый по правилу остаётся на месте
            continue;
        }
        let Some(step) = DIRS
            .iter()
            .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
            .find(|&(nx, ny)| map.walkable(nx, ny) && !blocked.contains(&(nx, ny)))
        else {
            continue; // отойти некуда — стоят вместе, это не ошибка
        };
        // Цель занимаем сразу: иначе двое из одной клетки шагнут в одну и ту же.
        blocked.push(step);
        commands
            .entity(cat_e)
            .insert((Path { steps: vec![step] }, MoveCooldown(0)));
    }
}

/// Кот ничего не может сделать сам — для подсветки в UI.
///
/// Два случая: замурован в пустоте без проходимых соседей, либо его приказ
/// сейчас невыполним (второе условие — ровно то, на котором `retry_orders`
/// раз за разом не находит путь; кот за работой или за сном застрявшим не
/// считается). Замурованный в пустоте виден даже спящим: выбираться ему
/// всё равно придётся, и игрок должен это видеть.
///
/// Ушедшего на миссию не касается ни то, ни другое: его позиция — это шлюз, с
/// которого он ушёл, и она ничего не говорит о том, где кот на самом деле.
pub(crate) fn is_stuck(map: &BaseMap, pos: &Position, tasks: Busy) -> bool {
    if tasks.away {
        return false;
    }
    let entombed = !map.walkable(pos.x, pos.y)
        && !DIRS
            .iter()
            .any(|(dx, dy)| map.walkable(pos.x + dx, pos.y + dy));
    entombed || (tasks.ordered && tasks.idle)
}

/// Чем кот занят — в том же наборе, что и фильтры `Without<…>` раздатчиков.
///
/// Отдельная структура, а не десяток аргументов: список задач общего слоя растёт
/// (`Assignment`, `Haul`, `Rest`, `Study`, `Researching`, `Crafting`, `Squad` —
/// и это не конец), и собирать его в каждой точке вызова заново значит однажды
/// забыть одну.
#[derive(Clone, Copy)]
pub(crate) struct Busy {
    /// Приказ игрока висит, но маршрута под него сейчас нет.
    pub(crate) ordered: bool,
    /// Ни одной задачи — то есть невыполнимый приказ и правда некому отменить.
    pub(crate) idle: bool,
    pub(crate) away: bool,
}

impl Busy {
    pub(crate) fn of(
        order: Option<&Order>,
        path: Option<&Path>,
        assignment: Option<&Assignment>,
        haul: Option<&Haul>,
        rest: Option<&Rest>,
        study: Option<&Study>,
        researching: Option<&Researching>,
        crafting: Option<&Crafting>,
        squad: Option<&Squad>,
        away: Option<&Away>,
    ) -> Self {
        Busy {
            ordered: order.is_some() && path.is_none(),
            idle: assignment.is_none()
                && haul.is_none()
                && rest.is_none()
                && study.is_none()
                && researching.is_none()
                && crafting.is_none()
                && squad.is_none(),
            away: away.is_some(),
        }
    }
}
