//! Участки внешнего мира и очаги на них (§12.198).
//!
//! Проверяем не отдельные функции, а прогон цепочки: очаг растёт и ползёт
//! системой `grow_blights`, а закрывает заказы и снимается на возвращении —
//! фасадом и `run_missions`, то есть в трёх разных местах одного тика.
//!
//! Схема всюду одна: коридор со шлюзом (тайл 1) и коты рядом. Карта заводится
//! явно (`set_site`, `set_blight_kind`) — у схемы `sim_from` её нет, как нет
//! фракций и рынка.

use super::*;

/// Мир со шлюзом и заказом; карту тесты добавляют сами.
fn sim_with_gate(rows: &[&str], gate: (i32, i32), squad: usize, ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    let mission = sim.set_mission(squad, ticks, &[(0, 5)]);
    (sim, mission)
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

// --- рост ------------------------------------------------------------------

#[test]
fn a_blight_climbs_a_stage_when_its_time_comes() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 3, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.seed_blight(site, kind);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "сел первой ступенью");

    sim.tick_n(9);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "срок ещё не вышел");
    sim.tick_n(1);
    assert_eq!(
        sim.blight_at(site),
        Some((kind, 2)),
        "и поднялся на ступень"
    );
    assert_eq!(sim.blight_age(site), 0, "счётчик пошёл заново");
}

#[test]
fn a_blight_stops_at_the_top_stage() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(5, 2, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.seed_blight(site, kind);

    sim.tick_n(100);
    assert_eq!(
        sim.blight_at(site),
        Some((kind, 2)),
        "выше предела не растёт"
    );
}

/// Ноль в `grows` — общее правило нулей: механика выключена, и очаг сидит на
/// той ступени, какой сел. На этом стоят все чужие тесты.
#[test]
fn a_blight_that_never_grows_just_sits() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(0, 3, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.seed_blight(a, kind);

    sim.tick_n(500);
    assert_eq!(sim.blight_at(a), Some((kind, 1)), "ступень не двинулась");
    assert_eq!(sim.blight_at(b), None, "и никуда не расползлось");
}

/// Ноль в `stages` читается как единица: очаг без ступеней — это очаг одной
/// ступени, а не очаг, которого нет.
#[test]
fn no_stages_means_one_stage() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(5, 0, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.seed_blight(site, kind);

    sim.tick_n(50);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "растить было некуда");
}

// --- расползание -----------------------------------------------------------

#[test]
fn a_ripe_blight_spreads_to_a_neighbour() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 2, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.seed_blight(a, kind);

    sim.tick_n(10); // ступень 1 → 2
    assert_eq!(sim.blight_at(a), Some((kind, 2)));
    assert_eq!(sim.blight_at(b), None, "на пределе, но ещё не пополз");

    sim.tick_n(10);
    assert_eq!(
        sim.blight_at(b),
        Some((kind, 1)),
        "сосед заражён первой ступенью"
    );
    assert_eq!(sim.blight_at(a), Some((kind, 2)), "а сам остался на месте");
}

/// Соседство симметрично: рулсет пишет связь один раз, ядро разворачивает её в
/// обе стороны. Иначе очаг полз бы только в одну сторону — молчаливый баг.
#[test]
fn links_are_symmetric() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 1, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]); // связь названа только здесь
    sim.seed_blight(b, kind);

    sim.tick_n(10);
    assert_eq!(
        sim.blight_at(a),
        Some((kind, 1)),
        "пополз в обратную сторону"
    );
}

/// Расползание идёт по шагу за раз: цепочки за один тик не бывает, у посеянного
/// очага возраст ноль.
#[test]
fn spreading_takes_one_step_at_a_time() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 1, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    let c = sim.set_site("Дорога", (2, 0), &[b]);
    sim.seed_blight(a, kind);

    sim.tick_n(10);
    assert_eq!(sim.blight_at(b), Some((kind, 1)));
    assert_eq!(
        sim.blight_at(c),
        None,
        "через одного за тик не перепрыгнуло"
    );

    sim.tick_n(10);
    assert_eq!(sim.blight_at(c), Some((kind, 1)), "дошло на следующем шаге");
}

/// Некуда — счётчик стоит на пороге и тронется, как только сосед освободится:
/// ждать полный круг заново очаг не обязан, он всё это время был готов.
#[test]
fn a_blocked_blight_waits_at_the_threshold() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 1, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.seed_blight(a, kind);
    sim.seed_blight(b, kind);

    sim.tick_n(50);
    assert_eq!(sim.site_step(a), Some((Step::Held, 0)), "идти некуда");

    // Соседа зачистили — очаг трогается ближайшим тиком, а не через 10.
    sim.world.resource_mut::<Blights>().clear(b);
    sim.tick_n(1);
    assert_eq!(
        sim.blight_at(b),
        Some((kind, 1)),
        "занял освободившееся место"
    );
}

/// На занятый участок очаг не садится: два очага на одном месте — это второе
/// состояние там, где игрок видит один кружок.
#[test]
fn a_blight_never_lands_on_an_occupied_site() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let weed = sim.set_blight_kind(10, 1, true, 0);
    let rust = sim.set_blight_kind(0, 1, false, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.seed_blight(a, weed);
    sim.seed_blight(b, rust);

    sim.tick_n(30);
    assert_eq!(sim.blight_at(b), Some((rust, 1)), "чужой очаг не подменён");
}

