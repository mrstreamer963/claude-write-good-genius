//! Производство: заказ → работа в мастерской → предмет под ноги (§12.30).
//!
//! Заказ — разметка работы, как чертёж и как тема, но первый **повторяемый**:
//! рецепт крутится, пока не выйдет заказанное число штук. Проверять надо именно
//! это и оплату по штуке — остальное те же схемы, что у науки.
//!
//! Мир везде один: коридор со складом (тайл 1) и мастерской (тайл 2) — в схеме
//! `sim_from` ни того, ни другого нет, поэтому свойства задаём явно.
//!
//! ⚠️ С §12.102 материал **везут ногами**, а не списывают со склада мгновенно,
//! поэтому между заявкой и первым очком работы стоит целая ходка: кот идёт к
//! куче, берёт груз, несёт к станку и только потом садится за него.
//!
//! Отсюда же и то, что мастером в этих схемах оказывается `b`, а не `a`:
//! носильщика выбирают по дороге к куче (§12.14), `b` стоит к складу вплотную,
//! а сдав груз, он оказывается **ближайшим свободным котом к станку** — и
//! садится за него сам. Это не случайность схемы, а следствие подвоза: везёт и
//! делает обычно один и тот же кот.

use super::*;

/// Из чего делают и что выходит: два типа предмета в мире теста.
const SCRAP: usize = 0;
const PART: usize = 1;

/// Коридор: склад в (5,1), мастерская в (3,1). Вернёт мир с одним котом `a`.
fn sim_with_shop() -> Sim {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.set_shop(2, true);
    sim.force_tile(3, 1, 2);
    sim
}

/// То же, но мастерских две — (3,1) и (4,1), и котов тоже двое (§12.55).
fn sim_with_two_shops() -> Sim {
    let mut sim = sim_with_shop();
    sim.force_tile(4, 1, 2);
    sim
}

/// Коридор пошире: три мастерских — (2,1), (3,1), (4,1), — склад в (6,1) и трое
/// котов. Мир под §12.96: один заказ обязан занять все три станка, а не один.
fn sim_with_three_shops() -> Sim {
    let mut sim = sim_from(&["###########", "#a.....b.c#", "###########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(6, 1, 1);
    sim.set_shop(2, true);
    for x in 2..=4 {
        sim.force_tile(x, 1, 2);
    }
    sim
}

// --- работа над заказом -----------------------------------------------------

#[test]
fn an_order_is_taken_to_the_shop_and_worked_on() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(200, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(recipe, 1));
    sim.tick_n(10); // ходка за материалом плюс дорога к станку (§12.102)
    assert_eq!(
        sim.crafter(),
        Some("b".to_string()),
        "кто привёз, тот и делает"
    );
    assert_eq!(sim.pos_of("b"), (3, 1), "мастер в мастерской");
    assert!(sim.craft_progress().is_some_and(|p| p > 0), "и работает");
}

/// Готовое ложится **кучей под ноги**, а не на склад: работа кончается там, где
/// стоял работник (§12.11, §12.22). Дальше его разносит обычная уборка.
#[test]
fn a_finished_item_lands_under_the_crafter() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false); // иначе готовое тут же уедет на склад
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);

    sim.tick_n(30);
    assert_eq!(sim.item_at(3, 1, PART), 1, "деталь лежит в мастерской");
    assert_eq!(sim.craft_left(), None, "а заказ закрыт");
}

/// **Повторяемость** — то единственное, чего нет у темы (§12.30): один заказ
/// даёт столько штук, сколько заказали, и мастера для этого не переназначают.
#[test]
fn an_order_repeats_until_the_count_is_done() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 3);

    sim.tick_n(20);
    assert_eq!(sim.craft_left(), Some(2), "первая штука готова, заказ жив");
    sim.tick_n(80);
    assert_eq!(sim.craft_left(), None, "и все три сделаны");
    assert_eq!(sim.item_at(3, 1, PART), 3);
}

/// Материал везут **на всю партию сразу**, а съедают **по штуке** (§12.102).
///
/// До §12.102 склад списывался за штуку и в момент, когда за неё брались.
/// Теперь платы нет вовсе: носильщики свозят на станок цену всех оставшихся
/// штук, и мастер вычитает из привезённого ровно одну цену, доделав деталь.
/// Иначе станок вставал бы после каждой штуки и ждал ходку.
#[test]
fn the_batch_is_hauled_at_once_and_eaten_piece_by_piece() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(400, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 2);

    sim.tick_n(20);
    assert_eq!(
        sim.item_at(5, 1, SCRAP),
        2,
        "со склада уехала цена обеих штук, а не одной",
    );
    assert_eq!(sim.craft_delivered(), Some(8), "и лежит она на станке");

    // Первая штука готова — из привезённого ушла ровно одна цена.
    sim.tick_n(40);
    assert_eq!(sim.craft_left(), Some(1), "первая готова");
    assert_eq!(sim.craft_delivered(), Some(4), "на вторую материал уже тут");
}

/// Заказ без материала **ждёт**, как чертёж без завезённого лома (§12.15): это
/// не ошибка и не отказ, и кот тем временем занят другой работой.
///
/// Заводит его здесь **правило-порог**: с §12.129 руками такой заказ не
/// разметить — заявка режется наличным, — а правилу это как раз и поручено
/// («дозакажи, когда появится»). Сама механика ожидания от этого не изменилась.
#[test]
fn an_order_without_material_waits() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(2);
    assert_eq!(sim.craft_left(), Some(1), "правило завело заказ без склада");

    sim.tick_n(10);
    assert_eq!(sim.crafter(), None, "исполнителя нет — заказ ждёт");
    assert_eq!(sim.craft_progress(), Some(0));

    sim.put_item(5, 1, SCRAP, 4);
    sim.tick_n(10);
    assert!(
        sim.crafter().is_some(),
        "материал появился — мастер нашёлся"
    );
}

/// Материала на базе хватило не на всю партию — мастер уходит **после** той
/// штуки, на которую хватило, а не стоит у верстака (§12.30, §12.102).
///
/// До §12.102 материал отбирали «из-под носа» со склада, пока мастер шёл;
/// теперь отобрать нечего — привезённое лежит на станке. Зато появился случай
/// честнее: заказ на две штуки при материале на одну. Первую делают, вторую
/// ждут, и ждёт её заказ, а не занятый кот.
///
/// Партию на две штуки при материале на одну заводит **правило-порог**: руками
/// такую с §12.129 не разметить, заявку срежет наличное. Проверяется здесь не
/// она, а то, что делает мастер, когда привезённое кончилось посреди партии.
#[test]
fn a_crafter_is_released_when_the_batch_runs_short() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 4); // цена ровно одной штуки
    let recipe = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 5);
    sim.tick_n(2);
    assert_eq!(sim.craft_left(), Some(5), "правило просит партию");

    sim.tick_n(40);
    assert_eq!(sim.craft_left(), Some(4), "на что хватило — то и сделали");
    assert_eq!(sim.craft_delivered(), Some(0), "материал кончился");
    assert_eq!(sim.crafter(), None, "заказ отпустил кота");
    assert!(!sim.is_crafting("a"), "и кот свободен для другой работы");
}

// --- навык ------------------------------------------------------------------

/// Производство — такая же работа, как стройка: навык растёт от неё самой.
#[test]
fn crafting_grows_the_craft_skill() {
    let mut sim = sim_with_shop();
    let craft = sim.set_skill("craft", &[100, 400]);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(40);

    assert!(sim.xp_of("b", craft) > 0, "навык растёт от работы");
}

