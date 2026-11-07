//! Место брифинга: отряд садится за стол и, когда сели все, идёт к ближайшему
//! шлюзу (§12.279).
//!
//! Остальные тесты вылазок ставят тайл, который одновременно и стол, и дверь
//! (`set_gate`), — то есть гараж до §12.279. Здесь они разведены: стол — штамп
//! 3×3, где строится только столешница посередине, а восемь клеток вокруг —
//! свободные слоты (§12.283); шлюзов два, на разном расстоянии от стола.

use super::*;
use crate::missions::SQUAD_NAME_MAX;

const TABLE: i16 = 2;
const EXIT: i16 = 3;
/// Любая постройка, которую пробуют поставить на слот.
const OTHER: i16 = 4;

/// Восемь клеток вокруг середины сетки 3×3 — свободные слоты стола.
const RING8: [(i32, i32); 8] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (0, 1),
    (2, 1),
    (0, 2),
    (1, 2),
    (2, 2),
];

/// Штамп «стол со слотами вокруг» — дословно `muster` боевого рулсета.
fn table_with_slots(sim: &mut Sim) -> usize {
    let def = sim.set_structure(vec![
        vec![None, None, None],
        vec![None, Some(TABLE), None],
        vec![None, None, None],
    ]);
    sim.set_structure_slots(def, &RING8);
    def
}

/// Ближний к столу шлюз — `(1, 4)`, дальний — `(9, 1)`.
const NEAR: (i32, i32) = (1, 4);
const FAR: (i32, i32) = (9, 1);

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Комната с местом сбора якорем в `(2, 1)` и двумя шлюзами. Коты стоят у
/// дальнего края, чтобы рассадка была видна: до стола им идти, а не стоять.
fn hall() -> Sim {
    let mut sim = sim_from(&[
        "###########",
        "#.........#",
        "#.........#",
        "#.........#",
        "#......a.b#",
        "###########",
    ]);
    sim.set_muster(TABLE, true);
    sim.set_solid(TABLE, true);
    sim.set_exit(EXIT, true);
    let def = table_with_slots(&mut sim);
    sim.force_structure(def, 2, 1);
    sim.force_tile(NEAR.0, NEAR.1, EXIT);
    sim.force_tile(FAR.0, FAR.1, EXIT);
    sim
}

/// Ушёл ли отряд целиком за `ticks` тиков. Ловится сам момент ухода:
/// вылазка короткая, и к концу ожидания отряд мог уже вернуться.
fn leaves_within(sim: &mut Sim, ticks: usize) -> bool {
    (0..ticks).any(|_| {
        sim.tick_n(1);
        sim.is_away("a") && sim.is_away("b")
    })
}

/// Места по `id`: сначала стороны стола, верх и низ (`seat_order`), — двое
/// садятся друг напротив друга, а не в ряд.
const SEAT_A: (i32, i32) = (3, 1);
const SEAT_B: (i32, i32) = (3, 3);

/// Восемь стульев — это **один** отряд, а не восемь (§12.161): слот держит
/// объект целиком.
#[test]
fn a_table_with_eight_seats_is_one_squad() {
    let mut sim = hall();
    assert_eq!(
        sim.gate_count(),
        1,
        "мест брифинга — одно, шлюзы в счёт не идут"
    );
}

/// Никто не уходит, пока не сели все: ушедший в момент, когда сосед ещё
/// шёл к стулу, — это отряд, который «дружно» не выходит.
#[test]
fn a_squad_sits_down_before_it_walks_to_the_door() {
    let mut sim = hall();
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));

    let (mut a_sat, mut b_sat) = (false, false);
    for _ in 0..200 {
        sim.tick_n(1);
        a_sat |= sim.pos_of("a") == SEAT_A;
        b_sat |= sim.pos_of("b") == SEAT_B;
        if sim.is_away("a") || sim.is_away("b") {
            assert!(a_sat && b_sat, "ушли, не рассевшись: a {a_sat}, b {b_sat}");
            assert!(sim.is_away("a") && sim.is_away("b"), "уходят вместе");
            assert_eq!(sim.mission_exit(), Some(NEAR), "через ближний шлюз");
            return;
        }
    }
    panic!("отряд так и не ушёл");
}

