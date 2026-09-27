//! Снаряжение: приписка на кота, за которой кот идёт сам (§12.29, §12.34,
//! §12.268).
//!
//! Снаряжение — свойство предмета (`force`), а не отдельная сущность, поэтому
//! проверять его надо там, где оно что-то меняет: в силе отряда. Отсюда и мир
//! на все тесты — коридор со складом и шлюзом: одеться и уйти в поле.
//!
//! Одевание — задача с маршрутом (§12.34), поэтому «оделся» здесь всегда стоит
//! тиков: кот доходит до кучи и берёт вещь оттуда. Это и есть главная разница с
//! первой редакцией §12.29, где склад одевал мгновенно.
//!
//! В схеме `sim_from` предметы бессильны, вечны и никому не приписаны, ровно как
//! тайл там бесплатен: включают их тесты сами (`set_force`, `set_wear`,
//! `outfit_all`, `set_outfit`).

use super::*;

/// Предмет-снаряжение в тестах: индекс палитры, у которого есть `force`.
const SUIT: usize = 1;

/// Отряд поимённо — состав выбирает игрок (§12.23).
fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Коридор: склад в (5,1), шлюз в (6,1), комплект из одного «комбинезона».
/// Коты `a` (1,1), `b` (3,1) и `c` (6,1) — в этом порядке их и одевают (по `id`).
fn sim_with_store_and_gate() -> Sim {
    let mut sim = sim_from(&["########", "#a.b..c#", "########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.set_gate(2, true);
    sim.set_relay(2, true);
    sim.force_tile(6, 1, 2);
    sim.set_force(SUIT, 1);
    sim.outfit_all(&[SUIT]);
    sim
}

// --- экипировка -------------------------------------------------------------

/// Комплект коты добирают сами: игрок его не выдаёт, как не выдаёт чертёж
/// конкретному коту (§12.16). Но добирают **ногами** — вещь лежит на складе, и
/// за ней надо дойти (§12.34).
#[test]
fn a_cat_walks_to_the_storage_for_the_loadout() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);

    sim.tick_n(1);
    assert!(sim.is_equipping("a"), "кот взялся за поход");
    assert!(sim.gear_of("a").is_empty(), "но пока ни во что не одет");

    sim.tick_n(10);
    // До соседней клетки: со склада берут с прохода (§12.250).
    assert_eq!(sim.pos_of("a"), (4, 1), "дошёл до склада");
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и надел комбинезон");
    assert_eq!(sim.item_at(5, 1, SUIT), 0, "склад стал легче ровно на него");
    assert!(!sim.is_equipping("a"), "задача закрыта");
}

/// Комбинезон на стеллаже кот берёт **с прохода** (§12.142): дорога та же, а
/// последний шаг не делается — вставать на полку нельзя.
#[test]
fn gear_is_taken_from_a_rack_from_a_neighbour() {
    let mut sim = sim_with_store_and_gate();
    sim.set_solid(1, true); // склад в (5, 1) стал стеллажом
    sim.put_item(5, 1, SUIT, 1);

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (4, 1), "встал рядом с полкой, а не на неё");
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и оделся");
    assert_eq!(
        sim.item_at(5, 1, SUIT),
        0,
        "полка стала легче на комбинезон"
    );
}

/// **С пола — тоже** (§12.34). Инвариант §12.16 («ничего не исчезает с пола в
/// лапы») этим не нарушается, а исполняется буквально: у подъёма есть адресат и
/// дорога к нему, и кот приходит на клетку сам.
#[test]
fn gear_is_picked_up_from_the_floor() {
    let mut sim = sim_with_store_and_gate();
    sim.set_auto_tidy(false); // иначе куча уедет на склад и проверять будет нечего
    sim.put_item(2, 1, SUIT, 1); // на полу коридора, а не на складе

    sim.tick_n(10);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "поднял с пола и надел");
    assert_eq!(sim.item_at(2, 1, SUIT), 0, "кучи больше нет");
}

