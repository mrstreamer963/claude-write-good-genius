//! Снаряжение: комплект по шаблону, снятый со склада (§12.29).
//!
//! Снаряжение — свойство предмета (`force`), а не отдельная сущность, поэтому
//! проверять его надо там, где оно что-то меняет: в силе отряда. Отсюда и мир
//! на все тесты — коридор со складом и шлюзом: одеться и уйти в поле.
//!
//! В схеме `sim_from` предметы бессильны и шаблон пуст, ровно как тайл там
//! бесплатен: это контент рулсета, и включают его тесты сами (`set_force`,
//! `set_loadout`).

use super::*;

/// Предмет-снаряжение в тестах: индекс палитры, у которого есть `force`.
const SUIT: usize = 1;

/// Отряд поимённо — состав выбирает игрок (§12.23).
fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Коридор: склад в (5,1), шлюз в (6,1), комплект из одного «комбинезона».
/// Коты `a`, `b` и `c` — в этом порядке их и одевают (по `id`).
fn sim_with_store_and_gate() -> Sim {
    let mut sim = sim_from(&["########", "#a.b..c#", "########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.set_gate(2, true);
    sim.force_tile(6, 1, 2);
    sim.set_force(SUIT, 1);
    sim.set_loadout(&[SUIT]);
    sim
}

// --- экипировка -------------------------------------------------------------

/// Комплект коты добирают сами: игрок его не выдаёт, как не выдаёт чертёж
/// конкретному коту (§12.16).
#[test]
fn a_cat_takes_the_loadout_from_storage() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);

    assert!(sim.gear_of("a").is_empty(), "пока склад не тронут");
    sim.tick_n(1);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "комбинезон надет");
    assert_eq!(sim.item_at(5, 1, SUIT), 0, "и снят со склада");
}

/// **Со склада, а не с пола** (§12.16, §12.29): валяющееся под ногами — ещё не
/// имущество базы, сперва его свезут туда обычной уборкой. Иначе предмет
/// исчезал бы с пола «в лапы», против чего и писался инвариант переноса.
#[test]
fn gear_is_not_taken_from_the_floor() {
    let mut sim = sim_with_store_and_gate();
    sim.set_auto_tidy(false); // иначе куча уедет на склад и проверять будет нечего
    sim.put_item(3, 1, SUIT, 1); // на полу коридора, а не на складе

    sim.tick_n(5);
    assert!(sim.gear_of("b").is_empty(), "с пола снаряжение не берут");
    assert_eq!(sim.item_at(3, 1, SUIT), 1, "куча на месте");
}

/// Одетого не одевают снова — иначе склад вычерпывался бы каждый тик.
#[test]
fn an_equipped_cat_is_not_equipped_twice() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 10);
    sim.tick_n(1);
    let left = sim.item_at(5, 1, SUIT);

    sim.tick_n(20);
    assert_eq!(sim.item_at(5, 1, SUIT), left, "склад больше не трогают");
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и надето по одному");
}

/// Комплектов меньше, чем котов, — и «кому достанется» должно быть решением
/// правила, а не порядка сущностей ECS (§11, §12.24).
#[test]
fn gear_goes_to_cats_in_a_fixed_order() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(1);

    assert_eq!(sim.gear_of("a"), vec![SUIT], "первый по id");
    assert!(sim.gear_of("b").is_empty());
    assert!(sim.gear_of("c").is_empty());
}

/// Пустой склад — это не ошибка: кот работает и ходит в поле как есть.
#[test]
fn an_empty_storage_leaves_cats_bare() {
    let mut sim = sim_with_store_and_gate();
    sim.tick_n(5);
    assert!(sim.gear_of("a").is_empty(), "надеть нечего — и ладно");
}

/// Ушедшего склад не достаёт: вне базы кота нет в мире базы (§12.22).
#[test]
fn an_away_cat_is_not_equipped() {
    let mut sim = sim_with_store_and_gate();
    let m = sim.set_mission(1, 40, &[]);
    assert!(sim.launch(m, squad(&["c"])));
    sim.tick_n(3);
    assert!(sim.is_away("c"), "ушёл");

    sim.put_item(5, 1, SUIT, 3);
    sim.tick_n(1);
    assert!(
        sim.gear_of("c").is_empty(),
        "до ушедшего склад не дотянется"
    );
    assert_eq!(
        sim.item_at(5, 1, SUIT),
        1,
        "комбинезон дождался его на складе"
    );
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
    sim.tick_n(1); // оба оделись
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
    sim.tick_n(1);
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
    sim.tick_n(1);
    let m = sim.set_risky_mission(2, 10, 2, 0, &[(0, 10)]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);

    assert_eq!(sim.gear_of("a"), vec![SUIT], "комбинезон цел");
    assert_eq!(sim.gear_of("b"), vec![SUIT]);
}

/// Ободранный отряд одевается заново — состояние обратимо (§12.10): комплект
/// наберётся, как только на складе снова будет из чего.
#[test]
fn a_stripped_cat_is_re_equipped() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 2); // ровно на двоих: на складе не остаётся запаса
    sim.tick_n(1);
    let m = sim.set_risky_mission(2, 10, 10, 0, &[]);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);
    assert!(sim.gear_of("a").is_empty(), "провал раздел");

    // А был бы запас — оделись бы сами тем же тиком: снаряжение не задача, его
    // никто не «назначает». Проверяем это, довезя на склад новый комбинезон.
    sim.put_item(5, 1, SUIT, 1);
    sim.tick_n(1);
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и склад одел снова");
}

/// Нанятый приходит голым и одевается по общему правилу: второго места, где
/// коту выдают вещи, не заводится (§12.24).
#[test]
fn a_hired_cat_is_equipped_too() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 4);
    let r = sim.set_recruit("nail", 0, &[], &[]);
    assert!(sim.hire(r));

    sim.tick_n(1);
    assert_eq!(sim.gear_of("nail"), vec![SUIT], "новичок одет");
}

// --- боевой рулсет ----------------------------------------------------------

/// На настоящем `core.yaml`: комбинезонов на старте нет, они приезжают со
/// «Свалки», ложатся на склад обычной уборкой и надеваются сами. Ловит контент,
/// в котором шаблон ссылается на предмет не тем `id`, снаряжение забыли положить
/// в добычу или у него нулевая `force`, — синтетическая схема этого не увидит.
#[test]
fn the_shipped_ruleset_equips_its_cats_from_loot() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // караван приносит своё: здесь считаем добычу вылазки
    let suit = 3; // индекс `suit` в палитре предметов

    assert_eq!(sim.item_total(suit), 0, "на старте одеться не во что");
    assert!(sim.gear_of("excellent").is_empty());

    assert!(sim.launch(0, squad(&["excellent", "sp2"])), "«Свалка»");
    sim.tick_n(600);
    assert_eq!(sim.mission_left(), None, "отряд вернулся");

    sim.tick_n(900); // уборка свозит добычу на склад, а склад одевает
    assert!(
        !sim.gear_of("excellent").is_empty(),
        "бригада оделась сама, без команды игрока — и добыча впервые ушла не внутрь базы",
    );
}
