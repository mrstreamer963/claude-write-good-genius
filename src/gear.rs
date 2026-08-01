//! Снаряжение: комплект по шаблону, за которым кот идёт сам (§4.2, §12.29,
//! §12.34 concept.md).
//!
//! Снаряжение — не подсистема, а **свойство предмета**: у записи в `items:`
//! есть `force`, и предмет с ней можно надеть. В кучах, в лапах и на складе он
//! живёт ровно как лом (§12.21), поэтому ни новой сущности, ни инвентаря здесь
//! нет — только компонент `Gear` со списком надетого.
//!
//! А вот **надевание — задача** (`Equipping`, §12.34), восьмая в общем слое:
//! кот идёт к куче с нужной вещью и берёт её оттуда сам. Раньше склад одевал
//! мгновенно, как платит за найм и науку; на практике это читалось как
//! телепортация — комбинезон исчезал со склада в другом конце базы и оказывался
//! на коте тем же тиком. Плата остаётся мгновенной там, где она **плата**
//! (§12.24, §12.26, §12.30): списание за услугу базы — это учёт, а надетая
//! вещь — предмет, который кто-то принёс.
//!
//! Источник — **любая куча**, склад или пол: кот приходит на клетку и поднимает
//! то, что там лежит. Инвариант §12.16 («ничего не исчезает с пола в лапы»)
//! этим не нарушается, а исполняется буквально — у подъёма появился адресат и
//! дорога к нему.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::BaseMap;
use crate::path::Reach;

