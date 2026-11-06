//! Одиночный выход наверх через шлюз (§12.349): приказ коту на клетку шлюза,
//! выход, возвращение с добычей, истощение окрестностей до пола — и отряд,
//! который ждёт своего кота, вышедшего наверх, а не уходит без него.

use super::*;

const TABLE: i16 = 2;
const EXIT: i16 = 3;
/// Шлюз — у дальнего от котов края комнаты.
const GATE: (i32, i32) = (1, 4);
/// Тиков наверху в этих тестах.
const UP: i32 = 20;

const RING8: [(i32, i32); 8] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (0, 1),
    (2, 1),
    (0, 2),
    (1, 2),
    (2, 2),
];

/// Комната со столом брифинга (якорь `(2, 1)`) и одним шлюзом. Окрестности:
/// первый выход — 3 лома, дальше по одному.
fn yard() -> Sim {
    let mut sim = sim_from(&[
        "###########",
        "#.........#",
        "#.........#",
        "#.........#",
        "#......a.b#",
        "###########",
    ]);
    sim.set_muster(TABLE, true);
    sim.set_solid(TABLE, true);
    sim.set_exit(EXIT, true);
    let def = sim.set_structure(vec![
        vec![None, None, None],
        vec![None, Some(TABLE), None],
        vec![None, None, None],
    ]);
    sim.set_structure_slots(def, &RING8);
    sim.force_structure(def, 2, 1);
    sim.force_tile(GATE.0, GATE.1, EXIT);
    sim.set_outing(UP, 0, &[&[(0, 3)]], &[(0, 1)]);
    sim
}

/// Дождаться, пока кот выйдет наверх; `false` — так и не вышел.
fn goes_up(sim: &mut Sim, unit: &str, ticks: usize) -> bool {
    (0..ticks).any(|_| {
        sim.tick_n(1);
        sim.is_outside(unit)
    })
}

/// Дождаться возвращения.
fn comes_back(sim: &mut Sim, unit: &str, ticks: usize) -> bool {
    (0..ticks).any(|_| {
        sim.tick_n(1);
        !sim.is_outside(unit)
    })
}

/// Приказ на шлюз — это выход наверх: кот доходит, пропадает с базы на
/// `ticks` тиков и возвращается с добычей кучей на шлюзе.
#[test]
fn a_cat_sent_to_the_gate_goes_up_and_brings_loot() {
    let mut sim = yard();
    assert_eq!(sim.hand_tag("a", GATE.0, GATE.1), "outing");
    assert!(sim.command_at("a", GATE.0, GATE.1));

    assert!(goes_up(&mut sim, "a", 100), "кот так и не вышел");
    assert!(sim.is_away("a"), "наверху — значит не на базе");
    assert!(
        comes_back(&mut sim, "a", UP as usize + 5),
        "кот не вернулся"
    );
    assert!(!sim.is_away("a"));
    assert_eq!(sim.scrap_at(GATE.0, GATE.1), 3, "добыча легла на шлюз");
    assert_eq!(sim.outings_done(), 1);
}

/// Окрестности истощаются, но не до нуля: список по порядку, за ним — пол
/// каждый раз.
#[test]
fn the_surroundings_run_down_to_the_floor() {
    let mut sim = yard();
    let mut got = Vec::new();
    for _ in 0..3 {
        let before = sim.scrap_total();
        assert!(sim.command_at("a", GATE.0, GATE.1));
        assert!(goes_up(&mut sim, "a", 100), "кот так и не вышел");
        assert!(comes_back(&mut sim, "a", UP as usize + 5));
        got.push(sim.scrap_total() - before);
    }
    assert_eq!(got, vec![3, 1, 1], "сперва запас, потом пол");
}

/// Наверх выходят по одному на всю базу: второй приказ отказан словом.
#[test]
fn only_one_cat_goes_up_at_a_time() {
    let mut sim = yard();
    assert!(sim.command_at("a", GATE.0, GATE.1));
    assert_eq!(
        sim.hand_tag("b", GATE.0, GATE.1),
        "outside",
        "пока первый идёт"
    );
    assert!(!sim.command_at("b", GATE.0, GATE.1));

    assert!(goes_up(&mut sim, "a", 100));
    assert_eq!(
        sim.hand_tag("b", GATE.0, GATE.1),
        "outside",
        "и пока он наверху"
    );
}

