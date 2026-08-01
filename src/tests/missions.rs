//! Вылазки: сбор отряда, уход через шлюз, исход и возвращение (§12.22, §12.23).
//!
//! Схема почти везде одна: коридор с клеткой-шлюзом (тайл 1) и коты по разные
//! его стороны. Проверяем не отдельные функции, а прогон полной цепочки — баги
//! здесь живут в фильтрах занятости и в порядке систем.
//!
//! Миссия из `set_mission` **безопасна и бесплатна**: тесты сбора отряда про
//! исход ничего не знают, а тесты исхода зовут `set_risky_mission`.

use super::*;

/// Мир с одной клеткой-шлюзом: тайл 1 в позиции `gate`, всё остальное — пол.
/// Возвращает готовую симуляцию и индекс заведённой миссии.
fn sim_with_gate(rows: &[&str], gate: (i32, i32), squad: usize, ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    let mission = sim.set_mission(squad, ticks, &[(0, 5)]);
    (sim, mission)
}

/// Отряд поимённо — в тестах он почти всегда «все, кто есть».
fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Тайл, которым в этих тестах занимают котов стройкой.
///
/// Не `0`: пол схемы `sim_from` — это уже тайл `0`, и чертёж на нём мгновенно
/// отсекается как «уже построено». Тест с таким чертежом зелёный и пустой.
const OTHER: i32 = 2;

// --- сбор и уход -----------------------------------------------------------

#[test]
fn a_squad_gathers_at_the_gate_and_leaves() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    assert!(sim.launch(m, squad(&["a", "b"])));

    assert!(
        sim.in_squad("a") && sim.in_squad("b"),
        "оба записаны в отряд"
    );
    assert!(!sim.is_away("a"), "но ещё на базе — идут к шлюзу");

    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (3, 1), "кот пришёл на шлюз");
    assert!(sim.is_away("a") && sim.is_away("b"), "отряд ушёл");
    assert_eq!(sim.mission_gate(), Some((3, 1)));
}

#[test]
fn the_squad_returns_with_loot_at_the_gate() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(11);
    assert!(sim.is_away("a"), "отряд в поле");
    assert_eq!(sim.scrap_total(), 0, "пока отряд в поле, добычи ещё нет");

    sim.tick_n(10);
    assert!(!sim.is_away("a") && !sim.is_away("b"), "отряд вернулся");
    assert!(!sim.in_squad("a"), "и снова свободен");
    assert_eq!(sim.pos_of("a"), (3, 1), "вернулись на тот же шлюз");
    assert_eq!(sim.item_at(3, 1, 0), 5, "добыча лежит кучей на шлюзе");
    assert_eq!(sim.mission_left(), None, "миссия закрыта");
}

/// Отсчёт идёт с ухода отряда, а не с приказа: пока последний кот в пути,
/// таймер стоит. Иначе миссия «шла» бы, пока бригада бегает по базе.
#[test]
fn the_timer_waits_for_the_last_cat() {
    let rows = &[
        "##########",
        "#a.......#",
        "#........#",
        "#b.......#",
        "##########",
    ];
    let (mut sim, m) = sim_with_gate(rows, (8, 1), 2, 20);
    sim.launch(m, squad(&["a", "b"]));

    sim.tick_n(8);
    assert_eq!(sim.mission_left(), Some(20), "таймер ещё не тронулся");
    assert!(!sim.is_away("a"), "первый пришёл, но ждёт второго");

    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"), "ушли вместе");
    assert!(sim.mission_left().is_some_and(|l| l < 20), "таймер пошёл");
}

/// Занятость отрядом — это фильтры: пропущенный `Without<Squad>` тихо уводит
/// бойца на стройку, и отряд не соберётся никогда (инвариант занятости).
#[test]
fn a_cat_in_a_squad_is_not_taken_by_jobs() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 50);
    sim.launch(m, squad(&["a", "b"]));
    sim.add_blueprint(1, 1, OTHER);
    sim.add_blueprint(5, 1, OTHER);
    sim.tick_n(20);

    assert!(
        sim.is_away("a") && sim.is_away("b"),
        "чертежи отряд не сорвали"
    );
    assert!(!sim.has_assignment("a") && !sim.has_assignment("b"));
    assert_ne!(sim.tile(1, 1), OTHER as i16, "и строить их было некому");
}

