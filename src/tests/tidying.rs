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
    let mut sim = Sim::new(yaml).expect("рулсет должен разбираться");
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

/// Ничью за кучу решает `id` кота, а не порядок сущностей — как и у чертежа
/// (`a_tie_is_broken_by_id_not_by_spawn_order` в `jobs`).
///
/// Уборка раздаётся тем же жадным перебором пар, и до §12.45 при равном
/// расстоянии выигрывал тот, кто раньше в таблице архетипа. Порядок вставок
/// у загруженного из снимка мира свой, и та же база повезла бы лом иначе.
#[test]
fn a_tie_over_a_pile_is_broken_by_id() {
    // Схемы отличаются только порядком появления котов: `sim_from` спавнит их
    // слева направо. Куча ровно посередине коридора — обоим по три шага, а
    // склад висит под ней, чтобы расстояние до него никого не выделяло.
    for rows in [
        ["#########", "#a.....b#", "####.####", "#########"],
        ["#########", "#b.....a#", "####.####", "#########"],
    ] {
        let mut sim = sim_from(&rows);
        sim.set_capacity(1, 20);
        sim.force_tile(4, 2, 1);
        sim.put_scrap(4, 1, 2);

        sim.tick_n(1);
        assert!(sim.has_haul("a"), "за кучей пошёл 'a' (схема {rows:?})");
        assert!(!sim.has_haul("b"), "и только он");
    }
}

// --- сколько котов на одну кучу (§12.48) -----------------------------------

/// Большую кучу разбирают несколько котов разом: одна ходка на кота, пока в
/// куче есть необещанное.
///
/// До §12.48 у пометки был claim на одного носильщика, и куча с вылазки или от
/// каравана превращала базу в одного работника и двух зрителей.
#[test]
fn a_big_pile_is_shared_by_several_cats() {
    let mut sim = sim_from(&["#########", "#a.b.c..#", "####.####", "#########"]);
    sim.set_capacity(1, 60);
    sim.force_tile(4, 2, 1);
    for cat in ["a", "b", "c"] {
        sim.set_carry(cat, 4);
    }
    sim.put_scrap(4, 1, 12); // трижды по четыре лапы

    sim.tick_n(1);
    for cat in ["a", "b", "c"] {
        assert!(sim.has_haul(cat), "за кучей пошёл и {cat}");
    }
}

/// А маленькую — один: обещанного хватило, и гнать вторую пару лап через
/// полбазы за пустым местом незачем (§12.34).
#[test]
fn a_small_pile_takes_only_one_cat() {
    let mut sim = sim_from(&["#########", "#a.b.c..#", "####.####", "#########"]);
    sim.set_capacity(1, 60);
    sim.force_tile(4, 2, 1);
    for cat in ["a", "b", "c"] {
        sim.set_carry(cat, 4);
    }
    sim.put_scrap(4, 1, 3); // меньше одних лап

    sim.tick_n(1);
    let went = ["a", "b", "c"].iter().filter(|c| sim.has_haul(c)).count();
    assert_eq!(went, 1, "пошёл ровно один кот");
}

/// Раздачу режет и место на складе: коту, которому некуда сдать, груз осел бы
/// в лапах, а лапы игроку не видны и не размечаются (§12.16).
#[test]
fn tidying_sends_no_more_cats_than_the_storage_holds() {
    let mut sim = sim_from(&["#########", "#a.b.c..#", "####.####", "#########"]);
    sim.set_capacity(1, 4); // одна клетка склада на четыре штуки
    sim.force_tile(4, 2, 1);
    for cat in ["a", "b", "c"] {
        sim.set_carry(cat, 4);
    }
    sim.put_scrap(4, 1, 12);

    sim.tick_n(1);
    let went = ["a", "b", "c"].iter().filter(|c| sim.has_haul(c)).count();
    assert_eq!(went, 1, "склад держит одну ходку — идёт один кот");
}

/// Куча уезжает целиком и ничего не оседает в лапах, сколько бы котов её ни
/// разбирало.
#[test]
fn a_shared_pile_leaves_nothing_in_paws() {
    let mut sim = sim_from(&["#########", "#a.b.c..#", "####.####", "#########"]);
    sim.set_capacity(1, 60);
    sim.force_tile(4, 2, 1);
    for cat in ["a", "b", "c"] {
        sim.set_carry(cat, 4);
    }
    sim.put_scrap(4, 1, 12);

    sim.tick_n(300);
    assert_eq!(sim.scrap_at(4, 2), 12, "весь лом на складе");
    for cat in ["a", "b", "c"] {
        assert_eq!(sim.carrying_of(cat), 0, "и ничего не осталось у {cat}");
    }
}

// --- очередь: пост вперёд прочих (§12.98) ----------------------------------

/// Куча в ячейке торгового поста разбирается раньше ближней кучи на полу:
/// она держит торговый слот, а обычный мусор — только место.
#[test]
fn a_pile_on_a_trade_post_is_tidied_first() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.set_trade_post(2, true);
    sim.force_tile(5, 1, 2);
    sim.put_scrap(2, 1, 4); // рядом с котом
    sim.put_scrap(5, 1, 4); // вчетверо дальше, но в ячейке поста

    let mut first = None;
    for _ in 0..200 {
        sim.tick_n(1);
        if sim.scrap_at(5, 1) == 0 {
            first = Some("post");
            break;
        }
        if sim.scrap_at(2, 1) == 0 {
            first = Some("floor");
            break;
        }
    }
    assert_eq!(first, Some("post"), "кот пошёл за постовой кучей");
    assert_eq!(
        sim.scrap_at(2, 1),
        4,
        "ближняя куча дождалась своей очереди"
    );

    sim.tick_n(300);
    assert_eq!(sim.scrap_at(7, 1), 8, "в итоге на складе обе");
}

/// Внутри одной очереди правило прежнее (§12.14): из двух обычных куч кот
/// берётся за ближнюю.
#[test]
fn among_ordinary_piles_the_nearest_still_wins() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 4);
    sim.put_scrap(5, 1, 4);

    let mut first = None;
    for _ in 0..200 {
        sim.tick_n(1);
        if sim.scrap_at(5, 1) == 0 {
            first = Some("far");
            break;
        }
        if sim.scrap_at(2, 1) == 0 {
            first = Some("near");
            break;
        }
    }
    assert_eq!(first, Some("near"), "ближняя куча первой");
}
