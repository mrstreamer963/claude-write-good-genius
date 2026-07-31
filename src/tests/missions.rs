//! Миссии: сбор отряда, уход через шлюз, возвращение с добычей (§12.22).
//!
//! Схема почти везде одна: коридор с шлюзом (`G`-тайл задаётся через
//! `set_gate`) и коты по разные его стороны. Проверяем не отдельные функции,
//! а прогон полной цепочки — баги здесь живут в фильтрах занятости и в порядке
//! раздатчиков.

use super::*;

/// Мир с одной клеткой-шлюзом: тайл 1 в позиции `gate`, всё остальное — пол.
/// Возвращает готовую симуляцию и индекс заведённой миссии.
fn sim_with_gate(rows: &[&str], gate: (i32, i32), squad: usize, ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    let mission = sim.set_mission(squad, ticks, &[(0, 5)]);
    (sim, mission)
}

#[test]
fn a_squad_gathers_at_the_gate_and_leaves() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    assert!(sim.launch(m));

    // Сбор: оба идут к шлюзу и по дороге ещё на базе.
    sim.tick_n(1);
    assert!(
        sim.in_squad("a") && sim.in_squad("b"),
        "оба записаны в отряд"
    );
    assert!(!sim.is_away("a"), "пока идут — ещё на базе");

    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (3, 1), "кот пришёл на шлюз");
    assert!(sim.is_away("a") && sim.is_away("b"), "отряд ушёл");
    assert_eq!(sim.mission_gate(), Some((3, 1)));
}

#[test]
fn the_squad_returns_with_loot_at_the_gate() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.launch(m);
    sim.tick_n(11);
    assert!(sim.is_away("a"), "отряд в поле");
    assert_eq!(sim.scrap_total(), 0, "пока отряд в поле, добычи ещё нет");

    sim.tick_n(10);
    assert!(!sim.is_away("a") && !sim.is_away("b"), "отряд вернулся");
    assert!(!sim.in_squad("a"), "и снова свободен");
    assert_eq!(sim.pos_of("a"), (3, 1), "вернулись на тот же шлюз");
    assert_eq!(sim.item_at(3, 1, 0), 5, "добыча лежит кучей на шлюзе");
    assert_eq!(sim.mission_left(), None, "миссия закрыта");
}

/// Отсчёт идёт с ухода отряда, а не с приказа: пока последний кот в пути,
/// таймер стоит. Иначе миссия «шла» бы, пока бригада бегает по базе.
#[test]
fn the_timer_waits_for_the_last_cat() {
    let rows = &[
        "##########",
        "#a.......#",
        "#........#",
        "#b.......#",
        "##########",
    ];
    let (mut sim, m) = sim_with_gate(rows, (8, 1), 2, 20);
    sim.launch(m);

    sim.tick_n(8);
    assert!(sim.in_squad("a") && sim.in_squad("b"));
    assert_eq!(sim.mission_left(), Some(20), "таймер ещё не тронулся");
    assert!(!sim.is_away("a"), "первый пришёл, но ждёт второго");

    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"), "ушли вместе");
    assert!(sim.mission_left().is_some_and(|l| l < 20), "таймер пошёл");
}

/// Занятость отрядом — это фильтры: пропущенный `Without<Squad>` тихо уводит
/// бойца на стройку, и отряд не соберётся никогда (инвариант занятости).
#[test]
fn a_cat_in_a_squad_is_not_taken_by_jobs() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 50);
    sim.launch(m);
    sim.add_blueprint(1, 1, 0);
    sim.add_blueprint(5, 1, 0);
    sim.tick_n(20);

    assert!(
        sim.is_away("a") && sim.is_away("b"),
        "чертежи отряд не сорвали"
    );
    assert!(!sim.has_assignment("a") && !sim.has_assignment("b"));
}

/// Приказ игрока снимает любую задачу — и место в отряде тоже. Раздатчик
/// добирает замену, а сам отряд не разваливается.
#[test]
fn an_order_pulls_a_cat_out_of_the_squad() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    sim.launch(m);
    sim.tick_n(1);
    // Ближайший к шлюзу — 'c'; его и уводим приказом.
    assert!(sim.in_squad("c"), "ближайший записан первым");
    assert!(sim.set_target("c", 1, 1));
    assert!(!sim.in_squad("c"), "приказ вывел из отряда");

    sim.tick_n(20);
    assert!(!sim.is_away("c"), "'c' ушёл по своим делам");
    let gone = ["a", "b"].iter().filter(|u| sim.is_away(u)).count();
    assert_eq!(gone, 2, "замена нашлась, и отряд всё равно ушёл вдвоём");
}

/// Добыча ложится на шлюз обычной кучей и дальше живёт по общим правилам:
/// её размечает автоуборка и увозит на склад свободный кот (§12.16).
#[test]
fn loot_reaches_storage_by_itself() {
    let (mut sim, m) = sim_with_gate(&["########", "#a..b..#", "########"], (3, 1), 2, 6);
    sim.set_capacity(2, 50);
    sim.force_tile(6, 1, 2); // склад в дальнем конце коридора
    sim.launch(m);
    sim.tick_n(40);

    assert_eq!(sim.scrap_total(), 5, "добыча не потерялась");
    assert!(sim.scrap_is_in_storage(), "и уехала на склад сама");
}

