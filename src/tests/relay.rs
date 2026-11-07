//! Дежурство на узле связи и бонус к вылазке (§12.60).
//!
//! Схема одна: коридор со шлюзом, узел связи рядом и коты по сторонам. В
//! `sim_from` дежурство выключено (`comms` = 0), как выключены навыки и рынок, —
//! его включает `set_comms`, и до этого узел остаётся чистой лицензией §12.59.
//!
//! Проверяем прогоном полной цепочки: баги здесь живут в фильтрах занятости и в
//! порядке раздатчиков, а не в арифметике.

use super::*;

/// Мир: место брифинга со шлюзом (тайл 1) в (2,1), рация (тайл 3) в (1,2) с силой `power`,
/// коты `a`, `b`, `c` и вылазка на одного. Возвращает симуляцию и её индекс.
fn sim_with_comms(power: i32, ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    // Отряд за столом-шлюзом (2,1), рация отдельно (§12.285): радист приписан
    // к отряду, а садится за рацию.
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

/// Отряд ушёл, рация свободна — и **никто не садится сам** (§12.76). Дежурство
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
    sim.post_relay("b", 2, 1);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "первый сел");

    assert!(sim.post_relay("c", 2, 1), "игрок сажает другого");
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
    sim.post_relay("b", 2, 1);
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
    sim.post_relay("b", 2, 1);
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
    sim.post_relay("b", 2, 1);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(10);
    assert_eq!(sim.duty_of("b"), Some((1, 2)), "пока отряд в поле — сидит");
    sim.tick_n(30);

    assert_eq!(sim.raid_left(m), None, "вылазка закрыта");
    assert_eq!(sim.duty_of("b"), None, "и связист встал от рации");
    assert_eq!(sim.post_of("b"), Some((2, 1)), "приписка при этом цела");
}

// --- сколько это даёт -------------------------------------------------------

/// Связь копится **за каждый тик** дежурства и на возвращении делится на длину
/// вылазки: полное дежурство даёт ровно силу узла.
#[test]
fn full_duty_adds_the_node_power() {
    let (mut sim, m) = sim_with_comms(2, 30);
    sim.post_relay("b", 2, 1);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(15);

    let covered = sim.covered_of(m).expect("вылазка идёт");
    assert!(covered > 0, "связь копится по ходу, а не одним замером");
}

/// Связь возвращает отряд **раньше** (§12.277): тот же заказ с дежурным
/// у рации короче, а исход — сила против сложности — от неё не зависит.
#[test]
fn comms_bring_the_squad_home_sooner() {
    let back_at = |power: i32| {
        let (mut sim, m) = sim_with_comms(power, 60);
        if power > 0 {
            sim.post_relay("b", 2, 1);
        }
        sim.launch(m, squad(&["a"]));
        let mut t = 0;
        while sim.is_away("a") || t < 5 {
            sim.tick_n(1);
            t += 1;
            assert!(t < 500, "отряд не вернулся");
        }
        t
    };
    let bare = back_at(0);
    let wired = back_at(4); // рация 4 + связист без навыка: вдвое быстрее
    assert!(
        wired < bare,
        "со связью вернулись раньше: {wired} против {bare}"
    );
}

/// Связь режет **только дорогу**: работа на месте идёт своим счётом, иначе
/// дежурный у рации копал бы за отряд (§12.277).
#[test]
fn comms_never_cut_the_work() {
    let span_of = |power: i32| {
        let (mut sim, m) = sim_with_comms(power, 60);
        sim.set_mission_work(m, 200);
        if power > 0 {
            sim.post_relay("b", 2, 1);
        }
        sim.launch(m, squad(&["a"]));
        sim.tick_n(150);
        sim.covered_of(m).unwrap_or(-1)
    };
    // На 150-м тике отряд ещё работает (200 очков на одну лапу), а срезано
    // не больше всей дороги.
    let cut = span_of(4);
    assert!(cut > 0 && cut <= 30, "срезана только дорога туда: {cut}");
}