/// Ближе — значит первым: выбор кучи тот же, что у любого раздатчика (§12.14).
#[test]
fn the_nearest_pile_wins() {
    let mut sim = sim_with_store_and_gate();
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, SUIT, 1); // в шаге от `a`
    sim.put_item(5, 1, SUIT, 1); // на складе, вчетверо дальше

    sim.tick_n(10);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "оделся");
    assert_eq!(
        sim.pos_of("a"),
        (1, 1),
        "взяв ближнюю кучу с места, а не сходив на склад (§12.250)"
    );
    assert_eq!(sim.item_at(2, 1, SUIT), 0, "её и забрал");
}

/// Одетого не одевают снова — иначе склад вычерпывался бы каждую ходку.
#[test]
fn an_equipped_cat_is_not_equipped_twice() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 10);
    sim.tick_n(20);
    let left = sim.item_at(5, 1, SUIT);

    sim.tick_n(20);
    assert_eq!(sim.item_at(5, 1, SUIT), left, "склад больше не трогают");
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и надето по одному");
}

/// Комплектов меньше, чем котов, — и «кому достанется» должно быть решением
/// правила, а не порядка сущностей ECS (§11, §12.24). Заодно проверяется, что
/// за одним комбинезоном не идут трое: раздатчик считает, сколько в куче уже
/// обещано тем, кто к ней идёт (§12.34).
#[test]
fn one_suit_goes_to_one_cat_in_a_fixed_order() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);

    sim.tick_n(1);
    assert!(sim.is_equipping("a"), "первый по id пошёл за ним");
    assert!(!sim.is_equipping("b"), "остальные не идут за той же кучей");
    assert!(!sim.is_equipping("c"));

    sim.tick_n(10);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "он же его и надел");
    assert!(sim.gear_of("b").is_empty());
    assert!(sim.gear_of("c").is_empty());
}

/// Кучи не стало, пока кот шёл, — это промах, а не ошибка (§12.15): задача
/// снимается, кот свободен, а раздатчик найдёт ему другую кучу.
#[test]
fn a_vanished_pile_just_frees_the_cat() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(1);
    assert!(sim.is_equipping("a"), "пошёл за комбинезоном");

    sim.take_item(5, 1, SUIT); // кучу забрали у кота из-под носа
    sim.tick_n(10);
    assert!(!sim.is_equipping("a"), "задача снята");
    assert!(sim.gear_of("a").is_empty(), "надеть было нечего");

    let at = sim.pos_of("a"); // а вот теперь есть — прямо под ногами
    sim.put_item(at.0, at.1, SUIT, 1);
    sim.tick_n(5);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и кот свободно взялся заново");
}

/// Пустой склад — это не ошибка: кот работает и ходит в поле как есть.
#[test]
fn an_empty_storage_leaves_cats_bare() {
    let mut sim = sim_with_store_and_gate();
    sim.tick_n(10);
    assert!(sim.gear_of("a").is_empty(), "надеть нечего — и ладно");
    assert!(!sim.is_equipping("a"), "и ходить незачем");
}

/// Экипировка — задача, а значит, занимает кота: приказ игрока её снимает, как
/// снимает стройку и сон (§12.15, §12.20).
#[test]
fn a_players_order_cancels_the_errand() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(1);
    assert!(sim.is_equipping("a"), "пошёл одеваться");

    assert!(sim.set_target("a", 1, 1), "приказ принят");
    assert!(!sim.is_equipping("a"), "и снял поход за вещью");
}

/// Ушедшего склад не достаёт: вне базы кота нет в мире базы (§12.22).
#[test]
fn an_away_cat_is_not_equipped() {
    let mut sim = sim_with_store_and_gate();
    let m = sim.set_mission(1, 40, &[]);
    assert!(sim.launch(m, squad(&["c"])));
    sim.tick_n(3);
    assert!(sim.is_away("c"), "ушёл");

    sim.put_item(6, 1, SUIT, 1); // прямо на шлюзе, откуда он ушёл
    sim.tick_n(10);
    assert!(
        sim.gear_of("c").is_empty(),
        "до ушедшего снаряжение не дотянется — он не на базе"
    );
}

