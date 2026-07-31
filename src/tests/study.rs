//! Обучение у парты (§12.18).
//!
//! Рост от работы не умеет стартовать сам: чтобы набрать опыт исследования,
//! надо уже уметь исследовать. Парта — второй путь: кот идёт на клетку, стоит
//! и тикает, а на выходе не построенный тайл, а опыт.
//!
//! Мир везде один: коридор с партой (тайл 1) — в схеме `sim_from` парты нет,
//! как нет ни склада, ни шлюза, поэтому свойство задаём явно.

use super::*;

/// Коридор с одной партой в (3,1), домен «Наука» с порогами и обучением до
/// первого уровня. Вернёт мир и индекс домена.
fn sim_with_desk() -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &[20, 100]);
    sim.set_taught(science, 1);
    sim.set_teaches(1, science);
    sim.force_tile(3, 1, 1);
    (sim, science)
}

#[test]
fn a_cat_sent_to_study_walks_to_the_desk_and_learns() {
    let (mut sim, science) = sim_with_desk();

    assert!(sim.teach("a", "science"));
    sim.tick_n(3);
    assert_eq!(sim.pos_of("a"), (3, 1), "дошёл до парты");
    assert!(sim.xp_of("a", science) > 0, "и она уже учит");
}

/// Опыт капает за тик, как и на любой другой работе (§12.17): начисляет его
/// `train_skills`, а парта только вешает маркер.
#[test]
fn studying_earns_a_point_per_tick() {
    let (mut sim, science) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(4); // дошёл и сел
    let before = sim.xp_of("a", science);

    sim.tick_n(5);
    assert_eq!(sim.xp_of("a", science) - before, 5, "очко за тик");
}

/// Парта — вход в домен, а не тренажёр: дойдя до `taught`, кот встаёт сам.
/// Иначе обучение заменяет работу, и «чем больше делает, тем лучше» перестаёт
/// что-либо значить.
#[test]
fn study_stops_at_the_taught_ceiling() {
    let (mut sim, science) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(60);

    assert_eq!(sim.xp_of("a", science), 20, "ровно первый порог");
    assert_eq!(sim.level_of("a", science), 1);
    assert!(!sim.is_studying("a"), "и кот свободен");
}

/// Доученного парта не берёт: учить его нечему, а команда, которая молча ничего
/// не делает, читается как поломка.
#[test]
fn a_cat_at_the_ceiling_is_not_taught_again() {
    let (mut sim, science) = sim_with_desk();
    sim.set_xp("a", science, 20);

    assert!(!sim.teach("a", "science"), "выше порога парта не учит");
    assert!(!sim.is_studying("a"));
}

/// «Стройка» доступна с первого тика и парты не требует (§12.18): домен без
/// `taught` не преподаётся вовсе.
#[test]
fn a_domain_without_teaching_has_no_desk() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let build = sim.set_skill("build", &[20]);
    sim.set_teaches(1, build);
    sim.force_tile(3, 1, 1);

    assert!(!sim.teach("a", "build"), "этому домену не учат");
    assert!(!sim.teach("a", "cooking"), "а такого домена и нет");
}

/// Парту занимает один кот — ровно как лежанку (§12.20). Делить её нельзя:
/// иначе число парт ни на что не влияет.
#[test]
fn a_desk_holds_one_cat() {
    let (mut sim, _) = sim_with_desk();

    assert!(sim.teach("a", "science"));
    assert!(!sim.teach("b", "science"), "вторая парта не выросла");
    assert_eq!(sim.desk_of("a"), Some((3, 1)));

    // Построили вторую — второму ученику есть куда сесть.
    sim.force_tile(4, 1, 1);
    assert!(sim.teach("b", "science"));
    assert_eq!(sim.desk_of("b"), Some((4, 1)), "и это именно вторая");
}

/// Ученик занят: раздатчики берут котов из общего пула, и пропущенный
/// `Without<Study>` увёл бы его с парты за первой же кучей лома.
#[test]
fn a_student_is_not_taken_by_other_work() {
    let (mut sim, science) = sim_with_desk();
    sim.set_capacity(2, 100);
    sim.force_tile(5, 1, 2); // склад, чтобы уборке было куда носить
    sim.teach("a", "science");
    sim.tick_n(3);

    sim.put_scrap(1, 1, 10); // куча на полу: автоуборка включена
    sim.add_blueprint(6, 1, 3); // и стройка рядом
    let before = sim.xp_of("a", science);
    sim.tick_n(5);

    assert!(sim.is_studying("a"), "ученик остался за партой");
    assert!(sim.xp_of("a", science) > before, "и продолжает учиться");
    assert!(!sim.has_haul("a") && !sim.has_assignment("a"));
}