/// Навык — множитель скорости и здесь; допуска у производства нет (§12.30).
#[test]
fn a_higher_level_crafts_faster() {
    let mut sim = sim_with_shop();
    let craft = sim.set_skill("craft", &[100, 400]);
    // Опыт даём `b`: с §12.102 за станок садится тот, кто привёз материал.
    sim.set_xp("b", craft, 400); // второй уровень
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(20);
    let fast = sim.craft_progress().unwrap();

    let mut sim = sim_with_shop();
    sim.set_skill("craft", &[100, 400]);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(20);
    assert!(fast > sim.craft_progress().unwrap(), "уровень ускоряет");
}

/// Новичок берётся за любой рецепт: «Ремесло» не отсекает исполнителя, в
/// отличие от «Науки» (§12.18, §12.30).
#[test]
fn crafting_has_no_skill_gate() {
    let mut sim = sim_with_shop();
    sim.set_skill("craft", &[100, 400]);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    // Работы тут на 100 очков, то есть на десять тиков (`WORK_RATE`): спросить
    // надо, пока заказ ещё жив.
    sim.tick_n(10);

    assert_eq!(sim.crafter(), Some("b".to_string()), "взялся без навыка");
}

// --- ворота и отказы --------------------------------------------------------

/// Рецепт открывает технология — те же ворота, что у тайла (§12.27).
#[test]
fn a_recipe_needs_its_tech() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &["materials"]);

    assert!(!sim.start_craft(recipe, 1), "без технологии рецепта нет");
    sim.set_tech("materials");
    assert!(sim.start_craft(recipe, 1), "с ней — есть");
}

#[test]
fn crafting_without_a_shop_is_refused() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(!sim.start_craft(recipe, 1), "работать негде");
}

/// Заказов столько, сколько мастерских: станок — это ячейка, и второму заказу
/// в неё не встать (§12.55, §12.96). Рецепт при этом чужой — своему заказу
/// заявка добавила бы штук, см. `a_repeat_order_of_the_same_recipe_adds_count`.
#[test]
fn orders_are_capped_by_the_number_of_shops() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 20);
    let first = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let second = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(first, 1));
    assert!(
        !sim.start_craft(second, 1),
        "мастерская одна — второму заказу негде встать"
    );
}

/// **Когда свободных ячеек не осталось**, повторная заявка добавляет штук к
/// своему заказу, а не отказывает (§12.96). Мастерская здесь одна, и это ровно
/// то поведение, что было до §12.96: на маленькой базе ничего не изменилось.
#[test]
fn a_repeat_order_of_the_same_recipe_adds_count() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(recipe, 2));
    assert!(sim.start_craft(recipe, 3), "тот же рецепт — не отказ");
    assert_eq!(sim.orders_count(), 1, "заказ по-прежнему один");
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(5),
        "а штук в нём стало больше"
    );
}

/// Вторая мастерская — вторая пара лап у дела: два заказа идут одновременно, и
/// каждый за своим станком (§12.55). До этого вторая мастерская была
/// декорацией, как бездонный склад до `capacity` (§12.16).
#[test]
fn two_shops_run_two_orders_at_once() {
    let mut sim = sim_with_two_shops();
    sim.put_item(5, 1, SCRAP, 40);
    // Работа длинная нарочно: с §12.102 сперва идут две ходки за материалом, и
    // на коротком рецепте первый заказ успевал закрыться раньше, чем второй
    // мастер садился за станок, — тест мерил бы очередь вместо параллельности.
    let bolt = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(bolt, 1));
    assert!(
        sim.start_craft(nut, 1),
        "второй станок принимает второй заказ"
    );
    sim.tick_n(15); // обе ходки за материалом плюс посадка за станки

    assert!(sim.crafter_at(3, 1).is_some(), "первый заказ взят");
    assert!(sim.crafter_at(4, 1).is_some(), "и второй тоже");
    assert_ne!(
        sim.crafter_at(3, 1),
        sim.crafter_at(4, 1),
        "и берут их разные коты: `commands` отложены, и без списков один и тот \
         же кот достался бы обоим"
    );
}

/// Два заказа никогда не садятся в одну ячейку: с §12.96 её держит сам
/// `Craft::cell`, как сделка держит ячейку поста, — и держит с самой заявки, а
/// не с той минуты, когда нашёлся мастер.
#[test]
fn two_orders_never_share_a_shop() {
    let mut sim = sim_with_two_shops();
    sim.put_item(5, 1, SCRAP, 40);
    let bolt = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(bolt, 1);
    sim.start_craft(nut, 1);

    // На каждом тике, а не только в конце: пересечение длится один тик и
    // конечным состоянием замаскировалось бы (ср. `demolish_job_is_done_...`).
    for _ in 0..40 {
        sim.tick_n(1);
        let (a, b) = (sim.craft_cells_of(bolt), sim.craft_cells_of(nut));
        assert!(
            a.iter().all(|c| !b.contains(c)),
            "два заказа за одним верстаком: {a:?} и {b:?}"
        );
    }
}

// --- ячейка станка (§12.96) -------------------------------------------------

/// **Один рецепт занимает все свободные станки.** Та самая жалоба, из-за
/// которой заказ и переехал в ячейку: пятнадцать деталей при трёх мастерских
/// делались на одной, потому что заказ был уникален по рецепту и держал одного
/// кота (§12.55).
#[test]
fn one_recipe_fills_every_shop() {
    let mut sim = sim_with_three_shops();
    sim.put_item(6, 1, SCRAP, 60);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(recipe, 5));
    assert!(sim.start_craft(recipe, 5), "второй клик — второй станок");
    assert!(sim.start_craft(recipe, 5), "третий — третий");
    assert_eq!(sim.orders_count(), 3, "три заказа по одному рецепту");
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(15),
        "и пятнадцать штук всего"
    );

    sim.tick_n(60); // трое возят, потом трое работают (§12.102)
    assert_eq!(
        sim.crafters_busy(),
        3,
        "работают трое разом, а не по очереди"
    );
}

/// Ячейку выбирает **ядро**, первую свободную по обходу карты (§12.96), а не
/// игрок мышью: §12.16 держится тем, что разметка не зависит ни от исполнителя,
/// ни от того, какую комнату игрок нашёл курсором.
#[test]
fn an_order_takes_the_first_free_cell_in_map_order() {
    let mut sim = sim_with_three_shops();
    sim.put_item(6, 1, SCRAP, 60);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.start_craft(recipe, 1);

    assert_eq!(
        sim.craft_cells_of(recipe),
        vec![(2, 1), (3, 1)],
        "заказы легли по обходу карты, а не по истории вставок (§11)"
    );
}

/// Ячеек не осталось — **своему** рецепту штуки добавляют, чужому отказывают
/// (§12.96). Первая ветка и есть всё поведение базы с одной мастерской.
#[test]
fn a_full_base_adds_to_its_own_order_and_refuses_a_stranger() {
    let mut sim = sim_with_three_shops();
    sim.put_item(6, 1, SCRAP, 60);
    let bolt = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    for _ in 0..3 {
        assert!(sim.start_craft(bolt, 1));
    }
    assert_eq!(sim.orders_count(), 3, "все три ячейки заняты");

    assert!(sim.start_craft(bolt, 4), "своему заказу штуки добавят");
    assert_eq!(sim.craft_left_of(bolt), Some(7));
    assert_eq!(sim.orders_count(), 3, "и четвёртой ячейки не завели");
    assert!(!sim.start_craft(nut, 1), "а чужому вставать некуда");
}

