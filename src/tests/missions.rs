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
    sim.set_relay(1, true);
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
    assert_eq!(sim.mission_left(), Some(0), "срок ещё не посчитан");
    assert!(!sim.is_away("a"), "первый пришёл, но ждёт второго");

    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"), "ушли вместе");
    assert!(sim.mission_left().is_some_and(|l| l < 20), "таймер пошёл");
}

/// Проводник режет опасность заказа для **всей** бригады (§12.70).
///
/// Реакция не прибавляет силу, а делит опасность, и форма важнее величины:
/// прибавка линейна и полезна везде одинаково, деление — нет. Здесь это видно
/// прямо: та же пара котов, тот же заказ, разница только в реакции одного.
#[test]
fn a_guide_cuts_the_danger_for_the_whole_squad() {
    let loot = |guide: i32| {
        let rows = &["#######", "#ab...#", "#######"];
        let (mut sim, m) = sim_with_gate(rows, (5, 1), 2, 10);
        sim.set_risky_mission(2, 10, 6, 0, &[(0, 100)]);
        let m = m + 1;
        sim.set_stat_steps("reflex", &[5]);
        sim.set_stat("a", 0, guide);
        assert!(sim.launch(m, squad(&["a", "b"])));
        sim.tick_n(40);
        sim.item_at(5, 1, 0)
    };

    // Без проводника: сила 2 против опасности 6 — вдвое меньше нужного, провал.
    assert_eq!(loot(0), 0, "новички не дотянули");
    // С проводником: опасность встречает их как 6*2/3 = 4, и половина добычи их.
    assert_eq!(loot(5), 50, "проводник вытащил обоих");
}

/// Считается **лучший** в отряде, а не сумма (§12.70).
///
/// Сумма дала бы квадратичный рост по размеру отряда — число котов уже входит в
/// силу, и в делителе оно работало бы вторым концом дроби: трое середняков
/// закрывали бы заказ на сотку. Максимум оставляет их множителем на бригаду.
#[test]
fn the_guide_is_the_best_of_the_squad_not_their_sum() {
    let rows = &["########", "#abc...#", "########"];
    let (mut sim, m) = sim_with_gate(rows, (6, 1), 3, 10);
    sim.set_risky_mission(3, 10, 6, 0, &[(0, 100)]);
    let m = m + 1;
    sim.set_stat_steps("reflex", &[5]);
    for cat in ["a", "b", "c"] {
        sim.set_stat(cat, 0, 5);
    }
    assert!(sim.launch(m, squad(&["a", "b", "c"])));
    sim.tick_n(40);

    // Ступень у всех троих одна, значит опасность режется один раз: 6*2/3 = 4.
    // Сила 3 против 4 — три четверти добычи. Сумма ступеней дала бы 6*2/5 = 2,
    // то есть всю добычу, и заказ перестал бы отличаться от лёгкого.
    assert_eq!(sim.item_at(6, 1, 0), 75, "режет лучший, а не все вместе");
}

/// Работа на месте делится на лапы, дорога — нет (§12.70).
///
/// Это та же идиома, что `BUILD_WORK / WORK_RATE` на базе (инвариант 9), только
/// очки набивает не один кот, а отряд. Отсюда и смысл большого состава: сила
/// сверх `danger` пропадает, а лишние лапы уходят в срок.
#[test]
fn more_paws_finish_the_raid_sooner() {
    let span = |crew: &[&str]| {
        let rows = &["########", "#abcd..#", "########"];
        let (mut sim, m) = sim_with_gate(rows, (6, 1), 1, 100);
        sim.set_squad_range(m, 1, 4);
        sim.set_mission_work(m, 120);
        assert!(sim.launch(m, squad(crew)));
        sim.tick_n(20);
        assert!(sim.is_away(crew[0]), "отряд ушёл");
        sim.mission_span().expect("миссия идёт")
    };

    // Дорога 100 у всех одна; работа 120 делится на лапы: 120, 60, 30.
    assert_eq!(span(&["a"]), 100 + 120, "один тянет всю работу сам");
    assert_eq!(span(&["a", "b"]), 100 + 60, "вдвоём вдвое быстрее");
    assert_eq!(
        span(&["a", "b", "c", "d"]),
        100 + 30,
        "вчетвером — вчетверо"
    );
}

