//! Уборка: лом с пола едет на склад — сам или по разметке игрока.
//!
//! Склад в этих тестах делается вручную: тайлу `1` назначается ёмкость
//! (`set_capacity`), и нужные клетки переводятся в него через `force_tile`.
//! Тайл `0` (обычный пол) ёмкости не имеет, как и в рулсете.

use super::{BUILD_TICKS, sim_from};
use crate::sim::Sim;

const CORRIDOR: [&str; 3] = ["#########", "#a......#", "#########"];

// --- автоуборка ------------------------------------------------------------

/// Базовый случай: куча на полу уезжает на склад без единого приказа.
#[test]
fn loose_scrap_is_carried_to_storage() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 4);

    sim.tick_n(200);
    assert_eq!(sim.scrap_at(7, 1), 4, "весь лом на складе");
    assert_eq!(sim.scrap_at(2, 1), 0, "на полу не осталось");
    assert_eq!(sim.carrying_of("a"), 0, "и не в лапах");
}

/// Лом, который уже лежит на складе, коты не трогают: иначе доставленное
/// становилось бы задачей на самого себя и ездило по кругу.
#[test]
fn scrap_in_storage_is_left_alone() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(7, 1, 4);

    sim.tick_n(300);
    assert_eq!(sim.scrap_at(7, 1), 4, "куча на складе не шевелилась");
    assert_eq!(sim.carrying_of("a"), 0);
}

/// Уборка — последняя в очереди работ: подбирать мусор, пока стоит стройка,
/// игрок не просил.
#[test]
fn tidying_never_outranks_building() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 4);
    assert!(
        sim.add_blueprint(3, 2, 0),
        "бесплатный чертёж (цена тайла 0)"
    );

    sim.tick_n(BUILD_TICKS / 2);
    assert!(sim.has_assignment("a"), "кот занят стройкой");
    assert_eq!(sim.scrap_at(2, 1), 4, "к куче он не притронулся");

    sim.tick_n(300);
    assert_eq!(sim.tile(3, 2), 0, "стройка закончена");
    assert_eq!(sim.scrap_at(7, 1), 4, "и только потом убрано");
}

/// Складов нет — уносить некуда, и куча остаётся лежать на виду. Забрать её
/// «в лапы» было бы хуже всего: с пола лом исчез, а игрок его не достанет.
#[test]
fn no_storage_means_scrap_stays_put() {
    let mut sim = sim_from(&CORRIDOR);
    sim.put_scrap(3, 1, 4);

    sim.tick_n(200);
    assert_eq!(sim.scrap_at(3, 1), 4, "куча на месте");
    assert_eq!(sim.carrying_of("a"), 0, "и не в лапах у кота");
}

/// Ёмкость клетки соблюдается: остаток едет в следующую, а не сваливается
/// сверх меры.
#[test]
fn a_full_storage_cell_is_skipped() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 3);
    sim.force_tile(6, 1, 1);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 5);

    sim.tick_n(400);
    assert_eq!(sim.scrap_at(2, 1), 0, "куча разобрана");
    assert!(
        sim.scrap_at(6, 1) <= 3 && sim.scrap_at(7, 1) <= 3,
        "ёмкость цела"
    );
    assert_eq!(
        sim.scrap_at(6, 1) + sim.scrap_at(7, 1),
        5,
        "лом разложен по двум клеткам"
    );
}

/// Кот с грузом от отменённого чертежа уносит его на склад, а не держит вечно
/// (хвост §12.15).
#[test]
fn a_leftover_load_goes_to_storage() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 2);
    sim.set_capacity(1, 20);
    sim.force_tile(1, 1, 1); // склад под ногами у кота
    sim.put_scrap(7, 1, 5);
    assert!(sim.add_blueprint(4, 2, 0));

    while sim.carrying_of("a") == 0 {
        sim.tick_n(1);
    }
    assert!(sim.plan_demolish(4, 2), "ластик снимает чертёж под грузом");

    sim.tick_n(400);
    assert_eq!(sim.carrying_of("a"), 0, "груз не завис в лапах");
    assert_eq!(sim.scrap_at(1, 1), 5, "весь лом доехал до склада");
}

