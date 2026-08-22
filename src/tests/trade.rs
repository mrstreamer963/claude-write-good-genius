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

/// Коридор со шлюзом в (3,1), торговым постом в (5,1) и **складом в (6,1)**;
/// фракция с рынком. Вернёт симуляцию и индекс фракции.
///
/// Склад здесь не украшение: с §12.69 наружу база отдаёт только учтённое, и
/// `put_scrap(6, 1, …)` кладёт товар именно туда — то есть в единственное место,
/// откуда его можно продать.
fn sim_with_market() -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_trade_post(2, true);
    sim.force_tile(5, 1, 2);
    sim.set_capacity(3, 500);
    sim.force_tile(6, 1, 3);
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
fn a_purchase_is_paid_at_once_and_arrives_in_the_post_cell() {
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
    assert_eq!(sim.scrap_at(5, 1), 3, "приехало кучей в ячейку поста");
    assert_eq!(
        sim.scrap_at(3, 1),
        0,
        "и не на шлюз — пост это место (§12.68)"
    );
    assert_eq!(sim.deal_of(), None, "сделка закрыта");
}

/// **Товар в ячейке базе ещё не принадлежит как склад** (§12.68): им нельзя
/// платить, пока его не вывезли.
///
/// Держит это нулевая `capacity` у поста, а не отдельная проверка: каждый вызов
/// `plan_spend` заранее фильтрует кучи по ёмкости клетки (§12.24). Дай ячейке
/// ёмкость — и она станет казной, а вывоз перестанет быть механикой.
#[test]
fn goods_in_the_post_cell_cannot_pay_for_a_hire() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(100);
    sim.set_capacity(3, 50);
    sim.force_tile(6, 1, 3); // склад в дальнем конце коридора
    let r = sim.set_recruit("newcomer", 0, &[(0, 2)], &[]);

    assert!(sim.trade(f, 0, 2, true));
    sim.tick_n(100);
    assert_eq!(sim.scrap_at(5, 1), 2, "товар приехал в ячейку");
    assert_eq!(
        sim.stock_of(0),
        (0, 2, 0),
        "и числится на полу, а не на складе",
    );
    assert!(!sim.hire(r), "платить лежащим в ячейке нельзя");

    sim.tick_n(400);
    assert!(sim.scrap_is_in_storage(), "коты вывезли его на склад");
    assert!(sim.hire(r), "и вот теперь им платят");
}

/// **Ячейка занята, пока её не разгребут** (§12.68) — то самое давление на
/// логистику, ради которого пост стал местом. Затор обратим своими силами, и
/// этим он отличается от невидимого счётчика, который оставалось только ждать.
#[test]
fn a_cell_stays_taken_until_the_goods_are_carried_off() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(1000);
    sim.set_capacity(3, 50);
    sim.force_tile(6, 1, 3); // склад в дальнем конце коридора

    assert!(sim.trade(f, 0, 2, true));
    sim.tick_n(100);
    assert_eq!(sim.scrap_at(5, 1), 2, "куча лежит в ячейке");
    assert_eq!(sim.deal_of(), None, "сама сделка давно закрыта");
    assert!(!sim.trade(f, 0, 1, true), "но ячейка ещё занята кучей");

    sim.tick_n(400);
    assert!(sim.scrap_is_in_storage(), "куча уехала");
    assert!(sim.trade(f, 0, 1, true), "и пост снова свободен");
}

/// Купленное ложится в ячейку, а не на склад. Дальше его разносит обычная
/// уборка, и отдельного пути у товара нет.
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

/// Сделок столько, сколько постов (§12.55). Один пост — одно окно, и решение
/// «на что я его трачу» остаётся: отмены у сделки по-прежнему нет (§12.44).
#[test]
fn one_post_holds_one_deal() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(1000);

    assert!(sim.trade(f, 0, 1, true));
    assert!(!sim.trade(f, 0, 1, true), "вторая сделка не принимается");
    assert!(!sim.trade(f, 0, 1, false), "и продать заодно тоже нельзя");
}

/// Второй пост — второе окно, и это первая причина его строить: до §12.55 он
/// был декорацией, как бездонный склад до `capacity` (§12.16).
#[test]
fn a_second_post_opens_a_second_deal() {
    let (mut sim, f) = sim_with_market();
    sim.set_money(1000);
    sim.force_tile(4, 1, 2); // второй торговый пост рядом с первым

    assert!(sim.trade(f, 0, 1, true));
    assert!(sim.trade(f, 0, 1, true), "второй пост держит вторую сделку");
    assert!(!sim.trade(f, 0, 1, true), "а третьей вставать некуда");
}

