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

// --- на стеллаже не стоят ---------------------------------------------------

/// Свободный кот со стеллажа сходит сам: мебель — не комната.
#[test]
fn a_cat_does_not_linger_on_a_rack() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK); // стеллаж прямо под котом

    sim.tick_n(5);
    assert_ne!(sim.pos_of("a"), (1, 1), "сошёл со стеллажа");
    assert_eq!(sim.pos_of("a"), (2, 1), "на соседнюю клетку");
}

/// Пройти сквозь можно — правило про остановку, а не про проходимость. Иначе
/// стеллаж резал бы маршруты и отрезал бы от базы всё, что за ним.
#[test]
fn a_rack_is_still_walkable() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(2, 1, RACK); // стеллаж посреди коридора

    assert!(sim.set_target("a", 3, 1), "приказ принят");
    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (3, 1), "кот прошёл сквозь стеллаж");
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

/// Единственный выход занят другим котом — и всё равно кот сходит: наложение
/// разберётся само (§12.32), а стеллаж сам себя не разберёт (§12.39).
#[test]
fn a_cat_steps_off_a_rack_even_when_the_way_out_is_taken() {
    let mut sim = sim_from(&["#####", "#ab.#", "#####"]);
    sim.set_solid(RACK, true);
    sim.force_tile(1, 1, RACK); // стеллаж под котом, за спиной — тупик

    sim.tick_n(20);
    assert_ne!(sim.pos_of("a"), (1, 1), "сошёл, хоть выход и был занят");
    assert_ne!(sim.pos_of("b"), (1, 1), "и соседа на стеллаж не загнал");
    assert_ne!(sim.pos_of("a"), sim.pos_of("b"), "а потом разошлись");
}

/// Развод двоих из одной клетки на стеллаж не загоняет: иначе первый сгонял бы
/// второго на полки, `clear_solids` возвращал бы его обратно, и так вечно.
/// Стоять вдвоём — легальное состояние, стоять на полках — нет.
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

/// Кот, пришедший **за содержимым**, стоит сколько нужно: носильщик сдаёт груз
/// именно здесь, и согнать его значило бы сломать доставку на стеллаж.
#[test]
fn a_hauler_may_stand_on_a_rack_while_it_works() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    sim.set_solid(RACK, true);
    sim.set_capacity(RACK, 10);
    sim.force_tile(4, 1, RACK); // стеллаж в конце коридора
    sim.put_item(1, 1, 0, 5); // и лом, который туда свезут

    let mut stood_there = false;
    for _ in 0..40 {
        sim.tick_n(1);
        stood_there |= sim.pos_of("a") == (4, 1);
    }
    assert!(stood_there, "носильщик дошёл до стеллажа");
    assert_eq!(sim.item_at(4, 1, 0), 5, "и сдал груз");
    assert_ne!(sim.pos_of("a"), (4, 1), "а закончив, сошёл");
}

/// Спящего стеллаж тоже сгоняет — ровно та картинка, ради которой правило и
/// заводилось. Досыпает кот рядом (§12.33).
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
        sim.pos_of("a"),
        (2, 1),
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
    assert_eq!(planned, 8, "восемь полок из девяти, девятая — проход");

    sim.tick_n(600); // коту хватит достроить всё размеченное
    assert!(
        sim.solid_without_aisle().is_empty(),
        "и построенное правилу не противоречит",
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
    assert_eq!(open, 8, "одна клетка мазка уйдёт под проход");

    sim.add_blueprint_rect(2, 1, 3, 3, RACK as i32);
    let planned = (1..4)
        .flat_map(|y| (2..5).map(move |x| (x, y)))
        .filter(|&(x, y)| sim.planned_tile(x, y) == Some(RACK) && mask[(y * 7 + x) as usize] == 1)
        .count();
    assert_eq!(
        planned, 8,
        "и это ровно та клетка, которую отклонит разметка"
    );
}

// --- боевой рулсет ----------------------------------------------------------

/// В `core.yaml` заставленный тайл есть, и это склад: свойство `solid` без
/// `capacity` значило бы комнату, в которую незачем заходить.
#[test]
fn the_shipped_ruleset_has_a_solid_storage_tile() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let solids = sim.solid_tiles();

    assert!(!solids.is_empty(), "хоть один заставленный тайл в палитре");
    assert!(
        solids.iter().all(|&t| sim.capacity_of(t) > 0),
        "и все они — хранилища: стоять нельзя, но заходить есть зачем",
    );
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
