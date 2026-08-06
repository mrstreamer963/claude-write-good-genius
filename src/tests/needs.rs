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

/// Спящий кот с приказом: мир, где кот уснул на лежанке под собой и получил
/// приказ идти в (5, 1). `spare` — включено ли «Беречь себя».
fn sim_with_an_ordered_sleeper(spare: bool) -> Sim {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 1);
    sim.force_tile(1, 1, 1); // лежанка под котом: уснёт на месте
    sim.set_needs(1000, 500, 1);
    sim.set_auto_rest(spare);
    sim.set_energy("a", 400);

    sim.tick_n(2);
    assert!(sim.is_resting("a"), "кот спит на лежанке");
    assert!(sim.set_target("a", 5, 1), "приказ принят");
    sim
}

/// Пока включено «Беречь себя», приказ спящего не поднимает (§12.51): он висит
/// и ждёт, а маршрут прокладывает `retry_orders` тем же тиком, каким кот
/// проснулся сам.
#[test]
fn a_players_order_waits_for_the_sleeper() {
    let mut sim = sim_with_an_ordered_sleeper(true);

    sim.tick_n(20);
    assert!(sim.is_resting("a"), "спит дальше");
    assert_eq!(sim.pos_of("a"), (1, 1), "и с лежанки не вставал");

    // Выспался — и приказ подхватывается сам, без второго клика игрока.
    // Считаем тики: дойдя, кот снова окажется без дела и побредёт дремать на ту
    // же лежанку (§12.52), так что «дошёл» ловим в момент прихода.
    sim.set_energy("a", 999);
    sim.tick_n(2);
    assert!(!sim.is_resting("a"), "проснулся");
    assert_eq!(sim.job_of("a"), ("order", true), "и сам пошёл по приказу");

    sim.tick_n(8);
    assert_eq!(sim.pos_of("a"), (5, 1), "дошёл, куда было велено");
}

/// Выключенное «Беречь себя» — решение игрока не жалеть котов: приказ будит
/// сразу, как будил всегда. Но усталость никуда не делась — выполнив приказ,
/// кот уходит спать снова.
#[test]
fn a_players_order_wakes_a_sleeping_cat_when_self_care_is_off() {
    let mut sim = sim_with_an_ordered_sleeper(false);

    assert!(!sim.is_resting("a"), "приказ разбудил кота");
    sim.tick_n(6);
    assert!(sim.pos_of("a").0 > 1, "кот пошёл выполнять приказ");

    sim.tick_n(60);
    assert!(
        sim.is_resting("a"),
        "а потом снова лёг — он всё ещё уставший"
    );
}

// --- потолок сна и дремота (§12.52) ----------------------------------------

/// Мир с лежанкой под котом: ставка 10 за тик, потолок сна `ceiling`.
/// Бодрость 1000, порог усталости 300 — кот ложится сам и просыпается на
/// потолке места, а не на полной.
fn sim_with_a_capped_bed(ceiling: i32, energy: i32) -> Sim {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.set_wake(1, ceiling);
    sim.force_tile(1, 1, 1); // лежанка под котом
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", energy);
    sim
}

/// Сон кончается на потолке места, а не на полной бодрости: докуда высыпаются,
/// решает клетка (§12.52).
#[test]
fn sleep_ends_at_the_ceiling_of_the_place() {
    let mut sim = sim_with_a_capped_bed(500, 100);

    sim.tick_n(41); // 100 + 10 за тик — потолок берётся на сороковом
    assert!(!sim.is_resting("a"), "сон кончился");
    assert!(
        (500..600).contains(&sim.energy_of("a")),
        "и кончился он на потолке лежанки, а не на 1000: {}",
        sim.energy_of("a")
    );
}

