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

/// Наука идёт **раньше подвоза** (§12.197): за тему уже заплачено, тему выбрал
/// игрок поимённо, и допуск отсекает почти всю базу — а лом донесёт кто угодно.
/// Стоя после подвоза, наука не начиналась вовсе: размеченный пол занимает
/// носильщиков сотнями тиков, и единственный учёный уходил таскать лом заново
/// каждым тиком, пока тема стояла на нуле.
#[test]
fn a_scientist_researches_instead_of_hauling() {
    let (mut sim, _) = sim_with_lab();
    // Работа носильщику: чертёж в пустоте, за ним цена, а лом лежит рядом.
    sim.set_cost(3, 1);
    sim.put_scrap(4, 1, 10);
    assert!(sim.add_blueprint(1, 0, 3));
    let topic = sim.set_topic("materials", 0, 400, &[], &[]);
    assert!(sim.start_research(topic));

    sim.tick_n(1);
    assert!(!sim.has_haul("a"), "учёного не увёл подвоз");
    sim.tick_n(6);
    assert!(sim.is_researching("a"), "он в лаборатории");
    assert!(sim.research_progress().is_some_and(|p| p > 0), "тема идёт");
    assert!(sim.has_haul("b"), "а лом несёт второй кот");
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

    assert!(sim.cancel_research(topic));
    assert_eq!(sim.research_progress(), None, "темы больше нет");
    assert_eq!(sim.item_total(0), 0, "образцы не вернулись");
    assert!(!sim.is_researching("a"), "а кот снова свободен");
}

