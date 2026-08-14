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

/// Отряд ушёл, рация свободна — и **никто не садится сам** (§12.75). Дежурство
/// это решение игрока, а не работа, которую подберёт первый освободившийся: на
/// базе из трёх котов «бонус из простоя» отнимал у стройки последние лапы.
#[test]
fn nobody_takes_the_radio_on_their_own() {
    let (mut sim, m) = sim_with_comms(2, 60);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert!(sim.is_away("a"), "отряд в поле");

    for id in ["b", "c"] {
        assert_eq!(sim.duty_of(id), None, "{id} сел к рации без приписки");
    }
}

/// Узел держит одного, как парта и лежанка: приписка второго снимает первую —
/// молча, как зачисление ко второму отряду (§12.61). Отказ читался бы сломанной
/// кнопкой: игрок не видит, кто приписан, пока не долистает список.
#[test]
fn one_node_holds_one_operator() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.post_relay("b", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "первый сел");

    assert!(sim.post_relay("c", 1, 2), "игрок сажает другого");
    assert_eq!(sim.post_of("b"), None, "прежняя приписка снята");
    assert_eq!(sim.duty_of("b"), None, "и рацию он оставил тем же тиком");
    sim.tick_n(10);
    assert_eq!(sim.duty_of("c"), Some((1, 2)), "место занял свежий связист");
    assert_eq!(sim.duty_of("b"), None, "вдвоём у одной рации не сидят");
}

/// Ноль в `comms` значит «дежурить незачем», и раздатчик к узлу не зовёт даже
/// приписанного. Тем же нулём механика выключена в чужих тестах.
#[test]
fn a_node_without_comms_calls_nobody() {
    let (mut sim, m) = sim_with_comms(0, 60);
    sim.post_relay("b", 1, 2);
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
    sim.post_relay("b", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "связист сел");

    sim.add_blueprint(5, 2, 2);
    sim.tick_n(30);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "дежурный на месте");
    assert!(!sim.has_assignment("b"), "и за чертёж не взялся");
}

/// Вылазка кончилась — дежурство снимается само, и кот возвращается в общий
/// пул. Второго места для конца задачи заводить не пришлось.
#[test]
fn duty_ends_with_the_raid() {
    let (mut sim, m) = sim_with_comms(2, 20);
    sim.post_relay("b", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(10);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "пока отряд в поле — сидит");
    sim.tick_n(30);

    assert_eq!(sim.raid_left(m), None, "вылазка закрыта");
    assert_eq!(sim.duty_of("b"), None, "и связист встал от рации");
    assert_eq!(sim.post_of("b"), Some((1, 2)), "приписка при этом цела");
}

// --- сколько это даёт -------------------------------------------------------

/// Связь копится **за каждый тик** дежурства и на возвращении делится на длину
/// вылазки: полное дежурство даёт ровно силу узла.
#[test]
fn full_duty_adds_the_node_power() {
    let (mut sim, m) = sim_with_comms(2, 30);
    sim.post_relay("b", 1, 2);
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
        sim.post_relay("b", 1, 2);
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
    sim.post_relay("b", 1, 2);
    sim.launch(m, squad(&["a"]));
    // Даём связи набежать почти всю вылазку и обрываем на последних тиках.
    sim.tick_n(45);
    sim.unpost_relay("b");
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
    sim.post_relay("b", 1, 2);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    let mid = sim.covered_of(m).expect("вылазка идёт");
    assert!(mid > 0, "связь набежала");

    // Уводим дежурного приказом — связь обрывается, но набежавшее остаётся.
    // Приписку приказ не снимает (§12.60), поэтому и снимаем её сами: иначе
    // кот вернулся бы к рации сам и связь бы не оборвалась.
    sim.unpost_relay("b");
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

/// В клетку без узла приписать нельзя: это не «молча ничего», а отказ.
#[test]
fn a_posting_needs_a_node() {
    let (mut sim, _) = sim_with_comms(2, 60);
    assert!(!sim.post_relay("c", 5, 1), "в коридоре рации нет");
    assert_eq!(sim.post_of("c"), None);
}

// --- постоянный состав отряда на узле (§12.61) -------------------------------

/// Мир с двумя узлами: рации в (1,2) и (2,2), шлюз в (2,1), коты `a`, `b`, `c`.
///
/// Дежурство здесь выключено (`comms` = 0): состав отряда к нему отношения не
/// имеет, а сидящий у рации кот — лишний шум в проверке того, кто ушёл.
fn sim_with_two_nodes() -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(2, 1, 1);
    sim.set_relay(3, true);
    sim.force_tile(1, 2, 3);
    sim.force_tile(2, 2, 3);
    let mission = sim.set_mission(1, 40, &[(0, 5)]);
    (sim, mission)
}

