//! «Пошарить рядом» (§12.349): заказ без отряда — свободные коты идут без
//! стола брифинга и без слота вылазки; добыча убывает до круга пола.

use super::*;

const EXIT: i16 = 3;
const GATE: (i32, i32) = (1, 3);

/// Комната со шлюзом и двумя котами; стола нет. Заказ без отряда: состав от
/// одного до трёх, добыча убывает — 3 лома, потом круг пола 1 и 5.
fn yard() -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#.....#", "#..a.b#", "#.....#", "#######"]);
    sim.set_exit(EXIT, true);
    sim.force_tile(GATE.0, GATE.1, EXIT);
    let def = sim.set_mission(1, 20, &[]);
    sim.set_squad_range(def, 1, 3);
    sim.set_nearby(def);
    sim.set_yields(def, &[&[(0, 3)]], &[&[(0, 1)], &[(0, 5)]]);
    (sim, def)
}
/// Дождаться, пока заказ выполнится в очередной раз.
fn finishes(sim: &mut Sim, def: usize, ticks: usize) -> bool {
    let before = sim.tally_of(def);
    (0..ticks).any(|_| {
        sim.tick_n(1);
        sim.tally_of(def) > before && sim.mission_left().is_none()
    })
}

/// Без стола: свободный кот уходит через шлюз и возвращается с добычей на
/// шлюзе.
#[test]
fn a_free_cat_scavenges_without_a_table() {
    let (mut sim, def) = yard();
    assert_eq!(sim.gate_count(), 0, "стола нет");
    assert!(sim.scavenge(def, vec!["a".into()], -1), "заявка принята");
    let left = (0..100).any(|_| {
        sim.tick_n(1);
        sim.is_away("a")
    });
    assert!(left, "кот вышел через шлюз");
    assert!(finishes(&mut sim, def, 200), "и вернулся");
    assert_eq!(sim.scrap_at(GATE.0, GATE.1), 3, "добыча легла на шлюз");
}

/// Двое свободных уходят вместе — это не отряд, а просто двое у порога.
#[test]
fn two_free_cats_scavenge_together() {
    let (mut sim, def) = yard();
    assert!(sim.scavenge(def, vec!["a".into(), "b".into()], -1));
    let left = (0..100).any(|_| {
        sim.tick_n(1);
        sim.is_away("a") && sim.is_away("b")
    });
    assert!(left, "вышли оба");
}

/// Убывающая добыча: сперва список по порядку, дальше круг пола.
#[test]
fn a_depleting_order_runs_down_to_its_floor_round() {
    let (mut sim, def) = yard();
    let mut got = Vec::new();
    for _ in 0..4 {
        let before = sim.scrap_total();
        assert!(sim.scavenge(def, vec!["a".into()], -1));
        assert!(finishes(&mut sim, def, 300));
        got.push(sim.scrap_total() - before);
    }
    assert_eq!(got, vec![3, 1, 5, 1], "список, потом пол по кругу");
}

/// Кот из отряда не шарит: у него свой отряд. Свободный идёт.
#[test]
fn a_cat_in_a_squad_does_not_scavenge() {
    let (mut sim, def) = yard();
    let table: i16 = 4;
    sim.set_muster(table, true);
    sim.force_tile(5, 1, table);
    assert!(sim.enlist("a", 5, 1), "a в отряде");
    assert!(
        !sim.scavenge(def, vec!["a".into()], -1),
        "кот из отряда не идёт"
    );
    assert!(
        !sim.scavenge(def, vec!["a".into(), "b".into()], -1),
        "и вместе со свободным тоже"
    );
    assert!(sim.scavenge(def, vec!["b".into()], -1), "свободный идёт");
}

/// «Пошарить» слота не занимает: отряд единственного стола уходит на другой
/// заказ, пока свободный кот у порога.
#[test]
fn scavenging_takes_no_raid_slot() {
    let (mut sim, def) = yard();
    let table: i16 = 4;
    sim.set_muster(table, true);
    sim.force_tile(5, 1, table);
    let raid = sim.set_mission(1, 50, &[]);
    assert!(sim.scavenge(def, vec!["a".into()], -1), "кот ушёл шарить");
    assert!(
        sim.launch(raid, vec!["b".into()]),
        "и отряд всё равно уходит"
    );
}

/// Свободные коты без стола ходят на **любой** заказ (§12.349) — с теми же
/// воротами, что у отряда; а вот «Пошарить» отряду не назначают.
#[test]
fn free_cats_take_any_order_but_squads_never_scavenge() {
    let (mut sim, def) = yard();
    let raid = sim.set_mission(1, 50, &[]);
    assert!(
        sim.scavenge(raid, vec!["b".into()], -1),
        "обычный заказ — и без стола"
    );
    let left = (0..100).any(|_| {
        sim.tick_n(1);
        sim.is_away("b")
    });
    assert!(left, "кот ушёл через шлюз");
    let table: i16 = 4;
    sim.set_muster(table, true);
    sim.force_tile(5, 1, table);
    assert!(sim.enlist("a", 5, 1));
    assert!(
        !sim.launch_node(def, 5, 1, -1),
        "отряду «Пошарить» не назначают"
    );
}

/// Ворота заказа у свободных котов те же, что у отряда: закрытый
/// известностью заказ не уходит и без стола.
#[test]
fn free_cats_meet_the_same_gates() {
    let (mut sim, _) = yard();
    let raid = sim.set_mission(1, 50, &[]);
    sim.set_mission_fame(raid, 0, 10);
    assert!(!sim.scavenge(raid, vec!["a".into()], -1), "известности нет");
    sim.set_fame(10);
    assert!(sim.scavenge(raid, vec!["a".into()], -1), "дорос — уходит");
}

