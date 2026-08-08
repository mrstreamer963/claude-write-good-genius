//! Фракции и репутация по каждой (§4.4, §12.43).
//!
//! Известность отвечает «насколько высоко» и только копится (§12.24); репутация
//! отвечает «от кого» — и потому единственная из трёх шкал-ворот умеет
//! закрывать. Проверяем три вещи, на которых держится всё остальное: заказ
//! двигает две фракции **одним** числом в разные стороны, провал не двигает
//! никого, а нейтралитет обходится в ноль, а не даром.
//!
//! Мир тот же, что у вылазок: коридор с клеткой-шлюзом (тайл 1). Фракций в
//! схеме `sim_from` нет — их заводит `set_faction`, как склад заводит
//! `set_capacity`.

use super::*;

/// Коридор со шлюзом в (3,1); миссия на двоих, безопасная и бесплатная.
fn sim_with_gate(ticks: i32) -> (Sim, usize) {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let m = sim.set_mission(2, ticks, &[]);
    (sim, m)
}

fn squad(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Отправить отряд и дождаться возвращения.
fn run(sim: &mut Sim, mission: usize, ticks: i32) {
    assert!(sim.launch(mission, squad(&["a", "b"])), "заявка принята");
    sim.tick_n((ticks + 40) as usize);
}

// --- арифметика ------------------------------------------------------------

#[test]
fn an_order_lifts_the_patron_and_drops_the_target() {
    let (mut sim, m) = sim_with_gate(10);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    sim.set_mission_factions(m, Some(police), Some(syndicate), 20);

    run(&mut sim, m, 10);

    assert_eq!(sim.standing(police), 20, "заказчик доволен");
    assert_eq!(
        sim.standing(syndicate),
        -20,
        "и пострадавший недоволен ровно настолько же — это одно число, а не два",
    );
}

/// Нейтральная вылазка не двигает никого: `patron` и `against` пусты, и это
/// **нормальная** запись рулсета, а не забытая (§4.4 начинает с очистки улиц).
#[test]
fn a_neutral_raid_touches_nobody() {
    let (mut sim, m) = sim_with_gate(10);
    let police = sim.set_faction(100);
    sim.set_mission_factions(m, None, None, 20);

    run(&mut sim, m, 10);

    assert_eq!(sim.standing(police), 0, "заказа не было — и стороны тоже");
}

/// Репутация расходится по сделанному: половина силы — половина репутации, той
/// же долей, что и добыча (§12.43).
#[test]
fn a_partial_order_moves_reputation_by_the_same_share() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    // Сила отряда — по единице с кота, навыков в схеме нет: двое против
    // сложности 4 берут ровно половину.
    let m = sim.set_risky_mission(2, 10, 4, 0, &[]);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    sim.set_mission_factions(m, Some(police), Some(syndicate), 20);

    run(&mut sim, m, 10);

    assert_eq!(sim.standing(police), 10, "полдела — полрепутации");
    assert_eq!(sim.standing(syndicate), -10, "и симметрия не нарушена");
}

/// **Главное свойство репутации:** провал не двигает её вовсе.
///
/// Отсюда вся честность закрывающихся ворот (§12.43): репутация падает только
/// от того, что игрок сделал, а не от того, что у него не вышло. Наказания
/// поверх наказания не выходит — §12.23 уже забрал добычу, силы и снаряжение.
#[test]
fn a_failed_order_moves_no_reputation() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    // Двое против сложности 10: силы вдвое меньше нужного — провал.
    let m = sim.set_risky_mission(2, 10, 10, 0, &[]);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    sim.set_mission_factions(m, Some(police), Some(syndicate), 20);

    run(&mut sim, m, 10);

    assert_eq!(sim.standing(police), 0, "заказчику предъявить нечего");
    assert_eq!(
        sim.standing(syndicate),
        0,
        "и пострадавший не пострадал: сорванный налёт никого не разозлил",
    );
}

