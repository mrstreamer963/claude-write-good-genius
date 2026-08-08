//! Голод: сытость, поход за пайком и цена пустого желудка (§12.36).
//!
//! Вторая потребность устроена как первая (§12.20), но удовлетворитель у неё
//! другой природы: не тайл-лежанка, а **предмет с `nutrition`**, лежащий кучей.
//! Поэтому «поел» здесь всегда стоит тиков — кот доходит до кучи, как доходит до
//! комбинезона (§12.34), — а сама еда мгновенна: вся длительность это дорога.
//!
//! В схеме `sim_from` голода нет (`FoodRules` пуст) и предметы несъедобны, ровно
//! как тайл там бесплатен: это контент рулсета, и включают его тесты сами
//! (`set_food`, `set_nutrition`).

use super::*;

/// Съедобный предмет в тестах: индекс палитры, у которого есть `nutrition`.
const RATION: usize = 1;

/// Коридор со складом в (5,1) и котами `a` (1,1) и `b` (3,1).
///
/// Голод включён с запасом: полная сытость 100, «пора есть» ниже 40, пустой
/// желудок жжёт бодрость вдвое. Паёк закрывает шкалу целиком.
fn sim_with_food() -> Sim {
    let mut sim = sim_from(&["#######", "#a.b..#", "#######"]);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim.set_nutrition(RATION, 100);
    sim.set_food(100, 40, 2);
    sim
}

// --- поход за едой ----------------------------------------------------------

/// Сытый кот за едой не идёт, голодный идёт сам: это вторая задача, которую кот
/// назначает себе без разметки игрока (§12.20, §12.36).
#[test]
fn a_hungry_cat_walks_to_the_food_and_eats_it() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);

    sim.tick_n(1);
    assert!(!sim.is_eating("a"), "сытому паёк не нужен");

    sim.set_fed("a", 10);
    sim.tick_n(1);
    assert!(sim.is_eating("a"), "голодный взялся за поход");
    assert!(sim.fed_of("a") < 40, "но пока не поел");

    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (5, 1), "дошёл до кучи");
    // Не ровно 100: пока кот шёл и стоял, голод тикал дальше (§12.36).
    assert!(sim.fed_of("a") > 90, "и наелся почти до полной");
    assert_eq!(sim.item_at(5, 1, RATION), 0, "паёк съеден");
    assert!(!sim.is_eating("a"), "задача закрыта");
}

/// **С пола — тоже**, и это тот же случай, что у снаряжения (§12.34): у подъёма
/// есть адресат и дорога к нему.
#[test]
fn food_is_eaten_from_the_floor() {
    let mut sim = sim_with_food();
    sim.set_auto_tidy(false); // иначе паёк уедет на склад и проверять будет нечего
    sim.put_item(2, 1, RATION, 1);
    sim.set_fed("a", 10);

    sim.tick_n(10);
    // Ровно 90: паёк долил до потолка, а голод отсчитал свои десять тиков.
    assert_eq!(sim.fed_of("a"), 90, "поел прямо с пола");
    assert_eq!(sim.item_at(2, 1, RATION), 0, "кучи больше нет");
}

/// Ближе — значит первым: выбор кучи тот же, что у любого раздатчика (§12.14).
#[test]
fn the_nearest_food_pile_wins() {
    let mut sim = sim_with_food();
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, RATION, 1); // в шаге от `a`
    sim.put_item(5, 1, RATION, 1); // на складе, вчетверо дальше
    sim.set_fed("a", 10);

    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (2, 1), "сходил за ближней кучей");
    assert_eq!(sim.item_at(2, 1, RATION), 0, "её и съел");
    assert_eq!(sim.item_at(5, 1, RATION), 1, "склад не тронут");
}

/// Несъедобное не едят: `nutrition` — такой же переключатель, как `force` у
/// снаряжения, и без него предмет для голодного кота не существует.
#[test]
fn a_cat_ignores_inedible_items() {
    let mut sim = sim_with_food();
    sim.set_auto_tidy(false);
    sim.put_item(2, 1, 0, 5); // предмет 0 несъедобен
    sim.set_fed("a", 10);

    sim.tick_n(10);
    assert!(!sim.is_eating("a"), "за ломом не пошёл");
    assert_eq!(sim.item_at(2, 1, 0), 5, "куча цела");
}