/// Пост стал **местом, но не рабочим местом** (§12.68): за ним никто не
/// работает, к нему только возят, — а возить не значит работать.
///
/// Проверяется это на продаже, потому что она к посту как раз зовёт: кот
/// приходит с грузом и уходит, но задачи у него всё время подвоз (`haul`), а не
/// работа за станком. Иначе счётность постов незаметно превратила бы их в
/// верстаки.
#[test]
fn a_post_never_becomes_a_workplace() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 4);
    assert!(sim.trade(f, 0, 4, false), "продаём — за грузом придут");

    for _ in 0..300 {
        sim.tick_n(1);
        for cat in ["a", "b"] {
            assert!(
                !sim.has_assignment(cat),
                "у поста нет работы, только подвоз",
            );
        }
    }
    assert_eq!(sim.money(), 40, "но товар к посту всё-таки привезли");
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
///
/// Платят при этом **разом и по отгрузке** (§12.68), а не поштучно по мере
/// сдачи: контейнер набирается, потом уезжает. Это делает продажу зеркалом
/// покупки — отдал сейчас, получил потом, — и оставляет один-единственный
/// момент, где считаются деньги.
#[test]
fn a_sale_is_carried_to_the_post_and_paid_on_shipment() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 4);

    assert!(sim.trade(f, 0, 4, false), "выставили на продажу");
    assert_eq!(sim.money(), 0, "пока не донесли — не платят");

    sim.tick_n(60);
    assert_eq!(
        sim.deal_of().map(|(_, _, _, _, done)| done),
        Some(4),
        "контейнер набит целиком",
    );
    assert_eq!(sim.scrap_total(), 0, "товар ушёл с базы в контейнер");
    assert_eq!(sim.money(), 0, "но денег ещё нет — груз не уехал");

    sim.tick_n(100); // срок отгрузки, тот же `lead`, что и у поставки
    assert_eq!(sim.money(), 40, "отгрузили и получили по курсу продажи");
    assert_eq!(sim.deal_of(), None, "сделка закрыта");
}

/// **Таймер продажи ждёт полного контейнера** (§12.68) — ровно как таймер
/// вылазки идёт с ухода отряда, а не с заявки (§12.22).
///
/// Отсюда и то, что состояния «срок вышел, а донесли половину» не существует, —
/// а значит, не нужно решать, платить ли за недовезённое.
#[test]
fn the_sale_timer_waits_for_a_full_container() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 4);

    assert!(sim.trade(f, 0, 4, false));
    // Пока набирают — таймер стоит на нуле, сколько бы ни прошло тиков.
    let mut partial = false;
    for _ in 0..60 {
        sim.tick();
        let Some((_, count, _, left, done)) = sim.deal_of() else {
            break;
        };
        if done < count {
            partial = true;
            assert_eq!(left, 0, "контейнер неполон — отгрузка не начиналась");
        }
    }
    assert!(partial, "набирали контейнер не мгновенно");

    let (_, count, _, left, done) = sim.deal_of().expect("сделка ещё идёт");
    assert_eq!(done, count, "контейнер полон");
    assert!(left > 0, "и вот теперь пошёл срок отгрузки");
}

/// **Проданное исчезает в момент сдачи и кучей не становится ни на тик.**
///
/// Иначе уборка увезла бы его обратно на склад: `mark_loose_scrap` метит всё,
/// что лежит на клетке без ёмкости, а ячейка поста ёмкости не имеет — и не
/// должна (§12.68). Содержимое контейнера живёт **счётчиком на сделке**, а не
/// кучами; это свойство ветки `HaulTo::Sale`, и его надо держать.
#[test]
fn sold_goods_never_pile_up_in_the_cell() {
    let (mut sim, f) = sim_with_market();
    sim.set_capacity(3, 50);
    sim.force_tile(6, 1, 3); // склад рядом с ломом
    sim.put_scrap(6, 1, 2);

    assert!(sim.trade(f, 0, 2, false));
    for _ in 0..200 {
        sim.tick();
        assert_eq!(sim.scrap_at(5, 1), 0, "в ячейке не задерживается ни тика");
        assert_eq!(sim.scrap_at(3, 1), 0, "и на шлюзе тоже");
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
    sim.tick_n(200); // расписание успело уйти, пока коты несли

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
    sim.tick_n(200);
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
    sim.tick_n(200);
    assert_eq!(sim.money(), 20, "и их унесли");
}

/// **Два поста не продают один и тот же лом дважды** (§12.50).
///
/// Пока пост был один, ворота «продать можно только то, что есть» держались
/// сами собой: второй заявке мешал занятый слот. §12.68 сделал постов много — и
/// заявка, считающая базу в одиночку, снова открывает сделку, которой нечего
/// везти: ровно ту вечную, ради которой бронь и заводилась.
#[test]
fn two_posts_cannot_sell_the_same_goods_twice() {
    let (mut sim, f) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй торговый пост
    sim.put_scrap(6, 1, 2);

    assert!(sim.trade(f, 0, 2, false), "весь лом выставлен на продажу");
    assert!(
        !sim.trade(f, 0, 1, false),
        "второму посту продавать уже нечего — он занят был бы навсегда"
    );

    sim.tick_n(300);
    assert_eq!(sim.money(), 20, "заплатили ровно за один лом дважды по 10");
    assert_eq!(sim.item_total(0), 0, "и лома на базе не осталось");
}

/// **Забронированное не раздаётся подвозом ни на тик** (§12.106).
///
/// Бронь считал только подъём (`work_hauls`), а раздача — нет, и это давало не
/// лишнюю ходку (§12.15), а вечный круг: кот получал `Haul`, подъём снимал его
/// тем же тиком, следующий тик повторял всё заново. Кот при этом не двигался,
/// но свободным его не видел никто — в том числе `assign_nap`, и дремать он не
/// уходил никогда.
#[test]
fn goods_booked_for_sale_are_never_handed_out_to_a_site() {
    const OTHER: i32 = 2;
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 2);
    assert!(sim.trade(f, 0, 2, false), "весь лом выставлен на продажу");

    sim.set_cost(OTHER as i16, 2);
    sim.add_blueprint(4, 1, OTHER);
    sim.tick_n(3);

    assert!(!sim.has_haul("a"), "площадке этот лом уже не принадлежит");
    assert_eq!(
        sim.job_of("a").0,
        "",
        "и кот свободен, а не занят вхолостую"
    );

    sim.tick_n(300);
    assert_eq!(sim.money(), 20, "лом уехал покупателю");
}

