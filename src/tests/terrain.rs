//! Клетка под лапами: где нельзя оставаться и что замедляет шаг (§12.35).
//!
//! Оба правила — про **пол, а не про задачу**: и стеллаж, и завал ничего не
//! запрещают раздатчикам и не меняют маршрут. Стеллаж сгоняет с себя после
//! факта (`clear_solids`), как `spread_units` разводит двоих из одной клетки
//! (§12.32); куча берёт своё временем шага, а BFS о ней не знает вовсе.
//!
//! В схеме `sim_from` заставленных тайлов нет — их включает сам тест
//! (`set_solid`), как цену и ёмкость.

use super::*;

/// Тайл `1` в этих тестах — стеллаж.
const RACK: i16 = 1;

// --- на стеллаж не встают и сквозь него не ходят (§12.142) -------------------

/// Стеллаж непроходим: мебель до потолка, сквозь которую ходят, читается
/// игроком как баг. Маршрут через него не прокладывается вовсе.
#[test]
fn a_rack_is_not_walkable() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(2, 1, RACK); // стеллаж посреди коридора

    assert!(sim.set_target("a", 3, 1), "приказ принимается: клетка есть");
    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (1, 1), "но пройти сквозь стеллаж нельзя");
    assert!(sim.stuck_of("a"), "и это видно игроку, а не молчит");
}

/// Приказ на саму полку отклоняется, как приказ в пустоту: встать туда нельзя,
/// и молчаливо принятый приказ был бы отказом без причины (§12.53).
#[test]
fn an_order_onto_a_rack_is_refused() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(3, 1, RACK);

    assert!(!sim.set_target("a", 3, 1), "на полку не приказывают");
}

/// Кот, оставшийся на полке от старого сохранения, сходит сам: правило то же,
/// что у ямы (инвариант 8), и отдельной системы под него больше нет.
#[test]
fn a_cat_left_on_a_rack_steps_off() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK); // полку поставили прямо под котом

    sim.tick_n(5);
    assert_eq!(sim.pos_of("a"), (2, 1), "сошёл на соседнюю клетку");
}

/// Сойти некуда — кот остаётся: как и `stuck`, это легальное состояние, а не
/// ошибка (§12.10).
#[test]
fn a_cat_stays_when_there_is_nowhere_to_step() {
    let mut sim = sim_from(&["###", "#a#", "###"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK);

    sim.tick_n(5);
    assert_eq!(sim.pos_of("a"), (1, 1), "деваться некуда");
}

/// Развод двоих из одной клетки на стеллаж не загоняет — теперь по той же
/// причине, по которой не загоняет в пустоту: туда просто не шагают.
#[test]
fn spreading_never_pushes_a_cat_onto_a_rack() {
    // Тупик на две клетки: сойти со стеллажа можно только под соседа.
    let mut sim = sim_from(&["####", "#ab#", "####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK);

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (2, 1), "сошёл со стеллажа");
    assert_eq!(
        sim.pos_of("b"),
        (2, 1),
        "и стоит с соседом: разойтись некуда"
    );

    // И это состояние покоя, а не качели «согнали — вернулся».
    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (2, 1), "никто не бегает по кругу");
    assert_eq!(sim.pos_of("b"), (2, 1), "и второй тоже");
}

/// Полка остаётся складом: носильщик сдаёт груз **с соседней клетки**, а сам на
/// неё не встаёт ни разу.
#[test]
fn a_hauler_works_a_rack_from_a_neighbour() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    sim.set_solid(RACK, true);
    sim.set_capacity(RACK, 10);
    sim.force_tile(4, 1, RACK); // стеллаж в конце коридора
    sim.put_item(1, 1, 0, 5); // и лом, который туда свезут

    for _ in 0..40 {
        sim.tick_n(1);
        assert_ne!(sim.pos_of("a"), (4, 1), "на полку он не встаёт никогда");
    }
    assert_eq!(sim.item_at(4, 1, 0), 5, "но груз на полке");
    assert_eq!(sim.pos_of("a"), (3, 1), "сдавал с прохода");
}

