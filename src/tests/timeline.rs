//! Таймлайн и записка (§12.28).
//!
//! Первая система, которая действует сама: у события нет инициатора, кроме
//! наступившего тика. Проверяем ровно это — что дата срабатывает один раз, что
//! у неё два исхода (успели / не успели) и что записка показывает не больше,
//! чем игроку положено знать.
//!
//! Мир тот же, что у вылазок: коридор с клеткой-шлюзом (тайл 1) — материальное
//! приходит через него.

use super::*;

/// Коридор со шлюзом в (3,1); бодрость включена, чтобы плату было чем брать.
fn sim_with_gate() -> Sim {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_needs(1000, 100, 1);
    sim
}

#[test]
fn an_event_happens_on_its_tick_and_only_once() {
    let mut sim = sim_with_gate();
    let e = sim.set_event(10, &[], &[(0, 5)], 3, 0);

    sim.tick_n(9);
    assert_eq!(sim.happened(e), None, "срок ещё не настал");

    sim.tick_n(1);
    assert_eq!(sim.happened(e), Some(true), "и настал ровно на своём тике");
    assert_eq!(sim.fame(), 3);

    sim.tick_n(50);
    assert_eq!(sim.fame(), 3, "второй раз событие не повторяется");
}

/// Материальное приходит через шлюз — тем же `spill`, что и добыча с вылазки
/// (§12.22): дальше груз разносит обычная уборка.
#[test]
fn a_gift_lands_on_the_gate() {
    let mut sim = sim_with_gate();
    sim.set_event(5, &[], &[(0, 12), (1, 2)], 0, 0);

    sim.tick_n(5);
    assert_eq!(sim.item_at(3, 1, 0), 12, "лом лёг на шлюз");
    assert_eq!(sim.item_at(3, 1, 1), 2, "и деталь тоже");
}

/// Нет шлюза — миру не через что дотянуться до базы; сама дата при этом
/// проходит, а не зависает.
#[test]
fn without_a_gate_nothing_arrives() {
    let mut sim = sim_with_gate();
    sim.force_tile(3, 1, 0); // шлюз разобрали
    let e = sim.set_event(5, &[], &[(0, 12)], 4, 0);

    sim.tick_n(5);
    assert_eq!(sim.happened(e), Some(true), "дата всё равно прошла");
    assert_eq!(sim.item_total(0), 0, "но груза нет");
    assert_eq!(sim.fame(), 4, "известность приходит и без шлюза");
}

// --- два исхода -------------------------------------------------------------

/// Успели к технологии — событие приносит. Ради этого записка и нужна: время
/// между предупреждением и датой — единственное, что игрок может потратить.
#[test]
fn a_prepared_base_is_rewarded() {
    let mut sim = sim_with_gate();
    let e = sim.set_event(10, &["materials"], &[(0, 8)], 5, 500);
    sim.set_tech("materials");

    sim.tick_n(10);
    assert_eq!(sim.happened(e), Some(true));
    assert_eq!(sim.item_at(3, 1, 0), 8, "груз пришёл");
    assert_eq!(sim.fame(), 5);
    assert!(sim.energy_of("a") > 900, "и никто не надорвался");
}

/// Не успели — расплата бодростью: та же валюта котовремени, в которой
/// измеряется всё остальное (§12.23). Наказание обратимо — коты отоспятся.
#[test]
fn an_unprepared_base_pays_with_energy() {
    let mut sim = sim_with_gate();
    let e = sim.set_event(10, &["materials"], &[(0, 8)], 5, 400);

    sim.tick_n(10);
    assert_eq!(sim.happened(e), Some(false), "база не была готова");
    assert!(sim.energy_of("a") <= 600, "бодрость забрали");
    assert_eq!(sim.item_total(0), 0, "подарка нет");
    assert_eq!(sim.fame(), 0, "и известности тоже");
}

/// Ушедших с базы мир не касается — как не касается их и усталость (§12.22).
#[test]
fn cats_in_the_field_are_not_touched() {
    let mut sim = sim_with_gate();
    let m = sim.set_mission(1, 200, &[]);
    sim.launch(m, vec!["b".to_string()]);
    sim.tick_n(8);
    assert!(sim.is_away("b"), "отряд ушёл");
    let away_energy = sim.energy_of("b");

    sim.set_event(12, &["materials"], &[], 0, 400);
    sim.tick_n(6);
    assert!(sim.energy_of("a") < 900, "оставшийся расплатился");
    assert_eq!(sim.energy_of("b"), away_energy, "а ушедшего не задело");
}

/// Расплата — не смерть: бодрость уходит в ноль, кот валится спать и встаёт.
/// §12.10 держит линию «состояния обратимы».
#[test]
fn the_toll_is_reversible() {
    let mut sim = sim_with_gate();
    sim.set_event(5, &["materials"], &[], 0, 10_000);

    sim.tick_n(6);
    assert!(sim.energy_of("a") <= 1, "надорвался вчистую");
    assert!(sim.is_resting("a"), "и слёг");

    sim.tick_n(1200);
    assert!(!sim.is_resting("a"), "но отоспался и снова в строю");
}

