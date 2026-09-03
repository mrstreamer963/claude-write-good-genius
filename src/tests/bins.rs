//! Ящик: хранилище верхнего яруса с набором принимаемого (§12.195).
//!
//! Ярусов в синтетической схеме нет: `priority` у всех тайлов нулевой, значит
//! уборка ведёт себя ровно как до ящиков. Заводит их `set_priority`, а набор —
//! `set_bin`; пустой набор означает «принимает всё», и второго представления у
//! этого состояния нет.
//!
//! Тайл `1` здесь склад (ярус 0), тайл `2` — ящик (ярус 1): оба переводятся
//! через `force_tile`, как в `tidying`.

use super::sim_from;
use crate::sim::Sim;

const CORRIDOR: [&str; 3] = ["###########", "#a........#", "###########"];

/// Склад на дальнем конце, ящик рядом с котом: обе клетки свободны, и обе
/// возьмут груз, — но ярус важнее расстояния.
fn shelves(sim: &mut Sim) {
    sim.set_capacity(1, 20);
    sim.set_capacity(2, 10);
    sim.set_priority(2, 1);
    sim.force_tile(9, 1, 1); // склад
    sim.force_tile(3, 1, 2); // ящик
}

// --- движение вверх по ярусам ----------------------------------------------

/// Базовый случай: с пола вещь едет в ящик, а не в склад, даже если склад
/// ближе не был бы. Ярус решает раньше расстояния.
#[test]
fn loose_scrap_prefers_the_higher_tier() {
    let mut sim = sim_from(&CORRIDOR);
    shelves(&mut sim);
    sim.put_scrap(6, 1, 4);

    sim.tick_n(300);
    assert_eq!(sim.scrap_at(3, 1), 4, "весь лом в ящике");
    assert_eq!(sim.scrap_at(9, 1), 0, "на складе пусто");
}

/// Главное, ради чего ящик заведён: уже сложенное на склад **поднимается** в
/// ящик у потребителя. До §12.195 склад был конечной остановкой.
#[test]
fn stored_scrap_moves_up_into_a_bin() {
    let mut sim = sim_from(&CORRIDOR);
    shelves(&mut sim);
    sim.put_scrap(9, 1, 4);

    sim.tick_n(400);
    assert_eq!(sim.scrap_at(3, 1), 4, "лом переехал в ящик");
    assert_eq!(sim.scrap_at(9, 1), 0, "склад опустел");
}

/// Ящик набран до краёв — остальное остаётся на складе. «Влезет» проверяется
/// вместе с «примут»: это один вопрос к клетке, а не два.
#[test]
fn a_full_bin_stops_pulling() {
    let mut sim = sim_from(&CORRIDOR);
    shelves(&mut sim);
    sim.put_scrap(9, 1, 16);

    sim.tick_n(600);
    assert_eq!(sim.scrap_at(3, 1), 10, "ящик набит по ёмкости");
    assert_eq!(sim.scrap_at(9, 1), 6, "остаток лежит на складе");
}

/// Два ящика одного яруса друг у друга ничего не отнимают: правило требует
/// **строго** большего яруса, поэтому качелей не бывает по построению.
#[test]
fn equal_tiers_do_not_swap_goods() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_capacity(2, 10);
    sim.set_priority(2, 1);
    sim.force_tile(3, 1, 2);
    sim.force_tile(8, 1, 2);
    sim.put_scrap(3, 1, 4);

    sim.tick_n(400);
    assert_eq!(sim.scrap_at(3, 1), 4, "куча осталась на месте");
    assert_eq!(sim.scrap_at(8, 1), 0, "во второй ящик никто не побежал");
    assert_eq!(sim.carrying_of("a"), 0, "и в лапах ничего не застряло");
}

// --- набор принимаемого ----------------------------------------------------

/// Ящик берёт только то, что ему назначено; чужое едет мимо, на склад.
#[test]
fn a_bin_takes_only_what_it_was_told_to() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);
    assert!(sim.set_bin(3, 1, 1, true), "ящик настраивается");
    sim.put_item(6, 1, 0, 3);
    sim.put_item(7, 1, 1, 3);

    sim.tick_n(500);
    assert_eq!(sim.item_at(3, 1, 1), 3, "предмет 1 в ящике");
    assert_eq!(sim.item_at(3, 1, 0), 0, "предмета 0 в ящике нет");
    assert_eq!(sim.item_at(9, 1, 0), 3, "он уехал на склад");
}