/// Свободных называет снимок — кот из отряда в списке не стоит.
#[test]
fn the_snapshot_names_only_free_cats() {
    let (mut sim, _) = yard();
    let table: i16 = 4;
    sim.set_muster(table, true);
    sim.force_tile(5, 1, table);
    assert!(sim.enlist("a", 5, 1));
    assert_eq!(sim.nearby_free(), vec!["b".to_string()]);
}

/// Клик свободным котом по шлюзу — дверь в окрестности (§12.349): тег руки
/// `nearby`, то есть ровно то, чего ждёт вид.
#[test]
fn a_free_cat_on_the_gate_opens_the_outskirts() {
    let (mut sim, _) = yard();
    assert_eq!(sim.hand_tag("a", GATE.0, GATE.1), "nearby");
}

/// **Боевой рулсет: стол брифинга открывает первое «Пошарить»** (§12.349).
#[test]
fn the_shipped_ruleset_opens_the_table_by_scavenging() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    let muster = sim.structure_index("muster").expect("место брифинга");
    let scavenge = sim.mission_index("scavenge").expect("«Пошарить рядом»");
    assert!(!sim.structure_is_open(muster), "на старте стола нет");
    assert!(sim.scavenge(scavenge, vec!["excellent".into()], -1));
    assert!(finishes(&mut sim, scavenge, 1500), "окрестности обшарены");
    assert!(sim.structure_is_open(muster), "и стол открылся");
}

/// **Боевой рулсет: один кот может вернуть пленного** (§12.349,
/// §12.40). Вылазка за своим идёт одним котом, значит плен обратим и до
/// первого стола: необратимых исходов в игре нет.
#[test]
fn the_shipped_ruleset_lets_one_cat_come_for_its_own() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let rules = sim.world.resource::<MissionRules>();
    assert!(
        rules.0.iter().any(|r| r.rescue && r.squad <= 1),
        "за своим не может пойти один — без стола плен необратим",
    );
}

/// **Боевой рулсет: круг пола «Пошарить» не ферма** (§12.349). Шарят сколько
/// угодно котов, значит деталей с пола на тик кота обязано быть меньше, чем у
/// **любой** другой вылазки при **любом** составе. Самый выгодный пол — у
/// одного кота: дорога на лапы не делится. Считается из самого рулсета.
#[test]
fn the_shipped_ruleset_never_farms_parts_upstairs() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let part = sim.item_index("part").expect("деталь в палитре");
    let scavenge = sim.mission_index("scavenge").expect("«Пошарить рядом»");
    let missions = sim.world.resource::<MissionRules>();
    let near = &missions.0[scavenge];
    let floor_parts: i64 = near
        .floor
        .iter()
        .flatten()
        .filter(|&&(i, _)| i == part)
        .map(|&(_, n)| n as i64)
        .sum();
    assert!(
        floor_parts > 0,
        "в круге пола нет детали — дороги назад нет"
    );
    let floor_ticks =
        near.floor.len() as i64 * crate::missions::duration(near, 1, Default::default()) as i64;

    let mut checked = 0;
    for (def, rule) in missions
        .0
        .iter()
        .enumerate()
        .filter(|&(d, _)| d != scavenge)
    {
        let parts: i64 = rule
            .loot
            .iter()
            .filter(|&&(i, _)| i == part)
            .map(|&(_, n)| n as i64)
            .sum();
        if parts == 0 {
            continue;
        }
        for paws in rule.squad.max(1)..=rule.squad_max.max(rule.squad) {
            let span = crate::missions::duration(rule, paws, Default::default()) as i64;
            let raid_ticks = paws as i64 * span;
            assert!(
                floor_parts * raid_ticks < parts * floor_ticks,
                "пол даёт {floor_parts} дет. за {floor_ticks} тиков кота, а вылазка №{def} \
                 отрядом в {paws} — {parts} за {raid_ticks}: у шлюза выгоднее",
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "в рулсете нет вылазки с деталями — сравнивать не с чем"
    );
}

/// Номер выполнения едет в снимок: загруженная партия не находит окрестности
/// снова полными; признак заказа без отряда у идущей вылазки — тоже.
#[test]
fn the_tally_and_a_scavenge_survive_a_save() {
    let core = include_str!("../../assets/rulesets/core.yaml");
    let mut live = Sim::new(core).expect("рулсет");
    live.without_timeline();
    let def = live.mission_index("scavenge").expect("«Пошарить рядом»");
    assert!(live.scavenge(def, vec!["excellent".into()], -1));
    assert!(finishes(&mut live, def, 1500));
    assert!(live.scavenge(def, vec!["excellent".into()], -1));
    live.tick_n(5);
    let json = live.save().expect("снимок");
    let mut loaded = Sim::load_from(core, &json).expect("загрузка");
    assert_eq!(loaded.tally_of(def), 1, "счёт выполнений пережил загрузку");
    assert!(loaded.mission_is_nearby(), "заказ без отряда остался им");
}

/// Прогноз «Окрестностей» — тот же `NodeSnap`, что у отряда (§12.349):
/// по отмеченным свободным котам, на все заказы разом.
#[test]
fn the_forecast_follows_the_picked_cats() {
    let (mut sim, _) = yard();
    sim.set_nearby_pick(vec!["a".into(), "b".into()]);
    let node = sim.outskirts_snap();
    assert_eq!(node.crew, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(node.ready.len(), 2, "оба готовы идти");
    sim.set_nearby_pick(vec!["a".into()]);
    assert_eq!(sim.outskirts_snap().crew, vec!["a".to_string()]);
}