/// Лабораторию снесли под работающим котом: тема уходит вместе с ней (§12.132).
///
/// Дословно снесённый станок, уносящий свой заказ (§12.96): комната
/// принадлежит теме, и темы без комнаты не бывает. До §12.132 тема оставалась
/// висеть «в ожидании лаборатории» — но комнату ей больше никто не искал бы,
/// потому что искать её теперь некому: ячейку выбирает заявка.
#[test]
fn a_demolished_lab_takes_its_topic_with_it() {
    let (mut sim, _) = sim_with_lab();
    let topic = sim.set_topic("materials", 0, 2000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    assert!(sim.researcher().is_some());

    sim.force_tile(3, 1, 0);
    sim.tick_n(6);
    assert_eq!(sim.researcher(), None, "исполнитель свободен");
    assert_eq!(sim.research_progress(), None, "и темы больше нет");
    assert!(!sim.knows_tech("materials"), "технологию она не подарила");
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

/// Боевой рулсет: **весь пролог науки целиком** — от непонятого образца до
/// первой открытой постройки.
///
/// Проверяет он не одну тему, а стык двух правил. «Свойства образца» открыты
/// не наукой, а находкой (ворота `Seen`, §12.139) и берут материал ногами
/// (§12.133); «Материаловедение» до них закрыто, потому что просит непонятый
/// предмет (§12.131) — ровно тот диссонанс, ради которого пролог и заведён:
/// цена не имеет права называть предмет, которого нет в реестре склада.
/// Ловит контент, в котором лаборатория забыта, допуск выше потолка обучения,
/// у пролога отобрали ворота или цепочку замкнули на саму себя.
#[test]
fn the_shipped_ruleset_researches_its_first_topic() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let sample = sim.item_index("sample").expect("образец есть");
    let lore = sim.topic_index("sample_lore").expect("пролог есть");
    let materials = sim.topic_index("materials").expect("тема есть");

    assert!(!sim.seen(sample), "до первой вылазки образца в мире нет");

    // Образцы приходят с вылазок; здесь кладём их прямо на склад — эта проверка
    // про науку, а лестницу вылазок ведёт `the_shipped_ruleset_has_a_reachable_ladder`.
    sim.put_item(4, 3, sample, 10);
    sim.tick_n(1); // `note_seen` замыкает тик и отмечает предмет виденным
    assert!(
        sim.seen(sample),
        "привезённый образец получил строку в складе"
    );

    assert!(!sim.start_research(lore), "учёного на базе ещё нет");

    assert!(sim.teach("excellent", "science"));
    sim.tick_n(400); // парта доводит до допуска

    assert!(
        !sim.start_research(materials),
        "«Материаловедение» закрыто прологом, а не известностью",
    );
    assert!(sim.start_research(lore), "а пролог по силам");

    sim.tick_n(1200); // сюда входит и дорога за образцом в лабораторию
    assert!(
        sim.knows_tech("sample_lore"),
        "образец понят за разумное время",
    );

    assert!(sim.start_research(materials), "и тема на нём открылась");
    sim.tick_n(1200);
    assert!(
        sim.knows_tech("materials"),
        "и доводится до технологии за разумное время",
    );

    // И технология что-то открывает: наука без выхода в базу — счётчик, а не
    // ворота. Ловит контент, где `tech` не совпал ни с одной темой. С §12.146
    // веха открывает не постройку, а подтему, поэтому шага здесь два.
    let rack = 6; // индекс `rack` в палитре тайлов
    assert!(
        !sim.add_blueprint(10, 7, rack),
        "веха постройку не даёт: у «Стеллажа» своя тема",
    );
    let racks = sim.topic_index("racks").expect("подтема есть");
    assert!(sim.start_research(racks), "и она открылась вехой");
    sim.tick_n(1200);
    assert!(
        sim.add_blueprint(10, 7, rack),
        "«Стеллаж» открылся своей темой",
    );
    let nest = 7; // а «Гнездо» ждёт следующей темы
    assert!(!sim.add_blueprint(11, 7, nest), "быт колонии ещё не изучен");
}

/// **Боевой рулсет: лаборатория считается.** Ячейка — слот темы (§12.132), и
/// стартовая застройка обязана дать их **несколько, но немного**: ноль запирает
/// науку, а комната, вымощенная лабораторией целиком, даёт два десятка тем разом
/// и молча выключает счётность (§12.55) — третья ячейка должна оставаться
/// стройкой, а не подарком.
#[test]
fn the_shipped_ruleset_counts_its_lab_cells() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let cells = sim.lab_cells();
    assert!(cells > 0, "без ячейки наука заперта");
    assert!(
        cells <= 4,
        "ячеек в стартовой застройке {cells} — счётность выключена",
    );
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

// --- тема-вскрытие (§12.133) -------------------------------------------------
//
// Образец **едет в лабораторию ногами**, как материал на станок (§12.102), и
// там тратится. Ворота при этом стоят не на складе, а на шкале `Seen`
// (§12.131) — «предмет хоть раз побывал на базе», — и вся эта развилка про
// один-единственный случай: коты разобрали привезённые комбинезоны по себе.

/// Первые ворота — «видели»: невиданное не вскрывают ни при каком складе.
#[test]
fn a_specimen_topic_is_shut_until_the_item_has_been_seen() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(2);
    // Уборка тут только мешает: кот подхватил бы образец в лапы, и «в мире его
    // больше нет» перестало бы быть правдой — груз в лапах кучей не считается.
    sim.set_auto_tidy(false);
    let topic = sim.set_topic("fabrics", 0, 400, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[]);

    assert!(!sim.start_research(topic), "предмета база ещё не видела");

    // Один раз полежал на полу — и этого хватило навсегда (§12.131).
    sim.put_item(1, 1, 1, 1);
    sim.tick_n(1);
    sim.take_item(1, 1, 1);
    sim.tick_n(1);
    assert_eq!(sim.item_total(1), 0, "в мире образца больше нет");

    assert!(
        !sim.start_research(topic),
        "тема повидана, но образца на складе нет — заявку не принимают (§12.139)",
    );

    // Привезли ещё один — и тема заводится.
    sim.put_item(5, 1, 1, 1);
    sim.tick_n(1);
    assert!(sim.start_research(topic), "теперь образец лежит на складе");
}

/// Вторые ворота — склад, и меряют они **складскую кучу**: валяющееся на полу
/// в счёт не идёт (§12.130), потому что подвоз в лабораторию его не возьмёт.
#[test]
fn a_specimen_on_the_floor_does_not_open_the_topic() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(2);
    sim.set_auto_tidy(false); // иначе образец уедет на склад сам
    let topic = sim.set_topic("fabrics", 0, 400, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[]);
    sim.put_item(1, 1, 1, 1); // пол
    sim.tick_n(1);

    assert!(!sim.start_research(topic), "с пола образец не берут");

    sim.set_auto_tidy(true);
    sim.tick_n(30);
    assert!(
        sim.start_research(topic),
        "убрали на склад — тема открылась"
    );
}