/// Смена набора **эвакуирует** прежнее содержимое: свежий ящик принимает всё и
/// набирается чем попало, а без обратной дороги мусор остался бы в нём навсегда
/// — материал-то ходит только вверх.
#[test]
fn retyping_a_bin_evicts_what_it_no_longer_takes() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);
    sim.put_item(3, 1, 0, 4);

    assert!(
        sim.set_bin(3, 1, 1, true),
        "теперь ящик только под предмет 1"
    );
    sim.tick_n(400);
    assert_eq!(sim.item_at(3, 1, 0), 0, "чужое из ящика уехало");
    assert_eq!(sim.item_at(9, 1, 0), 4, "и легло на склад");
}

/// Снятие последнего предмета возвращает «принимает всё»: пустого набора не
/// существует, иначе у одного состояния было бы два представления.
#[test]
fn clearing_the_last_item_means_take_everything_again() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);
    sim.set_bin(3, 1, 1, true);
    assert_eq!(sim.bin_of(3, 1), vec![1]);

    sim.set_bin(3, 1, 1, false);
    assert!(sim.bin_of(3, 1).is_empty(), "запись стёрлась целиком");

    sim.put_item(6, 1, 0, 3);
    sim.tick_n(300);
    assert_eq!(sim.item_at(3, 1, 0), 3, "ящик снова берёт что угодно");
}

/// Куче, которой некуда деться, кота не посылают. Иначе на базе с полным
/// складом и ящиком под чужой тип коты бегали бы к ней каждый тик впустую.
#[test]
fn nobody_is_sent_for_goods_with_nowhere_to_go() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    sim.set_capacity(2, 10);
    sim.set_priority(2, 1);
    sim.force_tile(3, 1, 2);
    sim.set_bin(3, 1, 1, true); // ящик только под предмет 1, склада нет вовсе
    sim.put_item(6, 1, 0, 3);

    sim.tick_n(300);
    assert_eq!(sim.item_at(6, 1, 0), 3, "куча лежит там, где лежала");
    assert_eq!(sim.carrying_of("a"), 0, "и никто её не поднял");
}

/// Ёмкость ящика — предел, а не пожелание: двое сдающих в один тик не должны
/// набить его сверх `capacity`.
///
/// **Механизм лежит в общем коде и старше ящиков**, но добраться до него было
/// нечем. `spill` кладёт груз в **существующую** кучу через мутабельный запрос
/// (это видно всем следующим в том же тике), а на пустой клетке **спавнит
/// новую через `Commands`** — то есть до конца тика её не видит никто, и
/// второй сдающий снова считает клетку пустой. Прикрывали это две вещи: общий
/// бюджет места в `assign_tidy` и, главное, сам выбор адресата — «ближайшая
/// клетка с местом» **разводила** котов по разным клеткам большого склада.
///
/// Ярусы (§12.195) развод убрали: все сходятся в один маленький ящик, а бюджет,
/// считающий место по базе целиком, его не ограничивает. Двое восьмилапых дают
/// «занято 16 / 10» — ровно то, что увидел игрок. Поэтому синтетической
/// схемы **без** ярусов, которая ловила бы то же самое, не существует: старое
/// правило само не давало двоим прийти в одну клетку.
#[test]
fn two_carriers_cannot_overfill_a_bin_in_a_tick() {
    // Симметрия нарочная: коты, кучи и ящик расставлены так, чтобы оба шагнули
    // на клетку ящика **одним тиком**. Развести их `spread_units` успеет только
    // после `work_hauls` (§12.32), то есть сдадут груз оба.
    let mut sim = sim_from(&["#############", "#a.........b#", "#############"]);
    sim.set_capacity(1, 60); // просторный склад: общего места на базе вдоволь
    sim.set_capacity(2, 10); // ящик, в который всё и польётся
    sim.set_priority(2, 1);
    sim.force_tile(1, 1, 1);
    sim.force_tile(6, 1, 2);
    sim.put_scrap(3, 1, 8);
    sim.put_scrap(9, 1, 8);
    sim.set_carry("a", 8);
    sim.set_carry("b", 8);

    for _ in 0..300 {
        sim.tick_n(1);
        assert!(
            sim.scrap_at(6, 1) <= 10,
            "ящик набит сверх ёмкости: {} при 10",
            sim.scrap_at(6, 1),
        );
    }
    assert_eq!(sim.scrap_total(), 16, "и ничего не потерялось");
}

// --- ворота и уборка за собой ----------------------------------------------

