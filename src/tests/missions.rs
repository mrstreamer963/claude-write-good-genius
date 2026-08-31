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

/// Стадия вылазки — дорога туда, работа на месте, дорога назад (§12.168), и
/// считает её ядро тем же разложением срока, каким его сложил `duration`.
///
/// Проверяется прогоном, а не арифметикой на месте: стадия обязана меняться в
/// живом мире по мере того, как тикает `left`, — второй экземпляр этого
/// разложения в виде однажды скажет «работают» про отряд, который ещё в пути
/// (инвариант 14).
#[test]
fn a_raid_reports_its_phase() {
    let rows = &["########", "#a.....#", "########"];
    // Дорога 40 (по 20 в конец), работа 40 очков: соло делает её за 40 тиков.
    let (mut sim, m) = sim_with_gate(rows, (6, 1), 1, 40);
    sim.set_mission_work(m, 40);
    assert!(sim.launch(m, squad(&["a"])));
    assert_eq!(sim.mission_phase(), None, "до ухода стадии нет");

    while !sim.is_away("a") {
        sim.tick_n(1);
    }
    assert_eq!(sim.mission_span(), Some(80), "40 дороги и 40 работы");
    assert_eq!(sim.mission_phase(), Some("travel"), "первым делом дорога");

    // Дорога в один конец — половина всей дороги.
    while sim.mission_left().is_some_and(|left| left > 80 - 20) {
        sim.tick_n(1);
    }
    assert_eq!(sim.mission_phase(), Some("work"), "дошли — работают");

    while sim.mission_left().is_some_and(|left| left > 80 - 60) {
        sim.tick_n(1);
    }
    assert_eq!(sim.mission_phase(), Some("back"), "отработали — назад");
}

/// Работы на месте может не быть вовсе (`work: 0`, поведение до §12.70) — тогда
/// отряд разворачивается на середине дороги, и «работают» не наступает никогда.
/// Стадия обязана это выдержать: пустой отрезок между двумя дорогами — не повод
/// сообщать о нём словом.
#[test]
fn a_raid_without_work_never_reports_working() {
    let rows = &["########", "#a.....#", "########"];
    let (mut sim, m) = sim_with_gate(rows, (6, 1), 1, 40);
    assert!(sim.launch(m, squad(&["a"])));
    while !sim.is_away("a") {
        sim.tick_n(1);
    }

    let mut seen = Vec::new();
    while sim.mission_left().is_some_and(|left| left > 0) {
        if let Some(p) = sim.mission_phase()
            && seen.last() != Some(&p)
        {
            seen.push(p);
        }
        sim.tick_n(1);
    }
    assert_eq!(seen, vec!["travel", "back"], "работать на месте нечего");
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

/// Больше предела бригада не уводит, а несуществующий кот отрядом не считается.
/// Недобор в этот список **не входит** (§12.113): он разрешён, см. соседний тест.
#[test]
fn an_oversized_or_bogus_squad_is_refused() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    assert!(!sim.launch(m, squad(&["a", "b", "c"])), "больше предела");
    assert_eq!(sim.mission_left(), None, "заявка не прошла");

    // Дубликат и призрак состав не надувают: «три раза a» — это один кот. До
    // §12.113 такая заявка отклонялась минимумом, теперь она уходит недоборем,
    // и проверять надо именно длину отряда, а не факт отказа.
    assert!(sim.launch(m, squad(&["a", "a"])), "уходит, но один");
    assert!(sim.in_squad("a"), "и это он");
    assert!(!sim.in_squad("b") && !sim.in_squad("c"), "больше никого");
}

/// Недокомплект разрешён (§12.113): минимум вилки — рекомендация, а не допуск.
/// Отдельного штрафа за него нет и быть не должно — цена уже посчитана дважды:
/// доля добычи считается по силе отряда, а срок делится на число лап.
#[test]
fn an_undermanned_squad_goes_and_pays_for_it() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 2, 50);
    sim.set_mission_work(m, 40);
    let alone = sim.mission_span_of(m, 1);
    let full = sim.mission_span_of(m, 2);
    assert!(alone > full, "один в поле дольше: {alone} против {full}");

    assert!(sim.launch(m, squad(&["a"])), "заявка на одного принята");
    sim.tick_n(80);
    assert!(sim.is_away("a"), "и он ушёл один");
    assert!(!sim.is_away("b") && !sim.is_away("c"), "остальные дома");
}

