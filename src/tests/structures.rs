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

// --- Счётность: слот — это объект, а не клетка (§12.161) ---

/// Ставит на карту готовый объект: тайлы кладутся сразу, чертежей не заводится.
/// Так проверяется счётность построенного, а не стройка.
fn built(sim: &mut Sim, def: usize, x: i32, y: i32, rot: i32) {
    let cells = {
        let rules = sim.world.resource::<StructureRules>();
        rules.0[def].stamp((x, y), rot.rem_euclid(4) as u8)
    };
    for ((cx, cy), t) in cells {
        sim.force_tile(cx, cy, t);
    }
    assert!(sim.place_structure(def as i32, x, y, rot), "объект встал");
}

#[test]
fn one_object_is_one_slot_however_many_cells_it_has() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    sim.set_lab(1, true);
    // Штамп из трёх клеток лаборатории: одна тема на три места, а не три темы.
    let def = sim.set_structure(vec![vec![Some(1), Some(1), Some(1)]]);

    built(&mut sim, def, 2, 1, 0);

    assert_eq!(sim.lab_slots(), 1, "три клетки одного объекта — один слот");
}

#[test]
fn loose_cells_still_count_one_by_one() {
    // Клетка вне объекта — слот сама по себе: она и есть объект из одной клетки.
    // На этом держится вся стартовая застройка, выложенная рамкой.
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    sim.set_lab(1, true);
    for x in 2..5 {
        sim.force_tile(x, 1, 1);
    }

    assert_eq!(sim.lab_slots(), 3, "три отдельные клетки — три слота");
}

#[test]
fn two_objects_are_two_slots() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    sim.set_shop(1, true);
    let def = sim.set_structure(vec![vec![Some(1), Some(1)]]);

    built(&mut sim, def, 2, 1, 0);
    built(&mut sim, def, 2, 2, 0);

    assert_eq!(sim.shop_slots(), 2, "второй объект — второй слот");
}

#[test]
fn a_hall_laid_as_one_object_does_not_multiply_raids() {
    // Ровно то, ради чего §12.161 и заведена: гараж, заложенный залом 6×2
    // штампом, даёт одну вылазку, а не двенадцать (§12.59, §12.152).
    let mut sim = sim_from(&["########", "#......#", "#......#", "########"]);
    sim.set_gate(1, true);
    let def = sim.set_structure(vec![vec![Some(1); 6], vec![Some(1); 6]]);

    built(&mut sim, def, 1, 1, 0);

    assert_eq!(
        sim.gate_count(),
        1,
        "зал одним объектом — один слот вылазки"
    );
}

#[test]
fn a_demolished_object_gives_its_cells_back_as_separate_slots() {
    // Объект перестал быть объектом (`prune_structures`), а тайлы остались:
    // клетки снова считаются поодиночке. Это не дыра, а то же правило —
    // «клетка без объекта сама себе слот», — и оно обязано быть проверяемым.
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#######"]);
    sim.set_lab(1, true);
    let def = sim.set_structure(vec![vec![Some(1), Some(1), Some(1)]]);
    built(&mut sim, def, 2, 1, 0);
    assert_eq!(sim.lab_slots(), 1);

    // Ломаем одну клетку: объекта больше нет, две оставшиеся — два слота.
    sim.force_tile(3, 1, 0);
    sim.tick_n(1);

    assert_eq!(sim.structures_count(), 0);
    assert_eq!(sim.lab_slots(), 2);
}

// --- Места: несколько учёных над одной темой (§12.163) ---

/// Лаборатория-штамп: глухой ряд сверху, ряд мест снизу — дословно §12.160.
/// Тайл 1 — глухая часть (в ней стоит машина, встать нельзя), тайл 2 — место.
/// Оба несут роль лаборатории: мест столько, сколько **проходимых** клеток.
fn lab_object(sim: &mut Sim, seats: usize) -> usize {
    sim.set_skill("science", &[100, 400]);
    sim.set_lab(1, true);
    sim.set_solid(1, true);
    sim.set_lab(2, true);
    sim.set_structure(vec![vec![Some(1); seats], vec![Some(2); seats]])
}