/// Настроить можно только клетку с ярусом. Склад адресным не делается: он и так
/// вместительнее, и адресный склад побил бы ящик разом по всем осям.
#[test]
fn only_a_tiered_cell_can_be_typed() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);

    assert!(!sim.set_bin(9, 1, 0, true), "склад не настраивается");
    assert!(!sim.set_bin(6, 1, 0, true), "и обычный пол тоже");
    assert!(sim.set_bin(3, 1, 0, true), "а ящик — да");
}

/// Несуществующий предмет отклоняется: индекс палитры приезжает из вида, и
/// молчаливая запись мусора пережила бы сохранение.
#[test]
fn an_unknown_item_is_refused() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);

    assert!(!sim.set_bin(3, 1, 7, true), "предмета 7 в палитре нет");
    assert!(sim.bin_of(3, 1).is_empty());
}

/// Снесённый ящик уносит свою запись: клетка перестала быть ящиком — правилу не
/// на чем стоять. Прецедент дословный — правило автовылазки на снесённом гараже.
#[test]
fn a_demolished_bin_forgets_its_setting() {
    let mut sim = sim_from(&CORRIDOR);
    sim.set_items(2);
    shelves(&mut sim);
    sim.set_bin(3, 1, 1, true);
    assert_eq!(sim.bin_of(3, 1), vec![1]);

    sim.force_tile(3, 1, 0); // ящик снесли
    sim.tick_n(2);
    assert!(sim.bin_of(3, 1).is_empty(), "запись убрана за собой");
}

// --- согласие с остальной базой --------------------------------------------

/// Ящик — такое же учтённое имущество, как склад: им платят. Иначе «где лежит»
/// начало бы значить «сколько у базы есть», и тройка чисел шапки раздвоилась бы.
#[test]
fn goods_in_a_bin_still_pay_for_things() {
    let mut sim = sim_from(&CORRIDOR);
    shelves(&mut sim);
    sim.set_cost(3, 6); // тайл `3` стоит шесть лома
    sim.put_scrap(3, 1, 10);

    let (stored, ..): (i32, i32, i32) = sim.stock_of(0);
    assert_eq!(stored, 10, "лом в ящике учтён");
    assert!(sim.add_blueprint(5, 1, 3), "и им можно оплатить чертёж");
}

/// Сумма «кучи + лапы» переживает переезд по ярусам (инвариант 11): ящик — это
/// новый адрес, а не новый способ терять материал.
#[test]
fn moving_between_tiers_conserves_goods() {
    let mut sim = sim_from(&CORRIDOR);
    shelves(&mut sim);
    sim.put_scrap(9, 1, 7);

    // Проверять надо **на каждом тике**: конечное состояние замело бы след, а
    // переезд по ярусам — это как раз середина пути (кучи нет, груз в лапах).
    for _ in 0..400 {
        sim.tick_n(1);
        assert_eq!(
            sim.scrap_total(),
            7,
            "ничего не потерялось и не удвоилось: `scrap_total` считает и лапы",
        );
    }
}

// --- боевой рулсет ---------------------------------------------------------

/// Ярусы обязаны идти в противоход ёмкости: максимум яруса строго меньше
/// минимума яруса под ним.
///
/// Меряется по **минимуму**, а не по любому попавшемуся хранилищу: ящик на 30
/// прошёл бы сравнение со стеллажом (60) и при этом убил бы склад (20) —
/// вместимее, адресный, выше ярусом, — и развилка «плотность против близости»
/// исчезла бы вместе с ним.
#[test]
fn the_shipped_ruleset_stacks_storage_tiers() {
    let sim = Sim::new(include_str!("../../assets/rulesets/core.yaml")).expect("рулсет");
    let mut tiers: Vec<(i32, i32)> = sim.storage_tiers();
    assert!(!tiers.is_empty(), "хранилища в рулсете есть");
    tiers.sort_unstable();

    for &(tier, cap) in &tiers {
        assert!(cap > 0, "ярус {tier} без ёмкости — это не хранилище");
    }
    for &(lower, _) in &tiers {
        let below = tiers
            .iter()
            .filter(|&&(t, _)| t == lower)
            .map(|&(_, c)| c)
            .min()
            .expect("ярус есть");
        for &(upper, cap) in &tiers {
            assert!(
                upper <= lower || cap < below,
                "ярус {upper} вмещает {cap} при {below} у яруса {lower}: \
                 близость перестала стоить плотности",
            );
        }
    }
    assert!(
        tiers.iter().any(|&(t, _)| t > 0),
        "хотя бы один ящик в рулсете есть — иначе механика выключена молча",
    );
}
