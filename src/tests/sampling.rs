//! Поле: тропы и сбор образцов (§12.256).
//!
//! Обе вещи правят срок вылазки, а не силу, и обе — свойство **отряда**: тропы
//! знает один, прибор несёт один, а пользуются все. Поэтому проверяется здесь
//! главное — что они не складываются и что прогноз, уход, стадия и добыча
//! считают их одним выражением (инвариант 14).

use super::*;

/// Перк троп в этих схемах — с вычетом 25 %, как в боевом рулсете.
const TRAILS: &str = "trails";

/// Коридор с гаражом на (6, 1) и одним котом `a`; добыча — предмет 0.
fn field(rows: &[&str], travel: i32, work: i32) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    sim.set_perk_road(TRAILS, 25);
    sim.set_gate(1, true);
    sim.force_tile(6, 1, 1);
    let m = sim.set_mission(rows_cats(rows), travel, &[(0, 5)]);
    sim.set_mission_work(m, work);
    (sim, m)
}

/// Сколько котов в схеме — отряд здесь всегда «все».
fn rows_cats(rows: &[&str]) -> usize {
    rows.iter()
        .flat_map(|r| r.chars())
        .filter(|c| c.is_ascii_lowercase())
        .count()
}

fn all(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Дойти до ухода отряда.
fn depart(sim: &mut Sim, lead: &str) {
    while !sim.is_away(lead) {
        sim.tick_n(1);
    }
}

// --- тропы -----------------------------------------------------------------

/// «Знание троп» срезает четверть дороги и не трогает работу на месте.
#[test]
fn a_trailblazer_shortens_the_road_not_the_work() {
    let rows = &["########", "#a.....#", "########"];
    let (mut sim, m) = field(rows, 40, 40);
    sim.set_perks("a", &[TRAILS]);
    assert!(sim.launch(m, all(&["a"])));
    depart(&mut sim, "a");

    assert_eq!(sim.mission_travel(), Some(30), "дорога 40 → 30");
    assert_eq!(sim.mission_span(), Some(70), "30 дороги и те же 40 работы");
}

/// Двое знатоков троп не сокращают дорогу вдвое: тропы знает отряд, а не лапы.
#[test]
fn trails_do_not_stack() {
    let rows = &["########", "#ab....#", "########"];
    let (mut sim, m) = field(rows, 40, 0);
    sim.set_perks("a", &[TRAILS]);
    sim.set_perks("b", &[TRAILS]);
    assert!(sim.launch(m, all(&["a", "b"])));
    depart(&mut sim, "a");
    assert_eq!(sim.mission_span(), Some(30));
}

/// Стадия и граница исхода мерят **ту** дорогу, по которой отряд идёт. Мерь
/// они правило, отряд с тропами «работал» бы, ещё не дойдя, а исход считался
/// бы раньше конца работы (§12.244).
#[test]
fn the_phase_follows_the_frozen_road() {
    let rows = &["########", "#a.....#", "########"];
    let (mut sim, m) = field(rows, 40, 40);
    sim.set_perks("a", &[TRAILS]);
    assert!(sim.launch(m, all(&["a"])));
    depart(&mut sim, "a");
    assert_eq!(sim.mission_phase(), Some("travel"));

    // Дорога туда — половина от 30.
    while sim.mission_left().is_some_and(|left| left > 70 - 15) {
        sim.tick_n(1);
    }
    assert_eq!(sim.mission_phase(), Some("work"), "дошли по короткой тропе");

    while sim.mission_left().is_some_and(|left| left > 70 - 15 - 40) {
        sim.tick_n(1);
    }
    assert_eq!(sim.mission_phase(), Some("back"), "отработали те же 40");
}

/// Прогноз срока до нажатия — тот же, что замёрзнет на уходе.
#[test]
fn the_forecast_span_knows_the_trails() {
    let rows = &["########", "#a.....#", "########"];
    let (mut sim, m) = field(rows, 40, 40);
    sim.set_perks("a", &[TRAILS]);
    assert!(sim.launch(m, all(&["a"])));
    depart(&mut sim, "a");
    let frozen = sim.mission_span();
    assert_eq!(frozen, Some(70));
    // Тем же `duration`, каким считает панель узла.
    let rules = sim.world.resource::<MissionRules>();
    let crew = crate::missions::Crew {
        road_cut: 25,
        work_toll: 0,
    };
    assert_eq!(
        Some(crate::missions::duration(&rules.0[m], 1, crew)),
        frozen
    );
}

// --- сбор образцов ---------------------------------------------------------

const SAMPLE: usize = 0;
const COLLECTOR: usize = 1;
const SUIT: usize = 2;

/// Образец падает только отряду с прибором сбора.
#[test]
fn samples_drop_only_with_a_collector_in_the_squad() {
    let rows = &["########", "#a.....#", "########"];
    for (with, expected) in [(false, 0), (true, 5)] {
        let (mut sim, m) = field(rows, 10, 0);
        sim.set_item_traits(SAMPLE, 0, false, true);
        sim.set_item_traits(COLLECTOR, 10, false, false);
        if with {
            sim.put_gear("a", &[COLLECTOR]);
        }
        assert!(sim.launch(m, all(&["a"])));
        sim.tick_n(40);
        assert!(!sim.is_away("a"), "отряд вернулся");
        assert_eq!(sim.item_total(SAMPLE), expected, "с прибором: {with}");
    }
}

/// Сбор длиннее подбора на десятую долю работы — один раз на отряд, сколько бы
/// приборов в нём ни несли.
#[test]
fn collecting_lengthens_the_work_once_per_squad() {
    let rows = &["########", "#ab....#", "########"];
    let (mut sim, m) = field(rows, 40, 40);
    sim.set_item_traits(COLLECTOR, 10, false, false);
    sim.put_gear("a", &[COLLECTOR]);
    sim.put_gear("b", &[COLLECTOR]);
    assert!(sim.launch(m, all(&["a", "b"])));
    depart(&mut sim, "a");
    // 40 дороги и 44 очка работы на две лапы.
    assert_eq!(sim.mission_span(), Some(40 + 22));
}

/// Провал ломает снаряжение, но не личную вещь: анализатор Антенны другого
/// пути в мир не имеет.
#[test]
fn a_failed_raid_keeps_personal_gear() {
    let mut sim = sim_from(&["########", "#a.....#", "########"]);
    sim.set_gate(1, true);
    sim.force_tile(6, 1, 1);
    let m = sim.set_risky_mission(1, 10, 100, 0, &[]);
    sim.set_item_traits(COLLECTOR, 10, true, false);
    sim.set_force(SUIT, 1);
    sim.put_gear("a", &[COLLECTOR, SUIT]);
    assert!(sim.launch(m, all(&["a"])));
    sim.tick_n(40);
    assert!(!sim.is_away("a"), "вернулся");
    assert_eq!(
        sim.gear_of("a"),
        vec![COLLECTOR],
        "комбинезон сломан, прибор цел"
    );
}

/// Кот с прибором сбора второй не надевает: сбор один на отряд, и лишний
/// пробоотборник на Антенне отнят у того, кому его не хватило.
#[test]
fn a_collector_does_not_take_a_second_one() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let sampler = 3;
    sim.set_item_traits(COLLECTOR, 10, true, false);
    sim.set_item_traits(sampler, 10, false, false);
    sim.set_loadout(&[sampler]);
    sim.put_gear("a", &[COLLECTOR]);
    sim.put_item(3, 1, sampler, 1);
    sim.tick_n(10);
    assert_eq!(sim.gear_of("a"), vec![COLLECTOR]);
    assert_eq!(sim.item_total(sampler), 1, "пробоотборник остался лежать");
}

