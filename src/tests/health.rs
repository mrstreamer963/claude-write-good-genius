//! Здоровье: ранение на вылазке, лазарет и работа медика (§12.5, §12.37).
//!
//! Ранений в схеме по умолчанию нет — их включает сам тест (`set_health_rules`),
//! как усталость и голод. Койка лазарета делается вручную: тайлу задаётся
//! скорость заживления (`set_heal`), и нужная клетка переводится в него через
//! `force_tile`.
//!
//! Урон приходит только с вылазки, поэтому половина тестов гоняет полный
//! прогон миссии; остальные ставят рану напрямую (`set_health`), как тесты сна
//! ставят бодрость.

use super::*;

const CORRIDOR: [&str; 3] = ["#########", "#a......#", "#########"];

/// Тайл, которым в этих тестах занимают котов стройкой: пол схемы — уже `0`,
/// и чертёж на нём отсекается как «уже построено».
const OTHER: i32 = 2;

/// Мир со шлюзом (тайл 1) в правом конце коридора и заведённой миссией.
fn sim_with_raid(
    rows: &[&str],
    gate: (i32, i32),
    squad: usize,
    danger: i32,
    harm: i32,
) -> (Sim, usize) {
    let mut sim = sim_from(rows);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(gate.0, gate.1, 1);
    let mission = sim.set_risky_mission(squad, 10, danger, 0, &[(0, 5)]);
    sim.set_mission_harm(mission, harm);
    sim.set_health_rules(100, 60, 1);
    (sim, mission)
}

// --- откуда берутся раны ---------------------------------------------------

/// Дотикать до возвращения отряда — и ни тиком дольше: лежачий кот заживает
/// каждый тик, и лишние шаги смазали бы величину урона.
fn tick_until_back(sim: &mut Sim, unit: &str) {
    for _ in 0..200 {
        sim.tick();
        if sim.in_squad(unit) && !sim.is_away(unit) {
            continue; // ещё идёт к шлюзу
        }
        if !sim.in_squad(unit) && !sim.is_away(unit) {
            return;
        }
    }
    panic!("отряд так и не вернулся");
}

/// Провальная вылазка ранит весь отряд и кладёт его лечиться.
#[test]
fn a_failed_raid_wounds_the_whole_squad() {
    let (mut sim, m) = sim_with_raid(&["#######", "#a...b#", "#######"], (3, 1), 2, 100, 50);
    assert!(sim.launch(m, vec!["a".to_string(), "b".to_string()]));

    tick_until_back(&mut sim, "a");
    // Ровно `harm`: лечь кот успеет только следующим тиком — раздатчики идут до
    // работ, а рану приносит `run_missions` уже после них.
    assert_eq!(sim.health_of("a"), 50, "провал снял весь harm");
    assert_eq!(sim.health_of("b"), 50);
    sim.tick();
    assert!(sim.is_healing("a") && sim.is_healing("b"), "оба выбыли");
}

/// Полный успех не царапает никого: урон считается той же долей, что и добыча.
#[test]
fn a_won_raid_leaves_everyone_whole() {
    let (mut sim, m) = sim_with_raid(&["#######", "#a...b#", "#######"], (3, 1), 2, 2, 50);
    sim.launch(m, vec!["a".to_string(), "b".to_string()]);

    sim.tick_n(30);
    assert_eq!(sim.health_of("a"), 100, "силы хватило — ран нет");
    assert!(!sim.is_healing("a"), "и лечиться незачем");
    assert_eq!(sim.item_at(3, 1, 0), 5, "добыча при этом полная");
}

/// Половина силы — половина добычи и **ровно половина** урона: одно выражение
/// исхода на обе стороны, иначе прогноз в панели врал бы (§12.23).
#[test]
fn a_half_raid_wounds_by_the_same_share() {
    let (mut sim, m) = sim_with_raid(&["#######", "#a...b#", "#######"], (3, 1), 2, 4, 40);
    sim.launch(m, vec!["a".to_string(), "b".to_string()]);

    sim.tick_n(30);
    assert_eq!(sim.item_at(3, 1, 0), 2, "добычи половина (5 * 50%)");
    assert_eq!(sim.health_of("a"), 80, "и урона половина");
    assert!(!sim.is_healing("a"), "порога это не достало — кот в строю");
}