/// Выше потолка кот дремлет: `Rest` снят, но бодрость идёт вверх, пока базе
/// нечем его занять. Полная бодрость поэтому достижима — но только в простой.
#[test]
fn a_dozing_cat_tops_up_to_full() {
    let mut sim = sim_with_a_capped_bed(500, 100);
    sim.tick_n(41);
    assert_eq!(sim.job_of("a"), ("nap", false), "панель говорит «дремлет»");

    // Ставка та же, что у сна, минус очко за прожитый тик: кот бодрствует,
    // просто лёжа.
    let before = sim.energy_of("a");
    sim.tick_n(10);
    assert_eq!(sim.energy_of("a") - before, 90, "девять очков за тик");

    // Добрал до полной — с точностью до очка: на потолке `tire` снимает своё, а
    // дремота возвращает следующим тиком, и бодрость колеблется на единицу.
    // Отдельного списка фильтров в `tire` ради этого не заводим (§12.52).
    sim.tick_n(100);
    assert!(
        sim.energy_of("a") >= 999,
        "добрал до полной: {}",
        sim.energy_of("a")
    );
}

/// Дремота не задача: раздатчик забирает кота с лежанки первым же чертежом —
/// иначе база вставала бы, пока все досыпают.
#[test]
fn a_dozing_cat_is_taken_by_work() {
    let mut sim = sim_with_a_capped_bed(500, 100);
    sim.tick_n(41);
    assert!(!sim.is_resting("a"), "дремлет");

    sim.add_blueprint(4, 2, 0); // работа появилась в пустоте рядом
    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "встал и пошёл строить");
}

/// И приказ игрока дремлющего поднимает сразу, при включённом «Беречь себя»:
/// §12.51 защищает сон, а дремота — не сон (§12.52).
#[test]
fn a_dozing_cat_obeys_an_order_at_once() {
    let mut sim = sim_with_a_capped_bed(500, 100);
    sim.tick_n(41);

    assert!(sim.set_target("a", 5, 1), "приказ принят");
    sim.tick_n(1);
    assert_eq!(sim.job_of("a"), ("order", true), "встал тем же тиком");

    // Ровно до прихода: дальше кот снова без дела и бредёт дремать обратно.
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (5, 1), "дошёл без второго клика");
}

/// Лежанку дремлющий не держит: пришедшему спать по нужде она достаётся, а
/// дремлющего сгоняет с клетки `spread_units` (§12.32, §12.52).
#[test]
fn a_dozing_cat_yields_the_bed() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(1, 1, 1); // единственная лежанка — под котом `a`
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 600); // выспался, дремлет
    sim.set_energy("b", 200); // а этому спать по-настоящему

    sim.tick_n(20);
    assert!(sim.is_resting("b"), "уставший спит");
    assert_eq!(sim.pos_of("b"), (1, 1), "и спит он на лежанке");
    assert_ne!(sim.pos_of("a"), (1, 1), "дремавший уступил место");
}

/// На полу свой потолок, и он ниже порога усталости: там кот только
/// отлёживается, чтобы доползти до кровати, — выспаться можно лишь в ней.
#[test]
fn floor_sleep_stops_at_its_own_ceiling() {
    let mut sim = sim_from(&CORRIDOR); // лежанок нет вовсе
    sim.set_needs(1000, 300, 2);
    sim.set_floor_wake(200);
    sim.set_energy("a", 0);

    sim.tick_n(101);
    assert!(!sim.is_resting("a"), "отлежался и встал");
    assert!(
        sim.energy_of("a") < 300,
        "но выспаться на полу не вышло: {}",
        sim.energy_of("a")
    );
}

/// Коту нечем заняться — он идёт к свободной лежанке дремать (§12.52).
/// Спать он при этом не ложится: `Rest` ему никто не выдаёт.
#[test]
fn an_idle_cat_walks_to_a_free_bed() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(7, 1, 1); // лежанка в дальнем конце коридора
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800); // не устал, но и не полон

    sim.tick_n(14);
    assert_eq!(sim.pos_of("a"), (7, 1), "дошёл до лежанки сам");
    assert!(!sim.is_resting("a"), "но не спит — это дремота");
    assert_eq!(sim.job_of("a"), ("nap", false), "так и подписано");
    assert!(sim.energy_of("a") > 800, "и бодрость пошла вверх");
}

/// Идёт он **по шагу за раз**: кот с маршрутом невидим раздатчикам, а дорога в
/// спальню длинная — появившаяся работа не должна ждать, пока он дойдёт.
#[test]
fn work_interrupts_the_walk_to_the_bed() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800);

    sim.tick_n(4);
    let on_the_way = sim.pos_of("a");
    assert!(on_the_way.0 > 1 && on_the_way.0 < 7, "кот в пути к лежанке");

    sim.add_blueprint(4, 2, 0);
    sim.tick_n(2);
    assert!(sim.has_assignment("a"), "работа перехватила его по дороге");
}

