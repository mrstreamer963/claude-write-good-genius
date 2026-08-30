//! Перемещение котов: шаги по маршруту, повтор приказов, выход из пустоты.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::{BaseMap, DIRS};
use crate::path::{Reach, find_path};

/// Тиков между шагами юнита (при BASE_TPS=6 и периоде 1 — ~3 тайла/сек на ×1).
const MOVE_PERIOD: u8 = 1;

/// Сколько штук в куче стоит одного лишнего тика на шаг (§12.35).
const CLUTTER_PER_TICK: i32 = 8;

/// Потолок задержки от завала: дальше куча уже не растёт в помеху. Без него
/// склад, случайно высыпанный в коридор, встал бы стеной на сотню тиков.
const CLUTTER_MAX: u8 = 3;

/// Двигает юнитов по маршруту; на прибытии снимает компоненты движения.
///
/// **Кот числится в клетке, пока не дошёл** (§12.140). Шаг — это состояние мира
/// (`Stride`), а не мгновенное присваивание: кот занимает свою клетку весь шаг и
/// появляется в соседней ровно на прибытии. Отсюда две вещи разом — вид рисует
/// дорогу по честному прогрессу, а не догадывается по прошлому снимку, и снос
/// пола под идущим котом перестал быть гонкой: пропавшая клетка отменяет шаг, а
/// кот остаётся там, где стоял.
///
/// **Завал под лапами замедляет шаг** (§12.35): чем больше куча в клетке, тем
/// дольше шаг **в неё**, — до потолка. Считается только то, что валяется **на
/// полу**: сложенное в склад или на стеллаж — это порядок, а не завал, иначе
/// собственное хранилище становилось бы болотом, а уборка наказывала бы сама
/// себя. До §12.140 это была пауза **после** шага, и в игре она читалась
/// дёрганьем; теперь это цена входа, и кот через кучу бредёт.
///
/// **Маршрут этого не знает и знать не должен.** BFS считает шаги, а не время
/// (§11): дай ему веса — и коты начнут обходить кучи, которые сами же и пришли
/// разбирать, а «ближайший кот» в шести раздатчиках станет считаться иначе.
/// Завал — это цена прохода, а не крюк.
pub(crate) fn move_units(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    // `Without<Path>` не сужает выборку — у куч маршрута не бывает, — а делает
    // запросы непересекающимися по `Position`: идущих котов забирает `q`.
    stacks: Query<(&Position, &Stack), Without<Path>>,
    mut q: Query<(Entity, &mut Position, &mut Path, Option<&mut Stride>)>,
) {
    let clutter = clutter_map(&stacks, &map, &rules);

    for (e, mut pos, mut path, stride) in &mut q {
        // 1. Едем. Прибытие и следующий шаг случаются **в одном тике**: разведи
        //    их — и между клетками появится лишний тик стояния, то есть база
        //    станет вдвое медленнее.
        if let Some(mut s) = stride {
            // Пол, в который кот шагает, могли снести за время шага. Кот при
            // этом числится в своей клетке, поэтому терять нечего: шаг
            // отменяется, маршрут перекладывается с места.
            if !map.walkable(&rules, s.to.0, s.to.1) {
                commands.entity(e).remove::<Stride>();
                repath(&map, &rules, &mut path, (pos.x, pos.y));
                continue;
            }
            s.left -= 1;
            if s.left > 0 {
                continue;
            }
            pos.x = s.to.0;
            pos.y = s.to.1;
            // Клетка снимается с маршрута **на прибытии**, а не в начале шага:
            // пока кот идёт, она обязана оставаться в `steps` — по ней его
            // цель читают и другие (`assign_nap` считает клетку занятой, чтобы
            // двое не пришли на одну лежанку, §12.39). Сними её раньше — и
            // клетка, в которую кот уже шагает, для всех остальных пуста.
            path.steps.pop();
            commands.entity(e).remove::<Stride>();
        }

        // 2. Трогаемся. `Path` держится до самого прибытия — снять его раньше
        //    значит объявить идущего кота свободным (инвариант 5), и раздатчик
        //    отправил бы его от клетки, из которой он уже вышел.
        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, Stride)>();
            continue;
        }
        let next = *path.steps.last().unwrap();
        if !map.walkable(&rules, next.0, next.1) {
            repath(&map, &rules, &mut path, (pos.x, pos.y));
            continue;
        }
        let span = step_span(&clutter, next);
        commands.entity(e).insert(Stride {
            to: next,
            left: span,
            span,
        });
    }
}

