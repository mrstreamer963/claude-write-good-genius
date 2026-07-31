//! Цепочка систем одного тика — карта всей симуляции.
//!
//! Новая система добавляется **только сюда**: тесты гоняют ровно эту функцию,
//! поэтому расписание, собранное где-то ещё, останется непокрытым.

use bevy_ecs::prelude::*;

use crate::components::SimTime;
use crate::hauling::{assign_hauls, assign_tidy, mark_loose_scrap, settle_stacks, work_hauls};
use crate::jobs::{assign_jobs, work_jobs};
use crate::missions::{gather_squad, run_missions};
use crate::movement::{escape_voids, move_units, retry_orders};
use crate::needs::{assign_rest, collapse_exhausted, sleep, tire};
use crate::skills::train_skills;

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

/// Цепочка систем одного тика. Вынесена отдельно, чтобы тесты гоняли ровно тот
/// же порядок, что и боевая симуляция.
///
/// Четыре раздатчика берут котов из одного пула свободных, поэтому их порядок —
/// это и есть приоритет работ (§12.15, §12.16, §12.20):
///   1. `assign_rest` — отдых. Иначе уставшего кота тут же уводит работа.
///   2. `assign_hauls` — подвоз на площадки. Иначе бригада разбирает уже
///      обеспеченные чертежи, а за ломом для остальных никто не идёт, и база
///      встаёт тем вернее, чем больше на ней работы.
///   3. `assign_jobs` — стройка и снос.
///   4. `assign_tidy` — уборка пола. Последняя: подбирать мусор, пока стоит
///      настоящая работа, — это ровно то, чего игрок не ждёт.
///
/// Вылазки в этот порядок не входят: отряд назначает игрок в момент заявки, и
/// раздавать там нечего (§12.23). `gather_squad` только держит выбранных котов
/// идущими к шлюзу, поэтому стоит рядом с раздатчиками, но пула не трогает.
///
/// `run_missions` — после `move_units`: кот, шагнувший на шлюз в этом тике,
/// засчитывается в отряд сразу. И до `settle_stacks`: добыча ложится кучей на
/// шлюз, а он к возвращению отряда может оказаться ямой.
///
/// `escape_voids` — предпоследним: если приказ или джоб уже дали маршрут, он и
/// выводит кота из ямы, отдельный шаг не нужен. `settle_stacks` после
/// `work_jobs`: обе системы разбирают последствия только что снесённого
/// тайла — кота в яме и кучу лома в ней же.
///
/// `train_skills` замыкает цепочку: он превращает в опыт маркеры `Worked`,
/// которые за тик оставили системы работы, — и потому должен идти после любой
/// из них, включая будущие (§12.17).
pub(crate) fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            advance_time,
            collapse_exhausted,
            assign_rest,
            gather_squad,
            assign_hauls,
            assign_jobs,
            mark_loose_scrap,
            assign_tidy,
            move_units,
            work_hauls,
            work_jobs,
            run_missions,
            retry_orders,
            escape_voids,
            settle_stacks,
            sleep,
            tire,
            train_skills,
        )
            .chain(),
    );
    schedule
}