/// Занятую лежанку не выбирают: ни ту, на которой спят, ни ту, к которой уже
/// идут, — иначе двое пришли бы на одно место и разошлись бы качелями (§12.39).
#[test]
fn only_one_cat_walks_to_each_bed() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(4, 1, 1); // одна лежанка на двоих
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800);
    sim.set_energy("b", 800);

    sim.tick_n(20);
    let (at_bed, other) = match sim.pos_of("a") == (4, 1) {
        true => ("a", "b"),
        false => ("b", "a"),
    };
    assert_eq!(sim.pos_of(at_bed), (4, 1), "лежанку занял один");
    assert_ne!(sim.pos_of(other), (4, 1), "второй на неё не пошёл");
    assert_eq!(sim.job_of(other), ("", false), "и остался без дела");
}

/// Полному дремать нечего: он никуда не идёт. Иначе бригада вечно бродила бы
/// в спальню за очком, которого ей не дадут.
#[test]
fn a_full_cat_stays_where_it_is() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_rest(1, 10);
    sim.force_tile(7, 1, 1);
    sim.set_needs(1000, 300, 1); // `set_needs` выдаёт всем полную бодрость

    sim.tick_n(1); // первый тик бодрствования ещё не потрачен
    assert_eq!(sim.pos_of("a"), (1, 1), "с места не двинулся");
}

/// Дремлют только там, где спят: посреди коридора бодрость не набегает, иначе
/// зона отдыха нужна была бы лишь для скорости.
#[test]
fn nobody_dozes_on_a_bare_floor() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_needs(1000, 300, 5);
    sim.set_energy("a", 500);

    sim.tick_n(10);
    assert_eq!(sim.energy_of("a"), 490, "стоял без дела и только тратил");
    assert_eq!(sim.job_of("a"), ("", false), "и это «без дела», не дремота");
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

/// На настоящем `core.yaml`: потолки сна расставлены так, что выспаться можно
/// только в кровати (§12.52).
///
/// Ловит рассогласование контента с самим смыслом потолка: потолок пола выше
/// `tired` означал бы, что зона отдыха не нужна вовсе, потолок лежанки ниже
/// `tired` — что кот встаёт всё ещё уставшим и ложится обратно тем же тиком, а
/// потолок выше полной бодрости — что потолка нет, только запись в рулсете.
#[test]
fn the_shipped_ruleset_makes_beds_the_only_full_sleep() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let sim = Sim::new(yaml).ok().expect("рулсет должен разбираться");
    let (tired, _) = sim.thresholds();
    let (floor_wake, max) = sim.ceilings();

    assert!(
        0 < floor_wake && floor_wake < tired,
        "на полу только отлёживаются: потолок {floor_wake} против порога {tired}",
    );

    let bed = sim.tile_index("bed").expect("лежанка есть в палитре");
    let (rate, wake) = sim.bed_of(bed);
    assert!(rate > 0, "лежанка обязана быть местом для сна");
    assert!(
        tired < wake && wake <= max,
        "потолок лежанки живёт между усталостью и полной: {wake}",
    );

    // Гнездо — вторая ступень отдыха, и разница у него не только в скорости:
    // оно высыпает целиком (потолок `0` = до полной, §12.52).
    let nest = sim.tile_index("nest").expect("гнездо есть в палитре");
    let (nest_rate, nest_wake) = sim.bed_of(nest);
    assert!(nest_rate > rate, "гнездо быстрее лежанки");
    assert!(
        nest_wake == 0 || nest_wake >= wake,
        "и высыпает не хуже: {nest_wake} против {wake}",
    );
}

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

    // Уставшего доводим до порога руками, а не ждём смены: с §12.52 бригада
    // добирает бодрость в простой (`assign_nap`), и на спокойной базе до
    // `tired` может не дойти никто — это и есть смысл дремоты, а не поломка.
    // Проверяем то, ради чего тест писался: уставший кот доходит до лежанки
    // сам и спит на ней.
    let sleeper = crew[0];
    sim.set_energy(sleeper, tired - 1);

    let mut asleep = false;
    for _ in 0..600 {
        sim.tick_n(1);
        asleep = sim.is_resting(sleeper) && !sim.has_path(sleeper);
        if asleep {
            break;
        }
    }
    assert!(asleep, "уставший дошёл до лежанки и лёг");
    assert!(
        sim.energy_of(sleeper) > 0,
        "ушёл сам, на своих лапах, а не свалился от истощения"
    );
    let (x, y) = sim.pos_of(sleeper);
    assert!(
        sim.bed_of(sim.tile(x, y)).0 > 0,
        "лёг именно на лежанку, а не где пришлось",
    );
}