// --- боевой рулсет ---------------------------------------------------------

fn shipped() -> Sim {
    Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет")
}

/// Образец на боевом рулсете собирают, а не подбирают, и **вход в него один**:
/// кандидат с личным прибором (Антенна). Пропади у неё прибор — и науки в
/// партии не будет вовсе: пробоотборник открывается темой, за которую платят
/// образцами.
#[test]
fn the_shipped_ruleset_brings_samples_with_a_recruit() {
    let sim = shipped();
    let sample = sim.item_index("sample").expect("образец");
    let items = sim.world.resource::<ItemRules>();
    assert!(items.collected(sample), "образец приносит только прибор");
    let recruits = sim.world.resource::<RecruitRules>();
    let bringers: Vec<&str> = recruits
        .0
        .iter()
        .filter(|r| {
            r.gear
                .iter()
                .any(|&i| items.collects(i) && items.personal(i))
        })
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(bringers, vec!["antenna"], "личный прибор сбора — у Антенны");
    // Числа — контент (§12.256): ноль в `road` или `collects` выключил бы
    // перк или прибор молча, без единой ошибки.
    let perks = sim.world.resource::<PerkRules>();
    assert!(
        perks.0.iter().any(|p| p.id == "trails" && p.road > 0),
        "«Знание троп» без вычета дороги — пустой перк",
    );
    assert!(
        recruits
            .0
            .iter()
            .find(|r| r.id == "antenna")
            .is_some_and(|r| r.needs.is_empty()),
        "вход в науку не закрыт фракцией — иначе вторая ветка без неё навсегда",
    );
}

/// Личная вещь — не ресурс базы (§12.256): надетый анализатор Антенны не
/// заводит строку в окне «Ресурсы» и не приходит новостью «новый ресурс».
#[test]
fn personal_gear_is_not_a_resource() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.set_item_traits(COLLECTOR, 10, true, false);
    sim.set_force(SUIT, 1);
    sim.put_gear("a", &[COLLECTOR, SUIT]);
    sim.tick_n(1);
    assert!(!sim.seen(COLLECTOR), "анализатор базе не ресурс");
    assert!(sim.seen(SUIT), "а обычное снаряжение — ресурс");
}