/// Зеркало предыдущего: с полки груз **берут** тоже с соседней клетки, и идёт
/// носильщик именно туда. Маршрута до самой полки не существует (она
/// непроходима), а `unwrap_or_default()` превращает его в пустой — то есть кот
/// «доходит», не сходя с места, и берёт с пустого пола. Так замирала вся база:
/// каждый тик каждому коту выдавалась ходка и тем же тиком снималась.
#[test]
fn a_hauler_takes_from_a_rack_from_a_neighbour() {
    const WALL: i16 = 2;
    let mut sim = sim_from(&["#######", "#a....#", "#######"]);
    sim.set_solid(RACK, true);
    sim.set_capacity(RACK, 10);
    sim.force_tile(5, 1, RACK); // стеллаж в конце коридора
    sim.put_item(5, 1, 0, 5); // и весь лом базы на нём
    sim.set_cost(WALL, 3);
    assert!(sim.add_blueprint(3, 1, WALL as i32), "площадка размечена");

    for _ in 0..60 {
        sim.tick_n(1);
        assert_ne!(sim.pos_of("a"), (5, 1), "на полку он не встаёт никогда");
    }
    assert_eq!(sim.item_at(5, 1, 0), 2, "три лома уехали с полки");
    assert_eq!(sim.tile(3, 1), WALL, "и стена на них построена");
}

/// Куча на полке остаётся на полке: полка непроходима, но это не пустота, и
/// `settle_stacks` её не трогает. Иначе склад съезжал бы на пол сам собой.
#[test]
fn a_pile_on_a_rack_stays_there() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.set_capacity(RACK, 10);
    sim.force_tile(3, 1, RACK);
    sim.put_item(3, 1, 0, 4);

    sim.tick_n(20);
    assert_eq!(sim.item_at(3, 1, 0), 4, "куча никуда не уехала");
}

/// Спящего полка тоже не держит: встать на неё нельзя, а поставленная под
/// спящим — выводит его на соседнюю клетку, где он досыпает (§12.33).
#[test]
fn a_sleeping_cat_is_moved_off_a_rack() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK);
    sim.set_needs(100, 90, 1);
    sim.set_energy("a", 0);

    sim.tick_n(1);
    assert!(sim.is_resting("a"), "свалился от истощения");
    sim.tick_n(5);
    assert_ne!(sim.pos_of("a"), (1, 1), "но не на стеллаже");
    assert!(sim.is_resting("a"), "и спит дальше");
}

/// Полка, к которой не подойти, никого не вешает: раздатчики её просто не
/// видят, и кот работает дальше. Легальное состояние, как `stuck` (§12.10) —
/// разметка такого не допустит (§12.111), но снос подхода допустит.
#[test]
fn an_unreachable_rack_does_not_hang_anyone() {
    // Полка в (4, 1) отрезана пустотой со всех сторон: подхода нет вовсе.
    let mut sim = sim_from(&["######", "#a.#.#", "######"]);
    sim.set_solid(RACK, true);
    sim.set_capacity(RACK, 10);
    sim.force_tile(4, 1, RACK);
    sim.put_item(1, 1, 0, 3); // и лом, который некуда убрать

    sim.tick_n(30);
    assert_eq!(sim.item_at(1, 1, 0), 3, "лом остался лежать на виду");
    assert_eq!(
        sim.item_at(3, 1, 0),
        0,
        "на недостижимую полку ничего не уехало"
    );
    assert!(!sim.stuck_of("a"), "и кот не считается застрявшим");
}

// --- шаг длится, а не случается (§12.140) ------------------------------------

