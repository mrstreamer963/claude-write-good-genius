//! Личное дело кота (§12.N): что мир о нём помнит.
//!
//! До этой записи не помнил ничего: три журнала (`Raids`, `Crafted`, `Earned`)
//! считают базу целиком, а на самом коте не было ни одного поля с тиком.
//!
//! Форма журнальная — только растёт, и каждое поле пишется **ровно в одном
//! месте**, там, где событие происходит. Поэтому и тесты здесь по одному на
//! точку записи: промах в любой из них тихий, дело просто останется пустым, а
//! пустое дело от настоящего не отличить.

use super::*;

/// Мир со шлюзом (тайл 1) и котами по коридору — дословно `captivity.rs`:
/// вылазка единственный источник трёх полей из пяти.
fn gate_world(rows: &[&str], gate: (i32, i32)) -> Sim {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    sim
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

// --- наём -------------------------------------------------------------------

/// Схемный кот заведён нулевым тиком, а нанятый помнит свой. Пишет это
/// `spawn_cat` — одно место на всех котов (инвариант 21).
#[test]
fn a_cat_remembers_the_tick_it_joined() {
    let mut sim = gate_world(&["#########", "#a......#", "#########"], (4, 1));
    assert_eq!(sim.joined_of("a"), 0, "схемный кот с нуля");

    let nail = sim.set_recruit("nail", 0, &[], &[]);
    sim.tick_n(120);
    assert!(sim.hire(nail), "нанялся");

    assert_eq!(sim.joined_of("nail"), 120, "и запомнил, когда именно");
}

// --- вылазки ----------------------------------------------------------------

/// Вернувшийся отряд засчитывает вылазку каждому — и **по заказу**, а не
/// счётчиком: «этот кот ходит на свалки» и есть та специализация, ради которой
/// дело заводилось.
#[test]
fn a_returning_squad_counts_its_raid_by_order() {
    let mut sim = gate_world(&["#########", "#a..b...#", "#########"], (4, 1));
    let junk = sim.set_mission(2, 5, &[]);
    sim.launch(junk, squad(&["a", "b"]));
    sim.tick_n(40);

    assert_eq!(sim.raids_of("a"), vec![(junk, 1)]);
    assert_eq!(sim.raids_of("b"), vec![(junk, 1)], "обоим");
}

/// Ходка засчитывается и провальная: «сходил» — это про кота, а не про добычу.
/// Журнал базы (`Raids`, §12.58) провал не берёт, и это разные вопросы: там
/// «взят ли заказ», здесь «где кот был».
#[test]
fn a_failed_raid_counts_as_a_completed_one() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    sim.set_rescue_mission(1, 5, 0); // обратимость плена
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(40);

    assert_eq!(sim.raids_of("a"), vec![(doomed, 1)]);
}

/// Разные заказы копятся раздельно и **в порядке заказа**: порядок обхода ECS
/// недетерминирован, а список едет и в снимок, и в сохранение.
#[test]
fn raids_pile_up_per_order_in_a_stable_order() {
    let mut sim = gate_world(&["#########", "#a......#", "#########"], (4, 1));
    let first = sim.set_mission(1, 5, &[]);
    let second = sim.set_mission(1, 5, &[]);
    for m in [second, first, second] {
        sim.launch(m, squad(&["a"]));
        sim.tick_n(40);
    }

    assert_eq!(sim.raids_of("a"), vec![(first, 1), (second, 2)]);
}

// --- раны -------------------------------------------------------------------

/// Считается **переход через порог**, а не урон: дело помнит выход из строя.
#[test]
fn a_wound_that_crosses_the_threshold_is_counted_once() {
    let mut sim = gate_world(&["#########", "#a......#", "#########"], (4, 1));
    sim.set_health_rules(100, 60, 0);
    let raid = sim.set_risky_mission(1, 5, 100, 0, &[]);
    sim.set_mission_harm(raid, 100);
    sim.launch(raid, squad(&["a"]));
    sim.tick_n(40);

    assert!(sim.health_of("a") <= 60, "провал довёл до порога");
    assert_eq!(sim.wounds_of("a"), 1);
}

