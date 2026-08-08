//! Цели партии (§12.58).
//!
//! Целей в схеме `sim_from` нет (`GoalRules` пуст), как нет вылазок и рынка:
//! цель — контент рулсета. Включают её `set_goal` и `set_hidden_goal`.
//!
//! Проверять здесь надо три вещи, и все три — про **разницу между состоянием и
//! поступком**:
//!   * взятая цель не снимается, когда условие перестало выполняться;
//!   * журнальное условие не засчитывает чужое (аптечку купили, а не сделали);
//!   * журнальное условие не теряет своё (заработал и потратил).
//! Остальное — арифметика счётчика и невидимость скрытой цели.

use super::*;

const SCRAP: usize = 0;
const PART: usize = 1;

/// Коридор со складом в (5,1); один кот `a`. Общий мир для целей на имущество.
fn sim_with_storage() -> Sim {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_items(2);
    sim.set_capacity(1, 100);
    sim.force_tile(5, 1, 1);
    sim
}

// --- условия-состояния ------------------------------------------------------

#[test]
fn a_built_tile_closes_its_goal() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let goal = sim.set_goal(GoalTest::Tile(1, 1));
    sim.tick_n(1);
    assert!(!sim.goal_taken(goal), "тайла ещё нет");

    sim.force_tile(3, 1, 1);
    sim.tick_n(1);
    assert!(sim.goal_taken(goal), "построили — цель взята");
}

#[test]
fn a_tile_goal_counts_cells() {
    let mut sim = sim_from(&["######", "#a...#", "######"]);
    let goal = sim.set_goal(GoalTest::Tile(1, 2));

    sim.force_tile(3, 1, 1);
    sim.tick_n(1);
    assert_eq!(sim.goal_progress(goal), Some((1, 2)), "одна из двух");
    assert!(!sim.goal_taken(goal));

    sim.force_tile(4, 1, 1);
    sim.tick_n(1);
    assert!(sim.goal_taken(goal), "вторая клетка закрыла цель");
}

/// Цель на склад мерит **склад**, а не всё имущество базы: платит именно он
/// (§12.24, §12.53). Куча на полу в счёт не идёт — иначе игрок видел бы полную
/// полоску и не понимал, почему цель не берётся.
#[test]
fn a_storage_goal_ignores_the_floor() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Stored(vec![(SCRAP, 10)]));

    sim.put_item(3, 1, SCRAP, 10); // на полу коридора
    sim.tick_n(1);
    assert_eq!(sim.goal_progress(goal), Some((0, 10)), "пол не склад");
    assert!(!sim.goal_taken(goal));

    sim.put_item(5, 1, SCRAP, 10); // на складе
    sim.tick_n(1);
    assert!(sim.goal_taken(goal), "склад закрыл цель");
}

/// Набор мерится по самому дальнему от цели предмету: узкое место — это и есть
/// то, что игроку надо знать.
#[test]
fn a_storage_goal_shows_the_worst_item() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Stored(vec![(SCRAP, 10), (PART, 10)]));

    sim.put_item(5, 1, SCRAP, 10);
    sim.put_item(5, 1, PART, 2);
    sim.tick_n(1);
    assert_eq!(
        sim.goal_progress(goal),
        Some((2, 10)),
        "деталь — узкое место"
    );
    assert!(!sim.goal_taken(goal), "набор покрыт не весь");
}

#[test]
fn a_tech_closes_its_goal() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let goal = sim.set_goal(GoalTest::Tech("materials".to_string()));
    sim.tick_n(1);
    assert!(!sim.goal_taken(goal));

    sim.set_tech("materials");
    sim.tick_n(1);
    assert!(sim.goal_taken(goal));
}

#[test]
fn a_cats_goal_counts_the_whole_base() {
    let mut sim = sim_from(&["######", "#ab..#", "######"]);
    let two = sim.set_goal(GoalTest::Cats(2));
    let three = sim.set_goal(GoalTest::Cats(3));
    sim.tick_n(1);

    assert!(sim.goal_taken(two), "двое есть");
    assert!(!sim.goal_taken(three), "третьего нет");
}

// --- взятая цель не снимается ----------------------------------------------

/// Главное свойство (§12.58): условие расходуемо, а поступок нет. Потраченный
/// лом не отнимает «Кладовую» — иначе галочка врала бы каждый раз, когда игрок
/// тратит то, что накопил, то есть постоянно.
#[test]
fn a_goal_survives_spending_what_earned_it() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Stored(vec![(SCRAP, 10)]));

    sim.put_item(5, 1, SCRAP, 10);
    sim.tick_n(1);
    assert!(sim.goal_taken(goal), "цель взята");

    sim.take_item(5, 1, SCRAP);
    sim.tick_n(2);
    assert!(sim.goal_taken(goal), "лом потрачен, а поступок остался");
    assert_eq!(sim.goals_taken(), 1, "и второй раз не засчиталась");
}

