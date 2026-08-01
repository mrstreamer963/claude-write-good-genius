//! Потребности: усталость и отдых (§12.20 concept.md).
//!
//! Бодрость тратится по очку за тик бодрствования и возвращается сном. Отдых —
//! **задача общего слоя** (`Rest`), а не множитель скорости: скорость уже
//! принадлежит навыку (§12.17), а задача видна в поведении — кот встал и ушёл
//! спать. Это первая задача, которую кот назначает себе сам; все четыре
//! остальные приходят от разметки игрока.
//!
//! Порогов у бодрости два (§12.33), и раздатчик читает оба:
//!   * `tired` — уставший **и свободный** кот идёт к лежанке. Начатое дело он
//!     при этом не бросает, как не бросает его и по приказу игрока (§12.15).
//!   * `critical` — кот бросает начатое и уходит спать сам. Без него половина
//!     сна на базе происходила на полу: длинная задача гарантированно упирается
//!     в ноль по дороге.
//!
//! Плюс `collapse_exhausted`: на нуле бодрости кот валится там, где стоит.
//! Второй порог его не отменяет — в поле, без лежанок и с выключенным
//! `AutoRest` до нуля по-прежнему доходят.
//!
//! Фаза отдыха отдельно не хранится: `Rest` с маршрутом — идёт спать, `Rest`
//! без маршрута — уже спит (так же устроен `Haul`).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::release_claim;
use crate::map::BaseMap;
use crate::path::Reach;

/// Бодрости тратится за тик бодрствования.
const TIRE_PER_TICK: i32 = 1;

