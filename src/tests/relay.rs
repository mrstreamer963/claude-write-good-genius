//! Дежурство на узле связи и бонус к вылазке (§12.60).
//!
//! Схема одна: коридор со шлюзом, узел связи рядом и коты по сторонам. В
//! `sim_from` дежурство выключено (`comms` = 0), как выключены навыки и рынок, —
//! его включает `set_comms`, и до этого узел остаётся чистой лицензией §12.59.
//!
//! Проверяем прогоном полной цепочки: баги здесь живут в фильтрах занятости и в
//! порядке раздатчиков, а не в арифметике.

use super::*;

/// Мир: шлюз (тайл 1) в (2,1), узел связи (тайл 3) в (1,2) с силой `power`,
/// коты `a`, `b`, `c` и вылазка на одного. Возвращает симуляцию и её индекс.
fn sim_with_comms(power: i32, ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(2, 1, 1);
    sim.set_relay(3, true);
    sim.set_comms(3, power);
    sim.force_tile(1, 2, 3);
    let mission = sim.set_mission(1, ticks, &[(0, 5)]);
    (sim, mission)
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

// --- кого сажают и когда ----------------------------------------------------

/// Пока вылазки нет, у рации никто не сидит: дежурство — задача без
/// естественного конца, и без отряда в поле она бессмысленна.
#[test]
fn no_raid_means_no_duty() {
    let (mut sim, _) = sim_with_comms(2, 40);
    sim.tick_n(20);

    for id in ["a", "b", "c"] {
        assert_eq!(sim.duty_of(id), None, "{id} сидит у пустого эфира");
    }
}

/// Отряд ушёл — свободный кот садится к рации сам. Это и есть «бонус из
/// простоя»: раздатчик стоит после всех работ.
#[test]
fn a_free_cat_takes_the_radio_while_the_squad_is_out() {
    let (mut sim, m) = sim_with_comms(2, 60);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(10);
    assert!(sim.is_away("a"), "отряд в поле");

    let on = ["b", "c"]
        .iter()
        .filter(|id| sim.duty_of(id).is_some())
        .count();
    assert_eq!(on, 1, "ровно один сел к рации");
}

/// Узел держит одного, как парта и лежанка: второй свободный кот к занятой
/// рации не идёт.
#[test]
fn one_node_holds_one_operator() {
    let (mut sim, m) = sim_with_comms(2, 60);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);

    let on: Vec<&str> = ["b", "c"]
        .into_iter()
        .filter(|id| sim.duty_of(id).is_some())
        .collect();
    assert_eq!(on.len(), 1, "у одной рации один дежурный: {on:?}");
}

/// Ноль в `comms` значит «дежурить незачем», и раздатчик к узлу никого не зовёт.
/// Тем же нулём механика выключена в чужих тестах.
#[test]
fn a_node_without_comms_calls_nobody() {
    let (mut sim, m) = sim_with_comms(0, 60);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);

    assert_eq!(sim.duty_of("b"), None);
    assert_eq!(sim.duty_of("c"), None);
}

/// Появившийся чертёж дежурного **не срывает**: `OnDuty` — обычная занятость, и
/// раздатчики видят кота таким же занятым, как строителя (инвариант 7).
#[test]
fn a_new_job_does_not_pull_the_operator_off() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    let sitting = ["b", "c"]
        .into_iter()
        .find(|id| sim.duty_of(id).is_some())
        .expect("кто-то сел");

    sim.add_blueprint(5, 2, 2);
    sim.tick_n(30);
    assert_eq!(sim.duty_of(sitting), Some((1, 2)), "дежурный на месте");
    assert!(!sim.has_assignment(sitting), "и за чертёж не взялся");
}

/// Вылазка кончилась — дежурство снимается само, и кот возвращается в общий
/// пул. Второго места для конца задачи заводить не пришлось.
#[test]
fn duty_ends_with_the_raid() {
    let (mut sim, m) = sim_with_comms(2, 20);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(40);

    assert_eq!(sim.raid_left(m), None, "вылазка закрыта");
    for id in ["b", "c"] {
        assert_eq!(sim.duty_of(id), None, "{id} встал от рации");
    }
}

// --- сколько это даёт -------------------------------------------------------

/// Связь копится **за каждый тик** дежурства и на возвращении делится на длину
/// вылазки: полное дежурство даёт ровно силу узла.
#[test]
fn full_duty_adds_the_node_power() {
    let (mut sim, m) = sim_with_comms(2, 30);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(15);

    let covered = sim.covered_of(m).expect("вылазка идёт");
    assert!(covered > 0, "связь копится по ходу, а не одним замером");
}

/// Сила узла доходит до исхода: тот же отряд с рацией вытягивает вылазку,
/// которую без неё провалил бы. Связь входит в **ту же** силу, а не пятым
/// слагаемым исхода (инвариант 14).
#[test]
fn comms_turn_a_failure_into_a_haul() {
    // Сила отряда — 1 (кот без навыка), сложность 4: вдвое меньше нужного,
    // то есть провал. Рация силой 2 доводит до 3 — уже не провал.
    let bare = {
        let (mut sim, _) = sim_with_comms(0, 30);
        let m = sim.set_risky_mission(1, 30, 4, 0, &[(0, 20)]);
        sim.launch(m, squad(&["a"]));
        sim.tick_n(70);
        sim.scrap_total()
    };
    let wired = {
        let (mut sim, _) = sim_with_comms(2, 30);
        let m = sim.set_risky_mission(1, 30, 4, 0, &[(0, 20)]);
        sim.launch(m, squad(&["a"]));
        sim.tick_n(70);
        sim.scrap_total()
    };

    assert_eq!(bare, 0, "без связи отряд провалился");
    assert!(wired > 0, "со связью вернулся с добычей: {wired}");
}

