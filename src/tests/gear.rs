//! Снаряжение: комплект по шаблону, за которым кот идёт сам (§12.29, §12.34).
//!
//! Снаряжение — свойство предмета (`force`), а не отдельная сущность, поэтому
//! проверять его надо там, где оно что-то меняет: в силе отряда. Отсюда и мир
//! на все тесты — коридор со складом и шлюзом: одеться и уйти в поле.
//!
//! Одевание — задача с маршрутом (§12.34), поэтому «оделся» здесь всегда стоит
//! тиков: кот доходит до кучи и берёт вещь оттуда. Это и есть главная разница с
//! первой редакцией §12.29, где склад одевал мгновенно.
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
/// Коты `a` (1,1), `b` (3,1) и `c` (6,1) — в этом порядке их и одевают (по `id`).
fn sim_with_store_and_gate() -> Sim {
    let mut sim = sim_from(&["########", "#a.b..c#", "########"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.set_gate(2, true);
    sim.set_relay(2, true);
    sim.force_tile(6, 1, 2);
    sim.set_force(SUIT, 1);
    sim.set_loadout(&[SUIT]);
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
    assert_eq!(sim.pos_of("a"), (5, 1), "дошёл до склада");
    assert_eq!(sim.gear_of("a"), vec![SUIT], "и надел комбинезон");
    assert_eq!(sim.item_at(5, 1, SUIT), 0, "склад стал легче ровно на него");
    assert!(!sim.is_equipping("a"), "задача закрыта");
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
        (2, 1),
        "сходив за ближней кучей, а не на склад"
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

/// Нанятый приходит голым и одевается по общему правилу: второго места, где
/// коту выдают вещи, не заводится (§12.24).
#[test]
fn a_hired_cat_is_equipped_too() {
    let mut sim = sim_with_store_and_gate();
    sim.put_item(5, 1, SUIT, 4);
    let r = sim.set_recruit("nail", 0, &[], &[]);
    assert!(sim.hire(r));

    sim.tick_n(15);
    assert_eq!(sim.gear_of("nail"), vec![SUIT], "новичок одет");
}

// --- боевой рулсет ----------------------------------------------------------

/// На настоящем `core.yaml`: комбинезонов на старте нет, они приезжают со
/// «Свалки» и надеваются сами — хоть с пола у шлюза, хоть со склада, куда их
/// свезёт уборка. Ловит контент, в котором шаблон ссылается на предмет не тем
/// `id`, снаряжение забыли положить в добычу или у него нулевая `force`, —
/// синтетическая схема этого не увидит.
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

    // Комбинезон в добыче один, и достаётся он первому по `id` — порядок
    // раздачи задан явно, чтобы «кому достанется» было видно игроку (§12.29).
    sim.tick_n(1500); // добыча ложится у шлюза, и за ней приходят сами
    assert!(
        !sim.gear_of("excellent").is_empty(),
        "бригада оделась сама, без команды игрока — и добыча впервые ушла не внутрь базы",
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
