//! Снаряжение: приписка на кота, за которой кот идёт сам (§4.2, §12.29,
//! §12.34, §12.268 concept.md).
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
use crate::jobs::{work_spot, worked_cells};
use crate::map::BaseMap;
use crate::path::Reach;

/// Почему коту нельзя приписать вещь или снять её приписку — тегом (§12.268).
///
/// Одно выражение на фасад (`Sim::set_outfit`) и снимок (`EntitySnap::outfit_gates`),
/// идиома `skills::desk_gate` (§12.196): слово по тегу подбирает вид. Пустая строка —
/// можно.
///
/// * `personal` — личная вещь (§12.256): не приписывается и не снимается;
/// * `away` — кота нет на базе: ушедший отряд заморожен (§12.22);
/// * `unseen` — вещь ни разу не бывала на базе (`Seen`, §12.131): назначить то,
///   о чём база не знает, — обещание механики, которой для игрока ещё нет;
/// * `unknown` — база вещь не поняла (§12.114) — ворота на команде, как у правил (§12.93);
/// * `collector` — у кота уже есть прибор сбора: второй ему ни к чему (§12.256).
///
/// Снятие спрашивает только первые два: запертая отмена оставила бы приписку
/// несбрасываемой (довод §12.93).
#[allow(clippy::too_many_arguments)]
pub(crate) fn outfit_gate(
    items: &ItemRules,
    techs: &Techs,
    seen: &Seen,
    item: usize,
    on: bool,
    away: bool,
    gear: Option<&Gear>,
    outfit: Option<&Outfit>,
) -> &'static str {
    if items.personal(item) {
        return "personal";
    }
    if away {
        return "away";
    }
    if !on {
        return "";
    }
    if !seen.saw(item) {
        return "unseen";
    }
    if !items.equippable(item) || !items.wearable(item, techs) {
        return "unknown";
    }
    // Прибор сбора уже приписан или надет (например, личный анализатор).
    let collector = items.collects(item)
        && (gear.is_some_and(|g| g.items().any(|i| i != item && items.collects(i)))
            || outfit.is_some_and(|o| o.0.iter().any(|&i| i != item && items.collects(i))));
    if collector {
        return "collector";
    }
    ""
}

/// На сколько выходов изнашивается вещь за вылазку с долей `share` (§12.268).
///
/// Та же доля, что у добычи и ран (инвариант 14), — новой арифметики исхода нет:
/// полный успех снимает выход, неполный — больше, по выходу на каждую
/// недобранную четверть (75 % — два, 50 % — три). Провал вещи сжигает целиком
/// и сюда не приходит. Зовут конец работы (`run_missions`) и прогноз штаба.
pub(crate) fn wear_toll(share: i32) -> i32 {
    1 + (100 - share.clamp(0, 100)) / 25
}

/// Износ надетого за вылазку (§12.268): что осталось на коте. Вещь без `wear`
/// и личная не изнашиваются; дошедшая до нуля исчезает — приписка поведёт кота
/// за новой.
pub(crate) fn worn_after(items: &ItemRules, gear: &Gear, share: i32) -> Vec<Worn> {
    let toll = wear_toll(share);
    gear.0
        .iter()
        .filter_map(|w| {
            if items.wear_of(w.item) <= 0 || items.personal(w.item) {
                return Some(*w);
            }
            let left = w.left - toll;
            (left > 0).then_some(Worn { item: w.item, left })
        })
        .collect()
}