/// Приказ игрока отбирает у заказа **мастера, но не ячейку** (§12.96): станок
/// принадлежит самому заказу, а не задаче кота. Снятая тут ячейка стоила бы
/// заказу станка вместе с оплаченной штукой.
#[test]
fn an_order_keeps_its_cell_when_its_crafter_is_pulled_away() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(15);
    assert_eq!(sim.crafter_at(3, 1), Some("b".to_string()));

    assert!(sim.set_target("b", 6, 1));
    assert_eq!(sim.crafter_at(3, 1), None, "мастера увели");
    assert_eq!(sim.craft_left_at(3, 1), Some(1), "а заказ остался в ячейке");
}

/// Освободившийся станок достаётся ждавшему заказу: слот отпускается вместе с
/// заказом, отдельного реестра занятости нет (§12.55).
#[test]
fn a_cancelled_order_frees_its_shop_for_the_next() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 20);
    let bolt = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(bolt, 1);
    sim.tick_n(6);
    assert!(!sim.start_craft(nut, 1), "станок занят");

    assert!(sim.cancel_craft(3, 1));
    assert!(
        sim.start_craft(nut, 1),
        "станок освободился вместе с заказом"
    );
    sim.tick_n(30);
    // По готовой детали, а не по исполнителю: заказ на 100 очков успевает
    // закрыться и despawn'иться, и «мастера нет» тут значит «всё сделано».
    assert_eq!(
        sim.item_total(PART),
        1,
        "второй заказ встал за станок и сделал деталь"
    );
}

#[test]
fn an_empty_order_is_refused() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[], &[(PART, 1)], &[]);
    assert!(!sim.start_craft(recipe, 0), "ноль штук — это не заказ");
}

// --- занятость мастера ------------------------------------------------------

/// Мастер занят: другая работа его не перехватывает (инвариант занятости).
#[test]
fn a_crafter_is_not_taken_by_other_work() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(15);

    // Чертёж вплотную к станку: строитель работает с соседней клетки, а
    // соседняя у мастерской — это (4,1) или сама (3,1). Встать на станок он
    // теперь не может (§12.103), так что тест меряет занятость, а не толкотню.
    sim.add_blueprint(4, 1, 3);
    sim.tick_n(10);
    assert!(sim.is_crafting("b"), "мастер остался у верстака");
    assert!(!sim.has_assignment("b"), "и стройку не взял");
}

/// Приказ игрока весомее заказа — и обязан освободить его явно, иначе
/// мастерская навсегда останется за ушедшим котом (§12.15).
#[test]
fn an_order_frees_the_craft() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(15);
    assert_eq!(sim.crafter(), Some("b".to_string()));

    assert!(sim.set_target("b", 6, 1));
    assert!(!sim.is_crafting("b"), "кот снят с заказа");
    assert_eq!(sim.crafter(), None, "а заказ свободен для другого мастера");
}

/// Истощение освобождает заказ так же, как тему (§12.20).
#[test]
fn an_exhausted_crafter_frees_the_order() {
    let mut sim = sim_with_shop();
    sim.set_needs(100, 10, 1);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);

    sim.set_energy("a", 0);
    sim.tick_n(1);
    assert!(sim.is_resting("a"), "свалился");
    assert!(!sim.is_crafting("a"), "и отпустил заказ");
    // Заказ при этом не потерян: его тем же тиком подхватил свободный сосед —
    // мастерская освободилась, а не осталась за спящим (§12.20).
    assert_ne!(sim.crafter(), Some("a".to_string()));
}

/// Оплаченная штука при этом не пропадает: `paid` живёт у заказа, а не у кота.
#[test]
fn a_paid_item_is_not_paid_twice() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 4);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(10);
    assert_eq!(sim.item_at(5, 1, SCRAP), 0, "материал списан");

    assert!(sim.set_target("a", 6, 1)); // мастера увели
    sim.tick_n(20);
    assert!(sim.craft_left().is_some(), "заказ жив");
    assert!(sim.crafter().is_some(), "и его взял мастер снова");
    sim.tick_n(100);
    assert_eq!(sim.item_total(PART), 1, "штука доделана без второй оплаты");
}

#[test]
fn cancelling_the_order_frees_the_cat() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);

    assert!(sim.cancel_craft(3, 1));
    assert_eq!(sim.craft_left(), None, "заказа нет");
    assert!(!sim.is_crafting("a"), "и кот свободен");
}

/// **Снесённый станок уносит свой заказ** (§12.96) — как снесённая рация уносит
/// правило автовылазки (§12.67). До §12.96 заказ искал себе другую комнату и
/// ждал; теперь ячейка и есть заказ, а другой ячейки у него нет.
///
/// **А вот материал уносит не станок, а пол** (§12.102): завезённое падает
/// кучей на месте, как возвращает сданное отменённая площадка (§12.31).
/// Материал не исчезает никогда — это инвариант 8, и до §12.102 производство
/// было единственным местом, где он молча сгорал.
#[test]
fn a_demolished_shop_drops_what_was_hauled_to_it() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false); // иначе куча тут же уедет обратно на склад
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(15);
    assert_eq!(sim.pos_of("b"), (3, 1));
    assert_eq!(sim.craft_delivered(), Some(2), "материал на станке");

    sim.force_tile(3, 1, 0); // мастерской больше нет
    sim.tick_n(3);
    assert_eq!(sim.craft_left(), None, "заказ ушёл вместе со станком");
    assert!(!sim.is_crafting("b"), "и мастер свободен");
    assert_eq!(sim.item_at(3, 1, SCRAP), 2, "а материал лёг кучей на месте");
}

// --- правило-порог (§12.65) -------------------------------------------------

/// Правило **набивает все свободные станки** — по одному заказу за тик
/// (§12.97). Один заказ на всё занял бы одну ячейку, и три мастерских работали
/// бы как одна: ровно та жалоба, из-за которой заказ и переехал в ячейку
/// (§12.96), а потом всё равно сползся в первую.
#[test]
fn a_threshold_spreads_its_shortfall_across_free_shops() {
    let mut sim = sim_with_three_shops();
    sim.set_auto_tidy(false);
    sim.put_item(6, 1, SCRAP, 60);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.set_stock(recipe, 15));
    sim.tick_n(3);
    assert_eq!(sim.orders_count(), 3, "недостача легла на все три станка");
    assert_eq!(sim.craft_left_at(2, 1), Some(5), "и порциями: пять…");
    assert_eq!(sim.craft_left_at(3, 1), Some(5), "…пять…");
    assert_eq!(sim.craft_left_at(4, 1), Some(5), "…и пять");
}

/// **Правило заказывает пятёрками** — тем же размером, что Shift-клик по кнопке
/// рецепта (§12.96, §12.97): оно заменяет повторный клик игрока, а не заводит
/// свою арифметику.
#[test]
fn a_threshold_orders_five_at_a_time() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 100);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(5), "порция — пять");
}

/// А на хвосте — по штуке: пятёрка перелетела бы порог, а лишнее правило не
/// штампует (§12.97). Это второй размер той же кнопки — обычный клик.
#[test]
fn a_threshold_orders_one_when_five_would_overshoot() {
    let mut sim = sim_with_three_shops();
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 7);
    sim.tick_n(3);
    assert_eq!(sim.craft_left_at(2, 1), Some(5), "первая порция — пять");
    assert_eq!(sim.craft_left_at(3, 1), Some(1), "а дальше по штуке…");
    assert_eq!(sim.craft_left_at(4, 1), Some(1), "…до самого порога");
    assert_eq!(sim.craft_left_of(recipe), Some(7), "ровно недостача");
}

