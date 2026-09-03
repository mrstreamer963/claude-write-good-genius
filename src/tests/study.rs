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
    sim.tick_n(5);
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

/// Учёба — приписка, а не разовая посадка (§12.84): ученик, которого увела
/// усталость, возвращается за парту сам. До §12.84 он не возвращался никогда, и
/// узнать об этом можно было только по не растущему уровню.
///
/// И возвращается он **раньше, чем берётся за работу**: приписка, уступающая
/// чертежу, не значила бы ничего — работа на базе есть всегда.
#[test]
fn a_rested_student_returns_to_the_desk_before_taking_work() {
    let (mut sim, science) = sim_with_desk();
    sim.set_taught(science, 2); // потолок повыше: тест не про него
    sim.set_needs(100, 50, 1);
    sim.set_critical(20); // ниже него кот бросает начатое (§12.33)
    sim.set_rest(2, 2);
    sim.set_wake(2, 60); // с потолком: проснётся, а не доспит до полной
    sim.force_tile(5, 1, 2);
    sim.set_energy("a", 23);
    sim.add_blueprint(4, 1, 3); // работа рядом с партой — на всякий случай

    sim.teach("a", "science");
    sim.tick_n(2);
    assert!(sim.is_studying("a"), "сел за парту");

    sim.tick_n(6);
    assert!(sim.is_resting("a"), "бодрость кончилась, парта брошена");
    assert!(!sim.is_studying("a"));
    assert!(sim.is_enrolled("a"), "но приписка держится");

    let xp = sim.xp_of("a", science);
    sim.tick_n(30);
    assert_eq!(sim.desk_of("a"), Some((3, 1)), "выспался и вернулся сам");
    assert!(sim.xp_of("a", science) > xp, "и учится дальше");
    assert!(!sim.has_assignment("a"), "а не ушёл на чертёж");
}

/// Дойдя до потолка, кот снимается с приписки, а не только встаёт из-за парты
/// (§12.84). Иначе доучившийся ходил бы к ней вечно и вечно вставал бы с неё —
/// игрок прочёл бы это как зависшего кота.
#[test]
fn the_ceiling_ends_the_enrolment() {
    let (mut sim, science) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(60);

    assert_eq!(sim.xp_of("a", science), 20, "ровно первый порог");
    assert!(!sim.is_enrolled("a"), "приписка снята");

    sim.tick_n(10);
    assert!(!sim.is_studying("a"), "и к парте кот не возвращается");
    assert!(sim.teach("b", "science"), "а парта досталась другому");
}

/// «Снять с учёбы» — своя команда (§12.84), зеркало `unpost_relay`: снимает и
/// приписку, и текущую задачу. Не снимай она задачу — кот досидел бы за партой
/// до потолка, и игрок решил бы, что кнопка не сработала.
#[test]
fn unteaching_takes_the_cat_off_the_desk_at_once() {
    let (mut sim, science) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(6);
    let xp = sim.xp_of("a", science);
    assert!(xp > 0, "учится");

    assert!(sim.unteach("a"));
    assert!(!sim.is_enrolled("a") && !sim.is_studying("a"), "снят разом");
    assert!(sim.teach("b", "science"), "и парта свободна");

    sim.tick_n(10);
    assert_eq!(sim.xp_of("a", science), xp, "опыт остался, но не растёт");
}

/// Снимать некого — команда отказывает, а не молчит: `false` наверх и есть
/// разница между «сделано» и «нечего делать».
#[test]
fn unteaching_a_cat_who_studies_nothing_fails() {
    let (mut sim, _) = sim_with_desk();
    assert!(!sim.unteach("a"), "никуда не приписан");
    assert!(!sim.unteach("нет такого"), "и такого кота нет");
}

/// Два клика игрока: выбрал кота, ткнул в парту — и кот учится (§12.85).
///
/// Это тот жест, которым механикой пользуются, и до §12.85 он не делал ничего:
/// «иди туда» на парте — это «постой и уйди работать». Кнопка в тулбаре была, но
/// её надо было знать; клетка же говорит о себе сама.
#[test]
fn an_order_onto_a_desk_enrols_the_cat() {
    let (mut sim, science) = sim_with_desk();

    assert!(sim.set_target("a", 3, 1), "приказ на парту принят");
    assert!(sim.is_enrolled("a"), "и это запись на учёбу");
    assert_eq!(sim.desk_of("a"), Some((3, 1)), "именно за эту парту");

    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (3, 1));
    assert!(sim.xp_of("a", science) > 0, "дошёл и учится");
}

