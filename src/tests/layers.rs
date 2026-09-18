//! Слои (§12.237): пол кладут на пустоту, всё прочее — только на пол, а ластик
//! снимает один слой — постройку до пола, пол до пустоты.
//!
//! В синтетической схеме основания нет, и слоёв нет вовсе; заводит их
//! `set_base(0)` — пол схемы и есть тайл `0`. Тайл `1` здесь постройка, тайл
//! `2` — склад: оба переводятся через `force_tile`, как в `tidying`.

use super::sim_from;
use crate::sim::Sim;

const ROOM: [&str; 3] = ["#######", "#a....#", "#######"];
const WIDTH: usize = 7;

const FLOOR: i32 = 0;
const WALL: i32 = 1;
const STORE: i16 = 2;

fn layered() -> Sim {
    let mut sim = sim_from(&ROOM);
    sim.set_base(0, true);
    sim
}

// --- ворота разметки ---------------------------------------------------------

#[test]
fn a_building_stands_only_on_a_floor() {
    let mut sim = layered();
    assert!(
        !sim.add_blueprint(0, 1, WALL),
        "на пустоту постройку не ставят"
    );
    assert!(sim.add_blueprint(3, 1, WALL), "на пол — ставят");
}

#[test]
fn a_floor_goes_only_on_void() {
    let mut sim = layered();
    sim.force_tile(3, 1, WALL as i16);
    assert!(
        !sim.add_blueprint(3, 1, FLOOR),
        "пол поверх постройки не кладут"
    );
    assert!(sim.add_blueprint(3, 0, FLOOR), "на пустоту — кладут");
}

/// Замены одним жестом нет: сперва стирают (§12.237).
#[test]
fn nothing_goes_over_a_building() {
    let mut sim = layered();
    sim.force_tile(3, 1, WALL as i16);
    assert!(
        !sim.add_blueprint(3, 1, 2),
        "поверх постройки не встаёт другая"
    );
}

/// Без основания в палитре правила нет: схема ведёт себя как до §12.237.
#[test]
fn without_a_base_anything_goes_anywhere() {
    let mut sim = sim_from(&ROOM);
    assert!(
        sim.add_blueprint(0, 1, WALL),
        "постройка на пустоте, как раньше"
    );
}

/// Маска превью говорит то же, что фасад (§12.111): пустота под постройкой
/// красная, пол под полом красный.
#[test]
fn the_mask_agrees_with_the_facade() {
    let mut sim = layered();
    let wall = sim.buildable(WALL, 0, 0, 0, 0);
    assert_eq!(wall[WIDTH], 0, "постройке пустота запрещена");
    assert_eq!(wall[WIDTH + 3], 1, "а пол разрешён");
    let floor = sim.buildable(FLOOR, 0, 0, 0, 0);
    assert_eq!(floor[0], 1, "полу разрешена пустота");
    assert_eq!(floor[WIDTH + 3], 0, "а пол — нет");
}

// --- ластик ------------------------------------------------------------------

/// Стирание снимает ровно один слой и возвращает цену именно его.
#[test]
fn erasing_takes_one_layer_at_a_time() {
    let mut sim = layered();
    sim.set_cost(WALL as i16, 2);
    sim.force_tile(4, 1, WALL as i16);
    let before = sim.scrap_total();

    assert!(sim.plan_demolish(4, 1));
    sim.tick_n(200);
    assert_eq!(sim.tile(4, 1), 0, "постройка снята до пола");
    assert_eq!(sim.scrap_total(), before + 2, "вернулась цена постройки");

    assert!(sim.plan_demolish(4, 1));
    sim.tick_n(200);
    assert_eq!(sim.tile(4, 1), -1, "второй жест снял пол");
}

/// Рамка по смешанной области снимает только постройки: поклеточно она
/// оставила бы рваный край ям рядом с ними.
#[test]
fn the_eraser_takes_the_top_layer_of_the_rect() {
    let mut sim = layered();
    sim.force_tile(3, 1, WALL as i16);
    assert!(sim.plan_demolish_rect(2, 1, 3, 1));
    assert_eq!(sim.planned_tile(3, 1), Some(-1), "постройка — в снос");
    assert_eq!(sim.planned_tile(2, 1), None, "пол слева не тронут");
    assert_eq!(sim.planned_tile(4, 1), None, "и справа тоже");
}

// --- чистый пол -----------------------------------------------------------------