/// Срок замерзает в момент ухода и больше не пересчитывается (§12.70) — как
/// курс сделки замерзает в момент заказа (§12.44).
///
/// До ухода срок не посчитан вовсе: он зависит от того, сколько лап дойдёт до
/// шлюза, а до тех пор состав ещё может измениться.
#[test]
fn the_span_is_frozen_when_the_squad_leaves() {
    let rows = &["########", "#ab....#", "########"];
    let (mut sim, m) = sim_with_gate(rows, (6, 1), 1, 40);
    sim.set_squad_range(m, 1, 2);
    sim.set_mission_work(m, 40);
    assert!(sim.launch(m, squad(&["a", "b"])));

    assert_eq!(sim.mission_left(), Some(0), "до ухода срока нет");
    sim.tick_n(12);
    assert!(sim.is_away("a") && sim.is_away("b"), "ушли вдвоём");

    // 40 дороги + 40/2 работы, минус тики, что уже прошли в поле.
    let left = sim.mission_left().expect("миссия идёт");
    assert!(
        left <= 60 && left > 40,
        "срок посчитан по двум лапам: {left}"
    );
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

    assert!(sim.cancel_mission(m));
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

    assert!(!sim.cancel_mission(m), "отозвать нельзя");
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

/// Мир из `exhaustion_makes_the_raid_wait`, но с лежанкой рядом с `b`: кот
/// укладывается спать сам, по порогу усталости, а не падает без сил.
/// Возвращает симуляцию, миссию и клетку лежанки.
fn sim_with_a_sleeping_cat(spare: bool) -> (Sim, usize, (i32, i32)) {
    let rows = &[
        "##########",
        "#a.......#",
        "#........#",
        "#b.......#",
        "##########",
    ];
    let (mut sim, m) = sim_with_gate(rows, (8, 1), 2, 20);
    let bed = (1, 2);
    sim.set_rest(2, 1);
    sim.force_tile(bed.0, bed.1, 2);
    sim.set_needs(100, 50, 1);
    sim.set_auto_rest(spare);
    sim.set_energy("b", 40);
    sim.tick_n(3);
    assert!(sim.is_resting("b") && sim.pos_of("b") == bed, "b лёг спать");
    (sim, m, bed)
}

/// Спящего заявка не поднимает (§12.51) и **не ждёт** (§12.70): он остаётся
/// дома, а вылазка уходит без него, раз оставшихся хватает на минимум.
///
/// До §12.70 спящего записывали в отряд, и бригада стояла у шлюза всё время его
/// сна. Ожидание — это не ротация, а простой: подменить кота в бригаде некем, и
/// один невыносливый держал бы весь узел.
#[test]
fn a_sleeping_cat_is_left_at_home() {
    let (mut sim, m, bed) = sim_with_a_sleeping_cat(true);
    sim.set_squad_range(m, 1, 2);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(20);

    assert!(!sim.in_squad("b"), "в отряд не записан вовсе");
    assert!(sim.is_resting("b"), "спит дальше");
    assert_eq!(sim.pos_of("b"), bed, "и с лежанки не вставал");
    assert!(sim.is_away("a"), "а вылазка ушла без него");
}

/// Минимум — единственное, что вылазка требует безусловно: если готовых меньше,
/// заявка не принимается вовсе (§12.70).
#[test]
fn a_raid_below_its_minimum_is_refused() {
    let (mut sim, m, _) = sim_with_a_sleeping_cat(true);
    assert!(
        !sim.launch(m, squad(&["a", "b"])),
        "готов один, а нужно двое"
    );
    assert_eq!(sim.mission_left(), None, "миссии не завелось");
}

/// Выключенное «Беречь себя» — это решение игрока не жалеть котов, и вылазка
/// поднимает спящего сразу: цена та же, что у работы до нуля бодрости (§12.33).
#[test]
fn a_raid_wakes_the_sleeper_when_self_care_is_off() {
    let (mut sim, m, bed) = sim_with_a_sleeping_cat(false);
    assert!(sim.launch(m, squad(&["a", "b"])));

    assert!(!sim.is_resting("b"), "заявка подняла спящего");
    sim.tick_n(4);
    assert_ne!(sim.pos_of("b"), bed, "и он пошёл к шлюзу");

    sim.tick_n(21);
    assert!(
        sim.is_away("a") && sim.is_away("b"),
        "отряд ушёл невыспавшимся"
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

/// Один узел связи — одна вылазка за раз (§12.59): вторая заявка не принимается,
/// пока первая не закрыта. В этой схеме узел один — он же клетка шлюза.
#[test]
fn one_node_holds_one_raid() {
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

// --- узлы связи: сколько вылазок разом (§12.59) -----------------------------

/// Мир с одним шлюзом и `nodes` узлами связи, тремя котами и вылазками на
/// одного: слотов ровно столько, сколько узлов. Тайл 1 — шлюз, тайл 3 — узел,
/// узлы лежат в нижнем ряду и ничем, кроме счёта, себя не проявляют.
fn sim_with_nodes(nodes: i32) -> Sim {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(2, 1, 1);
    sim.set_relay(3, true);
    for i in 0..nodes {
        sim.force_tile(1 + i, 2, 3);
    }
    sim
}

/// Два узла — два отряда в поле одновременно. Это и есть вся суть §12.59:
/// потолок параллельных вылазок перестал быть числом в коде.
#[test]
fn two_raids_run_at_once_with_two_nodes() {
    let mut sim = sim_with_nodes(2);
    let first = sim.set_mission(1, 30, &[(0, 5)]);
    let second = sim.set_mission(1, 30, &[(0, 5)]);

    assert!(sim.launch(first, squad(&["a"])), "первый узел занят");
    assert!(sim.launch(second, squad(&["b"])), "второй тоже");
    assert_eq!(sim.raid_count(), 2);

    sim.tick_n(10);
    assert!(sim.is_away("a") && sim.is_away("b"), "оба отряда в поле");
}

/// Третьей заявке при двух узлах отказывают: слот — это построенная клетка, а
/// не намерение игрока.
#[test]
fn a_third_raid_is_refused() {
    let mut sim = sim_with_nodes(2);
    let first = sim.set_mission(1, 30, &[(0, 5)]);
    let second = sim.set_mission(1, 30, &[(0, 5)]);
    let third = sim.set_mission(1, 30, &[(0, 5)]);
    sim.launch(first, squad(&["a"]));
    sim.launch(second, squad(&["b"]));

    assert!(!sim.launch(third, squad(&["c"])), "узлов больше нет");
    assert_eq!(sim.raid_count(), 2, "и третья миссия не завелась");
    assert!(!sim.in_squad("c"), "а кот остался при базе");
}

/// Ноль узлов — ноль вылазок, ровно как ноль мастерских это ноль заказов
/// (§12.55). Исключения «одна всегда бесплатна» нет: оно сделало бы первый узел
/// бесполезным, а правило — правилом с оговоркой.
#[test]
fn no_node_means_no_raid() {
    let mut sim = sim_with_nodes(0);
    let m = sim.set_mission(1, 30, &[(0, 5)]);

    assert!(!sim.launch(m, squad(&["a"])), "связи нет — отряд не выйдет");
    assert_eq!(sim.raid_count(), 0);
}

/// Двух вылазок по одному заказу не бывает: отменять их пришлось бы по номеру в
/// списке, а порядок обхода сущностей ECS недетерминирован (§11, §12.55).
#[test]
fn the_same_mission_is_not_taken_twice() {
    let mut sim = sim_with_nodes(2);
    let m = sim.set_mission(1, 30, &[(0, 5)]);

    assert!(sim.launch(m, squad(&["a"])), "заказ взят");
    assert!(!sim.launch(m, squad(&["b"])), "и второй раз не берётся");
    assert_eq!(sim.raid_count(), 1, "хотя узел свободен");
}

/// Отмена адресуется заказом и трогает **только свою** вылазку. До §12.59
/// отменять было нечего выбирать: миссия была одна.
#[test]
fn cancelling_one_raid_leaves_the_other() {
    let mut sim = sim_with_nodes(2);
    let first = sim.set_mission(1, 50, &[(0, 5)]);
    let second = sim.set_mission(1, 50, &[(0, 5)]);
    sim.launch(first, squad(&["a"]));
    sim.launch(second, squad(&["b"]));
    assert!(sim.in_squad("a") && sim.in_squad("b"), "оба отряда набраны");

    // Не тикаем: шлюз рядом, и через тик отряд ушёл бы, а ушедших не отзывают.
    assert!(sim.cancel_mission(first));
    assert!(!sim.in_squad("a"), "первый отряд распущен");
    assert!(sim.in_squad("b"), "а второй идёт как шёл");
    assert!(sim.raid_left(second).is_some(), "и его вылазка жива");
    assert_eq!(sim.raid_count(), 1, "слот освободился ровно один");
}

/// Узел остался **лицензией, а не рабочим местом** (§12.59), как и торговый
/// пост: за ним никто не работает и на нём никто не стоит. Проверка на каждом
/// тике — иначе однотиковый заход замаскировался бы конечным состоянием.
#[test]
fn a_node_never_becomes_a_workplace() {
    let mut sim = sim_with_nodes(2);
    let m = sim.set_mission(1, 30, &[(0, 5)]);
    sim.launch(m, squad(&["a"]));

    for _ in 0..60 {
        sim.tick_n(1);
        for id in ["a", "b", "c"] {
            assert_ne!(sim.pos_of(id), (1, 2), "кот работает за узлом");
            assert_ne!(sim.pos_of(id), (2, 2), "и за вторым тоже");
        }
    }
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

/// Стартовая застройка даёт **ровно один** узел связи (§12.59). Один — потому
/// что первая вылазка обязана быть доступна сразу: без неё не берётся
/// обязательная цель «Первая вылазка» (§12.58). И ровно один — потому что иначе
/// второй узел нечего строить, а вместе с ним пропадает единственная причина
/// его хотеть. Ловит и потерянную в `build:` рацию, и случайно размноженную.
#[test]
fn the_shipped_ruleset_starts_with_one_relay_node() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let relay = 11; // индекс `relay` в палитре тайлов

    assert_eq!(sim.relay_count(), 1, "рация в гараже одна");
    assert_eq!(sim.tile_tech(relay), None, "и технологией не закрыта");
    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "первая вылазка доступна с первого тика",
    );
}

#[test]
fn the_shipped_ruleset_has_a_mission_that_is_out_of_reach() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    // Связь глушим: тест про лестницу сложности, а дежурный у рации прибавляет
    // отряду силы и делает «заведомо провальную» вылазку спорной (§12.60).
    sim.without_comms();
    let before = sim.scrap_total();

    // Известность и доверие Полиции выдаём напрямую: здесь проверяется потолок
    // сложности, а не лестница ворот — её ведут
    // `the_shipped_ruleset_has_a_reachable_ladder` и тесты фракций (§12.43).
    sim.set_fame(60);
    sim.set_standing(0, 30);
    assert!(
        sim.launch(3, squad(&["excellent", "sp2"])),
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
    // (§12.40), и это первый по `id` — просто детерминизм, причины плена игра
    // не называет. Вернувшийся валится у шлюза без сил.
    assert!(sim.is_captive("excellent"), "одного оставили там");
    assert!(sim.is_resting("sp2"), "а второй свалился без сил");
}

/// Ворота вылазки считает ядро, а не панель (§12.24).
///
/// До этого «хватает ли известности» жило в JS, рядом с тем же правилом,
/// посчитанным в `launch`. Два экземпляра одного правила однажды расходятся, и
/// расхождение это видно игроку как кнопка, которая нажимается и ничего не
/// делает.
#[test]
fn the_core_says_whether_a_raid_is_open() {
    let (mut sim, mission) = sim_with_gate(&["#####", "#a.b#", "#####"], (2, 1), 2, 10);
    sim.set_mission_fame(mission, 0, 30);

    let gates = sim.raid_gates(mission);
    assert!(!gates.unlocked, "о базе ещё не слышали — вылазка закрыта");
    assert!(gates.possible, "но цель у неё есть");
    assert!(
        !sim.launch(mission, squad(&["a", "b"])),
        "и заявку ядро отклоняет — панель говорит то же, что фасад",
    );

    sim.set_fame(30);
    assert!(sim.raid_gates(mission).unlocked, "порог взят");
    assert!(sim.launch(mission, squad(&["a", "b"])), "теперь пускают");
}

/// У вылазки за своим цель появляется вместе с пленным (§12.40).
///
/// Второе правило, жившее в JS: «все дома, спасать некого». Оно того же рода,
/// что и известность, — знание ядра о том, есть ли ещё кого возвращать.
#[test]
fn a_rescue_raid_has_no_target_while_everyone_is_home() {
    let (mut sim, _) = sim_with_gate(&["#####", "#a.b#", "#####"], (2, 1), 2, 10);
    let rescue = sim.set_rescue_mission(1, 10, 0);

    let gates = sim.raid_gates(rescue);
    assert!(gates.unlocked, "порога у неё нет");
    assert!(!gates.possible, "но идти не за кем");
    assert!(
        !sim.launch(rescue, squad(&["a"])),
        "и заявку ядро отклоняет",
    );
}

// —— Автовылазки (§12.67) ————————————————————————————————————————————————
//
// Правило повторяет клик игрока по кнопке заказа, и потому проверяется тем же,
// чем проверялась бы кнопка: ушли или нет. Второго способа уйти в поле у него
// нет — все ворота остаются у `launch_node`.

/// Правило в чистом виде: игрок нажал один раз, отряд ушёл сам.
#[test]
fn a_node_with_a_rule_goes_out_by_itself() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    assert!(sim.enlist("a", 1, 2), "кот зачислен в отряд узла");
    assert!(sim.set_auto_raid(def as i32, 1, 2), "правило поставлено");

    sim.tick_n(10);
    assert!(sim.is_away("a"), "отряд ушёл без второго клика");
    assert_eq!(sim.raid_count(), 1, "и ровно одна вылазка");
}

/// То, ради чего правило и заводилось: вернувшийся отряд уходит снова. Без
/// этого автовылазка экономит один клик, а не рутину сотен тиков.
#[test]
fn the_rule_sends_the_squad_out_again_after_it_returns() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(200);
    // Журнал `Raids` помнит заказ, а не число ходок (§12.58), поэтому «сходил не
    // раз» читается парой: первая вылазка уже закрыта журналом, а прямо сейчас
    // идёт следующая — и увёл в неё отряд не игрок.
    assert!(
        sim.raids_done().contains(&def),
        "первая вылазка кончилась успехом",
    );
    assert_eq!(sim.raid_count(), 1, "и следующая уже идёт");
    assert!(sim.is_away("a"), "тем же отрядом");
}

/// Раненого правило не отправляет — и не снимается: оно ждёт, как порог ждёт
/// материала (§12.30). Иначе автовылазка отменяла бы цену провала (§12.37).
#[test]
fn the_rule_waits_while_a_cat_is_hurt() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    // Яма глубокая намеренно: самозаживление идёт всегда и не меньше очка за
    // тик, а рядом стоят коты, готовые лечить, — тест меряет «не ушёл, пока не
    // встал», а не скорость лечения.
    sim.set_health_rules(100, 90, 0);
    sim.set_health("a", 1);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(10);
    assert_eq!(sim.raid_count(), 0, "раненый в поле не идёт");

    sim.set_health("a", 100);
    sim.tick_n(20);
    assert!(sim.is_away("a"), "а как встал — правило его отправило");
}