/// Лом не размножается и не пропадает ни на одном тике: конечное состояние
/// потерю замаскировало бы (как в `returned_scrap_never_stays_in_the_void`).
#[test]
fn tidying_never_loses_scrap() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 4);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 3);
    sim.put_scrap(4, 1, 3);

    for _ in 0..300 {
        sim.tick_n(1);
        assert_eq!(sim.scrap_total(), 6, "лом не размножился и не пропал");
    }
    assert_eq!(sim.scrap_at(7, 1), 4, "склад заполнен под завязку");
}

// --- выключатель и ручная разметка -----------------------------------------

/// С выключенной автоуборкой коты мусор не трогают.
#[test]
fn auto_tidy_off_leaves_scrap_alone() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 4);
    sim.set_auto_tidy(false);

    sim.tick_n(300);
    assert_eq!(sim.scrap_at(2, 1), 4, "куча осталась лежать");
}

/// Выключатель снимает пометки и разворачивает кота, который уже шёл за кучей:
/// иначе он выглядел бы сломанным.
#[test]
fn switching_auto_tidy_off_turns_the_cat_around() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(1, 1, 1); // склад под котом, куча — в другом конце
    sim.put_scrap(7, 1, 4);

    sim.tick_n(2);
    assert!(sim.has_haul("a"), "кот вышел за кучей");

    sim.set_auto_tidy(false);
    assert!(!sim.has_haul("a"), "выключатель развернул его с полдороги");
    sim.tick_n(300);
    assert_eq!(sim.scrap_at(7, 1), 4, "куча осталась лежать");
}

/// Разметка рамкой не адресуется коту: помеченную кучу забирает любой свободный,
/// а непомеченная остаётся лежать.
#[test]
fn a_marked_pile_is_taken_by_any_free_cat() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.set_auto_tidy(false);
    sim.put_scrap(2, 1, 3);
    sim.put_scrap(4, 1, 3);

    assert!(
        sim.mark_to_store_rect(2, 1, 1, 1),
        "помечена только левая куча"
    );
    sim.tick_n(300);
    assert_eq!(sim.scrap_at(2, 1), 0, "помеченная уехала");
    assert_eq!(sim.scrap_at(7, 1), 3, "и лежит на складе");
    assert_eq!(sim.scrap_at(4, 1), 3, "непомеченная не тронута");
}

/// Жест решает за всю рамку сразу (§12.13): есть под ней помеченное — снимаем,
/// нет — помечаем.
#[test]
fn a_second_gesture_unmarks_the_area() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.set_auto_tidy(false);
    sim.put_scrap(2, 1, 3);
    sim.put_scrap(3, 1, 3);

    assert!(
        sim.mark_to_store_rect(2, 1, 2, 1),
        "первый жест помечает обе"
    );
    assert!(sim.mark_to_store_rect(2, 1, 2, 1), "второй — снимает обе");
    sim.tick_n(300);
    assert_eq!(sim.scrap_at(2, 1), 3, "разметка отменена");
    assert_eq!(sim.scrap_at(3, 1), 3);
}

/// Пустая рамка ничего не меняет — сообщать рендеру не о чем.
#[test]
fn marking_empty_floor_does_nothing() {
    let mut sim = sim_from(&CORRIDOR);
    assert!(!sim.mark_to_store_rect(2, 1, 3, 1), "куч под рамкой нет");
}

// --- боевой рулсет ---------------------------------------------------------

/// Сквозная проверка на настоящем `core.yaml`: снос половины гаража возвращает
/// лом, и коты свозят его на склад. Единственный тест, который трогает боевой
/// контент, — он ловит рассогласование кода и рулсета (пропавшую `capacity`,
/// склад без прохода, сломанный YAML), которое синтетические схемы не увидят.
#[test]
fn the_shipped_ruleset_tidies_demolition_scrap() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).ok().expect("рулсет должен разбираться");
    sim.without_timeline(); // тест про материал, а не про мир по расписанию
    let before = sim.scrap_total();

    assert!(
        sim.plan_demolish_rect(6, 10, 3, 4),
        "сносим половину гаража"
    );
    sim.tick_n(3000);

    assert_eq!(sim.floors_left([6, 10, 3, 4]), 0, "гараж разобран целиком");
    assert_eq!(
        sim.scrap_total(),
        before + 36,
        "12 тайлов по цене «2 лома + деталь» вернулись целиком"
    );
    assert!(sim.scrap_is_in_storage(), "и весь лом уехал на склад");
}