/// Предел держит обе стороны: дно достижимо и **конечно**, а значит и дорога
/// назад имеет измеримую длину (§12.43).
#[test]
fn reputation_stops_at_the_span() {
    let (mut sim, m) = sim_with_gate(10);
    let police = sim.set_faction(25);
    let syndicate = sim.set_faction(25);
    sim.set_mission_factions(m, Some(police), Some(syndicate), 20);

    run(&mut sim, m, 10);
    run(&mut sim, m, 10);

    assert_eq!(sim.standing(police), 25, "вверх дальше предела не растёт");
    assert_eq!(sim.standing(syndicate), -25, "и вниз тоже не проваливается");
}

/// **Нейтралитет — не бесплатный вариант, а нулевой.**
///
/// Две зеркальные вылазки, взятые по очереди, оставляют базу ровно там, где
/// нашли. Сидеть на заборе можно — но с забора не видно ни одних ворот, и это
/// и есть решение, ради которого §12.43 вернулся к отвергнутым фракциям.
#[test]
fn alternating_orders_net_to_zero() {
    let (mut sim, ours) = sim_with_gate(10);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    let theirs = sim.set_mission(2, 10, &[]);
    sim.set_mission_factions(ours, Some(police), Some(syndicate), 20);
    sim.set_mission_factions(theirs, Some(syndicate), Some(police), 20);

    run(&mut sim, ours, 10);
    run(&mut sim, theirs, 10);

    assert_eq!(sim.standing(police), 0, "что дали, то и забрали");
    assert_eq!(sim.standing(syndicate), 0, "и у второй стороны так же");
}

// --- ворота ----------------------------------------------------------------

#[test]
fn a_faction_gate_refuses_the_launch() {
    let (mut sim, m) = sim_with_gate(10);
    let police = sim.set_faction(100);
    sim.set_mission_needs(m, &[(police, 30)]);

    assert!(
        !sim.raid_gates(m).welcome,
        "с базой, которой не доверяют, заказчик не разговаривает",
    );
    assert!(
        !sim.launch(m, squad(&["a", "b"])),
        "и заявку ядро отклоняет — панель говорит то же, что фасад",
    );

    sim.set_standing(police, 30);
    assert!(sim.raid_gates(m).welcome, "пол доверия взят");
    assert!(sim.launch(m, squad(&["a", "b"])), "теперь заказ дают");
}

/// Причины отказа разведены: «не дорос» и «эти с тобой не разговаривают» —
/// разные новости, и панель обязана называть их по-разному (§12.43).
#[test]
fn fame_and_trust_are_separate_reasons() {
    let (mut sim, m) = sim_with_gate(10);
    let police = sim.set_faction(100);
    sim.set_mission_fame(m, 0, 30);
    sim.set_mission_needs(m, &[(police, 30)]);

    sim.set_standing(police, 30);
    let gates = sim.raid_gates(m);
    assert!(gates.welcome, "доверие есть");
    assert!(!gates.unlocked, "а известности нет — и это другая причина");
}

/// **Вот оно, решение:** взятый заказ закрывает дверь напротив.
#[test]
fn an_order_taken_closes_another_factions_door() {
    let (mut sim, ours) = sim_with_gate(10);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    let theirs = sim.set_mission(2, 10, &[]);
    sim.set_mission_factions(ours, Some(police), Some(syndicate), 20);
    sim.set_mission_needs(theirs, &[(syndicate, 0)]);

    assert!(
        sim.raid_gates(theirs).welcome,
        "поначалу Синдикат базу терпит: нейтралитет — это ноль",
    );

    run(&mut sim, ours, 10);

    assert_eq!(sim.standing(syndicate), -20, "заказ был им поперёк");
    assert!(
        !sim.raid_gates(theirs).welcome,
        "и теперь их заказ базе закрыт — цена решения, а не поломка",
    );
}

