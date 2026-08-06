//! Торговля с внешним миром (§12.44).
//!
//! Три вещи держат эту механику честной, и все три здесь проверяются: курс —
//! **чистая функция тика** (значит расписание видно вперёд, а не караулится с
//! секундомером), купить **всегда дороже**, чем продать (значит торговля не
//! создаёт богатства), и цена **фиксируется при заказе** (значит панель и
//! списание не расходятся).
//!
//! Рынка в схеме `sim_from` нет, как нет вылазок и фракций: его включают
//! `set_faction` + `set_market` + `set_prices`.

use super::*;

/// Коридор со шлюзом в (3,1) и торговым постом в (5,1); фракция с рынком.
/// Вернёт симуляцию и индекс фракции.
fn sim_with_market() -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_trade_post(2, true);
    sim.force_tile(5, 1, 2);
    let f = sim.set_faction(100);
    sim.set_market(f, 100, 40, 25, 0);
    sim.set_prices(f, 0, &[10]);
    (sim, f)
}

// --- курс ------------------------------------------------------------------

/// Купить дороже, чем продать, — и разница в этом и есть весь смысл спреда:
/// торговля меняет состав богатства, а не его размер (§12.44).
#[test]
fn buying_always_costs_more_than_selling_pays() {
    let (sim, f) = sim_with_market();

    assert_eq!(sim.quote(f, 0, false), Some(10), "продажа по базе");
    assert_eq!(sim.quote(f, 0, true), Some(14), "покупка с наценкой 40%");
}

/// Чего нет в прайсе, тем фракция не торгует: отдельного флага для этого не
/// нужно — так и выражается «Полиция берёт образцы, Синдикат — лом».
#[test]
fn an_item_outside_the_price_list_is_not_traded() {
    let (sim, f) = sim_with_market();

    assert_eq!(sim.quote(f, 1, true), None, "деталью эти не торгуют");
    assert_eq!(sim.quote(f, 1, false), None, "и не покупают её тоже");
}

/// Фазу выбирает тик, а не бросок кубика (§11). Отсюда и главное свойство:
/// расписание видно вперёд, значит игрок планирует, а не караулит (§12.40).
#[test]
fn the_price_follows_the_tick_and_nothing_else() {
    let (mut sim, f) = sim_with_market();
    sim.set_market(f, 100, 0, 0, 10); // фаза держится 10 тиков
    sim.set_prices(f, 0, &[10, 20, 30]);

    assert_eq!(sim.quote(f, 0, false), Some(10), "фаза 0");
    sim.tick_n(10);
    assert_eq!(sim.quote(f, 0, false), Some(20), "фаза 1");
    sim.tick_n(10);
    assert_eq!(sim.quote(f, 0, false), Some(30), "фаза 2");
    sim.tick_n(10);
    assert_eq!(sim.quote(f, 0, false), Some(10), "цикл замкнулся");
}

/// Нулевая длина фазы — курс не меняется вовсе: то же правило нулей, что у цены
/// тайла и ёмкости склада.
#[test]
fn a_zero_period_freezes_the_price() {
    let (mut sim, f) = sim_with_market();
    sim.set_prices(f, 0, &[10, 99]);

    assert_eq!(sim.quote(f, 0, false), Some(10));
    sim.tick_n(500);
    assert_eq!(sim.quote(f, 0, false), Some(10), "фазы не сменились");
}

/// Репутация двигает курс в свою сторону — и **не тратится** при этом (§12.43).
#[test]
fn reputation_bends_the_price_both_ways() {
    let (mut sim, f) = sim_with_market();

    sim.set_standing(f, 100); // полное доверие: favor 25%
    assert_eq!(sim.quote(f, 0, false), Some(12), "продаём выгоднее");
    assert_eq!(sim.quote(f, 0, true), Some(10), "и покупаем дешевле");
    assert_eq!(sim.standing(f), 100, "а доверие осталось прежним");

    sim.set_standing(f, -100);
    assert_eq!(sim.quote(f, 0, false), Some(7), "с врагами торгуют хуже");
    assert_eq!(sim.quote(f, 0, true), Some(17), "и втридорога");
}

// --- покупка ---------------------------------------------------------------

