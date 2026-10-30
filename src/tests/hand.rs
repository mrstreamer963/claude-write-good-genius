//! Приказ «взять и положить» (§12.303): куча и ячейка отвечают на приказ
//! действием с вещью, а ограничения кота те же, что у раздачи.

use super::*;
use crate::hauling::HAND_HOLD;

/// Кот с лапами на 4, куча из 10 и склад на 6 в конце коридора.
fn yard() -> Sim {
    let mut sim = sim_from(&["#########", "#a......#", "#########"]);
    sim.set_capacity(1, 6);
    sim.force_tile(7, 1, 1);
    sim.set_carry("a", 4);
    sim.put_scrap(3, 1, 10);
    sim.set_auto_tidy(false);
    sim
}

#[test]
fn a_cat_takes_only_what_fits_in_its_paws() {
    let mut sim = yard();
    assert_eq!(sim.hand_tag("a", 3, 1), "take");
    assert!(sim.command_at("a", 3, 1));
    sim.tick_n(20);
    assert_eq!(sim.carrying_of("a"), 4);
    assert_eq!(sim.scrap_at(3, 1), 6);
}

#[test]
fn a_second_order_puts_the_load_into_the_named_cell() {
    let mut sim = yard();
    sim.force_tile(6, 1, 1); // ближе, и сама раздача выбрала бы её
    sim.command_at("a", 3, 1);
    sim.tick_n(20);
    assert_eq!(sim.hand_tag("a", 7, 1), "put");
    assert!(sim.command_at("a", 7, 1));
    sim.tick_n(40);
    assert_eq!(sim.scrap_at(7, 1), 4);
    assert_eq!(sim.scrap_at(6, 1), 0);
}

#[test]
fn a_load_without_a_second_order_goes_to_the_usual_hauling() {
    let mut sim = yard();
    sim.command_at("a", 3, 1);
    sim.tick_n(20 + HAND_HOLD as usize + 40);
    assert!(sim.scrap_at(7, 1) >= 4, "груз не застрял в лапах");
}

#[test]
fn a_full_cell_refuses_and_keeps_the_load() {
    let mut sim = yard();
    sim.put_scrap(7, 1, 6);
    sim.command_at("a", 3, 1);
    sim.tick_n(20);
    assert_eq!(sim.hand_tag("a", 7, 1), "full");
    assert!(!sim.command_at("a", 7, 1));
    assert_eq!(sim.carrying_of("a"), 4);
}

#[test]
fn a_load_goes_to_the_blueprint_that_needs_it() {
    let mut sim = yard();
    sim.set_cost(2, 3);
    sim.command_at("a", 3, 1);
    sim.tick_n(20);
    sim.add_blueprint(5, 1, 2);
    assert_eq!(sim.hand_tag("a", 5, 1), "site");
    assert!(sim.command_at("a", 5, 1));
    sim.tick_n(3);
    assert!(sim.has_haul("a"));
    sim.tick_n(30);
    assert!(sim.delivered_at(5, 1).is_some_and(|n| n > 0) || sim.tile(5, 1) == 2);
}

#[test]
fn a_blueprint_refuses_what_it_does_not_need() {
    let mut sim = yard();
    sim.command_at("a", 3, 1);
    sim.tick_n(20);
    sim.add_blueprint(5, 1, 2); // цена нулевая: материал не нужен
    assert_eq!(sim.hand_tag("a", 5, 1), "unneeded");
}

#[test]
fn a_plain_floor_stays_a_plain_move() {
    let mut sim = yard();
    assert_eq!(sim.hand_tag("a", 5, 1), "");
    assert!(sim.command_at("a", 5, 1));
    sim.tick_n(12);
    assert_eq!(sim.pos_of("a"), (5, 1));
    assert_eq!(sim.carrying_of("a"), 0);
}

#[test]
fn the_pile_names_why_it_stays() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    sim.put_scrap(3, 1, 5);
    sim.tick_n(2);
    assert_eq!(sim.pile_why(3, 1), "nostore");
    sim.set_capacity(1, 9);
    sim.force_tile(2, 1, 1);
    sim.tick_n(1);
    assert_ne!(sim.pile_why(3, 1), "nostore");
    assert_eq!(sim.pile_why(2, 1), "");
}

#[test]
fn a_load_goes_to_the_shop_that_needs_it() {
    let mut sim = yard();
    sim.set_shop(2, true);
    sim.force_tile(5, 1, 2);
    sim.put_scrap(7, 1, 4);
    let recipe = sim.set_recipe(5000, &[(0, 2)], &[(1, 1)], &[]);
    sim.command_at("a", 3, 1);
    sim.tick_n(20);
    assert!(sim.start_craft(recipe, 1));
    assert_eq!(sim.hand_tag("a", 5, 1), "shop");
    assert!(sim.command_at("a", 5, 1));
    sim.tick_n(12);
    assert!(sim.craft_delivered().is_some_and(|n| n > 0));
}
