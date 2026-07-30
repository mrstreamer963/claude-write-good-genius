//! Раздача джобов и рамочные жесты игрока.

use super::{BUILD_TICKS, sim_from};

/// Ластик по рамке решает за всю рамку сразу: сперва снимает чертежи, и
/// только если снимать было нечего — планирует снос. Потайловый
/// переключатель на смешанной области дал бы кашу: половину клеток снял бы,
/// половину поставил, а что именно — зависело бы от прошлых действий игрока.
#[test]
fn erase_rect_cancels_plans_before_planning_demolition() {
    let mut sim = sim_from(&["######", "#a...#", "#..###", "######"]);
    // Чертёж постройки внутри будущей рамки: (3, 2) — пустая клетка.
    assert!(sim.add_blueprint(3, 2, 0));

    assert!(
        sim.plan_demolish_rect(2, 1, 3, 2),
        "первый жест снимает план"
    );
    sim.tick_n(200);
    assert_eq!(sim.tile(3, 2), -1, "снятый чертёж не построился");
    assert_eq!(
        sim.floors_left([2, 1, 3, 2]),
        4,
        "снос при этом не планировался — пол цел"
    );

    assert!(
        sim.plan_demolish_rect(2, 1, 3, 2),
        "снимать нечего — планируем снос"
    );
    sim.tick_n(400);
    assert_eq!(sim.floors_left([2, 1, 3, 2]), 0, "рамка снесена целиком");
    assert!(!sim.stuck_of("a"), "кот остался на полу за рамкой");
}

/// Рамка постройки ставит чертежи на все пустые клетки под ней.
#[test]
fn build_rect_plans_every_empty_cell() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    assert!(sim.add_blueprint_rect(1, 0, 4, 1, 0));
    sim.tick_n(400);
    assert_eq!(sim.floors_left([1, 0, 4, 1]), 4, "вся рамка построена");
}

/// Кот берёт ближайший чертёж, а не первый по счёту: закончив клетку, он
/// не должен уходить через полкарты мимо соседней (§12.14).
#[test]
fn cat_takes_the_nearest_blueprint_first() {
    let mut sim = sim_from(&["#########", "#a......#", "#########"]);
    sim.add_blueprint(7, 2, 0); // дальний конец коридора — размечен первым
    sim.add_blueprint(2, 2, 0); // в шаге от кота

    sim.tick_n(BUILD_TICKS + 2);
    assert_eq!(sim.tile(2, 2), 0, "ближний чертёж построен первым");
    assert_eq!(sim.tile(7, 2), -1, "дальний ждёт очереди");

    sim.tick_n(BUILD_TICKS + 8);
    assert_eq!(sim.tile(7, 2), 0, "дальний построен следом");
}

/// Двое котов не должны драться за один чертёж: пары (кот, чертёж)
/// разбираются от самой дешёвой, а не в порядке самих чертежей.
#[test]
fn each_cat_takes_the_blueprint_next_to_it() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.add_blueprint(7, 2, 0); // у ног 'b'
    sim.add_blueprint(1, 2, 0); // у ног 'a'

    sim.tick_n(BUILD_TICKS + 2);
    assert_eq!(sim.tile(1, 2), 0, "'a' построил свою клетку");
    assert_eq!(sim.tile(7, 2), 0, "'b' построил свою — параллельно");
}