/// Кот на нуле бодрости засыпает где стоит, отпустив задачу.
///
/// Чертёж и пометку кучи надо освободить явно: иначе площадка останется
/// «занятой» спящим и её больше никто не возьмёт.
///
/// А вот из **отряда** истощённый не выпадает: состав выбрал игрок, заменить
/// бойца некем, и вылазка просто ждёт, пока тот выспится (§12.23). Это и есть
/// разница с приказом игрока — тот распускает отряд, потому что это решение,
/// а не случившееся. Ушедшего с базы истощение не берёт: вне базы он не устаёт.
pub(crate) fn collapse_exhausted(
    mut commands: Commands,
    cats: Query<
        (
            Entity,
            &Energy,
            Option<&Assignment>,
            Option<&Haul>,
            Option<&Researching>,
            Option<&Crafting>,
        ),
        (With<UnitId>, Without<Rest>, Without<Away>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut marks: Query<&mut ToStore>,
    mut topics: Query<&mut Research>,
    mut orders: Query<&mut Craft>,
) {
    for (cat_e, energy, assignment, haul, researching, crafting) in &cats {
        if energy.0 > 0 {
            continue;
        }
        release_work(
            &mut commands,
            cat_e,
            (assignment, haul, researching, crafting),
            &mut blueprints,
            &mut marks,
            &mut topics,
            &mut orders,
        );
        // Лежанку упавший не занимает — падает где стоит, а не идёт к месту.
        commands.entity(cat_e).insert(Rest { spot: None });
    }
}

/// Отпустить всё, за что кот держался: чертёж, кучу, тему, заказ и парту.
///
/// Одна на два случая, когда усталость забирает кота с работы, —
/// `collapse_exhausted` (ноль бодрости) и критический порог в `assign_rest`
/// (§12.33). Две копии этой арифметики разошлись бы на первой же новой задаче,
/// а площадка молча осталась бы за спящим.
///
/// Груз кот не роняет: донесёт, когда выспится (§12.15). Парту, наоборот,
/// отпускает — занятость держит сам `Study`, и уснувший ученик держал бы её
/// вечно (§12.18). Оплаченная штука заказа не пропадает: `paid` живёт у
/// заказа, а не у кота (§12.30).
///
/// `Rest` и маршрут к лежанке вешает **вызывающий, после** этого вызова:
/// команды применяются в порядке добавления, и `remove::<Path>` отсюда снёс бы
/// только что выданный маршрут.
#[allow(clippy::too_many_arguments)]
fn release_work(
    commands: &mut Commands,
    cat_e: Entity,
    work: (
        Option<&Assignment>,
        Option<&Haul>,
        Option<&Researching>,
        Option<&Crafting>,
    ),
    blueprints: &mut Query<&mut Blueprint>,
    marks: &mut Query<&mut ToStore>,
    topics: &mut Query<&mut Research>,
    orders: &mut Query<&mut Craft>,
) {
    let (assignment, haul, researching, crafting) = work;
    if let Some(bp_e) = assignment.map(|a| a.0) {
        if let Ok(mut bp) = blueprints.get_mut(bp_e) {
            bp.assignee = None;
        }
    }
    match haul.map(|h| h.0) {
        Some(HaulTo::Site(bp_e)) => {
            if let Ok(mut bp) = blueprints.get_mut(bp_e) {
                bp.hauler = None;
            }
        }
        Some(HaulTo::Store(pile)) => release_claim(marks, pile),
        None => {}
    }
    if let Some(mut topic) = researching.and_then(|r| topics.get_mut(r.0).ok()) {
        topic.assignee = None;
        topic.spot = None;
    }
    if let Some(mut order) = crafting.and_then(|c| orders.get_mut(c.0).ok()) {
        order.assignee = None;
        order.spot = None;
    }
    commands.entity(cat_e).remove::<(
        Assignment,
        Haul,
        Study,
        Researching,
        Crafting,
        Equipping,
        Path,
        MoveCooldown,
    )>();
}

/// Отправляет уставших котов к ближайшей лежанке — тремя проходами по одному
/// и тому же списку свободных мест.
///
/// Раздаётся **первым** из всех работ: порядок раздатчиков и есть приоритет
/// (§12.15), иначе уставшего кота тут же уводит подвоз материала.
///
/// Проходы идут по убыванию нужды, и это и есть распределение дефицита
/// (§12.33):
///   1. **Критически уставшие занятые** — ниже `critical` кот бросает начатое.
///      Первыми, потому что до нуля им остались считаные тики.
///   2. **Уставшие свободные** — ниже `tired`, начатого при этом нет.
///   3. **Упавшие на полу** — те, кого уже подобрал `collapse_exhausted`,
///      перебираются на освободившуюся лежанку. Последними: они хотя бы спят,
///      пусть и вшестеро медленнее, а первые двое иначе не спят вовсе.
///
/// **Лежанка занята, пока на неё идут или на ней спят.** Делить её нельзя:
/// место для сна — ресурс, и если его можно занимать вдвоём, число лежанок
/// перестаёт на что-либо влиять (§12.20). Занятость не хранится отдельным
/// claim — её держит сам `Rest` спящего кота, поэтому она снимается вместе с
/// задачей: просыпанием, приказом игрока, чем угодно.
///
/// Свободной лежанки нет или до неё не дойти — кот продолжает работать до нуля
/// бодрости, и тогда его подберёт `collapse_exhausted`. Это и есть цена базы
/// с недостроенной зоной отдыха: не запрет работать, а медленный сон на полу.
/// По той же причине критический порог **срывает с работы только под лежанку**:
/// бросить дело и остаться стоять — это ни сна, ни работы.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_rest(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    auto: Res<AutoRest>,
    mut commands: Commands,
    resting: Query<(&Position, &Rest)>,
    free_cats: Query<
        (Entity, &Position, &Energy),
        (
            With<UnitId>,
            Without<Assignment>,
            Without<Haul>,
            Without<Rest>,
            Without<Study>,
            Without<Researching>,
            Without<Crafting>,
            Without<Equipping>,
            Without<Squad>,
            Without<Path>,
        ),
    >,
    // Ровно дополнение к `free_cats`: занят тот, у кого есть хоть одна задача
    // или маршрут. Два непересекающихся множества, поэтому кота не разберут
    // дважды — иначе первый проход выдал бы ему лежанку, а второй вторую.
    busy_cats: Query<
        (
            Entity,
            &Position,
            &Energy,
            Option<&Assignment>,
            Option<&Haul>,
            Option<&Researching>,
            Option<&Crafting>,
        ),
        (
            With<UnitId>,
            Without<Rest>,
            Without<Away>,
            Or<(
                With<Assignment>,
                With<Haul>,
                With<Study>,
                With<Researching>,
                With<Crafting>,
                With<Equipping>,
                With<Squad>,
                With<Path>,
            )>,
        ),
    >,
    fallen: Query<
        (Entity, &Position, &Energy, &Rest),
        (With<UnitId>, Without<Path>, Without<Away>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut marks: Query<&mut ToStore>,
    mut topics: Query<&mut Research>,
    mut orders: Query<&mut Craft>,
) {
    // Занято и то, к чему идут, и то, на чём лежат: кот, свалившийся прямо на
    // лежанку, места в `Rest` не держит, но занимает его собой.
    let mut taken: Vec<(i32, i32)> = Vec::new();
    for (pos, rest) in &resting {
        taken.extend(rest.spot);
        if rules.rest_of(map.tile_at(pos.x, pos.y)) > 0 {
            taken.push((pos.x, pos.y));
        }
    }

    let mut free: Vec<(i32, i32)> = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| rules.rest_of(map.tile_at(x, y)) > 0)
        .filter(|cell| !taken.contains(cell))
        .collect();
    if free.is_empty() {
        return; // лежанок нет вовсе или все заняты — работаем до упора
    }

    // 1. Критический порог: кот бросает начатое, но только если ему есть куда
    // лечь. Выключатель игрока отменяет ровно это — второй порог, а не сон.
    if auto.0 && needs.critical > 0 {
        for (cat_e, pos, energy, assignment, haul, researching, crafting) in &busy_cats {
            if energy.0 > needs.critical || free.is_empty() {
                continue;
            }
            let Some(bed) = claim_bed(&map, &mut free, (pos.x, pos.y)) else {
                continue; // до свободной лежанки не добраться — работает дальше
            };
            release_work(
                &mut commands,
                cat_e,
                (assignment, haul, researching, crafting),
                &mut blueprints,
                &mut marks,
                &mut topics,
                &mut orders,
            );
            // Маршрут вешается строго после освобождения задачи: `release_work`
            // снимает `Path`, а команды применяются в порядке добавления.
            lie_down(&mut commands, cat_e, bed);
        }
    }

    // 2. Обычный порог: свободный кот уходит спать, ничего не бросая.
    for (cat_e, pos, energy) in &free_cats {
        if energy.0 > needs.tired || free.is_empty() {
            continue;
        }
        if let Some(bed) = claim_bed(&map, &mut free, (pos.x, pos.y)) {
            lie_down(&mut commands, cat_e, bed);
        }
    }

    // 3. Переезд упавших: лежанка могла освободиться, пока кот спал на полу
    // (§12.33). Порог тот же, что и у ухода спать, — иначе кот, доспавший до
    // `tired`, пошёл бы через полбазы ради последних очков бодрости.
    for (cat_e, pos, energy, rest) in &fallen {
        if free.is_empty() {
            return;
        }
        if rest.spot.is_some() || energy.0 > needs.tired {
            continue; // лежанка у кота уже есть либо он почти выспался
        }
        if rules.rest_of(map.tile_at(pos.x, pos.y)) > 0 {
            continue; // упал прямо на лежанку — переезжать некуда и незачем
        }
        if let Some(bed) = claim_bed(&map, &mut free, (pos.x, pos.y)) {
            lie_down(&mut commands, cat_e, bed);
        }
    }
}