/// Перекладывает маршрут с текущей клетки к прежней цели. Не вышло — маршрут
/// стирается, и кот освобождается следующим тиком: приказ ему перепроложит
/// `retry_orders`, когда карта изменится.
fn repath(map: &BaseMap, rules: &TileRules, path: &mut Path, from: (i32, i32)) {
    let Some(&goal) = path.steps.first() else {
        return;
    };
    match find_path(map, rules, from, goal) {
        Some(p) => path.steps = p,
        None => path.steps.clear(),
    }
}

/// Что и где валяется **на полу**: клетки хранения сюда не попадают вовсе.
fn clutter_map(
    stacks: &Query<(&Position, &Stack), Without<Path>>,
    map: &BaseMap,
    rules: &TileRules,
) -> Vec<((i32, i32), i32)> {
    let mut clutter: Vec<((i32, i32), i32)> = Vec::new();
    for (pos, stack) in stacks {
        if stack.count <= 0 || rules.capacity_of(map.tile_at(pos.x, pos.y)) > 0 {
            continue;
        }
        match clutter.iter_mut().find(|(at, _)| *at == (pos.x, pos.y)) {
            Some((_, n)) => *n += stack.count,
            None => clutter.push(((pos.x, pos.y), stack.count)),
        }
    }
    clutter
}

/// Сколько тиков занимает шаг **в** клетку `at`.
///
/// `MOVE_PERIOD + 1` — установившийся темп: тик, в который кот переставляет
/// лапы, плюс период остывания. Сверх него — завал в самой клетке (§12.35).
fn step_span(clutter: &[((i32, i32), i32)], at: (i32, i32)) -> u8 {
    MOVE_PERIOD + 1 + clutter_delay(clutter, at)
}

/// Сколько лишних тиков стоит шаг на клетку с завалом.
///
/// Задержка ложится на клетку, **в которую** кот шагает: он пробирается через
/// кучу. Так замедление видно там же, где видна куча.
fn clutter_delay(clutter: &[((i32, i32), i32)], at: (i32, i32)) -> u8 {
    let count = clutter
        .iter()
        .find(|(cell, _)| *cell == at)
        .map_or(0, |&(_, n)| n);
    (count / CLUTTER_PER_TICK).clamp(0, CLUTTER_MAX as i32) as u8
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
    rules: Res<TileRules>,
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
            Without<Equipping>,
            Without<Eating>,
            Without<Healing>,
            Without<Treating>,
            Without<OnDuty>,
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
        ),
    >,
) {
    for (e, pos, mut order) in &mut q {
        if (pos.x, pos.y) == (order.x, order.y) {
            commands.entity(e).remove::<Order>();
            continue;
        }
        // Карта не менялась с прошлой **провалившейся** попытки — результат
        // будет тот же. Отметка снимается на удаче: маршрут теряется и от сна,
        // и от раны, и от вылазки, и такому коту повтор нужен сразу, иначе он
        // остаётся «не может дойти» при живой дороге до цели.
        if order.tried_version == Some(map.version) {
            continue;
        }
        match find_path(&map, &rules, (pos.x, pos.y), (order.x, order.y)) {
            Some(path) => {
                order.tried_version = None;
                commands.entity(e).insert(Path { steps: path });
            }
            None => order.tried_version = Some(map.version),
        }
    }
}