/// Кот везёт образец со склада в лабораторию, и только после этого тема идёт.
#[test]
fn a_specimen_is_hauled_to_the_lab_before_the_work_starts() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(2);
    let topic = sim.set_topic("fabrics", 0, 4000, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[]);
    sim.put_item(5, 1, 1, 1); // склад
    sim.tick_n(1);

    assert!(sim.start_research(topic));
    sim.tick_n(40);

    assert_eq!(sim.topic_delivered(1), 1, "образец завезли");
    assert_eq!(sim.item_total(1), 0, "и он ушёл со склада");
    assert!(sim.researcher().is_some(), "теперь за тему взялись");
    assert!(sim.research_progress().is_some_and(|p| p > 0));
}

/// Что вышло из образца, ложится **кучей на клетку лаборатории** (инвариант 8),
/// как добыча на шлюз и готовое под ноги мастеру.
#[test]
fn a_topic_drops_what_it_found_on_its_own_cell() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(3);
    let topic = sim.set_topic("fabrics", 0, 100, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[(2, 2)]);
    sim.put_item(5, 1, 1, 1);
    sim.tick_n(1);
    sim.start_research(topic);

    // Уборку глушим: она увезёт выход на склад, и «легло в лаборатории» станет
    // непроверяемым — а проверяем мы именно место, а не факт появления.
    sim.set_auto_tidy(false);
    sim.tick_n(60);

    assert!(sim.knows_tech("fabrics"), "тема доведена до конца");
    assert_eq!(sim.item_at(3, 1, 2), 2, "выход лёг кучей в лаборатории");
}

/// Отмена роняет завезённое кучей (§12.31): материал не горит никогда.
#[test]
fn cancelling_a_specimen_topic_drops_it_back() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(2);
    let topic = sim.set_topic("fabrics", 0, 4000, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[]);
    sim.put_item(5, 1, 1, 1);
    sim.tick_n(1);
    sim.start_research(topic);
    sim.tick_n(40);
    assert_eq!(sim.topic_delivered(1), 1);

    assert!(sim.cancel_research(topic));

    assert_eq!(
        sim.item_at(3, 1, 1),
        1,
        "образец вернулся кучей в лабораторию"
    );
}

/// Главный случай §12.139: комбинезоны разобраны котами по себе — заявку не
/// принимают, хотя предмет базе знаком и в шапке числится.
///
/// Это **отмена** прежнего правила (§12.133), где такая тема заводилась и
/// вставала намертво, держа ячейку лаборатории и учёного.
#[test]
fn worn_gear_does_not_count_as_a_specimen() {
    let (mut sim, _) = sim_with_lab();
    sim.set_items(2);
    sim.set_force(1, 1);
    sim.set_loadout(&[1]);
    let topic = sim.set_topic("fabrics", 0, 400, &[], &[]);
    sim.set_specimen(topic, &[(1, 1)], &[]);

    // Один комбинезон на двух котов: его наденут, и на складе не останется.
    sim.put_item(5, 1, 1, 1);
    sim.tick_n(30);
    assert_eq!(sim.item_total(1), 0, "комбинезон надет");
    assert!(!sim.gear_of("a").is_empty() || !sim.gear_of("b").is_empty());

    assert!(
        !sim.start_research(topic),
        "надетое образцом не считается: везти в лабораторию нечего",
    );
}

/// Шаблон снаряжения уступает **заведённой** теме, но не открытой (§12.115).
///
/// Два теста в одном намеренно: граница проходит ровно между ними, и порознь
/// они читались бы как два независимых правила.
#[test]
fn the_loadout_yields_to_a_started_topic_and_not_to_an_open_one() {
    let outfit = |started: bool| {
        let (mut sim, _) = sim_with_lab();
        sim.set_items(2);
        sim.set_force(1, 1);
        sim.set_loadout(&[1]);
        let topic = sim.set_topic("fabrics", 0, 4000, &[], &[]);
        sim.set_specimen(topic, &[(1, 1)], &[]);
        sim.put_item(5, 1, 1, 1);
        sim.tick_n(1);
        if started {
            assert!(sim.start_research(topic), "тема заводится");
        }
        sim.tick_n(40);
        (
            sim.gear_of("a").len() + sim.gear_of("b").len(),
            sim.topic_delivered(1),
        )
    };

    let (worn_when_open, _) = outfit(false);
    let (worn_when_started, delivered) = outfit(true);

    assert_eq!(
        worn_when_open, 1,
        "тема только открыта — комбинезон уходит на кота, шаблон важнее",
    );
    assert_eq!(
        (worn_when_started, delivered),
        (0, 1),
        "тема заведена — комбинезон едет в лабораторию, а не на кота",
    );
}

