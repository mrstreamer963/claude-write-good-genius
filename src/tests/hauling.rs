//! Материал: доставка лома на площадку, возврат его при сносе, судьба груза.

use super::sim_from;

/// Схема на все тесты доставки: коридор `y = 1`, площадка строится в стене под
/// ним. Стоимость и кучи каждый тест задаёт сам.
const CORRIDOR: [&str; 3] = ["##########", "#a.......#", "##########"];

// --- доставка -------------------------------------------------------------

/// Базовый цикл: пока лом не привезли, стройки нет; кот идёт к куче, берёт
/// ровно недостающее и возвращается на площадку.
#[test]
fn scrap_is_carried_to_the_site_before_building() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.put_scrap(8, 1, 5);
    assert!(sim.add_blueprint(4, 2, 0));

    sim.tick_n(8);
    assert_eq!(sim.tile(4, 2), -1, "без материала стройка не начата");
    assert_eq!(sim.delivered_at(4, 2), Some(0), "лом ещё не привезли");

    let mut carried = false;
    for _ in 0..120 {
        sim.tick_n(1);
        carried |= sim.carrying_of("a") > 0;
    }
    assert!(carried, "кот нёс лом в лапах");
    assert_eq!(sim.tile(4, 2), 0, "площадка обеспечена — тайл построен");
    assert_eq!(sim.scrap_at(8, 1), 3, "с кучи ушла ровно цена тайла");
    assert_eq!(sim.carrying_of("a"), 0, "лишнего кот не набирал");
}

/// Лома нет вовсе — чертёж ждёт, но кота собой не занимает: он берётся за
/// работу, которая материала не требует.
#[test]
fn an_unsupplied_site_does_not_hold_the_cat() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.set_cost(1, 0);
    assert!(sim.add_blueprint(8, 2, 0), "дорогой чертёж — без материала");
    assert!(
        sim.add_blueprint(2, 2, 1),
        "бесплатный чертёж рядом с котом"
    );

    sim.tick_n(120);
    assert_eq!(sim.tile(2, 2), 1, "бесплатный тайл построен");
    assert_eq!(sim.tile(8, 2), -1, "дорогой так и ждёт лома");
    assert!(!sim.stuck_of("a"), "кот не залип у необеспеченной площадки");
}

/// Одной ходки не хватило — кот идёт за остатком ко второй куче.
#[test]
fn a_partial_pile_takes_a_second_trip() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 3);
    sim.put_scrap(2, 1, 1);
    sim.put_scrap(8, 1, 2);
    assert!(sim.add_blueprint(5, 2, 0));

    sim.tick_n(200);
    assert_eq!(sim.tile(5, 2), 0, "тайл построен за две ходки");
    assert_eq!(sim.scrap_at(2, 1), 0, "первая куча разобрана");
    assert_eq!(sim.scrap_at(8, 1), 0, "вторая тоже");
}

/// Двое котов везут лом к разным площадкам параллельно: перенос — обычная
/// задача, а не узкое место на одного носильщика.
#[test]
fn two_cats_haul_in_parallel() {
    let mut sim = sim_from(&["##########", "#a......b#", "##########"]);
    sim.set_cost(0, 2);
    sim.put_scrap(1, 1, 2);
    sim.put_scrap(8, 1, 2);
    assert!(sim.add_blueprint(2, 2, 0));
    assert!(sim.add_blueprint(7, 2, 0));

    let mut both_hauling = false;
    for _ in 0..120 {
        sim.tick_n(1);
        both_hauling |= sim.has_haul("a") && sim.has_haul("b");
    }
    assert!(both_hauling, "оба кота несли лом одновременно");
    assert_eq!(sim.tile(2, 2), 0, "левая площадка построена");
    assert_eq!(sim.tile(7, 2), 0, "правая тоже");
}

// --- возврат и экономика --------------------------------------------------

/// Снос возвращает цену тайла ломом — под ноги коту, то есть на пол со стороны
/// берега, а не в пустоту, которую он же и создал.
#[test]
fn demolished_tile_returns_its_scrap() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_cost(0, 2);
    assert!(sim.plan_demolish(3, 1));

    sim.tick_n(200);
    assert_eq!(sim.tile(3, 1), -1, "тайл снесён");
    assert_eq!(
        sim.scrap_at(2, 1),
        2,
        "лом лежит на клетке, где работал кот"
    );
}