// --- отряд ------------------------------------------------------------------

/// Сбор ждёт одевающегося: уходить голым, когда на складе лежит комбинезон, —
/// это сила отряда, зависящая от того, успел ли склад пополниться до нажатия
/// кнопки (§12.34).
#[test]
fn the_squad_waits_for_a_dressing_cat() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);
    let m = sim.set_mission(1, 40, &[]);
    assert!(sim.launch(m, squad(&["a"])), "заявка принята");

    sim.tick_n(2);
    assert!(sim.is_equipping("a"), "боец сперва идёт за комбинезоном");
    assert!(!sim.is_away("a"), "и с базы ещё не ушёл");

    sim.tick_n(20);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "оделся");
    assert!(sim.is_away("a"), "и только потом ушёл");
}

// --- что снаряжение делает --------------------------------------------------

/// Ради этого всё и вводилось: снаряжение — слагаемое силы отряда, растущее не
/// от навыка (§12.29). Без него сложность 4 отдаёт половину добычи, с ним — всю.
#[test]
fn gear_adds_strength_to_the_squad() {
    let mut sim = sim_with_store_and_gate();
    let m = sim.set_risky_mission(2, 10, 4, 0, &[(0, 40)]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);
    let bare = sim.item_total(0);

    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 2);
    sim.tick_n(15); // оба сходили и оделись
    let m = sim.set_risky_mission(2, 10, 4, 0, &[(0, 40)]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);

    assert_eq!(bare, 20, "голый отряд вытянул половину");
    assert_eq!(sim.item_total(0), 40, "одетый — всю добычу");
}

/// Провал сдирает снаряжение. До этого он стоил только бодрости, а она
/// восстанавливается бесплатно — то есть заведомо провальная вылазка была
/// способом качать «Вылазку» за одно лишь время (§12.29).
#[test]
fn a_failed_raid_destroys_the_gear() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 2);
    sim.tick_n(15);
    // Сложность 10 против силы 2×(1+1): вдвое меньше нужного — провал.
    let m = sim.set_risky_mission(2, 10, 10, 0, &[(0, 40)]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);

    assert_eq!(sim.item_total(0), 0, "вернулись ни с чем");
    assert!(sim.gear_of("a").is_empty(), "и ободранными");
    assert!(sim.gear_of("b").is_empty());
}

/// Успех снаряжение не изнашивает: износ за каждый выход превратил бы петлю
/// «добыча → сила» в оброк (§12.29).
#[test]
fn a_successful_raid_keeps_the_gear() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 2);
    sim.tick_n(15);
    let m = sim.set_risky_mission(2, 10, 2, 0, &[(0, 10)]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);

    assert_eq!(sim.gear_of("a"), vec![SUIT], "комбинезон цел");
    assert_eq!(sim.gear_of("b"), vec![SUIT]);
}

/// Ободранный отряд одевается заново — состояние обратимо (§12.10): комплект
/// наберётся, как только на базе снова будет из чего.
#[test]
fn a_stripped_cat_is_re_equipped() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 2); // ровно на двоих: запаса не остаётся
    sim.tick_n(15);
    let m = sim.set_risky_mission(2, 10, 10, 0, &[]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);
    assert!(sim.gear_of("a").is_empty(), "провал раздел");

    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(15);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "сходил и оделся снова");
}

/// Нанятый приходит **со своим** (§12.268): пусто у кандидата — голым и без
/// приписки, и сам за комбинезоном не пойдёт, пока игрок не велит.
#[test]
fn a_hired_cat_comes_bare_and_waits_for_an_outfit() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 4);
    let r = sim.set_recruit("nail", 0, &[], &[]);
    assert!(sim.hire(r));

    sim.tick_n(15);
    assert!(sim.gear_of("nail").is_empty(), "новичок гол");
    assert!(
        sim.outfit_of("nail").is_empty(),
        "и ничего ему не приписано"
    );

    assert!(sim.set_outfit("nail", SUIT, true));
    sim.tick_n(15);
    assert_eq!(
        sim.gear_of("nail"),
        vec![SUIT],
        "приписали — сходил и оделся"
    );
}

