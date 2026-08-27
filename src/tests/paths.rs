//! Пути: BFS по построенному полу.

use super::sim_from;
use crate::map::BaseMap;
use crate::path::find_path;

#[test]
fn path_crosses_connected_floor() {
    let sim = sim_from(&["#####", "#...#", "#####"]);
    let rules = sim.world.resource::<crate::components::TileRules>();
    let map = sim.world.resource::<BaseMap>();
    let path = find_path(map, rules, (1, 1), (3, 1)).expect("путь должен быть");
    // Маршрут развёрнут: последний элемент — следующий шаг, первый — цель.
    assert_eq!(path.first(), Some(&(3, 1)));
    assert_eq!(path.last(), Some(&(2, 1)));
}

#[test]
fn no_path_across_gap() {
    let sim = sim_from(&["#####", "#.#.#", "#####"]);
    let rules = sim.world.resource::<crate::components::TileRules>();
    let map = sim.world.resource::<BaseMap>();
    assert!(find_path(map, rules, (1, 1), (3, 1)).is_none());
}

/// Свойство, на котором держится выход из ямы: старт может быть непроходим.
/// Кот на снесённой клетке всё ещё умеет проложить маршрут наружу.
#[test]
fn path_starts_from_unwalkable_tile() {
    let sim = sim_from(&["#####", "#...#", "#####"]);
    let mut sim = sim;
    sim.force_tile(1, 1, -1);
    let rules = sim.world.resource::<crate::components::TileRules>();
    let map = sim.world.resource::<BaseMap>();
    assert!(!map.walkable(rules, 1, 1));
    assert!(find_path(map, rules, (1, 1), (3, 1)).is_some());
}