/// Еды нет вовсе — кот работает голодным. Это цена базы без запаса, а не
/// поломка: состояние обратимо, как `stuck` (§12.10).
#[test]
fn without_food_a_cat_just_stays_hungry() {
    let mut sim = sim_with_food();
    sim.set_fed("a", 10);

    sim.tick_n(20);
    assert!(!sim.is_eating("a"), "идти некуда");
    assert_eq!(sim.fed_of("a"), 0, "и сытость просто кончилась");
}

/// Пайка на всех не хватает — и «кому достанется» должно быть решением правила,
/// а не порядка сущностей ECS (§11, §12.24). Заодно: за одной штукой не идут
/// двое, раздатчик считает обещанное тем, кто уже в пути (§12.34).
#[test]
fn one_ration_goes_to_one_cat_in_a_fixed_order() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);
    sim.set_fed("a", 10);
    sim.set_fed("b", 10);

    sim.tick_n(1);
    assert!(sim.is_eating("a"), "первый по `id` идёт за пайком");
    assert!(!sim.is_eating("b"), "второй за тем же не идёт");

    sim.tick_n(10);
    assert!(sim.fed_of("a") > 90, "поел один");
    assert!(sim.fed_of("b") < 40, "второй остался голодным");
}

/// Куча исчезла, пока кот шёл, — промах, а не ошибка (§12.15): задача снимается,
/// и следующим тиком раздатчик подберёт другую кучу.
#[test]
fn a_vanished_pile_is_a_miss_not_a_crash() {
    let mut sim = sim_with_food();
    sim.set_auto_tidy(false);
    sim.put_item(5, 1, RATION, 1);
    sim.set_fed("a", 10);

    sim.tick_n(2);
    assert!(sim.is_eating("a"), "идёт к складу");
    sim.take_item(5, 1, RATION); // кучу унесли

    sim.tick_n(10);
    assert!(!sim.is_eating("a"), "задача снята");
    assert_eq!(sim.fed_of("a"), 0, "и есть было нечего");
}

// --- приоритет и занятость --------------------------------------------------

/// Начатое дело голод не срывает — второго порога у него нет намеренно
/// (§12.36): с работы кота уводит бодрость, а не голод.
#[test]
fn hunger_does_not_tear_a_cat_off_work() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);
    sim.add_blueprint(2, 1, 1); // любой тайл, кроме пола схемы

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "кот взялся за стройку");
    sim.set_fed("a", 1);

    sim.tick_n(3);
    assert!(sim.has_assignment("a"), "и не бросил её ради еды");
    assert!(!sim.is_eating("a"));
}

/// Еда раздаётся **раньше отдыха**: ходка за пайком короткая, а сон длится
/// сотни тиков, и уснувший голодным просыпается голодным же (§12.36).
#[test]
fn eating_comes_before_sleeping() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);
    sim.set_needs(100, 40, 1);
    sim.set_rest(1, 10); // склад заодно и лежанка: цель у обеих задач одна
    sim.set_fed("a", 10);
    sim.set_energy("a", 10);

    sim.tick_n(1);
    assert!(sim.is_eating("a"), "сперва поесть");
    assert!(!sim.is_resting("a"), "спать — потом");
}

/// Спящего голод не будит: кот доспит и поест проснувшимся (§12.36).
#[test]
fn a_sleeping_cat_is_not_woken_by_hunger() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);
    sim.set_needs(100, 40, 1);
    sim.set_rest(1, 1); // спит медленно — успеваем посмотреть
    sim.set_energy("a", 10);

    sim.tick_n(8);
    assert!(sim.is_resting("a"), "ушёл спать");
    sim.set_fed("a", 1);

    sim.tick_n(5);
    assert!(sim.is_resting("a"), "и голод его не поднял");
    assert!(!sim.is_eating("a"));
}

