//! Снаряжение: комплект по шаблону, снятый со склада (§4.2, §12.29 concept.md).
//!
//! Снаряжение — не подсистема, а **свойство предмета**: у записи в `items:`
//! появилась `force`, и предмет с ней можно надеть. В кучах, в лапах и на складе
//! он живёт ровно как лом (§12.21), поэтому ни новой сущности, ни инвентаря
//! здесь нет — только компонент `Gear` со списком надетого.
//!
//! Система одна — `auto_equip`, и она **не раздатчик**: кота она не занимает и из
//! общего пула не берёт. Экипировка не задача, а состояние, поэтому фильтры
//! `Without<…>` ей не нужны: одеть можно и спящего, и строителя.
//!
//! **Платит склад, и платит мгновенно** — как за найм (§12.24) и за науку
//! (§12.26): склад это учтённое имущество базы, а не место, куда надо сходить.
//! С **пола** снаряжение при этом не берётся никогда: то, что валяется, сперва
//! свезёт на склад обычная уборка (§12.16), и инвариант «ничего не исчезает с
//! пола в лапы» остаётся целым.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::BaseMap;

/// Одевает котов на базе по шаблону, снимая недостающее со склада.
///
/// Комплект **добирается по частям**: шаблон — набор независимых вещей, а не
/// цена, и правило «либо всё, либо ничего» (§12.24) к нему не относится. Надел
/// один предмет из двух — это промежуточный результат, а не половинчатая покупка.
///
/// Порядок обхода задан явно — котов по `id`, кучи по клеткам карты: когда
/// комбинезонов меньше, чем котов, «кому достанется» видно игроку, а обход
/// сущностей ECS зависит от истории вставок и недетерминирован (§11, §12.24).
///
/// Ушедшего с базы не экипируем: его нет в мире базы, и склад до него не
/// дотягивается — как не дотягиваются усталость и событие таймлайна (§12.22).
pub(crate) fn auto_equip(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    loadout: Res<LoadoutRules>,
    mut commands: Commands,
    cats: Query<(Entity, &UnitId, Option<&Gear>), Without<Away>>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    if loadout.0.is_empty() {
        return;
    }

    // Кто чего недосчитался, в порядке `id`.
    let mut naked: Vec<(&str, Entity, Vec<usize>, Vec<usize>)> = cats
        .iter()
        .filter_map(|(cat_e, id, gear)| {
            let worn: Vec<usize> = gear.map(|g| g.0.clone()).unwrap_or_default();
            let missing: Vec<usize> = loadout
                .0
                .iter()
                .copied()
                .filter(|item| !worn.contains(item))
                .collect();
            (!missing.is_empty()).then_some((id.0.as_str(), cat_e, worn, missing))
        })
        .collect();
    if naked.is_empty() {
        return;
    }
    naked.sort_unstable_by_key(|&(id, ..)| id);

    for (_, cat_e, mut worn, missing) in naked {
        let before = worn.len();
        for item in missing {
            if take_from_storage(&map, &tiles, &mut commands, &mut stacks, item) {
                worn.push(item);
            }
        }
        if worn.len() == before {
            continue; // на складе пусто — кот идёт как есть, это не ошибка
        }
        // Компонент переписывается целиком: «надетого сверх шаблона» не бывает,
        // и собрать список заново дешевле, чем править его на месте.
        commands.entity(cat_e).insert(Gear(worn));
    }
}

/// Снять одну штуку предмета со склада; `false` — там его нет.
///
/// Кучи обходятся по клеткам карты, а не в порядке сущностей: любой
/// недетерминизм ломает и тесты, и модель времени (§11).
fn take_from_storage(
    map: &BaseMap,
    tiles: &TileRules,
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    item: usize,
) -> bool {
    let pile = stacks
        .iter()
        .filter(|(_, _, stack)| stack.item == item && stack.count > 0)
        // Лежащее на полу — ещё не имущество базы: платит склад (§12.24).
        .filter(|(_, pos, _)| tiles.capacity_of(map.tile_at(pos.x, pos.y)) > 0)
        .map(|(pile_e, pos, _)| (pos.y, pos.x, pile_e))
        .min_by_key(|&(y, x, _)| (y, x));
    let Some((.., pile_e)) = pile else {
        return false;
    };
    let Ok((_, _, mut stack)) = stacks.get_mut(pile_e) else {
        return false;
    };
    stack.count -= 1;
    if stack.count <= 0 {
        commands.entity(pile_e).despawn();
    }
    true
}