/// Отправляет котов за недостающими вещами приписки (§12.268).
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
    tiles: Res<TileRules>,
    items: Res<ItemRules>,
    techs: Res<Techs>,
    mut commands: Commands,
    cats: Query<
        (
            Entity,
            &UnitId,
            &Position,
            Option<&Gear>,
            &Outfit,
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
            Without<OnDuty>,
            Without<Away>,
        ),
    >,
    going: Query<&Equipping>,
    stacks: Query<(Entity, &Position, &Stack)>,
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
    topics: Query<&Research>,
    topic_rules: Res<ResearchRules>,
) {
    // Кто чего недосчитался, в порядке `id`.
    let mut naked: Vec<(&str, Entity, (i32, i32), Vec<usize>)> = cats
        .iter()
        .filter(|(_, _, _, _, _, path, squad)| path.is_none() || squad.is_some())
        .filter_map(|(cat_e, id, pos, gear, outfit, ..)| {
            // Ворота на надевание (§12.114) проверяет команда; здесь они
            // остаются подстраховкой — технологии не забываются (§12.18).
            let missing: Vec<usize> = outfit
                .0
                .iter()
                .copied()
                .filter(|&item| !gear.is_some_and(|g| g.has(item)) && items.wearable(item, &techs))
                // Второй прибор сбора коту ни к чему (§12.256): сбор идёт один
                // на отряд, и пробоотборник на Антенне с её анализатором —
                // вещь, отнятая у того, кому её не хватило.
                .filter(|&item| !(items.collects(item) && items.collects_any(gear)))
                .collect();
            (!missing.is_empty()).then_some((id.0.as_str(), cat_e, (pos.x, pos.y), missing))
        })
        .collect();
    if naked.is_empty() {
        return;
    }
    naked.sort_unstable_by_key(|&(id, ..)| id);

    // Проданный комбинезон коту уже не принадлежит (§12.50): с заявки товаром
    // распоряжается сделка. Проверяем в раздатчике, а не только при надевании,
    // иначе кот ходил бы к обещанной куче и возвращался ни с чем.
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());
    let sold = |item: usize| booked.iter().any(|&(s, _)| s == item);

    // **Заведённая тема-вскрытие важнее шаблона** (§12.133): образец, который
    // ей ещё везут, шаблон не разбирает. Без этого «ждёт образец» длилось бы
    // вечно на базе, где кто-то всегда не одет, — а комбинезонов привозят по
    // одному.
    //
    // Уступка ровно одна, и **незаведённой** теме не уступает ничего. Соблазн
    // придержать «последний экземпляр, у которого есть неизученная тема»
    // отвергнут: кот ушёл бы на вылазку раздетым, отряд стал бы слабее, а
    // причина лежала бы в чужой механике — скрытый штраф вместо слова (§12.53).
    // Тот же спор §12.115 уже решил в эту сторону: разбор уступает шаблону.
    let mut owed: Vec<(usize, i32)> = Vec::new();
    for topic in &topics {
        for (item, need) in topic_missing(&topic_rules, topic) {
            match owed.iter_mut().find(|(i, _)| *i == item) {
                Some((_, n)) => *n += need,
                None => owed.push((item, need)),
            }
        }
    }
    // Уже везомое лабораториям из недостачи вычтено самой `topic_missing`
    // только по `delivered`; груз в лапах ещё не сдан, поэтому вычитаем его
    // здесь — иначе один комбинезон числился бы обещанным дважды.
    for (haul, load) in &in_paws {
        if let HaulTo::Lab(_) = haul.to
            && let Some(slot) = owed.iter_mut().find(|(i, _)| *i == load.item)
        {
            slot.1 -= load.count;
        }
    }
    let promised = |item: usize| {
        owed.iter()
            .find(|&&(i, _)| i == item)
            .map_or(0, |&(_, n)| n)
    };
    let in_piles = |item: usize| -> i32 {
        stacks
            .iter()
            .filter(|(_, _, s)| s.item == item)
            .map(|(_, _, s)| s.count)
            .sum()
    };
    let free = |item: usize| !sold(item) && in_piles(item) > promised(item).max(0);

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
            free(item)
                && stacks.iter().any(|(pile_e, _, stack)| {
                    stack.item == item && stack.count > claimed(&taken, pile_e)
                })
        });
        if !anything {
            continue;
        }

        let reach = Reach::all(&map, &tiles, from);
        // Кучи по шаблону: первая вещь, за которой вообще есть куда идти.
        let found = missing
            .iter()
            .filter(|&&item| free(item))
            .find_map(|&item| {
                stacks
                    .iter()
                    .filter(|(pile_e, _, stack)| {
                        stack.item == item && stack.count > claimed(&taken, *pile_e)
                    })
                    .filter_map(|(pile_e, pos, _)| {
                        // Идут не на кучу, а к **подходу** к ней: комбинезон
                        // лежит на стеллаже, а на стеллаж не встать (§12.142).
                        let (spot, d) = work_spot(&map, &tiles, &reach, (pos.x, pos.y), &[])?;
                        Some((d, (pos.x, pos.y), spot, pile_e, item))
                    })
                    // При равном расстоянии — по карте, а не по порядку сущностей:
                    // обход ECS зависит от истории вставок (§11). Ключ — клетка
                    // **кучи**: подход у двух куч бывает общий.
                    .min_by_key(|&(d, at, ..)| (d, at.1, at.0))
            });
        let Some((_, _, at, pile_e, item)) = found else {
            continue; // до вещи не дойти — кот идёт как есть, это не ошибка
        };

        match taken.iter_mut().find(|(pile, _)| *pile == pile_e) {
            Some((_, n)) => *n += 1,
            None => taken.push((pile_e, 1)),
        }
        let path = reach.path_to(at.0, at.1).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Equipping { item, pile: pile_e }, Path { steps: path }));
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
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    items: Res<ItemRules>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Equipping, Option<&Gear>), Without<Path>>,
    mut stacks: Query<(&Position, &mut Stack)>,
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
) {
    // Вещь, обещанную покупателю, кот не надевает (§12.50).
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());
    for (cat_e, pos, job, gear) in &cats {
        commands.entity(cat_e).remove::<Equipping>();

        let Ok((pile_pos, mut stack)) = stacks.get_mut(job.pile) else {
            continue; // кучи больше нет
        };
        // Дотягивается ли кот до кучи: своя клетка или заставленный сосед
        // (§12.142) — вещь со стеллажа берут с прохода.
        if !worked_cells(&map, &tiles, (pos.x, pos.y)).contains(&(pile_pos.x, pile_pos.y))
            || stack.item != job.item
            || stack.count <= 0
        {
            continue; // куча уехала или опустела, пока кот шёл
        }
        if booked.iter().any(|&(item, _)| item == job.item) {
            continue; // вещь обещана покупателю
        }
        stack.count -= 1;
        if stack.count <= 0 {
            commands.entity(job.pile).despawn();
        }
        // Компонент переписывается целиком: «надетого сверх приписки» не бывает,
        // и собрать список заново дешевле, чем править его на месте.
        let mut worn = gear.map(|g| g.0.clone()).unwrap_or_default();
        worn.push(items.fresh(job.item));
        commands.entity(cat_e).insert(Gear(worn));
    }
}