/// Платят разом и вперёд, а товар едет: в этом зазоре и живёт решение (§12.44).
#[test]
fn a_purchase_is_paid_at_once_and_arrives_at_the_gate() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(100);

    assert!(sim.trade(f, 0, 3, true), "заказ принят");
    assert_eq!(
        sim.money(),
        100 - 14 * 3,
        "списали разом и по курсу покупки"
    );
    assert_eq!(sim.scrap_total(), 0, "но товара ещё нет");

    sim.tick_n(99);
    assert_eq!(sim.scrap_total(), 0, "и до срока не появится");

    sim.tick_n(1);
    assert_eq!(sim.scrap_at(3, 1), 3, "приехало кучей на шлюз");
    assert_eq!(sim.deal_of(), None, "сделка закрыта");
}

/// Купленное ложится на шлюз, а не на склад, — как добыча вылазки (§12.22).
/// Дальше его разносит обычная уборка, и отдельного пути у товара нет.
#[test]
fn a_purchase_is_tidied_away_like_any_loot() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(100);
    sim.set_capacity(3, 50);
    sim.force_tile(6, 1, 3); // склад в дальнем конце коридора

    assert!(sim.trade(f, 0, 2, true));
    sim.tick_n(400);

    assert!(sim.scrap_is_in_storage(), "товар уехал на склад сам");
}

#[test]
fn a_purchase_without_money_is_refused() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(13); // на штуку нужно 14

    assert!(!sim.trade(f, 0, 1, true), "нечем платить");
    assert_eq!(sim.money(), 13, "и ничего не списано");
    assert_eq!(sim.deal_of(), None);
}

/// Без торгового поста внешний мир с базой не разговаривает.
#[test]
fn trading_needs_a_post() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(100);
    sim.set_trade_post(2, false);

    assert!(!sim.trade(f, 0, 1, true), "поста нет — и сделки нет");
    assert_eq!(sim.money(), 100);
}

/// Сделка одна за раз, как вылазка, тема и заказ: с очередью задержка
/// перестала бы быть ограничением, а решение «на что я трачу окно» исчезло бы.
#[test]
fn only_one_deal_at_a_time() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(1000);

    assert!(sim.trade(f, 0, 1, true));
    assert!(!sim.trade(f, 0, 1, true), "вторая сделка не принимается");
    assert!(!sim.trade(f, 0, 1, false), "и продать заодно тоже нельзя");
}

/// Чем фракция не торгует, того у неё не купишь — и деньги при этом целы.
#[test]
fn an_untraded_item_cannot_be_bought() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(1000);

    assert!(!sim.trade(f, 1, 1, true), "деталью эти не торгуют");
    assert_eq!(sim.money(), 1000);
}

/// **Курс фиксируется при заказе.** Расписание успеет уйти, пока товар едет, —
/// и это не ошибка, а весь риск торговли: цена видна до клика и та же, что
/// спишется (§12.23, инвариант 14).
#[test]
fn the_price_is_locked_when_the_deal_is_struck() {
    let (mut sim, f) = sim_with_market();
    sim.set_market(f, 100, 40, 0, 10);
    sim.set_prices(f, 0, &[10, 99]);
    sim.set_money(1000);

    assert!(sim.trade(f, 0, 2, true), "берём по дешёвой фазе");
    assert_eq!(sim.money(), 1000 - 14 * 2);

    sim.tick_n(10); // расписание ушло на дорогую фазу
    assert_eq!(sim.quote(f, 0, true), Some(138), "курс сегодня другой");
    assert_eq!(
        sim.deal_of().map(|(_, _, unit, ..)| unit),
        Some(14),
        "а в сделке остался тот, по которому договорились",
    );
}

// --- продажа ---------------------------------------------------------------

/// Проданное **несут коты**, и это не украшение: продажа стоит котовремени, и
/// потому торговля не бесплатный оптимизатор (§12.44).
#[test]
fn a_sale_is_carried_to_the_gate_and_paid_per_unit() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 4);

    assert!(sim.trade(f, 0, 4, false), "выставили на продажу");
    assert_eq!(sim.money(), 0, "пока не донесли — не платят");

    sim.tick_n(60);

    assert_eq!(sim.money(), 40, "донесли всё и получили по курсу продажи");
    assert_eq!(sim.scrap_total(), 0, "товар ушёл с базы");
    assert_eq!(sim.deal_of(), None, "сделка закрыта");
}