/// Бронь тает по мере сдачи, и освободившееся место второй пост берёт честно:
/// правило запрещает продавать несуществующее, а не вторую сделку по предмету.
#[test]
fn a_second_post_sells_what_the_first_left_alone() {
    let (mut sim, f) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй торговый пост
    sim.put_scrap(6, 1, 3);

    assert!(sim.trade(f, 0, 2, false), "два лома — первому посту");
    assert!(
        sim.trade(f, 0, 1, false),
        "третий свободен, его берёт второй"
    );
    assert!(!sim.trade(f, 0, 1, false), "а четвёртого лома на базе нет");
}

/// **Неучтённое не продаётся** (§12.69): лом с пола годится на стройку внутри
/// базы, но наружу база отдаёт только то, что убрано на склад.
///
/// Это и есть вся граница «внутрь/наружу» одним прогоном: сначала отказ, потом
/// уборка, потом та же заявка проходит. Ничего, кроме места, не изменилось.
#[test]
fn unsorted_goods_cannot_be_sold_until_they_reach_storage() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(4, 1, 2); // пол посреди коридора, не склад

    assert!(
        !sim.trade(f, 0, 2, false),
        "на полу лежит, а продать нельзя — учтённого нет"
    );

    // `scrap_is_in_storage` тут не годится: пока лом в лапах, куч нет вовсе, и
    // «все кучи на складе» верно впустую. Ждём именно учтённого.
    for _ in 0..300 {
        sim.tick_n(1);
        if sim.stock_of(0).0 == 2 {
            break;
        }
    }
    assert_eq!(sim.stock_of(0).0, 2, "коты убрали лом на склад");
    assert!(sim.trade(f, 0, 2, false), "теперь он учтён — и продаётся");
}

/// Груз в лапах тоже неучтён (§12.69): кот поднял его с пола, на складе его
/// нет, и продать его нельзя — как нельзя и заплатить им за найм.
#[test]
fn goods_in_paws_are_not_sellable() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(4, 1, 2);
    for _ in 0..200 {
        sim.tick_n(1);
        if sim.carrying_of("a") + sim.carrying_of("b") > 0 {
            break;
        }
    }
    assert!(
        sim.carrying_of("a") + sim.carrying_of("b") > 0,
        "кто-то из котов поднял лом",
    );

    assert!(
        !sim.trade(f, 0, 2, false),
        "в лапах — значит ещё не на складе"
    );
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

/// Груз в лапах уже обещан сделке: считать его свободным нельзя (§12.50).
#[test]
fn goods_already_in_paws_cannot_be_sold_again() {
    let (mut sim, f) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй торговый пост
    sim.put_scrap(6, 1, 2);

    assert!(sim.trade(f, 0, 2, false), "весь лом выставлен на продажу");
    for _ in 0..200 {
        if sim.carrying_of("a") + sim.carrying_of("b") > 0 {
            break;
        }
        sim.tick_n(1);
    }
    assert!(
        sim.carrying_of("a") + sim.carrying_of("b") > 0,
        "лом в лапах"
    );

    assert!(
        !sim.trade(f, 0, 2, false),
        "лом в лапах уже продан — второй раз его не продать"
    );
}