/// Стеллаж — мебель, а не комната: пройти можно, остаться нельзя (§12.35).
/// Дорога к лежанке может лежать через него, и кот обязан её пройти, а не
/// топтаться на границе: шаг на полку — `clear_solids` гонит обратно — шаг на
/// полку. Ловит качели, найденные на боевой партии (трейс от 6 августа).
#[test]
fn a_nap_crosses_a_rack_instead_of_bouncing() {
    let mut sim = sim_from(&["########", "#a.....#", "########"]);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(5, 1, 1); // лежанка за стеллажами
    sim.set_solid(2, true);
    sim.force_tile(3, 1, 2);
    sim.force_tile(4, 1, 2); // два стеллажа подряд на пути
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800);

    sim.tick_n(40);
    assert_eq!(
        sim.pos_of("a"),
        (5, 1),
        "дошёл до лежанки, а не застрял у полок"
    );
    assert_eq!(sim.job_of("a"), ("nap", false), "и дремлет на ней");
}

/// Та же качель, но со вторым котом вместо полки: остановиться в чужой клетке
/// нельзя (§12.32), и шаг к лежанке обязан её перешагнуть — иначе `spread_units`
/// разведёт котов, а раздача пошлёт дремлющего обратно.
#[test]
fn a_nap_steps_over_a_standing_cat() {
    let mut sim = sim_from(&["########", "#a.b...#", "########"]);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(6, 1, 1); // лежанка за спиной у второго кота
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800);
    sim.set_energy("b", 800);
    sim.add_blueprint(3, 0, 0); // работа держит `b` на месте

    sim.tick_n(3);
    assert!(sim.has_assignment("b"), "второй занят делом и стоит");

    sim.tick_n(30);
    assert_eq!(sim.pos_of("a"), (6, 1), "дремлющий перешагнул и дошёл");
}

/// Занятую дремлющим лежанку второй кот не занимает и не «примеряет»: шагнув на
/// неё, он был бы разведён `spread_units` тем же тиком и вернулся обратно — то
/// самое дёрганье, которое видно на боевой партии. Проверяем **на каждом тике**:
/// одно конечное состояние такую качель прячет.
#[test]
fn a_napper_never_steps_onto_a_taken_bed() {
    let mut sim = sim_from(&["#######", "#a...b#", "#######"]);
    sim.set_rest(1, 10);
    sim.set_wake(1, 500);
    sim.force_tile(3, 1, 1); // одна лежанка на двоих, ровно посередине
    sim.set_needs(1000, 300, 1);
    sim.set_energy("a", 800);
    sim.set_energy("b", 800);

    // Ждём, пока лежанку займёт кто-нибудь один.
    let mut napper = None;
    for _ in 0..20 {
        sim.tick_n(1);
        napper = ["a", "b"].into_iter().find(|c| sim.pos_of(c) == (3, 1));
        if napper.is_some() {
            break;
        }
    }
    let napper = napper.expect("кто-то дошёл до лежанки");
    let other = if napper == "a" { "b" } else { "a" };

    let mut seen = Vec::new();
    for _ in 0..40 {
        sim.tick_n(1);
        assert_eq!(sim.job_of(napper), ("nap", false), "первый дремлет на ней");
        assert_ne!(
            sim.pos_of(other),
            (3, 1),
            "второй на занятую лежанку не встал"
        );
        seen.push(sim.pos_of(other));
    }
    let last = seen[seen.len() - 1];
    assert!(
        seen[20..].iter().all(|&p| p == last),
        "и не мечется рядом с ней: {:?}",
        &seen[20..]
    );
}
