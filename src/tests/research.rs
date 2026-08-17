//! Исследование образцов: тема → работа в лаборатории → технология (§12.26).
//!
//! Тема — разметка работы, как чертёж: игрок выбирает, что изучать, а
//! исполнителя берёт симуляция. Новое здесь одно — **допуск**: за тему сядет
//! только кот с нужным уровнем «Науки» (§12.18).
//!
//! Мир везде один: коридор с лабораторией (тайл 1) и складом (тайл 2) — в
//! схеме `sim_from` ни того, ни другого нет, поэтому свойства задаём явно.

use super::*;

/// Коридор: лаборатория в (3,1), склад в (5,1), домен «Наука» без обучения
/// (тесты допуска выдают опыт напрямую). Вернёт мир и индекс домена.
fn sim_with_lab() -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &[100, 400]);
    sim.set_lab(1, true);
    sim.force_tile(3, 1, 1);
    sim.set_capacity(2, 100);
    sim.force_tile(5, 1, 2);
    (sim, science)
}

// --- работа над темой -------------------------------------------------------

#[test]
fn a_topic_is_taken_to_the_lab_and_worked_on() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 200, &[], &[]);

    assert!(sim.start_research(topic));
    sim.tick_n(6);
    assert_eq!(sim.pos_of("a"), (3, 1), "исполнитель в лаборатории");
    assert!(sim.research_progress().is_some_and(|p| p > 0), "и работает");
}

/// Тема кончается технологией — иначе исследовать незачем.
#[test]
fn finished_research_records_the_tech() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 100, &[], &[]);
    sim.start_research(topic);

    assert!(!sim.knows_tech("materials"));
    sim.tick_n(30);
    assert!(sim.knows_tech("materials"), "технология записана");
    assert_eq!(sim.research_progress(), None, "а тема закрыта");
}

/// Исследование — такая же работа, как стройка: скорость даёт навык, и сам
/// навык от неё растёт (§12.17). Это и есть «мастерство из домена», ради
/// которого парта остановлена на пороге (§12.18).
#[test]
fn research_grows_the_science_skill() {
    let (mut sim, science) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 400, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(20);

    assert!(sim.xp_of("a", science) > 0, "навык растёт от работы");
}

/// Навык — множитель скорости и здесь: мастер закрывает тему быстрее.
#[test]
fn a_higher_level_researches_faster() {
    let (mut sim, science) = sim_with_lab();
    sim.set_xp("a", science, 400); // второй уровень
    let topic = sim.set_topic("materials", 0, 1000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    let fast = sim.research_progress().unwrap();

    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 1000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    assert!(fast > sim.research_progress().unwrap(), "уровень ускоряет");
}

// --- допуск -----------------------------------------------------------------

/// Навык бывает допуском, а не только скоростью (§12.18): без уровня кот за
/// тему не берётся вовсе — не медленно, а никак.
#[test]
fn only_a_qualified_cat_takes_the_topic() {
    let (mut sim, science) = sim_with_lab();
    sim.set_xp("b", science, 100); // первый уровень есть только у дальнего кота
    let topic = sim.set_topic("materials", 1, 400, &[], &[]);

    assert!(sim.start_research(topic));
    sim.tick_n(8);
    assert_eq!(
        sim.researcher().as_deref(),
        Some("b"),
        "ближайший не подошёл по допуску, взялся умеющий",
    );
}

/// Некому взяться — отказываем **до** оплаты: тема, за которую заплачено
/// образцами и которую никто не возьмёт, читается как потерянный ресурс.
#[test]
fn research_without_a_scientist_is_refused() {
    let (mut sim, _) = sim_with_lab();
    sim.put_item(5, 1, 0, 10);
    let topic = sim.set_topic("materials", 2, 400, &[(0, 4)], &[]);

    assert!(!sim.start_research(topic), "уровня нет ни у кого");
    assert_eq!(sim.item_at(5, 1, 0), 10, "и склад не тронут");
}

/// Работать негде — тема не берётся: лаборатория такое же условие, как склад
/// для уборки (§12.16).
#[test]
fn research_without_a_lab_is_refused() {
    let (mut sim, _) = sim_with_lab();
    sim.force_tile(3, 1, 0); // лабораторию разобрали
    let topic = sim.set_topic("materials", 0, 400, &[], &[]);

    assert!(!sim.start_research(topic));
}

// --- оплата -----------------------------------------------------------------

/// Платит склад — как за найм (§12.24): то, что валяется на полу, ещё не
/// сосчитано.
#[test]
fn research_is_paid_from_storage() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 400, &[(0, 6)], &[]);

    sim.put_scrap(1, 1, 20); // куча на полу, мимо склада
    assert!(!sim.start_research(topic), "на полу — ещё не казна");
    assert_eq!(sim.scrap_at(1, 1), 20, "и её не тронули");

    sim.put_scrap(5, 1, 6); // а это уже склад
    assert!(sim.start_research(topic));
    assert_eq!(sim.scrap_at(5, 1), 0, "образцы ушли на опыты");
}