/// Занятая котом лежанка и маршрут к ней.
type Bed = ((i32, i32), Vec<(i32, i32)>);

/// Забрать из `free` ближайшую достижимую лежанку и маршрут к ней; `None` —
/// свободных не осталось или ни до одной не дойти.
///
/// Выбор по расстоянию — то же правило, что у любого раздатчика (§12.14).
fn claim_bed(map: &BaseMap, free: &mut Vec<(i32, i32)>, from: (i32, i32)) -> Option<Bed> {
    let reach = Reach::all(map, from);
    let (i, cell, _) = free
        .iter()
        .enumerate()
        .filter_map(|(i, &(x, y))| reach.dist_at(x, y).map(|d| (i, (x, y), d)))
        .min_by_key(|&(_, _, d)| d)?;
    free.remove(i);
    Some((cell, reach.path_to(cell.0, cell.1).unwrap_or_default()))
}

/// Отправить кота к занятой им лежанке. Пустой маршрут значит «кот уже на
/// месте»; снимет его `move_units`, и со следующего тика кот спит.
fn lie_down(commands: &mut Commands, cat_e: Entity, bed: Bed) {
    let (cell, path) = bed;
    commands.entity(cat_e).insert((
        Rest { spot: Some(cell) },
        Path { steps: path },
        MoveCooldown(0),
    ));
}

/// Спящие коты восстанавливают бодрость и просыпаются на полной.
///
/// Скорость даёт клетка под котом: лежанка — свою, всё остальное — общий
/// `floor` из рулсета. Ноль не берём никогда: спать можно где угодно, вопрос
/// только в том, сколько это займёт, — иначе кот, уснувший на голом полу
/// рулсета без `floor`, не проснулся бы вовсе.
pub(crate) fn sleep(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut commands: Commands,
    mut cats: Query<(Entity, &Position, &mut Energy, Option<&Path>), With<Rest>>,
) {
    for (cat_e, pos, mut energy, path) in &mut cats {
        if path.is_some() {
            continue; // ещё идёт к лежанке
        }
        let rate = rules
            .rest_of(map.tile_at(pos.x, pos.y))
            .max(needs.floor)
            .max(1);
        energy.0 = (energy.0 + rate).min(needs.max);
        if energy.0 >= needs.max {
            commands.entity(cat_e).remove::<Rest>();
        }
    }
}

/// Бодрствование стоит бодрости — одинаково за работу, ходьбу и простой.
/// Кроме тех, кто на миссии: вне базы усталость не считается вовсе (§12.22).
pub(crate) fn tire(mut cats: Query<&mut Energy, (With<UnitId>, Without<Rest>, Without<Away>)>) {
    for mut energy in &mut cats {
        energy.0 = (energy.0 - TIRE_PER_TICK).max(0);
    }
}