/// Спящего правило тоже ждёт. `launch_node` принял бы заявку и над спящим
/// (§12.51), но правило повторяется каждый тик — и такая заявка заняла бы узел
/// на всё время сна.
#[test]
fn the_rule_waits_for_a_sleeping_cat() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.set_needs(100, 40, 1);
    sim.set_energy("a", 5);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(20);
    assert_eq!(sim.raid_count(), 0, "уставший спит, а не идёт в поле");
}

/// Снятое правило новых вылазок не заводит, но идущую **не отзывает**: отряд
/// уже в поле, а оттуда не отзывают вовсе (§12.22).
#[test]
fn clearing_the_rule_stops_the_next_raid_but_not_the_running_one() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);
    sim.tick_n(10);
    assert!(sim.is_away("a"), "первая вылазка ушла");

    assert!(sim.set_auto_raid(-1, 1, 2), "правило снято");
    sim.tick_n(5);
    assert_eq!(sim.raid_count(), 1, "идущую вылазку это не тронуло");

    sim.tick_n(200);
    assert_eq!(sim.raid_count(), 0, "а новой правило больше не заводит");
    assert_eq!(sim.raids_done().len(), 1, "сходили ровно раз");
}

/// Правило на снесённой рации убирает за собой само: узла нет — нет и строки.
/// Та же оговорка, из-за которой §12.65 пришлось убирать заказ снятого порога.
#[test]
fn a_rule_on_a_demolished_node_disappears() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    // Рации больше нет: клетка стала обычным полом.
    sim.force_tile(1, 2, 0);
    sim.tick_n(10);
    assert_eq!(sim.raid_count(), 0, "без узла вылазки не идут");
    assert!(sim.auto_raid_at(1, 2).is_none(), "и правило не осталось");
}