/// Кандидат в комбинезоне: вещь надета с порога **и** приписана — потеряет,
/// доберёт сам (§12.268).
#[test]
fn a_recruit_arrives_wearing_its_gear() {
    let mut sim = sim_with_store_and_gate();
    let r = sim.set_recruit("nail", 0, &[], &[]);
    sim.set_recruit_gear(r, &[SUIT]);
    assert!(sim.hire(r));

    assert_eq!(sim.gear_of("nail"), vec![SUIT]);
    assert_eq!(sim.outfit_of("nail"), vec![SUIT]);
}

// --- личная вещь (§12.256, §12.268) ------------------------------------------

/// Личная вещь приходит с котом, но в приписку не идёт никогда: она не решение
/// игрока, а свойство кота.
#[test]
fn a_personal_item_is_never_in_the_outfit() {
    let mut sim = sim_with_store_and_gate();
    sim.set_personal(2);
    let r = sim.set_recruit("antenna", 0, &[], &[]);
    sim.set_recruit_gear(r, &[2]);
    assert!(sim.hire(r));

    assert_eq!(sim.gear_of("antenna"), vec![2], "надета с порога");
    assert!(sim.outfit_of("antenna").is_empty(), "но не приписана");
    assert_eq!(sim.outfit_gate("antenna", 2, true), "personal");
}

/// Снять личную вещь обычным образом нельзя: команда отклонена тегом, на коте
/// вещь цела, и под ногами ничего не появилось.
#[test]
fn a_personal_item_cannot_be_taken_off() {
    let mut sim = sim_with_store_and_gate();
    sim.set_personal(2);
    let r = sim.set_recruit("antenna", 0, &[], &[]);
    sim.set_recruit_gear(r, &[2]);
    assert!(sim.hire(r));
    let at = sim.pos_of("antenna");

    assert_eq!(sim.outfit_gate("antenna", 2, false), "personal");
    assert!(!sim.set_outfit("antenna", 2, false), "снятие отклонено");
    assert_eq!(sim.gear_of("antenna"), vec![2]);
    assert_eq!(sim.item_at(at.0, at.1, 2), 0, "кучи не появилось");
}

/// Личная вещь не изнашивается, даже если у предмета есть `wear`.
#[test]
fn a_personal_item_never_wears() {
    let mut sim = sim_with_store_and_gate();
    sim.set_personal(2);
    sim.set_wear(2, 1);
    sim.put_gear("a", &[2]);
    let m = sim.set_risky_mission(1, 10, 0, 0, &[]);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    assert_eq!(sim.gear_of("a"), vec![2], "после вылазки вещь на месте");
}

// --- приписка (§12.268) ------------------------------------------------------

/// Снятая приписка роняет надетое под ноги — ничего не исчезает (инвариант 8).
#[test]
fn taking_off_drops_the_item_underfoot() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(15);
    assert_eq!(sim.gear_of("a"), vec![SUIT]);
    let at = sim.pos_of("a");

    assert!(sim.set_outfit("a", SUIT, false));
    assert!(sim.gear_of("a").is_empty());
    assert!(sim.outfit_of("a").is_empty());
    assert_eq!(sim.item_at(at.0, at.1, SUIT), 1, "комбинезон под ногами");
}

/// Ушедшего не переодевают: состав в поле заморожен (§12.22).
#[test]
fn a_cat_away_keeps_its_outfit() {
    let mut sim = sim_with_store_and_gate();
    let m = sim.set_risky_mission(1, 200, 0, 0, &[]);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(20);
    assert!(sim.is_away("a"));

    assert_eq!(sim.outfit_gate("a", SUIT, false), "away");
    assert!(!sim.set_outfit("a", SUIT, false));
    assert_eq!(sim.outfit_of("a"), vec![SUIT]);
}