/// Узел заменяет выбор отряда: кнопка вылазки берёт состав с клетки, а не из
/// того, кого игрок выделил перед уходом.
#[test]
fn a_node_sends_its_own_crew() {
    let (mut sim, m) = sim_with_two_nodes();
    assert!(sim.enlist("b", 1, 2), "зачислили в отряд первого узла");

    assert!(sim.launch_node(m, 1, 2), "отряд узла ушёл");
    sim.tick_n(20);

    assert!(sim.is_away("b"), "ушёл тот, кто числится");
    assert!(!sim.is_away("a"), "остальные остались дома");
    assert!(!sim.is_away("c"));
}

/// Состав **переживает вылазку**: вернувшийся отряд остаётся отрядом, и второй
/// раз его собирать не надо — в этом вся разница с `Squad`.
#[test]
fn a_crew_outlives_its_raid() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("b", 1, 2);
    sim.launch_node(m, 1, 2);
    sim.tick_n(80);

    assert!(!sim.is_away("b"), "вылазка кончилась");
    assert_eq!(
        sim.roster_at(1, 2),
        vec!["b".to_string()],
        "состав на месте"
    );
    assert!(sim.launch_node(m, 1, 2), "и уходит снова без пересбора");
}

/// Пустой узел никого не отправляет: заказу нужен ровно `squad` котов, и
/// состав, который ему не подходит, — это отказ, а не «возьмём кого-нибудь».
#[test]
fn an_empty_node_launches_nobody() {
    let (mut sim, m) = sim_with_two_nodes();
    assert!(!sim.launch_node(m, 1, 2), "отряда на узле нет");

    sim.enlist("a", 1, 2);
    sim.enlist("b", 1, 2);
    assert!(!sim.launch_node(m, 1, 2), "а теперь их слишком много");
}

/// Кот числится не более чем в одном отряде: зачисление ко второму узлу
/// снимает первое. Два узла, зовущих одного кота, — это вылазка, состав которой
/// зависит от того, какую кнопку нажали раньше.
#[test]
fn a_cat_belongs_to_one_crew_only() {
    let (mut sim, _) = sim_with_two_nodes();
    sim.enlist("b", 1, 2);
    sim.enlist("b", 2, 2);

    assert_eq!(sim.enlisted_at("b"), Some((2, 2)));
    assert!(sim.roster_at(1, 2).is_empty(), "с первого узла вычеркнули");
}