/// Неполный состав правило тоже ждёт: сколько нужно котов, знает заказ, и
/// решает это `launch_node` — второго экземпляра проверки у правила нет.
#[test]
fn the_rule_waits_for_a_full_squad() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(2, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(20);
    assert_eq!(sim.raid_count(), 0, "одного на двухместный заказ мало");

    sim.enlist("b", 1, 2);
    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"), "вдвоём ушли");
}

/// Явный приказ отменяет правило (§12.72): игрок отправил узел на **другой**
/// заказ, то есть принял решение взамен прежнего. Уцелевшее правило вернуло бы
/// отряд в поле тем же тиком, каким тот дошёл до базы, — коты вернулись бы с
/// назначенной вылазки и «сами собой» исчезли снова.
#[test]
fn a_manual_launch_elsewhere_drops_the_node_rule() {
    let mut sim = sim_with_nodes(1);
    let usual = sim.set_mission(1, 30, &[(0, 5)]);
    let other = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(usual as i32, 1, 2);

    assert!(sim.launch_node(other, 1, 2), "игрок отправил отряд сам");
    assert!(sim.auto_raid_at(1, 2).is_none(), "правило снято приказом");

    // И отряд после возвращения остаётся дома: рутина кончилась вместе с
    // правилом, а не пережила его.
    sim.tick_n(200);
    assert!(sim.raids_done().contains(&other), "ручная вылазка сходила");
    assert_eq!(sim.raid_count(), 0, "а новую заводить больше некому");
}

/// Граница снятия: правило уходит в поле **через ту же кнопку**, поэтому
/// сравнение идёт с заказом, а не с самим фактом заявки. Со своим заказом
/// правило снять себя не может — иначе автовылазка сработала бы ровно один раз.
#[test]
fn the_rule_survives_its_own_departure() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(10);
    assert!(sim.is_away("a"), "правило отправило отряд");
    assert_eq!(sim.auto_raid_at(1, 2), Some(def), "и осталось на узле");
}

/// Ручная отправка на **тот же** заказ правило тоже не трогает: другого
/// решения игрок не принимал, он лишь поторопил то же самое.
#[test]
fn a_manual_launch_of_the_same_mission_keeps_the_rule() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    assert!(sim.launch_node(def, 1, 2), "отправили руками тот же заказ");
    assert_eq!(sim.auto_raid_at(1, 2), Some(def), "правило на месте");
}