/// Цель отмечается тиком, а не «когда-нибудь»: панель разворачивает его в день.
#[test]
fn a_goal_remembers_the_tick_it_was_taken() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Stored(vec![(SCRAP, 1)]));

    sim.tick_n(5);
    assert_eq!(sim.goal_at(goal), None, "ещё не взята");

    sim.put_item(5, 1, SCRAP, 1);
    sim.tick_n(1);
    assert_eq!(sim.goal_at(goal), Some(6), "взята на шестом тике");
}

// --- условия-журналы --------------------------------------------------------

/// Ради этого условие и сделано журнальным (§12.58). Аптечка, попавшая на базу
/// со стороны — покупкой или добычей, — цель не закрывает: смысл цели в
/// мастерской, а «предмет есть на базе» этого не различает.
#[test]
fn a_craft_goal_ignores_goods_that_came_from_elsewhere() {
    let mut sim = sim_with_storage();
    let recipe = sim.set_recipe(200, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let goal = sim.set_goal(GoalTest::Craft(recipe));

    // Ровно то, что выдаёт рецепт, но принесённое извне.
    sim.put_item(5, 1, PART, 5);
    sim.tick_n(2);
    assert!(!sim.goal_taken(goal), "принесённое не считается сделанным");
    assert!(sim.crafted_done().is_empty(), "и в журнал не попало");
}

#[test]
fn a_craft_goal_closes_when_the_shop_makes_one() {
    let mut sim = sim_with_storage();
    sim.set_shop(2, true);
    sim.force_tile(3, 1, 2);
    let recipe = sim.set_recipe(20, &[(SCRAP, 2)], &[(PART, 1)], &[]);
    let goal = sim.set_goal(GoalTest::Craft(recipe));
    sim.put_item(5, 1, SCRAP, 10);

    assert!(sim.start_craft(recipe, 1));
    sim.tick_n(30);
    assert_eq!(sim.crafted_done(), vec![recipe], "штука сделана");
    assert!(sim.goal_taken(goal), "и цель взята");
}

/// Зеркало предыдущего теста с другой стороны: журнал не теряет **своё**.
/// `Money` тут не годится — продавший сотню и потративший её не увидел бы цели
/// никогда, хотя торговал дважды.
#[test]
fn an_earned_goal_is_not_reset_by_spending() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Earned(50));

    // Журнал и счёт двигаются порознь: заработок только растёт.
    sim.world.resource_mut::<Earned>().0 = 30;
    sim.set_money(30);
    sim.tick_n(1);
    assert_eq!(sim.goal_progress(goal), Some((30, 50)));

    sim.set_money(0); // всё потратили
    sim.world.resource_mut::<Earned>().0 = 60;
    sim.tick_n(1);
    assert!(
        sim.goal_taken(goal),
        "цель мерит заработанное, а не остаток"
    );
    assert_eq!(sim.money(), 0, "денег при этом нет");
}

// --- скрытая цель -----------------------------------------------------------

/// Скрытая цель не уходит наружу, пока не взята: прятать её в JS значит
/// объявить её в devtools (§12.28, §12.58).
#[test]
fn a_hidden_goal_stays_out_of_the_snapshot_until_taken() {
    let mut sim = sim_with_storage();
    let goal = sim.set_hidden_goal(GoalTest::Stored(vec![(SCRAP, 1)]));

    sim.tick_n(1);
    assert!(!sim.goal_is_visible(goal), "невзятая скрытая не видна");

    sim.put_item(5, 1, SCRAP, 1);
    sim.tick_n(1);
    assert!(sim.goal_taken(goal));
    assert!(sim.goal_is_visible(goal), "взятая — видна");
}

/// Скрытая не входит в знаменатель: партия проходится вчистую без неё.
#[test]
fn a_hidden_goal_is_not_counted_towards_the_finale() {
    let mut sim = sim_with_storage();
    sim.set_goal(GoalTest::Cats(1));
    sim.set_hidden_goal(GoalTest::Stored(vec![(SCRAP, 1)]));

    assert_eq!(sim.goals_required(), 1, "в счёт идёт только нескрытая");
}

// --- журналы пишутся в одном месте ------------------------------------------