/// Каждый узел отправляет свой отряд, и слот занимает **именно он**: дежурный
/// иначе сядет не к той рации, а игрок этого не поймёт.
#[test]
fn each_node_sends_its_own_squad() {
    let (mut sim, first) = sim_with_two_nodes();
    let second = sim.set_mission(1, 40, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.enlist("c", 2, 2);

    assert!(sim.launch_node(first, 1, 2));
    assert!(sim.launch_node(second, 2, 2));
    sim.tick_n(20);

    assert!(sim.is_away("a") && sim.is_away("c"), "оба отряда в поле");
    assert_eq!(sim.mission_node(first), Some((1, 2)), "слот первого узла");
    assert_eq!(sim.mission_node(second), Some((2, 2)), "слот второго");
}

/// Занятый узел второй вылазки не отправляет: узел — слот (§12.59), и то, что
/// состав теперь его собственный, этого не отменяет.
#[test]
fn a_busy_node_launches_nothing() {
    let (mut sim, first) = sim_with_two_nodes();
    let second = sim.set_mission(1, 40, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    assert!(sim.launch_node(first, 1, 2));

    assert!(!sim.launch_node(second, 1, 2), "узел уже ведёт вылазку");
}

/// Зачисляют только в клетку с рацией — как приписывают только к ней (§12.60).
#[test]
fn enlisting_needs_a_node() {
    let (mut sim, _) = sim_with_two_nodes();
    assert!(!sim.enlist("a", 5, 1), "в коридоре рации нет");
    assert_eq!(sim.enlisted_at("a"), None);
}

/// Состав вышедшего отряда не переигрывают: для этого есть отзыв. Иначе игрок
/// вычеркнул бы кота, который уже идёт к шлюзу, и не понял, почему тот всё
/// равно ушёл.
#[test]
fn a_departed_cat_cannot_be_enlisted_elsewhere() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("a", 1, 2);
    sim.launch_node(m, 1, 2);

    assert!(!sim.enlist("a", 2, 2), "заявка уже подана");
    assert_eq!(sim.enlisted_at("a"), Some((1, 2)));
}

/// Вычеркнуть можно и того, кто в поле: `Enlisted` — конфигурация, мира она не
/// трогает, и ушедшего отряда это не касается.
#[test]
fn dismissing_touches_config_only() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("a", 1, 2);
    sim.launch_node(m, 1, 2);
    sim.tick_n(20);

    assert!(sim.dismiss("a"), "вычеркнули");
    assert!(sim.is_away("a"), "но вылазку это не отменило");
    assert!(sim.roster_at(1, 2).is_empty());
    assert!(!sim.dismiss("a"), "второй раз вычёркивать некого");
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

    // Сам по себе к рации никто не садится (§12.75) — связиста назначает игрок.
    let node = *sim
        .relay_cells()
        .first()
        .expect("рация в стартовой застройке");
    assert!(sim.post_relay("sp3", node.0, node.1), "связист назначен");
    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "ушли на свалку"
    );
    sim.tick_n(60);

    assert_eq!(sim.duty_of("sp3"), Some(node), "и дошёл до рации");
}

/// Автовылазка в некомплекте не уходит (§12.70): кнопка ждать перестала, а
/// правило ждёт полного состава.
///
/// Игрок, нажимая кнопку, соглашается на просевшую долю разово и видя цену.
/// Автомат делал бы этот выбор за него на каждом круге.
#[test]
fn an_auto_raid_waits_for_the_whole_brigade() {
    let rows = &["########", "#ab....#", "########"];
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(6, 1, 1);
    let m = sim.set_mission(1, 20, &[(0, 5)]);
    sim.set_squad_range(m, 1, 2);
    sim.set_rest(2, 1);
    sim.force_tile(3, 1, 2); // лежанка, до которой два шага
    sim.set_needs(1000, 500, 1);

    sim.enlist("a", 6, 1);
    sim.enlist("b", 6, 1);
    sim.set_auto_raid(m as i32, 6, 1);

    // Один спит — правило молчит, хотя минимума хватило бы на одного.
    sim.set_energy("b", 100);
    sim.tick_n(20);
    assert!(sim.is_resting("b"), "второй спит");
    assert!(!sim.is_away("a"), "и правило не отправило первого одного");

    // Выспался — ушли оба.
    sim.set_energy("b", 1000);
    sim.tick_n(30);
    assert!(!sim.is_resting("b"), "проснулся");
    sim.tick_n(30);
    assert!(sim.is_away("a") && sim.is_away("b"), "ушли полным составом");
}