/// **Кот числится в клетке, пока не дошёл.** Шаг — состояние мира, а не
/// мгновенное присваивание: между A и B есть промежуток, и он виден снаружи.
/// До §12.140 позиция флипалась в начале шага, и нарисовать дорогу было нечем.
#[test]
fn a_cat_stays_in_its_cell_until_it_arrives() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    assert!(sim.set_target("a", 3, 1), "приказ принят");

    sim.tick_n(1);
    assert_eq!(sim.pos_of("a"), (1, 1), "тронулся, но клетки ещё не сменил");
    assert_eq!(
        sim.stride_of("a"),
        Some(((2, 1), 2, 2)),
        "а шаг уже виден: куда идёт и сколько тиков осталось",
    );

    sim.tick_n(1);
    assert_eq!(sim.pos_of("a"), (1, 1), "на середине шага — всё ещё здесь");
    assert_eq!(sim.stride_of("a").map(|(_, left, _)| left), Some(1));

    sim.tick_n(1);
    assert_eq!(
        sim.pos_of("a"),
        (2, 1),
        "и появился в новой ровно на приходе"
    );
}

/// Прибытие и следующий шаг случаются **в одном тике**: разведи их — и между
/// клетками появится лишний тик стояния, то есть база станет вдвое медленнее.
#[test]
fn arriving_starts_the_next_step_at_once() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    assert!(sim.set_target("a", 4, 1), "приказ принят");

    sim.tick_n(3);
    assert_eq!(sim.pos_of("a"), (2, 1), "первая клетка пройдена");
    assert_eq!(
        sim.stride_of("a"),
        Some(((3, 1), 2, 2)),
        "и шаг к следующей начат тем же тиком, а не следующим",
    );
}

/// Завал сидит в длительности самого шага, а не в паузе после него: кот через
/// кучу **бредёт**. До §12.140 это была задержка на месте, и в игре она
/// читалась дёрганьем.
#[test]
fn clutter_stretches_the_step_itself() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, 0, 64);

    assert!(sim.set_target("a", 3, 1), "приказ принят");
    sim.tick_n(1);
    let (to, _, span) = sim.stride_of("a").expect("кот пошёл");
    assert_eq!(to, (2, 1), "шагает в заваленную клетку");
    assert!(span > 2, "и шаг туда длиннее обычного: {span}");
}

/// Пол, в который кот шагает, могли снести за время шага. Кот числится в своей
/// клетке, поэтому терять нечего: шаг отменяется, а маршрут перекладывается с
/// места. До §12.140 кот успевал оказаться в клетке, которой уже нет.
#[test]
fn demolishing_the_next_cell_cancels_the_step() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    assert!(sim.set_target("a", 3, 1), "приказ принят");
    sim.tick_n(1);
    assert_eq!(sim.stride_of("a").map(|(to, _, _)| to), Some((2, 1)));

    sim.force_tile(2, 1, -1); // пол из-под лапы убрали
    sim.tick_n(1);
    assert_eq!(sim.pos_of("a"), (1, 1), "кот остался там, где стоял");
    assert_eq!(sim.stride_of("a"), None, "а шаг отменён");
}

// --- завал замедляет --------------------------------------------------------

/// Куча под лапами стоит времени: чем больше завал, тем дольше через него
/// пробираться.
#[test]
fn a_pile_slows_the_step() {
    let clean = walk_ticks(0);
    let small = walk_ticks(8);
    let big = walk_ticks(64);

    assert!(
        small > clean,
        "через кучу идти дольше: {small} против {clean}"
    );
    assert!(
        big > small,
        "через завал — ещё дольше: {big} против {small}"
    );
}

/// У задержки есть потолок: склад, высыпанный в коридор, не встаёт стеной.
#[test]
fn the_slowdown_has_a_ceiling() {
    assert_eq!(
        walk_ticks(64),
        walk_ticks(6400),
        "выше потолка куча в помеху уже не растёт",
    );
}

/// Сложенное на склад — порядок, а не завал: иначе собственное хранилище
/// становилось бы болотом, а уборка наказывала бы сама себя.
#[test]
fn storage_does_not_slow_anyone_down() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_capacity(RACK, 100);
    sim.force_tile(2, 1, RACK);
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, 0, 64); // столько же, сколько в «завале» выше

    assert!(sim.set_target("a", 3, 1), "приказ принят");
    let ticks = ticks_to_arrive(&mut sim, (3, 1));
    assert_eq!(
        ticks,
        Some(walk_ticks(0)),
        "по складу кот идёт ровно так же, как по чистому полу",
    );
}