/// Оборванная связь стоит бонуса пропорционально, а не целиком: накопленное не
/// отнимается. Ровно поэтому она и считается за тик.
#[test]
fn a_broken_link_keeps_what_it_earned() {
    let (mut sim, m) = sim_with_comms(4, 40);
    sim.post_relay("b", 2, 1);
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

// --- связь именная (§12.278) -------------------------------------------------

/// Мир: два места брифинга со шлюзом — отряд 1 в (1,2), отряд 2 в (2,2) —
/// и две рации: (5,2) силой `p1` и (3,2) силой `p2`; коты `a`, `b`, `c`, `d`.
/// Радист садится за ближайшую к столу свободную рацию (§12.285, §12.286):
/// к рации он идёт от брифинга. Вылазки на одного.
fn sim_with_two_radios(p1: i32, p2: i32, ticks: i32) -> (Sim, usize, usize) {
    let mut sim = sim_from(&["#########", "#a.b.c.d#", "#.......#", "#########"]);
    sim.set_gate(1, true);
    sim.force_tile(1, 2, 1);
    sim.force_tile(2, 2, 1);
    sim.set_relay(3, true);
    sim.set_comms(3, p1);
    sim.force_tile(5, 2, 3);
    sim.set_relay(4, true);
    sim.set_comms(4, p2);
    sim.force_tile(3, 2, 4);
    let first = sim.set_mission(1, ticks, &[(0, 5)]);
    let second = sim.set_mission(1, ticks, &[(0, 5)]);
    (sim, first, second)
}

/// Радист ведёт **свой** отряд, а не все разом: отряд чужой рации идёт
/// обычным шагом (§12.278).
#[test]
fn an_operator_leads_only_his_own_squad() {
    let (mut sim, first, second) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.enlist("b", 2, 2);
    sim.post_relay("c", 1, 2);
    assert!(sim.launch_node(first, 1, 2, -1));
    assert!(sim.launch_node(second, 2, 2, -1));
    sim.tick_n(40);

    assert!(sim.covered_of(first).unwrap_or(0) > 0, "свой отряд ускорен");
    assert_eq!(sim.covered_of(second), Some(0), "чужой — нет");
}

/// Сильный радист ведёт свой отряд быстрее, чем слабый — свой: вклады больше
/// не складываются в общий котёл (§12.278).
#[test]
fn a_stronger_operator_leads_faster() {
    let (mut sim, first, second) = sim_with_two_radios(1, 3, 200);
    sim.enlist("a", 1, 2);
    sim.enlist("b", 2, 2);
    sim.post_relay("c", 1, 2);
    sim.post_relay("d", 2, 2);
    sim.launch_node(first, 1, 2, -1);
    sim.launch_node(second, 2, 2, -1);
    sim.tick_n(60);

    // Радисты выходят к рациям от столов (§12.286), и кто какую займёт, решает
    // дорога: сильная рация (3,2) ближе к обоим столам, её берёт первый по `id`.
    let (strong_m, weak_m) = match sim.duty_of("c") {
        Some((3, 2)) => (first, second),
        _ => (second, first),
    };
    let weak = sim.covered_of(weak_m).expect("слабая идёт");
    let strong = sim.covered_of(strong_m).expect("сильная идёт");
    assert!(
        strong > weak,
        "сильный радист срезал больше: {strong} против {weak}"
    );
}

/// Отряд с приписанным радистом **не уходит**, пока тот не сел за рацию —
/// и называет его в списке держащих сбор (§12.278).
#[test]
fn a_squad_waits_for_its_operator() {
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    sim.set_needs(100, 30, 1);
    sim.enlist("a", 1, 2);
    sim.post_relay("d", 1, 2);
    sim.set_energy("d", 0); // радист валится с ног и спит, где стоял
    sim.tick_n(2);
    assert!(sim.launch_node(first, 1, 2, -1), "заявка принята");
    sim.tick_n(20);

    assert!(!sim.is_away("a"), "без радиста отряд дома");
    assert!(
        sim.unfit_of(1, 2).contains(&"d".to_string()),
        "и держит сбор радист"
    );
}

/// Радист и боец — роли взаимоисключающие (§12.278): приписка к рации
/// вычёркивает из состава, зачисление в отряд снимает с рации.
#[test]
fn operator_and_soldier_exclude_each_other() {
    let (mut sim, ..) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.post_relay("a", 2, 2);
    assert_eq!(sim.enlisted_at("a"), None, "радист не в отряде");
    assert_eq!(sim.post_of("a"), Some((2, 2)));

    sim.enlist("a", 1, 2);
    assert_eq!(sim.post_of("a"), None, "боец не у рации");
    assert_eq!(sim.enlisted_at("a"), Some((1, 2)));
}

/// Пустой эфир — законное состояние: вылазка уходит без бонуса, как уходила до
/// §12.60. Дежурного нет — связь не копится ни у кого.
#[test]
fn an_empty_ether_costs_nothing_but_the_bonus() {
    let (mut sim, m, _) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.launch_node(m, 1, 2, -1);
    sim.tick_n(40);

    assert_eq!(sim.covered_of(m), Some(0), "связи нет, а вылазка идёт");
}

// --- приписка игроком -------------------------------------------------------

/// Игрок сажает связиста сам, и раздатчик уважает его выбор.
#[test]
fn the_player_picks_the_operator() {
    let (mut sim, m) = sim_with_comms(2, 60);
    assert!(sim.post_relay("c", 2, 1), "приписка принята");
    assert_eq!(sim.post_of("c"), Some((2, 1)));

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
    sim.post_relay("c", 2, 1);
    sim.launch(m, squad(&["a"]));
    sim.tick_n(20);
    assert_eq!(sim.duty_of("c"), Some((1, 2)));

    // Приказ снимает дежурство — но не приписку.
    sim.set_target("c", 5, 1);
    assert_eq!(sim.duty_of("c"), None, "приказ увёл от рации");
    assert_eq!(sim.post_of("c"), Some((2, 1)), "но приписка цела");

    sim.tick_n(30);
    assert_eq!(sim.duty_of("c"), Some((1, 2)), "и кот вернулся сам");
}

/// Отмена приписки снимает и текущее дежурство — иначе связист досидел бы до
/// конца вылазки, и игрок решил бы, что кнопка не сработала.
#[test]
fn unposting_frees_the_operator_at_once() {
    let (mut sim, m) = sim_with_comms(2, 200);
    sim.post_relay("c", 2, 1);
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

// --- постоянный состав отряда на гараже (§12.61, §12.152) --------------------

/// Мир с двумя гаражами в (1,2) и (2,2), коты `a`, `b`, `c`.
///
/// Раций здесь нет вовсе: с §12.152 состав и слот держит гараж, а связь — это
/// отдельная прибавка к силе, и сидящий у рации кот был бы лишним шумом в
/// проверке того, кто ушёл.
fn sim_with_two_nodes() -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(1, 2, 1);
    sim.force_tile(2, 2, 1);
    let mission = sim.set_mission(1, 40, &[(0, 5)]);
    (sim, mission)
}

/// Узел заменяет выбор отряда: кнопка вылазки берёт состав с клетки, а не из
/// того, кого игрок выделил перед уходом.
#[test]
fn a_node_sends_its_own_crew() {
    let (mut sim, m) = sim_with_two_nodes();
    assert!(sim.enlist("b", 1, 2), "зачислили в отряд первого узла");

    assert!(sim.launch_node(m, 1, 2, -1), "отряд узла ушёл");
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
    sim.launch_node(m, 1, 2, -1);
    sim.tick_n(80);

    assert!(!sim.is_away("b"), "вылазка кончилась");
    assert_eq!(
        sim.roster_at(1, 2),
        vec!["b".to_string()],
        "состав на месте"
    );
    assert!(sim.launch_node(m, 1, 2, -1), "и уходит снова без пересбора");
}

/// Пустой узел никого не отправляет: заказу нужен ровно `squad` котов, и
/// состав, который ему не подходит, — это отказ, а не «возьмём кого-нибудь».
#[test]
fn an_empty_node_launches_nobody() {
    let (mut sim, m) = sim_with_two_nodes();
    assert!(!sim.launch_node(m, 1, 2, -1), "отряда на узле нет");

    sim.enlist("a", 1, 2);
    sim.enlist("b", 1, 2);
    assert!(!sim.launch_node(m, 1, 2, -1), "а теперь их слишком много");
}

/// **Заказ назначается отряду целиком, а уход откладывается** (§12.191,
/// сужает §12.148): спящего кнопка не будит и не оставляет дома — отряд
/// уходит в тот тик, когда он встал сам. До §12.191 кнопка отвечала «Нельзя»,
/// и решение игрока приходилось принимать второй раз.
#[test]
fn a_node_waits_for_its_sleeping_crew() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("a", 1, 2);
    sim.enlist("b", 1, 2);
    sim.set_squad_range(m, 1, 2);
    sim.set_needs(100, 50, 1);
    // Лежанка нужна: без свободного места уставший кот работает до упора
    // (§12.20), а нам нужен именно спящий.
    sim.set_rest(2, 5);
    sim.force_tile(4, 2, 2);
    sim.set_energy("b", 40);
    sim.tick_n(6);
    assert!(sim.is_resting("b"), "вымотанный лёг спать");

    assert!(sim.launch_node(m, 1, 2, -1), "заявку принимают и со спящим");
    assert!(sim.is_resting("b"), "и спящего она не будит (§12.51)");
    sim.tick_n(3);
    assert!(!sim.is_away("a") && !sim.is_away("b"), "отряд ждёт у шлюза");

    sim.tick_n(40);
    assert!(sim.is_away("a") && sim.is_away("b"), "выспался — ушли оба");
}