/// **Главное число падает один раз — на заявке — и держится до отгрузки**
/// (§12.69, §12.53).
///
/// Товар для продажи кот берёт со склада, поэтому при обычном счёте он на время
/// ходки перетекал из главного числа в серое: то прыгнет вверх, то упадёт
/// обратно, и вдобавок сообщит про уже проданное, что оно «валяется». Груз под
/// сделку числится складским, и качелей нет.
#[test]
fn the_headline_count_drops_once_and_then_holds() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 10);
    let free = |sim: &mut Sim| {
        let (stored, _, booked) = sim.stock_of(0);
        stored - booked
    };

    assert_eq!(free(&mut sim), 10, "до сделки — всё учтённое");
    assert!(sim.trade(f, 0, 4, false), "продаём четыре");
    assert_eq!(free(&mut sim), 6, "на заявке число упало разом");

    // Дальше — вся ходка целиком: подъём со склада, дорога, сдача в контейнер.
    let mut carried = false;
    for _ in 0..300 {
        sim.tick_n(1);
        carried |= sim.carrying_of("a") + sim.carrying_of("b") > 0;
        assert_eq!(free(&mut sim), 6, "и держится, пока товар везут");
        assert_eq!(sim.stock_of(0).1, 0, "в «валяется» проданное не попадает");
        if sim.deal_of().is_none() {
            break;
        }
    }
    assert!(carried, "лом действительно несли в лапах");
    assert_eq!(sim.deal_of(), None, "сделка отгружена");
    assert_eq!(free(&mut sim), 6, "и после отгрузки число то же");
}

/// **Показанное и есть продаваемое — в том числе посреди ходки** (§12.69).
///
/// Груз под сделку числится учтённым (§12.53), и это законный повод заподозрить
/// дыру: пока кот его несёт, он виден в главном числе — не выйдет ли продать его
/// второй раз? Не выйдет: в `stored` лапы прибавлены, но из брони они больше не
/// вычтены, и слагаемые сокращаются. Тест берёт самый опасный момент — груз в
/// лапах, сделка открыта — и сверяет обе стороны: на штуку больше показанного
/// ворота не пропускают, ровно показанное берут.
#[test]
fn what_the_headline_shows_is_exactly_what_sells() {
    let (mut sim, f) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй пост — под проверочную сделку
    sim.put_scrap(6, 1, 10);

    assert!(sim.trade(f, 0, 4, false), "продаём четыре из десяти");
    for _ in 0..300 {
        sim.tick_n(1);
        if sim.carrying_of("a") + sim.carrying_of("b") > 0 {
            break;
        }
    }
    let paws = sim.carrying_of("a") + sim.carrying_of("b");
    assert!(paws > 0, "груз в лапах — самый опасный момент");

    let (stored, _, booked) = sim.stock_of(0);
    let shown = stored - booked;
    assert_eq!(shown, 6, "шапка обещает шесть");

    assert!(
        !sim.trade(f, 0, shown + 1, false),
        "на штуку больше обещанного ворота не пропускают",
    );
    assert!(
        sim.trade(f, 0, shown, false),
        "а ровно обещанное — берут, и это не второй раз тот же лом",
    );
    assert_eq!(
        sim.stock_of(0).0 - sim.stock_of(0).2,
        0,
        "теперь продано всё"
    );
}

/// Груз, унесённый на стройку, тоже не продать (§12.69) — но по другой причине,
/// чем груз под сделку.
///
/// Площадка ничего не бронирует (§12.15): материал уходит из продаваемого не в
/// момент разметки, а в момент, когда кот поднял его со склада. С этого мига он
/// неучтён — числится в сером «валяется» и воротам не виден. Асимметрия с
/// продажей намеренная: там бронь, здесь ходка.
#[test]
fn goods_carried_to_a_building_site_are_not_sellable() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 10);
    sim.set_cost(0, 4); // пол стоит четыре лома
    assert!(sim.add_blueprint(1, 0, 0), "разметили стройку в стене");

    for _ in 0..300 {
        sim.tick_n(1);
        if sim.carrying_of("a") + sim.carrying_of("b") > 0 {
            break;
        }
    }
    let paws = sim.carrying_of("a") + sim.carrying_of("b");
    assert!(paws > 0, "кот понёс материал на площадку");

    let (stored, loose, booked) = sim.stock_of(0);
    assert_eq!(loose, paws, "унесённое на стройку числится неучтённым");
    assert_eq!(booked, 0, "площадка ничего не бронирует");
    assert_eq!(stored, 10 - paws, "и из учтённого оно уже вышло");

    assert!(
        !sim.trade(f, 0, 10, false),
        "все десять продать уже нельзя — часть в лапах строителя",
    );
    assert!(
        sim.trade(f, 0, stored, false),
        "а оставшееся учтённое — можно"
    );
}

// --- порог автопродажи -----------------------------------------------------
//
// Правило замыкает набор автоматики (§12.87): «убирать сам», «беречь себя»,
// порог производства (§12.65) и автовылазка (§12.67) уже были. Форма общая
// (§12.64) — ресурс-правило плюс повтор того же клика, — а своего здесь ровно
// две вещи, и обе про счёт: заявку тормозит **ячейка**, а не число постов, и
// в одном выражении встречаются **обе формы брони** (§12.50).