/// Маршрут о завалах не знает: BFS считает шаги, а не время (§11, §12.35).
/// Дай ему веса — и коты начнут обходить кучи, которые сами же пришли разбирать.
#[test]
fn the_path_ignores_clutter() {
    // Два пути до (3,1) одной длины: верхний коридор завален, нижний чист.
    let mut sim = sim_from(&["#####", "#a..#", "#...#", "#####"]);
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, 0, 64);

    assert!(sim.set_target("a", 3, 1), "приказ принят");
    sim.tick_n(1);
    assert_eq!(
        sim.stride_of("a").map(|(to, _, _)| to),
        Some((2, 1)),
        "кот пошёл напрямик, через кучу: обход стоил бы лишнего шага",
    );
}

// --- полке нужен подход (§12.111) -------------------------------------------

/// Полка внутри полок незаконна: подойти к ней негде, а сквозь соседнюю мебель
/// груз не сдают.
#[test]
fn a_rack_needs_an_aisle() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);
    for (x, y) in [(3, 1), (2, 2), (4, 2), (3, 3)] {
        sim.force_tile(x, y, RACK); // полки вокруг клетки (3, 2)
    }

    assert!(
        !sim.add_blueprint(3, 2, RACK as i32),
        "к этой полке не подойти ни с одной стороны",
    );
    assert!(
        sim.add_blueprint(5, 4, RACK as i32),
        "а полка с проходом рядом размечается как раньше",
    );
}

/// Ворота стоят и на соседе: иначе правило снимается за два клика — полки
/// вдоль прохода, а потом сам проход полкой.
#[test]
fn a_rack_cannot_seal_its_neighbour() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);
    for (x, y) in [(2, 1), (1, 2), (2, 3)] {
        sim.force_tile(x, y, RACK); // у полки (2, 2) остался один подход — (3, 2)
    }
    sim.force_tile(2, 2, RACK);

    assert!(
        !sim.add_blueprint(3, 2, RACK as i32),
        "эта разметка отняла бы у соседней полки последний подход",
    );
    assert!(
        sim.add_blueprint(4, 2, RACK as i32),
        "клеткой дальше — пожалуйста: проход у соседа остаётся",
    );
}

/// Мазок рамкой монолита не собирает: одна клетка остаётся проходом. Какая
/// именно — решает порядок обхода рамки, и он детерминирован.
#[test]
fn a_rect_of_racks_keeps_an_aisle() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);

    sim.add_blueprint_rect(2, 1, 3, 3, RACK as i32);
    let planned = (1..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(RACK))
        .count();
    assert_eq!(
        planned, 7,
        "семь полок из девяти: девятая клетка — проход, а восьмая заперла бы          его в кармане (§12.144)",
    );

    sim.tick_n(600); // коту хватит достроить всё размеченное
    assert!(
        sim.solid_without_aisle().is_empty(),
        "и построенное правилу не противоречит",
    );
}

/// Полка, отрезающая кусок базы, отклоняется — даже если подход у неё самой
/// есть (§12.144). Правило доступа спрашивает не только «подойду ли я к ней»,
/// но и «останется ли подход ко всему, что за ней».
#[test]
fn a_rack_that_seals_a_pocket_is_refused() {
    let mut sim = sim_from(&["#####", "#a..#", "#...#", "#####"]);
    sim.set_solid(RACK, true);

    sim.add_blueprint_rect(2, 1, 1, 1, RACK as i32);
    assert_eq!(sim.planned_tile(2, 1), Some(RACK), "первая полка законна");

    // А эта заперла бы кота в углу: подход у самой полки есть — (2, 2), — но
    // клетка (1, 1) осталась бы без выхода.
    sim.add_blueprint_rect(1, 2, 1, 1, RACK as i32);
    assert_eq!(sim.planned_tile(1, 2), None, "запирающая полка отклонена");
    assert_eq!(
        sim.buildable(RACK as i32, 1, 2, 1, 1)[2 * 5 + 1],
        0,
        "и превью перечёркивает её до жеста",
    );
}