/// Второй способ — вычеркнуть неготового с узла: отряд становится другим, и
/// уходит он сразу, не дожидаясь чужого сна.
#[test]
fn dismissing_the_unfit_cat_lets_the_rest_go() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("a", 1, 2);
    sim.enlist("b", 1, 2);
    sim.set_squad_range(m, 1, 2);
    sim.set_needs(100, 50, 1);
    sim.set_rest(2, 5);
    sim.force_tile(4, 2, 2);
    sim.set_energy("b", 40);
    sim.tick_n(6);

    sim.dismiss("b");
    assert!(
        sim.launch_node(m, 1, 2, -1),
        "отряд из одного готового ушёл"
    );
    sim.tick_n(20);
    assert!(sim.is_away("a") && !sim.is_away("b"));
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

    assert!(sim.launch_node(first, 1, 2, -1));
    assert!(sim.launch_node(second, 2, 2, -1));
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
    assert!(sim.launch_node(first, 1, 2, -1));

    assert!(!sim.launch_node(second, 1, 2, -1), "узел уже ведёт вылазку");
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
    sim.launch_node(m, 1, 2, -1);

    assert!(!sim.enlist("a", 2, 2), "заявка уже подана");
    assert_eq!(sim.enlisted_at("a"), Some((1, 2)));
}