/// Добыча ложится на шлюз обычной кучей и дальше живёт по общим правилам:
/// её размечает автоуборка и увозит на склад свободный кот (§12.16).
#[test]
fn loot_reaches_storage_by_itself() {
    let (mut sim, m) = sim_with_gate(&["########", "#a..b..#", "########"], (3, 1), 2, 6);
    sim.set_capacity(2, 50);
    sim.force_tile(6, 1, 2); // склад в дальнем конце коридора
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(40);

    assert_eq!(sim.scrap_total(), 5, "добыча не потерялась");
    assert!(sim.scrap_is_in_storage(), "и уехала на склад сама");
}

/// Шлюз снесли, пока отряд был в поле. Кот выбирается из ямы общим механизмом
/// (`escape_voids`), добыча съезжает на соседний пол (`settle_stacks`): правило
/// «ничего не остаётся в пустоте» на миссии не делает исключения (§12.15).
#[test]
fn a_squad_returns_into_a_demolished_gate() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    sim.demolish(3, 1); // шлюза больше нет
    sim.tick_n(12);

    assert!(!sim.is_away("a"), "отряд вернулся");
    assert!(sim.tile(3, 1) < 0, "вернулся в яму");
    assert_ne!(sim.pos_of("a"), (3, 1), "и вышел из неё сам");
    assert_eq!(sim.scrap_total(), 5, "добыча цела");
    assert!(sim.scrap_is_on_floor(), "и лежит на полу, а не в яме");
}

// --- заявка: кого посылают --------------------------------------------------

/// Отряд выбирает игрок, а не симуляция: идут названные коты, даже если рядом
/// со шлюзом стоял кто-то другой (§12.23).
#[test]
fn the_player_picks_the_squad() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    assert!(sim.launch(m, squad(&["a", "b"])), "заявка принята");

    assert!(sim.in_squad("a") && sim.in_squad("b"));
    assert!(!sim.in_squad("c"), "ближайший к шлюзу остался на базе");
    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"));
    assert!(!sim.is_away("c"));
}

/// Недобор — это неполная заявка, а не «пойдут вдвоём вместо троих»: молча
/// дополнять состав симуляция не станет.
#[test]
fn an_incomplete_squad_is_refused() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    assert!(!sim.launch(m, squad(&["a"])), "меньше нужного");
    assert!(!sim.launch(m, squad(&["a", "b", "c"])), "больше нужного");
    assert!(
        !sim.launch(m, squad(&["a", "a"])),
        "один кот дважды — не отряд"
    );
    assert!(!sim.launch(m, squad(&["a", "ghost"])), "кота нет в мире");
    assert_eq!(sim.mission_left(), None, "ни одна заявка не прошла");
}

/// Заявка снимает начатую работу — как приказ игрока (§12.15): решение послать
/// кота в поле весомее его текущей стройки, и чертёж при этом освобождается.
#[test]
fn a_raid_takes_a_cat_off_its_job() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 20);
    sim.add_blueprint(1, 1, OTHER);
    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "кот взялся за чертёж");

    assert!(sim.launch(m, squad(&["a", "b"])));
    assert!(!sim.has_assignment("a"), "стройка снята");
    sim.tick_n(20);
    assert!(sim.is_away("a"), "ушёл на вылазку");

    // Чертёж освобождён, а не остался за ушедшим: вернувшись, его добьют.
    sim.tick_n(BUILD_TICKS * 3 + 20);
    assert_eq!(sim.tile(1, 1), OTHER as i16, "чертёж всё-таки построен");
}