/// Два ряда полок законны целиком: подход у каждой сверху или снизу. Правило
/// запрещает клетку, у которой полки со **всех четырёх** сторон, а не третий
/// ряд как таковой.
#[test]
fn two_rows_of_racks_are_legal() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);

    sim.add_blueprint_rect(2, 2, 3, 2, RACK as i32);
    let planned = (2..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(RACK))
        .count();
    assert_eq!(planned, 6, "блок три на два размечается целиком");
}

/// Правило висит на `solid`, а не на списке тайлов: комната-склад, лежанки и
/// гнёзда остаются комнатами (§12.16).
#[test]
fn only_solid_tiles_need_an_aisle() {
    const STORAGE: i16 = 2;
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_capacity(STORAGE, 20); // склад, но пройти и стоять на нём можно

    sim.add_blueprint_rect(2, 1, 3, 3, STORAGE as i32);
    let planned = (1..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(STORAGE))
        .count();
    assert_eq!(planned, 9, "склад размечается целой комнатой");
}

/// Сноса ворота не касаются (§12.27): он создаёт пустоту, а не полки, и новых
/// мест хранения от него не прибавляется.
#[test]
fn demolition_is_not_gated_by_the_aisle_rule() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(2, 1, RACK); // единственный подход к ней — (3, 1)

    assert!(sim.plan_demolish(3, 1), "снос прохода планируется");
}

/// Маску превью считает ядро, и она говорит ровно то же, что ворота: второй
/// экземпляр правила в JS однажды показал бы клетку, которую фасад отклонит.
#[test]
fn the_mask_says_what_the_gate_says() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);
    for (x, y) in [(3, 1), (2, 2), (4, 2), (3, 3)] {
        sim.force_tile(x, y, RACK);
    }

    assert!(
        sim.buildable(0, 0, 0, 0, 0).is_empty(),
        "у обычного пола ограничений нет вовсе — и красить нечего",
    );
    let mask = sim.buildable(RACK as i32, 0, 0, 0, 0);
    assert_eq!(mask.len(), 7 * 6, "по байту на клетку карты");
    assert_eq!(mask[2 * 7 + 3], 0, "клетка внутри полок закрыта");
    assert_eq!(mask[4 * 7 + 5], 1, "клетка с проходом открыта");
    assert_eq!(
        mask[2 * 7 + 3] == 1,
        sim.add_blueprint(3, 2, RACK as i32),
        "маска и ворота отвечают одно",
    );
}

/// Маска рамки считает мазок целиком: девять зелёных клеток при восьми
/// размеченных — это молчаливое расхождение, ради которого маска и заводится.
#[test]
fn the_mask_counts_the_whole_stroke() {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_solid(RACK, true);

    let mask = sim.buildable(RACK as i32, 2, 1, 3, 3);
    let open = (1..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| mask[(y * 7 + x) as usize] == 1)
        .count();
    assert_eq!(
        open, 7,
        "две клетки мазка уйдут: одна под проход, вторая — чтобы к нему подойти",
    );

    sim.add_blueprint_rect(2, 1, 3, 3, RACK as i32);
    let planned = (1..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(RACK) && mask[(y * 7 + x) as usize] == 1)
        .count();
    assert_eq!(
        planned, 7,
        "и это ровно те клетки, которые отклонит разметка"
    );
}

// --- зонирование: шум и грязь (§12.157) --------------------------------------

/// Тайлы зонирования в этих тестах: спальня и цех.
const BED: i16 = 2;
const SHOP: i16 = 3;