/// **Уже заказанное идёт в счёт порога** (§12.97): `left` убывает только когда
/// деталь легла под ноги, и не считать очередь значило бы выдать по заказу на
/// каждый свободный станок при недостаче в одну штуку.
#[test]
fn a_threshold_counts_pieces_already_ordered() {
    let mut sim = sim_with_three_shops();
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 1);
    sim.tick_n(10);
    assert_eq!(sim.orders_count(), 1, "станков три, а заказ один");
    assert_eq!(sim.craft_left_of(recipe), Some(1), "и в нём одна штука");
}

/// **Сползшийся в одну ячейку заказ правило разносит по новым станкам** — та
/// самая партия, из-за которой §12.97 и писалась: один станок, потом ещё два, а
/// недостача так и осталась в первой ячейке.
#[test]
fn a_concentrated_order_is_spread_over_the_new_shops() {
    let mut sim = sim_with_three_shops();
    sim.force_tile(3, 1, 0); // сперва мастерская одна: (2,1)
    sim.force_tile(4, 1, 0);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 15);
    sim.tick_n(3);
    assert_eq!(sim.orders_count(), 1, "станок один — и заказ один");

    sim.force_tile(3, 1, 2); // достроили ещё две мастерских
    sim.force_tile(4, 1, 2);
    sim.tick_n(2);
    assert_eq!(
        sim.craft_cells_of(recipe).len(),
        3,
        "правило заняло все три"
    );
    assert_eq!(sim.craft_left_of(recipe), Some(15), "и это вся недостача");
}

/// **Сперва рецепты, у которых станка нет вовсе** (§12.97). Иначе первый по
/// палитре рецепт с вечной недостачей забирал бы каждый освободившийся станок, и
/// порог на пайке не сработал бы никогда — на базе игрока порогов четыре, а
/// станков три.
#[test]
fn a_small_threshold_still_gets_a_shop() {
    let mut sim = sim_with_two_shops();
    let bolt = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(bolt, 100); // недостача на сотню
    sim.set_stock(nut, 1); //    и недостача в штуку
    sim.tick_n(4);
    assert_eq!(sim.craft_left_of(bolt), Some(5), "жадному — одна порция");
    assert_eq!(sim.craft_left_of(nut), Some(1), "а скромному — станок");
}

/// Штуки, заказанные игроком, **идут в счёт порога** (§12.97): это тот же будущий
/// запас, и не зачесть их значило бы наштамповать вдвое. До §12.97 правило
/// пропускало такой рецепт целиком (§12.64) — довод там был в срезании, а
/// срезать оно умеет только своё.
#[test]
fn manual_pieces_count_towards_the_threshold() {
    let mut sim = sim_with_two_shops();
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.put_item(5, 1, SCRAP, 10); // на пять штук: заявка режется наличным (§12.129)

    assert!(sim.start_craft(recipe, 5));
    sim.set_stock(recipe, 5);
    sim.tick_n(5);
    assert_eq!(sim.orders_count(), 1, "порог закрыт заказом игрока");
    assert_eq!(sim.craft_is_auto_at(3, 1), Some(false), "и заказ ручной");

    sim.set_stock(recipe, 7);
    sim.tick_n(3);
    assert_eq!(
        sim.orders_count(),
        2,
        "а недостачу сверх него правило добрало"
    );
    assert_eq!(
        sim.craft_left_at(4, 1),
        Some(1),
        "порцией, а не всей семёркой"
    );
    assert_eq!(sim.craft_is_auto_at(3, 1), Some(false), "чужого не трогая");
}

/// Порог — это **правило**, а не заказ (§12.64): игрок задал число один раз, а
/// заказ заводит система, когда запас просел.
#[test]
fn a_threshold_orders_what_the_base_is_missing() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.set_stock(recipe, 2));
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(1),
        "заказ на порцию (§12.97)"
    );
    assert_eq!(sim.craft_is_auto(recipe), Some(true), "и ведёт его правило");

    sim.tick_n(60);
    assert_eq!(
        sim.item_total(PART),
        2,
        "деталей ровно столько, сколько велено"
    );
    assert_eq!(sim.craft_left_of(recipe), None, "и заказ закрылся сам");
}

/// Порог держится, а не срабатывает один раз: потратил запас — правило завело
/// заказ снова, без второго клика.
#[test]
fn a_threshold_reorders_after_the_stock_is_spent() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(40);
    assert_eq!(sim.item_total(PART), 1);

    sim.take_item(3, 1, PART); // деталь ушла в дело
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(1),
        "просело — правило заказало снова"
    );
}

/// **Считается всё добро базы, а не склад** (§12.65): готовое лежит под ногами
/// мастера, и склад узнаёт о нём только после уборки. По складу правило
/// штамповало бы лишнее всё время, пока носильщик идёт.
#[test]
fn a_threshold_counts_goods_on_the_floor() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 20);
    sim.put_item(1, 1, PART, 3); // лежат на полу, а не в складе
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 3);
    sim.tick_n(5);
    assert_eq!(sim.craft_left_of(recipe), None, "порог уже закрыт полом");
}

/// Обещанное покупателю базе больше не принадлежит (§12.50), поэтому порога не
/// закрывает: иначе бронь под продажу тихо съедала бы запас.
#[test]
fn goods_promised_to_a_buyer_do_not_count() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.set_gate(3, true);
    sim.force_tile(1, 1, 3); // шлюз, через который поедет продажа
    sim.set_trade_post(4, true);
    sim.force_tile(6, 1, 4);
    let faction = sim.set_faction(100);
    sim.set_market(faction, 1, 0, 0, 0);
    sim.set_prices(faction, PART, &[10]);
    sim.put_item(5, 1, SCRAP, 20);
    sim.put_item(5, 1, PART, 2);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.trade(faction, PART, 2, false), "обе детали проданы");
    sim.set_stock(recipe, 2);
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(1),
        "проданное не в счёт — правило заказало взамен"
    );
}

/// Станок — слот заказа (§12.55), и правилу он тоже не бесплатен: занят —
/// правило молчит и ждёт, как ждёт кнопка у игрока.
#[test]
fn a_threshold_waits_for_a_free_shop() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 20);
    let bolt = sim.set_recipe(2000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(bolt, 1); // единственный станок занят вручную

    sim.set_stock(nut, 2);
    sim.tick_n(5);
    assert_eq!(sim.craft_left_of(nut), None, "второму заказу негде встать");

    assert!(sim.cancel_craft(3, 1));
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(nut),
        Some(1),
        "станок освободился — заказ"
    );
}

/// **Приказ игрока удаляет приказ автопроизводства** (§12.97): свободного
/// станка нет — клик забирает ячейку у неоплаченного заказа правила, а не
/// доращивает его. Заказ игрока — его заказ, а не чужой, доросший до нужного
/// числа.
#[test]
fn a_manual_order_takes_a_shop_from_the_rule() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(1);
    assert_eq!(sim.craft_is_auto(recipe), Some(true));

    assert!(sim.start_craft(recipe, 4), "станок отобран у правила");
    sim.tick_n(1);
    assert_eq!(sim.orders_count(), 1, "заказ по-прежнему один");
    assert_eq!(sim.craft_is_auto(recipe), Some(false), "и он ручной");
    assert_eq!(sim.craft_left_of(recipe), Some(4), "ровно на заказанное");
}