/// Заявка роняет ношу под ноги — сразу, не дожидаясь ухода (§12.38).
///
/// До этого лом ехал с котом в поле и возвращался с ним же: сумма сходилась, но
/// на сотни тиков вещь пропадала из мира — её не видно кучей, не взять на
/// стройку и не разметить. Тот же случай, что куча в пустоте (§12.15).
///
/// Проверяем **без единого тика**: заявка — это фасад, и лом должен вернуться
/// базе в тот же миг, а не после сбора отряда, который может длиться сколько
/// угодно (спящего бойца `gather_squad` ждёт молча).
#[test]
fn a_summoned_cat_drops_its_load_where_it_stood() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 20);
    sim.set_cost(OTHER as i16, 2);
    sim.put_scrap(1, 1, 2);
    sim.add_blueprint(5, 1, OTHER);
    sim.tick_n(3);
    assert!(sim.carrying_of("a") > 0, "кот поднял лом на площадку");
    let total = sim.scrap_total();
    let at = sim.pos_of("a");

    assert!(
        sim.launch(m, squad(&["a", "b"])),
        "и тут его послали в поле"
    );
    assert_eq!(sim.carrying_of("a"), 0, "лапы пусты сразу же");
    assert_eq!(sim.scrap_at(at.0, at.1), 2, "лом лёг там, где кот стоял");
    assert_eq!(sim.scrap_total(), total, "и ничего не пропало");

    sim.tick_n(20);
    assert!(sim.is_away("a"), "отряд ушёл налегке");
    assert_eq!(sim.scrap_total(), total, "лом остался на базе");
}

/// Брошенная ноша — обычная куча, и отдельной цены у решения нет (§12.38): её
/// размечает автоуборка, а увозит **любой свободный кот**, а не тот, кто нёс.
#[test]
fn the_dropped_load_is_tidied_by_anyone() {
    // Носильщиком заведомо станет `a`: лом лежит прямо под ним, а площадка — в
    // дальнем конце, так что через шлюз он пойдёт уже гружёным (§12.14).
    let rows = &["########", "#a...b.#", "#....c.#", "########"];
    // Вылазка нарочно длинная: пока отряд в поле, на базе виден только тот, кто
    // остался, — и то, что он сделал с брошенной кучей.
    let (mut sim, m) = sim_with_gate(rows, (3, 1), 2, 200);
    sim.set_cost(OTHER as i16, 2);
    sim.set_capacity(3, 50);
    sim.force_tile(5, 2, 3); // склад в нижнем ряду, у оставшегося кота
    sim.put_scrap(1, 1, 2);
    sim.add_blueprint(6, 1, OTHER); // площадка в дальнем конце — кот идёт с грузом
    sim.tick_n(3);
    assert!(sim.carrying_of("a") > 0, "кот поднял лом на площадку");
    let total = sim.scrap_total();

    assert!(sim.launch(m, squad(&["a", "b"])), "их послали в поле");
    // Площадку убираем: иначе кучу подхватит подвоз и довезёт её туда же —
    // тоже правильный исход, но тест перестал бы говорить про уборку.
    assert!(sim.plan_demolish(6, 1), "чертёж снят");

    sim.tick_n(40);
    assert!(
        sim.is_away("a") && sim.is_away("b"),
        "отряд ушёл: a={:?} b={:?} squad_a={} gate={:?} left={:?}",
        sim.pos_of("a"),
        sim.pos_of("b"),
        sim.in_squad("a"),
        sim.mission_gate(),
        sim.mission_left()
    );
    assert_eq!(sim.scrap_total(), total, "лом цел");
    assert!(
        sim.scrap_is_in_storage(),
        "и оставшийся на базе кот свёз его на склад сам"
    );
}

