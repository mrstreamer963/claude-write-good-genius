//! Усталость и отдых (§12.20).
//!
//! Усталости в схеме по умолчанию нет — её включает сам тест (`set_needs`), как
//! цену тайла в тестах материала. Лежанка делается вручную: тайлу `1` задаётся
//! скорость восстановления (`set_rest`), и нужная клетка переводится в него
//! через `force_tile`.

use super::sim_from;
use crate::sim::Sim;

const CORRIDOR: [&str; 3] = ["#########", "#a......#", "#########"];

// --- обычная жизнь ---------------------------------------------------------

/// Базовый случай: уставший кот сам доходит до лежанки и спит на ней.
#[test]
fn a_tired_cat_walks_to_the_bed_and_sleeps() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 5);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 200);

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (7, 1), "дошёл до лежанки");
    assert!(sim.is_resting("a"), "и лёг спать");

    let before = sim.energy_of("a");
    sim.tick_n(10);
    assert!(sim.energy_of("a") > before, "во сне бодрость растёт");
}

/// Выспавшись, кот встаёт и возвращается к работе сам.
#[test]
fn a_rested_cat_goes_back_to_work() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.force_tile(1, 1, 1); // лежанка прямо под котом
    sim.set_needs(200, 150, 1);
    sim.set_energy("a", 100);
    sim.add_blueprint(3, 2, 0);

    sim.tick_n(2);
    assert!(sim.is_resting("a"), "сперва спать");
    assert!(!sim.has_assignment("a"), "чертёж подождёт");

    sim.tick_n(400);
    assert!(!sim.is_resting("a"), "выспался и встал");
    assert_eq!(sim.tile(3, 2), 0, "и доделал работу");
}

/// Отдых — первый в очереди работ: уставший кот идёт спать, а не строить.
#[test]
fn rest_outranks_every_other_work() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 5);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 100);
    sim.add_blueprint(1, 2, 0); // работа прямо у ног

    sim.tick_n(3);
    assert!(sim.is_resting("a"), "уставший кот ушёл спать");
    assert!(!sim.has_assignment("a"), "чертёж он не взял");
}

/// Но начатое дело усталость не срывает — то же правило, что у приказа игрока
/// (§12.15). Кот уйдёт спать, когда освободится.
#[test]
fn tiredness_does_not_abandon_a_started_job() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 5);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "работа взята на свежую голову");

    sim.set_energy("a", 50); // вымотался посреди дела
    sim.tick_n(3);
    assert!(sim.has_assignment("a"), "стройку не бросил");
    assert!(!sim.is_resting("a"), "и спать не ушёл");
}

// --- критический порог (§12.33) --------------------------------------------

/// Ниже критического порога кот бросает начатое и уходит спать сам: иначе
/// длинная работа гарантированно упирается в ноль по дороге.
#[test]
fn a_critical_cat_abandons_its_job() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 5);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_critical(50);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "работа взята на свежую голову");

    sim.set_energy("a", 40); // выдохся посреди дела
    sim.tick_n(2);
    assert!(!sim.has_assignment("a"), "стройку бросил");
    assert_eq!(sim.rest_spot_of("a"), Some((7, 1)), "и занял лежанку");

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (7, 1), "дошёл до неё на своих лапах");
    assert!(sim.energy_of("a") > 40, "и спит, а не валяется на полпути");
}

/// Брошенный по критическому порогу чертёж достаётся другому — как и брошенный
/// от истощения. Иначе площадка навсегда останется за ушедшим спать.
#[test]
fn a_critical_cat_frees_its_blueprint() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 5);
    sim.force_tile(4, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_critical(50);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "ближний кот взял чертёж");

    sim.set_energy("a", 40);
    sim.tick_n(3);
    assert!(sim.is_resting("a"), "и ушёл спать");
    assert!(sim.has_assignment("b"), "работу подхватил второй");
}

/// Срываем с работы только под лежанку: бросить дело и остаться стоять — это
/// ни сна, ни работы. Без свободного места кот доработает до нуля.
#[test]
fn a_critical_cat_without_a_bed_keeps_working() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_needs(1000, 900, 1); // лежанок в мире нет вовсе
    sim.set_critical(50);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "работа взята");

    sim.set_energy("a", 40);
    sim.tick_n(3);
    assert!(sim.has_assignment("a"), "бросать нечего ради чего");
    assert!(!sim.is_resting("a"), "и спать не ушёл");
}

/// Выключатель отменяет ровно второй порог: коты снова работают до нуля и
/// валятся где стоят. Это осознанный выбор игрока — гнать базу до упора.
#[test]
fn auto_rest_off_lets_the_cat_work_to_zero() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 5);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_critical(50);
    sim.set_auto_rest(false);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    sim.set_energy("a", 40);
    sim.tick_n(3);
    assert!(sim.has_assignment("a"), "работу не бросил");

    sim.set_energy("a", 0);
    sim.tick_n(1);
    assert!(sim.is_resting("a"), "упал только на нуле");
    assert!(!sim.has_assignment("a"), "и отпустил чертёж");

    // А вот переезд упавшего выключатель не отменяет: он про второй порог, а
    // не про сон. Спящему на полу коту всё равно, чего хотел игрок час назад.
    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (7, 1), "и всё же добрался до лежанки");
}

