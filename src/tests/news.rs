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
fn a_raid_closes_when_the_patron_turns_away() {
    let mut sim = sim_bare();
    let faction = sim.set_faction(100);
    let job = sim.set_mission(1, 10, &[]);
    sim.set_mission_needs(job, &[(faction, 20)]);
    sim.set_standing(faction, 30);
    sim.tick_n(2);
    assert!(sim.news().is_empty(), "открыт с начала — молчим");

    // Репутация — единственная знаковая шкала (§12.43), и заказ умеет
    // закрыться. Молчать об этом хуже, чем об открытии: заказ, который вчера
    // был в списке, игрок помнит.
    sim.set_standing(faction, -10);
    sim.tick_n(1);
    assert_eq!(sim.news(), vec![(NewsKind::Raid, job, false)]);

    // И открывается обратно, когда с базой снова говорят.
    sim.set_standing(faction, 40);
    sim.tick_n(1);
    assert_eq!(
        sim.news(),
        vec![(NewsKind::Raid, job, false), (NewsKind::Raid, job, true)]
    );
}

#[test]
fn the_feed_never_grows_past_its_cap() {
    let mut sim = sim_bare();
    let faction = sim.set_faction(100);
    let job = sim.set_mission(1, 10, &[]);
    sim.set_mission_needs(job, &[(faction, 20)]);
    sim.set_standing(faction, 30);
    sim.tick_n(2);

    // Лента — не журнал §12.58: на неё смотрит человек, и прочитанная новость
    // ценности не имеет. Поэтому старое вытесняется, а не копится.
    for i in 0..NEWS_MAX + 10 {
        sim.set_standing(faction, if i % 2 == 0 { -10 } else { 40 });
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