/// Прогноз считает по тем лапам, которые заказ **и правда** уведёт: сверх
/// `squad_max` в поле не выходит никто (§12.113), и срок, посчитанный по всему
/// перекомплектованному составу узла, обещал бы работу впятером там, где
/// пойдут четверо.
#[test]
fn a_span_never_counts_more_paws_than_the_order_takes() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a.c.b#", "#######"], (3, 1), 1, 50);
    sim.set_squad_range(m, 1, 2);
    sim.set_mission_work(m, 40);

    let full = sim.mission_span_of(m, 2);
    assert_eq!(sim.mission_span_of(m, 3), full, "третья лапа срок не режет");
    assert_eq!(sim.mission_span_of(m, 9), full, "и девятая тоже");
    assert!(sim.mission_span_of(m, 1) > full, "а недобор платит сроком");
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
    sim.tick_n(12);
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

/// Спящий не держит вылазку и после §12.113: заявка уходит тем составом,
/// который готов, — даже если готовых меньше минимума вилки.
#[test]
fn a_raid_below_its_minimum_leaves_without_the_sleeper() {
    let (mut sim, m, bed) = sim_with_a_sleeping_cat(true);
    assert!(
        sim.launch(m, squad(&["a", "b"])),
        "готов один из двух, и этого довольно (§12.113)",
    );
    assert!(sim.in_squad("a") && !sim.in_squad("b"), "спящего не взяли");

    sim.tick_n(25);
    assert!(sim.is_away("a"), "вылазка ушла недокомплектом");
    assert_eq!(sim.pos_of("b"), bed, "а спящий досыпает своё");
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

// --- гаражи: сколько вылазок разом (§12.59, §12.152) ------------------------

/// Мир с `nodes` гаражами, тремя котами и вылазками на одного: слотов ровно
/// столько, сколько гаражей. Тайл 1 — гараж, гаражи лежат в нижнем ряду.
///
/// До §12.152 слоты считала рация, а шлюз был один и подбирался автоматом;
/// теперь это одна и та же клетка, и второй тайл здесь не нужен.
fn sim_with_nodes(nodes: i32) -> Sim {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    for i in 0..nodes {
        sim.force_tile(1 + i, 2, 1);
    }
    sim
}

/// Два гаража — два отряда в поле одновременно. Это и есть вся суть §12.59:
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

/// Гараж остался **дверью, а не рабочим местом** (§12.59, §12.152): на нём
/// стоит только уходящий отряд, а всем прочим там делать нечего — за гаражом
/// никто не работает и ничего к нему не возят.
///
/// До §12.152 это же свойство проверялось у рации, и проверялось на всех котах
/// разом: слот был чистой лицензией, и на его клетку не вставал никто. Теперь
/// слот и дверь — одна клетка, и уходящий на ней стоит по определению (§12.22),
/// поэтому спрашивается то, что от свойства осталось: **остальные** туда не
/// ходят. Проверка на каждом тике — иначе однотиковый заход замаскировался бы
/// конечным состоянием.
#[test]
fn a_node_never_becomes_a_workplace() {
    let mut sim = sim_with_nodes(2);
    let m = sim.set_mission(1, 30, &[(0, 5)]);
    sim.launch(m, squad(&["a"]));

    for _ in 0..60 {
        sim.tick_n(1);
        for id in ["b", "c"] {
            assert_ne!(sim.pos_of(id), (1, 2), "кот работает за гаражом");
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

/// Стартовая застройка даёт **ровно один** гараж и **ни одной** рации
/// (§12.59, §12.152).
///
/// Гараж один — потому что первая вылазка обязана быть доступна сразу (без неё
/// не берётся обязательная цель «Первая вылазка», §12.58), и ровно один —
/// потому что с §12.152 гараж держит слот вылазки: заложи его залом, и база
/// стартует с двумя десятками одновременных отрядов, то есть счётность
/// выключена молча. Ровно этим сторож и полезен: правку `rect` в `build:`
/// сделать легко, а увидеть её последствие — нет.
///
/// Рации нет **намеренно**: слота она больше не держит, значит первая вылазка
/// без неё уходит, — и разница «с рацией / без» стала тем, что игрок открывает
/// сам. Вернуть её в `build:` легко и по привычке, поэтому и стережём.
#[test]
fn the_shipped_ruleset_starts_with_one_relay_node() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let relay = 11; // индекс `relay` в палитре тайлов
    let garage = 3; // индекс `garage` в палитре тайлов

    assert_eq!(sim.gate_count(), 1, "гараж — одна клетка, а не зал");
    assert_eq!(sim.relay_count(), 0, "а рации на старте нет вовсе");
    assert_eq!(sim.tile_tech(relay), None, "рация технологией не закрыта");
    assert_eq!(sim.tile_tech(garage), None, "и гараж тоже");
    assert!(
        sim.launch(0, squad(&["excellent", "sp2"])),
        "первая вылазка доступна с первого тика и без связи",
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

/// Правило ждёт не состава, а **исхода** (§12.117): на заказе, который одному
/// не по силам, отряд стоит, пока не наберётся полная доля.
///
/// До §12.117 здесь стоял счёт лап («одного на двухместный заказ мало»), и он
/// мерил не то: число в вилке — про срок, а справится ли отряд, считает
/// `outcome`. На безопасном заказе тот же одиночка теперь уходит сам — это
/// проверяет `the_gate_measures_the_outcome_not_the_headcount`.
#[test]
fn the_rule_waits_until_the_odds_are_full() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_risky_mission(2, 30, 2, 0, &[(0, 5)]);
    sim.set_squad_range(def, 1, 3);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    sim.tick_n(20);
    assert_eq!(sim.raid_count(), 0, "одному сложность 2 не поднять целиком");

    sim.enlist("b", 1, 2);
    sim.tick_n(20);
    assert!(sim.is_away("a") && sim.is_away("b"), "вдвоём ушли");
}

/// Явный приказ усыпляет правило (§12.72, §12.77): игрок отправил узел на
/// **другой** заказ, то есть принял решение взамен ближайшего круга. Уцелевшее
/// активным правило вернуло бы отряд в поле тем же тиком, каким тот дошёл до
/// базы, — коты вернулись бы с назначенной вылазки и «сами собой» исчезли снова.
#[test]
fn a_manual_launch_elsewhere_pauses_the_node_rule() {
    let mut sim = sim_with_nodes(1);
    let usual = sim.set_mission(1, 30, &[(0, 5)]);
    let other = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(usual as i32, 1, 2);

    assert!(sim.launch_node(other, 1, 2), "игрок отправил отряд сам");
    assert!(!sim.auto_raid_is_on(1, 2), "правило усыплено приказом");
    assert_eq!(sim.auto_raid_at(1, 2), Some(usual), "но заказ помнится");

    // И отряд после возвращения остаётся дома: усыплённое правило круга не
    // ведёт, пока игрок его не разбудил.
    sim.tick_n(200);
    assert!(sim.raids_done().contains(&other), "ручная вылазка сходила");
    assert_eq!(sim.raid_count(), 0, "а новую заводить больше некому");
}

/// Пауза в чистом виде (§12.77): игрок прервал рутину, разгрёб базу и вернул
/// правило в строй — не выбирая заказ второй раз.
#[test]
fn a_paused_rule_stays_home_and_resumes_by_the_same_switch() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    sim.enlist("a", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);

    assert!(sim.set_auto_raid_on(1, 2, false), "правило усыплено");
    sim.tick_n(50);
    assert_eq!(sim.raid_count(), 0, "неактивное правило в поле не гонит");
    assert_eq!(sim.auto_raid_at(1, 2), Some(def), "но заказ помнится");

    assert!(
        sim.set_auto_raid_on(1, 2, true),
        "и будится тем же тумблером"
    );
    sim.tick_n(10);
    assert!(sim.is_away("a"), "отряд снова ушёл сам");
}

/// Отзыв собравшегося отряда усыпляет правило (§12.77). Без этого «Отозвать» у
/// автоматического отряда не значит ничего: заявка заводится заново тем же
/// тиком, и кнопка читается как сломанная.
#[test]
fn recalling_an_auto_squad_pauses_the_rule() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    // Кот подальше от шлюза: отозвать можно только тех, кто ещё на базе
    // (§12.22), а от соседней клетки отряд уходит первым же тиком.
    sim.enlist("c", 1, 2);
    sim.set_auto_raid(def as i32, 1, 2);
    sim.tick_n(1);
    assert_eq!(sim.raid_count(), 1, "правило подало заявку");

    assert!(sim.cancel_mission(def), "игрок отозвал отряд");
    assert!(!sim.auto_raid_is_on(1, 2), "правило усыплено отзывом");
    assert_eq!(sim.auto_raid_at(1, 2), Some(def), "но заказ помнится");

    sim.tick_n(50);
    assert_eq!(sim.raid_count(), 0, "и заявка заново не заводится");
}

/// Будить нечего — правила нет: тумблер сам правил не заводит, это делает
/// только `set_auto_raid` (§12.77).
#[test]
fn switching_a_missing_rule_changes_nothing() {
    let mut sim = sim_with_nodes(1);
    assert!(!sim.set_auto_raid_on(1, 2, true), "правила на узле нет");
    assert!(sim.auto_raid_at(1, 2).is_none(), "и тумблер его не завёл");
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

/// Автовылазка открывается наукой (§12.93) — и это единственное правило, которое
/// само отправляет котов в поле, поэтому в боевом рулсете его тема второго
/// уровня. Ворота живут в рулсете, в схеме их нет: включаем руками.
#[test]
fn an_auto_raid_rule_needs_its_technology() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    assert!(sim.enlist("a", 1, 2), "кот зачислен в отряд узла");
    sim.set_auto_gates("", "", "callsigns");

    assert!(
        !sim.set_auto_raid(def as i32, 1, 2),
        "технологии нет — правило не ставится"
    );
    assert_eq!(sim.auto_raid_at(1, 2), None);

    sim.set_tech("callsigns");
    assert!(sim.set_auto_raid(def as i32, 1, 2), "изучили — можно");
    assert_eq!(sim.auto_raid_at(1, 2), Some(def));

    // Снятие ворот не спрашивает.
    sim.forget_techs();
    assert!(sim.set_auto_raid(-1, 1, 2), "снять можно всегда");
    assert_eq!(sim.auto_raid_at(1, 2), None);
}

// --- разведка: лишний кот мешает (§12.113) ----------------------------------

/// Мир для разведки: коридор со шлюзом, три кота и один заказ, у которого
/// вилка от одного до трёх. Опасность задаётся тестом.
fn sim_with_stealth(danger: i32, stealth: bool) -> (Sim, usize) {
    let mut sim = sim_from(&["#######", "#a.c.b#", "#######"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let m = sim.set_risky_mission(1, 10, danger, 0, &[(0, 10)]);
    sim.set_squad_range(m, 1, 3);
    if stealth {
        sim.set_stealth_mission(m);
    }
    (sim, m)
}

/// Разведка меняет знак решения о составе: **соло приносит добычу, а пара
/// проваливается** (§12.113). Работают тут обе половины правила разом — сила по
/// лучшему (пара не сильнее одного) и опасность, умноженная на число лап.
#[test]
fn a_stealth_raid_is_better_alone_than_in_a_pair() {
    let (mut sim, m) = sim_with_stealth(2, true);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    let alone = sim.scrap_total();
    assert!(alone > 0, "один принёс долю добычи: {alone}");

    let (mut sim, m) = sim_with_stealth(2, true);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);
    assert_eq!(sim.scrap_total(), 0, "вдвоём заметили — провал");
}

/// Контроль: без флага тот же заказ ведёт себя ровно наоборот — вдвоём лучше.
/// Значит дело в `stealth`, а не в сроке, добыче или числе тиков.
#[test]
fn an_ordinary_raid_is_better_in_a_pair_than_alone() {
    let (mut sim, m) = sim_with_stealth(2, false);
    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    let alone = sim.scrap_total();

    let (mut sim, m) = sim_with_stealth(2, false);
    assert!(sim.launch(m, squad(&["a", "b"])));
    sim.tick_n(40);
    assert!(
        sim.scrap_total() > alone,
        "сумма сил берёт заказ целиком: {} против {alone}",
        sim.scrap_total(),
    );
}

/// Нулевая опасность разведке безразлична — общее правило нулей в рулсете
/// (§12.113): множитель ничего не множит, и втроём заказ удаётся целиком.
#[test]
fn stealth_means_nothing_at_zero_danger() {
    let (mut sim, m) = sim_with_stealth(0, true);
    assert!(sim.launch(m, squad(&["a", "b", "c"])));
    sim.tick_n(40);
    assert_eq!(sim.scrap_total(), 10, "вся добыча, сколько бы лап ни шло");
}

/// Сила разведотряда — **лучший**, а не сумма (§12.113). Кот с уровнем
/// «Вылазки» тащит заказ, который троим новичкам не по зубам, — и он же не
/// становится сильнее оттого, что рядом идут двое.
#[test]
fn a_stealth_raid_counts_the_best_cat_not_the_sum() {
    let (mut sim, m) = sim_with_stealth(4, true);
    let raid = sim.set_skill("raid", &[10]);
    sim.set_xp("a", raid, 10); // уровень 1: сила 2 против единицы у новичка

    assert!(sim.launch(m, squad(&["a"])));
    sim.tick_n(40);
    assert!(sim.scrap_total() > 0, "мастер прошёл в одиночку");

    // Контроль: дело в навыке, а не в том, что заказ берётся кем угодно.
    let (mut sim, m) = sim_with_stealth(4, true);
    assert!(sim.launch(m, squad(&["b"])));
    sim.tick_n(40);
    assert_eq!(sim.scrap_total(), 0, "новичку тот же заказ не по силам");

    let (mut sim, m) = sim_with_stealth(4, true);
    let raid = sim.set_skill("raid", &[10]);
    sim.set_xp("a", raid, 10);
    assert!(sim.launch(m, squad(&["a", "b", "c"])));
    sim.tick_n(40);
    assert_eq!(
        sim.scrap_total(),
        0,
        "втроём та же сила, но втрое больше заметности",
    );
}

/// **Почему правило стоит, отвечает ядро — и теми же выражениями, какими само
/// решает, идти ли** (§12.116, §12.117).
///
/// Правило уводит отряд только на полной доле, и до §12.116 это было тишиной:
/// заказ выбран, правило «включено», а отряд стоит месяцами. Игрок, вычеркнувший
/// кота, читает такое как поломку, а не как своё же решение.
#[test]
fn a_rule_says_why_it_holds_the_squad() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_risky_mission(1, 30, 2, 0, &[(0, 5)]);
    sim.set_squad_range(def, 1, 3);
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.enlist("b", 1, 2));
    assert!(sim.set_auto_raid(def as i32, 1, 2));

    assert_eq!(
        sim.auto_hold_at(1, 2),
        (100, false, true),
        "двоих на сложность 2 хватает — правило ничего не ждёт",
    );
    sim.tick_n(2);
    assert_eq!(sim.raid_count(), 1, "и отряд ушёл");
}

/// **Полная доля, а не «не провал»** (§12.117): правило повторяет решение без
/// присмотра, и частичная добыча тут не риск, на который игрок согласился, а
/// тихая утечка каждый круг. Отправить как есть можно руками — из штаба, где
/// доля написана до нажатия.
#[test]
fn a_partial_share_holds_the_rule() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_risky_mission(1, 30, 2, 0, &[(0, 5)]);
    sim.set_squad_range(def, 1, 3);
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.set_auto_raid(def as i32, 1, 2));

    let (share, failed, fit) = sim.auto_hold_at(1, 2);
    assert!(!failed && (1..100).contains(&share), "доля есть, но не вся");
    assert!(fit, "и это не про сбор: кот дома, цел и не спит");
    sim.tick_n(5);
    assert_eq!(sim.raid_count(), 0, "правило стоит");

    // Взяли второго — доля полная, и правило пошло само.
    assert!(sim.enlist("b", 1, 2));
    assert_eq!(sim.auto_hold_at(1, 2), (100, false, true));
    sim.tick_n(2);
    assert_eq!(sim.raid_count(), 1);
}