// --- что делает раненый ----------------------------------------------------

/// Ранение **срывает начатое**, в отличие от усталости, — и обязано освободить
/// чертёж: иначе площадка навсегда останется за лежачим.
#[test]
fn a_wounded_cat_drops_its_job() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_health_rules(100, 60, 1);
    sim.add_blueprint(3, 1, OTHER);

    sim.tick_n(4);
    assert!(sim.has_assignment("a"), "кот взялся строить");

    sim.set_health("a", 10);
    sim.tick_n(2);
    assert!(!sim.has_assignment("a"), "ранение сорвало работу");
    assert!(sim.is_healing("a"), "и уложило кота");

    sim.set_health("a", 100);
    sim.tick_n(20);
    assert!(
        sim.has_assignment("a") || sim.tile(3, 1) == OTHER as i16,
        "выздоровев, кот вернулся к тому же чертежу"
    );
}

/// Раненого не берут на вылазку: выбывший — это и есть цена провала.
#[test]
fn a_wounded_cat_is_not_taken_on_a_raid() {
    let (mut sim, m) = sim_with_raid(&["#######", "#a...b#", "#######"], (3, 1), 2, 0, 0);
    sim.set_health("a", 10);

    assert!(
        sim.launch(m, vec!["a".to_string(), "b".to_string()]),
        "вылазка уходит недокомплектом (§12.113)",
    );
    assert!(!sim.in_squad("a"), "но раненого в отряд не взяли");
    assert!(sim.in_squad("b"), "ушёл целый");
}

/// Раны затягиваются и без лазарета — просто медленно; на полной шкале кот
/// встаёт сам. Ноль скорости не берётся никогда, иначе базa без койки была бы
/// тупиком.
#[test]
fn wounds_close_without_a_ward() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_health_rules(100, 60, 1);
    sim.set_health("a", 50);

    sim.tick_n(5);
    assert!(sim.is_healing("a"), "кот лежит");
    assert!(sim.health_of("a") > 50, "и заживает прямо на полу");

    sim.tick_n(60);
    assert_eq!(sim.health_of("a"), 100, "долечился до полной");
    assert!(!sim.is_healing("a"), "и встал");
}

/// Койка лазарета лечит быстрее пола, и раненый доходит до неё сам.
#[test]
fn a_wounded_cat_walks_to_the_ward() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_heal(1, 10);
    sim.force_tile(7, 1, 1);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);

    sim.tick_n(20);
    assert_eq!(sim.pos_of("a"), (7, 1), "дошёл до койки");
    assert_eq!(sim.ward_of("a"), Some((7, 1)), "и занял её");
    let before = sim.health_of("a");
    sim.tick_n(10);
    assert!(
        sim.health_of("a") - before >= 100,
        "на койке заживает вдесятеро быстрее пола"
    );
}

/// Койку занимает **один** кот: делить её нельзя, иначе число коек ни на что
/// не влияет (§12.20).
#[test]
fn a_ward_bed_is_taken_by_one_cat() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_heal(1, 10);
    sim.force_tile(4, 1, 1);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);
    sim.set_health("b", 100);

    sim.tick_n(20);
    let beds = [sim.ward_of("a"), sim.ward_of("b")];
    assert_eq!(
        beds.iter().filter(|b| b.is_some()).count(),
        1,
        "койка досталась ровно одному"
    );
    assert!(sim.is_healing("a") && sim.is_healing("b"), "лежат оба");
}

/// Лежачий на полу перебирается на койку, как только та появилась, — тот же
/// третий проход, что у сна (§12.33).
#[test]
fn a_floored_patient_moves_to_a_new_ward() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);

    sim.tick_n(3);
    assert!(sim.is_healing("a"), "лёг где стоял");
    assert_eq!(sim.ward_of("a"), None, "койки не было");

    sim.set_heal(1, 10);
    sim.force_tile(6, 1, 1); // лазарет достроили
    sim.tick_n(20);
    assert_eq!(sim.ward_of("a"), Some((6, 1)), "переехал на койку");
    assert_eq!(sim.pos_of("a"), (6, 1));
}

// --- медик -----------------------------------------------------------------