/// Готовая база с парой «тишина»: цех шумит, лежанке нужна тишина.
fn sim_with_zones() -> Sim {
    let mut sim = sim_from(&[
        "#######", "#a....#", "#.....#", "#.....#", "#.....#", "#######",
    ]);
    sim.set_quiet(BED, true);
    sim.set_noisy(SHOP, true);
    sim
}

/// Лежанка не встаёт боком к цеху: спальня, в которой не спят, — это отказ
/// механики, а не украшение.
#[test]
fn a_bed_is_refused_next_to_a_workshop() {
    let mut sim = sim_with_zones();
    sim.force_tile(3, 2, SHOP);

    assert!(!sim.add_blueprint(3, 1, BED as i32), "сверху нельзя");
    assert!(!sim.add_blueprint(2, 2, BED as i32), "и сбоку тоже");
    assert!(sim.add_blueprint(3, 4, BED as i32), "через клетку — можно");
}

/// Правило про **пару**, а не про того, кого поставили вторым: запрет
/// симметричен.
#[test]
fn zoning_is_symmetric() {
    let mut sim = sim_with_zones();
    sim.force_tile(3, 2, BED);

    assert!(
        !sim.add_blueprint(3, 1, SHOP as i32),
        "цех к лежанке — тот же запрет, что лежанка к цеху",
    );
}

/// Зазор от углов не растёт: полоски пола в одну клетку по-прежнему довольно —
/// комната за ней соседом уже не считается.
#[test]
fn one_tile_of_floor_separates_the_zones() {
    let mut sim = sim_with_zones();
    sim.force_tile(2, 1, SHOP);
    sim.force_tile(3, 1, SHOP);

    assert!(
        sim.add_blueprint(2, 3, BED as i32),
        "через ряд пола — можно"
    );
    assert!(sim.add_blueprint(3, 3, BED as i32), "и соседняя тоже");
}

/// Углы считаются наравне со сторонами: цех наискосок от лежанки стоит к ней
/// ближе, чем цех через клетку пола, и пропущенная пара читалась бы как дырка
/// в правиле.
#[test]
fn a_corner_touch_is_refused_too() {
    let mut sim = sim_with_zones();
    sim.force_tile(2, 2, SHOP);

    assert!(!sim.add_blueprint(3, 3, BED as i32), "угол к углу — нельзя");
    assert!(
        !sim.add_blueprint(1, 1, BED as i32),
        "и с другой стороны тоже"
    );
}

/// Одинаковые роли рядом законны: правило про несовместимую пару, а не про
/// «любой тип не касается любого».
#[test]
fn the_same_role_packs_together() {
    let mut sim = sim_with_zones();

    assert!(
        sim.add_blueprint_rect(2, 1, 3, 2, BED as i32),
        "спальня целиком"
    );
    let planned = (1..3)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(BED))
        .count();
    assert_eq!(planned, 6, "шесть лежанок из шести");
}

/// Чертёж считается наравне с построенным: иначе правило снимается за два
/// клика — тем же мазком, ради которого оно и заводилось (§12.111).
#[test]
fn a_blueprint_counts_as_a_neighbour() {
    let mut sim = sim_with_zones();

    assert!(sim.add_blueprint(3, 2, SHOP as i32), "цех размечен");
    assert!(
        !sim.add_blueprint(3, 1, BED as i32),
        "лежанка рядом с обещанным цехом отклонена — строить его ещё не начали",
    );
}

/// Сноса ворота не касаются, как и у правила доступа (§12.27): у пустоты все
/// четыре свойства нули.
#[test]
fn demolition_is_not_gated_by_zoning() {
    let mut sim = sim_with_zones();
    sim.force_tile(3, 2, SHOP);
    sim.force_tile(3, 1, BED); // досталось от старого сохранения

    assert!(sim.plan_demolish(3, 1), "снос планируется");
}