/// Выводит кота с клетки, на которой ему стоять нельзя, на соседний пол
/// обычным шагом: снесённая клетка (пустота) или полка (§12.142).
///
/// Полка сюда попадает только из старого сохранения: в новой партии на неё
/// не встать вовсе. Правило одно на оба случая, и второго прохода ради
/// переходного состояния заводить не за чем.
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
    rules: Res<TileRules>,
    mut commands: Commands,
    q: Query<(Entity, &Position), (With<UnitId>, Without<Path>, Without<Away>)>,
) {
    for (e, pos) in &q {
        if map.walkable(&rules, pos.x, pos.y) {
            continue;
        }
        // Порядок DIRS фиксирован, значит выбор соседа детерминирован (§11).
        if let Some(step) = DIRS
            .iter()
            .map(|(dx, dy)| (pos.x + dx, pos.y + dy))
            .find(|(nx, ny)| map.walkable(&rules, *nx, *ny))
        {
            commands.entity(e).insert(Path { steps: vec![step] });
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
/// уходят на свободного соседа в фиксированном порядке `DIRS`, а если соседа
/// нет — маршрутом к ближайшей свободной клетке (`step_aside`, §12.32). Отойти
/// некуда — стоят вместе: это легальное состояние, как `stuck`, — но «некуда»
/// значит тупик, а не «соседи заняты».
///
/// **На заставленную клетку развод не отходит** (§12.39): `clear_solids` тут же
/// погнал бы кота обратно, а обратно — это клетка, из которой его развели.
/// Двое в одной клетке — состояние покоя, а качели между полкой и соседом —
/// нет.
///
/// **Отряд не трогаем**: сбор у шлюза по определению сводит котов в одну точку,
/// и `run_missions` ждёт, пока все встанут именно на неё (§12.22). Ушедших с
/// базы — тем более: их позиция это шлюз, а не место, где они есть.
pub(crate) fn spread_units(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
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
            Option<&Equipping>,
            Option<&Eating>,
            Option<&Healing>,
            Option<&Treating>,
            Option<&OnDuty>,
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
        .map(
            |(
                e,
                id,
                pos,
                job,
                haul,
                rest,
                study,
                research,
                craft,
                equip,
                eat,
                hurt,
                medic,
                duty,
            )| {
                let at_work = job.is_some()
                    || haul.is_some()
                    || rest.is_some()
                    || study.is_some()
                    || research.is_some()
                    || craft.is_some()
                    || equip.is_some()
                    || eat.is_some()
                    || hurt.is_some()
                    || medic.is_some()
                    || duty.is_some();
                (at_work, id.0.as_str(), e, (pos.x, pos.y))
            },
        )
        .collect();
    standing.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));

    let mut kept: Vec<(i32, i32)> = Vec::new();
    for (_, _, cat_e, at) in standing {
        if !kept.contains(&at) {
            kept.push(at); // первый по правилу остаётся на месте
            continue;
        }
        let Some(route) = step_aside(&map, &rules, &blocked, at) else {
            continue; // отойти некуда — стоят вместе, это не ошибка
        };
        // Цель занимаем сразу: иначе двое из одной клетки шагнут в одну и ту же.
        blocked.push(route[0]);
        commands.entity(cat_e).insert(Path { steps: route });
    }
}

/// Куда отойти лишнему из клетки: свободный сосед, иначе ближайшая свободная
/// клетка маршрутом (§12.32).
///
/// **Соседом дело не кончается.** Правило «двое не стоят в одной клетке»
/// железное, а «отойти некуда» — это тупик, а не «все четыре соседа заняты».
/// Комната лежанок три на три, девять котов и девять мест: восьмеро разошлись,
/// девятый оказался в углу, где сосед слева и сосед сверху — коты, а справа и
/// снизу край карты, — и остался стоять под чужим силуэтом навсегда, при
/// пустующей лежанке через клетку. Один шаг разводит только тех, кому повезло
/// с геометрией.
///
/// Обход один на кота и только когда соседа не нашлось: в тесноте он и правда
/// нужен, а в обычном коридоре первый же `DIRS` отвечает раньше. Ничью решает
/// не порядок обхода ECS, а `(шаги, y, x)` — детерминизм тот же, что у `DIRS`
/// (§11).
fn step_aside(
    map: &BaseMap,
    rules: &TileRules,
    blocked: &[(i32, i32)],
    at: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    let free = |c: (i32, i32)| map.walkable(rules, c.0, c.1) && !blocked.contains(&c);
    // Порядок DIRS фиксирован, значит выбор соседа детерминирован (§11).
    if let Some(step) = DIRS
        .iter()
        .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
        .find(|&c| free(c))
    {
        return Some(vec![step]);
    }
    let reach = Reach::all(map, rules, at);
    let mut best: Option<(i32, i32, i32)> = None; // (шаги, y, x)
    for y in 0..map.height {
        for x in 0..map.width {
            if !free((x, y)) {
                continue;
            }
            let Some(d) = reach.dist_at(x, y).filter(|&d| d > 0) else {
                continue;
            };
            if best.is_none_or(|b| (d, y, x) < b) {
                best = Some((d, y, x));
            }
        }
    }
    let (_, y, x) = best?;
    reach.path_to(x, y)
}

/// Кот ничего не может сделать сам — для подсветки в UI.
///
/// Два случая: замурован — уйти некуда, — либо его приказ сейчас невыполним
/// (второе условие — ровно то, на котором `retry_orders` раз за разом не
/// находит путь; кот за работой или за сном застрявшим не считается).
/// Замурованный виден даже спящим: выбираться ему всё равно придётся, и игрок
/// должен это видеть.
///
/// **Замурован — это «ни одного проходимого соседа», а не «стою в пустоте»**
/// (§12.144). До §12.142 запереть кота могла только яма, и клетка под лапами
/// входила в условие. Полки стали непроходимы, и появился второй способ: кот
/// стоит на нормальном полу, а по всем четырём сторонам стеллажи. Раздатчики
/// такого кота видят свободным, работы ему не достаётся ни одной, и панель
/// говорит «без дела» — то есть отказ без причины ровно там, где §12.53 требует
/// слово. Проверять клетку под лапами больше не нужно вовсе: стоящему в яме
/// уйти тоже некуда, если некуда шагнуть, а если есть куда — его уводит
/// `escape_voids` тем же тиком.
///
/// Карман шире одной клетки (кот ходит внутри, но наружу хода нет) сюда не
/// попадает намеренно: это обход области на каждого кота каждым кадром, а
/// заводить такой карман больше нечем — жест, отрезающий кусок базы, отклоняет
/// правило доступа (§12.111).
///
/// Ушедшего на миссию не касается ни то, ни другое: его позиция — это шлюз, с
/// которого он ушёл, и она ничего не говорит о том, где кот на самом деле.
pub(crate) fn is_stuck(map: &BaseMap, rules: &TileRules, pos: &Position, tasks: Busy) -> bool {
    if tasks.away {
        return false;
    }
    let entombed = !DIRS
        .iter()
        .any(|(dx, dy)| map.walkable(rules, pos.x + dx, pos.y + dy));
    entombed || (tasks.ordered && tasks.idle)
}

/// Ключ занятия «спит» — он же «идёт спать», если при этом есть маршрут.
///
/// Именованными эти два стали с §12.99: по ним `needs::Toil` узнаёт лежащего
/// кота, а лежащий не тратит бодрости. Литералом в двух местах ключ разошёлся
/// бы молча — панель показывала бы сон, а расход считал бы работу.
pub(crate) const JOB_REST: &str = "rest";

/// Ключ занятия «дремлет»: задачи нет, а место для сна под лапами (§12.52).
pub(crate) const JOB_NAP: &str = "nap";

/// Чем кот занят — в том же наборе, что и фильтры `Without<…>` раздатчиков.
///
/// Отдельная структура, а не десяток аргументов: список задач общего слоя растёт
/// (`Assignment`, `Haul`, `Rest`, `Study`, `Researching`, `Crafting`, `Equipping`,
/// `Eating`, `Healing`, `Treating`, `Squad` — и это не конец), и собирать его в
/// каждой точке вызова заново значит однажды забыть одну.
///
/// Отсюда же берётся `job` — то, что видит игрок в карточке кота (§12.41).
/// Второй такой же разбор задач в снапшоте был бы ровно тем дублированием,
/// ради которого эта структура и заведена: одна из двух копий однажды отстанет
/// на новую задачу, и кот молча станет «бездельником».
#[derive(Clone, Copy)]
pub(crate) struct Busy {
    /// Приказ игрока висит, но маршрута под него сейчас нет.
    pub(crate) ordered: bool,
    /// Ни одной задачи — то есть невыполнимый приказ и правда некому отменить.
    pub(crate) idle: bool,
    pub(crate) away: bool,
    /// Ключ текущего занятия для панели; текст живёт в UI, как подписи тайлов.
    /// Пустая строка — кот свободен.
    pub(crate) job: &'static str,
    /// Кот **в пути** к своему делу, а не за ним. Отдельно от `job`, потому что
    /// «идёт спать» и «спит» — разные картинки при одной задаче, и различает их
    /// везде один и тот же признак: маршрут ещё есть.
    pub(crate) moving: bool,
}

impl Busy {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn of(
        order: Option<&Order>,
        path: Option<&Path>,
        assignment: Option<&Assignment>,
        haul: Option<&Haul>,
        rest: Option<&Rest>,
        study: Option<&Study>,
        researching: Option<&Researching>,
        crafting: Option<&Crafting>,
        equipping: Option<&Equipping>,
        eating: Option<&Eating>,
        healing: Option<&Healing>,
        treating: Option<&Treating>,
        squad: Option<&Squad>,
        duty: Option<&OnDuty>,
        away: Option<&Away>,
        // Место для сна под лапами и бодрость не полная — всё, что нужно знать
        // о дремоте снаружи (§12.52). Компонента у неё нет и не должно быть,
        // поэтому приходит она признаком от вызывающего: карта и рулсет сюда
        // не ходят. Задача ли это, решает уже сам разбор — последним пунктом.
        bed: bool,
    ) -> Self {
        // Порядок разбора — тот же, что у раздатчиков (§12.15): задачи друг
        // друга исключают фильтрами, так что выбирать обычно не из чего, но там,
        // где всё-таки есть (кот в отряде идёт одеваться, §12.34), показать надо
        // то же, что решила симуляция. `away` первым: ушедшего с базы не
        // касается ни одна задача, а `Squad` у пленного и вовсе нет (§12.40).
        let job = if away.is_some() {
            "away"
        } else if healing.is_some() {
            "heal"
        } else if eating.is_some() {
            "eat"
        } else if rest.is_some() {
            JOB_REST
        } else if treating.is_some() {
            "treat"
        } else if equipping.is_some() {
            "equip"
        } else if squad.is_some() {
            "squad"
        } else if haul.is_some() {
            "haul"
        } else if researching.is_some() {
            "research"
        } else if crafting.is_some() {
            "craft"
        } else if study.is_some() {
            "study"
        } else if duty.is_some() {
            // Ниже работ и выше приказа: дежурство раздаётся предпоследним, и
            // порядок разбора повторяет порядок раздатчиков (§12.60).
            "relay"
        } else if assignment.is_some() {
            // Снос от стройки здесь не отличить: `Busy` знает, какая задача, а
            // не во что она обернулась, — чертёж читает снапшот (§12.41).
            "build"
        } else if order.is_some() {
            "order"
        } else if bed && path.is_none() {
            // Последней: дремлет тот, у кого не нашлось вообще ничего, — сюда
            // разбор доходит, только перебрав все задачи. Маршрут исключаем
            // явно: идущий по своим делам кот проходит мимо лежанки, а не
            // дремлет на ней (§12.52).
            JOB_NAP
        } else {
            ""
        };
        Busy {
            ordered: order.is_some() && path.is_none(),
            idle: assignment.is_none()
                && haul.is_none()
                && rest.is_none()
                && study.is_none()
                && researching.is_none()
                && crafting.is_none()
                && equipping.is_none()
                && eating.is_none()
                && healing.is_none()
                && treating.is_none()
                && squad.is_none()
                && duty.is_none(),
            away: away.is_some(),
            job,
            moving: path.is_some(),
        }
    }
}