/// Свободный кот приходит к раненому и лечит его быстрее, чем тот заживает сам;
/// «Медицина» при этом растёт от самой работы.
#[test]
fn a_medic_comes_and_speeds_the_healing() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    let medicine = sim.set_skill("medicine", &[10, 100]);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);

    sim.tick_n(20);
    assert!(sim.is_healing("a"), "раненый лежит");
    assert!(sim.is_treating("b"), "сосед взялся лечить");
    assert_eq!(sim.medic_of("a").as_deref(), Some("b"), "и записан за ним");
    assert!(
        (sim.pos_of("b").0 - sim.pos_of("a").0).abs()
            + (sim.pos_of("b").1 - sim.pos_of("a").1).abs()
            == 1,
        "работает с соседней клетки, а не из клетки пациента"
    );

    let before = sim.health_of("a");
    sim.tick_n(10);
    assert!(
        sim.health_of("a") - before >= 20,
        "с медиком заживает быстрее, чем 1 за тик"
    );
    assert!(sim.xp_of("b", medicine) > 0, "и навык медика растёт");
}

/// Выздоровевший отпускает медика, и тот возвращается к обычной работе.
#[test]
fn a_healed_cat_frees_its_medic() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_health_rules(100, 60, 1);
    sim.set_health("a", 50);
    sim.add_blueprint(4, 2, OTHER);

    sim.tick_n(10);
    assert!(sim.is_treating("b"), "сперва лечит");

    sim.tick_n(60);
    assert!(!sim.is_healing("a"), "раненый встал");
    assert!(!sim.is_treating("b"), "медик свободен");
    assert!(
        sim.has_assignment("b") || sim.tile(4, 2) == OTHER as i16,
        "и взялся за чертёж"
    );
}

/// Приказ игрока уводит медика, а пациент замечает потерю сам: claim чинится в
/// одном месте, а не в каждом, кто отбирает работу (§12.37).
#[test]
fn an_order_releases_the_medic_and_the_claim() {
    let mut sim = sim_from(&["#########", "#a..b..c#", "#########"]);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);

    sim.tick_n(10);
    assert_eq!(sim.medic_of("a").as_deref(), Some("b"), "ближайший взялся");

    sim.set_target("b", 7, 1); // игрок увёл медика на другой конец
    sim.tick_n(2);
    assert!(!sim.is_treating("b"), "приказ снял лечение");
    sim.tick_n(10);
    assert_eq!(
        sim.medic_of("a").as_deref(),
        Some("c"),
        "и пациента подобрал другой кот"
    );
}

/// Раненый не идёт в медики: лечить, лёжа самому, нечем.
#[test]
fn a_wounded_cat_does_not_treat_others() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_health_rules(1000, 600, 1);
    sim.set_health("a", 100);
    sim.set_health("b", 100);

    sim.tick_n(20);
    assert!(sim.is_healing("a") && sim.is_healing("b"), "лежат оба");
    assert!(
        !sim.is_treating("a") && !sim.is_treating("b"),
        "и никто никого не лечит"
    );
}

// --- боевой рулсет ---------------------------------------------------------

/// Контентная проверка: у боевого рулсета есть шкала здоровья, лазарет с
/// койкой, домен «Медицина» и урон у каждой вылазки, а порог выбывания лежит
/// ниже потолка. Синтетические схемы этого не увидят — там всё задаётся руками.
#[test]
fn the_shipped_ruleset_can_treat_its_wounded() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let (max, hurt, mend) = {
        let rules = sim.world.resource::<crate::components::HealthRules>();
        (rules.max, rules.hurt, rules.mend)
    };
    assert!(max > 0, "здоровье в рулсете включено");
    assert!(
        hurt > 0 && hurt < max,
        "порог выбывания ниже потолка, иначе коты выбывают сразу или никогда"
    );
    assert!(mend > 0, "раны заживают и без лазарета");

    assert!(
        sim.skill_index("medicine").is_some(),
        "домен «Медицина» есть под тем же id, что знает код"
    );

    let wards: Vec<i16> = {
        let rules = sim.world.resource::<crate::components::TileRules>();
        (0..rules.0.len() as i16)
            .filter(|&t| rules.heal_of(t) > 0)
            .collect()
    };
    assert!(!wards.is_empty(), "в палитре есть койка лазарета");
    for tile in wards {
        assert!(
            sim.capacity_of(tile) == 0,
            "койка не совмещена со складом: лечиться и хранить в одной клетке нельзя"
        );
    }

    // Провал самой лёгкой вылазки обязан **выводить кота из строя**: иначе
    // ранение — это украшение, а не цена (§12.37).
    let (danger, harm) = {
        let rules = sim.world.resource::<crate::components::MissionRules>();
        let first = rules.0.first().expect("хотя бы одна вылазка");
        (first.danger, first.harm)
    };
    assert!(
        danger > 0 && harm > 0,
        "у первой вылазки есть и риск, и урон"
    );
    assert!(
        max - harm <= hurt,
        "провал первой вылазки кладёт кота в лазарет, а не царапает"
    );
}