/// Провал в журнал не идёт: «сходил и вернулся ни с чем» — не поступок, за
/// который дают цель (§12.58).
#[test]
fn a_failed_raid_is_not_logged() {
    let mut sim = sim_from(&["#######", "#ab...#", "#######"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(5, 1, 1);
    // Заведомо непосильная сложность: сила отряда 2, danger 100.
    let mission = sim.set_risky_mission(2, 2, 100, 0, &[(SCRAP, 4)]);
    let goal = sim.set_goal(GoalTest::Raid(mission));

    assert!(sim.launch(mission, vec!["a".into(), "b".into()]));
    sim.tick_n(40);
    assert!(sim.raids_done().is_empty(), "провал в журнал не пишется");
    assert!(!sim.goal_taken(goal), "и цель не берётся");
}

#[test]
fn a_successful_raid_closes_its_goal() {
    let mut sim = sim_from(&["#######", "#ab...#", "#######"]);
    sim.set_items(1);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(5, 1, 1);
    let mission = sim.set_mission(2, 2, &[(SCRAP, 4)]);
    let goal = sim.set_goal(GoalTest::Raid(mission));

    assert!(sim.launch(mission, vec!["a".into(), "b".into()]));
    sim.tick_n(40);
    assert_eq!(sim.raids_done(), vec![mission], "успех записан");
    assert!(sim.goal_taken(goal), "и цель взята");
}

// --- счётчик панели ---------------------------------------------------------

/// Панель и система считают **одним выражением** (§12.58, инвариант 14):
/// разойдись они — игрок увидел бы полную полоску у незакрытой цели.
#[test]
fn the_panel_counter_matches_the_threshold() {
    let mut sim = sim_with_storage();
    let goal = sim.set_goal(GoalTest::Stored(vec![(SCRAP, 10)]));

    for n in [3, 7, 9] {
        sim.put_item(5, 1, SCRAP, n);
        sim.tick_n(1);
        let (have, need) = sim.goal_progress(goal).expect("цель видна");
        assert_eq!(need, 10);
        assert_eq!(
            have >= need,
            sim.goal_taken(goal),
            "полоска и засчитывание разошлись на {have}"
        );
    }
}

/// У целей без числа счётчик двоичный: панели хватает «нет» и «есть», а полоску
/// там рисовать нечем.
#[test]
fn a_yes_or_no_goal_has_a_binary_counter() {
    let mut sim = sim_from(&["#####", "#a..#", "#####"]);
    let goal = sim.set_goal(GoalTest::Tech("materials".to_string()));

    sim.tick_n(1);
    assert_eq!(sim.goal_progress(goal), Some((0, 1)));

    sim.set_tech("materials");
    sim.tick_n(1);
    assert_eq!(sim.goal_progress(goal), Some((1, 1)));
}

// --- боевой рулсет ----------------------------------------------------------
//
// Сторож ловит рассогласование кода и контента, которого синтетика не увидит:
// цель со сбитым `id`, цель, выполненную на старте, и — главное — цель, которая
// требует от игрока проиграть или выбрать сторону развилки.

fn shipped() -> Sim {
    Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет")
}

/// **Ни одна цель не потерялась при разборе.**
///
/// `GoalRules` собирается `filter_map`'ом: цель со сбитым `id` предмета, тайла
/// или рецепта не даёт ошибки — она просто исчезает. Это самый тихий из
/// возможных отказов (список молча короче), поэтому его и сторожим первым.
#[test]
fn the_shipped_ruleset_keeps_every_goal() {
    let sim = shipped();
    assert_eq!(
        sim.goal_specs().len(),
        sim.goals.len(),
        "цель потерялась при разборе — сбит id предмета, тайла, миссии или рецепта",
    );
    assert!(!sim.goal_specs().is_empty(), "целей нет вовсе");
}

/// **Ни одна цель не выполнена на старте** — иначе это не цель, а поздравление.
#[test]
fn the_shipped_ruleset_starts_with_every_goal_open() {
    let mut sim = shipped();
    sim.tick_n(1);
    assert_eq!(
        sim.goals_taken(),
        0,
        "цель закрылась на первом тике: игроку нечего в ней делать",
    );
}

/// **Ни одна обязательная цель не требует провала.**
///
/// Плен случается только от провала вылазки (§12.40), поэтому цель на `rescue`
/// обязана быть скрытой: требовать её значило бы требовать проиграть, а партия
/// должна проходиться вчистую.
#[test]
fn the_shipped_ruleset_never_requires_losing() {
    let sim = shipped();
    let rescue: Vec<bool> = sim.mission_sides().iter().map(|s| s.4).collect();
    for (def, (hidden, test)) in sim.goal_specs().iter().enumerate() {
        if let GoalTest::Raid(m) = test {
            assert!(
                !rescue[*m] || *hidden,
                "цель {def} требует вылазки за пленным, а она бывает только после провала",
            );
        }
    }
}

/// **Ни одна обязательная цель не выбирает сторону развилки за игрока.**
///
/// Гвоздь приходит от Синдиката, «Логово» открывает Полиция, и взять обе стороны
/// разом нельзя (§12.43). Цель, выполнимая только с одной из них, превратила бы
/// развилку в единственно верный путь и обесценила бы самую дорогую механику
/// проекта — поэтому цель на найм спрашивает **любого** новичка, а цель на
/// вылазку обязана иметь незакрытый вариант.
#[test]
fn the_shipped_ruleset_never_picks_a_side_for_the_player() {
    let sim = shipped();
    let sides = sim.mission_sides();
    let recruits = sim.recruit_needs();

    for (def, (hidden, test)) in sim.goal_specs().iter().enumerate() {
        if *hidden {
            continue; // скрытая ничего не требует
        }
        match test {
            GoalTest::Raid(m) => assert!(
                sides[*m].3.is_empty(),
                "цель {def} требует вылазки с полом доверия — это выбор стороны",
            ),
            // Найм адресован числу котов, а не кандидату: закрыть её обязан
            // хоть кто-то, кого не присылает фракция.
            GoalTest::Cats(_) => assert!(
                recruits.iter().any(|needs| needs.is_empty()),
                "цель {def} на пополнение: все кандидаты за фракционными воротами",
            ),
            _ => {}
        }
    }
}