/// Базовый случай: всё, что сверх порога, уезжает покупателю само.
#[test]
fn a_surplus_threshold_sells_what_the_base_does_not_need() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 10);

    assert!(sim.set_sale(f, 0, 4), "правило принято");
    sim.tick();
    assert_eq!(
        sim.sales(),
        vec![(f, 0, 6)],
        "заявка на весь излишек разом, а не по штуке",
    );

    sim.tick_n(300);
    assert_eq!(sim.money(), 60, "шесть ушли по курсу продажи");
    assert_eq!(sim.item_total(0), 4, "а порог остался на базе");
    assert_eq!(sim.deals_open(), 0, "и второй заявки не было — излишка нет");
}

/// Порог — это «держать», а не «продавать»: ниже него правило молчит, сколько
/// бы тиков ни прошло.
#[test]
fn a_threshold_below_the_stock_stays_quiet() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 3);

    assert!(sim.set_sale(f, 0, 4));
    sim.tick_n(50);
    assert_eq!(
        sim.deals_open(),
        0,
        "трёх штук на порог в четыре не хватает"
    );
}

/// **Первая ловушка: слот — это ячейка, а не пост** (§12.68).
///
/// Постов может быть сколько угодно, а занята ячейка бывает не только сделкой,
/// но и непойманным привозом. Правило, считающее посты, открыло бы сделку,
/// которой некуда лечь, — а отменить её нечем (§12.44).
#[test]
fn a_threshold_waits_for_a_free_cell() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 10);

    assert!(sim.trade(f, 0, 2, false), "единственную ячейку занял игрок");
    assert!(sim.set_sale(f, 0, 4));
    sim.tick_n(20);
    assert_eq!(
        sim.deals_open(),
        1,
        "правило ждёт, а не лезет второй сделкой"
    );

    sim.tick_n(300);
    assert_eq!(sim.item_total(0), 4, "ячейка освободилась — излишек ушёл");
    assert_eq!(sim.money(), 60, "и заплатили за все восемь");
}

/// **Вторая ловушка, сторона первая: излишек считается без скидки на лапы**
/// (§12.50).
///
/// Пока носильщик несёт товар к посту, он числится и в добре базы (`Carrying`),
/// и в долге сделки (`owed`). Возьми правило `booked` — лапы вычлись бы дважды,
/// и оно выставило бы на продажу уже проданное: вторая сделка на тот же лом,
/// закрыть которую нечем.
#[test]
fn goods_already_promised_are_not_offered_twice() {
    let (mut sim, f) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй пост: ячейка под ошибку есть
    sim.put_scrap(6, 1, 10);

    assert!(sim.set_sale(f, 0, 4));
    // Ловим именно тот тик, когда товар в лапах: там ошибка и живёт.
    let mut carried = false;
    for _ in 0..300 {
        sim.tick();
        carried |= sim.carrying_of("a") + sim.carrying_of("b") > 0;
        assert!(
            sim.deals_open() <= 1,
            "второй заявки на тот же лом быть не может"
        );
    }
    assert!(carried, "товар и правда несли лапами");
    assert_eq!(sim.money(), 60, "продали ровно излишек, а не дважды");
    assert_eq!(sim.item_total(0), 4);
}

/// **Излишек считается по складу, и пол в него не входит** (§12.69, §12.91).
///
/// Учтённое — единственное, чем база распоряжается наружу, поэтому и порог
/// меряется им: лежащее на полу правило своим не считает, но и не теряет —
/// уборка доносит его до склада, и следующая сделка забирает уже его.
#[test]
fn a_threshold_offers_only_what_storage_holds() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 5); // склад
    sim.put_scrap(2, 1, 5); // пол посреди коридора

    assert!(sim.set_sale(f, 0, 4));
    sim.tick();
    assert_eq!(
        sim.sales(),
        vec![(f, 0, 1)],
        "на складе пять при пороге четыре — лишняя ровно одна, пол не в счёт",
    );

    sim.tick_n(400);
    assert_eq!(
        sim.item_total(0),
        4,
        "убранное с пола стало учтённым и ушло следующими сделками"
    );
    assert_eq!(sim.money(), 60, "и всего продано ровно шесть");
}

/// Снятое правило открытую сделку **не отзывает**: у автопродажи нет уборки за
/// собой, и это единственное правило, у которого её нет (ср. §12.65).
///
/// Причина не в лени: сделка необратима (§12.44), и «отменить» тут нечего —
/// закроет её обычная отгрузка.
#[test]
fn clearing_the_threshold_leaves_the_open_deal_alone() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 10);

    assert!(sim.set_sale(f, 0, 4));
    sim.tick();
    assert_eq!(sim.deals_open(), 1);

    assert!(sim.set_sale(f, 0, 0), "порог снят");
    assert_eq!(sim.sale_of(0), None, "правила больше нет");
    sim.tick_n(300);
    assert_eq!(sim.money(), 60, "а начатая сделка дошла до конца");
    assert_eq!(sim.item_total(0), 4, "и новых заявок не появилось");
}

