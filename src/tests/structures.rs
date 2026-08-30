//! Объект-штамп: постройка, занимающая несколько клеток одним решением (§12.160).
//!
//! Штамп — это сетка **обычных** тайлов, поэтому проверять надо не «появился ли
//! новый вид клетки», а три вещи, которых у одиночной разметки нет: решение
//! атомарно, геометрия поворота одна на всех, и объект перестаёт существовать,
//! как только перестал совпадать с картой.

use super::*;

/// Двухрядный штамп: верхний ряд — тайл `top`, нижний — `bottom`.
/// Дословно лаборатория из §12.160: глухой ряд и ряд мест под ним.
fn two_rows(sim: &mut Sim, top: i16, bottom: i16, width: usize) -> usize {
    sim.set_structure(vec![vec![Some(top); width], vec![Some(bottom); width]])
}

#[test]
fn a_stamp_marks_every_cell_of_its_footprint() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    assert!(sim.place_structure(def as i32, 2, 1, 0));

    for x in 2..5 {
        assert_eq!(sim.planned_tile(x, 1), Some(1), "верхний ряд в ({x}, 1)");
        assert_eq!(sim.planned_tile(x, 2), Some(2), "нижний ряд в ({x}, 2)");
    }
    assert_eq!(sim.structures_count(), 1);
}

#[test]
fn a_stamp_that_does_not_fit_marks_nothing() {
    // Штамп 3×2 якорем в (5, 1) вылезает за правый край карты: карта шириной 7,
    // а третий столбец штампа встал бы в x = 7.
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    assert!(!sim.place_structure(def as i32, 5, 1, 0));

    // Атомарность: ни одной клетки, даже той, что помещалась (§12.160).
    assert_eq!(sim.planned_tile(5, 1), None);
    assert_eq!(sim.planned_tile(6, 1), None);
    assert_eq!(sim.structures_count(), 0);
}

#[test]
fn rotation_swaps_the_sides_of_the_footprint() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    // Четверть оборота: 3×2 становится 2×3.
    assert!(sim.place_structure(def as i32, 2, 1, 1));

    // Верхний ряд исходной сетки уезжает в правый столбец.
    for y in 1..4 {
        assert_eq!(sim.planned_tile(3, y), Some(1), "правый столбец в (3, {y})");
        assert_eq!(sim.planned_tile(2, y), Some(2), "левый столбец в (2, {y})");
    }
}

#[test]
fn four_quarter_turns_return_the_stamp_to_where_it_started() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    assert!(sim.place_structure(def as i32, 2, 1, 4));

    for x in 2..5 {
        assert_eq!(sim.planned_tile(x, 1), Some(1));
        assert_eq!(sim.planned_tile(x, 2), Some(2));
    }
}

#[test]
fn an_empty_cell_in_the_grid_is_left_alone() {
    // Г-образный штамп выражается дыркой в сетке, а не вторым понятием.
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = sim.set_structure(vec![vec![Some(1), Some(1)], vec![Some(2), None]]);

    assert!(sim.place_structure(def as i32, 2, 1, 0));

    assert_eq!(sim.planned_tile(2, 1), Some(1));
    assert_eq!(sim.planned_tile(3, 1), Some(1));
    assert_eq!(sim.planned_tile(2, 2), Some(2));
    assert_eq!(sim.planned_tile(3, 2), None, "дырка в сетке остаётся полом");
}

#[test]
fn erasing_one_cell_condemns_the_whole_object() {
    // Половина лаборатории — не лаборатория на полтора места, а мусор (§12.160).
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);
    assert!(sim.place_structure(def as i32, 2, 1, 0));

    // Ластик по одной клетке снимает чертежи всего штампа разом.
    assert!(sim.plan_demolish(3, 1));

    for x in 2..5 {
        assert_eq!(sim.planned_tile(x, 1), None, "верхний ряд в ({x}, 1)");
        assert_eq!(sim.planned_tile(x, 2), None, "нижний ряд в ({x}, 2)");
    }
    assert_eq!(sim.structures_count(), 0);
}

#[test]
fn a_built_object_forgets_itself_when_a_cell_stops_matching() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    // Кладём тайлы штампа заранее: тогда чертежей не заводится вовсе, и объект
    // сразу считается построенным — то же состояние, в которое его приводит
    // `work_jobs`, только без сотни тиков стройки.
    for x in 2..5 {
        sim.force_tile(x, 1, 1);
        sim.force_tile(x, 2, 2);
    }
    assert!(sim.place_structure(def as i32, 2, 1, 0));
    sim.tick_n(1);
    assert!(sim.structure_here(2, 1), "целый объект остаётся объектом");

    // Одна клетка перестала быть собой — объекта больше нет (§12.160).
    sim.force_tile(3, 1, 0);
    sim.tick_n(1);
    assert!(!sim.structure_here(2, 1));
    assert_eq!(sim.structures_count(), 0);
}

#[test]
fn the_preview_mask_answers_for_the_whole_stamp() {
    // Маска у объекта отвечает «встанет ли отсюда весь штамп», а не «можно ли
    // сюда тайл»: у объекта запрещённой бывает только постановка целиком.
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);

    let mask = sim.structure_buildable(def as i32, 0, 0, 7, 4, 0);
    let at = |x: i32, y: i32| mask[(y * 7 + x) as usize];

    assert_eq!(at(2, 1), 1, "штамп целиком на карте — можно");
    assert_eq!(at(5, 1), 0, "правым столбцом за карту — нельзя");
    assert_eq!(at(2, 3), 0, "нижним рядом за карту — нельзя");
}

#[test]
fn a_locked_object_does_not_go_up_even_when_its_tiles_are_open() {
    // Ворота у объекта свои: тайлы штампа открыты с начала партии, а сам он —
    // нет. Ровно так лаборатория на три места становится наукой (§12.160).
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    let def = two_rows(&mut sim, 1, 2, 3);
    sim.set_structure_tech(def, "big_labs");

    assert!(!sim.place_structure(def as i32, 2, 1, 0), "тема не изучена");
    assert_eq!(sim.planned_tile(2, 1), None);
    assert_eq!(sim.structures_count(), 0);

    sim.set_tech("big_labs");
    assert!(sim.place_structure(def as i32, 2, 1, 0), "тема изучена");
    assert_eq!(sim.planned_tile(2, 1), Some(1));
}