/// Дорога назад есть всегда, и она стоит котовремени, а не удачи (§12.43).
#[test]
fn the_way_back_reopens_the_gate() {
    let (mut sim, ours) = sim_with_gate(10);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    let theirs = sim.set_mission(2, 10, &[]);
    sim.set_mission_factions(ours, Some(police), Some(syndicate), 20);
    sim.set_mission_factions(theirs, Some(syndicate), Some(police), 20);
    sim.set_mission_needs(theirs, &[(syndicate, 0)]);
    // Заказ, который Синдикат даёт всегда: без него дверь захлопнулась бы
    // навсегда, и обратимость держалась бы на обещании, а не на проверке.
    let mending = sim.set_mission(2, 10, &[]);
    sim.set_mission_factions(mending, Some(syndicate), None, 20);

    run(&mut sim, ours, 10);
    assert!(!sim.raid_gates(theirs).welcome, "дверь захлопнулась");

    run(&mut sim, mending, 10);
    assert_eq!(sim.standing(syndicate), 0, "и вернулась к нулю");
    assert!(sim.raid_gates(theirs).welcome, "дверь снова открыта");
}

/// Вылазка за своим не закрыта репутацией **никогда** (§12.40, §12.43).
///
/// Иначе база, рассорившаяся со всеми, теряла бы кота навсегда — смерть через
/// чёрный ход, которой в игре нет (§12.37).
#[test]
fn a_rescue_raid_is_never_gated_by_a_faction() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let syndicate = sim.set_faction(100);
    let doomed = sim.set_risky_mission(2, 10, 10, 0, &[]);
    let rescue = sim.set_rescue_mission(1, 10, 0);
    sim.set_standing(syndicate, -100); // с базой не разговаривает никто

    run(&mut sim, doomed, 10);
    assert!(sim.is_captive("a"), "провал оставил кота там");

    let gates = sim.raid_gates(rescue);
    assert!(gates.unlocked, "за своим идут при любой репутации");
    assert!(gates.possible, "и цель у вылазки есть");
    assert!(sim.launch(rescue, squad(&["b"])), "заявка принята");
}

#[test]
fn a_recruit_can_be_gated_by_reputation() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let syndicate = sim.set_faction(100);
    let nail = sim.set_recruit("nail", 0, &[], &[]);
    sim.set_recruit_needs(nail, &[(syndicate, 30)]);

    assert!(!sim.hire(nail), "своего присылают тем, кому доверяют");

    sim.set_standing(syndicate, 30);
    assert!(sim.hire(nail), "а теперь он откликнулся");
    assert!(sim.has_unit("nail"), "и пришёл на базу");
}

/// Найм репутацию **не двигает**: это покупка, а не поступок (§12.43).
///
/// Платит склад. Шкала, которая и открывает, и тратится, — ровно та ловушка,
/// которую §12.24 отверг у известности: заплатив за кота, игрок обнаружил бы
/// закрывшуюся вылазку и не увидел бы в этом причины.
#[test]
fn hiring_does_not_move_reputation() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let syndicate = sim.set_faction(100);
    let nail = sim.set_recruit("nail", 0, &[], &[]);
    sim.set_recruit_needs(nail, &[(syndicate, 30)]);
    sim.set_standing(syndicate, 30);

    assert!(sim.hire(nail), "нанят");
    assert_eq!(
        sim.standing(syndicate),
        30,
        "и доверие осталось прежним — за кота заплатил склад",
    );
}