/// Технология, изученная к сроку, меняет исход — это и есть «гонка развития
/// против часов» (§6). Тот же мир, та же дата, разница в одном.
#[test]
fn the_same_date_has_two_outcomes() {
    let mut early = sim_with_gate();
    let e = early.set_event(10, &["materials"], &[(0, 8)], 5, 400);
    early.tick_n(5);
    early.set_tech("materials"); // успели в последний момент
    early.tick_n(5);
    assert_eq!(early.happened(e), Some(true));

    let mut late = sim_with_gate();
    let e = late.set_event(10, &["materials"], &[(0, 8)], 5, 400);
    late.tick_n(10);
    late.set_tech("materials"); // изучили после срока — поздно
    assert_eq!(late.happened(e), Some(false));
}

// --- туман предзнания -------------------------------------------------------

/// Детали проступают только к сроку (§4.6). Считает это ядро: спрятанный в JS
/// текст — не «повреждённые данные», а непоказанный текст.
#[test]
fn details_surface_only_near_the_date() {
    let mut sim = sim_with_gate();
    let e = sim.set_event(100, &["materials"], &[], 0, 100);
    sim.set_reveal(e, 20);

    assert!(!sim.note_revealed(e), "срок далеко — виден только скелет");
    assert!(sim.note_requires(e).is_empty(), "и требований не видно");

    sim.tick_n(80);
    assert!(sim.note_revealed(e), "подошли к сроку — детали проступили");
    assert_eq!(sim.note_requires(e), vec!["materials".to_string()]);
}

/// Готовность — тоже деталь: пока она не проступила, её не показывают, даже
/// если технология уже есть.
#[test]
fn readiness_is_a_detail_too() {
    let mut sim = sim_with_gate();
    let e = sim.set_event(100, &["materials"], &[], 0, 100);
    sim.set_reveal(e, 20);
    sim.set_tech("materials");

    assert!(!sim.note_ready(e), "рано");
    sim.tick_n(80);
    assert!(sim.note_ready(e), "а теперь видно, что база успевает");
}

/// Боевой рулсет: записка на месте, первое событие приходит подарком, а второе
/// действительно требует науки. Ловит контент, где дата раньше, чем база вообще
/// способна что-то сделать, или требование указывает на несуществующую тему.
#[test]
fn the_shipped_ruleset_has_a_readable_note() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    // Первое событие безусловно и приносит: таймлайн должен сперва показать
    // себя подарком, а уже потом требовать.
    assert!(sim.note_requires(0).is_empty(), "караван ничего не требует");
    let before = sim.scrap_total();
    sim.tick_n(2000);
    assert_eq!(sim.happened(0), Some(true), "караван пришёл");
    assert!(sim.scrap_total() > before, "и не с пустыми лапами");

    // Второе требует технологии; ждём, пока детали проступят, — до этого
    // записка молчит о том, чем оно грозит.
    assert!(sim.note_requires(1).is_empty(), "срок ещё далеко");
    sim.tick_n(1800);
    assert_eq!(
        sim.note_requires(1),
        vec!["materials".to_string()],
        "детали второго события уже проступили",
    );
    assert!(!sim.knows_tech("materials"), "и база к нему пока не готова");
}

/// Сутки на боевом рулсете — это цикл кота, а не круглое число с потолка
/// (§12.4, §12.46).
///
/// День нигде не участвует в механике: ядро о календаре не знает, число ходит
/// только в подпись. Ровно поэтому его легко разойтись с содержимым — и ловится
/// это здесь. Проверяем три вещи: календарь вообще есть; кот спит **раз в
/// сутки**, а не дважды и не через день; даты записки идут по возрастанию.
#[test]
fn the_shipped_ruleset_day_is_the_cats_cycle() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let day = sim.day;
    assert!(day > 0, "суток нет — вид покажет сырой тик вместо дня");

    // Полный цикл: от полного заряда до порога усталости и обратно на лучшей
    // лежанке, какая в палитре есть.
    let needs = sim.world.resource::<NeedRules>();
    let best_rest = sim
        .world
        .resource::<TileRules>()
        .0
        .iter()
        .map(|t| t.rest)
        .max()
        .unwrap_or(0);
    assert!(best_rest > 0, "лежанок в палитре нет — коту негде спать");

    // Бодрствование считается **в тиках, а не в очках** (§12.70): расход теперь
    // задан рулсетом и режется «Выносливостью», поэтому `max` сам по себе суток
    // больше не значит. До §12.70 расход был единицей, и очки совпадали с
    // тиками — совпадение, а не правило: сутки это `max / drain`.
    let drain = if needs.drain > 0 { needs.drain } else { 1 };
    let awake = (needs.max - needs.tired) / drain;
    let asleep = (needs.max - needs.tired) / best_rest;
    let cycle = u64::try_from(awake + asleep).expect("цикл не отрицателен");

    assert!(
        cycle <= day,
        "цикл кота ({cycle}) длиннее суток ({day}): кот спит реже, чем раз в день, \
         и «день» перестаёт быть его ритмом",
    );
    assert!(
        cycle * 2 > day,
        "цикл кота ({cycle}) меньше половины суток ({day}): кот успевает выспаться \
         дважды за день, и сутки взяты с потолка",
    );

    // Две даты в один тик игрок прочтёт как одну, а убывающие — как ошибку
    // записки: она читается сверху вниз.
    let dates: Vec<u64> = sim
        .world
        .resource::<TimelineRules>()
        .0
        .iter()
        .map(|e| e.at)
        .collect();
    assert!(dates.len() >= 2, "записки нет — забег ничем не размечен");
    assert!(
        dates.windows(2).all(|w| w[0] < w[1]),
        "даты записки не возрастают строго: {dates:?}",
    );
}
