//! Лента новостей: что открылось и что закрылось (§12.120).
//!
//! До неё игрок узнавал о новом заказе, кандидате или теме, только заглянув в
//! список руками. Считает ленту **наблюдатель** в `Sim::tick`, после цепочки, —
//! дословно `check_goals` (§12.58): крючков в местах, где ворота проверяются,
//! нет ни одного.
//!
//! Проверяется здесь ровно три вещи, и все три — про **границу новости**:
//! стартовая доступность новостью не считается, сделанное игроком не считается
//! тоже, а закрытие — считается.

use super::*;

const CORE: &str = include_str!("../../assets/rulesets/core.yaml");

/// Мир на две клетки: механики здесь не нужны, нужны только ворота.
fn sim_bare() -> Sim {
    sim_from(&["####", "#a.#", "####"])
}

#[test]
fn starting_availability_is_not_news() {
    let mut sim = sim_bare();
    // Открытый с самого начала кандидат: известности он не требует.
    sim.set_recruit("nail", 0, &[], &[]);
    sim.tick_n(5);
    // Партия начинается с открытой первой ступени, и объявлять её нечего:
    // иначе игрока встречала бы стопка тикеров на первом же тике.
    assert!(
        sim.news().is_empty(),
        "стартовая доступность — не новость: {:?}",
        sim.news()
    );
}

#[test]
fn a_recruit_opens_when_fame_grows() {
    let mut sim = sim_bare();
    let nail = sim.set_recruit("nail", 30, &[], &[]);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "до порога говорить не о чем");

    sim.set_fame(30);
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Recruit, nail, true)]);

    // Новость об одном и том же не повторяется каждым тиком: наблюдатель
    // отмечает **перемену**, а не состояние.
    sim.tick_n(20);
    assert_eq!(sim.news().len(), 1, "новость сказана один раз");
}

#[test]
fn a_topic_opens_when_its_prerequisite_is_learned() {
    let mut sim = sim_bare();
    let second = sim.set_topic("second", 0, 10, &[], &["first"]);
    sim.tick_n(2);
    assert!(sim.news().is_empty());

    sim.set_tech("first");
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Topic, second, true)]);
}

#[test]
fn learning_a_topic_is_not_news() {
    let mut sim = sim_bare();
    sim.set_topic("first", 0, 10, &[], &[]);
    sim.tick_n(2);
    // Тема доведена до конца: она изучена, но **не закрылась**. Иначе лента
    // объявила бы новостью победу, которую игрок только что одержал сам.
    sim.set_tech("first");
    sim.tick_n(2);
    assert!(
        sim.news().is_empty(),
        "изученная тема — не закрывшаяся: {:?}",
        sim.news()
    );
}

#[test]
fn a_recipe_opens_with_its_technology() {
    let mut sim = sim_bare();
    let suit = sim.set_recipe(10, &[(0, 2)], &[(0, 1)], &["fabrics"]);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "без технологии рецепта нет вовсе");

    sim.set_tech("fabrics");
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Recipe, suit, true)]);
}

#[test]
fn a_tile_opens_with_its_technology() {
    let mut sim = sim_bare();
    // Второй тайл палитры за технологией: первый — пол самой схемы.
    sim.set_tile_tech(1, "masonry");
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "без технологии постройки нет вовсе");

    // С §12.126 закрытая постройка из палитры пропадает совсем, и её появление
    // видно только лентой: молча удлинившийся список игрок не заметит.
    sim.set_tech("masonry");
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Tile, 1, true)]);
}

#[test]
fn a_tile_never_closes() {
    let mut sim = sim_bare();
    sim.set_tile_tech(1, "masonry");
    sim.set_tech("masonry");
    sim.tick_n(2);
    let after_open = sim.news().len();
    sim.tick_n(50);
    // Технологии не забываются (§12.18): постройка умеет появиться и не умеет
    // исчезнуть — ровно на этом свойстве и держится право её прятать.
    assert_eq!(sim.news().len(), after_open, "постройка закрыться не может");
}

#[test]
fn a_recipe_never_closes() {
    let mut sim = sim_bare();
    sim.set_recipe(10, &[(0, 2)], &[(0, 1)], &["fabrics"]);
    sim.set_tech("fabrics");
    sim.tick_n(2);
    let after_open = sim.news().len();

    // Мастерскую снесли, склад опустел — рецепт от этого не закрывается: его
    // ворота это одна технология, а технологии не забываются (§12.18). «Пока
    // нечем» новостью не считается ровно так же, как у темы без лаборатории.
    sim.tick_n(20);
    assert_eq!(
        sim.news().len(),
        after_open,
        "у рецепта закрытий не бывает: {:?}",
        sim.news()
    );
}