/// **Ворота мерят исход, а не число лап** (§12.117), и видно это на двух краях
/// сразу: одиночка, которому заказ по силам, уходит сам, хотя вилка просит
/// троих, — а трое слабых на тяжёлый заказ не уходят, хотя вилку набирают.
///
/// До §12.117 воротами был минимум вилки, и оба случая он решал наоборот:
/// запрещал заведомо удачное и пускал заведомо провальное — с потерей
/// снаряжения и пленом, каждый круг.
#[test]
fn the_gate_measures_the_outcome_not_the_headcount() {
    let mut sim = sim_with_nodes(2);
    let easy = sim.set_risky_mission(1, 30, 1, 0, &[(0, 5)]);
    let hard = sim.set_risky_mission(1, 30, 9, 0, &[(0, 5)]);
    sim.set_squad_range(easy, 3, 5); // вилка просит троих
    sim.set_squad_range(hard, 1, 5); // а эта — хоть одного

    // Узел с одиночкой на лёгком заказе.
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.set_auto_raid(easy as i32, 1, 2));
    // Узел с тройкой на неподъёмном.
    assert!(sim.enlist("b", 2, 2));
    assert!(sim.enlist("c", 2, 2));
    assert!(sim.set_auto_raid(hard as i32, 2, 2));

    sim.tick_n(3);
    assert_eq!(
        sim.mission_node(easy),
        Some((1, 2)),
        "одиночка ушёл: заказ ему по силам, а вилка про срок, а не про силу",
    );
    assert_eq!(
        sim.mission_node(hard),
        None,
        "а бригада на неподъёмный заказ осталась дома",
    );
}