// --- истощение -------------------------------------------------------------

/// Лежанок нет — кот работает до нуля и валится там, где стоит, отпустив
/// задачу. Это цена базы без зоны отдыха, а не запрет работать.
#[test]
fn an_exhausted_cat_collapses_where_it_stands() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_needs(100, 90, 10); // лежанок в мире нет вовсе
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "работа взята");

    sim.set_energy("a", 0);
    sim.tick_n(1);
    assert!(sim.is_resting("a"), "на нуле кот засыпает");
    assert_eq!(sim.pos_of("a"), (1, 1), "прямо на месте");
    assert!(!sim.has_assignment("a"), "и отпускает чертёж");

    sim.tick_n(400);
    assert_eq!(sim.tile(1, 2), 0, "выспался и доделал");
}

/// Чертёж, отпущенный упавшим котом, достаётся другому — иначе площадка
/// осталась бы «занятой» спящим навсегда.
#[test]
fn a_collapsed_cat_frees_its_blueprint_for_others() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_needs(1000, 0, 1);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "ближний кот взял чертёж");

    sim.set_energy("a", 0);
    sim.tick_n(3);
    assert!(sim.is_resting("a"), "и свалился");
    assert!(sim.has_assignment("b"), "работу подхватил второй");
}

/// На лежанке кот восстанавливается быстрее, чем на голом полу.
#[test]
fn a_bed_restores_faster_than_bare_floor() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 10);
    sim.force_tile(1, 1, 1); // лежанка под 'a', под 'b' — обычный пол
    sim.set_needs(1000, 0, 2);
    sim.set_energy("a", 0);
    sim.set_energy("b", 0);

    sim.tick_n(10);
    assert!(sim.is_resting("a") && sim.is_resting("b"), "спят оба");
    assert!(
        sim.energy_of("a") > sim.energy_of("b"),
        "на лежанке бодрость возвращается быстрее"
    );
}

// --- лежанка как место ------------------------------------------------------

/// Лежанку занимает ровно один кот: второму уставшему места нет, и он работает
/// дальше. Иначе число лежанок ни на что не влияет и строить их незачем.
#[test]
fn a_bed_takes_only_one_cat() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 1); // спит долго — место занято надолго
    sim.force_tile(4, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 100);
    sim.set_energy("b", 100);

    sim.tick_n(20);
    let spots = [sim.rest_spot_of("a"), sim.rest_spot_of("b")];
    assert_eq!(
        spots.iter().filter(|s| **s == Some((4, 1))).count(),
        1,
        "лежанку занял ровно один кот"
    );
    assert!(
        sim.is_resting("a") != sim.is_resting("b"),
        "второй остался на ногах"
    );
}

/// Освободившаяся лежанка достаётся следующему: занятость снимается вместе
/// с задачей, отдельного «отпустить место» не требуется.
#[test]
fn a_freed_bed_goes_to_the_next_cat() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 100); // просыпается быстро
    sim.force_tile(4, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 100);
    sim.set_energy("b", 100);

    sim.tick_n(10);
    let first = if sim.rest_spot_of("a").is_some() {
        "a"
    } else {
        "b"
    };
    let second = if first == "a" { "b" } else { "a" };
    assert_eq!(
        sim.rest_spot_of(first),
        Some((4, 1)),
        "первый занял лежанку"
    );
    assert_eq!(sim.rest_spot_of(second), None, "второму места не досталось");

    let mut took_bed = false;
    for _ in 0..300 {
        sim.tick_n(1);
        if sim.rest_spot_of(second) == Some((4, 1)) {
            took_bed = true;
            break;
        }
    }
    assert!(took_bed, "как только место освободилось, лёг второй");
}

/// Двум котам — две лежанки, каждому своя.
#[test]
fn two_beds_take_two_cats() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 1);
    sim.force_tile(4, 1, 1);
    sim.force_tile(5, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 100);
    sim.set_energy("b", 100);

    sim.tick_n(20);
    assert!(sim.is_resting("a") && sim.is_resting("b"), "спят оба");
    assert_ne!(
        sim.rest_spot_of("a"),
        sim.rest_spot_of("b"),
        "каждый на своей лежанке"
    );
}

// --- переезд упавшего (§12.33) ---------------------------------------------

/// Упавший на пол перебирается на лежанку, как только она появляется: сон на
/// полу вшестеро медленнее, и до утра там кот лежал бы зря.
#[test]
fn a_collapsed_cat_moves_to_a_bed_when_one_appears() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 0);

    sim.tick_n(1);
    assert!(sim.is_resting("a"), "свалился где стоял");
    assert_eq!(sim.rest_spot_of("a"), None, "лежанки у него нет");

    sim.force_tile(7, 1, 1); // зону отдыха достроили, пока он спал
    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (7, 1), "перебрался на лежанку");
    assert_eq!(sim.rest_spot_of("a"), Some((7, 1)), "и занял её");
}