/// Провал не только не отнимает репутацию, но и ничего не закрывает.
#[test]
fn a_failed_order_closes_nothing() {
    let mut sim = sim_from(&["########", "#a....b#", "########"]);
    sim.set_gate(1, true);
    sim.set_relay(1, true);
    sim.force_tile(3, 1, 1);
    let police = sim.set_faction(100);
    let syndicate = sim.set_faction(100);
    let doomed = sim.set_risky_mission(2, 10, 10, 0, &[]);
    sim.set_mission_factions(doomed, Some(police), Some(syndicate), 20);
    let theirs = sim.set_mission(2, 10, &[]);
    sim.set_mission_needs(theirs, &[(syndicate, 0)]);

    run(&mut sim, doomed, 10);

    assert!(
        sim.raid_gates(theirs).welcome,
        "сорванный налёт никого не разозлил — предъявить базе нечего",
    );
}

// --- боевой рулсет ---------------------------------------------------------
//
// Эти пятеро ловят рассогласование кода и контента, которого синтетика не
// увидит: фракцию без дороги назад, первую ступень, закрытую заказчиком,
// вылазку за своим с фракционными воротами, недостижимый пол доверия и
// развилку, у которой на самом деле нет двух сторон.

fn shipped() -> Sim {
    Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет")
}

/// **Дорога назад есть у каждой фракции.**
///
/// У каждой обязан быть заказ, который она сама не закрывает, — иначе, упав к
/// ней в минус, база не смогла бы подняться обратно ничем, и обратимость
/// держалась бы на обещании, а не на проверке (§12.40, §12.43).
#[test]
fn the_shipped_ruleset_always_leaves_a_way_back() {
    let sim = shipped();
    let sides = sim.mission_sides();
    for (f, span) in sim.faction_spans().iter().enumerate() {
        assert!(*span > 0, "у фракции {f} нет предела репутации");
        assert!(
            sides
                .iter()
                .any(|(patron, _, gives, needs, ..)| *patron == Some(f)
                    && *gives > 0
                    && needs.is_empty()),
            "фракция {f} не даёт ни одного заказа без пола доверия — из минуса к ней не вернуться",
        );
    }
}

/// Первая ступень не закрыта никем: база, которая ещё никто, обязана иметь
/// работу. §4.4 начинает с нейтральной очистки — с неё и начинаем.
#[test]
fn the_shipped_ruleset_never_gates_the_first_rung() {
    let sim = shipped();
    assert!(
        sim.mission_sides()
            .iter()
            .any(
                |(patron, against, _, needs, rescue, requires)| *requires == 0
                    && patron.is_none()
                    && against.is_none()
                    && needs.is_empty()
                    && !rescue
            ),
        "нет ни одной вылазки, доступной безвестной базе без стороны",
    );
}

/// **Вылазка за своим не закрыта фракциями никогда** (§12.40, §12.43).
///
/// Стоило бы ей зависеть от репутации, и база, рассорившаяся со всеми, теряла
/// бы кота навсегда — смерть через чёрный ход, которой в игре нет (§12.37).
#[test]
fn the_shipped_ruleset_never_gates_a_rescue() {
    let sim = shipped();
    for (patron, against, gives, needs, rescue, _) in sim.mission_sides() {
        if !rescue {
            continue;
        }
        assert!(needs.is_empty(), "за своим идут при любой репутации");
        assert!(
            patron.is_none() && against.is_none() && gives == 0,
            "и своих вытаскивают не по чьему-то заказу",
        );
    }
}

/// Пол доверия достижим: `needs` выше `span` — это мёртвый контент, как
/// требование параметра выше потолка (§12.42).
#[test]
fn the_shipped_ruleset_asks_for_nothing_past_the_span() {
    let sim = shipped();
    let spans = sim.faction_spans();
    let gates = sim
        .mission_sides()
        .into_iter()
        .map(|(.., needs, _, _)| needs)
        .chain(sim.recruit_needs());
    for needs in gates {
        for (f, need) in needs {
            let span = spans.get(f).copied().unwrap_or(0);
            assert!(
                need <= span,
                "пол доверия {need} у фракции {f} выше её предела {span} — до него не дойти",
            );
        }
    }
}