/// Дверь не выбрана, пока отряд рассаживается: выбрать её раньше значило бы
/// звать к ней тех, кто ещё не сел.
#[test]
fn the_door_is_chosen_only_once_everyone_is_seated() {
    let mut sim = hall();
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));
    sim.tick_n(1);
    assert_eq!(sim.mission_exit(), None, "все ещё в пути к столу");
}

/// Без единого шлюза севший отряд ждёт за столом: уйти ему некуда, а
/// распускать его не за что — стол на месте, и шлюз можно построить.
#[test]
fn without_a_door_the_squad_waits_at_the_table() {
    let mut sim = hall();
    sim.force_tile(NEAR.0, NEAR.1, 0);
    sim.force_tile(FAR.0, FAR.1, 0);
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));

    sim.tick_n(200);
    assert!(!sim.is_away("a") && !sim.is_away("b"), "уйти некуда");
    assert_eq!(sim.pos_of("a"), SEAT_A);
    assert_eq!(sim.pos_of("b"), SEAT_B);
    assert_eq!(sim.mission_exit(), None);
    assert_eq!(sim.raid_count(), 1, "заявка жива");

    // Построили шлюз — отряд уходит сам, второго клика не нужно.
    sim.force_tile(FAR.0, FAR.1, EXIT);
    assert!(leaves_within(&mut sim, 100), "ушли через новый шлюз");
}

/// Снесённый по дороге шлюз заменяется следующим ближайшим: дверь не
/// приписана к отряду, и выбирать её снова — не отмена решения игрока.
#[test]
fn a_door_torn_down_on_the_way_is_replaced_by_the_next_nearest() {
    let mut sim = hall();
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));
    for _ in 0..200 {
        sim.tick_n(1);
        if sim.mission_exit().is_some() {
            break;
        }
    }
    assert_eq!(sim.mission_exit(), Some(NEAR));

    sim.force_tile(NEAR.0, NEAR.1, 0);
    sim.tick_n(1);
    assert_eq!(sim.mission_exit(), Some(FAR), "взяли следующий");
    assert!(leaves_within(&mut sim, 100), "ушли через дальний");
}

/// Добыча ложится на тот шлюз, через который ушли, а не на стол: дверь —
/// единственное место контакта с миром (§12.22).
#[test]
fn loot_lands_on_the_door_the_squad_left_through() {
    let mut sim = hall();
    let def = sim.set_mission(2, 30, &[(0, 5)]);
    assert!(sim.launch(def, squad(&["a", "b"])));
    sim.tick_n(300);

    assert!(!sim.is_away("a"), "вернулись");
    assert_eq!(sim.raid_count(), 0);
    // Уборки в схеме нет — склада нет, — так что куча лежит, где упала.
    assert_eq!(sim.scrap_at(NEAR.0, NEAR.1), 5, "добыча у ближнего шлюза");
}

/// Снесённый стол распускает собирающийся отряд — как снесённый станок
/// уносит свой заказ (§12.96).
#[test]
fn a_torn_down_table_disbands_the_gathering_squad() {
    let mut sim = hall();
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));
    sim.tick_n(1);

    sim.force_tile(3, 2, 0); // стол снесли
    sim.tick_n(1);
    assert_eq!(sim.raid_count(), 0);
    assert!(!sim.in_squad("a") && !sim.in_squad("b"));
}

/// Брифинг (§12.280): севший отряд сидит за столом `brief` тиков и только
/// потом выбирает дверь. Дверь, выбранная раньше, звала бы к выходу посреди
/// разговора.
#[test]
fn a_seated_squad_sits_through_the_briefing() {
    let mut sim = hall();
    sim.set_brief(TABLE, 20);
    let def = sim.set_mission(2, 30, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));

    // Ждём, пока сядут все.
    let mut sat_at = None;
    for t in 0..200 {
        sim.tick_n(1);
        if sim.pos_of("a") == SEAT_A && sim.pos_of("b") == SEAT_B {
            sat_at = Some(t);
            break;
        }
    }
    assert!(sat_at.is_some(), "так и не сели");

    sim.tick_n(15);
    assert_eq!(sim.mission_exit(), None, "брифинг ещё идёт");
    assert_eq!(sim.pos_of("a"), SEAT_A, "и сидят на местах");

    sim.tick_n(10);
    assert_eq!(sim.mission_exit(), Some(NEAR), "отслушали — к шлюзу");
}