/// Вытесняется **только неоплаченный** заказ правила: за начатую штуку материал
/// уже списан, и отобрать её значило бы сжечь его молча (§12.26, §12.97). Такой
/// заказ клик переворачивает в ручной — штука доделается, а вести его дальше
/// будет игрок.
#[test]
fn a_manual_order_never_takes_a_paid_shop() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(400, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(10);
    assert_eq!(sim.item_at(5, 1, SCRAP), 36, "штука оплачена");

    assert!(sim.start_craft(recipe, 4));
    assert_eq!(sim.orders_count(), 1, "заказ тот же");
    assert_eq!(sim.craft_left_of(recipe), Some(5), "штуки добавлены к нему");
    assert_eq!(sim.craft_is_auto(recipe), Some(false), "и он стал ручным");
}

/// Заказ правила отменяют **снятием порога**, а не кнопкой «Отменить»: правило
/// завело бы его обратно тем же тиком, и отмена оказалась бы командой, которая
/// ничего не делает (§12.65).
#[test]
fn an_auto_order_is_not_cancelled_by_hand() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 2);
    sim.tick_n(1);

    assert!(!sim.cancel_craft(3, 1), "отмена не про заказ правила");
    assert!(sim.craft_left_of(recipe).is_some(), "заказ на месте");
}

/// Снятый порог убирает свой заказ **сам** — правило пересчитывается каждым
/// тиком, и руками мир не трогает никто (§12.64).
#[test]
fn clearing_the_threshold_ends_its_order() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 2); // материала нет: заказ ждёт и не оплачен
    sim.tick_n(2);
    assert!(sim.craft_left_of(recipe).is_some());

    assert!(sim.set_stock(recipe, 0));
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        None,
        "заказ ушёл вместе с порогом"
    );
}

/// **Снабжённую штуку правило отбирает тоже** (§12.123). До §12.102 материал за
/// неё был списан со складa, и отмена сожгла бы его молча (§12.26); с тех пор он
/// лежит на станке и при отмене возвращается кучей на его клетку — то есть цена
/// отмены это ходка носильщика, а не потеря.
#[test]
fn a_supplied_piece_is_cancelled_with_the_threshold() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 4);
    let recipe = sim.set_recipe(400, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(10);
    assert_eq!(sim.craft_delivered(), Some(4), "материал завезли на станок");

    sim.set_stock(recipe, 0);
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        None,
        "заказ ушёл вместе с порогом"
    );
    assert_eq!(sim.item_total(SCRAP), 4, "а материал вернулся, весь");
    assert_eq!(sim.item_total(PART), 0, "деталь не начата");
}

/// Поднятый порог **не доращивает бегущий заказ, а кладёт следующую порцию на
/// свободный станок** (§12.97). Станок один и занят — правило молчит и ждёт, как
/// ждёт кнопка у игрока; порция при этом не растёт, и накопиться в одной ячейке
/// недостаче больше негде.
#[test]
fn a_raised_threshold_waits_for_a_free_shop() {
    let mut sim = sim_with_two_shops();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(1));
    assert_eq!(sim.orders_count(), 1);

    sim.set_stock(recipe, 4);
    sim.tick_n(1);
    assert_eq!(sim.orders_count(), 2, "порция легла на второй станок");
    assert_eq!(sim.craft_left_of(recipe), Some(2), "по штуке в каждом");

    sim.tick_n(3);
    assert_eq!(sim.orders_count(), 2, "станков больше нет — правило ждёт");
    assert_eq!(sim.craft_left_of(recipe), Some(2), "и порции не растут");
}

/// Рецепт, закрытый технологией, не существует (§12.27) — порог на нём ждёт
/// науки, как ждал бы кнопки.
#[test]
fn a_locked_recipe_ignores_its_threshold() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &["materials"]);

    sim.set_stock(recipe, 2);
    sim.tick_n(5);
    assert_eq!(
        sim.craft_left_of(recipe),
        None,
        "технологии нет — заказа нет"
    );

    sim.set_tech("materials");
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(1),
        "а с ней правило работает"
    );
}

/// Порог выдаёт **штуки заказа**, а не штуки предмета: рецепт, дающий по три за
/// раз, закрывает недостачу в пять двумя заходами, а не пятью.
#[test]
fn a_threshold_counts_pieces_not_items() {
    // Три станка, чтобы оба захода легли сразу: правило кладёт по одному заказу
    // за тик (§12.97), и на одной мастерской второй ждал бы первого.
    let mut sim = sim_with_three_shops();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 3)], &[]);

    sim.set_stock(recipe, 5);
    sim.tick_n(3);
    assert_eq!(sim.craft_left_of(recipe), Some(2), "два захода по три");
}

// --- боевой рулсет ----------------------------------------------------------

/// На настоящем `core.yaml`: мастерская и первый рецепт открываются одной
/// технологией, а сделанная деталь ложится на пол и уезжает на склад. Ловит
/// контент, где рецепт ссылается на предмет не тем `id`, мастерскую забыли
/// пометить `shop` или закрыли технологией, до которой рецепту не дожить.
#[test]
fn the_shipped_ruleset_makes_a_part_from_scrap() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // караван приносит своё — здесь считаем сделанное
    let part = 1; // индекс `part` в палитре предметов
    let shop = 8; // индекс `shop` в палитре тайлов

    assert!(!sim.start_craft(0, 1), "без «Материаловедения» рецепта нет");
    assert!(
        !sim.add_blueprint(10, 7, shop),
        "и мастерскую пока не построить",
    );

    sim.set_tech("workshops");
    assert!(sim.add_blueprint(10, 7, shop), "технология открыла обе");
    sim.tick_n(600); // коты возят материал и строят
    assert_eq!(i32::from(sim.tile(10, 7)), shop, "мастерская готова");

    let before = sim.item_total(part);
    assert!(sim.start_craft(0, 2), "и заказ по силам");
    sim.tick_n(900);
    assert!(
        sim.item_total(part) > before,
        "деталей стало больше: у лома появился выход, а у детали — источник",
    );
}

/// Проданное порог своим не считает — **и пока его несут покупателю, тоже**
/// (§12.50, §12.65).
///
/// Порог меряет добро базы вместе с лапами, а бронь из куч лапы вычитает: возьми
/// он бронь как есть, носильщик засчитался бы дважды, и правило переставало бы
/// дозаказывать ровно в тот момент, когда товар уходит с базы.
#[test]
fn the_stock_rule_does_not_count_goods_on_their_way_to_a_buyer() {
    // Кучу и пост разводим по разным концам коридора: важен как раз тот
    // десяток тиков, что деталь едет в лапах.
    let mut sim = sim_from(&["################", "#a............b#", "################"]);
    sim.set_shop(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_trade_post(2, true);
    sim.force_tile(5, 1, 2);
    sim.set_capacity(3, 100);
    sim.force_tile(12, 1, 3); // склад: продаётся только учтённое (§12.69)
    let f = sim.set_faction(100);
    sim.set_market(f, 100, 40, 25, 0);
    sim.set_prices(f, PART, &[10]);
    let recipe = sim.set_recipe(50, &[(SCRAP, 1)], &[(PART, 1)], &[]);

    sim.put_item(12, 1, PART, 2); // две детали уже лежат на складе
    sim.set_stock(recipe, 2);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), None, "порог закрыт — заказа нет");

    assert!(sim.trade(f, PART, 2, false), "обе детали проданы");
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(1), "взамен заказана порция");

    for _ in 0..200 {
        sim.tick_n(1);
        if sim.carrying_item_of("b").is_some() {
            break;
        }
    }
    assert!(
        sim.carrying_item_of("b").is_some(),
        "деталь понесли к посту"
    );

    sim.tick_n(1); // тик, на котором правило пересчитывается с грузом в лапах
    assert!(sim.carrying_item_of("b").is_some(), "и всё ещё несут");
    assert_eq!(
        sim.craft_left_of(recipe),
        Some(1),
        "заказ на месте: несомое покупателю базе уже не принадлежит",
    );
}