/// Приказ игрока снимает учёбу и освобождает парту — учёба такая же задача, как
/// стройка и сон, и решение игрока весомее любой из них (§12.15, §12.20).
#[test]
fn an_order_takes_the_cat_off_the_desk() {
    let (mut sim, _) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(3);

    sim.set_target("a", 1, 1);
    assert!(!sim.is_studying("a"), "учёба снята");
    assert!(sim.teach("b", "science"), "и парта свободна");
}

/// Учёба стоит котовремени — это вся её цена (§12.18), поэтому усталость
/// ученика не щадит: на нуле бодрости он валится прямо у парты и отпускает её.
#[test]
fn an_exhausted_student_falls_asleep_and_frees_the_desk() {
    let (mut sim, _) = sim_with_desk();
    sim.set_needs(1000, 100, 1);
    sim.set_energy("a", 3);
    sim.teach("a", "science");
    sim.tick_n(6);

    assert!(sim.is_resting("a"), "уснул от истощения");
    assert!(!sim.is_studying("a"), "учёба снята");
    assert!(sim.teach("b", "science"), "парта досталась другому");
}

/// Парту могли снести, пока ученик шёл: он пересаживается за свободную, а нет
/// её — учёба кончается. Тот же случай, что снесённый под отрядом шлюз (§12.22).
#[test]
fn a_demolished_desk_moves_the_student_or_ends_the_lesson() {
    let (mut sim, science) = sim_with_desk();
    sim.set_taught(science, 2); // потолок повыше: тест не про него
    sim.force_tile(5, 1, 1); // вторая парта в дальнем конце
    sim.teach("a", "science");
    sim.tick_n(10);
    let xp = sim.xp_of("a", science);
    assert!(xp > 0, "сел за ближнюю и учится");

    sim.force_tile(3, 1, 0); // ту, за которой сидит, разобрали
    sim.tick_n(12);
    assert_eq!(sim.desk_of("a"), Some((5, 1)), "пересел за вторую");
    assert!(sim.xp_of("a", science) > xp, "и учится дальше");

    sim.force_tile(5, 1, 0); // и вторую тоже
    let xp = sim.xp_of("a", science);
    sim.tick_n(3);
    assert!(!sim.is_studying("a"), "учиться стало негде");
    assert_eq!(sim.xp_of("a", science), xp, "опыт остался при коте");
}

/// Ушедшего на вылазку не учат: его позиция — это шлюз, с которого он ушёл, а
/// сам он вне базы (§12.22).
#[test]
fn a_cat_in_the_field_cannot_be_taught() {
    let (mut sim, _) = sim_with_desk();
    sim.set_gate(2, true);
    sim.force_tile(6, 1, 2);
    let m = sim.set_mission(1, 50, &[]);
    sim.launch(m, vec!["b".to_string()]);
    sim.tick_n(8);

    assert!(sim.is_away("b"), "отряд ушёл");
    assert!(!sim.teach("b", "science"), "учить некого");
}

/// Заявка на учёбу распускает вылазку, как и приказ игрока: состав выбран
/// поимённо, заменить выбывшего некем (§12.23).
#[test]
fn teaching_a_squad_member_disbands_the_raid() {
    let (mut sim, _) = sim_with_desk();
    sim.set_gate(2, true);
    sim.force_tile(6, 1, 2);
    let m = sim.set_mission(2, 50, &[]);
    sim.launch(m, vec!["a".to_string(), "b".to_string()]);

    assert!(sim.teach("a", "science"));
    assert_eq!(sim.mission_left(), None, "вылазка распущена");
    assert!(!sim.in_squad("b"), "и второй свободен");
}

/// Боевой рулсет: «Науке» действительно учат, и парта в базе стоит на проходе.
/// Ловит контент, в котором домена нет, `taught` забыт или класс замурован.
#[test]
fn the_shipped_ruleset_teaches_science() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let science = sim.skill_index("science").expect("домен «Наука» в рулсете");

    assert_eq!(sim.level_of("excellent", science), 0, "с нуля");
    assert!(sim.teach("excellent", "science"), "парта нашлась");

    sim.tick_n(1200);
    assert!(
        sim.level_of("excellent", science) >= 1,
        "и довела до допуска: без него исследовать нечем",
    );
}
