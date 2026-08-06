//! Типы предметов: цена набором, кучи разных типов, ходка на один тип (§12.21).
//!
//! В схеме по умолчанию тип один — предмет `0`, и остальные тесты о типах не
//! знают. Здесь миры собираются явно: цена задаётся `set_cost_items`, кучи
//! кладутся `put_item`.

use super::sim_from;
use crate::sim::Sim;

const CORRIDOR: [&str; 3] = ["##########", "#a.......#", "##########"];

const SCRAP: usize = 0;
const PART: usize = 1;

// --- цена набором ----------------------------------------------------------

/// Площадка, которой нужны два типа, ждёт оба: пока не завезли деталь, стройка
/// не начинается, даже если лома с запасом.
#[test]
fn a_site_waits_for_every_item_of_its_cost() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost_items(0, &[(SCRAP, 2), (PART, 1)]);
    sim.put_item(3, 1, SCRAP, 10);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(60);
    assert_eq!(sim.delivered_item_at(1, 2, SCRAP), 2, "лом завезли весь");
    assert_eq!(sim.tile(1, 2), -1, "но без детали стройка не началась");

    sim.put_item(5, 1, PART, 1);
    sim.tick_n(200);
    assert_eq!(sim.tile(1, 2), 0, "деталь появилась — тайл построен");
}

/// Снос возвращает цену целиком, каждым типом. Это единственный источник
/// деталей на POC: производства нет, разбирать приходится наследство.
#[test]
fn demolition_returns_every_item_of_the_cost() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost_items(0, &[(SCRAP, 2), (PART, 1)]);
    assert!(sim.plan_demolish(4, 1), "снос запланирован");

    sim.tick_n(60);
    assert_eq!(sim.tile(4, 1), -1, "клетка снесена");
    assert_eq!(sim.scrap_total(), 3, "вернулась вся цена");
    assert_eq!(sim.item_total(SCRAP), 2, "два лома");
    assert_eq!(sim.item_total(PART), 1, "и деталь");
}

// --- кучи и ходки ----------------------------------------------------------

/// Кучи разных типов лежат на одной клетке и не сливаются в одну.
#[test]
fn piles_of_different_items_share_a_cell() {
    let mut sim = sim_from(&CORRIDOR);
    sim.put_item(3, 1, SCRAP, 4);
    sim.put_item(3, 1, PART, 2);

    sim.tick_n(5);
    assert_eq!(sim.item_at(3, 1, SCRAP), 4, "лом на месте");
    assert_eq!(sim.item_at(3, 1, PART), 2, "деталь рядом, не слилась");
    assert_eq!(sim.scrap_at(3, 1), 6, "на клетке шесть штук всего");
}

/// За ходку кот везёт один тип: взяв лом, он не прихватывает деталь с той же
/// клетки — площадка дозаправится следующей ходкой.
#[test]
fn a_cat_carries_one_item_kind_per_trip() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost_items(0, &[(SCRAP, 1), (PART, 1)]);
    sim.put_item(3, 1, SCRAP, 1);
    sim.put_item(3, 1, PART, 1);
    sim.add_blueprint(1, 2, 0);

    // Ловим момент, когда кот поднял груз.
    for _ in 0..50 {
        sim.tick_n(1);
        if sim.carrying_of("a") > 0 {
            break;
        }
    }
    assert_eq!(sim.carrying_of("a"), 1, "в лапах одна штука");
    let carried = sim.carrying_item_of("a").expect("кот несёт груз");
    assert_eq!(
        sim.item_at(3, 1, 1 - carried),
        1,
        "второй тип остался лежать"
    );

    sim.tick_n(200);
    assert_eq!(sim.tile(1, 2), 0, "обе ходки сделаны, тайл построен");
    assert_eq!(sim.scrap_total(), 0, "и весь материал ушёл в стройку");
}

/// Носильщик берёт с кучи тот тип, которого площадке не хватает, а не первый
/// попавшийся под ноги.
#[test]
fn a_hauler_picks_the_item_the_site_is_missing() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost_items(0, &[(PART, 1)]);
    sim.put_item(3, 1, SCRAP, 5); // лом рядом, но он не нужен
    sim.put_item(3, 1, PART, 1);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(200);
    assert_eq!(sim.tile(1, 2), 0, "тайл построен деталью");
    assert_eq!(sim.item_at(3, 1, SCRAP), 5, "лом никто не трогал");
}