/// Освободившаяся лежанка достаётся и упавшему — тому самому случаю, ради
/// которого переезд и заведён: кот падает в двух шагах от занятого места.
#[test]
fn a_freed_bed_is_taken_by_the_cat_on_the_floor() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 100); // спит быстро — место освободится скоро
    sim.force_tile(4, 1, 1);
    sim.set_needs(1000, 900, 1);
    sim.set_energy("b", 100); // `b` уходит на единственную лежанку
    sim.tick_n(5);
    assert_eq!(sim.rest_spot_of("b"), Some((4, 1)), "лежанку занял второй");

    sim.set_energy("a", 0); // а первый свалился на голом полу
    sim.tick_n(1);
    assert!(sim.is_resting("a"), "спит на полу");
    assert_eq!(sim.rest_spot_of("a"), None, "места ему не досталось");

    let mut moved = false;
    for _ in 0..300 {
        sim.tick_n(1);
        if sim.rest_spot_of("a") == Some((4, 1)) {
            moved = true;
            break;
        }
    }
    assert!(moved, "как только место освободилось, упавший перебрался");
}

/// Почти выспавшийся не встаёт ради лежанки: порог переезда тот же, что и у
/// ухода спать, иначе кот тащился бы через полбазы за последними очками.
#[test]
fn an_almost_rested_cat_stays_where_it_fell() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.set_needs(1000, 50, 10); // на полу восстанавливается быстро
    sim.set_energy("a", 0);

    sim.tick_n(10);
    assert!(sim.energy_of("a") > 50, "поднялся выше порога усталости");

    sim.force_tile(7, 1, 1);
    sim.tick_n(10);
    assert_eq!(sim.pos_of("a"), (1, 1), "досыпает там, где упал");
}

/// Упавший прямо на лежанку с неё не уходит: место у него уже лучшее, какое
/// есть, — а занятым оно считается по позиции, а не по `Rest::spot`.
#[test]
fn a_cat_that_fell_on_a_bed_stays_on_it() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 1); // спит долго — успеем понаблюдать
    sim.force_tile(1, 1, 1); // лежанка под котом
    sim.force_tile(7, 1, 1); // и свободная в другом конце
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 0);

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (1, 1), "спит на той, где упал");
}

// --- границы механики ------------------------------------------------------

/// Приказ игрока будит: это осознанное действие, а не автоматика. Но усталость
/// никуда не делась — выполнив приказ, кот уходит спать снова.
#[test]
fn a_players_order_wakes_a_sleeping_cat() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 1);
    sim.force_tile(1, 1, 1); // лежанка под котом: уснёт на месте
    sim.set_needs(1000, 500, 1);
    sim.set_energy("a", 400);

    sim.tick_n(2);
    assert!(sim.is_resting("a"), "кот спит на лежанке");

    assert!(sim.set_target("a", 5, 1), "приказ принят");
    assert!(!sim.is_resting("a"), "и разбудил кота");
    sim.tick_n(6);
    assert!(sim.pos_of("a").0 > 1, "кот пошёл выполнять приказ");

    sim.tick_n(60);
    assert!(
        sim.is_resting("a"),
        "а потом снова лёг — он всё ещё уставший"
    );
}

/// Рулсет без усталости — коты не устают и не спят. На этом держатся все
/// остальные тесты: механика включается контентом, а не самим фактом кода.
#[test]
fn without_energy_rules_nobody_sleeps() {
    let mut sim = sim_from(&CORRIDOR);
    sim.add_blueprint(1, 2, 0);

    sim.tick_n(200);
    assert!(!sim.is_resting("a"), "спать некому — усталости нет");
    assert_eq!(sim.tile(1, 2), 0, "работа идёт как раньше");
}

// --- боевой рулсет ---------------------------------------------------------

/// На настоящем `core.yaml`: за смену кто-то из бригады доходит до лежанки сам,
/// а не падает от истощения. Ловит рассогласование кода и контента — лежанку
/// без прохода, забытый `energy`, порог выше потолка.
#[test]
fn the_shipped_ruleset_sends_the_crew_to_bed() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).ok().expect("рулсет должен разбираться");
    let crew = ["excellent", "sp2", "sp3"];

    // Пороги идут в правильном порядке: критический выше нуля (иначе второго
    // порога в контенте нет вовсе) и ниже обычного (иначе кот бросал бы работу
    // ровно тогда, когда и брать её перестал, — §12.33).
    let (tired, critical) = sim.thresholds();
    assert!(
        0 < critical && critical < tired,
        "критический порог живёт между нулём и усталостью: {critical} против {tired}",
    );

    let mut sleeper = None;
    for _ in 0..6000 {
        sim.tick_n(1);
        if let Some(c) = crew.iter().find(|c| sim.is_resting(c)) {
            sleeper = Some(*c);
            break;
        }
    }
    let sleeper = sleeper.expect("за смену кто-то отправился спать");
    assert!(
        sim.energy_of(sleeper) > 0,
        "ушёл сам, на своих лапах, а не свалился от истощения"
    );
}