/// А приказ игрока ношу **не** роняет: кот сходит куда велено и донесёт груз
/// сам (§12.15). Это и есть граница исключения — роняет только вылазка, потому
/// что она убирает кота с базы надолго.
#[test]
fn an_order_does_not_drop_the_load() {
    let mut sim = sim_from(&["#######", "#a....#", "#######"]);
    sim.set_cost(OTHER as i16, 2);
    sim.put_scrap(1, 1, 2);
    sim.add_blueprint(5, 1, OTHER);
    sim.tick_n(3);
    assert!(sim.carrying_of("a") > 0, "кот поднял лом");

    assert!(sim.set_target("a", 3, 1), "приказ отдан");
    assert!(sim.carrying_of("a") > 0, "груз при нём");
    assert_eq!(sim.scrap_at(1, 1), 0, "и на пол ничего не упало");

    // Дальше кот несёт лом до площадки: груз с приказом не теряется, а первым
    // же освободившимся тиком его подберёт та же доставка (§12.15).
    sim.tick_n(6);
    assert_eq!(sim.carrying_of("a"), 0, "донёс");
    assert_eq!(sim.delivered_at(5, 1), Some(2), "именно на площадку");
    sim.tick_n(BUILD_TICKS + 2);
    assert_eq!(sim.tile(5, 1), OTHER as i16, "и построил из него");
}

/// Приказ игрока распускает вылазку целиком: состав выбран поимённо, заменить
/// выбывшего некем, а отряд, который никогда не соберётся, хуже честного
/// роспуска (§12.23).
#[test]
fn an_order_disbands_the_raid() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 50);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(1);

    assert!(sim.set_target("a", 1, 1));
    assert_eq!(sim.mission_left(), None, "вылазка снята");
    assert!(!sim.in_squad("a") && !sim.in_squad("b"), "оба свободны");

    sim.tick_n(30);
    assert!(!sim.is_away("b"), "никто никуда не ушёл");
}

#[test]
fn cancelling_frees_the_squad() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 50);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(1);
    assert!(sim.in_squad("a"));

    assert!(sim.cancel_mission());
    assert!(!sim.in_squad("a") && !sim.in_squad("b"), "отряд распущен");
    assert_eq!(sim.mission_left(), None);

    sim.add_blueprint(1, 1, OTHER);
    sim.tick_n(BUILD_TICKS + 5);
    assert_eq!(sim.tile(1, 1), OTHER as i16, "и коты вернулись к работе");
}

/// Ушедший отряд не отзывается: что с ним происходит, симуляция не знает —
/// вылазка считается разом по возвращении.
#[test]
fn an_away_squad_cannot_be_recalled() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 30);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    assert!(!sim.cancel_mission(), "отозвать нельзя");
    assert!(!sim.set_target("a", 1, 1), "и приказать тоже");
    assert!(sim.is_away("a"), "кот всё ещё в поле");
}

/// Истощение — не решение игрока, поэтому из отряда оно не выводит: боец
/// досыпает своё, а вылазка ждёт. Этим оно и отличается от приказа (§12.23).
#[test]
fn exhaustion_makes_the_raid_wait() {
    let rows = &[
        "##########",
        "#a.......#",
        "#........#",
        "#b.......#",
        "##########",
    ];
    let (mut sim, m) = sim_with_gate(rows, (8, 1), 2, 20);
    sim.set_needs(100, 10, 50);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(1);

    sim.set_energy("b", 0);
    sim.tick_n(1);
    assert!(sim.is_resting("b"), "упал где стоял");
    assert!(sim.in_squad("b"), "но из отряда не выпал");
    assert!(!sim.is_away("a"), "и вылазка ждёт его");

    // Просыпается за пару тиков и идёт дальше к шлюзу — девять шагов.
    sim.tick_n(20);
    assert!(
        sim.is_away("a") && sim.is_away("b"),
        "выспался — и отряд ушёл"
    );
}

/// Вне базы кот не устаёт: считать усталость там нечем, вылазка — авторасчёт,
/// а не симуляция (§12.22). Плата берётся разом, на возвращении.
#[test]
fn a_cat_on_a_mission_does_not_tire() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 40);
    sim.set_needs(3000, 100, 1);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(11);
    assert!(sim.is_away("a"));

    let before = sim.energy_of("a");
    sim.tick_n(20);
    assert!(sim.is_away("a"), "ещё в поле");
    assert_eq!(sim.energy_of("a"), before, "бодрость не тронулась");
}