/// Маска говорит ровно то же, что ворота, — и считается теперь не только для
/// полок: у тайла с зонированием она тоже есть.
#[test]
fn the_mask_says_what_the_zoning_gate_says() {
    let mut sim = sim_with_zones();
    sim.force_tile(3, 2, SHOP);

    let mask = sim.buildable(BED as i32, 0, 0, 0, 0);
    assert_eq!(mask.len(), 7 * 6, "по байту на клетку карты");
    assert_eq!(mask[7 + 3], 0, "клетка над цехом закрыта");
    assert_eq!(mask[4 * 7 + 3], 1, "клетка через одну открыта");
    assert_eq!(
        mask[7 + 3] == 1,
        sim.add_blueprint(3, 1, BED as i32),
        "маска и ворота отвечают одно",
    );
}

/// Маска рамки считает мазок целиком и здесь, и это не формальность: мазок,
/// проходящий **по самому цеху**, его же и стирает — значит клетки за ним
/// перестают конфликтовать по ходу дела. Посчитай маска каждую клетку
/// независимо, и превью перечеркнуло бы то, что разметка примет.
#[test]
fn the_zoning_mask_counts_the_whole_stroke() {
    let mut sim = sim_with_zones();
    sim.force_tile(2, 2, SHOP);

    let mask = sim.buildable(BED as i32, 1, 2, 4, 1);
    assert_eq!(mask[2 * 7 + 1], 0, "слева от цеха нельзя: он ещё стоит");
    assert_eq!(mask[2 * 7 + 2], 1, "поверх самого цеха — можно");
    assert_eq!(
        mask[2 * 7 + 3],
        1,
        "а справа уже можно: этим же мазком цеха не станет",
    );

    sim.add_blueprint_rect(1, 2, 4, 1, BED as i32);
    let planned: Vec<i32> = (1..5)
        .map(|x| i32::from(sim.planned_tile(x, 2) == Some(BED)))
        .collect();
    assert_eq!(planned, vec![0, 1, 1, 1], "и разметка отвечает то же самое");
}

/// Правило не про роль клетки, а про её свойства: тайл без флагов маске не
/// подчиняется вовсе, и красить у него нечего.
#[test]
fn a_tile_without_flags_has_no_mask() {
    let mut sim = sim_with_zones();

    assert!(
        sim.buildable(0, 0, 0, 0, 0).is_empty(),
        "у обычного пола ограничений нет вовсе",
    );
}

/// Двое ворот на одном тайле не мешают друг другу: стеллаж и заставлен, и
/// грязнит.
#[test]
fn both_gates_hold_on_one_tile() {
    let mut sim = sim_from(&["#######", "#a....#", "#.....#", "#.....#", "#######"]);
    sim.set_solid(RACK, true);
    sim.set_dirty(RACK, true);
    sim.set_clean(BED, true);
    sim.force_tile(4, 2, BED); // лазарет по соседству

    assert!(
        !sim.add_blueprint(4, 1, RACK as i32),
        "грязь к чистоте нельзя"
    );
    assert!(sim.add_blueprint(2, 1, RACK as i32), "в стороне — можно");
    assert!(
        !sim.add_blueprint(2, 2, RACK as i32) || sim.planned_tile(2, 2).is_some(),
        "а правило доступа считается по-прежнему",
    );
}

// --- боевой рулсет ----------------------------------------------------------

/// У заставленного тайла обязан быть смысл сверх того, что на нём не стоят.
///
/// Смыслов ровно два, и оба проверяемы. **Хранилище** (`capacity`): стоять
/// нельзя, но заходить есть зачем — это стеллаж, ради которого §12.142 и
/// заведена. **Внутренность объекта** (`internal`, §12.163): сама по себе такая
/// клетка не постройка, в палитру не идёт и ставится только штампом — это
/// приборный стол, за которым работают с соседней клетки.
///
/// `solid` без обоих — комната, в которую незачем заходить и которую при этом
/// предлагают построить.
#[test]
fn the_shipped_ruleset_gives_every_solid_tile_a_reason() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let solids = sim.solid_tiles();

    assert!(!solids.is_empty(), "хоть один заставленный тайл в палитре");
    for t in solids {
        assert!(
            sim.capacity_of(t) > 0 || sim.is_internal(t),
            "заставленный тайл {t} не хранилище и не внутренность объекта",
        );
    }
}