#[test]
fn a_three_seat_lab_seats_three_scientists_on_one_topic() {
    let mut sim = sim_from(&["######", "#abc.#", "#....#", "#....#", "######"]);
    let def = lab_object(&mut sim, 3);
    built(&mut sim, def, 1, 2, 0);
    let topic = sim.set_topic("materials", 0, 100000, &[], &[]);
    assert!(sim.start_research(topic), "тема взята");

    sim.tick_n(30);

    assert_eq!(sim.topic_crew(), 3, "все три места заняты");
    assert_eq!(sim.researchers_busy(), 1, "и тема при этом одна");
}

#[test]
fn three_scientists_work_three_times_faster() {
    // Скорость — сумма вкладов, а не число от размера лаборатории (§12.163).
    let progress = |rows: &[&str]| {
        let mut sim = sim_from(rows);
        let def = lab_object(&mut sim, 3);
        built(&mut sim, def, 1, 2, 0);
        let topic = sim.set_topic("materials", 0, 100000, &[], &[]);
        assert!(sim.start_research(topic));
        sim.tick_n(40);
        sim.research_progress().unwrap_or(0)
    };
    let one = progress(&["######", "#a...#", "#....#", "#....#", "######"]);
    let three = progress(&["######", "#abc.#", "#....#", "#....#", "######"]);

    assert!(one > 0, "одиночка всё-таки работает: {one}");
    assert!(
        three > one * 2,
        "трое обязаны обогнать одного втрое: {three} против {one}",
    );
}

#[test]
fn an_undermanned_lab_just_works_slower() {
    // Недобор — законное состояние, а не отказ: лаборатория работает вполсилы,
    // и это ровно та причина, по которой второго учёного стоит выучить.
    let mut sim = sim_from(&["######", "#a...#", "#....#", "#....#", "######"]);
    let def = lab_object(&mut sim, 3);
    built(&mut sim, def, 1, 2, 0);
    let topic = sim.set_topic("materials", 0, 100000, &[], &[]);
    assert!(sim.start_research(topic));

    sim.tick_n(30);

    assert_eq!(sim.topic_crew(), 1, "занято одно место из трёх");
    assert!(
        sim.research_progress().is_some_and(|p| p > 0),
        "и тема всё-таки движется",
    );
}

#[test]
fn a_scientist_joins_a_topic_already_under_way() {
    // Доучившийся кот подсаживается к идущей теме, а не ждёт следующей: иначе
    // связка с партой откладывалась бы на сотни тиков (§12.163).
    let mut sim = sim_from(&["######", "#ab..#", "#....#", "#....#", "######"]);
    let def = lab_object(&mut sim, 3);
    built(&mut sim, def, 1, 2, 0);
    sim.set_skill_level("a", "science", 1);
    // Теме нужен допуск: «b» его пока не дорос.
    let topic = sim.set_topic("materials", 1, 100000, &[], &[]);
    assert!(sim.start_research(topic));
    sim.tick_n(30);
    assert_eq!(sim.topic_crew(), 1, "работает только допущенный");

    sim.set_skill_level("b", "science", 1);
    sim.tick_n(30);

    assert_eq!(sim.topic_crew(), 2, "доучившийся подсел к идущей теме");
}

#[test]
fn a_loose_lab_cell_still_seats_exactly_one() {
    // Клетка вне объекта — одно место: она и есть объект из одной клетки, и
    // лаборатория, выложенная рамкой, ведёт себя ровно как до §12.160.
    let mut sim = sim_from(&["######", "#abc.#", "#....#", "######"]);
    sim.set_skill("science", &[100, 400]);
    sim.set_lab(2, true);
    sim.force_tile(1, 2, 2);
    let topic = sim.set_topic("materials", 0, 100000, &[], &[]);
    assert!(sim.start_research(topic));

    sim.tick_n(30);

    assert_eq!(sim.topic_crew(), 1, "одна клетка — одно место");
}

#[test]
fn an_internal_tile_cannot_be_marked_on_its_own() {
    // Правило, которое держит только палитра, — это правило, которого нет:
    // ворота обязаны стоять в ядре (§12.163, инвариант 14).
    let mut sim = sim_from(&["#####", "#...#", "#####"]);
    sim.set_internal(1, true);

    assert!(!sim.add_blueprint(2, 1, 1), "внутренность — не постройка");
    assert_eq!(sim.planned_tile(2, 1), None);
}