/// Обрыв под самый конец **не** съедает бонус целиком: доля округляется к
/// ближайшему, а не отбрасывается. При силе рации в единицу деление нацело
/// давало бы ноль всю вылазку и единицу в последний тик — то самое «всё или
/// ничего», от которого счёт за тик и уводит.
#[test]
fn a_link_dropped_at_the_end_still_counts() {
    let (mut sim, _) = sim_with_comms(1, 40);
    let m = sim.set_risky_mission(1, 40, 4, 0, &[(0, 20)]);
    sim.launch(m, squad(&["a"]));
    // Даём связи набежать почти всю вылазку и обрываем на последних тиках.
    sim.tick_n(45);
    for id in ["b", "c"] {
        if sim.duty_of(id).is_some() {
            sim.set_target(id, 5, 1);
        }
    }
    sim.tick_n(30);

    assert!(
        sim.scrap_total() > 0,
        "почти полное дежурство всё равно дало свой плюс",
    );
}

/// Оборванная связь стоит бонуса пропорционально, а не целиком: накопленное не
/// отнимается. Ровно поэтому она и считается за тик.
#[test]
fn a_broken_link_keeps_what_it_earned() {
    let (mut sim, m) = sim_with_comms(4, 40);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    let mid = sim.covered_of(m).expect("вылазка идёт");
    assert!(mid > 0, "связь набежала");

    // Уводим дежурного приказом — связь обрывается, но набежавшее остаётся.
    for id in ["b", "c"] {
        if sim.duty_of(id).is_some() {
            sim.set_target(id, 5, 1);
        }
    }
    sim.tick_n(5);
    let after = sim.covered_of(m).expect("вылазка ещё идёт");
    assert_eq!(after, mid, "накопленное не отнимается");
}

// --- приписка игроком -------------------------------------------------------

/// Игрок сажает связиста сам, и раздатчик уважает его выбор.
#[test]
fn the_player_picks_the_operator() {
    let (mut sim, m) = sim_with_comms(2, 60);
    assert!(sim.post_relay("c", 1, 2), "приписка принята");
    assert_eq!(sim.post_of("c"), Some((1, 2)));

    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("c"), Some((1, 2)), "сел приписанный");
    assert_eq!(sim.duty_of("b"), None, "а не первый попавшийся");
}

/// Приписка **переживает** голод и сон: `release_work` снимает дежурство, но не
/// её, и раздатчик вернёт кота к рации сам. Иначе повторилась бы дыра парты —
/// игрок посадил, кот поел и больше не вернулся.
#[test]
fn a_posting_outlives_the_task() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.post_relay("c", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("c"), Some((1, 2)));

    // Приказ снимает дежурство — но не приписку.
    sim.set_target("c", 5, 1);
    assert_eq!(sim.duty_of("c"), None, "приказ увёл от рации");
    assert_eq!(sim.post_of("c"), Some((1, 2)), "но приписка цела");

    sim.tick_n(30);
    assert_eq!(sim.duty_of("c"), Some((1, 2)), "и кот вернулся сам");
}

/// Отмена приписки снимает и текущее дежурство — иначе связист досидел бы до
/// конца вылазки, и игрок решил бы, что кнопка не сработала.
#[test]
fn unposting_frees_the_operator_at_once() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.post_relay("c", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("c"), Some((1, 2)));

    assert!(sim.unpost_relay("c"), "приписка снята");
    assert_eq!(sim.post_of("c"), None);
    assert_eq!(sim.duty_of("c"), None, "и рацию кот оставил тем же тиком");
}

/// Приписанный вытесняет самосевшего: узел держит одного, и молчащая кнопка
/// читалась бы как сломанная.
#[test]
fn a_posting_evicts_the_squatter() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    let squatter = ["b", "c"]
        .into_iter()
        .find(|id| sim.duty_of(id).is_some())
        .expect("кто-то сел сам");
    let other = if squatter == "b" { "c" } else { "b" };

    assert!(sim.post_relay(other, 1, 2), "игрок сажает своего");
    assert_eq!(sim.duty_of(squatter), None, "самосевший уступил");
    sim.tick_n(10);
    assert_eq!(
        sim.duty_of(other),
        Some((1, 2)),
        "и место занял приписанный"
    );
}

/// В клетку без узла приписать нельзя: это не «молча ничего», а отказ.
#[test]
fn a_posting_needs_a_node() {
    let (mut sim, _) = sim_with_comms(2, 60);
    assert!(!sim.post_relay("c", 5, 1), "в коридоре рации нет");
    assert_eq!(sim.post_of("c"), None);
}

// --- боевой рулсет ----------------------------------------------------------

/// В `core.yaml` рация не только считается, но и работает: за ней сидят и она
/// прибавляет отряду силы. Ловит рулсет, где узел завели, а `comms` забыли, —
/// то есть весь §12.60 молча выключен.
#[test]
fn the_shipped_ruleset_puts_someone_on_the_radio() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    let relay = 11; // индекс `relay` в палитре тайлов
    assert!(sim.comms_of_tile(relay) > 0, "у рации есть сила связи");

    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "ушли на свалку"
    );
    sim.tick_n(60);

    assert!(
        sim.duty_of("sp3").is_some(),
        "оставшийся дома кот сел к рации",
    );
}