/// Адресата называет игрок, но фракция обязана этим предметом торговать: чем не
/// торгует — на то у неё и правила нет. Молча не работающее правило читалось бы
/// как поломка (§12.53).
#[test]
fn a_threshold_needs_the_faction_to_trade_that_item() {
    let (mut sim, f) = sim_with_market();

    assert!(!sim.set_sale(f, 1, 5), "деталью эти не торгуют");
    assert!(!sim.set_sale(f + 1, 0, 5), "и фракции такой нет");
    assert_eq!(sim.sale_of(1), None);
}

/// **Правило на предмет одно** (§12.88): второе перезаписывает первое вместе с
/// покупателем.
///
/// До §12.88 правило висело на паре «фракция + предмет», и у лома их выходило
/// два: включить можно было оба, а излишек молча доставался первому по палитре.
/// Порядок был честно детерминирован — но игроку неоткуда его прочесть, то есть
/// интерфейс предлагал выбор, который сам же и разрешал.
#[test]
fn a_second_rule_on_the_same_item_replaces_the_first() {
    let (mut sim, first) = sim_with_market();
    sim.force_tile(4, 1, 2); // второй пост: ячейка под «вторую» сделку есть
    let second = sim.set_faction(100);
    sim.set_market(second, 100, 40, 25, 0);
    sim.set_prices(second, 0, &[10]);
    sim.put_scrap(6, 1, 10);

    assert!(sim.set_sale(first, 0, 4), "сначала сбывать первой стороне");
    assert!(sim.set_sale(second, 0, 4), "потом передумали — второй");
    assert_eq!(
        sim.sale_of(0),
        Some((second, 4)),
        "правило одно, и оно про нового покупателя",
    );

    sim.tick();
    assert_eq!(
        sim.sales(),
        vec![(second, 0, 6)],
        "излишек ушёл выбранной стороне, и сделка ровно одна",
    );
}

/// Смена покупателя — это не сброс порога: число остаётся, меняется адресат.
/// Иначе «передумал, кому продавать» стоило бы игроку набора числа заново.
#[test]
fn switching_the_buyer_keeps_the_threshold() {
    let (mut sim, first) = sim_with_market();
    let second = sim.set_faction(100);
    sim.set_market(second, 100, 40, 25, 0);
    sim.set_prices(second, 0, &[10]);

    assert!(sim.set_sale(first, 0, 20));
    assert!(sim.set_sale(second, 0, 20), "тот же порог, другая сторона");
    assert_eq!(sim.sale_of(0), Some((second, 20)));
}

/// Правила на **разные** предметы живут порознь и друг друга не трогают: ключ —
/// предмет, и один предмет о другом ничего не знает.
#[test]
fn rules_on_different_items_do_not_collide() {
    let (mut sim, f) = sim_with_market();
    sim.set_prices(f, 1, &[20]); // этой фракции продают и деталь

    assert!(sim.set_sale(f, 0, 4));
    assert!(sim.set_sale(f, 1, 7));
    assert_eq!(sim.sale_of(0), Some((f, 4)), "лом на месте");
    assert_eq!(sim.sale_of(1), Some((f, 7)), "и деталь рядом");
}

// --- ёмкость контейнера ----------------------------------------------------
//
// Пост считает **сделки**, а не товар (§12.55, §12.68), и до §12.90 этого было
// довольно: заявку игрок набивал кликами по пять. Автопродажа (§12.87) кликов
// не делает — она грузила в одну ячейку весь излишек разом, и второй пост
// становился не нужен. Контейнер переводит счётность поста из «сколько сделок»
// в «сколько товара».

/// Больше, чем влезает, в ячейку не кладут — и заявку отклоняют, а не срезают
/// молча: игрок просил сделку на сто, а получил бы на двадцать пять.
#[test]
fn a_deal_bigger_than_the_container_is_refused() {
    let (mut sim, f) = sim_with_market();
    sim.set_lot(2, 25);
    sim.put_scrap(6, 1, 100);

    assert!(
        !sim.trade(f, 0, 26, false),
        "в контейнер влезает двадцать пять"
    );
    assert!(
        sim.trade(f, 0, 25, false),
        "а ровно двадцать пять — влезает"
    );
    assert_eq!(sim.deals_open(), 1);
}

/// Предел общий на обе стороны: контейнер — это место, а не направление
/// (§12.90). Купить вагон через одну ячейку так же нельзя, как продать.
#[test]
fn the_container_caps_purchases_too() {
    let (mut sim, f) = sim_with_market();
    sim.set_lot(2, 25);
    sim.set_money(100000);

    assert!(!sim.trade(f, 0, 26, true), "покупка тем же контейнером");
    assert!(sim.trade(f, 0, 25, true));
}