/// А спящий кот — это **другая** причина: прогноз хорош, но состав не в сборе.
/// Она чинится сама, временем, и путать её со слабым прогнозом нельзя: игроку
/// нечего делать, кроме как подождать.
#[test]
fn a_sleeping_crew_is_a_different_reason() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_risky_mission(1, 30, 1, 0, &[(0, 5)]);
    sim.set_squad_range(def, 1, 3);
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.enlist("b", 1, 2));
    assert!(sim.set_auto_raid(def as i32, 1, 2));
    sim.set_needs(20, 100, 10); // порог усталости есть, и «b» до него дошёл
    sim.set_energy("b", 5);
    sim.tick();

    assert_eq!(
        sim.auto_hold_at(1, 2),
        (100, false, false),
        "прогноз полный, но один из двоих спит",
    );
    assert_eq!(sim.raid_count(), 0, "и правило ждёт, а не уводит одного");
}

/// **Снесённый гараж распускает собирающийся отряд** (§12.152) — как снесённый
/// станок уносит свой заказ (§12.96), а снесённая рация — правило автовылазки
/// (§12.67): вылазка живёт в клетке, а клетки больше нет.
///
/// До §12.152 этого случая не было вовсе: шлюз подбирался заново каждым тиком
/// (`pick_gate`), и снос просто переводил отряд к другой двери. Теперь дверь
/// называет игрок, и подставлять ему другую значило бы отменить его решение
/// молча — а оставить отряд идти в пустоту значило бы повесить его навсегда.
#[test]
fn razing_the_garage_disbands_the_gathering_squad() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 200, &[(0, 5)]);
    assert!(sim.launch(def, squad(&["c"])), "отряд собирается");
    sim.tick_n(2);
    assert!(sim.in_squad("c"), "кот идёт к гаражу");

    sim.force_tile(1, 2, 0); // снесли гараж под собирающимся отрядом
    sim.tick_n(2);

    assert!(!sim.in_squad("c"), "отряд распущен");
    assert_eq!(sim.raid_count(), 0, "и слот освободился");
    assert!(!sim.is_away("c"), "в поле при этом никто не ушёл");
}