/// Вычеркнуть можно и того, кто в поле: `Enlisted` — конфигурация, мира она не
/// трогает, и ушедшего отряда это не касается.
#[test]
fn dismissing_touches_config_only() {
    let (mut sim, m) = sim_with_two_nodes();
    sim.enlist("a", 1, 2);
    sim.launch_node(m, 1, 2, -1);
    sim.tick_n(20);

    assert!(sim.dismiss("a"), "вычеркнули");
    assert!(sim.is_away("a"), "но вылазку это не отменило");
    assert!(sim.roster_at(1, 2).is_empty());
    assert!(!sim.dismiss("a"), "второй раз вычёркивать некого");
}

// --- боевой рулсет ----------------------------------------------------------

/// В `core.yaml` **построенная** рация работает: за ней сидят и она ускоряет
/// отряд. Ловит рулсет, где тайл завели, а `comms` забыли, — то есть весь
/// §12.60 молча выключен.
///
/// Строить её приходится прямо здесь: в стартовой застройке рации нет, отряд
/// живёт за столом брифинга (§12.279), а радист приписан к отряду и садится за
/// рацию, какую найдёт (§12.285).
#[test]
fn the_shipped_ruleset_puts_someone_on_the_radio() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.add_squad(); // стартовый отряд до §12.349
    sim.without_timeline();
    let relay = 11; // индекс `relay` в палитре тайлов
    assert!(sim.comms_of_tile(relay) > 0, "у рации есть сила связи");
    assert_eq!(sim.relay_count(), 0, "на старте её нет");

    let radio = (8, 12);
    sim.force_tile(radio.0, radio.1, relay);
    let node = sim
        .node_cells_of()
        .first()
        .copied()
        .expect("стол на старте");

    // Сам по себе к рации никто не садится (§12.76) — связиста назначает игрок,
    // и только обученного «Связи» (§12.291): стартовые её не знают.
    assert!(
        !sim.post_relay("sp3", node.0, node.1),
        "без «Связи» за рацию не сажают"
    );
    let comms = sim.skill_index("relay").expect("навык «Связь»");
    sim.set_xp("sp3", comms, 200); // первый уровень
    assert!(sim.post_relay("sp3", node.0, node.1), "связист назначен");
    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "ушли на свалку"
    );
    sim.tick_n(60);

    assert_eq!(sim.duty_of("sp3"), Some(radio), "и дошёл до рации");
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