/// Новый приказ гасит намерение: кот, которого передумали посылать, наверх
/// не выходит, даже пройдя по шлюзу.
#[test]
fn a_new_order_cancels_the_outing() {
    let mut sim = yard();
    assert!(sim.command_at("a", GATE.0, GATE.1));
    sim.tick_n(1);
    assert!(sim.command_at("a", 5, 1));
    assert!(
        !goes_up(&mut sim, "a", 100),
        "передумали — а кот всё равно вышел"
    );
}

/// **Отряд ждёт своего кота, вышедшего наверх**, а не уходит без него: брифинг
/// не начнётся, пока тот не вернётся и не сядет за стол.
#[test]
fn a_squad_waits_for_its_cat_upstairs() {
    let mut sim = yard();
    let node = sim.node_cells_of()[0];
    assert!(sim.enlist("a", node.0, node.1));
    assert!(sim.enlist("b", node.0, node.1));
    let def = sim.set_mission(2, 30, &[]);

    assert!(sim.command_at("a", GATE.0, GATE.1));
    assert!(goes_up(&mut sim, "a", 100));
    assert!(sim.launch_node(def, node.0, node.1, -1), "заявка принята");

    while sim.is_outside("a") {
        assert!(!sim.is_away("b"), "отряд ушёл без кота, вышедшего наверх");
        sim.tick_n(1);
    }
    let left = (0..400).any(|_| {
        sim.tick_n(1);
        sim.is_away("a") && sim.is_away("b")
    });
    assert!(left, "вернувшийся кот сел, и отряд ушёл вместе с ним");
}

/// Кот собирающегося отряда наверх не выходит: он уже взят на вылазку.
#[test]
fn a_gathering_cat_does_not_go_up() {
    let mut sim = yard();
    let node = sim.node_cells_of()[0];
    assert!(sim.enlist("a", node.0, node.1));
    let def = sim.set_mission(1, 30, &[]);
    assert!(sim.launch_node(def, node.0, node.1, -1));

    assert_eq!(sim.hand_tag("a", GATE.0, GATE.1), "squad");
    assert!(!sim.command_at("a", GATE.0, GATE.1));
}

/// Без выхода в рулсете шлюз отвечает на приказ обычным «иди туда».
#[test]
fn without_outings_a_gate_is_just_a_place() {
    let mut sim = yard();
    sim.set_outing(0, 0, &[], &[]);
    assert_eq!(sim.hand_tag("a", GATE.0, GATE.1), "");
    assert!(sim.command_at("a", GATE.0, GATE.1));
    assert!(!goes_up(&mut sim, "a", 100));
    assert_eq!(sim.pos_of("a"), GATE, "дошёл и стоит");
}

/// Бодрость выход стоит разом, на возвращении, как у вылазки.
#[test]
fn an_outing_costs_energy_on_return() {
    let mut sim = yard();
    sim.set_outing(UP, 500, &[&[(0, 3)]], &[(0, 1)]);
    sim.set_energy("a", 10_000);
    assert!(sim.command_at("a", GATE.0, GATE.1));
    assert!(goes_up(&mut sim, "a", 100));
    let up = sim.energy_of("a");
    assert!(comes_back(&mut sim, "a", UP as usize + 5));
    // Плата разом плюс обычный расход тика возвращения: в этот тик кот уже
    // на базе (в схеме расход — очко за тик).
    let paid = up - sim.energy_of("a");
    assert_eq!(paid, 501, "наверху не устают, платят разом: {paid}");
}

/// **Боевой рулсет: пол окрестностей — деталь** (§12.349). Шлюз и стол
/// брифинга стоят по детали, и пол — дорога назад для базы, истратившей все.
#[test]
fn the_shipped_ruleset_always_finds_a_part_upstairs() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let part = sim.item_index("part").expect("деталь в палитре");
    let rules = sim.world.resource::<OutingRules>();
    assert!(rules.ticks > 0, "выход наверх в рулсете есть");
    assert!(
        rules.floor.iter().any(|&(i, n)| i == part && n > 0),
        "пол окрестностей без детали: базе, истратившей все, не вернуться",
    );
}
