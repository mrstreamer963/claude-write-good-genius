//! Производство: заказ → работа в мастерской → предмет под ноги (§12.30).
//!
//! Заказ — разметка работы, как чертёж и как тема, но первый **повторяемый**:
//! рецепт крутится, пока не выйдет заказанное число штук. Проверять надо именно
//! это и оплату по штуке — остальное те же схемы, что у науки.
//!
//! Мир везде один: коридор со складом (тайл 1) и мастерской (тайл 2) — в схеме
//! `sim_from` ни того, ни другого нет, поэтому свойства задаём явно.

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

// --- работа над заказом -----------------------------------------------------

#[test]
fn an_order_is_taken_to_the_shop_and_worked_on() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(200, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(recipe, 1));
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (3, 1), "мастер в мастерской");
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

/// Платят **за штуку**, а не за заказ: склад не замораживается под работу,
/// которая начнётся через полтысячи тиков (§12.30).
#[test]
fn each_item_is_paid_for_separately() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 2);

    sim.tick_n(6);
    assert_eq!(sim.item_at(5, 1, SCRAP), 6, "снято ровно за первую штуку");
    sim.tick_n(24);
    assert_eq!(sim.item_at(5, 1, SCRAP), 2, "и потом ровно за вторую");
}

/// Заказ без материала **ждёт**, как чертёж без завезённого лома (§12.15): это
/// не ошибка и не отказ, и кот тем временем занят другой работой.
#[test]
fn an_order_without_material_waits() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    assert!(sim.start_craft(recipe, 1), "заявку принимают и без склада");

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

/// Материал разобрали, пока мастер шёл, — он уходит на другую работу, а не
/// стоит у верстака (§12.30).
#[test]
fn a_crafter_is_released_when_the_material_is_gone() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 4);
    let recipe = sim.set_recipe(100, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(1);
    assert_eq!(sim.crafter(), Some("a".to_string()), "мастер пошёл");

    // Забираем лом со склада «из-под носа» — так делает найм или наука.
    let pile = sim.item_at(5, 1, SCRAP);
    sim.put_item(5, 1, SCRAP, -pile);
    sim.tick_n(10);
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
    sim.tick_n(20);

    assert!(sim.xp_of("a", craft) > 0, "навык растёт от работы");
}

/// Навык — множитель скорости и здесь; допуска у производства нет (§12.30).
#[test]
fn a_higher_level_crafts_faster() {
    let mut sim = sim_with_shop();
    let craft = sim.set_skill("craft", &[100, 400]);
    sim.set_xp("a", craft, 400); // второй уровень
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);
    let fast = sim.craft_progress().unwrap();

    let mut sim = sim_with_shop();
    sim.set_skill("craft", &[100, 400]);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);
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
    sim.tick_n(10);

    assert_eq!(sim.crafter(), Some("a".to_string()), "взялся без навыка");
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

/// Заказов столько, сколько мастерских: станок — это слот (§12.55).
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

/// Повторная заявка на тот же рецепт **добавляет штук**, а не заводит второй
/// заказ: счётчик у заказа для этого и есть, а два одинаковых заняли бы два
/// станка ради работы, которую один сделает подряд (§12.55).
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
    let bolt = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let nut = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);

    assert!(sim.start_craft(bolt, 1));
    assert!(
        sim.start_craft(nut, 1),
        "второй станок принимает второй заказ"
    );
    sim.tick_n(6);

    assert!(sim.crafter_of(bolt).is_some(), "первый заказ взят");
    assert!(sim.crafter_of(nut).is_some(), "и второй тоже");
    assert_ne!(
        sim.crafter_of(bolt),
        sim.crafter_of(nut),
        "и берут их разные коты: `commands` отложены, и без списков один и тот \
         же кот достался бы обоим"
    );
}

/// Два заказа никогда не садятся за один станок: занятость держит `Craft::spot`,
/// как лежанку держит `Rest::spot` (§12.55, §12.20).
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
        let (a, b) = (sim.craft_spot_of(bolt), sim.craft_spot_of(nut));
        if let (Some(a), Some(b)) = (a, b) {
            assert_ne!(a, b, "два заказа за одним верстаком");
        }
    }
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

    assert!(sim.cancel_craft(bolt));
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
    sim.tick_n(6);

    sim.add_blueprint(4, 1, 3); // чертёж рядом с мастерской
    sim.tick_n(10);
    assert!(sim.is_crafting("a"), "мастер остался у верстака");
    assert!(!sim.has_assignment("a"), "и стройку не взял");
}