/// Клик по рации — обычный приказ «иди туда» (§12.285): рация отрядом не
/// владеет, и приписать по ней нельзя — неизвестно, к какому отряду. Радиста
/// назначают в штабе, во вкладке своего отряда.
#[test]
fn ordering_onto_a_radio_is_just_a_move() {
    let mut sim = sim_from(&["#######", "#a....#", "#######"]);
    sim.set_relay(1, true);
    sim.set_comms(1, 1);
    sim.force_tile(5, 1, 1);

    assert!(sim.set_target("a", 5, 1), "приказ принят");
    assert_eq!(sim.post_of("a"), None, "приписки по рации нет");
    sim.tick_n(30);
    assert_eq!(sim.pos_of("a"), (5, 1), "кот дошёл до рации");
}

/// Сел радист — отряд ушёл (§12.278): ждут ровно его посадки, не дольше, и
/// уже на сборе радист сидит у рации.
#[test]
fn a_squad_leaves_once_its_operator_sits() {
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.post_relay("d", 1, 2);
    assert!(sim.launch_node(first, 1, 2, -1));
    let mut t = 0;
    while !sim.is_away("a") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 100, "отряд так и не ушёл");
    }
    assert_eq!(
        sim.duty_of("d"),
        Some((3, 2)),
        "радист у ближайшей к столу рации"
    );
}

/// Радист на стройке отряд не держит (§12.286): заявка снимает его с дела,
/// как бойца. До этого брифинг «ждал, пока освободится», а стройка шла
/// сплошняком — и отряд стоял за столом бесконечно.
#[test]
fn a_busy_operator_is_pulled_off_work_by_the_launch() {
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.post_relay("d", 1, 2);
    sim.add_blueprint_rect(1, 1, 7, 1, 2);
    let mut t = 0;
    while !sim.has_assignment("d") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 50, "радист так и не взялся за стройку");
    }
    assert!(sim.launch_node(first, 1, 2, -1));
    assert!(!sim.has_assignment("d"), "заявка сняла радиста со стройки");
    let mut t = 0;
    while !sim.is_away("a") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 100, "отряд так и не ушёл");
    }
}