/// Ушедшего отряда снос гаража **не касается**: он вернётся туда, откуда ушёл,
/// даже если дверь успели разобрать (§12.22).
#[test]
fn razing_the_garage_does_not_recall_a_departed_squad() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 40, &[(0, 5)]);
    sim.launch(def, squad(&["a"]));
    sim.tick_n(20);
    assert!(sim.is_away("a"), "отряд уже за периметром");

    sim.force_tile(1, 2, 0); // гараж снесли, пока отряд в поле
    sim.tick_n(60);

    assert!(!sim.is_away("a"), "отряд вернулся");
    assert_eq!(sim.raid_left(def), None, "вылазка закрылась как обычно");
}

/// Снесли все гаражи — уйти некуда, и это видно **числом**, а не молчанием
/// (§12.53). Свободной клетки под слот не находится, заявка отклоняется, а вид
/// называет причину словом по тому же счёту гаражей, каким его считает ядро.
#[test]
fn without_a_gate_no_raid_leaves_and_the_count_says_so() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_mission(1, 30, &[(0, 5)]);
    assert_eq!(sim.gate_count(), 1, "гараж на карте один");

    sim.force_tile(1, 2, 0); // снесли гараж: на его месте обычный пол
    assert_eq!(sim.gate_count(), 0, "и снос виден числом");
    assert!(
        !sim.launch(def, squad(&["a"])),
        "без шлюза заявка отклоняется",
    );
    assert_eq!(sim.raid_count(), 0);
}

