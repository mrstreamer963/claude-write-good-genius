//! Врождённые параметры кота (§12.19, §12.42).
//!
//! Параметр — не вторая шкала мастерства, а предел: докуда кот вырастет в
//! домене. Проверяем ровно это — что предел режет опыт (а не показанный
//! уровень), что он одинаково держит и работу, и парту, что сам он не растёт
//! ни от чего и что первый уровень не закрыт никогда.
//!
//! Параметров в схеме `sim_from` нет, как нет навыков и потребностей: их
//! включает сам тест (`set_demands`, `set_stat`) — тесты чужих механик о
//! врождённом знать не должны.

use super::*;

const CORRIDOR: [&str; 3] = ["#########", "#a.....b#", "#########"];

/// Пять уровней по пять очков: каждый набирается меньше чем за тайл, поэтому
/// упереться в предел кот успевает на одной рамке.
const LEVELS: [i32; 5] = [5, 10, 15, 20, 25];

/// Коридор, домен «Стройка» с пятью уровнями и пределом по параметру 0
/// (пороги [3, 5, 7, 9] — те же, что в боевом рулсете). Вернёт мир и домен.
fn sim_with_ceiling() -> (Sim, usize) {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_demands(build, 0, &[3, 5, 7, 9]);
    (sim, build)
}

// --- предел ----------------------------------------------------------------

/// Базовый случай: параметра хватает на третий уровень — там опыт и встаёт.
/// Режется именно опыт, а не показанный уровень: иначе кот годами копил бы
/// очки, которые никогда ни во что не превратятся (§12.17).
#[test]
fn experience_stops_at_the_stat_ceiling() {
    let (mut sim, build) = sim_with_ceiling();
    sim.set_stat("a", 0, 6); // 3 и 5 взяты, 7 — нет
    sim.add_blueprint_rect(1, 2, 6, 1, 0); // работы заведомо больше, чем нужно

    sim.tick_n(400);
    assert_eq!(sim.floors_left([1, 2, 6, 1]), 6, "вся рамка построена");
    assert_eq!(sim.level_cap_of("a", build), 3, "предел — третий уровень");
    assert_eq!(sim.xp_of("a", build), 15, "опыт встал на его пороге");
    assert_eq!(sim.level_of("a", build), 3);
}

/// Контрольный случай: домен без параметра ограничен только потолком навыка.
/// Так живут «Ремесло» и «Медицина» — вход в них открыт всем и предела нет.
#[test]
fn an_unrestricted_domain_grows_to_the_skill_cap() {
    let mut sim = sim_from(&CORRIDOR);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_stat("a", 0, 1); // параметр есть, но домену он не назначен
    sim.add_blueprint_rect(1, 2, 6, 1, 0);

    sim.tick_n(400);
    assert_eq!(sim.level_cap_of("a", build), 5, "потолок навыка целиком");
    assert_eq!(sim.xp_of("a", build), 25, "и опыт дошёл до него");
}

/// Предел мягкий, а не запрет (§12.19): первый уровень параметром не закрыт
/// никогда, и даже кот без параметров работать умеет. Домен закрывается
/// допуском (§12.18), и живёт он у навыка.
#[test]
fn the_first_level_is_never_closed() {
    let (mut sim, build) = sim_with_ceiling();
    // Параметра у кота нет вовсе — ровно то, что бывает при неполном рулсете.
    sim.add_blueprint_rect(1, 2, 6, 1, 0);

    sim.tick_n(400);
    assert_eq!(sim.stat_of("a", 0), 0, "параметров у кота нет");
    assert_eq!(sim.level_cap_of("a", build), 1, "предел — первый уровень");
    assert_eq!(sim.level_of("a", build), 1, "но первый он взял");
    assert_eq!(sim.xp_of("a", build), 5, "и остановился ровно на нём");
}

/// Параметр не растёт **ни от чего**: иначе это второй навык под другим именем
/// — две шкалы двигаются одним действием, а решение игрок принимает одно.
#[test]
fn a_stat_never_grows_from_work() {
    let (mut sim, build) = sim_with_ceiling();
    sim.set_stat("a", 0, 4);
    sim.add_blueprint_rect(1, 2, 6, 1, 0);

    sim.tick_n(400);
    assert!(sim.xp_of("a", build) > 0, "работа была");
    assert_eq!(sim.stat_of("a", 0), 4, "а параметр не сдвинулся");
    assert_eq!(sim.level_cap_of("a", build), 2, "и предел остался прежним");
}

/// То, ради чего §12.19 и писалась: одна и та же работа разводит котов, а не
/// сводит их к одинаковым мастерам. Дыра §12.17 закрыта.
#[test]
fn two_cats_doing_the_same_work_end_up_different() {
    let (mut sim, build) = sim_with_ceiling();
    sim.set_stat("a", 0, 9);
    sim.set_stat("b", 0, 3);
    sim.add_blueprint_rect(1, 2, 7, 1, 0);

    sim.tick_n(600);
    assert_eq!(sim.level_of("a", build), 5, "жилистый дорос до потолка");
    assert_eq!(sim.level_of("b", build), 2, "а хилый встал на втором");
}