/// Не хватило — не списываем ничего: половинчатая оплата оставила бы игрока и
/// без образцов, и без технологии.
#[test]
fn a_short_payment_takes_nothing() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 400, &[(0, 6), (1, 3)], &[]);
    sim.put_item(5, 1, 0, 20);
    sim.put_item(5, 1, 1, 1);

    assert!(!sim.start_research(topic), "цену не покрыть");
    assert_eq!(sim.item_at(5, 1, 0), 20);
    assert_eq!(sim.item_at(5, 1, 1), 1);
}

// --- дерево тем -------------------------------------------------------------

/// Технология — ворота для следующей темы: это и есть дерево из §4.3.
#[test]
fn a_topic_waits_for_its_prerequisite() {
    let (mut sim, _) = sim_with_lab();
    let first = sim.set_topic("materials", 0, 100, &[], &[]);
    let second = sim.set_topic("comfort", 0, 100, &[], &["materials"]);

    assert!(
        !sim.start_research(second),
        "без предыдущей темы не существует"
    );
    sim.start_research(first);
    sim.tick_n(30);

    assert!(sim.knows_tech("materials"));
    assert!(sim.start_research(second), "и открылась сама");
}

/// Изученное не изучают дважды: технология только записывается, а повторная
/// тема была бы способом слить образцы впустую.
#[test]
fn a_known_tech_is_not_researched_again() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 100, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(30);

    assert!(sim.knows_tech("materials"));
    assert!(!sim.start_research(topic), "второй раз не берутся");
}

/// Тема одна за раз — как и вылазка (§12.22): вторую на POC некому взять.
#[test]
fn only_one_topic_at_a_time() {
    let (mut sim, _) = sim_with_lab();
    let first = sim.set_topic("materials", 0, 400, &[], &[]);
    let second = sim.set_topic("comfort", 0, 400, &[], &[]);

    assert!(sim.start_research(first));
    assert!(!sim.start_research(second), "одна за раз");
}

// --- занятость --------------------------------------------------------------

/// Учёный занят: раздатчики берут котов из общего пула, и пропущенный
/// `Without<Researching>` увёл бы его из лаборатории за первой же кучей лома.
#[test]
fn a_researcher_is_not_taken_by_other_work() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 2000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);

    sim.put_scrap(1, 1, 10); // автоуборка включена
    let before = sim.research_progress().unwrap();
    sim.tick_n(5);

    assert_eq!(sim.researcher().as_deref(), Some("a"), "остался за темой");
    assert!(sim.research_progress().unwrap() > before);
    assert!(!sim.has_haul("a") && !sim.has_assignment("a"));
}

/// Наука раздаётся раньше стройки: за тему уже заплачено, а чертёж подождёт
/// (§12.26). Иначе наука не двинулась бы, пока на базе есть работа, — а она
/// есть почти всегда.
#[test]
fn research_outranks_building() {
    let (mut sim, science) = sim_with_lab();
    sim.set_xp("a", science, 100);
    sim.set_xp("b", science, 100);
    let topic = sim.set_topic("materials", 1, 2000, &[], &[]);
    sim.add_blueprint(1, 1, 3); // чертёж у самого дальнего кота
    sim.start_research(topic);
    sim.tick_n(4);

    assert!(sim.researcher().is_some(), "тему взяли, а не отложили");
}

/// Приказ игрока снимает тему с кота и освобождает её — как и любую другую
/// задачу (§12.15). Тема при этом не пропадает: её подберёт следующий.
#[test]
fn an_order_frees_the_topic() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 2000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    let progress = sim.research_progress().unwrap();

    sim.set_target("a", 1, 1);
    assert_eq!(sim.researcher(), None, "тема освобождена");
    assert_eq!(sim.research_progress(), Some(progress), "прогресс цел");

    sim.tick_n(10);
    assert!(sim.researcher().is_some(), "и её взял другой кот");
}

/// Истощение снимает тему так же, как снимает стройку: иначе она навсегда
/// осталась бы за спящим (§12.20).
#[test]
fn an_exhausted_researcher_frees_the_topic() {
    let (mut sim, _) = sim_with_lab();
    sim.set_needs(1000, 100, 1);
    sim.set_energy("a", 8);
    sim.set_energy("b", 1000);
    let topic = sim.set_topic("materials", 0, 2000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(10);

    assert!(sim.is_resting("a"), "уснул от истощения");
    assert_ne!(sim.researcher().as_deref(), Some("a"), "тема отпущена");
}

/// Тему можно бросить, но образцы не возвращаются: их уже разобрали на опыты —
/// та же цена поспешной разметки, что и у отменённого чертежа.
#[test]
fn cancelling_research_frees_the_cat_but_not_the_samples() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 2000, &[(0, 5)], &[]);
    sim.put_scrap(5, 1, 5);
    sim.start_research(topic);
    sim.tick_n(6);

    assert!(sim.cancel_research());
    assert_eq!(sim.research_progress(), None, "темы больше нет");
    assert_eq!(sim.item_total(0), 0, "образцы не вернулись");
    assert!(!sim.is_researching("a"), "а кот снова свободен");
}