#[test]
fn hiring_a_recruit_is_not_news() {
    let mut sim = sim_bare();
    sim.set_recruit("nail", 0, &[], &[]);
    // Новичку неоткуда взяться без шлюза (§12.24) — в схеме его нет.
    sim.set_gate(1, true);
    sim.force_tile(2, 1, 1);
    sim.tick_n(2);
    assert!(sim.hire(0), "нанять открытого кандидата можно");
    sim.tick_n(2);
    // То же, что с темой: найм — это действие игрока, а не перемена в мире.
    assert!(
        sim.news().is_empty(),
        "нанятый кандидат — не закрывшийся: {:?}",
        sim.news()
    );
}

#[test]
fn a_raid_opens_when_fame_grows() {
    let mut sim = sim_bare();
    let job = sim.set_mission(1, 10, &[]);
    sim.set_mission_fame(job, 0, 60);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "до порога заказа в списке нет вовсе");

    // Известность открывает — и ровно в этот тик заказ появляется карточкой в
    // штабе (§12.79). Лента говорит о появлении, потому что говорит о том же,
    // что видно на экране.
    sim.set_fame(60);
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Raid, job, true)]);
}

#[test]
fn a_patron_turning_away_is_not_news() {
    let mut sim = sim_bare();
    let faction = sim.set_faction(100);
    let job = sim.set_mission(1, 10, &[]);
    sim.set_mission_needs(job, &[(faction, 20)]);
    sim.set_standing(faction, 30);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "открыт с начала — молчим");

    // Репутация — единственная знаковая шкала (§12.43), но заказ она из списка
    // **не убирает**: карточка остаётся с причиной словом, «нужна Полиция +20»
    // (§12.79). Объявить тут «больше не откликается» значило бы сказать про то,
    // что у игрока на экране, — та же ошибка, что новость про нанятого кота.
    sim.set_standing(faction, -10);
    sim.tick_n(2);
    assert!(
        sim.news().is_empty(),
        "недоверие — причина на карточке, а не новость: {:?}",
        sim.news()
    );

    // И обратно: вернувшееся доверие тоже не новость — заказ никуда не девался.
    sim.set_standing(faction, 40);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "и обратно молчим: {:?}", sim.news());
}

#[test]
fn a_rescue_raid_closes_when_the_captive_comes_home() {
    let mut sim = sim_bare();
    let job = sim.set_rescue_mission(1, 10, 0);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "спасать некого — и карточки нет");

    // Вылазка за своим — единственная, которая и правда **пропадает** из
    // списка: без пленных у неё нет цели (§12.40). Появление и пропажа — это и
    // есть то, о чём лента говорит.
    sim.set_captive("a", true);
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Raid, job, true)]);

    sim.set_captive("a", false);
    sim.tick_n(1);
    assert_eq!(
        sim.news(),
        vec![(NewsKind::Raid, job, true), (NewsKind::Raid, job, false)]
    );
}

#[test]
fn the_feed_never_grows_past_its_cap() {
    let mut sim = sim_bare();
    let job = sim.set_mission(1, 10, &[]);
    sim.set_mission_fame(job, 0, 60);
    sim.tick_n(2);

    // Лента — не журнал §12.58: на неё смотрит человек, и прочитанная новость
    // ценности не имеет. Поэтому старое вытесняется, а не копится.
    for i in 0..NEWS_MAX + 10 {
        sim.set_fame(if i % 2 == 0 { 0 } else { 60 });
        sim.tick_n(1);
    }
    assert_eq!(sim.news().len(), NEWS_MAX);
}

#[test]
fn the_shipped_ruleset_announces_the_next_rung() {
    let mut sim = Sim::new(CORE).expect("мир");
    sim.without_timeline();
    sim.tick_n(5);
    assert!(sim.news().is_empty(), "старт молчит: {:?}", sim.news());

    // Лестница известности (§12.24): вторая ступень открывается на 20.
    sim.set_fame(80);
    sim.tick_n(1);
    assert!(
        sim.news().iter().any(|&(k, _, o)| k == NewsKind::Raid && o),
        "заказы верхних ступеней не объявлены: {:?}",
        sim.news()
    );
    assert!(
        sim.news()
            .iter()
            .any(|&(k, _, o)| k == NewsKind::Recruit && o),
        "кандидат не объявлен: {:?}",
        sim.news()
    );
}