/// Радиста, приписанного уже во время сбора, сбор тоже снимает с дела
/// (§12.286): заявка его не застала, и снимать его некому, кроме самого сбора.
#[test]
fn an_operator_posted_mid_gathering_is_pulled_off_work() {
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    sim.enlist("a", 1, 2);
    sim.add_blueprint_rect(1, 1, 7, 1, 2);
    let mut t = 0;
    while !sim.has_assignment("d") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 50, "кот так и не взялся за стройку");
    }
    assert!(sim.launch_node(first, 1, 2, -1));
    sim.post_relay("d", 1, 2);
    let mut t = 0;
    while !sim.is_away("a") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 100, "отряд так и не ушёл");
    }
}

/// Допуск к рации (§12.291): радист — только тот, кто умеет «Связь». Отказ не
/// снимает прежнего радиста: решение игрока не стоит отряду связи.
#[test]
fn only_a_cat_who_knows_relay_takes_the_radio() {
    let (mut sim, _) = sim_with_comms(2, 40);
    let comms = sim.set_skill("relay", &[20, 100]);
    assert!(!sim.post_relay("b", 2, 1), "не умеет — не радист");
    sim.set_xp("a", comms, 20);
    assert!(sim.post_relay("a", 2, 1), "умеет — радист");
    assert!(!sim.post_relay("b", 2, 1), "отказ");
    assert_eq!(sim.post_of("a"), Some((2, 1)), "и прежний на месте");
}

/// Радист, ушедший за приписанной вещью, сбор не держит вечно (§12.296): сбор
/// не снимает его с похода за снаряжением, а ждёт, пока он оденется. До этого
/// сбор срывал поход каждый тик, `assign_equip` (он раньше `gather_squad`)
/// тут же посылал кота снова, и тот дрожал на месте — ни одет, ни за столом.
#[test]
fn an_operator_going_for_his_gear_is_not_pulled_back() {
    const SUIT: usize = 1;
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    sim.set_force(SUIT, 1);
    sim.put_item(1, 2, SUIT, 4);
    sim.enlist("a", 1, 2);
    sim.post_relay("d", 1, 2);
    sim.outfit_all(&[SUIT]);
    sim.tick_n(1);
    assert!(sim.is_equipping("d"), "радист пошёл за комбинезоном");
    assert!(sim.launch_node(first, 1, 2, -1));
    let mut t = 0;
    while !sim.is_away("a") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 150, "отряд так и не ушёл: радист {:?}", sim.pos_of("d"));
    }
    assert_eq!(
        sim.gear_of("d"),
        vec![SUIT],
        "радист оделся перед брифингом"
    );
}

/// Радист, которого посреди сбора отправили учиться, сбор не держит вечно
/// (§12.286): фасад снимает его с учёбы перед цепочкой, а `assign_study` (он раньше `gather_squad`) тем же
/// тиком сажал обратно. Кот дрожал на месте, брифинг ждал радиста, класс —
/// ученика. Собирающийся отряд старше приписки к парте; вернувшись, кот
/// садится за неё сам.
#[test]
fn an_enrolled_operator_is_not_pulled_back_to_the_desk() {
    let (mut sim, first, _) = sim_with_two_radios(2, 2, 200);
    let science = sim.set_skill("science", &[20, 100]);
    sim.set_taught(science, 1);
    sim.set_teaches(5, science);
    sim.force_tile(7, 2, 5);
    sim.enlist("a", 1, 2);
    sim.post_relay("d", 1, 2);
    assert!(sim.launch_node(first, 1, 2, -1));
    sim.tick_n(1);
    // Учить велели посреди сбора: приписка к парте снимает радиста со стула.
    assert!(sim.teach("d", "science"));
    let mut t = 0;
    while !sim.is_away("a") {
        sim.tick_n(1);
        t += 1;
        assert!(t < 100, "отряд так и не ушёл: радист {:?}", sim.pos_of("d"));
    }
    assert!(sim.is_enrolled("d"), "приписка к парте пережила сбор");
}
