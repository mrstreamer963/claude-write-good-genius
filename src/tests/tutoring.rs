//! Парта на три места: ученик, стол, учитель (§12.258).
//!
//! Учит парта, только пока на месте учителя сидит тот, кто знает больше
//! ученика, и доводит до уровня учителя минус один. Учитель садится сам —
//! лучший из тех, кто может научить, — и срывается ради этого с работы; сам он
//! за это не растёт. Нет учителя — ученик ждёт за партой.

use super::*;

const DESK: i16 = 1;
const TABLE: i16 = 2;
const LECTERN: i16 = 3;

/// Комната 7×2, парта-штамп `[парта, стол, учитель]` в верхнем ряду с (2, 1),
/// нижний ряд — проход. Порог «Науки» — 20 / 100 / 300, парта сама по себе
/// учит хоть до третьего уровня: предел задаёт учитель.
fn classroom(rows: &[&str]) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    let science = sim.set_skill("science", &[20, 100, 300]);
    sim.set_taught(science, 3);
    sim.set_teaches(DESK, science);
    sim.set_solid(TABLE, true);
    sim.tile_rule(LECTERN, |r| r.lectern = true);
    let def = sim.set_structure(vec![vec![Some(DESK), Some(TABLE), Some(LECTERN)]]);
    assert!(sim.place_structure(def as i32, 2, 1, 0), "штамп встал");
    sim.tick_n(80); // коты строят все три клетки
    assert_eq!(sim.tile(2, 1), DESK, "парта построена");
    assert_eq!(sim.tile(4, 1), LECTERN, "место учителя построено");
    (sim, science)
}

const ROOM: &[&str] = &["#########", "#.......#", "#a.....b#", "#########"];

/// Учить некому — ученик сидит за партой и ждёт, опыт не растёт.
#[test]
fn a_pupil_waits_without_a_teacher() {
    let (mut sim, science) = classroom(ROOM);
    assert!(sim.teach("a", "science"));
    sim.tick_n(40);
    assert_eq!(sim.pos_of("a"), (2, 1), "сидит за партой");
    assert!(sim.is_studying("a"), "и остаётся приписан к ней");
    assert_eq!(sim.xp_of("a", science), 0, "но без учителя не учится");
    assert!(!sim.is_teaching("b"), "b учить нечему — он сам ноль");
}

/// Учитель садится сам и доводит ученика до своего уровня минус один; сам за
/// это не растёт.
#[test]
fn a_teacher_sits_down_and_teaches_up_to_his_level_minus_one() {
    let (mut sim, science) = classroom(ROOM);
    sim.set_xp("b", science, 300); // третий уровень
    assert!(sim.teach("a", "science"));
    sim.tick_n(20);
    assert!(sim.is_teaching("b"), "учитель сел сам");
    assert_eq!(sim.pos_of("b"), (4, 1), "на своё место у парты");

    sim.tick_n(400);
    assert_eq!(sim.xp_of("a", science), 100, "ровно второй уровень");
    assert_eq!(sim.xp_of("b", science), 300, "учитель за учёбу не растёт");
}

/// Выучил всё, что этот учитель знает, — учитель встаёт: держать его у парты
/// значит молча отнимать у базы работника.
#[test]
fn a_teacher_leaves_when_he_has_nothing_left_to_teach() {
    let (mut sim, science) = classroom(ROOM);
    sim.set_xp("b", science, 100); // второй уровень — учит только до первого
    assert!(sim.teach("a", "science"));
    sim.tick_n(200);
    assert_eq!(sim.xp_of("a", science), 20, "первый уровень");
    assert!(!sim.is_teaching("b"), "учить больше нечему — встал");
    assert_eq!(sim.xp_of("a", science), 20);
}

/// Учитель срывается с работы (§12.258): учить или исследовать — выбор игрока,
/// и сделан он приписью ученика.
#[test]
fn a_teacher_is_pulled_off_his_work() {
    let (mut sim, science) = classroom(ROOM);
    sim.set_xp("b", science, 300);
    // Занимаем b стройкой в дальнем углу.
    assert!(sim.add_blueprint(7, 1, 5));
    sim.tick_n(2);
    assert!(
        sim.has_assignment("b"),
        "b ближе всех к площадке — строит он"
    );
    assert!(sim.teach("a", "science"));
    sim.tick_n(3);
    assert!(sim.is_teaching("b"), "бросил стройку ради ученика");
    assert!(!sim.has_assignment("b"));
}