/// Шлюз снесли, пока отряд был в поле. Кот выбирается из ямы общим механизмом
/// (`escape_voids`), добыча съезжает на соседний пол (`settle_stacks`): правило
/// «ничего не остаётся в пустоте» на миссии не делает исключения (§12.15).
#[test]
fn a_squad_returns_into_a_demolished_gate() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.launch(m);
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    sim.demolish(3, 1); // шлюза больше нет
    sim.tick_n(12);

    assert!(!sim.is_away("a"), "отряд вернулся");
    assert!(sim.tile(3, 1) < 0, "вернулся в яму");
    assert_ne!(sim.pos_of("a"), (3, 1), "и вышел из неё сам");
    assert_eq!(sim.scrap_total(), 5, "добыча цела");
    assert!(sim.scrap_is_on_floor(), "и лежит на полу, а не в яме");
}

#[test]
fn cancelling_frees_the_squad() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 50);
    sim.launch(m);
    sim.tick_n(1);
    assert!(sim.in_squad("a"));

    assert!(sim.cancel_mission());
    assert!(!sim.in_squad("a") && !sim.in_squad("b"), "отряд распущен");
    assert_eq!(sim.mission_left(), None);

    sim.add_blueprint(1, 1, 0);
    sim.tick_n(BUILD_TICKS + 5);
    assert_eq!(sim.tile(1, 1), 0, "и коты вернулись к работе");
}

/// Ушедший отряд не отзывается: что с ним происходит, симуляция не знает —
/// вылазка считается разом по возвращении.
#[test]
fn an_away_squad_cannot_be_recalled() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 30);
    sim.launch(m);
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    assert!(!sim.cancel_mission(), "отозвать нельзя");
    assert!(!sim.set_target("a", 1, 1), "и приказать тоже");
    assert!(sim.is_away("a"), "кот всё ещё в поле");
}

/// Истощение забирает кота откуда угодно — в том числе из отряда: он выпадает
/// из состава и ложится спать, а раздатчик доберёт замену (§12.20).
#[test]
fn exhaustion_pulls_a_cat_out_of_the_squad() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    sim.set_needs(100, 10, 1);
    sim.launch(m);
    sim.tick_n(1);
    assert!(sim.in_squad("c"));

    sim.set_energy("c", 0);
    sim.tick_n(1);
    assert!(!sim.in_squad("c"), "истощённый выпал из отряда");
    assert!(sim.is_resting("c"), "и спит там, где стоял");

    sim.tick_n(20);
    let gone = ["a", "b"].iter().filter(|u| sim.is_away(u)).count();
    assert_eq!(gone, 2, "замена нашлась, отряд ушёл");
}

/// Вне базы кот не устаёт: считать усталость там нечем, миссия — авторасчёт,
/// а не симуляция (§12.22).
#[test]
fn a_cat_on_a_mission_does_not_tire() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 40);
    sim.set_needs(3000, 100, 1);
    sim.launch(m);
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    let before = sim.energy_of("a");
    sim.tick_n(20);
    assert!(sim.is_away("a"), "ещё в поле");
    assert_eq!(sim.energy_of("a"), before, "бодрость не тронулась");
}

/// Шлюза на базе нет — отряд не набирается вовсе, и коты не стоят столбом:
/// миссия просто ждёт, пока игрок построит гараж.
#[test]
fn without_a_gate_nobody_leaves() {
    let mut sim = sim_from(&["#######", "#a...b#", "#######"]);
    let m = sim.set_mission(2, 10, &[(0, 5)]);
    sim.launch(m);
    sim.add_blueprint(3, 1, 0);
    sim.tick_n(20);

    assert!(!sim.in_squad("a") && !sim.in_squad("b"), "набирать некуда");
    assert_eq!(sim.mission_gate(), None);
}

/// Миссия одна за раз: вторая заявка не принимается, пока первая не закрыта.
#[test]
fn only_one_mission_runs_at_a_time() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    assert!(sim.launch(m));
    assert!(!sim.launch(m), "вторая заявка отклонена");
    sim.tick_n(21);
    assert!(sim.launch(m), "после возвращения — снова можно");
}

/// Боевой рулсет: гараж — шлюз, отряд собирается, уходит и возвращается с
/// деталями. Ловит рассогласование кода и контента — гараж без `gate`, миссию
/// с отрядом больше, чем котов на базе, или добычу под чужим `id` предмета.
#[test]
fn the_shipped_ruleset_sends_a_squad_out_and_back() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let parts = 1; // индекс `part` в палитре предметов
    let before = sim.item_total(parts);

    assert!(sim.launch(0), "первая миссия рулсета запускается");
    // Сбор + вылазка + запас на дорогу: длительность берём с потолком.
    sim.tick_n(600);

    assert!(
        sim.item_total(parts) > before,
        "деталей стало больше: вылазка — источник дохода, которого у базы не было",
    );
    assert_eq!(sim.mission_left(), None, "миссия закрыта");
}
