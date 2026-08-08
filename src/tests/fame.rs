//! Известность и найм (§12.24).
//!
//! Одна экономика, два применения: известность копится от вылазок и **только
//! открывает** — вылазки посложнее и кандидатов на найм. Платит за найм склад,
//! а не известность: шкала, которая и открывает, и тратится, закрывала бы уже
//! доступное, и это читалось бы как поломка.
//!
//! Мир тот же, что у вылазок: коридор с клеткой-шлюзом (тайл 1). Склад заводим
//! явно (`set_capacity` + `force_tile`) — в схеме `sim_from` его нет.

use super::*;

/// Коридор со шлюзом в (3,1) и складом в (5,1); миссия на двоих.
fn sim_with_gate_and_store(ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_capacity(2, 100);
    sim.force_tile(5, 1, 2);
    let m = sim.set_mission(2, ticks, &[(0, 5)]);
    (sim, m)
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

// --- известность -----------------------------------------------------------

#[test]
fn a_successful_raid_earns_fame() {
    let (mut sim, m) = sim_with_gate_and_store(10);
    sim.set_mission_fame(m, 30, 0);
    assert_eq!(sim.fame(), 0, "база начинает безвестной");

    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);
    assert_eq!(sim.fame(), 30, "полный успех — вся известность");
}

/// Слухи расходятся по сделанному: половина добычи — половина известности.
#[test]
fn a_partial_raid_earns_a_partial_fame() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let m = sim.set_risky_mission(2, 10, 4, 0, &[(0, 20)]);
    sim.set_mission_fame(m, 30, 0);

    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);
    assert_eq!(sim.fame(), 15, "сила 2 из 4 — половина");
}

/// Провал не приносит известности — но и не отнимает. Ворота, которые
/// закрываются, читались бы как поломка: вчера вылазка была, сегодня нет.
#[test]
fn a_failed_raid_costs_no_fame() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    sim.set_fame(50);
    let m = sim.set_risky_mission(2, 10, 10, 0, &[(0, 20)]);
    sim.set_mission_fame(m, 30, 0);

    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);
    assert_eq!(sim.fame(), 50, "известность не убыла");
}

/// Известность — ворота, а не подсказка: заявку ниже порога ядро отклоняет,
/// каким бы сильным ни был отряд.
#[test]
fn a_mission_below_the_threshold_is_refused() {
    let (mut sim, m) = sim_with_gate_and_store(10);
    sim.set_mission_fame(m, 10, 40);

    assert!(!sim.launch(m, squad(&["a", "b"])), "о базе ещё не слышали");
    assert_eq!(sim.mission_left(), None);

    sim.set_fame(40);
    assert!(sim.launch(m, squad(&["a", "b"])), "порог взят — берутся");
}

/// Лестница целиком: простая вылазка приносит известность, та открывает
/// следующую. Ради этого ворота и вводились (§4.4).
#[test]
fn fame_from_one_raid_opens_the_next() {
    let (mut sim, easy) = sim_with_gate_and_store(10);
    sim.set_mission_fame(easy, 25, 0);
    let hard = sim.set_mission(2, 10, &[(0, 50)]);
    sim.set_mission_fame(hard, 40, 20);

    assert!(!sim.launch(hard, squad(&["a", "b"])), "сложная закрыта");
    sim.launch(easy, squad(&["a", "b"]));
    sim.tick_n(25);

    assert_eq!(sim.fame(), 25);
    assert!(sim.launch(hard, squad(&["a", "b"])), "и открылась сама");
}

// --- найм ------------------------------------------------------------------

#[test]
fn a_recruit_arrives_at_the_gate() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let r = sim.set_recruit("nail", 0, &[], &[]);

    assert!(sim.hire(r));
    assert!(sim.has_unit("nail"), "новый кот на базе");
    assert_eq!(sim.pos_of("nail"), (3, 1), "и появился у шлюза");
}

/// Известность открывает кандидата — тем же порогом, что и вылазку.
#[test]
fn a_recruit_below_the_threshold_does_not_answer() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let r = sim.set_recruit("nail", 30, &[], &[]);

    assert!(!sim.hire(r), "о базе ещё не слышали");
    assert!(!sim.has_unit("nail"));

    sim.set_fame(30);
    assert!(sim.hire(r), "услышали — откликнулся");
}

/// Платит склад: то, что валяется на полу, ещё не сосчитано. Это третий смысл
/// клетки с ёмкостью после самой ёмкости и места назначения (§12.24).
#[test]
fn hiring_is_paid_from_storage() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let r = sim.set_recruit("nail", 0, &[(0, 10)], &[]);

    sim.put_scrap(1, 1, 40); // куча на полу, мимо склада
    assert!(!sim.hire(r), "на полу — ещё не казна");
    assert_eq!(sim.scrap_at(1, 1), 40, "и её не тронули");

    sim.put_scrap(5, 1, 10); // а это уже склад
    assert!(sim.hire(r));
    assert_eq!(sim.scrap_at(5, 1), 0, "цена списана со склада");
    assert_eq!(sim.scrap_at(1, 1), 40, "пол по-прежнему не тронут");
}