/// Порог производства открывается наукой (§12.93). Имя технологии живёт в
/// рулсете, поэтому в схеме ворот нет вовсе — здесь их включаем руками.
#[test]
fn a_stock_threshold_needs_its_technology() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_auto_gates("", "planning", "");

    assert!(
        !sim.set_stock(recipe, 2),
        "технологии нет — порога не будет"
    );
    assert_eq!(sim.stock_min(recipe), 0);

    sim.set_tech("planning");
    assert!(sim.set_stock(recipe, 2), "изучили — можно");
    assert_eq!(sim.stock_min(recipe), 2);

    // Снятие ворот не спрашивает: запертая отмена оставила бы порог навсегда.
    sim.forget_techs();
    assert!(sim.set_stock(recipe, 0), "снять можно всегда");
    assert_eq!(sim.stock_min(recipe), 0);
}

/// **Отменённый заказ возвращает завезённое** — кучей на клетку станка (§12.102).
///
/// До §12.102 возвращать было нечего: материал списывался со склада мгновенно, и
/// отмена его молча сжигала. Теперь это тот же случай, что отмена площадки
/// (§12.31), и подчиняется он инварианту 8: материал не исчезает никогда.
#[test]
fn a_cancelled_order_drops_what_was_hauled_to_it() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false); // иначе куча тут же уедет обратно на склад
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(15);
    assert_eq!(sim.craft_delivered(), Some(2), "материал на станке");

    assert!(sim.cancel_craft(3, 1));
    assert_eq!(sim.craft_left(), None, "заказа нет");
    assert_eq!(sim.item_at(3, 1, SCRAP), 2, "а материал лёг кучей на месте");
}

/// **Станок кормит только склад** (§12.130): валяющееся ему не материал, как и
/// торговому посту.
///
/// До §12.130 пол годился и станку: производство считалось стройкой внутри
/// базы, а §12.69 разрешает неучтённому ровно это. Заказ, однако, есть трата
/// запаса, и платить за него надо тем же, чем за найм, науку и аптечку, —
/// иначе «чем можно платить» получает в игре два разных ответа. Валяющееся не
/// пропадает: его увезёт уборка, и заказ станет возможен сам собой.
#[test]
fn a_shop_is_fed_from_storage_only() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false); // иначе лом уедет на склад прежде, чем понадобится
    sim.put_item(2, 1, SCRAP, 4); // куча на голом полу, мимо склада
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(!sim.start_craft(recipe, 1), "валяющимся не заказать");

    // Тот же лом, но на складе, — и заказ проходит, и материал доезжает.
    let store = sim.first_storage_cell();
    sim.put_item(store.0, store.1, SCRAP, 4);
    assert!(sim.start_craft(recipe, 1), "складское — материал");
    sim.tick_n(15);
    assert_eq!(sim.craft_delivered(), Some(2), "привезли со склада");
    assert_eq!(sim.item_at(2, 1, SCRAP), 4, "а валяющееся не тронули");
}

// --- разбор (§12.114) --------------------------------------------------------

/// Разбор — обычный заказ: кот везёт на станок то, что разбирают, и кладёт под
/// ноги то, что вышло. Своей механики у него нет и заводить её незачем — §12.30
/// уже описал заказом всё, что здесь происходит.
#[test]
fn salvage_is_an_ordinary_order() {
    let mut sim = sim_with_shop();
    let cloth = 2;
    sim.set_items(cloth + 1);
    let def = sim.set_recipe(100, &[(PART, 1)], &[(cloth, 2)], &[]);
    sim.set_salvage(def);
    sim.put_item(5, 1, PART, 1);

    assert!(sim.start_craft(def, 1), "заказ принят");
    sim.tick_n(120);
    assert_eq!(sim.item_total(PART), 0, "трофей разобран без остатка");
    assert_eq!(sim.item_total(cloth), 2, "и стал двумя единицами сырья");
}

/// **Порога у разбора нет** (§12.114): правило «держать ткани до N» рвало бы
/// надетые комбинезоны без спроса, а решение необратимо (§12.44). Вид такой
/// полоски не рисует, но отказывает фасад — команда приезжает и из трейса.
#[test]
fn a_salvage_recipe_takes_no_threshold() {
    let mut sim = sim_with_shop();
    let cloth = 2;
    sim.set_items(cloth + 1);
    let def = sim.set_recipe(100, &[(PART, 1)], &[(cloth, 2)], &[]);
    sim.set_salvage(def);

    assert!(!sim.set_stock(def, 5), "порог на разбор не ставится");
    sim.put_item(5, 1, PART, 3);
    sim.tick_n(200);
    assert_eq!(sim.item_total(cloth), 0, "и сам собой разбор не идёт");

    // А обычному рецепту тот же порог по-прежнему по силам: отказ адресный.
    let plain = sim.set_recipe(100, &[(PART, 1)], &[(cloth, 2)], &[]);
    assert!(sim.set_stock(plain, 5), "у обычного рецепта порог есть");
}

/// Сторож на боевой рулсет: **цикл рецептов не должен быть прибыльным**
/// (§12.114). Разобрал и сшил обратно — обязан остаться в убытке хотя бы по
/// одному предмету, иначе мастерская печатает комбинезоны, а вылазка — их
/// единственный вход в мир — обесценивается. Тот же довод и тот же перебор,
/// что у `the_shipped_ruleset_leaves_no_arbitrage` на рынке.
#[test]
fn the_shipped_ruleset_never_profits_from_salvage() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let recipes = sim.recipe_flows();

    for (a_id, a_cost, a_gives) in &recipes {
        for (b_id, b_cost, b_gives) in &recipes {
            if a_id == b_id {
                continue;
            }
            // Замкнутая пара: выход A целиком идёт в цену B и наоборот. Только
            // такие и образуют круг, который можно крутить бесконечно.
            let closed = |gives: &Vec<(String, i32)>, cost: &Vec<(String, i32)>| {
                gives
                    .iter()
                    .all(|(it, _)| cost.iter().any(|(c, _)| c == it))
            };
            if !closed(a_gives, b_cost) || !closed(b_gives, a_cost) {
                continue;
            }
            // Сколько раз надо повторить A, чтобы хватило на один прогон B, — и
            // что от круга останется. Убыток обязан быть хотя бы по одному
            // предмету, входящему в цену A.
            let per = |set: &Vec<(String, i32)>, id: &str| {
                set.iter().find(|(i, _)| i == id).map_or(0, |&(_, n)| n)
            };
            let runs = b_cost
                .iter()
                .map(|(it, need)| {
                    let out = per(a_gives, it);
                    if out == 0 {
                        i32::MAX
                    } else {
                        (need + out - 1) / out
                    }
                })
                .max()
                .unwrap_or(1);
            let profits = a_cost
                .iter()
                .all(|(it, spent)| per(b_gives, it) >= spent * runs);
            assert!(
                !profits,
                "круг «{a_id} → {b_id}» возвращает не меньше, чем съел: \
                 мастерская печатает то, за чем ходят в поле",
            );
        }
    }
}