/// **Прогноз узла считается по всему составу, а не по готовым сию минуту**
/// (§12.184). С §12.148 узел уходит целиком или никак, значит уйдёт весь
/// состав — и сила, проводник и срок обязаны быть его. По готовым полный
/// отряд, прилёгший поспать, описывался как «идут 0 · проводника нет», то есть
/// как вылазка, которой у этого узла не бывает.
#[test]
fn a_sleeping_squad_still_shows_its_full_strength() {
    let mut sim = sim_with_nodes(1);
    let def = sim.set_risky_mission(1, 30, 2, 0, &[(0, 5)]);
    sim.set_squad_range(def, 1, 3);
    sim.set_needs(100, 20, 1);
    assert!(sim.enlist("a", 1, 2));
    assert!(sim.enlist("b", 1, 2));
    assert!(sim.set_auto_raid(def as i32, 1, 2));
    let (share, failed, fit) = sim.auto_hold_at(1, 2);
    assert_eq!((share, failed, fit), (100, false, true));

    // Один укладывается спать: сбор он держит, но прогноз не меняет — уйдут всё
    // равно оба, когда он выспится.
    sim.set_energy("b", 1);
    sim.tick_n(2);
    assert!(sim.is_resting("b"), "кот ушёл спать");
    let (share, failed, fit) = sim.auto_hold_at(1, 2);
    assert_eq!(
        (share, failed),
        (100, false),
        "доля прежняя: состав тот же, просто выйдет позже",
    );
    assert!(!fit, "а вот сбор он держит");
    assert_eq!(
        sim.unfit_at(1, 2),
        vec!["b".to_string()],
        "и панель называет его поимённо — тем же списком, каким гаснет кнопка",
    );
    assert_eq!(sim.raid_count(), 0, "правило ждёт");
}