// --- прогноз ---------------------------------------------------------------

/// Прогноз считает **ядро**, и он обязан совпадать с тем, что случится: это
/// то же выражение, каким шагает сама система (инвариант 14).
#[test]
fn the_forecast_tells_what_will_actually_happen() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 2, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.seed_blight(a, kind);

    assert_eq!(
        sim.site_step(a),
        Some((Step::Grow(2), 10)),
        "сперва ступень"
    );
    sim.tick_n(4);
    assert_eq!(sim.site_step(a), Some((Step::Grow(2), 6)), "срок тикает");

    sim.tick_n(6);
    assert_eq!(
        sim.site_step(a),
        Some((Step::Spread(b), 10)),
        "на пределе — и названо, куда именно"
    );
}

#[test]
fn a_clean_site_has_no_forecast() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    assert_eq!(sim.site_step(site), None, "очага нет — и шага нет");
}

// --- цена бездействия ------------------------------------------------------

/// Заражённый участок закрывает свои заказы. Это вся цена бездействия: игрок
/// теряет возможности, а не котов.
#[test]
fn a_blighted_site_closes_its_orders() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_site(m, site);
    assert!(sim.raid_gates(m).reachable, "пока чисто — заказ открыт");

    sim.seed_blight(site, kind);
    assert!(!sim.raid_gates(m).reachable, "заражено — заказ закрыт");
    assert!(!sim.launch(m, squad(&["a", "b"])), "и заявка отклонена");
    // С §12.242 заказ под очагом из колонки штаба прячется — его место на
    // карте, где стоит зачистка, — и лента говорит то же, что экран (§12.324).
    assert!(!sim.raid_is_open(m), "и в штабе его нет");
}

/// Заказ без места не закрывается никогда: вылазка за своим идёт за котом, а не
/// в точку, и запирать её картой значило бы сделать плен необратимым (§12.40).
#[test]
fn a_placeless_order_is_never_closed_by_the_map() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.seed_blight(site, kind);

    assert!(
        sim.raid_gates(m).reachable,
        "места у заказа нет — и ворот нет"
    );
    assert!(sim.launch(m, squad(&["a", "b"])), "заявка принята");
}

// --- разведка (§12.320) ----------------------------------------------------

/// Участок разведан, когда на нём открылся заказ; участок без заказов — нет.
#[test]
fn a_site_is_found_by_its_first_open_order() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let dump = sim.set_site("Свалка", (0, 0), &[]);
    let empty = sim.set_site("Шахты", (1, 0), &[dump]);
    sim.set_mission_site(m, dump);

    sim.tick_n(1);
    assert!(sim.site_met(dump), "заказ открыт — участок разведан");
    assert!(!sim.site_met(empty), "заказов нет — и знать о нём нечего");
}

/// Заказ, до которого база не доросла, участок не открывает: на карте он
/// назвал бы место раньше, чем там появилось дело.
#[test]
fn a_locked_order_does_not_reveal_its_site() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let dump = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_site(m, dump);
    sim.set_mission_fame(m, 0, 20);

    sim.tick_n(1);
    assert!(!sim.site_met(dump), "известности мало — участок в тумане");
    sim.set_fame(20);
    sim.tick_n(1);
    assert!(sim.site_met(dump), "доросли — разведан");
}

/// Очаг открывает и свой участок, и соседей — туда он поползёт, и прогноз
/// называет их по имени. Дальше соседей туман стоит.
#[test]
fn a_blight_reveals_its_site_and_neighbours() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let a = sim.set_site("А", (0, 0), &[]);
    let b = sim.set_site("Б", (1, 0), &[a]);
    let c = sim.set_site("В", (2, 0), &[b]);
    sim.seed_blight(a, kind);

    sim.tick_n(1);
    assert!(sim.site_met(a), "горящий участок разведан");
    assert!(sim.site_met(b), "и сосед, куда очаг поползёт");
    assert!(!sim.site_met(c), "а сосед соседа — нет");
}

/// Разведанное не забывается: очаг снят, заказов на участке нет, а участок
/// остаётся на карте с именем.
#[test]
fn a_found_site_stays_found() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 2, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);
    sim.tick_n(1);
    assert!(sim.site_met(site));

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(sim.blight_at(site), None, "очаг снят");
    assert!(sim.site_met(site), "а участок по-прежнему разведан");
}

// --- зачистка --------------------------------------------------------------

/// Заказ на зачистку открыт там, где очаг, и нигде больше.
#[test]
fn a_cleanup_order_lives_where_the_blight_is() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);

    assert!(!sim.raid_gates(m).possible, "очага нет — цели нет");
    assert!(!sim.raid_is_open(m), "и заказа в списке нет вовсе");
    assert!(sim.raid_gates(m).targets.is_empty());

    sim.seed_blight(site, kind);
    assert!(sim.raid_gates(m).possible, "очаг есть — есть и цель");
    assert!(sim.raid_is_open(m), "и заказ появился в списке");
    assert_eq!(
        sim.raid_gates(m).targets,
        vec![site],
        "и названо, куда идти"
    );
}