/// Второй прибор сбора коту ни к чему (§12.256): ворота называют причину.
#[test]
fn a_collector_cannot_be_given_a_second_sampler() {
    let mut sim = sim_with_store_and_gate();
    sim.set_item_traits(2, 10, false, false);
    sim.set_item_traits(3, 10, false, false);
    sim.put_gear("a", &[2]);
    sim.put_item(5, 1, 3, 1); // пробоотборник база видела
    sim.tick_n(1);
    assert_eq!(sim.outfit_gate("a", 3, true), "collector");
    assert!(!sim.set_outfit("a", 3, true));
}

// --- износ (§12.268) ---------------------------------------------------------

/// Успешная вылазка снимает выход; вещь с запасом остаётся на коте.
#[test]
fn a_success_wears_gear_by_one() {
    let mut sim = sim_with_store_and_gate();
    sim.set_wear(SUIT, 3);
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(15);
    assert_eq!(
        sim.wear_left("a", SUIT),
        Some(3),
        "новая — с полной прочностью"
    );

    let m = sim.set_risky_mission(1, 10, 0, 0, &[]);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    assert_eq!(sim.wear_left("a", SUIT), Some(2));
}

/// Неполный успех изнашивает сильнее полного — той же долей, что и добычу.
#[test]
fn a_partial_success_wears_more() {
    assert_eq!(crate::gear::wear_toll(100), 1);
    assert_eq!(crate::gear::wear_toll(75), 2);
    assert_eq!(crate::gear::wear_toll(50), 3);
}

/// Вещь без `wear` вечна: сколько ни ходи.
#[test]
fn zero_wear_never_wears_out() {
    let mut sim = sim_with_store_and_gate();
    sim.put_gear("a", &[SUIT]);
    for _ in 0..3 {
        let m = sim.set_risky_mission(1, 10, 0, 0, &[]);
        assert!(sim.launch(m, squad(&["a"])));
        sim.tick_n(40);
    }
    assert_eq!(sim.gear_of("a"), vec![SUIT]);
}

/// Сношенная до нуля вещь исчезает, а приписка ведёт за новой.
#[test]
fn worn_out_gear_vanishes_and_the_cat_fetches_a_new_one() {
    let mut sim = sim_with_store_and_gate();
    sim.set_wear(SUIT, 1);
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(15);
    assert_eq!(sim.gear_of("a"), vec![SUIT]);

    let m = sim.set_risky_mission(1, 10, 0, 0, &[]);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    assert!(sim.gear_of("a").is_empty(), "износился до нуля");
    assert_eq!(sim.item_total(SUIT), 0, "и кучей не стал");

    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(15);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "сходил за новым сам");
    assert_eq!(sim.wear_left("a", SUIT), Some(1));
}

// --- боевой рулсет ----------------------------------------------------------

/// На настоящем `core.yaml`: комбинезонов на старте нет, они приезжают со
/// **второй** ступени лестницы («Свалка» их не даёт: первая вылазка и без того
/// открывает разом склад, лабораторию, пост и тему) и надеваются сами — хоть с
/// пола у шлюза, хоть со склада, куда их свезёт уборка. Ловит контент, в котором
/// снаряжение забыли положить в добычу
/// или у него нулевая `force`, — синтетическая схема этого не увидит.
#[test]
fn the_shipped_ruleset_equips_its_cats_from_loot() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // караван приносит своё: здесь считаем добычу вылазки
    let suit = 3; // индекс `suit` в палитре предметов

    assert_eq!(sim.item_total(suit), 0, "на старте одеться не во что");
    assert!(sim.gear_of("excellent").is_empty());
    // Невиданную вещь не назначить (§12.131, §12.268).
    assert_eq!(sim.outfit_gate("excellent", suit, true), "unseen");
    assert!(!sim.set_outfit("excellent", suit, true));

    // Известность второй ступени база набирает вылазками; здесь она не предмет
    // проверки, поэтому выставлена прямо.
    sim.set_fame(20);
    assert!(
        sim.launch(1, squad(&["excellent", "sp2", "sp3"])),
        "«Сопровождение каравана»",
    );
    sim.tick_n(700);
    assert_eq!(sim.mission_left(), None, "отряд вернулся");

    // Добыча у шлюза — теперь база комбинезон видела, и его можно назначить.
    // Комбинезон в добыче один и приписан одному — он за ним и идёт.
    sim.tick_n(1);
    assert!(
        sim.set_outfit("excellent", suit, true),
        "виданный — назначается"
    );
    sim.tick_n(1500); // добыча ложится у шлюза, и за ней приходят сами
    assert!(
        !sim.gear_of("excellent").is_empty(),
        "приписанный кот оделся сам — и добыча впервые ушла не внутрь базы",
    );
}