/// А царапина — нет: кот продолжил работать, и рассказывать тут не о чем.
#[test]
fn a_scratch_that_stays_above_the_threshold_is_not_counted() {
    let mut sim = gate_world(&["#########", "#a......#", "#########"], (4, 1));
    sim.set_health_rules(100, 60, 0);
    // Заведомо успешная вылазка с уроном: доля полная, значит `harm` не
    // списывается вовсе, — а вот полусилы хватило бы на царапину.
    let raid = sim.set_risky_mission(1, 5, 1, 0, &[]);
    sim.set_mission_harm(raid, 20);
    sim.launch(raid, squad(&["a"]));
    sim.tick_n(40);

    assert!(sim.health_of("a") > 60, "остался в строю");
    assert_eq!(sim.wounds_of("a"), 0);
}

// --- плен -------------------------------------------------------------------

/// Плен засчитывается **только тому, кто остался**: остальные вернулись домой,
/// и в их деле этой строки нет.
#[test]
fn captivity_is_counted_only_on_the_cat_who_stays() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    sim.set_rescue_mission(1, 5, 0);
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(40);

    let (left, home) = if sim.is_captive("a") {
        ("a", "b")
    } else {
        ("b", "a")
    };
    assert_eq!(sim.captures_of(left), 1, "остался в плену");
    assert_eq!(sim.captures_of(home), 0, "а этот вернулся");
}

// --- учёба ------------------------------------------------------------------

/// Мир из `study.rs`: коридор с партой в (3,1).
fn sim_with_desk() -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &[20, 100]);
    sim.set_taught(science, 1);
    sim.set_teaches(1, science);
    sim.force_tile(3, 1, 1);
    (sim, science)
}

/// Досидел за партой до потолка — домен отмечен. Первая из двух веток.
#[test]
fn a_desk_marks_the_domain_it_finished() {
    let (mut sim, science) = sim_with_desk();
    assert!(sim.teach("a", "science"));
    assert!(sim.schooled_of("a").is_empty(), "пока учится — нечего");

    sim.tick_n(60);
    assert!(!sim.is_enrolled("a"), "приписка исчерпана");
    assert_eq!(sim.schooled_of("a"), vec![science]);
}

/// Вторая ветка, и забыть её легко: кота увели от парты **на последнем очке**,
/// `Study` с него снят, а отпускает приписку уже `assign_study`. Без отметки
/// там дело молча теряет ровно тех, кто доучился в последний тик.
#[test]
fn a_cat_pulled_from_the_desk_at_the_ceiling_is_still_marked() {
    let (mut sim, science) = sim_with_desk();
    assert!(sim.teach("a", "science"));
    sim.tick_n(4); // дошёл и сел
    // Потолок выдаём руками и тем же тиком уводим кота приказом: `Study` снят,
    // `Enrolled` остался — ровно то состояние, которое разбирает `assign_study`.
    sim.set_xp("a", science, 20);
    sim.set_target("a", 5, 1);
    assert!(!sim.is_studying("a"), "от парты увели");
    assert!(sim.is_enrolled("a"), "а приписка при нём");

    // Пока кот в пути, `assign_study` его не видит (`Without<Path>`): приписку
    // отпускает первый же тик, когда он снова свободен.
    sim.tick_n(30);
    assert!(!sim.is_enrolled("a"), "и она исчерпана");
    assert_eq!(
        sim.schooled_of("a"),
        vec![science],
        "отметка всё равно есть"
    );
}

/// Отметка идемпотентна: обе ветки зовут одно выражение, и вторая запись о том
/// же домене — не событие.
#[test]
fn a_domain_is_marked_once_however_long_he_sits() {
    let (mut sim, science) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(60);
    assert_eq!(sim.schooled_of("a"), vec![science]);

    sim.tick_n(200); // стоит рядом с партой и ничего не делает
    assert_eq!(sim.schooled_of("a"), vec![science], "по-прежнему одна");
}