#[test]
fn a_successful_cleanup_clears_the_blight() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 2, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    assert_eq!(sim.mission_site(), Some(site), "цель заморожена в заявке");
    sim.tick_n(30);
    assert_eq!(sim.blight_at(site), None, "очаг снят");
}

/// Очаг гаснет **на конце работы, а не на возвращении** (§12.244): отряд,
/// который идёт домой, дело уже сделал, и гореть участку всю дорогу назад
/// незачем. Добыча при этом по-прежнему ложится на шлюз по приходу.
#[test]
fn a_cleanup_clears_the_blight_when_the_work_ends_not_at_the_gate() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    sim.set_mission_work(m, 20);
    let kind = sim.set_blight_kind(0, 2, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    // Дождаться ухода и пройти дорогу туда и работу — до первого тика «домой».
    let mut guard = 0;
    while sim.mission_phase() != Some("back") {
        assert_eq!(
            sim.blight_at(site),
            Some((kind, 1)),
            "пока работают — горит"
        );
        sim.tick_n(1);
        guard += 1;
        assert!(guard < 200, "стадия «домой» так и не настала");
    }
    assert_eq!(
        sim.blight_at(site),
        None,
        "снят в первый же тик дороги домой"
    );
    assert!(
        sim.raid_left(m).is_some_and(|l| l > 0),
        "а отряд ещё в поле"
    );
    assert_eq!(sim.item_at(3, 1, 0), 0, "добычи на шлюзе пока нет");
    let left = sim.raid_left(m).unwrap() as usize;
    sim.tick_n(left);
    assert_eq!(sim.raid_left(m), None, "вернулись");
    assert_eq!(sim.item_at(3, 1, 0), 5, "и только теперь добыча на шлюзе");
}

/// Провал не снимает ничего: доля меряет добычу, а половины очага не бывает.
#[test]
fn a_failed_cleanup_leaves_the_blight_alone() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    // Заведомо провальная: сложность вдвое выше силы двух безоружных котов.
    sim.set_risky_mission(2, 10, 0, 0, &[]);
    if let Some(rule) = sim.world.resource_mut::<MissionRules>().0.get_mut(m) {
        rule.danger = 40;
    }
    sim.seed_blight(site, kind);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "очаг на месте");
}

/// Неполный успех сбивает ступень: отряд отработал, но не дотянул. Отсюда и
/// вся драматургия §12.206 — отлежаться и сходить снова, если сбиваешь быстрее,
/// чем очаг растёт обратно.
#[test]
fn a_partial_cleanup_knocks_one_stage_off() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    // Ступень тяжелее отряда, но не вдвое: доля неполная, провала нет.
    let kind = sim.set_blight_kind(0, 5, false, 1);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);
    if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(site)
        && let Some(b) = slot.as_mut()
    {
        b.stage = 4;
    }

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(
        sim.blight_at(site),
        Some((kind, 3)),
        "ступень сбита на одну"
    );
}

/// Свежее пятно неполный успех не снимает: очистка — это полный успех, иначе
/// «справился» и «почти справился» на первой ступени неразличимы, и состав
/// отряда там не значит ничего.
#[test]
fn a_partial_cleanup_never_clears_the_first_stage() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    // Сложность первой ступени выше силы двойки, но не вдвое: доля неполная.
    let kind = sim.set_blight_kind(0, 5, false, 3);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "пятно на месте");
}

/// Сбитая ступень **покупает время**: счётчик роста обнуляется, иначе очаг
/// вернул бы её тем же тиком и работа отряда не значила бы ничего.
#[test]
fn a_knocked_blight_starts_its_clock_over() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(40, 5, false, 1);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);
    if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(site)
        && let Some(b) = slot.as_mut()
    {
        b.stage = 4;
    }

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(sim.blight_at(site), Some((kind, 3)), "ступень сбита");
    // Счёт до следующей ступени пошёл заново — с конца работы отряда
    // (§12.244), а не с посева: это и есть купленное базе время.
    let (step, left) = sim.site_step(site).expect("очаг растёт дальше");
    assert_eq!(step, Step::Grow(4), "следующим шагом вернёт сбитое");
    // Считай очаг от посева — осталось бы десять (сорок минус тридцать
    // прожитых); он считает от конца работы, и потому заметно больше.
    assert!(left > 15, "счётчик пошёл заново: осталось {left}");
}

/// Провал зачистки пленных не оставляет: очаг — биологическая угроза от
/// природы, у неё нет ни воли, ни лагеря, и брать в плен там некому. Цена
/// провала прежняя — добыча и здоровье, — но кот возвращается всегда.
#[test]
fn a_failed_cleanup_takes_no_captives() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    // Вылазка за своим есть и открыта — то есть плен был бы возможен.
    sim.set_rescue_mission(1, 5, 0);
    if let Some(rule) = sim.world.resource_mut::<MissionRules>().0.get_mut(m) {
        rule.danger = 40;
    }
    sim.seed_blight(site, kind);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    sim.tick_n(30);
    assert_eq!(
        sim.blight_at(site),
        Some((kind, 1)),
        "провал, очаг на месте"
    );
    assert!(!sim.is_captive("a") && !sim.is_captive("b"), "оба дома");
    assert!(!sim.is_away("a") && !sim.is_away("b"), "и оба на базе");
}