/// Шлюза на базе нет — заявку не принимаем вовсе: отряд, которому некуда идти,
/// просто стоял бы столбом.
#[test]
fn without_a_gate_nobody_leaves() {
    let mut sim = sim_from(&["#######", "#a...b#", "#######"]);
    let m = sim.set_mission(2, 10, &[(0, 5)]);
    assert!(!sim.launch(m, squad(&["a", "b"])));
    assert_eq!(sim.mission_left(), None);
}

/// Миссия одна за раз: вторая заявка не принимается, пока первая не закрыта.
#[test]
fn only_one_mission_runs_at_a_time() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    assert!(sim.launch(m, squad(&["a", "b"])));
    assert!(
        !sim.launch(m, squad(&["a", "b"])),
        "вторая заявка отклонена"
    );
    sim.tick_n(21);
    assert!(
        sim.launch(m, squad(&["a", "b"])),
        "после возвращения — снова можно"
    );
}

// --- исход -----------------------------------------------------------------

/// Сила отряда покрыла сложность — вся добыча. По коту это единица плюс его
/// уровень «Вылазки», поэтому даже новички что-то да могут (§12.23).
#[test]
fn a_squad_that_matches_the_danger_brings_everything() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let m = sim.set_risky_mission(2, 10, 2, 0, &[(0, 20)]);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);

    assert_eq!(sim.scrap_total(), 20, "сила 2 против сложности 2 — всё");
}

/// Силы не хватило, но и не вдвое: добыча приходит долей. Округление вниз —
/// «донесли, сколько смогли», а не «получите половинку детали».
#[test]
fn a_weak_squad_brings_only_a_share() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let m = sim.set_risky_mission(2, 10, 3, 0, &[(0, 20), (1, 3)]);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);

    // 2 из 3 = 66%: лома 20*66/100 = 13, деталей 3*66/100 = 1.
    assert_eq!(sim.item_total(0), 13, "лома — две трети");
    assert_eq!(sim.item_total(1), 1, "деталей — сколько донесли");
}

/// Силы меньше половины нужного — провал: ни добычи, ни сил. Коты валятся у
/// шлюза, и цена вылазки становится видимой сразу.
#[test]
fn a_hopeless_squad_fails_and_collapses() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.set_needs(1000, 100, 1);
    let m = sim.set_risky_mission(2, 10, 10, 0, &[(0, 20)]);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);

    assert_eq!(sim.scrap_total(), 0, "вернулись ни с чем");
    // Ноль бодрости держится ровно тик: `collapse_exhausted` тут же роняет
    // кота, и `sleep` начинает его поднимать. Видимый след провала — не число,
    // а то, что оба спят у шлюза, ничего не проработав.
    assert!(
        sim.is_resting("a") && sim.is_resting("b"),
        "падают там, где стояли"
    );
    assert!(sim.energy_of("a") < 100, "и подниматься им долго");
}

/// Успешная вылазка тоже стоит бодрости — просто не всей. Плата списывается
/// разом на возвращении: сама вылазка не симулируется (§12.22).
#[test]
fn a_successful_raid_still_costs_energy() {
    // Сравниваем два одинаковых мира, различающихся только платой: обычное
    // бодрствование тратит бодрость в обоих, и вычитать его из ответа значит
    // считать то же самое дважды.
    let after_toll = |toll: i32| {
        let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
        sim.set_needs(1000, 100, 1);
        let m = sim.set_risky_mission(2, 10, 2, toll, &[(0, 5)]);
        sim.launch(m, squad(&["a", "b"]));
        sim.tick_n(25);
        assert!(!sim.is_away("a"), "вернулись");
        sim.energy_of("a")
    };

    assert_eq!(
        after_toll(0) - after_toll(300),
        300,
        "плата за вылазку снята разом и ровно один раз",
    );
}