/// **Проданное исчезает в момент сдачи и кучей на шлюзе не становится.**
///
/// Иначе уборка увезла бы его обратно на склад: `mark_loose_scrap` метит всё,
/// что лежит на клетке без ёмкости, а шлюз ёмкости не имеет. Это не «повезло»,
/// а свойство ветки `HaulTo::Sale`, и его надо держать (§12.44).
#[test]
fn sold_goods_never_pile_up_at_the_gate() {
    let (mut sim, f) = sim_with_market();
    sim.set_capacity(3, 50);
    sim.force_tile(6, 1, 3); // склад рядом с ломом
    sim.put_scrap(6, 1, 2);

    assert!(sim.trade(f, 0, 2, false));
    for _ in 0..80 {
        sim.tick();
        assert_eq!(sim.scrap_at(3, 1), 0, "на шлюзе не задерживается ни тика");
    }
    assert_eq!(sim.money(), 20, "а деньги пришли");
}

/// Курс не переезжает за время ходки: договорились — значит договорились.
#[test]
fn a_sale_pays_the_agreed_price_even_if_the_schedule_moves() {
    let (mut sim, f) = sim_with_market();
    sim.set_market(f, 100, 40, 0, 10);
    sim.set_prices(f, 0, &[10, 1]); // вторая фаза вдесятеро дешевле
    sim.put_scrap(6, 1, 2);

    assert!(sim.trade(f, 0, 2, false), "продаём по дорогой фазе");
    sim.tick_n(60); // расписание успело уйти, пока коты несли

    assert_eq!(
        sim.money(),
        20,
        "заплатили по договору, а не по сегодняшнему"
    );
}

/// Продажа денег вперёд не берёт и не даёт: платят по факту сдачи.
#[test]
fn a_sale_needs_no_money_up_front() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 1);

    assert!(sim.trade(f, 0, 1, false), "продавать можно и без гроша");
    assert_eq!(sim.money(), 0);
    sim.tick_n(60);
    assert_eq!(sim.money(), 10);
}

/// Продать можно только то, что на базе есть (§12.50).
///
/// Раньше такая сделка просто ждала товара, как чертёж ждёт лом (§12.15). Но
/// слот торговли один и отмены у него нет: заявка на то, чего нет, занимала его
/// навсегда — и торговля кончалась на первой же ошибке игрока.
#[test]
fn a_sale_needs_the_goods_on_base() {
    let (mut sim, f) = sim_with_market();

    assert!(
        !sim.trade(f, 0, 2, false),
        "продавать нечего — заявка отклонена"
    );
    sim.put_scrap(6, 1, 1);
    assert!(!sim.trade(f, 0, 2, false), "одной штуки на две мало");

    sim.put_scrap(6, 1, 1);
    assert!(sim.trade(f, 0, 2, false), "две есть — заявку приняли");
    sim.tick_n(60);
    assert_eq!(sim.money(), 20, "и их унесли");
}

/// Товар в лапах кота — тоже товар на базе: его считает и счётчик в шапке, и
/// донести его коту ничто не мешает (§12.50).
#[test]
fn goods_in_paws_count_as_goods_on_base() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 2);
    sim.set_capacity(1, 20);
    sim.force_tile(2, 1, 1); // склад, чтобы кот поднял лом уборкой
    for _ in 0..200 {
        if sim.carrying_of("a") > 0 || sim.carrying_of("b") > 0 {
            break;
        }
        sim.tick_n(1);
    }
    let paws = sim.carrying_of("a") + sim.carrying_of("b");
    assert!(paws > 0, "кто-то из котов поднял лом");

    assert_eq!(sim.item_on_base(0), 2, "весь лом цел, часть — в лапах");
    assert!(sim.trade(f, 0, 2, false), "и продать его можно");
}

/// Товар под сделкой базе больше не принадлежит: стройка его не заберёт
/// (§12.50). Иначе продажа осталась бы вечно открытой — слот торговли один и
/// отмены у него нет.
#[test]
fn goods_under_a_sale_are_not_spent_by_building() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 2);
    sim.set_cost(0, 2); // тайл стоит ровно те же два лома

    assert!(sim.trade(f, 0, 2, false), "выставили на продажу оба");
    assert!(sim.add_blueprint(1, 0, 0), "и разметили стройку рядом");

    sim.tick_n(200);
    assert_eq!(sim.money(), 20, "лом ушёл покупателю");
    assert_eq!(sim.tile(1, 0), -1, "а стройка его не перехватила");
    assert_eq!(sim.deal_of(), None, "сделка закрылась сама собой — донесли");
}

