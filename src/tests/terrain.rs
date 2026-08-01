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