/// Навык «Вылазка» растёт от самих вылазок — по очку за тик в поле, тем же
/// механизмом `Worked`, что и «Стройка» (§12.17).
#[test]
fn raiding_trains_the_raid_skill() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let raid = sim.set_skill("raid", &[5, 50]);
    let m = sim.set_risky_mission(2, 10, 2, 0, &[(0, 5)]);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);

    assert_eq!(sim.xp_of("a", raid), 10, "очко за каждый тик в поле");
    assert_eq!(sim.level_of("a", raid), 1, "и первый уровень взят");
}

/// Тот же отряд с навыком приносит больше: ради этого выбор состава и нужен.
#[test]
fn a_trained_squad_brings_more_loot() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let raid = sim.set_skill("raid", &[100]);
    let m = sim.set_risky_mission(2, 10, 4, 0, &[(0, 20)]);

    // Новички: сила 2 из 4 — половина добычи.
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);
    assert_eq!(sim.scrap_total(), 10, "половина");

    // Те же коты первого уровня: сила 4 из 4 — всё.
    sim.set_xp("a", raid, 100);
    sim.set_xp("b", raid, 100);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);
    assert_eq!(sim.scrap_total(), 30, "и ещё двадцать сверху");
}

/// Провалу тоже учатся: опыт капает за время в поле, а не за успех (§12.17).
#[test]
fn even_a_failed_raid_teaches() {
    let (mut sim, _) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let raid = sim.set_skill("raid", &[100]);
    let m = sim.set_risky_mission(2, 10, 10, 0, &[(0, 20)]);
    sim.launch(m, squad(&["a", "b"]));
    sim.tick_n(25);

    assert_eq!(sim.scrap_total(), 0, "вылазка провалена");
    assert_eq!(sim.xp_of("a", raid), 10, "но опыт всё равно набран");
}

/// Боевой рулсет: гараж — шлюз, «Свалка» по силам стартовой тройке, а «Логово»
/// на старте гарантированно проваливается. Ловит рассогласование кода и
/// контента — гараж без `gate`, навык под другим `id`, лестницу сложности,
/// в которой первая же вылазка невыполнима.
#[test]
fn the_shipped_ruleset_sends_a_squad_out_and_back() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let parts = 1; // индекс `part` в палитре предметов
    let before = sim.item_total(parts);

    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "первая миссия рулсета по силам стартовому отряду",
    );
    sim.tick_n(600);

    assert!(
        sim.item_total(parts) > before,
        "деталей стало больше: вылазка — источник дохода, которого у базы не было",
    );
    assert!(
        sim.skill_index("raid")
            .is_some_and(|s| sim.xp_of("excellent", s) > 0),
        "и навык «Вылазка» вырос: домен работы, а не украшение",
    );
    assert_eq!(sim.mission_left(), None, "миссия закрыта");
}

#[test]
fn the_shipped_ruleset_has_a_mission_that_is_out_of_reach() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let before = sim.scrap_total();

    // Известность выдаём напрямую: здесь проверяется потолок сложности, а не
    // лестница ворот — её ведёт `the_shipped_ruleset_has_a_reachable_ladder`.
    sim.set_fame(60);
    assert!(
        sim.launch(2, squad(&["excellent", "sp2"])),
        "«Логово» заявлено"
    );
    // Вылазка идёт 360 тиков; 500 — возвращение плюс запас на сбор у шлюза, но
    // ещё не полный сон: упавший отряд доберётся до лежанок и выспится (§12.33),
    // и на длинном прогоне следа от провала не осталось бы.
    sim.tick_n(500);

    assert_eq!(
        sim.scrap_total(),
        before,
        "новички возвращаются из логова ни с чем — механике провала есть что показать",
    );
    // Возвращается при этом не весь отряд: одного провал оставляет в плену
    // (§12.40), и на боевом рулсете это «Excellent» — при равном здоровье
    // выбирают первого по `id`. Вернувшийся валится у шлюза без сил.
    assert!(sim.is_captive("excellent"), "одного оставили там");
    assert!(sim.is_resting("sp2"), "а второй свалился без сил");
}