/// Бронь снимается по мере сдачи: донесённое сделке больше не нужно, и остаток
/// базы освобождается сразу, а не после закрытия сделки.
#[test]
fn a_partly_delivered_sale_frees_the_rest() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 3);
    sim.set_cost(0, 1);

    assert!(sim.trade(f, 0, 2, false), "продаём два лома из трёх");
    assert!(sim.add_blueprint(1, 0, 0), "третий — стройке");

    sim.tick_n(200);
    assert_eq!(sim.money(), 20, "два ушли покупателю");
    assert_eq!(sim.tile(1, 0), 0, "а третий достался стройке");
}

// --- боевой рулсет ---------------------------------------------------------

fn shipped() -> Sim {
    Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет")
}

/// **Главный тест шага: бесплатных денег не бывает.**
///
/// Если где-то купить дешевле, чем где-то продать, — появляется станок:
/// возить товар по кругу между фракциями или просто ждать дешёвой фазы и
/// сбывать в дорогую. Тогда вылазки и мастерская становятся необязательными, а
/// с ними и вся остальная игра. Проверяем **худший случай** — полное доверие
/// обеим сторонам, потому что именно оно и делает покупку дешевле всего, а
/// продажу дороже всего.
///
/// Перебор идёт по всем парам фракций (включая пару фракции с самой собой: фазы
/// одного прайса — тот же станок) и по всем фазам. Синтетика этого не увидит:
/// дыра живёт в цифрах контента, а не в коде.
#[test]
fn the_shipped_ruleset_leaves_no_arbitrage() {
    let mut sim = shipped();
    let count = sim.faction_spans().len();
    // Полное доверие всем: покупка дешевеет, продажа дорожает — худший случай.
    for f in 0..count {
        let span = sim.faction_spans()[f];
        sim.set_standing(f, span);
    }
    let period = sim.market_period();

    for buyer in 0..count {
        for seller in 0..count {
            for item in 0..sim.item_count() {
                // Минимум по фазам у покупки против максимума по фазам у продажи.
                let mut min_buy = None;
                let mut max_sell = None;
                for phase in 0..8u64 {
                    sim.set_tick(phase * period);
                    if let Some(b) = sim.quote(buyer, item, true) {
                        min_buy = Some(min_buy.map_or(b, |m: i32| m.min(b)));
                    }
                    if let Some(s) = sim.quote(seller, item, false) {
                        max_sell = Some(max_sell.map_or(s, |m: i32| m.max(s)));
                    }
                }
                let (Some(buy), Some(sell)) = (min_buy, max_sell) else {
                    continue; // кто-то из двоих этим не торгует
                };
                assert!(
                    buy >= sell,
                    "предмет {item}: у {buyer} купить можно за {buy}, а {seller} даёт {sell} — \
                     это станок для печати денег",
                );
            }
        }
    }
}

/// У каждой фракции есть чем торговать, иначе прайс — мёртвый контент.
#[test]
fn the_shipped_ruleset_trades_something_with_everyone() {
    let sim = shipped();
    for f in 0..sim.faction_spans().len() {
        assert!(
            (0..sim.item_count()).any(|i| sim.quote(f, i, true).is_some()),
            "фракция {f} не торгует ничем",
        );
        assert!(
            sim.market_lead(f) > 0,
            "у фракции {f} поставка мгновенна — задержка и есть половина решения",
        );
    }
}

/// Пост в палитре есть, он по карману и не заперт технологией: торговля — это
/// ранний мост, а не поздняя награда (§12.44).
#[test]
fn the_shipped_ruleset_has_a_post_to_trade_from() {
    let sim = shipped();
    let posts: Vec<i16> = sim.trade_post_tiles();
    assert_eq!(posts.len(), 1, "торговый пост в палитре ровно один");
    assert!(
        sim.tile_tech(posts[0]).is_none(),
        "пост не заперт технологией: без него у лишнего лома нет выхода до науки",
    );
}