/// Возврат не должен оставаться в пустоте. Клетка, с которой кот сносил
/// соседа, — это клетка на шаг ближе к берегу, и волна убирает её следующей:
/// без присмотра куча оказывается в яме, куда за ней не дойти.
///
/// Проверяем каждый тик: к концу мазка яма может уже зарасти чем угодно, и по
/// конечному состоянию потеря неотличима от нормы.
#[test]
fn returned_scrap_never_stays_in_the_void() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    sim.set_cost(0, 2);
    assert!(sim.plan_demolish_rect(2, 1, 3, 1));

    for _ in 0..400 {
        sim.tick_n(1);
        assert!(sim.scrap_is_on_floor(), "куча лома провалилась в пустоту");
    }
    assert_eq!(sim.floors_left([2, 1, 3, 1]), 0, "мазок снесён целиком");
}

/// Петля замыкается: снесённое оплачивает следующую стройку, хотя стартового
/// лома в мире нет вовсе.
#[test]
fn demolition_pays_for_the_next_build() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_cost(0, 2);
    assert!(sim.plan_demolish(3, 1));
    sim.tick_n(200);
    assert_eq!(sim.tile(3, 1), -1, "снос прошёл");

    assert!(sim.add_blueprint(3, 1, 0), "строим обратно на возврат");
    sim.tick_n(200);
    assert_eq!(sim.tile(3, 1), 0, "тайл восстановлен из возвращённого лома");
    assert_eq!(sim.scrap_at(2, 1), 0, "весь возврат ушёл в стройку");
}

// --- судьба груза ---------------------------------------------------------

/// Приказ снимает задачу переноса, но не груз: лом остаётся на коте и потом
/// доезжает до площадки, а не теряется и не сыплется на пол.
#[test]
fn an_order_releases_the_haul_but_not_the_load() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.put_scrap(8, 1, 5);
    assert!(sim.add_blueprint(4, 2, 0));

    while sim.carrying_of("a") == 0 {
        sim.tick_n(1);
    }
    assert!(sim.set_target("a", 1, 1), "уводим носильщика приказом");
    assert!(!sim.has_haul("a"), "задача переноса снята");
    assert_eq!(sim.carrying_of("a"), 2, "груз остался в лапах");

    sim.tick_n(200);
    assert_eq!(sim.tile(4, 2), 0, "тот же лом всё-таки доехал до площадки");
}

/// Отмена чертежа посреди доставки: груз тоже остаётся на коте — иначе каждая
/// исправленная разметка сыпала бы лом по всей базе.
#[test]
fn a_cancelled_site_leaves_the_load_on_the_cat() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.put_scrap(8, 1, 5);
    assert!(sim.add_blueprint(4, 2, 0));

    while sim.carrying_of("a") == 0 {
        sim.tick_n(1);
    }
    assert!(sim.plan_demolish(4, 2), "ластик снимает чертёж");

    sim.tick_n(20);
    assert!(
        !sim.has_haul("a"),
        "задача переноса снята вместе с чертежом"
    );
    assert_eq!(sim.carrying_of("a"), 2, "груз при коте");
    assert_eq!(sim.scrap_at(8, 1), 3, "на пол ничего не высыпалось");
}

/// Отмена площадки, на которую материал уже **сдали**: он возвращается кучей на
/// пол, а не исчезает (§12.31). Ошибка разметки стоит котовремени — терять за
/// неё материал незачем.
#[test]
fn a_cancelled_site_returns_the_delivered_material() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.put_scrap(8, 1, 5);
    assert!(sim.add_blueprint(4, 2, 0));

    // Ждём, пока лом окажется именно на площадке, а не в лапах.
    while sim.delivered_at(4, 2).unwrap_or(0) == 0 {
        sim.tick_n(1);
    }
    let before = sim.scrap_total();
    assert!(sim.plan_demolish(4, 2), "ластик снимает чертёж");

    assert_eq!(sim.scrap_at(4, 2), 2, "завезённое легло кучей на клетку");
    assert_eq!(sim.scrap_total(), before, "и ничего не пропало");
}

/// Возврат ложится на саму площадку, а она обычно ещё пустота, — оттуда кучу
/// штатно сдвигает `settle_stacks` (§12.15), отдельного случая не нужно.
#[test]
fn material_returned_into_a_void_settles_onto_the_floor() {
    let mut sim = sim_from(&["#####", "#a..#", "#.###", "#####"]);
    sim.set_cost(0, 2);
    sim.put_scrap(3, 1, 4);
    sim.force_tile(1, 2, -1); // площадка в пустоте
    assert!(sim.add_blueprint(1, 2, 0));

    while sim.delivered_at(1, 2).unwrap_or(0) == 0 {
        sim.tick_n(1);
    }
    let before = sim.scrap_total();
    assert!(sim.plan_demolish(1, 2));

    sim.tick_n(2);
    assert_eq!(sim.scrap_at(1, 2), 0, "в пустоте лом не остался");
    assert_eq!(sim.scrap_total(), before, "он съехал на пол, а не пропал");
    assert!(sim.scrap_is_on_floor(), "и лежит на проходимой клетке");
}