/// Сначала стороны стола, потом углы (§12.279): первые четверо садятся
/// крестом, а не шеренгой по верхнему ряду.
#[test]
fn seats_fill_the_sides_before_the_corners() {
    let ring = vec![
        (2, 1),
        (3, 1),
        (4, 1),
        (2, 2),
        (4, 2),
        (2, 3),
        (3, 3),
        (4, 3),
    ];
    assert_eq!(
        crate::missions::seat_order(ring),
        vec![
            (3, 1),
            (3, 3),
            (2, 2),
            (4, 2),
            (2, 1),
            (4, 1),
            (2, 3),
            (4, 3)
        ],
    );
}

/// Стол брифинга шумный и грязный (§12.281): лежанку вплотную к нему, даже
/// наискосок, не поставить — а через клетку пола можно. Боевой рулсет:
/// стартовая комната для того и удлинена на ряд.
#[test]
fn the_shipped_ruleset_keeps_beds_away_from_the_muster_table() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.add_squad(); // стол там, где он стоял на старте до §12.349
    let bed = i32::from(sim.tile_index("bed").expect("лежанка"));
    // Стол брифинга — якорем в (3, 10): слоты x 3–5, нижний ряд на y = 12.
    assert!(!sim.add_blueprint(5, 13, bed), "вплотную под слотом");
    assert!(!sim.add_blueprint(6, 13, bed), "наискосок от угла");
    assert!(sim.add_blueprint(7, 13, bed), "через клетку — можно");
}

/// Недостроенный стол — ещё не отряд (§12.282): стулья бесплатны и встают
/// раньше стола, и до §12.282 такой объект уже звал набирать состав.
#[test]
fn an_unfinished_table_is_not_a_squad_yet() {
    let mut sim = hall();
    sim.force_tile(3, 2, 0); // стол ещё не построен, слоты на месте
    assert_eq!(sim.gate_count(), 0, "слоты без стола — не отряд");
    let def = sim.set_mission(2, 30, &[]);
    assert!(!sim.launch(def, squad(&["a", "b"])), "уйти не за кем");

    sim.force_tile(3, 2, TABLE);
    assert_eq!(sim.gate_count(), 1, "достроили — отряд появился");
}

// --- свободные слоты (§12.283) ----------------------------------------------

/// Строится только стол: слоты — это пол, который никто не перестраивает.
#[test]
fn placing_the_table_builds_only_the_table() {
    let mut sim = sim_from(&["#######", "#.....#", "#.....#", "#.....#", "#######"]);
    sim.set_muster(TABLE, true);
    sim.set_solid(TABLE, true);
    let def = table_with_slots(&mut sim);
    assert!(sim.place_structure(def as i32, 1, 1, 0));

    assert_eq!(sim.planned_tile(2, 2), Some(TABLE), "чертёж стола");
    for (dx, dy) in RING8 {
        assert_eq!(
            sim.planned_tile(1 + dx, 1 + dy),
            None,
            "на слоте чертежа нет"
        );
    }
}

/// На слоте стоящего стола не строится ничего, а рядом — можно.
#[test]
fn nothing_is_built_on_a_slot() {
    let mut sim = hall();
    assert!(
        !sim.add_blueprint(3, 1, i32::from(OTHER)),
        "слот держит место"
    );
    assert!(
        sim.add_blueprint(6, 1, i32::from(OTHER)),
        "за слотами — свободно"
    );
}

/// Слоту нужен свободный пол: стол, чей слот лёг бы на постройку или в
/// пустоту, не ставится вовсе — у объекта отказ один на весь штамп.
#[test]
fn a_slot_needs_free_floor() {
    let mut sim = hall();
    // Слои (§12.237): без основания в палитре «свободный пол» — это любая
    // проходимая клетка, и постройка от пола не отличается.
    sim.set_base(0, true);
    let def = table_with_slots(&mut sim);
    sim.force_tile(7, 1, OTHER);
    assert!(
        !sim.place_structure(def as i32, 6, 1, 0),
        "слот на постройке"
    );
    assert!(!sim.place_structure(def as i32, 0, 1, 0), "слот в пустоте");
    assert!(
        !sim.place_structure(def as i32, 4, 1, 0),
        "слот на чужом слоте"
    );
}

/// Снесли стол — слоты свободны: объекта нет, держать место некому.
#[test]
fn tearing_down_the_table_frees_its_slots() {
    let mut sim = hall();
    sim.force_tile(3, 2, 0);
    sim.tick_n(1); // `prune_structures` снимает объект
    assert!(sim.add_blueprint(3, 1, i32::from(OTHER)));
}