/// Упёршийся в предел кот — полноценный работник своего уровня, а не
/// отстранённый: параметр режет рост, а не саму работу.
#[test]
fn a_capped_cat_still_works_at_his_level() {
    let (mut sim, build) = sim_with_ceiling();
    sim.set_stat("a", 0, 3);
    sim.set_xp("a", build, 10); // сразу на своём пределе
    assert_eq!(sim.level_of("a", build), 2);

    // Второй уровень — это +2 очка работы за тик к базовым 10, то есть тайл
    // за 10 тиков вместо 12.
    sim.add_blueprint(1, 2, 0);
    sim.tick_n(10);
    assert_eq!(sim.tile(1, 2), 0, "предел скорости не отнял");
}

// --- парта -----------------------------------------------------------------

/// Парта упирается в тот же предел: тупому коту она помогает **меньше**, а не
/// дольше. Иначе обучение обходило бы врождённое, и параметр значил бы только
/// «сколько ты сам наработаешь».
#[test]
fn the_desk_stops_at_the_stat_ceiling() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &LEVELS);
    sim.set_taught(science, 3); // парта доводит до третьего
    sim.set_demands(science, 0, &[3, 5, 7, 9]);
    sim.set_teaches(1, science);
    sim.force_tile(3, 1, 1);
    sim.set_stat("a", 0, 4); // а параметр пускает только до второго

    assert!(sim.teach("a", "science"), "учиться ему ещё есть чему");
    sim.tick_n(60);
    assert_eq!(sim.xp_of("a", science), 10, "встал на пороге второго");
    assert_eq!(sim.level_of("a", science), 2);
    assert!(!sim.is_studying("a"), "и ушёл с парты сам");
}

/// Того, кому парта уже ничего не даст, за неё не отправляют: он встал бы с
/// неё в тот же тик, а игрок прочёл бы это как поломку.
#[test]
fn teaching_is_refused_at_the_stat_ceiling() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    let science = sim.set_skill("science", &LEVELS);
    sim.set_taught(science, 3);
    sim.set_demands(science, 0, &[3, 5, 7, 9]);
    sim.set_teaches(1, science);
    sim.force_tile(3, 1, 1);
    sim.set_stat("a", 0, 4);
    sim.set_xp("a", science, 10); // уже на своём пределе

    assert!(!sim.teach("a", "science"), "выше предела парта не учит");
    assert!(!sim.is_studying("a"));
}

// --- найм ------------------------------------------------------------------

/// Новичок собирается тем же `spawn_cat`, что и стартовая тройка (§12.24), —
/// значит и параметры у него настоящие, а не «как у всех».
#[test]
fn a_recruit_arrives_with_his_own_ceiling() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_demands(build, 0, &[3, 5, 7, 9]);
    sim.set_gate(0, true);
    sim.set_relay(0, true);
    let nail = sim.set_recruit("nail", 0, &[], &[(build, 5)]);
    sim.set_recruit_stats(nail, &[(0, 9)]);

    assert!(sim.hire(nail), "известности хватает, платить нечем не надо");
    assert!(sim.has_unit("nail"), "новичок на базе");
    assert_eq!(sim.stat_of("nail", 0), 9, "и с обещанным параметром");
    assert_eq!(
        sim.level_cap_of("nail", build),
        5,
        "предел — потолок навыка"
    );
}

/// Стартовый опыт кандидата предел не отменяет, но и не режет задним числом:
/// его прошлое — это контент рулсета, а предел — правило на будущее.
#[test]
fn a_recruit_keeps_the_experience_he_was_promised() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let build = sim.set_skill("build", &LEVELS);
    sim.set_demands(build, 0, &[3, 5, 7, 9]);
    sim.set_gate(0, true);
    sim.set_relay(0, true);
    let brick = sim.set_recruit("brick", 0, &[], &[(build, 20)]);
    sim.set_recruit_stats(brick, &[(0, 9)]);

    assert!(sim.hire(brick));
    assert_eq!(sim.xp_of("brick", build), 20, "пришёл с обещанным опытом");
    assert_eq!(sim.level_of("brick", build), 4);
}

// --- боевой рулсет ---------------------------------------------------------