/// Стартовая застройка правилу доступа не противоречит. Ловит контент, в
/// котором полки положены монолитом: игрок видел бы на старте базу, которую
/// сам построить не может.
#[test]
fn the_shipped_ruleset_starts_with_every_shelf_reachable() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    assert!(
        sim.solid_without_aisle().is_empty(),
        "к каждой заставленной клетке стартовой базы можно подойти",
    );
}

/// Стартовая застройка зонированию не противоречит (§12.157). Тот же сторож,
/// что у правила доступа, и тот же довод: игрок видел бы на старте базу,
/// которую сам построить не может, — а починить её пришлось бы сносом.
#[test]
fn the_shipped_ruleset_starts_without_zoning_clashes() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let clashes = sim.zoning_clashes();
    assert!(
        clashes.is_empty(),
        "стартовая база нарушает зонирование: {clashes:?}",
    );
}

/// Правило не выключено молча: у каждой пары есть обе стороны. Односторонняя
/// пара (одни шумные, ни одного, кому нужна тишина) — это механика, которая не
/// срабатывает никогда, то есть мёртвый контент; довод тот же, что у сторожа
/// непустого контейнера у поста (§12.90).
#[test]
fn the_shipped_ruleset_uses_both_sides_of_every_zoning_pair() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let any = |f: fn(&TileRules, i16) -> bool| {
        let rules = sim.world.resource::<TileRules>();
        (0..rules.0.len()).any(|i| f(rules, i as i16))
    };

    assert!(any(TileRules::is_quiet), "кому-то нужна тишина");
    assert!(any(TileRules::is_noisy), "и кто-то шумит");
    assert!(any(TileRules::is_clean), "кому-то нужна чистота");
    assert!(any(TileRules::is_dirty), "и кто-то грязнит");
}

/// Стартовая база **связна при непроходимых полках** (§12.142). Сторож на
/// контент: до §12.142 сквозь стеллаж ходили, и коридор, выложенный полками,
/// ничего не резал. Теперь режет — и увидеть это можно было бы только в игре.
///
/// Меряется всё, ради чего по базе ходят: склад, лежанка, парта, лаборатория,
/// станок, торговый пост, рация и шлюз. Полка при этом считается достигнутой,
/// если к ней есть подход, — ровно как её видит носильщик.
#[test]
fn the_shipped_ruleset_starts_with_every_room_reachable() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let lost = sim.unreachable_cells(|rules, t| {
        t >= 0
            && (rules.capacity_of(t) > 0
                || rules.rest_of(t) > 0
                || rules.heal_of(t) > 0
                || rules.is_gate(t)
                || rules.is_lab(t)
                || rules.is_shop(t)
                || rules.is_trade_post(t)
                || rules.is_relay_node(t)
                || rules.teaches_of(t).is_some())
    });

    assert!(
        lost.is_empty(),
        "стартовая база разрезана полками: не дойти до {lost:?}",
    );
}

// --- хелперы ----------------------------------------------------------------

/// За сколько тиков кот пройдёт клетку, на которой лежит `count` предметов.
fn walk_ticks(count: i32) -> usize {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_auto_tidy(false); // иначе кот пойдёт не туда, куда послали, а за кучей
    if count > 0 {
        sim.put_item(2, 1, 0, count);
    }
    assert!(sim.set_target("a", 3, 1), "приказ принят");
    ticks_to_arrive(&mut sim, (3, 1)).expect("кот должен дойти")
}

/// Сколько тиков заняло прибытие в клетку; `None` — не дошёл за 200 тиков.
fn ticks_to_arrive(sim: &mut Sim, at: (i32, i32)) -> Option<usize> {
    for tick in 1..=200 {
        sim.tick_n(1);
        if sim.pos_of("a") == at {
            return Some(tick);
        }
    }
    None
}