/// Постройка ждёт, пока уборка унесёт кучу с площадки, и встаёт на чистый пол.
/// Проверяется на каждом тике: конечное состояние замело бы след — куча уехала
/// бы и из-под построенного.
#[test]
fn a_building_waits_for_the_floor_to_be_tidied() {
    let mut sim = layered();
    sim.set_capacity(STORE, 20);
    sim.force_tile(5, 1, STORE);
    sim.put_scrap(3, 1, 3);
    assert!(sim.add_blueprint(3, 1, WALL));

    for _ in 0..400 {
        sim.tick_n(1);
        if sim.tile(3, 1) == WALL as i16 {
            assert_eq!(sim.scrap_at(3, 1), 0, "построено поверх кучи");
            break;
        }
    }
    assert_eq!(sim.tile(3, 1), WALL as i16, "постройка встала");
    assert_eq!(sim.scrap_at(5, 1), 3, "куча уехала на склад");
}

/// Везти некуда — ждать нечего: строят поверх кучи, как до §12.237. Иначе
/// первый склад, поставленный на стартовый запас, ждал бы сам себя.
#[test]
fn with_nowhere_to_tidy_a_building_goes_over_the_pile() {
    let mut sim = layered();
    sim.put_scrap(3, 1, 3);
    assert!(sim.add_blueprint(3, 1, WALL));
    sim.tick_n(200);
    assert_eq!(sim.tile(3, 1), WALL as i16, "построено");
    assert_eq!(sim.scrap_at(3, 1), 3, "куча осталась на месте");
}

// --- улучшения (§12.238) -----------------------------------------------------

/// Тайл `2` — улучшение тайла `1`: ставится только поверх него.
fn upgradable() -> Sim {
    let mut sim = layered();
    sim.set_on(2, WALL as i16);
    sim
}

#[test]
fn an_upgrade_goes_only_on_its_base() {
    let mut sim = upgradable();
    sim.force_tile(3, 1, WALL as i16);
    assert!(sim.add_blueprint(3, 1, 2), "поверх своей основы — ставится");
    assert!(!sim.add_blueprint(4, 1, 2), "на голый пол — нет");
}

#[test]
fn erasing_an_upgrade_returns_its_base() {
    let mut sim = upgradable();
    sim.force_tile(3, 1, 2);
    assert!(sim.plan_demolish(3, 1));
    sim.tick_n(200);
    assert_eq!(sim.tile(3, 1), WALL as i16, "улучшение снято до основы");
}

/// Рамка снимает самый высокий уровень под ней: улучшение, а постройку рядом
/// и пол не трогает — один жест, одна ступень.
#[test]
fn the_eraser_takes_the_highest_level() {
    let mut sim = upgradable();
    sim.force_tile(2, 1, WALL as i16);
    sim.force_tile(3, 1, 2);
    assert!(sim.plan_demolish_rect(2, 1, 3, 1));
    assert_eq!(sim.planned_tile(3, 1), Some(-1), "улучшение — в снос");
    assert_eq!(sim.planned_tile(2, 1), None, "постройка не тронута");
    assert_eq!(sim.planned_tile(4, 1), None, "пол не тронут");
}

// --- боевой рулсет -----------------------------------------------------------

/// На `core.yaml` стеллаж — улучшение склада: на пол не встаёт, поверх склада
/// встаёт, а стёртый возвращается складом.
#[test]
fn the_shipped_ruleset_upgrades_storage_to_a_rack() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    sim.add_storage();
    sim.set_tech("racks");
    let rack = sim.tile_index("rack").expect("стеллаж");
    let storage = sim.tile_index("storage").expect("склад");

    assert!(
        !sim.add_blueprint(8, 7, rack as i32),
        "на пол стеллаж не встаёт"
    );
    assert!(
        sim.add_blueprint(3, 2, rack as i32),
        "поверх склада — встаёт"
    );
    sim.tick_n(1500);
    assert_eq!(sim.tile(3, 2), rack, "стеллаж построен");

    assert!(sim.plan_demolish(3, 2));
    sim.tick_n(600);
    assert_eq!(sim.tile(3, 2), storage, "стёртый стеллаж вернулся складом");
}

/// На `core.yaml` основание — пол: стёртая лежанка стартовой застройки
/// оставляет пол, а не яму.
#[test]
fn the_shipped_ruleset_erases_a_bed_to_its_floor() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    let floor = sim.tile_index("floor").expect("пол");
    let bed = sim.tile_index("bed").expect("лежанка");
    assert_eq!(sim.tile(3, 13), bed, "лежанка в стартовой застройке");

    assert!(sim.plan_demolish(3, 13));
    sim.tick_n(600);
    assert_eq!(sim.tile(3, 13), floor, "под лежанкой остался пол");
}