/// Породу сверяем на месте: пока отряд шёл, очаг мог зачистить сосед, а на
/// освободившийся участок сесть что-то другое. Снять чужой очаг «за компанию»
/// значило бы отдать игроку работу, которой он не делал.
#[test]
fn a_cleanup_does_not_wipe_a_different_blight() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let weed = sim.set_blight_kind(0, 1, false, 0);
    let rust = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, weed);
    sim.seed_blight(site, weed);

    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    // Пока отряд в поле, очаг подменили другой породой.
    sim.tick_n(12);
    sim.world.resource_mut::<Blights>().clear(site);
    sim.seed_blight(site, rust);

    sim.tick_n(20);
    assert_eq!(sim.blight_at(site), Some((rust, 1)), "чужой очаг не тронут");
}

// --- сложность -------------------------------------------------------------

/// Ступень делает зачистку тяжелее — тем же `outcome`, каким считается любой
/// исход: ходить рано дёшево, поздно дорого. В этом весь выбор игрока.
#[test]
fn a_riper_blight_is_harder_to_clear() {
    let clear_at = |stage: i32| {
        let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
        // Растить некогда: ступень выставляем прямо, чтобы мерить только её.
        let kind = sim.set_blight_kind(0, 5, false, 1);
        let site = sim.set_site("Свалка", (0, 0), &[]);
        sim.set_mission_cleanses(m, kind);
        sim.seed_blight(site, kind);
        if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(site)
            && let Some(b) = slot.as_mut()
        {
            b.stage = stage;
        }
        sim.launch_to(m, squad(&["a", "b"]), site);
        sim.tick_n(30);
        sim.blight_at(site).is_none()
    };

    assert!(clear_at(1), "свежий очаг двое снимают");
    assert!(!clear_at(5), "запущенный им уже не по зубам");
}

// --- цель ------------------------------------------------------------------

/// Не назвал участок — заказ идёт на самый запущенный очаг: это и есть
/// «держи район чистым» у правила автовылазки.
#[test]
fn an_unnamed_cleanup_goes_to_the_ripest_blight() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 3, false, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(a, kind);
    sim.seed_blight(b, kind);
    if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(b)
        && let Some(blight) = slot.as_mut()
    {
        blight.stage = 3;
    }

    assert_eq!(sim.raid_aim(m), Some(b), "цель — самый запущенный");
    assert_eq!(
        sim.raid_gates(m).targets,
        vec![b, a],
        "и список в том же порядке"
    );
}

/// Обычный заказ идёт к себе домой, участка называть нечего.
#[test]
fn a_placed_order_aims_at_its_own_site() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_site(m, site);
    assert_eq!(sim.raid_aim(m), Some(site));
    assert!(sim.raid_gates(m).targets.is_empty(), "целей у него нет");
}

/// Очаг зачистили, пока отряд собирался, — заявка отклоняется: заказ на
/// зачистку без цели это заказ, которого нет.
#[test]
fn a_cleanup_without_a_target_is_refused() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);

    assert!(!sim.launch(m, squad(&["a", "b"])), "идти некуда");
    assert!(
        !sim.launch_to(m, squad(&["a", "b"]), site),
        "и на чистый тоже"
    );
}

// --- посев расписанием -----------------------------------------------------

/// Расписание называет дату, карта — адрес (§12.198): вместе они делают записку
/// картой путей, а не списком дат.
#[test]
fn a_timeline_event_seeds_the_map() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    let event = sim.set_event(5, &[], &[], 0, 0);
    if let Some(rule) = sim.world.resource_mut::<TimelineRules>().0.get_mut(event) {
        rule.seeds = vec![(site, kind)];
    }

    sim.tick_n(4);
    assert_eq!(sim.blight_at(site), None, "срок не вышел");
    sim.tick_n(1);
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "очаг сел по адресу");
}

/// Посев не зависит от готовности базы: очаг — событие мира, а не штраф за
/// неуспетую технологию.
#[test]
fn seeding_ignores_whether_the_base_was_ready() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(0, 1, false, 0);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    let event = sim.set_event(5, &["нечто"], &[], 0, 0);
    if let Some(rule) = sim.world.resource_mut::<TimelineRules>().0.get_mut(event) {
        rule.seeds = vec![(site, kind)];
    }

    sim.tick_n(6);
    assert_eq!(
        sim.happened(event),
        Some(false),
        "событие прошло, база не успела"
    );
    assert_eq!(sim.blight_at(site), Some((kind, 1)), "очаг всё равно сел");
}

// --- сторожа на боевом рулсете ---------------------------------------------
//
// Разбираем **сам YAML**, а не мир `Sim::new`, по доводу §12.134: конструктор
// схлопывает пропавшие `id` через `filter_map`, то есть глотает как раз те
// опечатки, которые сторож и ловит. Опечатка в `site:` не падает, а тихо делает
// заказ безместным — то есть навсегда открытым, — и заметить это можно только
// по тому, что цена бездействия куда-то делась.

use crate::ruleset::Ruleset;