// --- склад -----------------------------------------------------------------

/// Склад типо-агностичен: ёмкость считает штуки, и разные типы делят её.
#[test]
fn storage_capacity_counts_pieces_of_any_kind() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 3);
    sim.force_tile(8, 1, 1);
    sim.put_item(3, 1, SCRAP, 2);
    sim.put_item(5, 1, PART, 2);

    sim.tick_n(400);
    assert_eq!(sim.scrap_at(8, 1), 3, "склад забит под завязку — три штуки");
    assert_eq!(sim.scrap_total(), 4, "четвёртая осталась на полу");
    assert!(
        sim.item_at(8, 1, SCRAP) > 0 && sim.item_at(8, 1, PART) > 0,
        "на складе лежат оба типа"
    );
}

// --- боевой рулсет ---------------------------------------------------------

/// На настоящем `core.yaml`: лежанка стоит лом **и** деталь, и коты довозят оба
/// типа. Ловит рассогласование кода и контента — предмет под другим `id`,
/// цену, оставшуюся числом, пустую палитру предметов.
#[test]
fn the_shipped_ruleset_builds_from_two_item_kinds() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).ok().expect("рулсет должен разбираться");
    sim.without_timeline(); // тест про материал, а не про мир по расписанию
    let before = sim.scrap_total();

    // Лежанка в коридоре: рядом со складом, куда за деталью и пойдут.
    assert!(sim.add_blueprint(9, 7, 4), "чертёж лежанки поставлен");
    sim.tick_n(2000);

    assert_eq!(sim.tile(9, 7), 4, "лежанка построена");
    assert_eq!(
        sim.scrap_total(),
        before - 2,
        "ушли ровно лом и деталь — цена тайла"
    );
}

// --- имущество базы по типам (§12.53) ---------------------------------------

/// Шапка считает не всё добро разом, а три вещи порознь: чем можно платить
/// (склад), что ещё валяется и что обещано покупателю. Одно число обманывало:
/// игрок видел «всё есть», а найм отклонялся, потому что платит склад (§12.24).
#[test]
fn stock_tells_the_shelf_from_the_floor() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_capacity(1, 20);
    sim.force_tile(2, 1, 1); // склад
    sim.put_item(2, 1, 0, 7); // на складе
    sim.put_item(4, 1, 0, 3); // на полу
    sim.put_item(4, 1, 1, 2); // и другого типа тоже на полу

    assert_eq!(sim.stock_of(0), (7, 3, 0), "склад и пол считаются порознь");
    assert_eq!(sim.stock_of(1), (0, 2, 0), "и по каждому типу свои числа");
}

/// Груз в лапах — имущество базы, и он идёт в «валяется»: счётчик, проседающий,
/// пока кот несёт кучу, читается как потеря материала.
#[test]
fn goods_in_paws_are_counted_as_loose() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_capacity(1, 20);
    sim.force_tile(2, 1, 1);
    sim.put_item(5, 1, 0, 4); // лом на полу, за ним придёт уборка

    let mut in_paws = 0;
    for _ in 0..200 {
        sim.tick_n(1);
        in_paws = sim.carrying_of("a") + sim.carrying_of("b");
        if in_paws > 0 {
            break;
        }
    }
    assert!(in_paws > 0, "кот поднял лом");
    let (stored, loose, _) = sim.stock_of(0);
    assert_eq!(stored + loose, 4, "сумма не проседает, пока груз в лапах");
    assert!(
        loose >= in_paws,
        "и лапы считаются валяющимся, а не складом"
    );
}

/// Забронированное под сделку показывается отдельно: базе оно уже не
/// принадлежит (§12.50), и платить им нельзя, хотя лежит оно на складе.
#[test]
fn booked_goods_are_shown_apart_from_the_shelf() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_trade_post(2, true);
    sim.force_tile(5, 1, 2);
    sim.set_capacity(3, 20);
    sim.force_tile(6, 1, 3); // склад в дальнем конце
    let f = sim.set_faction(100);
    sim.set_market(f, 100, 40, 25, 0);
    sim.set_prices(f, 0, &[10]);
    sim.put_item(6, 1, 0, 5);

    assert_eq!(sim.stock_of(0), (5, 0, 0), "пока всё своё");
    assert!(sim.trade(f, 0, 2, false), "заявка на продажу двух принята");
    assert_eq!(sim.stock_of(0), (5, 0, 2), "две обещаны покупателю");
}