/// Номер отряда держится за отрядом, а не за местом на карте (§12.284): стол,
/// поставленный выше стартового, становится вторым, а не первым, — и порядок
/// переживает сохранение, потому что едет у самого объекта.
#[test]
fn a_new_table_takes_the_next_number_wherever_it_stands() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).expect("рулсет");
    sim.add_squad(); // стартовый стол до §12.349
    let start = *sim.node_cells_of().first().expect("стартовый стол");
    let def = sim.structure_index("muster").expect("место брифинга");
    // Северная комната — выше стартовой: по обходу карты он встал бы первым.
    sim.force_structure(def, 4, 2);
    let order = sim.node_cells_of();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0], start, "стартовый отряд остался первым");
    assert!(
        order[1].1 < start.1,
        "второй стоит выше, а номер у него второй"
    );

    let saved = sim.save().expect("снимок");
    let mut loaded = Sim::load(yaml, &saved).expect("загрузка");
    assert_eq!(loaded.node_cells_of(), order, "порядок пережил сохранение");
}

// --- радист на брифинге (§12.286) ------------------------------------------

/// Рация в зале и её клетка.
const RADIO: i16 = 5;
const RADIO_AT: (i32, i32) = (9, 3);
/// Стул радиста: третий по `seat_order` — левая сторона стола.
const SEAT_OP: (i32, i32) = (2, 2);

/// Тот же зал, плюс рация и кот `c` — радист отряда стола.
fn hall_with_radio() -> Sim {
    let mut sim = sim_from(&[
        "###########",
        "#.........#",
        "#.........#",
        "#.........#",
        "#....c.a.b#",
        "###########",
    ]);
    sim.set_muster(TABLE, true);
    sim.set_solid(TABLE, true);
    sim.set_exit(EXIT, true);
    let def = table_with_slots(&mut sim);
    sim.force_structure(def, 2, 1);
    sim.force_tile(NEAR.0, NEAR.1, EXIT);
    sim.force_tile(FAR.0, FAR.1, EXIT);
    sim.set_relay(RADIO, true);
    sim.set_comms(RADIO, 1);
    sim.force_tile(RADIO_AT.0, RADIO_AT.1, RADIO);
    let node = sim.node_cells_of()[0];
    assert!(sim.post_relay("c", node.0, node.1), "радист приписан");
    sim
}

/// Радист слушает брифинг вместе с отрядом, а за рацию садится, когда отряд
/// встал и пошёл к двери: до этого рация пуста.
#[test]
fn the_operator_sits_through_the_briefing_then_takes_the_radio() {
    let mut sim = hall_with_radio();
    let def = sim.set_mission(2, 60, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));

    let mut sat = false;
    for _ in 0..300 {
        sim.tick_n(1);
        if sim.mission_exit().is_none() {
            assert_ne!(
                sim.duty_of("c"),
                Some(RADIO_AT),
                "на брифинге за рацию не садятся"
            );
            sat |= sim.pos_of("c") == SEAT_OP;
        }
        if sim.is_away("a") {
            assert!(sat, "радист сидел за столом");
            assert_eq!(
                sim.duty_of("c"),
                Some(RADIO_AT),
                "а ушли — при радисте в эфире"
            );
            assert_eq!(sim.pos_of("c"), RADIO_AT);
            return;
        }
    }
    panic!("отряд так и не ушёл");
}

/// Спящий радист держит брифинг, как спящий боец: дверь не выбирается, пока
/// он не сел за стол.
#[test]
fn a_sleeping_operator_holds_the_briefing() {
    let mut sim = hall_with_radio();
    sim.set_needs(1000, 30, 1);
    sim.set_energy("c", 0);
    let def = sim.set_mission(2, 60, &[]);
    assert!(sim.launch(def, squad(&["a", "b"])));
    sim.tick_n(120);

    assert!(sim.is_resting("c"), "радист спит");
    assert_eq!(sim.mission_exit(), None, "брифинг не начат без радиста");
    assert!(!sim.is_away("a"), "отряд дома");
}

// --- имя отряда (§12.300) ---------------------------------------------------

/// Боевой рулсет и второй стол в северной комнате: два отряда.
fn two_squads() -> (Sim, (i32, i32), (i32, i32)) {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).expect("рулсет");
    sim.add_squad(); // стартовый стол до §12.349
    let def = sim.structure_index("muster").expect("место брифинга");
    sim.force_structure(def, 4, 2);
    let order = sim.node_cells_of();
    (sim, order[0], order[1])
}

