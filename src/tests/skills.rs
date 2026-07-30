//! Навыки и перки: рост от работы, скорость работы, объём лап (§12.17).
//!
//! Навыков в схеме по умолчанию нет — их заводит сам тест (`set_skill`), как
//! цену и ёмкость тайла в тестах материала. Порогом уровня удобно ставить
//! ровно столько очков, сколько кот набивает за один тайл (`BUILD_TICKS`).

use super::{BUILD_TICKS, sim_from};
use crate::sim::Sim;

const CORRIDOR: [&str; 3] = ["#########", "#a......#", "#########"];

/// Пороги «мастера»: пять уровней, каждый достижим сразу через `set_xp`.
const LEVELS: [i32; 5] = [10, 20, 30, 40, 50];

// --- рост ------------------------------------------------------------------

/// Базовый случай: кот строит — навык растёт ровно на тик работы за тик.
#[test]
fn building_trains_the_build_skill() {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &[BUILD_TICKS as i32, 100]);
    sim.add_blueprint(1, 2, 0); // у ног кота: идти никуда не надо

    sim.tick_n(BUILD_TICKS);
    assert_eq!(sim.tile(1, 2), 0, "тайл построен");
    assert_eq!(
        sim.xp_of("a", build),
        BUILD_TICKS as i32,
        "опыт капал за каждый тик работы"
    );
    assert_eq!(sim.level_of("a", build), 1, "и хватило на первый уровень");

    sim.tick_n(50);
    assert_eq!(
        sim.xp_of("a", build),
        BUILD_TICKS as i32,
        "без работы навык не растёт"
    );
}

/// Снос — тот же навык, что и стройка: домен один, потому что и джоб один.
#[test]
fn demolition_trains_the_same_skill_as_building() {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &[100]);
    assert!(sim.plan_demolish(4, 1), "снос запланирован");

    sim.tick_n(BUILD_TICKS + 6);
    assert_eq!(sim.tile(4, 1), -1, "клетка снесена");
    assert!(sim.xp_of("a", build) > 0, "«Стройка» выросла и на сносе");
}

/// Перенос — не домен работы: за ходки на склад «Стройка» не растёт. Носить
/// коты умеют и без навыка, это сложение, а не мастерство.
#[test]
fn hauling_trains_nothing() {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &[10]);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 4);

    sim.tick_n(200);
    assert_eq!(sim.scrap_at(7, 1), 4, "лом убран на склад");
    assert_eq!(sim.xp_of("a", build), 0, "но навык за это не вырос");
}

/// Опыт упирается в последний порог: потолок — длина списка уровней.
#[test]
fn experience_stops_at_the_cap() {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &[10, 20]);
    sim.add_blueprint_rect(1, 2, 5, 1, 0); // работы заведомо больше, чем нужно

    sim.tick_n(400);
    assert_eq!(sim.floors_left([1, 2, 5, 1]), 5, "вся рамка построена");
    assert_eq!(sim.xp_of("a", build), 20, "опыт встал на последнем пороге");
    assert_eq!(sim.level_of("a", build), 2, "уровень — на потолке");
}

// --- скорость --------------------------------------------------------------

/// Навык ускоряет работу: на потолке (+5 очков к базовым 10) тайл выходит за
/// 8 тиков вместо 12.
#[test]
fn a_higher_skill_builds_the_same_tile_faster() {
    assert_eq!(ticks_to_build(0), 12, "нулевой навык — базовая скорость");
    assert_eq!(
        ticks_to_build(LEVELS[4]),
        8,
        "пятый уровень — в полтора раза быстрее"
    );
}

/// Сколько тиков уходит на один тайл у кота с таким опытом.
fn ticks_to_build(xp: i32) -> usize {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_xp("a", build, xp);
    sim.add_blueprint(1, 2, 0);

    for t in 1..100 {
        sim.tick_n(1);
        if sim.tile(1, 2) == 0 {
            return t;
        }
    }
    panic!("тайл так и не построен");
}

/// Навык не участвует в выборе исполнителя: работу берёт ближний кот, а не
/// умелый. Иначе вернулась бы регрессия §12.14 — беготня через полкарты.
#[test]
fn skill_does_not_decide_who_takes_the_job() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_xp("b", build, LEVELS[4]); // мастер — на дальнем конце коридора
    sim.add_blueprint(1, 2, 0); // а чертёж у ног новичка

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "чертёж взял ближний кот");
    assert!(
        !sim.has_assignment("b"),
        "мастер за ним через полкарты не пошёл"
    );
}

// --- перк «Носильщик»: объём лап -------------------------------------------

/// Кот берёт с кучи не больше, чем влезает в лапы; остаток остаётся лежать на
/// виду и уезжает следующими ходками.
#[test]
fn paws_limit_what_a_cat_picks_up() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(1, 20);
    sim.force_tile(7, 1, 1);
    sim.put_scrap(2, 1, 10);
    sim.set_carry("a", 3);

    for _ in 0..50 {
        sim.tick_n(1);
        if sim.carrying_of("a") > 0 {
            break;
        }
    }
    assert_eq!(sim.carrying_of("a"), 3, "в лапы влезло только три");
    assert_eq!(sim.scrap_at(2, 1), 7, "остальное осталось на полу");

    sim.tick_n(400);
    assert_eq!(
        sim.scrap_at(7, 1),
        10,
        "и уехало на склад следующими ходками"
    );
    assert_eq!(sim.scrap_total(), 10, "лом не пропал и не размножился");
}

/// Площадка дороже лап снабжается в несколько ходок — стройка от этого не
/// встаёт, просто ждёт дозаправки.
#[test]
fn a_site_costlier_than_paws_is_supplied_in_several_trips() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_cost(0, 5);
    sim.set_carry("a", 2);
    sim.put_scrap(3, 1, 5);
    sim.add_blueprint(1, 2, 0);

    // Первая ходка приносит ровно то, что влезло в лапы, а не всю цену.
    for _ in 0..50 {
        sim.tick_n(1);
        if sim.delivered_at(1, 2).unwrap_or(0) > 0 {
            break;
        }
    }
    assert_eq!(sim.delivered_at(1, 2), Some(2), "за ходку донёс только две");

    sim.tick_n(400);
    assert_eq!(sim.tile(1, 2), 0, "площадка дозаправлена и построена");
    assert_eq!(sim.scrap_at(3, 1), 0, "куча ушла в стройку целиком");
    assert_eq!(
        sim.scrap_total(),
        0,
        "и потрачена на тайл: цена тайла — вся куча"
    );
}

// --- боевой рулсет ---------------------------------------------------------

/// Проверка на настоящем `core.yaml`: бригада действительно растёт от работы,
/// а перк из рулсета доходит до лап. Ловит рассогласование кода и контента —
/// навык под другим `id`, забытые `levels`, пропавший `carry`, — которого
/// синтетические схемы не увидят.
#[test]
fn the_shipped_ruleset_grows_skills_and_deals_paws() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).ok().expect("рулсет должен разбираться");
    let build = sim.skill_index("build").expect("навык стройки в рулсете");

    assert_eq!(sim.carry_max_of("sp2"), 8, "«Носильщик» удваивает лапы");
    assert_eq!(sim.carry_max_of("sp3"), 4, "у остальных — базовые");

    assert!(
        sim.plan_demolish_rect(6, 10, 3, 4),
        "сносим половину гаража"
    );
    sim.tick_n(3000);
    let crew = ["excellent", "sp2", "sp3"];
    assert!(
        crew.iter().any(|c| sim.xp_of(c, build) > 0),
        "на разборке гаража бригада набрала опыт"
    );
}