/// **Развилка настоящая:** на одной ступени есть два зеркальных заказа.
///
/// Одинаковый порог, одинаковая репутация, противоположные стороны — только так
/// чередование даёт ровно ноль, а нейтралитет оказывается нулевым, а не
/// бесплатным (§12.43). Разойдись цифры, и одна сторона стала бы выгоднее
/// другой при том же усилии, то есть выбора бы не было.
#[test]
fn the_shipped_ruleset_forces_a_side() {
    let sim = shipped();
    let sides = sim.mission_sides();
    assert!(
        sides
            .iter()
            .any(|(patron, against, gives, needs, _, requires)| {
                let (Some(p), Some(a)) = (*patron, *against) else {
                    return false;
                };
                needs.is_empty()
                    && sides.iter().any(|(p2, a2, gives2, needs2, _, requires2)| {
                        *p2 == Some(a)
                            && *a2 == Some(p)
                            && gives2 == gives
                            && requires2 == requires
                            && needs2.is_empty()
                    })
            }),
        "зеркальной пары заказов нет — сторону выбирать не из чего",
    );
}

/// **Развилка целиком, на боевом рулсете:** взяв сторону, база открывает одну
/// дверь и закрывает другую.
///
/// Полный прогон, а не арифметика: здесь сходятся лестница известности,
/// лестница доверия и то, что за них платят одними и теми же котами. Две
/// одинаковые по усилию ветки ведут к разным наградам — Полиция даёт работу
/// («Логово»), Синдикат даёт людей («Гвоздь»), — и взять обе разом нельзя.
/// Симметричные ветки свелись бы к выбору цвета (§12.43).
#[test]
fn the_shipped_ruleset_opens_one_door_by_closing_another() {
    // Две «Свалки» — нейтральный старт: обе стороны ещё на нуле.
    fn base() -> Sim {
        let mut sim = shipped();
        sim.without_timeline(); // мир по расписанию — шум для чужой механики
        for _ in 0..2 {
            assert!(sim.launch(0, squad(&["excellent", "sp2"])), "«Свалка»");
            sim.tick_n(1200);
        }
        assert_eq!(sim.fame(), 20, "лестница известности взята");
        assert_eq!(
            (sim.standing(0), sim.standing(1)),
            (0, 0),
            "стороны не выбраны"
        );
        sim
    }

    // Ветка Синдиката: два «Разбора ангара».
    let mut theirs = base();
    for _ in 0..2 {
        assert!(
            theirs.launch(2, squad(&["excellent", "sp2", "sp3"])),
            "«Разбор ангара» — заказ Синдиката, и он никогда не закрыт",
        );
        theirs.tick_n(2000);
    }
    assert_eq!(theirs.standing(1), 40, "Синдикат доволен");
    assert_eq!(
        theirs.standing(0),
        -40,
        "ровно настолько же недовольна Полиция"
    );
    assert!(
        !theirs.raid_gates(3).welcome,
        "«Логово» закрылось: Полиция с базой больше не разговаривает",
    );
    theirs.set_fame(60); // известности на «Логово» хватит и без этого — проверяем доверие
    assert!(
        !theirs.launch(3, squad(&["excellent", "sp2"])),
        "и заявку ядро отклоняет, хотя известности довольно",
    );

    // Ветка Полиции: два «Сопровождения каравана» — то же усилие, другая сторона.
    let mut ours = base();
    for _ in 0..2 {
        assert!(
            ours.launch(1, squad(&["excellent", "sp2", "sp3"])),
            "«Сопровождение» — заказ Полиции, и он тоже никогда не закрыт",
        );
        ours.tick_n(2000);
    }
    assert_eq!(ours.standing(0), 40, "теперь наоборот");
    assert_eq!(ours.standing(1), -40);
    assert!(ours.raid_gates(3).welcome, "«Логово» открылось");
    assert!(
        !ours.hire(0),
        "а «Гвоздя» Синдикат больше не пришлёт — за работу заплачено ходоком",
    );
}