/// Без имени отряд называется номером, с именем — именем, и имя переживает
/// сохранение: оно живёт на объекте и едет в снимке вместе с ним.
#[test]
fn a_squad_keeps_the_name_the_player_gave_it() {
    let (mut sim, first, second) = two_squads();
    assert_eq!(sim.squad_labels(), ["Отряд 1", "Отряд 2"]);

    assert_eq!(sim.rename_squad(second.0, second.1, "  Мусорщики  "), "");
    assert_eq!(sim.squad_labels(), ["Отряд 1", "Мусорщики"]);

    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let saved = sim.save().expect("снимок");
    let mut loaded = Sim::load(yaml, &saved).expect("загрузка");
    assert_eq!(loaded.squad_labels(), ["Отряд 1", "Мусорщики"]);
    assert_eq!(loaded.rename_squad(first.0, first.1, "Альфа"), "");
    assert_eq!(loaded.squad_labels(), ["Альфа", "Мусорщики"]);
}

/// Пустое имя и своё номерное возвращают умолчание: безымянный отряд
/// представлен одним способом, а не двумя.
#[test]
fn an_empty_or_own_number_name_restores_the_default() {
    let (mut sim, first, _) = two_squads();
    assert_eq!(sim.rename_squad(first.0, first.1, "Альфа"), "");
    assert_eq!(sim.rename_squad(first.0, first.1, "   "), "");
    assert_eq!(sim.squad_labels()[0], "Отряд 1");

    assert_eq!(sim.rename_squad(first.0, first.1, "Альфа"), "");
    assert_eq!(sim.rename_squad(first.0, first.1, "отряд 1"), "");
    assert_eq!(sim.squad_labels()[0], "Отряд 1");
}

/// Двух отрядов с одним именем не бывает — ни с именем игрока, ни с чужим
/// номером, в том числе номером отряда, которого ещё нет. Регистр не спасает.
#[test]
fn a_squad_name_is_never_shared() {
    let (mut sim, first, second) = two_squads();
    assert_eq!(sim.rename_squad(first.0, first.1, "Альфа"), "");
    assert_eq!(sim.rename_squad(second.0, second.1, "альфа"), "taken");
    assert_eq!(sim.rename_squad(second.0, second.1, "Отряд 1"), "numbered");
    assert_eq!(sim.rename_squad(first.0, first.1, "Отряд 2"), "numbered");
    assert_eq!(sim.rename_squad(first.0, first.1, "Отряд 7"), "numbered");
    assert_eq!(
        sim.squad_labels(),
        ["Альфа", "Отряд 2"],
        "отказ имени не меняет"
    );
}

/// Длинное имя отклоняется целиком, а не режется: обрезанное имя — это имя,
/// которого игрок не давал. Пробелы внутри сжимаются, и длина меряется после.
#[test]
fn a_long_squad_name_is_refused() {
    let (mut sim, first, _) = two_squads();
    let long = "Ж".repeat(SQUAD_NAME_MAX + 1);
    assert_eq!(sim.rename_squad(first.0, first.1, &long), "long");
    let fits = "Ж".repeat(SQUAD_NAME_MAX);
    assert_eq!(sim.rename_squad(first.0, first.1, &fits), "");
    assert_eq!(sim.rename_squad(first.0, first.1, "Ночная     смена"), "");
    assert_eq!(sim.squad_labels()[0], "Ночная смена");
    assert_eq!(sim.rename_squad(9, 9, "Альфа"), "nonode", "не отряд");
}

/// Снесённый стол уносит имя вместе с отрядом: новый стол на том же месте —
/// новый отряд, и чужого имени он не наследует.
#[test]
fn a_rebuilt_table_does_not_inherit_the_name() {
    let (mut sim, _, second) = two_squads();
    assert_eq!(sim.rename_squad(second.0, second.1, "Мусорщики"), "");
    // Столешница посередине штампа 3×3 якорем в (4, 2).
    sim.force_tile(5, 3, 0);
    sim.tick_n(1);
    assert_eq!(sim.node_cells_of().len(), 1, "стол снесён");

    let def = sim.structure_index("muster").expect("место брифинга");
    sim.force_structure(def, 4, 2);
    assert_eq!(sim.squad_labels(), ["Отряд 1", "Отряд 2"]);
}