/// Не хватило — не списываем ничего: половинчатая оплата оставила бы игрока
/// и без предметов, и без кота.
#[test]
fn a_short_payment_takes_nothing() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let r = sim.set_recruit("nail", 0, &[(0, 10), (1, 5)], &[]);
    sim.put_item(5, 1, 0, 30); // лома вдоволь
    sim.put_item(5, 1, 1, 2); // а деталей нет

    assert!(!sim.hire(r), "цену не покрыть");
    assert_eq!(sim.item_at(5, 1, 0), 30, "лом на месте");
    assert_eq!(sim.item_at(5, 1, 1), 2, "и детали тоже");
    assert!(!sim.has_unit("nail"));
}

/// Коты уникальны (§4.2): один и тот же кандидат не приходит дважды. Проверяем
/// по самому коту, а не по списку нанятых, — второй источник правды однажды
/// разошёлся бы с миром.
#[test]
fn a_recruit_comes_only_once() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let r = sim.set_recruit("nail", 0, &[], &[]);

    assert!(sim.hire(r));
    assert!(!sim.hire(r), "второй раз того же кота не нанять");
}

/// Кандидат приходит уже умеющим — тот самый «навык из найма» (§12.18), и это
/// единственная причина, по которой кандидаты вообще различимы.
#[test]
fn a_recruit_brings_its_skills() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let raid = sim.set_skill("raid", &[100, 400]);
    let r = sim.set_recruit("nail", 0, &[], &[(raid, 450)]);

    assert!(sim.hire(r));
    assert_eq!(sim.xp_of("nail", raid), 450);
    assert_eq!(
        sim.level_of("nail", raid),
        2,
        "приходит мастером, а не с нуля"
    );
}

/// Нанятый — обычный кот: его берут раздатчики, он строит и ходит на вылазки.
/// Собирается он тем же `spawn_cat`, что и стартовая тройка (§12.24).
#[test]
fn a_recruit_is_an_ordinary_cat() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    sim.set_needs(500, 50, 1);
    let r = sim.set_recruit("nail", 0, &[], &[]);
    assert!(sim.hire(r));

    assert_eq!(sim.energy_of("nail"), 500, "бодрость по тем же правилам");
    sim.add_blueprint(6, 1, 3);
    sim.tick_n(BUILD_TICKS + 10);
    assert_eq!(sim.tile(6, 1), 3, "и работает наравне со всеми");
}

/// Нанятый ходит на вылазки и добавляет отряду силы — ради этого его и берут.
#[test]
fn a_hired_cat_can_be_sent_on_a_raid() {
    let (mut sim, _) = sim_with_gate_and_store(10);
    let raid = sim.set_skill("raid", &[100]);
    let r = sim.set_recruit("nail", 0, &[], &[(raid, 100)]);
    assert!(sim.hire(r));

    // Сложность 3: новичок с 'a' дают 1 + (1+1) = 3, то есть всю добычу.
    let m = sim.set_risky_mission(2, 10, 3, 0, &[(0, 30)]);
    assert!(sim.launch(m, squad(&["a", "nail"])));
    sim.tick_n(25);
    assert_eq!(sim.scrap_total(), 30, "нанятый мастер вытянул вылазку");
}

/// Боевой рулсет: лестница известности сходится с лестницей сложности. Ловит
/// контент, в котором первая вылазка закрыта, кандидат стоит дешевле, чем есть
/// на старте, или порог найма недостижим.
#[test]
fn the_shipped_ruleset_has_a_reachable_ladder() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // известность здесь только от вылазок (§12.28)

    // Первая вылазка доступна безвестной базе, остальные — нет.
    assert_eq!(sim.fame(), 0);
    assert!(
        !sim.launch(2, squad(&["excellent", "sp2", "sp3"])),
        "ангар закрыт"
    );
    assert!(
        !sim.launch(3, squad(&["excellent", "sp2"])),
        "логово закрыто"
    );
    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "свалка открыта"
    );

    // Первого кандидата на старте не нанять: ни известности, ни деталей.
    assert!(!sim.hire(0), "кандидат ещё не слышал о базе");

    sim.tick_n(600);
    assert_eq!(sim.fame(), 10, "свалка принесла известность");
    sim.tick_n(600); // ждём, пока добычу свезут на склад

    // Две свалки открывают ангар — и первого кандидата.
    assert!(sim.launch(0, squad(&["excellent", "sp2"])));
    sim.tick_n(1200);
    assert_eq!(sim.fame(), 20);
    // Доверие Синдиката выдаём напрямую: здесь проверяется лестница
    // известности, а репутация — чужая механика со своей лестницей (§12.43),
    // ровно как таймлайн выключен выше по той же причине.
    sim.set_standing(1, 30);
    assert!(sim.hire(0), "теперь и откликается, и по карману");
    assert!(
        sim.launch(2, squad(&["excellent", "sp2", "sp3"])),
        "ангар открыт"
    );

    // Кандидат приходит уже умеющим — иначе платить за него незачем. Опыт ниже
    // первого порога сделал бы его дорогим новичком, и заметить это в игре было
    // бы почти нечем.
    let raid = sim.skill_index("raid").expect("навык «Вылазка» в рулсете");
    assert!(
        sim.level_of("nail", raid) >= 2,
        "нанятый ходок сильнее стартовой тройки — за то и платили",
    );
}