/// Проверка на настоящем `core.yaml`: у каждого кота есть параметры, которых
/// требуют домены. Ловит рассогласование кода и контента, которого синтетика не
/// увидит: забытый у юнита блок `stats:` не падает, а молча запирает кота на
/// первом уровне — и выглядит это как «навык почему-то не растёт».
#[test]
fn the_shipped_ruleset_gives_every_cat_a_real_ceiling() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).expect("рулсет должен разбираться");

    let capped: Vec<usize> = {
        let rules = sim.world.resource::<SkillRules>();
        (0..rules.0.len())
            .filter(|&i| !rules.0[i].demands.is_empty())
            .collect()
    };
    assert!(!capped.is_empty(), "хоть один домен ограничен параметром");

    for &skill in &capped {
        for cat in ["excellent", "sp2", "sp3"] {
            assert!(
                sim.level_cap_of(cat, skill) >= 2,
                "{cat} заперт на первом уровне домена {skill}: забыт параметр"
            );
        }
    }
}

/// Пороги параметра не длиннее лестницы уровней: требование к 6-му уровню у
/// пятиуровневого домена — мёртвый контент, который никто никогда не увидит.
#[test]
fn the_shipped_ruleset_demands_nothing_past_the_ceiling() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let sim = Sim::new(yaml).expect("рулсет должен разбираться");

    let rules = sim.world.resource::<SkillRules>();
    for rule in &rules.0 {
        assert!(
            rule.demands.len() < rule.levels.len().max(1),
            "у домена {} порогов параметра больше, чем уровней",
            rule.id
        );
        assert_eq!(
            rule.stat.is_some(),
            !rule.demands.is_empty(),
            "у домена {} параметр и его пороги заданы порознь",
            rule.id
        );
    }
}

/// Кандидат приходит с опытом, который его же параметр позволяет: иначе игрок
/// платит складом за кота, уже стоящего выше собственного предела, — и первый
/// же тик работы этот опыт никуда не двинет.
#[test]
fn the_shipped_ruleset_hires_no_one_above_his_own_ceiling() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let sim = Sim::new(yaml).expect("рулсет должен разбираться");

    let rules = sim.world.resource::<SkillRules>();
    for recruit in &sim.world.resource::<RecruitRules>().0 {
        let mut born = Stats::default();
        for &(stat, value) in &recruit.stats {
            born.set(stat, value);
        }
        for &(skill, xp) in &recruit.skills {
            assert!(
                rules.level(skill, xp) <= crate::skills::level_cap_of(rules, Some(&born), skill),
                "{} приходит выше своего предела в домене {skill}",
                recruit.id
            );
        }
    }
}

/// До чего доводит парта, должно быть кому взять: если врождённое у всей
/// стартовой тройки ниже `taught`, наука недостижима, а класс на базе стоит зря.
#[test]
fn the_shipped_ruleset_keeps_the_desk_within_reach() {
    let yaml = include_str!("../../assets/rulesets/core.yaml");
    let mut sim = Sim::new(yaml).expect("рулсет должен разбираться");

    let taught: Vec<(usize, i32)> = {
        let rules = sim.world.resource::<SkillRules>();
        (0..rules.0.len())
            .filter(|&i| rules.0[i].taught > 0)
            .map(|i| (i, rules.0[i].taught))
            .collect()
    };
    assert!(!taught.is_empty(), "хоть одному домену учат");

    for (skill, level) in taught {
        assert!(
            ["excellent", "sp2", "sp3"]
                .iter()
                .any(|cat| sim.level_cap_of(cat, skill) >= level),
            "парте домена {skill} некого доводить до {level}-го уровня"
        );
    }
}

/// **Бюджет врождённого один на всех** (§12.70, §12.141).
///
/// Кот — это **форма**, а не ступень качества: по одной шкале «лучше/хуже»
/// выбирать нечего, всегда бери верхнего. Держится это ровно на том, что сумма
/// параметров у всех одна, — и до §12.141 держалось молча, дисциплиной автора
/// рулсета. Кандидатов стало вдвое больше, и молчаливое соглашение о шести
/// записях уже не соглашение, а надежда.
///
/// Единица допуска — не щедрость, а факт: у «Гвоздя» 19 против 18 у остальных.
/// Двойка означала бы уже ранг.
#[test]
fn the_shipped_ruleset_gives_every_cat_the_same_budget() {
    let mut sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let mut budgets: Vec<(String, i32)> = {
        let mut q = sim.world.query::<(&UnitId, &Stats)>();
        q.iter(&sim.world)
            .map(|(id, stats)| (id.0.clone(), stats.0.iter().sum()))
            .collect()
    };
    for r in &sim.world.resource::<RecruitRules>().0 {
        budgets.push((r.id.clone(), r.stats.iter().map(|&(_, v)| v).sum()));
    }
    assert!(budgets.len() > 1, "сверять нечего: кот в рулсете один");

    let lo = budgets.iter().map(|b| b.1).min().expect("непустой список");
    let hi = budgets.iter().map(|b| b.1).max().expect("непустой список");
    assert!(
        hi - lo <= 1,
        "бюджет врождённого разъехался ({lo}…{hi}) — это лестница качества, а не \
         набор форм: {budgets:?}",
    );
}