/// Сытость тикает и во сне: время идёт для всех (§12.36).
#[test]
fn hunger_ticks_while_asleep() {
    let mut sim = sim_with_food();
    sim.set_needs(100, 40, 1);
    sim.set_rest(1, 1);
    sim.set_energy("a", 10);

    sim.tick_n(8);
    assert!(sim.is_resting("a"), "спит");
    let before = sim.fed_of("a");
    sim.tick_n(5);
    assert_eq!(sim.fed_of("a"), before - 5, "и всё это время голодал");
}

/// Приказ игрока снимает поход за едой, как снимает стройку и сон: это
/// осознанное действие (§12.20). Кот вернётся к пайку, дойдя до цели.
#[test]
fn a_players_order_interrupts_eating() {
    let mut sim = sim_with_food();
    sim.put_item(5, 1, RATION, 1);
    sim.set_fed("a", 10);

    sim.tick_n(2);
    assert!(sim.is_eating("a"), "идёт есть");
    assert!(sim.set_target("a", 1, 1), "приказ «стой где стоишь»");
    assert!(!sim.is_eating("a"), "поход отменён");
}

// --- цена пустого желудка ---------------------------------------------------

/// Голод стоит бодрости, а не жизни (§12.36): на пустой желудок она горит в
/// `starve` раз быстрее. Это вся его цена — необратимого наказания нет.
#[test]
fn an_empty_stomach_burns_energy_twice_as_fast() {
    let mut sim = sim_with_food();
    sim.set_needs(1000, 0, 1); // спать никто не уходит: меряем только трату
    sim.set_fed("a", 100);
    sim.set_fed("b", 0);

    sim.tick_n(10);
    assert_eq!(sim.energy_of("a"), 990, "сытый тратит по очку за тик");
    assert_eq!(sim.energy_of("b"), 980, "голодный — вдвое");
}

/// Наелся — и трата вернулась к обычной. Состояние обратимо (§12.10).
#[test]
fn eating_stops_the_double_burn() {
    let mut sim = sim_with_food();
    sim.set_auto_tidy(false);
    sim.set_needs(1000, 0, 1);
    sim.put_item(2, 1, RATION, 1);
    sim.set_fed("a", 0);

    sim.tick_n(10); // дошёл и поел
    assert_eq!(sim.fed_of("a"), 90, "сыт");
    let before = sim.energy_of("a");
    sim.tick_n(10);
    assert_eq!(sim.energy_of("a"), before - 10, "жжёт по очку, как все");
}

/// Ушедших с базы голод не берёт вовсе — как и усталость: в поле кот не
/// симулируется, он считается (§12.22).
#[test]
fn a_cat_away_on_a_mission_does_not_starve() {
    let mut sim = sim_from(&["######", "#a.b.#", "######"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(4, 1, 1);
    sim.set_nutrition(RATION, 100);
    sim.set_food(100, 40, 2);
    let mission = sim.set_mission(2, 100, &[]);

    assert!(sim.launch(mission, vec!["a".to_string(), "b".to_string()]));
    sim.tick_n(20);
    assert!(sim.is_away("a"), "отряд ушёл");
    let fed = sim.fed_of("a");

    sim.tick_n(20);
    assert_eq!(sim.fed_of("a"), fed, "за шлюзом сытость не считается");
}

// --- боевой рулсет ----------------------------------------------------------

/// Тест на рассогласование кода и контента: паёк без `nutrition`, забытый блок
/// `food:`, порог выше потолка или еда, до которой не дойти со старта, —
/// синтетическая схема ничего этого не увидит.
#[test]
fn the_shipped_ruleset_feeds_a_hungry_cat() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // мир по расписанию — шум для чужой механики

    let full = sim.fed_of("excellent");
    assert!(full > 0, "голод в рулсете включён");
    sim.set_fed("excellent", 1);

    sim.tick_n(200);
    assert!(
        sim.fed_of("excellent") > full / 2,
        "кот дошёл до склада и поел из стартового запаса",
    );
}