#[test]
fn a_hired_recruit_never_makes_news() {
    let mut sim = sim_bare();
    let faction = sim.set_faction(100);
    sim.set_recruit("nail", 0, &[], &[]);
    sim.set_recruit_needs(0, &[(faction, 20)]);
    sim.set_standing(faction, 30);
    sim.set_gate(1, true);
    sim.force_tile(2, 1, 1);
    sim.tick_n(2);
    assert!(sim.hire(0), "нанять открытого кандидата можно");
    sim.tick_n(2);

    // Репутация пострадавшего просела (§12.43), и ворота кандидата закрылись —
    // но кот уже на базе, и «больше не откликается» про него это новость о том,
    // кого нет. Игроку она ничего не предлагает: в списке найма нанятый не
    // виден вовсе (§12.94).
    sim.set_standing(faction, 10);
    sim.tick_n(2);
    assert!(
        sim.news().is_empty(),
        "про нанятого лента молчит в обе стороны: {:?}",
        sim.news()
    );
}

// --- новый ресурс (§12.136) --------------------------------------------------

/// Предмет, впервые попавший на базу, становится новостью — иначе его появление
/// невидимо (§12.131: до этого строки на складе не было вовсе).
///
/// Это отмена решения §12.131, где шестой вид был отвергнут доводом «приезд
/// кучи — и так самое заметное событие в игре». Довод оказался неверным на
/// живой партии: куча у шлюза среди других куч не читается как «в мире появился
/// новый вид вещей», а окно «Склад» в этот момент закрыто.
#[test]
fn a_first_time_item_is_news() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_items(3);
    // Базовую линию снимает первый тик: что база держит с самого начала,
    // новостью не становится.
    sim.tick_n(1);
    assert!(sim.news().is_empty(), "стартовый мир новостей не даёт");

    sim.put_item(3, 1, 2, 4);
    sim.tick_n(1);

    assert_eq!(
        sim.news(),
        vec![(NewsKind::Item, 2, true)],
        "новый ресурс обязан подать голос",
    );
}

/// Второй раз тот же предмет новостью не становится: шкала `Seen` только
/// растёт, и «открылось» у неё случается ровно однажды.
#[test]
fn a_known_item_never_repeats_itself() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_items(3);
    sim.set_auto_tidy(false);
    // Тик до кучи — чтобы базовая линия снялась по пустому миру: предмет,
    // лежавший там с самого начала, новостью не бывает по определению.
    sim.tick_n(1);
    sim.put_item(3, 1, 2, 1);
    sim.tick_n(2);
    assert_eq!(sim.news().len(), 1);

    sim.take_item(3, 1, 2);
    sim.tick_n(2);
    sim.put_item(3, 1, 2, 5);
    sim.tick_n(2);

    assert_eq!(
        sim.news().len(),
        1,
        "закрытий и повторов у ресурса не бывает"
    );
}

/// Стартовый склад боевого рулсета в ленту не идёт, а добыча первой вылазки —
/// идёт: ровно то, что игрок и должен заметить.
#[test]
fn the_shipped_ruleset_announces_only_what_is_new() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    sim.tick_n(1);

    let items: Vec<usize> = sim
        .news()
        .iter()
        .filter(|(kind, ..)| *kind == NewsKind::Item)
        .map(|&(_, def, _)| def)
        .collect();
    assert!(
        items.is_empty(),
        "стартовый склад новостью не бывает: {items:?}"
    );

    let suit = sim.item_index("suit").expect("предмет `suit`");
    sim.put_item(1, 1, suit, 1);
    sim.tick_n(1);

    assert!(
        sim.news().contains(&(NewsKind::Item, suit, true)),
        "первый комбинезон обязан подать голос",
    );
}

/// **Предмет, который база умеет делать, новостью не бывает** (§12.145).
///
/// Строка выхода стоит в окне «Склад» с нулём с самого открытия рецепта
/// (§12.131 — иначе не нажать «Произвести»), и «новый ресурс» про неё
/// объявляет то, что игрок видел всё это время. Само умение объявляет рецепт,
/// и второй строки о том же в тот же тик не встаёт.
#[test]
fn an_item_a_recipe_already_makes_is_never_news() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_items(3);
    sim.set_auto_tidy(false);
    sim.set_recipe(10, &[(0, 1)], &[(2, 1)], &[]);
    sim.tick_n(2);

    assert!(
        !sim.news().iter().any(|&(kind, ..)| kind == NewsKind::Item),
        "открытый рецепт объявляет себя сам: {:?}",
        sim.news(),
    );

    // А теперь предмет и правда приезжает — строка на экране уже была, значит
    // и говорить не о чем.
    sim.put_item(3, 1, 2, 1);
    sim.tick_n(2);

    assert!(
        !sim.news().iter().any(|&(kind, ..)| kind == NewsKind::Item),
        "строка стояла с нулём, приезд вещи её не открывает: {:?}",
        sim.news(),
    );
}