/// Приказ игрока весомее заказа — и обязан освободить его явно, иначе
/// мастерская навсегда останется за ушедшим котом (§12.15).
#[test]
fn an_order_frees_the_craft() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);
    assert_eq!(sim.crafter(), Some("a".to_string()));

    assert!(sim.set_target("a", 6, 1));
    assert!(!sim.is_crafting("a"), "кот снят с заказа");
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
    sim.tick_n(6);
    assert_eq!(sim.item_at(5, 1, SCRAP), 0, "материал списан");

    assert!(sim.set_target("a", 6, 1)); // мастера увели
    sim.tick_n(20);
    assert!(sim.craft_left().is_some(), "заказ жив");
    assert!(sim.crafter().is_some(), "и его взял мастер снова");
    sim.tick_n(80);
    assert_eq!(sim.item_total(PART), 1, "штука доделана без второй оплаты");
}

#[test]
fn cancelling_the_order_frees_the_cat() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);

    assert!(sim.cancel_craft(recipe));
    assert_eq!(sim.craft_left(), None, "заказа нет");
    assert!(!sim.is_crafting("a"), "и кот свободен");
}

/// Мастерскую снесли, пока мастер шёл, — заказ отпускает кота и ждёт (§12.26).
#[test]
fn a_demolished_shop_releases_the_order() {
    let mut sim = sim_with_shop();
    sim.put_item(5, 1, SCRAP, 10);
    let recipe = sim.set_recipe(1000, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.start_craft(recipe, 1);
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (3, 1));

    sim.force_tile(3, 1, 0); // мастерской больше нет
    sim.tick_n(3);
    assert_eq!(sim.crafter(), None, "заказ свободен");
    assert!(sim.craft_left().is_some(), "но не потерян");
}

// --- правило-порог (§12.65) -------------------------------------------------

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
    assert_eq!(sim.craft_left_of(recipe), Some(2), "заказ на всю недостачу");
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
        Some(2),
        "проданное не в счёт — правило заказало новые"
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

    assert!(sim.cancel_craft(bolt));
    sim.tick_n(1);
    assert_eq!(
        sim.craft_left_of(nut),
        Some(2),
        "станок освободился — заказ"
    );
}

/// **На одну разметку — один источник** (§12.64): заказ, которого коснулся
/// игрок, становится ручным, и правило его больше не ведёт. Иначе добавленные
/// штуки срезались бы тем же тиком, и объяснить это было бы нечем.
#[test]
fn a_manual_order_takes_the_recipe_over_from_the_rule() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 40);
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(1);
    assert_eq!(sim.craft_is_auto(recipe), Some(true));

    assert!(sim.start_craft(recipe, 4));
    sim.tick_n(1);
    assert_eq!(sim.craft_is_auto(recipe), Some(false), "заказ стал ручным");
    assert_eq!(sim.craft_left_of(recipe), Some(5), "и штуки на месте");
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

    assert!(!sim.cancel_craft(recipe), "отмена не про заказ правила");
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

/// Но **начатую штуку правило не отбирает**: материал за неё уже списан, и
/// отменить её значило бы сжечь его молча (§12.26).
#[test]
fn a_paid_piece_survives_the_threshold_being_dropped() {
    let mut sim = sim_with_shop();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, SCRAP, 4);
    let recipe = sim.set_recipe(400, &[(SCRAP, 4)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(6);
    assert_eq!(sim.item_at(5, 1, SCRAP), 0, "материал списан");

    sim.set_stock(recipe, 0);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(1), "штука доделывается");
    sim.tick_n(60);
    assert_eq!(sim.item_total(PART), 1, "и материал не сгорел зря");
    assert_eq!(sim.craft_left_of(recipe), None, "а дальше заказ закрылся");
}

/// Поднятый порог дотягивает свой заказ, а не заводит второй: двух заказов на
/// один рецепт не бывает (§12.55).
#[test]
fn a_raised_threshold_grows_its_own_order() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    sim.set_stock(recipe, 1);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(1));

    sim.set_stock(recipe, 4);
    sim.tick_n(1);
    assert_eq!(sim.craft_left_of(recipe), Some(4), "заказ подрос");
    assert_eq!(sim.orders_count(), 1, "и остался одним");
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
        Some(2),
        "а с ней правило работает"
    );
}

/// Порог выдаёт **штуки заказа**, а не штуки предмета: рецепт, дающий по три за
/// раз, закрывает недостачу в пять двумя заходами, а не пятью.
#[test]
fn a_threshold_counts_pieces_not_items() {
    let mut sim = sim_with_shop();
    let recipe = sim.set_recipe(100, &[(SCRAP, 2)], &[(PART, 3)], &[]);

    sim.set_stock(recipe, 5);
    sim.tick_n(1);
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

    sim.set_tech("materials");
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
