//! Таймлайн: мир идёт по расписанию, и записка о нём предупреждает (§12.28).
//!
//! Это первая система, которая **действует сама** — без разметки игрока и без
//! свободного кота. У всего остального на базе есть инициатор: чертёж, пометка,
//! заявка, команда. Событие происходит потому, что настал его тик.
//!
//! Ровно поэтому у события два исхода, а не один: **дата известна заранее**
//! (§4.6), значит между «предупредили» и «случилось» есть время, и это время —
//! единственное, что игрок может потратить. База успела к технологии — событие
//! приносит; не успела — забирает бодрость. Наказание обратимо: коты отоспятся
//! (§12.10), потерян день работы, а не забег.
//!
//! Материальное приходит **через шлюз** — тем же `spill`, что и добыча с
//! вылазки (§12.22). Нет шлюза — база не соприкасается с миром, и подарку
//! неоткуда взяться; сама дата при этом всё равно проходит.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::spill;
use crate::map::BaseMap;
use crate::missions::gate_cells;

/// Проступили ли детали события к этому тику («туман предзнания», §4.6).
///
/// Считает ядро, а не интерфейс: спрятанный в JS текст — это не повреждённые
/// данные, а непоказанный текст, и однажды он покажется.
pub(crate) fn revealed(rule: &EventRule, tick: u64) -> bool {
    tick + rule.reveal >= rule.at
}

/// Готова ли база к событию: все требуемые технологии изучены.
pub(crate) fn ready_for(rule: &EventRule, techs: &Techs) -> bool {
    techs.covers(&rule.requires)
}

/// Проводит события, которым настал срок.
///
/// Событие срабатывает **ровно один раз** — на своём тике; журнал (`Chronicle`)
/// он же признак «уже было». Проверка идёт по `>=`, а не по `==`: тик может
/// проскочить только вместе с перезапуском мира, но зависшее навсегда событие
/// было бы худшим из возможных ответов на это.
pub(crate) fn run_timeline(
    time: Res<SimTime>,
    rules: Res<TimelineRules>,
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    techs: Res<Techs>,
    mut chronicle: ResMut<Chronicle>,
    mut fame: ResMut<Fame>,
    mut commands: Commands,
    mut cats: Query<&mut Energy, (With<UnitId>, Without<Away>)>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    for (def, rule) in rules.0.iter().enumerate() {
        if time.tick < rule.at || chronicle.happened(def).is_some() {
            continue;
        }
        let ready = ready_for(rule, &techs);
        chronicle.0.push(Happened { def, ready });

        if !ready {
            // Расплата — котовремя, та же валюта, в которой измеряется всё
            // остальное (§12.23). Ушедших с базы мир не касается.
            for mut energy in &mut cats {
                energy.0 = (energy.0 - rule.toll).max(0);
            }
            continue;
        }

        fame.0 += rule.fame;
        if rule.gift.is_empty() {
            continue;
        }
        // Груз ложится кучей на шлюз — оттуда его разнесёт обычная уборка
        // (§12.16), ровно как добычу с вылазки.
        // Первый гараж по обходу карты — детерминированно (§11), и этого
        // довольно: мир кладёт груз у любой двери, а не выбирает ближайшую к
        // кому-то. До §12.152 тут звался `pick_gate` с пустым списком котов, то
        // есть ровно то же самое, только через выражение про отряд.
        let Some(gate) = gate_cells(&map, &tiles).next() else {
            continue; // шлюза нет — миру не через что дотянуться до базы
        };
        for &(item, count) in &rule.gift {
            if count > 0 {
                spill(&mut commands, &mut stacks, gate, item, count);
            }
        }
    }
}