// --- ворота на надевание (§12.114) ------------------------------------------

/// Трофей, которого база ещё не поняла, надеть нельзя: `requires` у предмета —
/// те же ворота технологии, что у тайла и у рецепта, только на третьем месте.
/// Кот к такой куче не идёт вовсе — задача не заводится, а не бросается на
/// полпути: отказ живёт в раздатчике (§12.114).
#[test]
fn an_ununderstood_item_is_not_worn() {
    let mut sim = sim_with_store_and_gate();
    sim.set_wear_tech(SUIT, "xenotech");
    sim.put_item(5, 1, SUIT, 1);

    sim.tick_n(20);
    assert!(!sim.is_equipping("a"), "за непонятной вещью никто не пошёл");
    assert!(sim.gear_of("a").is_empty(), "и никто её не надел");
    assert_eq!(sim.item_at(5, 1, SUIT), 1, "трофей так и лежит на складе");
}

/// А как только тема изучена, тот же кот идёт за тем же трофеем — без второй
/// команды игрока: шаблон не менялся, менялось знание базы.
#[test]
fn understanding_opens_the_trophy() {
    let mut sim = sim_with_store_and_gate();
    sim.set_wear_tech(SUIT, "xenotech");
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(20);
    assert!(sim.gear_of("a").is_empty(), "пока не поняли — не носим");

    sim.set_tech("xenotech");
    sim.tick_n(20);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "поняли — надели");
}

/// Стартовые коты боевого рулсета голы и ничего не носят (§12.268): кому что
/// приписать, решает игрок. Кандидаты тоже приходят голыми — кроме тех, у кого
/// личная вещь: неличного снаряжения в записи кандидата нет ни у кого.
#[test]
fn the_shipped_ruleset_starts_its_cats_bare() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    for id in ["excellent", "sp2", "sp3"] {
        assert!(sim.gear_of(id).is_empty(), "{id} одет с порога");
        assert!(sim.outfit_of(id).is_empty(), "{id} с припиской с порога");
    }
    let items = sim.world.resource::<ItemRules>();
    let recruits = sim.world.resource::<RecruitRules>();
    for r in &recruits.0 {
        assert!(
            r.gear.iter().all(|&i| items.personal(i)),
            "кандидат «{}» приходит в неличном снаряжении",
            r.id,
        );
    }
}

/// Антенна приходит с анализатором, и снять его обычным образом нельзя
/// (§12.256, §12.268): вещь надета, в приписке её нет, команда снятия
/// отклонена тегом.
#[test]
fn the_shipped_antenna_keeps_her_analyzer() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    sim.add_storage();
    let scrap = sim.item_index("scrap").expect("лом");
    let part = sim.item_index("part").expect("деталь");
    let analyzer = sim.item_index("analyzer").expect("анализатор");
    sim.put_item(4, 3, scrap, 60);
    sim.put_item(5, 3, part, 20);
    sim.set_fame(90);
    let r = sim
        .world
        .resource::<RecruitRules>()
        .0
        .iter()
        .position(|r| r.id == "antenna")
        .expect("Антенна");
    assert!(sim.hire(r), "наняли");

    assert_eq!(sim.gear_of("antenna"), vec![analyzer]);
    assert!(sim.outfit_of("antenna").is_empty());
    assert!(!sim.set_outfit("antenna", analyzer, false), "не снимается");
    assert_eq!(sim.gear_of("antenna"), vec![analyzer]);
}
