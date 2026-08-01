//! Плен и вылазка за своим (§12.40).
//!
//! Провал вылазки до сих пор стоил бодрости, снаряжения и здоровья — всего
//! того, что восстанавливается само. Плен — первая потеря, которая **сама не
//! проходит**: кота нет на базе, пока за ним не сходят.
//!
//! Проверяем прогоном полной цепочки: плен живёт на стыке `run_missions`,
//! фильтров занятости и фасада (`launch`), и баги здесь — это «пленного увела
//! работа» и «за пленным некому прийти», а не арифметика.
//!
//! В схеме `sim_from` вылазок нет вовсе, поэтому и плена в чужих тестах не
//! случается: миссии заводит сам тест.

use super::*;

/// Мир с одной клеткой-шлюзом (тайл 1) и котами по коридору.
fn gate_world(rows: &[&str], gate: (i32, i32)) -> Sim {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    sim
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Трое котов, шлюз посередине и заведомо провальная вылазка на двоих.
/// Возвращает мир, индекс провальной вылазки и индекс вылазки за своим.
fn world_with_a_doomed_raid() -> (Sim, usize, usize) {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    let rescue = sim.set_rescue_mission(1, 5, 0);
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);
    (sim, doomed, rescue)
}

// --- кого оставляют ---------------------------------------------------------

/// Провал оставляет **одного** кота: остальные приходят домой. Цена провала
/// перестаёт быть только бодростью, которая набежит сама.
#[test]
fn a_failed_raid_leaves_a_cat_behind() {
    let (mut sim, doomed, _) = world_with_a_doomed_raid();
    assert!(sim.launch(doomed, squad(&["a", "b"])));
    sim.tick_n(30);

    assert!(sim.is_captive("a"), "первый по id остался там");
    assert!(!sim.is_captive("b") && !sim.is_away("b"), "второй дома");
    assert!(sim.is_away("a"), "пленный — тот же `Away`: его нет на базе");
    assert!(!sim.in_squad("a"), "но отряда за ним больше нет");
}

/// Оставляют самого битого: он и шёл тяжелее всех. Провал бьёт всех одинаково,
/// поэтому порядок здоровья до вычета урона тот же, что и после.
#[test]
fn the_worst_hurt_cat_is_the_one_left_behind() {
    let (mut sim, doomed, _) = world_with_a_doomed_raid();
    sim.set_health_rules(100, 20, 1);
    sim.set_health("a", 90);
    sim.set_health("b", 50); // «b» позже по id, но битее

    assert!(sim.launch(doomed, squad(&["a", "b"])));
    sim.tick_n(30);

    assert!(sim.is_captive("b"), "оставили того, кому досталось больше");
    assert!(!sim.is_captive("a"), "а целого унесли");
}

/// Идти за пленным должно быть кому. Некем набрать самый маленький
/// спасательный отряд — плена не случается вовсе: отряд тащит раненого сам.
/// Иначе провал на трёх котах запирал бы игру насмерть (§12.10).
#[test]
fn no_one_is_left_behind_when_no_one_could_come_for_him() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    sim.set_rescue_mission(3, 5, 0); // спасать пришлось бы втроём
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);

    assert!(sim.launch(doomed, squad(&["a", "b"])));
    sim.tick_n(30);

    assert!(!sim.is_captive("a") && !sim.is_captive("b"), "оба дома");
    assert!(!sim.is_away("a") && !sim.is_away("b"), "и оба на базе");
}

/// Вылазки за своим в мире нет вовсе — значит и плена нет: оставлять кота там,
/// откуда его нечем вернуть, — это смерть, которой в игре нет (§12.37).
#[test]
fn without_a_rescue_raid_captivity_does_not_happen() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);

    assert!(sim.launch(doomed, squad(&["a", "b"])));
    sim.tick_n(30);
    assert!(
        !sim.is_captive("a") && !sim.is_captive("b"),
        "оба вернулись"
    );
}

// --- что с пленным происходит (ничего) --------------------------------------

/// Пленный не работает: отряда за ним больше нет, и фильтр по `Squad` его бы не
/// поймал — раздатчики смотрят на «нет на базе», а не на «в отряде».
#[test]
fn a_captive_takes_no_work() {
    let (mut sim, doomed, _) = world_with_a_doomed_raid();
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    let was = sim.pos_of("a");
    sim.add_blueprint(1, 1, 2); // работа, за которую взялся бы любой свободный
    sim.tick_n(20);

    assert!(!sim.has_assignment("a"), "работа пленного не касается");
    assert!(!sim.has_path("a"), "и никуда он не идёт");
    assert_eq!(sim.pos_of("a"), was, "он вообще не в мире базы");
}

/// Приказы пленному не отдаются — как и любому, кого нет на базе (§12.22).
#[test]
fn a_captive_takes_no_orders() {
    let (mut sim, doomed, _) = world_with_a_doomed_raid();
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);

    assert!(sim.is_captive("a"), "кот в плену");
    assert!(!sim.set_target("a", 1, 1), "приказывать некому");
}