/// Лабораторию снесли под работающим котом: тема отпускается и ждёт, как ждёт
/// чертёж без материала. Молчаливое зависание было бы хуже.
#[test]
fn a_demolished_lab_releases_the_topic() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 2000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    assert!(sim.researcher().is_some());

    sim.force_tile(3, 1, 0);
    sim.tick_n(6);
    assert_eq!(sim.researcher(), None, "работать негде — тема свободна");
    assert!(sim.research_progress().is_some(), "но не потеряна");
}

// --- технология как ворота --------------------------------------------------

/// Технология открывает постройку — вторые ворота прогрессии после известности
/// (§12.27). Отказ приходит в момент разметки: чертёж, который никто никогда не
/// возьмёт, читался бы как поломка.
#[test]
fn an_unresearched_tile_cannot_be_planned() {
    let (mut sim, _) = sim_with_lab();
    sim.set_tile_tech(3, "materials");

    assert!(!sim.add_blueprint(6, 1, 3), "тайл ещё не открыт");
    assert!(sim.add_blueprint(6, 1, 4), "а незакрытый — размечается");

    let topic = sim.set_topic("materials", 0, 100, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(30);

    assert!(sim.knows_tech("materials"));
    assert!(sim.add_blueprint(6, 1, 3), "изучили — и построить можно");
}

/// Снос воротами не закрыт: разбирать можно что угодно и всегда, иначе игрок
/// оказался бы заперт постройкой, которую сам же и поставил.
#[test]
fn demolition_needs_no_tech() {
    let (mut sim, _) = sim_with_lab();
    sim.set_tile_tech(1, "materials"); // лаборатория «закрыта»

    assert!(sim.plan_demolish_rect(3, 1, 1, 1), "снос разрешён");
}

/// Боевой рулсет: первая тема по силам коту, которого довела парта, а образцы
/// для неё есть чем добыть. Ловит контент, в котором лаборатория забыта, допуск
/// выше потолка обучения или цена темы в предмете, которого не бывает.
#[test]
fn the_shipped_ruleset_researches_its_first_topic() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let sample = 2; // индекс `sample` в палитре предметов

    // Образцы приходят с вылазок; здесь кладём их прямо на склад — эта проверка
    // про науку, а лестницу вылазок ведёт `the_shipped_ruleset_has_a_reachable_ladder`.
    sim.put_item(4, 3, sample, 10);
    assert!(!sim.start_research(0), "учёного на базе ещё нет");

    assert!(sim.teach("excellent", "science"));
    sim.tick_n(400); // парта доводит до допуска
    assert!(sim.start_research(0), "теперь тема по силам");

    sim.tick_n(1200);
    assert!(
        sim.knows_tech("materials"),
        "и доводится до технологии за разумное время",
    );

    // И технология что-то открывает: наука без выхода в базу — счётчик, а не
    // ворота. Ловит контент, где `tech` не совпал ни с одной темой.
    let rack = 6; // индекс `rack` в палитре тайлов
    assert!(
        sim.add_blueprint(10, 7, rack),
        "«Стеллаж» открылся вместе с материаловедением",
    );
    let nest = 7; // а «Гнездо» ждёт следующей темы
    assert!(!sim.add_blueprint(11, 7, nest), "быт колонии ещё не изучен");
}

/// **Боевой рулсет: автоматика достижима.** Ловит контент, в котором ворота
/// названы технологией, которой нет; тема-веха забыта в `requires`; или допуск
/// подтемы выше того, до которого доводит парта, — то есть автоматика,
/// запертая навсегда (§12.93).
#[test]
fn the_shipped_ruleset_can_reach_its_automation() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let cap = sim.taught_cap("science");
    let gates = sim.auto_gates();
    assert_eq!(gates.len(), 3, "ворота названы у всех трёх правил");

    for gate in gates {
        let topic = sim
            .topic_by_id(&gate)
            .unwrap_or_else(|| panic!("ворота named `{gate}`, а темы с таким id нет"));
        assert!(
            topic.level <= cap,
            "тема `{gate}` требует «Науки» {}, а парта доводит до {cap} — автоматика заперта",
            topic.level,
        );
        // Веха обязана стоять в `requires`: иначе подтема висит сама по себе, и
        // ветки нет — а именно ветка и была решением (§12.93).
        assert!(
            !topic.requires.is_empty(),
            "тема `{gate}` не требует вехи: ветки автоматики нет, есть три отдельные темы",
        );
        for need in &topic.requires {
            let parent = sim
                .topic_by_id(need)
                .unwrap_or_else(|| panic!("тема `{gate}` требует `{need}`, которой нет"));
            assert!(
                parent.level <= cap,
                "веха `{need}` требует «Науки» {}, а парта доводит до {cap}",
                parent.level,
            );
        }
    }
}