/// Отправляет котов за недостающими вещами шаблона.
///
/// Порядок обхода задан явно — котов по `id`: когда комбинезонов меньше, чем
/// котов, «кому достанется» видно игроку, а обход сущностей ECS зависит от
/// истории вставок и недетерминирован (§11, §12.24).
///
/// **За ходку берётся одна вещь.** Шаблон — набор независимых вещей, а не цена
/// (§12.29): надел один предмет из двух — промежуточный результат, а не
/// половинчатая покупка; за вторым кот сходит следующей ходкой.
///
/// **Кучу выбираем ближайшую и считаем, сколько в ней уже «занято»** теми, кто
/// к ней идёт. Это не резервирование источника, которое §12.15 отверг для лома,
/// а отказ отправлять троих котов через полбазы за одним комбинезоном: там
/// промах стоил лишней ходки и только.
///
/// Кота **с маршрутом** не трогаем — он идёт по приказу игрока или к своей
/// работе (§12.15). Исключение — отряд: сбор ещё не закончен, и боец, который
/// может одеться, должен одеться, иначе сила отряда зависит от того, успел ли
/// склад пополниться до нажатия кнопки (§12.29). `gather_squad` его подождёт.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_equip(
    map: Res<BaseMap>,
    loadout: Res<LoadoutRules>,
    mut commands: Commands,
    cats: Query<
        (
            Entity,
            &UnitId,
            &Position,
            Option<&Gear>,
            Option<&Path>,
            Option<&Squad>,
        ),
        (
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Crafting>,
            Without<Equipping>,
            Without<Eating>,
            Without<Healing>,
            Without<Treating>,
            Without<Away>,
        ),
    >,
    going: Query<&Equipping>,
    stacks: Query<(Entity, &Position, &Stack)>,
) {
    if loadout.0.is_empty() {
        return;
    }

    // Кто чего недосчитался, в порядке `id`.
    let mut naked: Vec<(&str, Entity, (i32, i32), Vec<usize>)> = cats
        .iter()
        .filter(|(_, _, _, _, path, squad)| path.is_none() || squad.is_some())
        .filter_map(|(cat_e, id, pos, gear, ..)| {
            let worn = gear.map(|g| g.0.as_slice()).unwrap_or(&[]);
            let missing: Vec<usize> = loadout
                .0
                .iter()
                .copied()
                .filter(|item| !worn.contains(item))
                .collect();
            (!missing.is_empty()).then_some((id.0.as_str(), cat_e, (pos.x, pos.y), missing))
        })
        .collect();
    if naked.is_empty() {
        return;
    }
    naked.sort_unstable_by_key(|&(id, ..)| id);

    // Сколько в каждой куче уже обещано тем, кто к ней идёт.
    let mut taken: Vec<(Entity, i32)> = Vec::new();
    for job in &going {
        match taken.iter_mut().find(|(pile, _)| *pile == job.pile) {
            Some((_, n)) => *n += 1,
            None => taken.push((job.pile, 1)),
        }
    }

    for (_, cat_e, from, missing) in naked {
        // Обход строим только когда есть за чем идти: голая база с непустым
        // шаблоном иначе гоняла бы BFS на каждого кота каждый тик.
        let anything = missing.iter().any(|&item| {
            stacks.iter().any(|(pile_e, _, stack)| {
                stack.item == item && stack.count > claimed(&taken, pile_e)
            })
        });
        if !anything {
            continue;
        }

        let reach = Reach::all(&map, from);
        // Кучи по шаблону: первая вещь, за которой вообще есть куда идти.
        let found = missing.iter().find_map(|&item| {
            stacks
                .iter()
                .filter(|(pile_e, _, stack)| {
                    stack.item == item && stack.count > claimed(&taken, *pile_e)
                })
                .filter_map(|(pile_e, pos, _)| {
                    Some((reach.dist_at(pos.x, pos.y)?, (pos.x, pos.y), pile_e, item))
                })
                // При равном расстоянии — по карте, а не по порядку сущностей:
                // обход ECS зависит от истории вставок (§11).
                .min_by_key(|&(d, at, ..)| (d, at.1, at.0))
        });
        let Some((_, at, pile_e, item)) = found else {
            continue; // до вещи не дойти — кот идёт как есть, это не ошибка
        };

        match taken.iter_mut().find(|(pile, _)| *pile == pile_e) {
            Some((_, n)) => *n += 1,
            None => taken.push((pile_e, 1)),
        }
        let path = reach.path_to(at.0, at.1).unwrap_or_default();
        commands.entity(cat_e).insert((
            Equipping { item, pile: pile_e },
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

/// Сколько штук из кучи уже обещано тем, кто к ней идёт.
fn claimed(taken: &[(Entity, i32)], pile: Entity) -> i32 {
    taken
        .iter()
        .find(|(p, _)| *p == pile)
        .map_or(0, |&(_, n)| n)
}

/// Дошедший до кучи кот надевает вещь; кучи не оказалось — задача снимается.
///
/// Промах — легальный исход, а не ошибка: кучу мог унести носильщик или разобрать
/// на неё же нацелившийся сосед. Кот просто освобождается, и следующий тик
/// раздатчик подберёт ему другую кучу — ровно как с ломом (§12.15).
pub(crate) fn work_equip(
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Equipping, Option<&Gear>), Without<Path>>,
    mut stacks: Query<(&Position, &mut Stack)>,
) {
    for (cat_e, pos, job, gear) in &cats {
        commands.entity(cat_e).remove::<Equipping>();

        let Ok((pile_pos, mut stack)) = stacks.get_mut(job.pile) else {
            continue; // кучи больше нет
        };
        if (pile_pos.x, pile_pos.y) != (pos.x, pos.y) || stack.item != job.item || stack.count <= 0
        {
            continue; // куча уехала или опустела, пока кот шёл
        }
        stack.count -= 1;
        if stack.count <= 0 {
            commands.entity(job.pile).despawn();
        }
        // Компонент переписывается целиком: «надетого сверх шаблона» не бывает,
        // и собрать список заново дешевле, чем править его на месте.
        let mut worn = gear.map(|g| g.0.clone()).unwrap_or_default();
        worn.push(job.item);
        commands.entity(cat_e).insert(Gear(worn));
    }
}