/// Сквозной прогон ветки на боевом рулсете (§12.114, §12.133): трофей едут
/// вскрывать в лабораторию, оттуда выходит ткань, и после второй темы из ткани
/// шьётся свой комбинезон, а трофей становится разбираемым.
///
/// Порядок ворот здесь и есть содержание, и с §12.133 он другой: **узнают о
/// вещи в лаборатории, а не на станке**. «Материаловедение» открывает
/// мастерскую, «Вскрыть комбинезон» даёт ткань, и только «Свойства ткани»
/// делают её понятной — а вместе с ней открывают и пошив, и разбор. Рвать вещь,
/// не понимая, что из неё выйдет, — это мусор, а не производство.
#[test]
fn the_shipped_ruleset_opens_the_cloth_branch_in_order() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // караван приносит своё — здесь считаем добытое
    let name = |id: &str| {
        sim.item_index(id)
            .unwrap_or_else(|| panic!("предмет `{id}`"))
    };
    let (suit, cloth, part) = (name("suit"), name("cloth"), name("part"));
    let salvage = 1; // «Разбор комбинезона» в `recipes:`
    let tailoring = 2; // «Пошив комбинезона»
    let shop = 8; // индекс `shop` в палитре тайлов

    assert!(!sim.start_craft(salvage, 1), "без мастерской рецепта нет");
    sim.set_tech("materials"); // веха: за ней и станок, и вскрытие (§12.146)
    sim.set_tech("workshops");
    assert!(
        sim.add_blueprint(10, 7, shop),
        "технология открыла мастерскую"
    );
    sim.tick_n(600);
    assert_eq!(i32::from(sim.tile(10, 7)), shop, "мастерская готова");

    // Трофей кладём **на склад**: и вскрытие, и разбор берут учтённое (§12.130).
    let store = sim.first_storage_cell();
    sim.put_item(store.0, store.1, suit, 1);
    sim.tick_n(1);
    assert!(
        !sim.start_craft(salvage, 1),
        "разбор закрыт вскрытием: как эта вещь разобрана, база ещё не видела",
    );

    // Вскрытие: тема забирает трофей ногами и возвращает ткань.
    let fabrics = sim.topic_index("fabrics").expect("тема `fabrics`");
    sim.set_skill_level("sp3", "science", 2); // допуск темы: самый головастый
    assert!(
        sim.start_research(fabrics),
        "трофей база видела — тема открыта"
    );
    sim.tick_n(1200);
    assert!(sim.knows_tech("fabrics"), "вскрытие доведено до конца");
    assert_eq!(sim.item_total(suit), 0, "трофей ушёл на опыты");
    assert_eq!(sim.item_total(cloth), 2, "и стал тканью");

    // **Разбор открылся тем же вскрытием, и это главное здесь** (§12.138):
    // отдельной темы «научиться разбирать» не существует — вскрытие и есть тот
    // момент, когда база увидела, как вещь разобрана. Трофей кладём заново:
    // первый ушёл на опыты, а заявка режется наличным (§12.129).
    sim.put_item(store.0, store.1, suit, 1);
    sim.tick_n(1);
    assert!(
        sim.start_craft(salvage, 1),
        "вскрыли — значит и разобрать умеем",
    );
    // Мастерская одна, и заказ занимает её целиком (§12.96), — снимаем, чтобы
    // дальше проверять ворота, а не занятость станка.
    let cell = sim.craft_cells_of(salvage)[0];
    assert!(sim.cancel_craft(cell.0, cell.1));

    // А вот ткань есть, и что с ней делать — ещё неизвестно (§12.131). Это
    // вторая ступень ветки, и она про **материал**, а не про вещь.
    sim.put_item(store.0, store.1, cloth, 3);
    sim.put_item(store.0, store.1, part, 1);
    assert!(
        !sim.start_craft(tailoring, 1),
        "пошив закрыт «Свойствами ткани», и это не «нет материала»",
    );

    let lore = sim.topic_index("cloth_lore").expect("тема `cloth_lore`");
    assert!(sim.start_research(lore), "ткань база видела — тема открыта");
    sim.tick_n(1200);
    assert!(sim.knows_tech("cloth_lore"), "вторая тема доведена");

    assert!(sim.start_craft(tailoring, 1), "изучили ткань — шьём своё");
}

/// **Разобрать можно только то, что есть** (§12.114d, §12.129). Заказ на пять
/// трофеев при одном на базе стал бы правилом «съедать каждый следующий»,
/// которого игрок не ставил и нигде не снимет (§12.114c).
///
/// Считается это **по складу** (§12.114d, §12.130): станок так и снабжается —
/// материал к нему возят ногами и берут только со складских куч. Ворота у обеих
/// кнопок одни, и это то же число, которое игрок видит главным в строке окна.
#[test]
fn salvage_never_orders_more_than_there_is() {
    let mut sim = sim_with_shop();
    let cloth = 2;
    sim.set_items(cloth + 1);
    let def = sim.set_recipe(100, &[(PART, 1)], &[(cloth, 2)], &[]);
    sim.set_salvage(def);

    assert!(!sim.start_craft(def, 1), "пустая база — нечего разбирать");
    sim.put_item(1, 1, PART, 9); // на полу, мимо склада — в счёт не идёт
    assert!(!sim.start_craft(def, 1), "валяющееся станку не материал");
    sim.put_item(5, 1, PART, 2);
    assert!(sim.start_craft(def, 5), "заявку режем, а не отклоняем");
    assert_eq!(sim.craft_left(), Some(2), "ровно по числу трофеев");
    assert!(!sim.start_craft(def, 1), "и второй раз те же не пообещать");
}

/// **Обычный рецепт живёт по тому же правилу** (§12.129). До него нехватка
/// материала была у него законным ожиданием (§12.30): лом придёт сам, с уборки
/// и с вылазок. Но ждущий заказ **держит станок** (§12.96) и **расписывает на
/// себя будущий лом** (§12.128) — то есть молча отнимает у следующего заказа
/// ровно то, чем игрок за него расплатился бы.
#[test]
fn a_plain_order_is_cut_by_material_too() {
    let mut sim = sim_with_shop();
    let def = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);

    assert!(!sim.start_craft(def, 5), "пустая база — заказывать нечем");
    sim.put_item(5, 1, SCRAP, 9);
    assert!(sim.start_craft(def, 5), "заявку режем, а не отклоняем");
    assert_eq!(sim.craft_left(), Some(2), "на девять лома — две штуки");
    assert!(!sim.start_craft(def, 1), "тот же лом второй раз не обещаем");
}

/// А **правило-порог этим воротам не подчиняется** (§12.129): оно как раз и
/// говорит «дозакажи, когда появится», и заказ у него ждёт материала законно
/// (§12.30). Иначе просевший запас чинился бы только вручную и только в тот
/// тик, когда лом уже лежит на базе.
#[test]
fn the_threshold_rule_still_orders_without_material() {
    let mut sim = sim_with_shop();
    let def = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);

    sim.set_stock(def, 3);
    sim.tick_n(2);
    assert!(
        sim.craft_left_of(def).unwrap_or(0) > 0,
        "правило завело заказ"
    );
    assert_eq!(sim.craft_is_auto(def), Some(true), "оно, а не игрок");
}

