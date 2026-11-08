//! Сборная вылазка (§12.351): на один заказ разом идут несколько отрядов и
//! свободные коты. Дорога у каждой группы своя, работа на месте общая и
//! убывает по лапам дошедших, исход — по всем, кто был на месте.

use super::*;

/// Два гаража в нижнем ряду, коты `a`, `b`, `c` в верхнем. Гараж здесь — и
/// стол, и шлюз (`set_gate`), как в тестах вылазок.
fn two_tables() -> Sim {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(1, 2, 1);
    sim.force_tile(5, 2, 1);
    sim
}

fn until_home(sim: &mut Sim, ids: &[&str]) -> u32 {
    let mut ticks = 0;
    while ids.iter().any(|id| sim.is_away(id) || sim.in_squad(id)) {
        sim.tick_n(1);
        ticks += 1;
        assert!(ticks < 2000, "вылазка не вернулась");
    }
    ticks
}

/// Два отряда и свободный кот уходят на один заказ: три группы, одна вылазка.
#[test]
fn two_squads_and_a_free_cat_go_together() {
    let mut sim = two_tables();
    let m = sim.set_mission(1, 20, &[(0, 6)]);
    sim.set_squad_range(m, 1, 3);
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.enlist("b", 5, 2));
    assert!(sim.launch_group(m, vec![1, 2, 5, 2], vec!["c".into()], -1));
    assert_eq!(sim.raid_count(), 3, "по группе на отряд и на свободного");
    assert!(sim.in_squad("a") && sim.in_squad("b") && sim.in_squad("c"));

    // Второй сборной на тот же заказ не бывает.
    assert!(!sim.launch_group(m, vec![], vec!["c".into()], -1));

    until_home(&mut sim, &["a", "b", "c"]);
    assert_eq!(sim.raid_count(), 0, "все группы вернулись");
    assert_eq!(sim.scrap_total(), 6, "добыча целиком дошла до базы");
}

/// Каждая следующая лапа работу ускоряет: втроём вылазка короче, чем вдвоём.
#[test]
fn more_paws_finish_the_work_sooner() {
    let solo = {
        let mut sim = two_tables();
        let m = sim.set_mission(1, 20, &[]);
        sim.set_squad_range(m, 1, 3);
        sim.set_mission_work(m, 60);
        sim.enlist("a", 1, 2);
        assert!(sim.launch_group(m, vec![1, 2], vec![], -1));
        until_home(&mut sim, &["a"])
    };
    let team = {
        let mut sim = two_tables();
        let m = sim.set_mission(1, 20, &[]);
        sim.set_squad_range(m, 1, 3);
        sim.set_mission_work(m, 60);
        sim.enlist("a", 1, 2);
        sim.enlist("b", 5, 2);
        assert!(sim.launch_group(m, vec![1, 2, 5, 2], vec!["c".into()], -1));
        until_home(&mut sim, &["a", "b", "c"])
    };
    assert!(team + 20 < solo, "втроём {team}, в одиночку {solo}");
}

/// Добыча делится по лапам на месте, каждая группа несёт свою долю на свой
/// шлюз; остаток от деления — первой по столу.
#[test]
fn loot_is_split_by_paws_on_site() {
    let mut sim = two_tables();
    let m = sim.set_mission(1, 20, &[(0, 5)]);
    sim.set_squad_range(m, 1, 3);
    // Работа длиннее разницы в уходе: на месте окажутся обе группы.
    sim.set_mission_work(m, 20);
    sim.enlist("a", 1, 2);
    sim.enlist("b", 5, 2);
    assert!(sim.launch_group(m, vec![1, 2, 5, 2], vec![], -1));
    // Обе группы дома — каждая положила свою долю на свой шлюз.
    let mut seen = (0, 0);
    for _ in 0..400 {
        sim.tick_n(1);
        seen = (
            seen.0.max(sim.scrap_at(1, 2)),
            seen.1.max(sim.scrap_at(5, 2)),
        );
        if sim.raid_count() == 0 {
            break;
        }
    }
    assert_eq!(seen, (3, 2), "5 на двоих: 2 + 2 и остаток первому");
}

/// Опоздавшая группа в исход не входит и разворачивается с полпути пустой.
#[test]
fn a_late_group_turns_back_empty() {
    let mut sim = sim_from(&["##########", "#a......b#", "##########"]);
    sim.set_gate(1, true);
    sim.force_tile(1, 1, 1);
    let m = sim.set_mission(1, 20, &[(0, 4)]);
    sim.set_squad_range(m, 1, 2);
    sim.set_mission_work(m, 2);
    // `b` далеко от шлюза: до него семь клеток, а работы на два тика.
    assert!(sim.launch_group(m, vec![], vec!["a".into(), "b".into()], -1));
    until_home(&mut sim, &["a", "b"]);
    assert_eq!(sim.scrap_total(), 4, "добычу принёс тот, кто был на месте");
}

/// Усталость на месте списывается за тик, а не разом на конце работы.
#[test]
fn fatigue_is_paid_while_working() {
    let mut sim = two_tables();
    sim.set_needs(1000, 0, 0);
    let m = sim.set_risky_mission(1, 20, 0, 400, &[]);
    sim.set_mission_work(m, 40);
    sim.enlist("a", 1, 2);
    sim.set_energy("a", 1000);
    assert!(sim.launch_group(m, vec![1, 2], vec![], -1));
    while sim.mission_phase() != Some("work") {
        sim.tick_n(1);
    }
    let start = sim.energy_of("a");
    sim.tick_n(20);
    let mid = sim.energy_of("a");
    assert!(
        start - mid >= 150 && start - mid <= 250,
        "полработы — полцены: {start} → {mid}"
    );
}

/// Пустой отряд на вылазку не идёт (§12.352): сборная с ним отклоняется
/// целиком, а прогноз «Идут» называет его по имени — тем же выражением.
#[test]
fn an_empty_squad_cannot_go() {
    let mut sim = two_tables();
    let m = sim.set_mission(1, 20, &[(0, 6)]);
    sim.set_squad_range(m, 1, 3);
    assert!(sim.enlist("a", 1, 2));
    sim.set_group_pick(vec![1, 2, 5, 2], vec![]);
    assert_eq!(sim.outskirts_snap().empty, vec!["Отряд 2".to_string()]);
    assert!(!sim.launch_group(m, vec![1, 2, 5, 2], vec![], -1));
    assert_eq!(sim.raid_count(), 0, "не ушёл никто");
    assert!(!sim.in_squad("a"));

    // Пустой убрали — непустой уходит.
    sim.set_group_pick(vec![1, 2], vec![]);
    assert!(sim.outskirts_snap().empty.is_empty());
    assert!(sim.launch_group(m, vec![1, 2], vec![], -1));
}