fn shipped() -> Ruleset {
    serde_yaml::from_str(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет читается")
}

/// Каждое имя участка и породы, названное где угодно, существует.
#[test]
fn the_shipped_ruleset_names_only_sites_that_exist() {
    let rs = shipped();
    let sites: Vec<&str> = rs.sites.iter().map(|s| s.id.as_str()).collect();
    let kinds: Vec<&str> = rs.blights.iter().map(|b| b.id.as_str()).collect();

    for site in &rs.sites {
        for link in &site.links {
            assert!(
                sites.contains(&link.as_str()),
                "участок «{}» ссылается на несуществующего соседа «{link}»",
                site.id,
            );
        }
        assert!(
            site.blight.is_empty() || kinds.contains(&site.blight.as_str()),
            "стартовый посев на «{}» называет неизвестную породу «{}»",
            site.id,
            site.blight,
        );
    }
    for m in &rs.missions {
        assert!(
            m.site.is_empty() || sites.contains(&m.site.as_str()),
            "заказ «{}» стоит на несуществующем участке «{}»",
            m.id,
            m.site,
        );
        assert!(
            m.cleanses.is_empty() || kinds.contains(&m.cleanses.as_str()),
            "заказ «{}» чистит неизвестную породу «{}»",
            m.id,
            m.cleanses,
        );
    }
    for e in &rs.timeline {
        for (site, kind) in &e.seeds {
            assert!(
                sites.contains(&site.as_str()),
                "событие «{}» сеет на несуществующий участок «{site}»",
                e.id,
            );
            assert!(
                kinds.contains(&kind.as_str()),
                "событие «{}» сеет неизвестную породу «{kind}»",
                e.id,
            );
        }
    }
}

/// У каждой породы очага есть, чем её снять, и снятие **ничем не закрыто**.
///
/// Очаг умеет только расти, а известность и репутация — ворота, которые база
/// может ещё не пройти или уже потерять (§12.43). Заказ на зачистку за такими
/// воротами означал бы пожар, который нечем тушить, — то есть необратимую
/// потерю территории, а необратимости в игре нет (§12.5, §12.40).
#[test]
fn the_shipped_ruleset_can_answer_its_own_blight() {
    let rs = shipped();
    for kind in &rs.blights {
        let cure = rs
            .missions
            .iter()
            .find(|m| m.cleanses == kind.id)
            .unwrap_or_else(|| panic!("породу «{}» нечем зачистить", kind.id));
        assert_eq!(
            cure.requires, 0,
            "зачистка «{}» закрыта известностью",
            kind.id
        );
        assert!(
            cure.needs.is_empty(),
            "зачистка «{}» закрыта репутацией — рассорившаяся база теряла бы \
             территорию навсегда",
            kind.id,
        );
        assert!(
            cure.site.is_empty(),
            "у зачистки «{}» есть своё место: она обязана ходить по очагам, \
             а не стоять в точке",
            kind.id,
        );
    }
}

/// Расползающемуся очагу есть куда ползти, а участок, где он садится, ведёт к
/// заказам: иначе цены бездействия нет вовсе.
///
/// Обход в ширину, а не «есть ли сосед»: очаг, запертый в тупике из двух пустых
/// участков, растёт три ступени и не отнимает у базы ничего — мёртвый контент,
/// ровно как правило, которое не срабатывает никогда (§12.157).
#[test]
fn the_shipped_ruleset_lets_a_blight_threaten_something() {
    let rs = shipped();
    let index = |id: &str| rs.sites.iter().position(|s| s.id == id);
    // Связь симметрична (§12.198) — разворачиваем так же, как `Sim::new`.
    let mut links: Vec<Vec<usize>> = rs
        .sites
        .iter()
        .map(|s| s.links.iter().filter_map(|l| index(l)).collect())
        .collect();
    for from in 0..links.len() {
        for i in 0..links[from].len() {
            let to = links[from][i];
            if !links[to].contains(&from) {
                links[to].push(from);
            }
        }
    }
    let earns: Vec<bool> = rs
        .sites
        .iter()
        .map(|s| rs.missions.iter().any(|m| m.site == s.id))
        .collect();

    let seeds = rs.timeline.iter().flat_map(|e| e.seeds.iter()).chain(
        rs.sites
            .iter()
            .filter_map(|s| (!s.blight.is_empty()).then_some((&s.id, &s.blight))),
    );
    let mut any = false;
    for (site, kind) in seeds {
        any = true;
        let spreads = rs
            .blights
            .iter()
            .find(|b| &b.id == kind)
            .is_some_and(|b| b.spreads);
        assert!(spreads, "посеянная порода «{kind}» никуда не ползёт");
        let from = index(site).expect("участок посева");
        // Дотягивается ли очаг до места, которое базе чем-то дорого.
        let mut seen = vec![false; rs.sites.len()];
        let mut queue = vec![from];
        seen[from] = true;
        let mut threatens = earns[from];
        while let Some(at) = queue.pop() {
            for &next in &links[at] {
                if !seen[next] {
                    seen[next] = true;
                    threatens |= earns[next];
                    queue.push(next);
                }
            }
        }
        assert!(
            threatens,
            "очаг на «{site}» не дотягивается ни до одного заказа — расти ему \
             некуда и отнимать нечего",
        );
    }
    assert!(
        any,
        "очаг не сеется вообще ниоткуда: механика выключена молча"
    );
}

// --- прогноз по каждой цели ------------------------------------------------

/// У заказа на зачистку целей несколько, а сложность у них разная — она растёт
/// со ступенью. Один прогноз на карточку обещал бы исход самого запущенного
/// очага и на кнопке свежего: игрок читал бы «провал» там, где отряд справится.
#[test]
fn every_target_gets_its_own_forecast() {
    let mut sim = sim_from(&["#######", "#a...b#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(3, 1, 1);
    let m = sim.set_risky_mission(2, 10, 1, 0, &[(0, 5)]);
    // Ступень стоит по единице: двое безоружных котов (сила 2) свежий очаг
    // снимают, а запущенный им уже не по зубам — ровно та развилка, ради
    // которой прогноз и считается по каждой цели.
    let kind = sim.set_blight_kind(0, 3, false, 1);
    let fresh = sim.set_site("Свалка", (0, 0), &[]);
    let ripe = sim.set_site("Шахты", (1, 0), &[fresh]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(fresh, kind);
    sim.seed_blight(ripe, kind);
    if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(ripe)
        && let Some(b) = slot.as_mut()
    {
        b.stage = 3;
    }
    sim.enlist("a", 3, 1);
    sim.enlist("b", 3, 1);

    let at = (3, 1);
    let (soft, ..) = sim.aim_forecast(at, m, fresh).expect("цель — свежий очаг");
    let (hard, ..) = sim.aim_forecast(at, m, ripe).expect("цель — запущенный");
    assert!(
        soft < hard,
        "свежий очаг легче запущенного: {soft} против {hard}",
    );
    // И это тот же исход, какой посчитается на возвращении (инвариант 14).
    assert!(sim.launch_to(m, squad(&["a", "b"]), fresh));
    sim.tick_n(30);
    assert_eq!(sim.blight_at(fresh), None, "свежий очаг сняли");
    assert_eq!(sim.blight_at(ripe), Some((kind, 3)), "запущенный на месте");
}

/// **Свежий очаг обязан быть по зубам стартовой базе.**
///
/// Сторож про то же, про что `the_shipped_ruleset_has_a_reachable_ladder` у
/// лестницы вылазок, только с другого конца: очаг закрывает заказы, то есть
/// отнимает доход, а сила отряда растёт **от вылазок**. Заказ на зачистку,
/// непосильный с первого дня, превращает первый же разлив в спираль — база
/// теряет заказы, а вернуть их нечем, — и это была бы первая необратимость в
/// игре, которой §12.5 и §12.40 не предусмотрели.
///
/// Мерим **первую ступень**: дальше цена растёт намеренно, и «поздно — дорого»
/// это и есть решение, которое §12.198 просит принять.
#[test]
fn the_shipped_ruleset_lets_the_first_squad_answer_a_fresh_blight() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.add_squad(); // стартовый отряд до §12.349
    // Связь глушим по тому же доводу, что и в лестнице вылазок: дежурный у
    // рации прибавляет силы, а тут меряется сам заказ (§12.60).
    sim.without_comms();
    let rs = shipped();
    let kind = rs
        .blights
        .iter()
        .position(|b| !b.id.is_empty())
        .expect("порода очага");
    let purge = rs
        .missions
        .iter()
        .position(|m| !m.cleanses.is_empty())
        .expect("заказ на зачистку");
    let site = rs
        .sites
        .iter()
        .position(|s| rs.missions.iter().any(|m| m.site == s.id))
        .expect("участок с заказом");

    // Стартовый отряд целиком — тот, что у базы есть в первый день.
    let gate = sim
        .node_cells_of()
        .first()
        .copied()
        .expect("рация стартовой застройки");
    let crew: Vec<String> = sim.unit_ids();
    for id in &crew {
        sim.enlist(id, gate.0, gate.1);
    }
    sim.seed_blight(site, kind);

    let (danger, share, failed) = sim
        .aim_forecast(gate, purge, site)
        .expect("прогноз по свежему очагу");
    assert!(
        !failed,
        "стартовый отряд ({} котов) не тушит свежий очаг: сложность {danger}, доля {share} %",
        crew.len(),
    );
}

// --- дальность на карте (§12.292) -----------------------------------------

/// Карта штаба не врёт про дальность (§12.292): путь от базы до участка — до
/// `at` и дальше вдоль `road` — в масштабе `pace` сходится с дорогой участка в
/// одну сторону. Ядро координат не читает, поэтому разойтись им ничто не
/// мешает, кроме этого сторожа: сдвинутый кружок молча сделал бы «Свалку»
/// на экране дальше «Логова», а значок отряда шёл бы к ней медленнее.
///
/// Разбирает **сам YAML**, а не мир `Sim::new`, по доводу сторожа дерева
/// (§12.134): конструктор дорогу с участка на заказ уже перенёс, и
/// продублированная в заказе дорога в нём не видна.
#[test]
fn the_shipped_ruleset_draws_roads_to_scale() {
    let rs: crate::ruleset::Ruleset =
        serde_yaml::from_str(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let pace = rs.outskirts.pace;
    assert!(pace > 0, "у карты нет масштаба — сторожить нечего");
    let home = rs.outskirts.home;
    let dist = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();

    for s in rs.sites.iter().filter(|s| s.travel > 0) {
        let mut len = dist(home, s.at);
        let mut from = s.at;
        for &p in &s.road {
            len += dist(from, p);
            from = p;
        }
        let drawn = len * pace as f32;
        let road = s.travel as f32 / 2.0;
        assert!(
            (drawn - road).abs() <= pace as f32 * 0.1,
            "«{}»: на карте {drawn:.0} тиков в одну сторону, а дорога {road:.0}",
            s.label
        );
    }
    for m in rs.missions.iter().filter(|m| !m.site.is_empty()) {
        let site = rs
            .sites
            .iter()
            .find(|s| s.id == m.site)
            .expect("участок заказа");
        assert!(
            site.travel > 0,
            "у «{}» есть заказ «{}», а дороги до него нет",
            site.label,
            m.label
        );
        assert_eq!(
            m.travel, 0,
            "«{}» пишет свою дорогу, а её пишет участок «{}»",
            m.label, site.label
        );
    }
}

/// Дорога заказа на участке приходит с участка (§12.292) — и в правила, и в
/// палитру `meta`, которую читает вид: разные числа там значили бы срок в
/// штабе, не совпадающий с тем, что отряд проходит.
#[test]
fn the_shipped_ruleset_takes_mission_roads_from_sites() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let rules = sim.world.resource::<MissionRules>();
    for (i, m) in sim.missions.iter().enumerate() {
        if let Some(site) = sim.sites.iter().find(|s| s.id == m.site) {
            assert_eq!(m.travel, site.travel, "палитра: «{}»", m.label);
            assert_eq!(rules.0[i].travel, site.travel, "правила: «{}»", m.label);
        }
    }
}

// --- два отряда на зачистке (§12.315) -----------------------------------------

/// Мир с двумя слотами и двумя горящими участками одной породы.
fn two_squads_two_blights() -> (Sim, usize, usize, usize) {
    let mut sim = sim_from(&["#######", "#a.b.c#", "#.....#", "#######"]);
    sim.set_gate(1, true);
    sim.force_tile(1, 2, 1);
    sim.force_tile(2, 2, 1);
    let m = sim.set_mission(1, 30, &[(0, 5)]);
    let kind = sim.set_blight_kind(0, 2, false, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(a, kind);
    sim.seed_blight(b, kind);
    (sim, m, a, b)
}

/// Зачистка — один заказ с целью на каждый очаг (§12.198), и занимает
/// отряд **очаг**, а не весь заказ: второй отряд уходит в другой очаг.
#[test]
fn two_squads_clean_two_blights_at_once() {
    let (mut sim, m, a, b) = two_squads_two_blights();
    assert!(sim.launch_to(m, squad(&["a"]), a), "первый — в первый очаг");
    assert!(sim.launch_to(m, squad(&["b"]), b), "второй — во второй");
    assert_eq!(sim.raid_count(), 2);
    sim.tick_n(60);
    assert_eq!(sim.blight_at(a), None, "оба очага сняты");
    assert_eq!(sim.blight_at(b), None);
}

/// А в один очаг двоих не шлют: ключ — пара (заказ, очаг).
#[test]
fn two_squads_never_go_to_one_blight() {
    let (mut sim, m, a, _) = two_squads_two_blights();
    assert!(sim.launch_to(m, squad(&["a"]), a));
    assert!(!sim.launch_to(m, squad(&["b"]), a), "этот очаг уже жгут");
    assert_eq!(sim.raid_count(), 1);
}

/// Без названной цели (так ходит правило автовылазки) второй отряд берёт
/// очаг, куда ещё никто не идёт, а не упирается в занятый.
#[test]
fn an_unnamed_cleanup_skips_a_blight_already_taken() {
    let (mut sim, m, a, b) = two_squads_two_blights();
    assert!(sim.launch(m, squad(&["a"])));
    assert!(sim.launch(m, squad(&["b"])), "второй ушёл в свободный очаг");
    sim.tick_n(60);
    assert_eq!(sim.blight_at(a), None);
    assert_eq!(sim.blight_at(b), None);
}

/// На старте боевой партии разведаны только участки с открытыми заказами:
/// шахты и отстойник заказов не имеют вовсе, теплицы ждут технологии (§12.320),
/// а свалка — стола брифинга (§12.349): до него наружу ходят только
/// «Пошарить рядом».
#[test]
fn the_shipped_ruleset_starts_with_unexplored_sites() {
    let rs = shipped();
    let at = |id: &str| rs.sites.iter().position(|s| s.id == id).expect(id);
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    sim.without_timeline();
    for id in ["junkyard", "mines", "sump", "greenhouse"] {
        assert!(!sim.site_met(at(id)), "«{id}» на старте в тумане");
    }
    sim.add_squad();
    sim.tick_n(1);
    assert!(
        sim.site_met(at("junkyard")),
        "стол брифинга встал — свалка разведана"
    );
}

/// Разведка переживает снимок, а снимок старше неё добирается по миру.
#[test]
fn explored_sites_survive_a_save() {
    let core = include_str!("../../assets/rulesets/core.yaml");
    let rs = shipped();
    let mines = rs.sites.iter().position(|s| s.id == "mines").unwrap();
    let dump = rs.sites.iter().position(|s| s.id == "junkyard").unwrap();
    let mut live = Sim::new(core).expect("рулсет");
    // Свалку открывает стол брифинга (§12.349).
    live.add_squad();
    live.world.resource_mut::<SitesMet>().mark(mines);

    let json = live.save().expect("снимок");
    let loaded = Sim::load_from(core, &json).expect("загрузка");
    assert!(loaded.site_met(mines), "разведанное помнится");

    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old["state"].as_object_mut().unwrap().remove("sites_met");
    let old = Sim::load_from(core, &old.to_string()).expect("старый снимок грузится");
    assert!(
        old.site_met(dump),
        "участок с открытым заказом добран по миру"
    );
    assert!(!old.site_met(mines), "а историю старый снимок не знает");
}

// --- выжженная земля (§12.323) ----------------------------------------------

/// Отправить зачистку на участок и дождаться конца работы — тика, когда очаг
/// сняли.
fn cleanse_until_back(sim: &mut Sim, m: usize, site: usize) {
    assert!(sim.launch_to(m, squad(&["a", "b"]), site));
    let mut guard = 0;
    while sim.mission_phase() != Some("back") {
        sim.tick_n(1);
        guard += 1;
        assert!(guard < 200, "стадия «домой» так и не настала");
    }
}

/// Сосед на пределе ждал на пороге и садился на очищенный участок тем же
/// тиком — полная зачистка на заражённой карте не значила ничего. Выжженный
/// участок держится круг роста той породы, что с него сняли.
#[test]
fn a_scorched_site_keeps_a_ripe_neighbour_out_for_one_growth() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    let kind = sim.set_blight_kind(30, 1, true, 0);
    let dump = sim.set_site("Свалка", (0, 0), &[]);
    let mines = sim.set_site("Шахты", (1, 0), &[dump]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(dump, kind);
    sim.seed_blight(mines, kind);
    // Шахтам ползти некуда — счётчик стоит на пороге.
    sim.tick_n(40);
    assert_eq!(sim.site_step(mines), Some((Step::Held, 0)));

    cleanse_until_back(&mut sim, m, dump);
    assert_eq!(sim.blight_at(dump), None, "выжгли целиком");
    let ward = sim.world.resource::<Blights>().ward_at(dump);
    assert!(ward > 0 && ward < 30, "земля выжжена на круг роста: {ward}");
    assert_eq!(
        sim.site_step(mines),
        Some((Step::Spread(dump), ward)),
        "прогноз называет и куда, и когда — когда земля остынет"
    );

    sim.tick_n(ward as usize - 1);
    assert_eq!(sim.blight_at(dump), None, "пока выжжено — чисто");
    sim.tick_n(1);
    assert_eq!(
        sim.blight_at(dump),
        Some((kind, 1)),
        "остыла — очаг вернулся тем же тиком"
    );
}

/// Выжженный сосед не держит очаг, если рядом есть свободный: тот берётся
/// первым, а выжженный — только когда остальные заняты.
#[test]
fn a_blight_prefers_open_ground_to_scorched_ground() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let kind = sim.set_blight_kind(10, 1, true, 0);
    let a = sim.set_site("Свалка", (0, 0), &[]);
    let b = sim.set_site("Шахты", (1, 0), &[a]);
    let c = sim.set_site("Отстойник", (2, 0), &[a]);
    sim.seed_blight(a, kind);
    sim.world.resource_mut::<Blights>().scorch(b, 100);

    assert_eq!(
        sim.site_step(a),
        Some((Step::Spread(c), 10)),
        "в обход выжженного"
    );
    sim.tick_n(10);
    assert_eq!(sim.blight_at(c), Some((kind, 1)));
    assert_eq!(sim.blight_at(b), None, "выжженный не тронут");
}

/// Неполная зачистка землю не выжигает: очаг там остался, выжигать нечего.
#[test]
fn a_partial_cleanup_scorches_nothing() {
    let (mut sim, m) = sim_with_gate(&["#######", "#a...b#", "#######"], (3, 1), 2, 10);
    // Ступень тяжелее отряда, но не вдвое — как в `a_partial_cleanup_knocks_one_stage_off`.
    let kind = sim.set_blight_kind(0, 5, false, 1);
    let site = sim.set_site("Свалка", (0, 0), &[]);
    sim.set_mission_cleanses(m, kind);
    sim.seed_blight(site, kind);
    if let Some(slot) = sim.world.resource_mut::<Blights>().0.get_mut(site)
        && let Some(b) = slot.as_mut()
    {
        b.stage = 4;
    }
    cleanse_until_back(&mut sim, m, site);
    assert_eq!(sim.blight_at(site), Some((kind, 3)), "ступень сбита");
    assert_eq!(sim.world.resource::<Blights>().ward_at(site), 0, "и только");
}

/// Выжженная земля переживает снимок, а снимок старше неё грузится без неё.
#[test]
fn scorched_ground_survives_a_save() {
    let core = include_str!("../../assets/rulesets/core.yaml");
    let mut live = Sim::new(core).expect("рулсет");
    live.world.resource_mut::<Blights>().scorch(2, 321);

    let json = live.save().expect("снимок");
    let loaded = Sim::load_from(core, &json).expect("загрузка");
    assert_eq!(loaded.world.resource::<Blights>().ward_at(2), 321);

    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old["state"].as_object_mut().unwrap().remove("wards");
    let old = Sim::load_from(core, &old.to_string()).expect("старый снимок грузится");
    assert_eq!(old.world.resource::<Blights>().ward_at(2), 0);
}