/// Парта именно та, в которую ткнули, а не ближайшая свободная: игрок указал
/// клетку, и посадить за соседнюю значило бы ответить не на тот жест.
#[test]
fn an_order_picks_the_desk_the_player_pointed_at() {
    let (mut sim, _) = sim_with_desk();
    sim.force_tile(5, 1, 1); // вторая парта, дальняя

    sim.set_target("a", 5, 1);
    assert_eq!(sim.desk_of("a"), Some((5, 1)), "дальняя, раз ткнули в неё");
}

/// Занятую парту клик не отнимает: два кота за одной не сидят (§12.20), и приказ
/// остаётся приказом — кот дойдёт и займётся своим.
#[test]
fn an_order_onto_a_taken_desk_stays_an_order() {
    let (mut sim, _) = sim_with_desk();
    sim.teach("a", "science");
    sim.tick_n(3);

    assert!(sim.set_target("b", 3, 1), "приказ принят");
    assert!(!sim.is_enrolled("b"), "но за партой не он");
    assert_eq!(sim.desk_of("a"), Some((3, 1)), "она осталась за первым");
}

/// Доучившегося клик по парте не записывает: молчаливая запись, которая ничего
/// не даёт, — это та же молчащая кнопка (§12.84). Приказ при этом работает.
#[test]
fn an_order_onto_a_desk_at_the_ceiling_stays_an_order() {
    let (mut sim, science) = sim_with_desk();
    sim.set_xp("a", science, 20); // потолок парты

    assert!(sim.set_target("a", 3, 1));
    assert!(!sim.is_enrolled("a"), "учить нечему");
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (3, 1), "но дойти — дошёл");
}

/// Приказ «иди туда» приписку **не** снимает (§12.84), как не снимает приписку
/// к рации: кот сходит куда велено и вернётся за парту сам.
///
/// Это регрессия, стоившая механике целого дня. День приказ приписку снимал —
/// «два адресных распоряжения противоречат друг другу», — и выглядело это
/// стройно ровно до первой живой партии: клетка парты и есть то место, куда
/// игрок кликает, отправляя кота учиться, а клик по клетке это приказ. Кот
/// доходил до парты и уходил работать, потому что тем же кликом учёбу и
/// отменили. Отмена живёт в кнопке (`unteach`), а не в побочном действии.
#[test]
fn an_order_does_not_end_the_enrolment() {
    let (mut sim, science) = sim_with_desk();
    sim.set_taught(science, 2); // потолок повыше: тест не про него
    sim.teach("a", "science");
    sim.tick_n(3);

    sim.set_target("a", 1, 1);
    assert!(sim.is_enrolled("a"), "приписка держится");
    assert!(
        !sim.is_studying("a"),
        "но задача снята: приказ весомее (§12.15)"
    );

    sim.tick_n(10);
    assert_eq!(sim.desk_of("a"), Some((3, 1)), "сходил и вернулся за парту");
}

