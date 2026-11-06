//! Одиночный выход наверх через шлюз (§12.349).
//!
//! Кота посылают обычным приказом на клетку шлюза (`Sim::command_at`), и
//! приказ этот несёт намерение (`Outbound`). Дошёл — кот выходит наверх: `Away`
//! вместе с `Outing`, таймер на `ticks`. Вернулся — на шлюз ложится добыча,
//! бодрость платит `toll` разом, как у вылазки, а счёт выходов (`Outings`)
//! растёт: по нему окрестности и истощаются.
//!
//! Риска у выхода нет **намеренно**: ни ран, ни плена. Вытащить одиночку некому
//! (`rescue_is_possible` меряет отряд), а потерять кота за «выглянул наружу» —
//! это читалось бы поломкой. Цена выхода — время и бодрость.

use bevy_ecs::prelude::*;

use crate::components::{
    Away, Energy, Order, Outbound, Outing, OutingRules, Outings, Path, Position, Stack, Stride,
    TileRules,
};
use crate::hauling::spill;
use crate::map::BaseMap;

/// Выход и возвращение. Стоит в цепочке **после `retry_orders`**: тот снимает
/// `Order`, когда кот дошёл, и выход начинается тем же тиком, — и **до
/// `settle_stacks`**: добыча ложится на шлюз, а тот мог стать ямой, пока кот
/// был наверху.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn run_outings(
    mut commands: Commands,
    rules: Res<OutingRules>,
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    mut done: ResMut<Outings>,
    outbound: Query<(
        Entity,
        &Position,
        &Outbound,
        Has<Order>,
        Option<&Path>,
        Has<Away>,
    )>,
    mut out: Query<(Entity, &mut Outing, Option<&mut Energy>)>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    if rules.ticks <= 0 {
        return;
    }
    for (cat_e, at, plan, ordered, path, away) in &outbound {
        // Ушёл с базы другим путём — намерение кончилось.
        if away {
            commands.entity(cat_e).remove::<Outbound>();
            continue;
        }
        if ordered || path.is_some_and(|p| !p.steps.is_empty()) {
            continue;
        }
        // Приказа больше нет: либо дошёл, либо его сняли. Не дошёл —
        // намерение гаснет вместе с приказом.
        commands.entity(cat_e).remove::<Outbound>();
        if (at.x, at.y) != plan.gate || !tiles.is_gate(map.tile_at(plan.gate.0, plan.gate.1)) {
            continue;
        }
        commands.entity(cat_e).remove::<(Path, Stride)>().insert((
            Away,
            Outing {
                gate: plan.gate,
                left: rules.ticks,
            },
        ));
    }

    for (cat_e, mut outing, energy) in &mut out {
        outing.left -= 1;
        if outing.left > 0 {
            continue;
        }
        commands.entity(cat_e).remove::<(Away, Outing)>();
        if let Some(mut energy) = energy {
            energy.0 = (energy.0 - rules.toll).max(0);
        }
        for &(item, count) in rules.yield_of(done.0) {
            if count > 0 {
                spill(&mut commands, &mut stacks, outing.gate, item, count);
            }
        }
        done.0 += 1;
    }
}