// --- аптечка (§12.47) -------------------------------------------------------

/// Аптечка режет срок лечения, а склад платит за неё один раз на раненого.
///
/// Мир один и тот же, разница только в непустом складе — иначе сравнивались бы
/// два разных мира, а не наличие аптечки.
#[test]
fn a_medkit_speeds_the_healing_and_is_spent_once() {
    const KIT: usize = 0;

    let healed = |with_kit: bool| {
        let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
        sim.set_health_rules(1000, 600, 1);
        sim.set_health("a", 100);
        // Склад с аптечками: платит он, а не пол под ногами (§12.47).
        sim.set_capacity(1, 20);
        sim.force_tile(4, 1, 1);
        sim.set_mends(KIT, 8);
        if with_kit {
            sim.put_item(4, 1, KIT, 3);
        }

        sim.tick_n(30);
        assert!(sim.is_treating("b"), "медик взялся в обоих мирах");
        let start = sim.health_of("a");
        sim.tick_n(40);
        (sim.health_of("a") - start, sim.item_total(KIT))
    };

    let (without, _) = healed(false);
    let (with, left) = healed(true);

    assert!(
        with > without,
        "с аптечкой заживает быстрее: {with} против {without}",
    );
    assert_eq!(
        left, 2,
        "и потрачена ровно одна штука, а не по одной за тик"
    );
}

/// Главное свойство: аптечка **ускоряет, а не открывает**. Пустой склад — это
/// не тупик, а просто дольше (§12.37: базу, где выбывшего некому поднять, мы
/// не строим).
#[test]
fn healing_works_without_any_medkit() {
    let mut sim = sim_from(&["#########", "#a.....b#", "#########"]);
    sim.set_health_rules(100, 60, 1);
    sim.set_health("a", 50);
    sim.set_mends(0, 8); // аптечки в мире есть как понятие, но не на складе

    sim.tick_n(80);
    assert!(!sim.is_healing("a"), "раненый встал и без аптечки");
}

/// На боевом рулсете аптечку есть чем сделать, и она правда ускоряет.
///
/// Ловит контент, где `mends` забыли вовсе, рецепт ссылается на предмет не тем
/// `id`, или аптечка спрятана за технологией, до которой ещё дожить надо, —
/// раненые появляются раньше второй ступени науки.
#[test]
fn the_shipped_ruleset_can_bandage_the_wounded() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");

    let kits: Vec<(usize, i32)> = sim.world.resource::<ItemRules>().medkits().collect();
    let &(kit, mends) = kits.first().expect("в палитре нет ни одной аптечки");

    // Аптечка должна быть сравнима с койкой, иначе она украшение: с ней ставка
    // обязана заметно отличаться от лучшей койки без неё.
    let best_bed = sim
        .world
        .resource::<TileRules>()
        .0
        .iter()
        .map(|t| t.heal)
        .max()
        .unwrap_or(0);
    assert!(best_bed > 0, "в палитре нет лазарета");
    assert!(
        mends >= best_bed,
        "аптечка ({mends}) слабее койки ({best_bed}) — это украшение, а не расходник",
    );

    // Её должно быть чем сделать, и до рецепта должно быть можно дожить.
    let recipes = sim.world.resource::<CraftRules>();
    let recipe = recipes
        .0
        .iter()
        .find(|r| r.gives.iter().any(|&(item, n)| item == kit && n > 0))
        .expect("аптечку нечем сделать: нет рецепта, который её даёт");

    let topics = sim.world.resource::<ResearchRules>();
    for tech in &recipe.requires {
        assert!(
            topics.0.iter().any(|t| &t.id == tech),
            "рецепт аптечки закрыт технологией «{tech}», которой нет ни в одной теме",
        );
    }
}