// --- лаборатория как ячейка (§12.132) ----------------------------------------
//
// Идиома §12.96 дословно: тема рождается **в ячейке** и держит её до последнего
// очка работы. Отличие от заказа ровно одно — двух тем на один `def` не бывает
// никогда (тема одноразова и необратима, §12.18), поэтому отменяют её по теме,
// а не по клетке.

/// Коридор с **двумя** лабораториями: (3,1) и (4,1).
fn sim_with_two_labs() -> Sim {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_skill("science", &[100, 400]);
    sim.set_lab(1, true);
    sim.force_tile(3, 1, 1);
    sim.force_tile(4, 1, 1);
    sim.set_capacity(2, 100);
    sim.force_tile(6, 1, 2);
    sim
}

/// Вторая лаборатория даёт вторую работу, а не декорацию (§12.55).
#[test]
fn two_labs_run_two_topics_at_once() {
    let mut sim = sim_with_two_labs();
    let first = sim.set_topic("materials", 0, 4000, &[], &[]);
    let second = sim.set_topic("comfort", 0, 4000, &[], &[]);

    assert!(sim.start_research(first));
    assert!(sim.start_research(second), "вторая комната — вторая тема");
    sim.tick_n(10);

    assert_eq!(sim.topics_count(), 2, "обе темы живы");
    assert_eq!(sim.researchers_busy(), 2, "и обе с исполнителями");
}

/// Одного кота на две темы не хватает, и это не значит «обе стоят»: раздатчик
/// откладывает `commands` до конца тика, поэтому на втором витке цикла занятый
/// учёный выглядел бы свободным и получил бы вторую тему разом с первой — та
/// навсегда осталась бы «с исполнителем», который к ней не придёт (§12.132,
/// дословно грабли `assign_craft`, §12.96).
#[test]
fn one_scientist_is_never_seated_at_two_topics_at_once() {
    let mut sim = sim_from(&["########", "#a.....#", "########"]);
    sim.set_skill("science", &[100, 400]);
    sim.set_lab(1, true);
    sim.force_tile(3, 1, 1);
    sim.force_tile(4, 1, 1);
    let first = sim.set_topic("materials", 0, 4000, &[], &[]);
    let second = sim.set_topic("comfort", 0, 4000, &[], &[]);

    assert!(sim.start_research(first));
    assert!(sim.start_research(second));
    sim.tick_n(10);

    assert_eq!(sim.researchers_busy(), 1, "кот один — и тема у него одна");
    assert!(
        sim.research_progress().is_some_and(|p| p > 0),
        "и он над ней работает, а не стоит между двумя"
    );
}

/// Свободной ячейки нет — заявка **отклоняется**, а не встаёт в очередь, и
/// склад при этом не трогают: очередь тем была бы вторым планировщиком рядом с
/// раздатчиком (§12.16).
#[test]
fn a_second_topic_without_a_free_lab_is_refused() {
    let (mut sim, _) = sim_with_lab();
    let first = sim.set_topic("materials", 0, 4000, &[], &[]);
    let second = sim.set_topic("comfort", 0, 4000, &[(0, 3)], &[]);
    sim.put_item(5, 1, 0, 10);

    assert!(sim.start_research(first));
    assert!(!sim.start_research(second), "комната одна");
    assert_eq!(sim.item_at(5, 1, 0), 10, "и склад не тронут");
}

/// Одну тему не берут дважды даже при свободной второй комнате: она одноразова.
#[test]
fn the_same_topic_is_never_taken_twice() {
    let mut sim = sim_with_two_labs();
    let topic = sim.set_topic("materials", 0, 4000, &[], &[]);

    assert!(sim.start_research(topic));
    assert!(!sim.start_research(topic), "второй раз не берутся");
    assert_eq!(sim.topics_count(), 1);
}

