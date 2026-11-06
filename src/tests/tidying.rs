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

/// Сквозная проверка на настоящем `core.yaml`: снос хода к шлюзу вместе с самим
/// шлюзом возвращает лом, и коты свозят его на склад. Единственный тест, который
/// трогает боевой контент, — он ловит рассогласование кода и рулсета (пропавшую
/// `capacity`, склад без прохода, сломанный YAML), которое синтетические схемы
/// не увидят.
///
/// Под рамку намеренно попадают **две разные цены**: пол по лому и шлюз за
/// «2 лома + деталь». Цена из одного предмета не поймала бы возврат набором.
/// Рамка — ход из северной комнаты в верхний ряд (§12.348).
///
/// Жест один: шлюз лежит на пустоте, как пол (§12.348), — уровень у них общий,
/// и ластик снимает оба разом (§12.237).
#[test]
fn the_shipped_ruleset_tidies_demolition_scrap() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).expect("рулсет должен разбираться");
    sim.without_timeline(); // тест про материал, а не про мир по расписанию
    sim.add_storage(); // увозить некуда, пока склада нет (§12.215)
    let before = sim.scrap_total();

    assert!(
        sim.plan_demolish_rect(5, 0, 1, 2),
        "жест сносит ход со шлюзом"
    );
    sim.tick_n(3000);

    assert_eq!(sim.tile(5, 0), -1, "под шлюзом пустота, а не пол");
    assert_eq!(sim.floors_left([5, 0, 1, 2]), 0, "ход разобран целиком");
    assert_eq!(
        sim.scrap_total(),
        before + 4,
        "пол по лому и шлюз за «2 лома + деталь» вернулись целиком: счёт \
         штучный, деталь в нём тоже"
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

// --- груз, которому некуда деться ------------------------------------------

/// Склад полон, в лапах у кота вещь, которую не ждёт ни одна площадка, а
/// чертежу нужен другой материал: кот кладёт вещь под ноги и освобождается.
/// Иначе он стоял бы с ней вечно — уборке её нести некуда, а за чужим
/// материалом с полными лапами не сходить. Так в партии игрока кот с
/// комбинезоном дремал, пока пол строил вернувшийся отряд.
#[test]
fn an_undeliverable_load_is_put_down() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    sim.set_cost(0, 1);
    sim.set_capacity(1, 2);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(7, 1, 2); // склад забит
    sim.give_load("a", 1, 1);
    assert!(sim.add_blueprint(3, 2, 0), "чертежу нужен лом");

    sim.tick_n(5);
    assert_eq!(sim.carrying_of("a"), 0, "лапы свободны");
    assert_eq!(sim.item_total(1), 1, "вещь лежит на полу, а не пропала");
}

/// Работы для пустых лап нет — груз остаётся при коте, даже если сдать его
/// некуда: класть его на пол незачем, и лом не рассыпается по базе.
#[test]
fn an_undeliverable_load_stays_without_work() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    sim.set_capacity(1, 2);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(7, 1, 2); // склад забит
    sim.give_load("a", 1, 1);

    sim.tick_n(50);
    assert_eq!(sim.carrying_of("a"), 1, "груз при коте");
}

/// Освободив лапы, тот же кот идёт за ломом к чертежу: стройку на базе с
/// полным складом ведёт тот, кто дома, а не ждёт вернувшийся отряд.
#[test]
fn a_cat_with_an_undeliverable_load_still_builds() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    sim.set_cost(0, 1);
    sim.set_capacity(1, 2);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(7, 1, 2); // склад забит
    sim.put_scrap(5, 1, 1); // лом для пола
    sim.give_load("a", 1, 1);
    assert!(sim.add_blueprint(3, 2, 0), "пол на пустоту под коридором");

    sim.tick_n(BUILD_TICKS + 200);
    assert_eq!(sim.tile(3, 2), 0, "пол построен");
    assert_eq!(sim.item_total(1), 1, "отложенная вещь цела");
}

/// Груз, который ждёт площадка, кот несёт туда, а не бросает: под ноги
/// кладётся только то, чему на базе нет адресата вовсе.
#[test]
fn a_wanted_load_is_carried_not_dropped() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    sim.set_cost_items(0, &[(1, 1)]);
    sim.set_capacity(1, 2);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(7, 1, 2); // склад забит
    sim.give_load("a", 1, 1);
    assert!(sim.add_blueprint(3, 2, 0));

    sim.tick_n(3);
    assert!(sim.has_haul("a"), "кот понёс груз на площадку");
    assert_eq!(sim.carrying_of("a"), 1, "и из лап его не выпустил");
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

/// **Боевой рулсет: склада на старте нет, но он по карману** (§12.215).
///
/// Обе половины важны, и вторая тише. Склад — единственная дорога к учёту
/// (§12.69): без него не пустить образец в науку, не заплатить за найм, не
/// продать. Стройка — единственное, на что годится неучтённое, — поэтому
/// первый склад обязан покрываться тем, что лежит на полу с нулевого тика.
/// Сделай его на деталь дороже стартового запаса, и партия упрётся в стену
/// молча: кнопка в палитре есть, а построить нечем.
///
/// Первую половину сторожим отдельно от `sim_from`-тестов: стартовый склад —
/// ровно то, что §12.215 убрала, и вернувшийся в `build:` он отменил бы весь
/// урок, не уронив ни одного теста.
#[test]
fn the_shipped_ruleset_starts_without_storage_it_can_afford() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let storage = sim.tile_index("storage").expect("склад в палитре");

    for (item, _) in sim.cost_of(storage) {
        let (stored, _, _) = sim.stock_of(item);
        assert_eq!(
            stored, 0,
            "на старте учтённого нет: склада ещё не построили"
        );
    }
    for (item, need) in sim.cost_of(storage) {
        let (_, loose, _) = sim.stock_of(item);
        assert!(
            loose >= need,
            "первый склад стоит {need} предмета {item}, а на полу лежит {loose}",
        );
    }
}

/// **Боевой рулсет: построенный на старте склад собирает в себя запас**
/// (§12.216).
///
/// Это и есть та половина урока, ради которой склад открыт с нулевого тика:
/// игрок ставит одну клетку и **видит**, как валяющееся добро превращается в
/// учтённое, — вместо того чтобы узнать роль склада из отказа после первой
/// вылазки (§12.215). Сторож держит саму награду: пропади она (закройся склад
/// воротами, отвались уборка, перестань `stock:` лежать на полу), и решение
/// останется в тексте, а в игре его не будет.
#[test]
fn the_shipped_ruleset_fills_a_freshly_built_storage() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // мир по расписанию тут только шум
    let scrap = sim.item_index("scrap").expect("лом в палитре");
    let storage = sim.tile_index("storage").expect("склад в палитре");

    let (stored, loose, _) = sim.stock_of(scrap);
    assert_eq!(stored, 0, "учтённого на старте нет");
    assert!(loose > 0, "а на полу лежит: {loose}");
    let all = stored + loose;

    // Одна клетка в жилой комнате, рядом с котами: ставим её напрямую, потому
    // что проверяем уборку, а не стройку (ту ведут свои тесты).
    sim.force_tile(7, 11, storage);
    sim.tick_n(2000);

    let (stored, loose, _) = sim.stock_of(scrap);
    assert!(stored > 0, "коты снесли лом в склад сами");
    // Сумма «склад + неучтённое» — то же число, что было на старте: уборка
    // перекладывает, а не создаёт и не съедает (инвариант 11). Считаем по
    // тройке из шапки (§12.53), потому что груз в лапах она уже учитывает.
    assert_eq!(stored + loose, all, "и ничего по дороге не потеряли");
}
