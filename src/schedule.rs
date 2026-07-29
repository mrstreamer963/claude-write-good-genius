//! Цепочка систем одного тика — карта всей симуляции.
//!
//! Новая система добавляется **только сюда**: тесты гоняют ровно эту функцию,
//! поэтому расписание, собранное где-то ещё, останется непокрытым.

use bevy_ecs::prelude::*;

use crate::components::SimTime;
use crate::jobs::{assign_jobs, work_jobs};
use crate::movement::{escape_voids, move_units, retry_orders};

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

/// Цепочка систем одного тика. Вынесена отдельно, чтобы тесты гоняли ровно тот
/// же порядок, что и боевая симуляция.
///
/// `escape_voids` — последним: если приказ или джоб уже дали маршрут, он и
/// выводит кота из ямы, отдельный шаг не нужен.
pub(crate) fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            advance_time,
            assign_jobs,
            move_units,
            work_jobs,
            retry_orders,
            escape_voids,
        )
            .chain(),
    );
    schedule
}
