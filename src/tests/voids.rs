//! Снос пола под котом: выход из ямы и честное «замурован».

use super::sim_from;

#[test]
fn cat_steps_out_of_demolished_tile() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    assert!(sim.demolish(1, 1));
    sim.tick_n(4);
    assert_eq!(sim.pos_of("a"), (2, 1), "кот должен уйти на соседний пол");
    assert!(!sim.stuck_of("a"));
}

#[test]
fn entombed_cat_stays_and_is_flagged() {
    // Одиночная клетка пола: снести её — и шагнуть некуда.
    let mut sim = sim_from(&["###", "#a#", "###"]);
    assert!(sim.demolish(1, 1));
    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (1, 1), "выхода нет — кот остаётся");
    assert!(
        sim.stuck_of("a"),
        "замурованный кот должен помечаться stuck"
    );
}

#[test]
fn entombed_cat_recovers_when_floor_returns() {
    let mut sim = sim_from(&["###", "#a#", "###"]);
    sim.demolish(1, 1);
    sim.tick_n(5);
    assert!(sim.stuck_of("a"));

    sim.force_tile(1, 0, 0); // игрок вернул пол рядом
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (1, 0), "кот должен выбраться на новый пол");
    assert!(!sim.stuck_of("a"));
}