/// **Правило грузит контейнер, а не вагон** (§12.90): излишек в сто штук уходит
/// партиями, а не одной сделкой.
///
/// Это и есть закрытый читкод: до §12.90 автопродажа вывозила через одну ячейку
/// сколько угодно, и число постов не значило ничего.
#[test]
fn the_rule_ships_one_container_at_a_time() {
    let (mut sim, f) = sim_with_market();
    sim.set_lot(2, 25);
    sim.put_scrap(6, 1, 100);

    assert!(sim.set_sale(f, 0, 4));
    sim.tick();
    assert_eq!(
        sim.sales(),
        vec![(f, 0, 25)],
        "в первой сделке ровно контейнер, а не весь излишек",
    );

    // Ячейка занята, пока сделка не уедет: второй партии ждать своей очереди.
    sim.tick_n(20);
    assert_eq!(sim.deals_open(), 1, "второй сделке некуда лечь");
}

/// Второй пост — это второй контейнер, то есть **вдвое больше товара за то же
/// время**. Ради этого предел и заведён: строить посты должно быть зачем.
#[test]
fn a_second_post_doubles_what_the_rule_can_ship() {
    let (mut sim, f) = sim_with_market();
    sim.set_lot(2, 25);
    sim.force_tile(4, 1, 2); // второй пост
    sim.put_scrap(6, 1, 100);

    assert!(sim.set_sale(f, 0, 4));
    sim.tick_n(2); // по сделке за тик: правило берёт по свободной ячейке
    assert_eq!(
        sim.sales(),
        vec![(f, 0, 25), (f, 0, 25)],
        "две ячейки — два контейнера",
    );
}

/// Без предела (ноль) всё как было: синтетические схемы о контейнере не знают,
/// и сделки там любого размера — то же правило нулей, что у `capacity` и
/// `comms`.
#[test]
fn a_zero_container_means_no_limit() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 500);

    assert!(
        sim.trade(f, 0, 500, false),
        "предела нет — берут сколько дали"
    );
}

/// Боевой рулсет обязан назвать предел: ноль здесь означал бы, что механика
/// выключена молча, а автопродажа снова вывозит склад через одну ячейку.
#[test]
fn the_shipped_ruleset_caps_its_container() {
    let sim = shipped();
    for tile in sim.trade_post_tiles() {
        assert!(
            sim.lot_of(tile) > 0,
            "у торгового поста {tile} контейнер без предела — автопродажа обходит счёт постов",
        );
    }
}

/// Новая партия начинает с непустым избранным (§12.112): тем же списком
/// отобраны строки шапки главного экрана, и пустой он показал бы там всю
/// палитру — то есть механика была бы выключена молча, как предел контейнера
/// нулём. Кто именно закреплён — контент, поэтому проверяем не имена, а то,
/// что закреплённое есть и это не вся палитра.
#[test]
fn the_shipped_ruleset_pins_something_to_the_header() {
    let sim = shipped();
    let items = sim.item_count();
    let pinned = (0..items).filter(|&i| sim.is_favorite(i)).count();
    assert!(pinned > 0, "шапка боевой партии показала бы всю палитру");
    assert!(
        pinned < items,
        "закреплено всё — отбор ничего не отбирает, шапка та же, что без него",
    );
}

/// **Порог меряет склад, а не всё добро базы** (§12.91).
///
/// Наблюдение с игры: порог 500, на складе 200, в кучах 192 — а коты набивают
/// контейнер. Правило считало «на базе» вместе с полом и лапами, а тратит
/// только учтённое (§12.69), поэтому и сливало склад ниже порога: база
/// оставалась «на пределе» за счёт неубранного, а продаваемого не оставалось.
#[test]
fn the_threshold_measures_storage_not_the_floor() {
    let (mut sim, f) = sim_with_market();
    sim.put_scrap(6, 1, 400); // склад
    sim.put_scrap(2, 1, 300); // пол: базе принадлежит, но наружу не уходит

    assert!(sim.set_sale(f, 0, 500), "держать пятьсот");
    sim.tick();
    assert_eq!(
        sim.deals_open(),
        0,
        "на складе четыреста — продавать нечего, сколько бы ни лежало на полу",
    );
}

// --- ворота автоматики -----------------------------------------------------
//
// Правила игрока открываются наукой (§12.93), а имена технологий живут в
// рулсете: в синтетической схеме `AutoRules` пуст, значит ворот нет вовсе — и
// все тесты выше про это не знают. Здесь ворота включаются вручную.

/// Без технологии правило не поставить, с ней — поставить.
#[test]
fn a_sale_rule_needs_its_technology() {
    let (mut sim, f) = sim_with_market();
    sim.set_auto_gates("logistics", "", "");

    assert!(
        !sim.set_sale(f, 0, 20),
        "технологии нет — правило не ставится"
    );
    assert_eq!(sim.sale_of(0), None);

    sim.set_tech("logistics");
    assert!(sim.set_sale(f, 0, 20), "изучили — можно");
    assert_eq!(sim.sale_of(0), Some((f, 20)));
}