/// Спящий учёный **не отдаёт ячейку**: комната принадлежит теме, а не задаче
/// кота (§12.132) — дословно как станок принадлежит заказу (§12.96).
#[test]
fn a_sleeping_scientist_keeps_the_lab_for_the_topic() {
    let (mut sim, _) = sim_with_lab();
    let first = sim.set_topic("materials", 0, 4000, &[], &[]);
    sim.start_research(first);
    sim.tick_n(6);
    let cell = sim.topic_cell().expect("тема в комнате");

    // Уводим исполнителя усталостью — задачу отбирает `release_work`.
    sim.set_needs(1000, 900, 1);
    sim.set_energy("a", 1);
    sim.set_energy("b", 1);
    sim.tick_n(4);

    assert!(sim.researcher().is_none(), "исполнителя увели");
    assert_eq!(sim.topic_cell(), Some(cell), "но комната осталась за темой");

    let second = sim.set_topic("comfort", 0, 4000, &[], &[]);
    assert!(!sim.start_research(second), "и второй теме её не отдадут");
}

/// Отменяют тему **по ней самой**, а не по клетке: соседняя тема цела.
#[test]
fn cancelling_names_the_topic_and_leaves_the_other_alone() {
    let mut sim = sim_with_two_labs();
    let first = sim.set_topic("materials", 0, 4000, &[], &[]);
    let second = sim.set_topic("comfort", 0, 4000, &[], &[]);
    sim.start_research(first);
    sim.start_research(second);
    sim.tick_n(6);

    assert!(sim.cancel_research(second));

    assert_eq!(sim.topics_count(), 1, "осталась одна");
    assert!(!sim.start_research(first), "и это та, которую не трогали");
    // А брошенную можно завести заново: комната освободилась вместе с ней.
    assert!(sim.start_research(second));
}

/// Учёного увели на вылазку: тема отпускается и её подхватывает другой
/// допущенный кот — ровно как после приказа игрока.
///
/// Заявка на вылазку — не приказ и не истощение, но освобождение у неё то же
/// (`release_task`, инвариант 7). Пропусти его — и тема осталась бы за котом,
/// которого на базе нет, то есть зависла бы до его возвращения при живом
/// втором учёном рядом.
#[test]
fn a_raid_frees_the_topic_for_the_others() {
    let (mut sim, science) = sim_with_lab();
    // Оба кота проходят допуск: проверяем именно передачу темы, а не то, что
    // единственный учёный ушёл.
    sim.set_xp("a", science, 100);
    sim.set_xp("b", science, 100);
    let topic = sim.set_topic("materials", 1, 4000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    assert_eq!(sim.researcher().as_deref(), Some("a"), "взялся ближний");
    let progress = sim.research_progress().unwrap();

    sim.set_gate(2, true);
    sim.force_tile(7, 1, 2);
    sim.set_relay(2, true);
    let mission = sim.set_mission(1, 400, &[]);
    assert!(sim.launch(mission, vec!["a".into()]), "отряд ушёл");

    assert_eq!(sim.researcher(), None, "тема отпущена в тот же миг");
    assert_eq!(sim.research_progress(), Some(progress), "прогресс цел");

    sim.tick_n(20);
    assert_eq!(
        sim.researcher().as_deref(),
        Some("b"),
        "и её подхватил второй допущенный кот",
    );
}

/// А если допущенный кот на базе один и он ушёл — тема **ждёт**, и это не
/// зависание: она стоит в своей комнате с прежним прогрессом, и вернувшийся
/// учёный садится за неё сам.
#[test]
fn a_topic_waits_out_the_only_scientist_and_resumes() {
    let (mut sim, science) = sim_with_lab();
    sim.set_xp("a", science, 100);
    let topic = sim.set_topic("materials", 1, 4000, &[], &[]);
    sim.start_research(topic);
    sim.tick_n(6);
    let progress = sim.research_progress().unwrap();

    sim.set_gate(2, true);
    sim.force_tile(7, 1, 2);
    sim.set_relay(2, true);
    let mission = sim.set_mission(1, 60, &[]);
    sim.launch(mission, vec!["a".into()]);

    sim.tick_n(30);
    assert!(sim.is_away("a"), "учёный ещё в поле");
    assert_eq!(sim.researcher(), None, "браться некому");
    assert_eq!(sim.research_progress(), Some(progress), "но тема цела");
    assert!(
        !sim.topic_has_scientist_home(),
        "и панель обязана сказать это словом, а не «ждёт исполнителя» (§12.135)",
    );

    sim.tick_n(120);
    assert!(!sim.is_away("a"), "вернулся");
    assert!(
        sim.research_progress().is_some_and(|p| p > progress),
        "и работа пошла дальше сама",
    );
}