/// Тот же клик, каким игрок отправляет кота к парте: приказ **на саму парту**.
/// Он не имеет права отменить учёбу — иначе механика ломается ровно тем
/// действием, которым её включают.
#[test]
fn an_order_onto_the_desk_keeps_the_cat_learning() {
    let (mut sim, science) = sim_with_desk();
    sim.set_taught(science, 2);
    sim.teach("a", "science");
    sim.set_target("a", 3, 1); // «иди на парту» — клик по её клетке
    sim.tick_n(12);

    assert_eq!(sim.pos_of("a"), (3, 1), "стоит за партой");
    assert!(sim.is_enrolled("a") && sim.is_studying("a"), "и учится");
    assert!(sim.xp_of("a", science) > 0, "опыт капает");
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
    sim.set_relay(2, true);
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
    sim.set_relay(2, true);
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

// --- Ворота обучения (§12.53) -------------------------------------------
//
// До них у `Sim::teach` было пять отказов подряд, и наружу они уходили одним
// `false`. Теперь это одно выражение с тегом, и тесты сторожат ровно две вещи:
// что каждый отказ **назван своим словом** и что ворота **совпадают с фасадом**
// (инвариант 14) — разойдись они, кнопка загорится там, где `teach` откажет.

/// «Стройка» растёт только работой: `taught` у неё ноль, и парта ей не поможет.
/// Отказ про домен, а не про кота, поэтому и спрашивается он первым.
#[test]
fn the_teach_gate_names_a_domain_nobody_teaches() {
    let (mut sim, _) = sim_with_desk();
    let build = sim.set_skill("build", &[20, 100]);
    sim.set_taught(build, 0);

    assert_eq!(sim.teach_gate("a", build), "untaught");
    assert!(!sim.teach("a", "build"), "и фасад отказывает так же");
}

/// Кота нет на базе — учить некого: его позиция это шлюз, с которого он ушёл
/// (§12.22). Ворота обязаны сказать это до того, как пойдут искать парту.
#[test]
fn the_teach_gate_names_a_cat_who_is_away() {
    let (mut sim, science) = sim_with_desk();
    sim.set_gate(2, true);
    sim.force_tile(6, 1, 2);
    let m = sim.set_mission(1, 50, &[]);
    sim.launch(m, vec!["b".to_string()]);
    sim.tick_n(6);
    assert!(sim.is_away("b"), "ушёл");

    assert_eq!(sim.teach_gate("b", science), "away");
}

/// Доученного парта не берёт: отправленный за неё кот встал бы с неё в тот же
/// тик, а игрок прочёл бы это как поломку.
#[test]
fn the_teach_gate_names_a_cat_at_the_desk_ceiling() {
    let (mut sim, science) = sim_with_desk();
    sim.set_xp("a", science, 20);

    assert_eq!(sim.teach_gate("a", science), "topped");
}

/// «Парт нет» и «все заняты» — разные ответы: первый чинится стройкой, второй
/// ожиданием. Слить их в одно «нельзя» значило бы отказ без причины (§12.53).
#[test]
fn the_teach_gate_tells_a_missing_desk_from_a_taken_one() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &[20, 100]);
    sim.set_taught(science, 1);
    sim.set_teaches(1, science);
    assert_eq!(sim.teach_gate("a", science), "nodesk", "парт нет вовсе");

    sim.force_tile(3, 1, 1);
    assert_eq!(sim.teach_gate("a", science), "", "парта появилась");

    assert!(sim.teach("b", "science"), "её занял сосед");
    assert_eq!(sim.teach_gate("a", science), "taken");
}

/// Тот самый отказ, который виду невыразим: парта свободна, но до неё не дойти.
/// Достижимость — это BFS по карте, и второй его экземпляр в JS однажды
/// покажет живой кнопку, которую фасад отклонит (инвариант 14).
#[test]
fn the_teach_gate_names_a_desk_behind_a_wall() {
    let mut sim = sim_from(&["#####", "#a..#", "#####", "#...#", "#####"]);
    let science = sim.set_skill("science", &[20, 100]);
    sim.set_taught(science, 1);
    sim.set_teaches(1, science);
    sim.force_tile(2, 3, 1);

    assert_eq!(
        sim.teach_gate("a", science),
        "taken",
        "свободна, но за стеной"
    );
    assert!(!sim.teach("a", "science"), "и фасад отказывает");
}

/// Сторож инварианта 14: открытые ворота и успех `teach` — одно и то же.
/// Разойдутся — кнопка в «Личном деле» начнёт врать, и заметить это можно будет
/// только по молчащему клику.
#[test]
fn the_teach_gate_is_open_exactly_when_teach_succeeds() {
    // Шесть миров, по одному на каждый ответ ворот и один открытый.
    let cases: Vec<(&str, fn(&mut Sim, usize))> = vec![
        ("", |_, _| {}),
        ("untaught", |sim, skill| sim.set_taught(skill, 0)),
        ("topped", |sim, skill| sim.set_xp("a", skill, 20)),
        ("nodesk", |sim, _| {
            sim.force_tile(3, 1, 0);
        }),
        ("taken", |sim, _| {
            sim.teach("b", "science");
        }),
    ];
    for (want, setup) in cases {
        let (mut sim, science) = sim_with_desk();
        setup(&mut sim, science);
        let gate = sim.teach_gate("a", science);
        assert_eq!(gate, want, "ворота назвали не тот отказ");
        assert_eq!(
            gate.is_empty(),
            sim.teach("a", "science"),
            "ворота и фасад разошлись на «{want}»",
        );
    }
}