/// **Снятие проходит и без технологии.** Иначе правило, поставленное до правки
/// рулсета, стало бы несбрасываемым — запертая отмена это не ворота, а ловушка.
#[test]
fn clearing_a_rule_needs_no_technology() {
    let (mut sim, f) = sim_with_market();
    sim.set_tech("logistics");
    sim.set_auto_gates("logistics", "", "");
    assert!(sim.set_sale(f, 0, 20));

    sim.forget_techs();
    assert!(sim.set_sale(f, 0, 0), "снять можно всегда");
    assert_eq!(sim.sale_of(0), None);
}

/// Пустые ворота — это «правило доступно сразу»: так живут все синтетические
/// миры, и так же будет жить рулсет, в котором ветки автоматики нет.
#[test]
fn empty_gates_open_everything() {
    let (mut sim, f) = sim_with_market();

    assert!(sim.set_sale(f, 0, 20), "ворот нет — правило ставится");
}

/// **Три флага не перепутаны местами.** Снапшот на хосте не собрать, поэтому
/// панель и этот тест спрашивают одно выражение (`auto_gates_open`) — иначе
/// перепутанные поля дали бы молча не работающую строку, и заметить это можно
/// было бы только в игре (§12.93).
#[test]
fn each_automation_gate_answers_for_itself() {
    let (mut sim, _) = sim_with_market();
    sim.set_auto_gates("logistics", "planning", "callsigns");
    assert_eq!(sim.auto_gates_open(), (false, false, false), "закрыто всё");

    sim.set_tech("planning");
    assert_eq!(
        sim.auto_gates_open(),
        (false, true, false),
        "изучили «Автопроизводство» — открылось только производство",
    );

    sim.set_tech("logistics");
    assert_eq!(sim.auto_gates_open(), (true, true, false), "и сбыт");
}

// --- закладки склада (§12.100) ---------------------------------------------

/// Избранное **ворот не спрашивает вовсе**.
///
/// Порядок строк в окне — не механика: закрепить наверху можно и то, чем не
/// торгует никто. Ровно этот случай и есть у аптечки в боевом рулсете: под ней
/// стоит порог производства, следить за её запасом надо, а покупателя у неё
/// нет. Тикер там не поставить, а избранное — да, и это два разных вопроса.
#[test]
fn an_untraded_item_can_be_a_favorite() {
    let (mut sim, f) = sim_with_market();
    let untraded = 1; // прайс в `sim_with_market` заведён только на предмет 0

    assert!(sim.set_favorite(untraded, true), "избранное берёт любой");
    assert!(sim.is_favorite(untraded));
    assert!(
        !sim.set_ticker(untraded, f, true),
        "а тикер — нет: торговать по нему нечем",
    );
    assert_eq!(sim.ticker_of(untraded), None);
}

/// Тикер требует стороны, которая **правда торгует** этим предметом — те же
/// ворота, что у порога автопродажи (§12.87): иначе в ленте встала бы строка с
/// кнопками, которые фасад отклонит, а молчащая кнопка читается как поломка.
#[test]
fn a_ticker_needs_a_side_that_trades_the_item() {
    let (mut sim, f) = sim_with_market();

    assert!(sim.set_ticker(0, f, true), "этим предметом сторона торгует");
    assert_eq!(sim.ticker_of(0), Some(f));
    assert!(
        !sim.set_ticker(0, f + 1, true),
        "а такой фракции в рулсете нет вовсе",
    );
    assert_eq!(sim.ticker_of(0), Some(f), "и прежний тикер не тронут");
}

/// **Тикер на предмет один, и смена стороны — это правка того же решения**, а
/// не второй тикер (§12.88). На паре «фракция + предмет» их выходило бы по два
/// на лом, и лента показывала бы одну строку дважды.
#[test]
fn a_second_ticker_on_the_same_item_rewrites_the_side() {
    let (mut sim, f) = sim_with_market();
    // Вторая сторона с тем же товаром: развилка §12.43 в миниатюре.
    let other = sim.set_faction(100);
    sim.set_market(other, 100, 40, 25, 0);
    sim.set_prices(other, 0, &[7]);

    assert!(sim.set_ticker(0, f, true));
    assert!(
        sim.set_ticker(0, other, true),
        "перевесили на другую сторону"
    );
    assert_eq!(sim.ticker_of(0), Some(other));
    assert_eq!(
        sim.world.resource::<Tickers>().0.len(),
        1,
        "тикеров на предмет по-прежнему один",
    );
}

/// **Снятие ворот не спрашивает** — по тому же доводу, по которому их не
/// спрашивает снятие правила (§12.93): запертая отмена оставила бы тикер
/// несбрасываемым. Ключ у тикера — предмет, поэтому снимается он по предмету,
/// какую бы сторону ни назвали.
#[test]
fn dropping_a_ticker_needs_no_gate() {
    let (mut sim, f) = sim_with_market();
    assert!(sim.set_ticker(0, f, true));

    assert!(
        sim.set_ticker(0, f + 1, false),
        "сторона несуществующая, а снять всё равно можно",
    );
    assert_eq!(sim.ticker_of(0), None);
}