/// **Надетое разбору не материал** (§12.114d). Считается только то, что лежит
/// кучами, — а комбинезон на коте это `Gear`, а не куча. Иначе заказ на разбор
/// раздевал бы отряд перед вылазкой, и «сила отряда» зависела бы от того, что
/// игрок нажал в окне «Склад» за сотню тиков до кнопки «Отправить».
///
/// Сторож нарочный: сегодня это следует из устройства кода (`salvage_room`
/// смотрит `Stack`), а такие «следует» и отваливаются молча.
#[test]
fn a_worn_suit_is_not_salvage_material() {
    let mut sim = sim_with_shop();
    let suit = 2;
    sim.set_items(suit + 1);
    sim.set_force(suit, 1);
    sim.set_loadout(&[suit]);
    let def = sim.set_recipe(100, &[(suit, 1)], &[(PART, 1)], &[]);
    sim.set_salvage(def);
    sim.put_item(5, 1, suit, 1);

    // Пока трофей лежит на складе, разобрать его можно.
    assert_eq!(sim.item_at(5, 1, suit), 1);

    sim.tick_n(20);
    assert_eq!(sim.gear_of("a"), vec![suit], "кот оделся сам (§12.29)");
    assert_eq!(
        sim.item_total(suit),
        0,
        "и куч с трофеем на базе не осталось"
    );
    assert!(!sim.start_craft(def, 1), "надетое в счёт не идёт");
    assert_eq!(sim.gear_of("a"), vec![suit], "и с кота его никто не снял");

    // Отказ был именно про надетое, а не про рецепт: второй трофей кучей — и
    // заказ проходит, а одетый кот так и остаётся одетым.
    sim.put_item(5, 1, suit, 1);
    assert!(sim.start_craft(def, 1), "лежащий трофей разбирают");
    assert_eq!(sim.craft_left(), Some(1), "ровно один — второй на коте");
    assert_eq!(sim.gear_of("a"), vec![suit]);
}

// --- правило излишка: разбор (§12.115) ---------------------------------------

/// Мир под правило: мастерская, склад и рецепт разбора. Возвращает индекс
/// рецепта; предмет-трофей — `PART`, сырьё — `2`.
fn sim_with_salvage_rule() -> (Sim, usize) {
    let mut sim = sim_with_shop();
    let cloth = 2;
    sim.set_items(cloth + 1);
    let def = sim.set_recipe(100, &[(PART, 1)], &[(cloth, 2)], &[]);
    sim.set_salvage(def);
    (sim, def)
}

/// Излишек сверх порога уходит в разбор сам — тем же заказом, каким его завёл
/// бы игрок (§12.115).
#[test]
fn the_surplus_rule_sends_leftovers_to_the_shop() {
    let (mut sim, def) = sim_with_salvage_rule();
    sim.put_item(5, 1, PART, 5);

    assert!(sim.set_salvage_rule(PART, 2), "правило принято");
    sim.tick();
    assert_eq!(
        sim.craft_left_of(def),
        Some(3),
        "в разбор ушло всё сверх двух"
    );

    sim.tick_n(10);
    assert_eq!(
        sim.craft_left_of(def),
        Some(3),
        "и второй раз те же три не заказаны: обещанное в счёт идёт",
    );
}

/// Порог держит запас, а не сливает склад: ровно по числу трофей остаётся на
/// месте. Зеркало `the_threshold_measures_storage_not_the_floor` у сбыта.
#[test]
fn the_salvage_rule_keeps_what_it_promised() {
    let (mut sim, def) = sim_with_salvage_rule();
    sim.put_item(5, 1, PART, 2);
    assert!(sim.set_salvage_rule(PART, 2));

    sim.tick_n(20);
    assert_eq!(sim.craft_left_of(def), None, "заказывать нечего");
    assert_eq!(sim.item_total(PART), 2, "запас цел");
}

/// **Разбор уступает шаблону снаряжения** (§12.115): комбинезон это ещё и вещь,
/// за которой кот идёт сам, и два автомата на один дефицит оставили бы отряд
/// голым. Придерживается по штуке на каждого неодетого кота — числом, а не
/// запретом: одетой базе правило не мешает ничем.
#[test]
fn the_salvage_rule_yields_to_the_loadout() {
    let (mut sim, def) = sim_with_salvage_rule();
    sim.set_force(PART, 1);
    sim.set_loadout(&[PART]);
    sim.put_item(5, 1, PART, 4); // один по порогу, двое на котов, один лишний
    assert!(sim.set_salvage_rule(PART, 1), "правило принято");

    sim.tick();
    assert_eq!(
        sim.craft_left_of(def),
        Some(1),
        "двое неодетых придержаны, в разбор ушёл только лишний",
    );

    sim.tick_n(60); // коты доходят до склада и одеваются
    assert!(!sim.gear_of("a").is_empty(), "и одеться им было во что");
    assert!(!sim.gear_of("b").is_empty());
}

/// Ворота — технология **производства**, а не сбыта (§12.115): разбор это заказ
/// мастерской. Снятие проходит и без неё, как у всех правил (§12.93).
#[test]
fn a_salvage_rule_needs_the_crafting_technology() {
    let (mut sim, _) = sim_with_salvage_rule();
    sim.set_auto_gates("", "planning", "");

    assert!(!sim.set_salvage_rule(PART, 2), "технологии нет");
    assert_eq!(sim.salvage_rule_of(PART), None);

    sim.set_tech("planning");
    assert!(sim.set_salvage_rule(PART, 2), "изучили — можно");
    assert_eq!(sim.salvage_rule_of(PART), Some(2));
    assert!(sim.set_salvage_rule(PART, 0), "а снять можно всегда");
}

/// Предмет, который никакой рецепт не разбирает, правила не получает — зеркало
/// ворот у сбыта («чем сторона не торгует, на то и порога нет», §12.88).
#[test]
fn a_rule_needs_a_recipe_that_tears_the_item() {
    let (mut sim, _) = sim_with_salvage_rule();
    assert!(!sim.set_salvage_rule(SCRAP, 2), "лом никто не разбирает");
}

/// Порог, снятый в ноль, снимает и **снабжённый** заказ правила (§12.123).
///
/// Просадку порога снабжённая штука переживает (§12.102) — материал за неё уже
/// привезли ногами. Но ноль это не просадка, а отмена: у заказа правила кнопки
/// нет ни в панели, ни у клетки, и оставленная штука держала бы станок, снять
/// который игроку нечем. Реальная партия вставала так шестью заказами на шести
/// станках при пороге в ноль.
#[test]
fn zeroing_a_threshold_cancels_even_a_supplied_order() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 20);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    sim.set_stock(recipe, 1);
    // Ждём, пока материал доедет до станка, но не дольше: доделанная штука
    // закрыла бы заказ сама, и проверять было бы нечего.
    sim.tick_n(10);
    assert_eq!(sim.craft_delivered(), Some(2), "материал на станке");

    // Снятие порога — ноль и есть отмена.
    assert!(sim.set_stock(recipe, 0));
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), None, "заказ снят");
    // Материал не исчез (инвариант 8): он лёг кучей на клетку станка.
    assert_eq!(sim.item_total(SCRAP), 20, "лом на месте, весь");
}

/// Порог, опущенный **ниже уже накопленного**, снимает лишние заказы целиком —
/// включая снабжённые (§12.123). Это тот же срез, что и ноль: правило меряет
/// недостачу, а её нет.
#[test]
fn lowering_a_threshold_below_the_stock_trims_supplied_orders() {
    let mut sim = sim_with_three_shops();
    sim.set_auto_tidy(false);
    sim.put_item(6, 1, SCRAP, 40);
    let recipe = sim.set_recipe(400, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    // Высокий порог занимает все три станка и завозит на них материал.
    sim.set_stock(recipe, 10);
    sim.tick_n(16);
    assert!(sim.orders_count() >= 2, "правило заняло станки");
    assert!(sim.craft_delivered().is_some(), "и материал уже на станке");

    // На базе деталей нет, но и порог опускается ниже накопленного лома:
    // недостачи не осталось, значит и заказам стоять не на чем.
    sim.put_item(6, 1, PART, 3);
    sim.set_stock(recipe, 2);
    sim.tick_n(1);
    assert_eq!(sim.orders_count(), 0, "лишние заказы сняты");
    assert_eq!(sim.item_total(SCRAP), 40, "материал вернулся весь");
}