/// Пленного в спасательный отряд не берут: иначе неудачное спасение плодило бы
/// второго пленника, а база уходила бы в спираль (§12.24, §12.40).
#[test]
fn a_captive_cannot_join_the_rescue_squad() {
    let (mut sim, doomed, rescue) = world_with_a_doomed_raid();
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    assert!(!sim.launch(rescue, squad(&["a"])), "сам себя не спасает");
    assert!(sim.launch(rescue, squad(&["c"])), "а свободный кот — идёт");
}

// --- вылазка за своим -------------------------------------------------------

/// Спасать некого — заявку не принимают: у такой вылазки нет ни добычи, ни цели.
#[test]
fn a_rescue_needs_someone_to_rescue() {
    let (mut sim, _, rescue) = world_with_a_doomed_raid();
    assert!(!sim.launch(rescue, squad(&["a"])), "все дома");
}

/// Успех возвращает кота домой — и в мир базы: он снова ходит и работает.
#[test]
fn a_successful_rescue_brings_him_home() {
    let (mut sim, doomed, rescue) = world_with_a_doomed_raid();
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    assert!(sim.launch(rescue, squad(&["c"])));
    sim.tick_n(30);

    assert!(!sim.is_captive("a"), "вернулся");
    assert!(!sim.is_away("a"), "и он на базе");
    assert!(sim.set_target("a", 1, 1), "приказ снова принят");
    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (1, 1), "и выполнен: кот снова в мире");
}

/// Частичный успех возвращает так же: доля считается в добыче, а кот либо дома,
/// либо нет — половины кота не бывает.
#[test]
fn a_partial_rescue_still_brings_him_home() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    let rescue = sim.set_rescue_mission(1, 5, 2); // сила 1 из 2 — половина
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);

    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    assert!(sim.launch(rescue, squad(&["c"])));
    sim.tick_n(30);
    assert!(!sim.is_captive("a"), "вынесли, пусть и не блестяще");
}

/// Провал спасения нового пленника не плодит: спираль, в которой за каждым
/// ушедшим уходит следующий, — это не сложность, а тупик.
#[test]
fn a_failed_rescue_leaves_no_second_captive() {
    let mut sim = gate_world(&["#########", "#a..b..c#", "#########"], (4, 1));
    let rescue = sim.set_rescue_mission(1, 5, 100); // заведомо провальная
    let doomed = sim.set_risky_mission(2, 5, 100, 0, &[]);

    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    assert!(sim.launch(rescue, squad(&["c"])));
    sim.tick_n(30);
    assert!(
        !sim.is_captive("c"),
        "спасатель вернулся ни с чем, но вернулся"
    );
    assert!(sim.is_captive("a"), "а пленный так и остался там");
}

/// Бросить кота в плену и уйти за добычей игроку никто не запрещает: это его
/// решение и его цена — на базе тем временем на одни лапы меньше.
#[test]
fn the_base_may_leave_him_there_and_go_for_loot() {
    let (mut sim, doomed, _) = world_with_a_doomed_raid();
    sim.launch(doomed, squad(&["a", "b"]));
    sim.tick_n(30);
    assert!(sim.is_captive("a"), "кот в плену");

    let easy = sim.set_mission(2, 5, &[(0, 5)]);
    assert!(sim.launch(easy, squad(&["b", "c"])), "обычная вылазка идёт");
    sim.tick_n(30);
    assert_eq!(sim.scrap_total(), 5, "и добыча получена");
    assert!(sim.is_captive("a"), "пленному от этого не легче");
}

// --- боевой рулсет ----------------------------------------------------------

/// В `core.yaml` за своим есть кому и на что сходить. Ловит рассогласование
/// контента с механикой: вылазка за своим, закрытая известностью или требующая
/// больше котов, чем у базы останется, — это плен без выхода, то есть смерть
/// кота через чёрный ход (§12.37).
#[test]
fn the_shipped_ruleset_can_always_come_for_its_own() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline(); // мир по расписанию — шум для чужой механики (§12.28)
    let units = sim.unit_count();

    let rescues = sim.rescue_missions();
    assert!(!rescues.is_empty(), "вылазка за своим в палитре есть");
    for &(_, squad, danger, requires) in &rescues {
        assert_eq!(requires, 0, "за своим идут, не спрашивая известности");
        assert!(
            squad < units,
            "спасателей нужно меньше, чем котов на базе: {squad} из {units}",
        );
        // Сила новичка — единица (§12.23): вдвое меньше нужного — провал, и
        // тогда первая же спасательная вылазка обернулась бы билетом в один
        // конец, а плен — смертью, которой в игре нет.
        assert!(
            squad as i32 * 2 >= danger,
            "новичок доносит своего: {danger}"
        );
    }

    // И то же самое целиком: заведомо провальное «Логово» оставляет кота, а
    // вылазка за своим приводит его обратно.
    let rescue = rescues[0].0;
    sim.set_fame(60); // логово открыто, но силы стартовой пары на него нет
    assert!(sim.launch(2, squad(&["excellent", "sp2"])), "ушли в логово");
    sim.tick_n(1200);
    assert!(sim.is_captive("excellent"), "провал оставил кота там");

    assert!(sim.launch(rescue, squad(&["sp3"])), "за ним пошёл третий");
    sim.tick_n(1200);
    assert!(!sim.is_captive("excellent"), "и привёл домой");
    assert!(!sim.is_away("excellent"), "кот снова на базе");
}
