//! Голод: сытость, приём пищи и цена пустого желудка (§12.20, §12.36).
//!
//! Вторая потребность после усталости, и §12.20 отложил её ровно до того, что
//! теперь есть: типы предметов (§12.21), производство (§12.30) и доставка
//! (§12.15). Устроена она так же — шкала, порог и **задача**, которую кот
//! назначает себе сам, — но удовлетворитель у неё другой природы.
//!
//! **Еда — свойство предмета, а не тайл-кухня.** `nutrition` у записи в `items:`
//! делает предмет съедобным ровно так же, как `force` делает его надеваемым
//! (§12.29): в кучах, в лапах и на складе паёк живёт как лом. Поэтому приём
//! пищи — копия экипировки (§12.34): кот идёт к ближайшей куче с едой (на
//! складе или прямо на полу) и съедает штуку на месте, мгновенно по приходу.
//! Вся длительность задачи — дорога.
//!
//! **Порог один.** Ниже `hungry` кот идёт есть, но только свободным: начатое
//! дело голод не срывает. Второго порога (§12.33) нет намеренно — голод уводит
//! кота с работы чужими руками: на пустой желудок бодрость горит в `starve` раз
//! быстрее, кот раньше упирается в критический порог и уходит спать, а
//! проснувшегося подберёт `assign_eat`. Второй механизм срыва был бы §12.33,
//! переписанным под другим именем.
//!
//! **Пустой желудок стоит бодрости, а не жизни.** Голод не убивает и не калечит
//! (§12.5 — отдельный вопрос с отдельной ценой): в песочнице, которую игрок
//! щупает, необратимое наказание за неразобранную механику читается как
//! поломка (§12.10). Цена при этом видна и без цифр — база, забывшая про еду,
//! всё время спит.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::{work_spot, worked_cells};
use crate::map::BaseMap;
use crate::path::Reach;

/// Сытости тратится за тик — одинаково за работу, ходьбу, простой и сон.
const HUNGER_PER_TICK: i32 = 1;

/// Отправляет голодных котов к ближайшей куче с едой.
///
/// Раздаётся **первым** из всех работ, даже раньше отдыха: порядок раздатчиков
/// и есть приоритет (§12.15), а ходка за едой короткая и разовая, тогда как сон
/// длится сотни тиков. Кот, уснувший голодным, просыпается голодным же — только
/// позже и потратив вдвое больше бодрости по дороге.
///
/// Берёт **только свободных**: начатое дело голод не срывает, как не срывает его
/// приказ игрока (§12.15) и обычная усталость (§12.20). Кота в отряде тоже не
/// трогаем — в отличие от экипировки (§12.34), которая уходит с отрядом в поле
/// и меняет исход; сытость за шлюзом не считается вовсе.
///
/// Порядок обхода задан явно, котов по `id`: когда пайков меньше, чем голодных,
/// «кому достанется» видно игроку, а обход сущностей ECS зависит от истории
/// вставок и недетерминирован (§11, §12.24).
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_eat(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    items: Res<ItemRules>,
    techs: Res<Techs>,
    food: Res<FoodRules>,
    mut commands: Commands,
    cats: Query<
        (Entity, &UnitId, &Position, &Fed),
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
            Without<Squad>,
            Without<Path>,
            Without<Away>,
        ),
    >,
    going: Query<&Eating>,
    stacks: Query<(Entity, &Position, &Stack)>,
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
) {
    if food.max <= 0 {
        return; // голода в мире нет
    }

    let mut hungry: Vec<(&str, Entity, (i32, i32))> = cats
        .iter()
        .filter(|(_, _, _, fed)| fed.0 <= food.hungry)
        .map(|(cat_e, id, pos, _)| (id.0.as_str(), cat_e, (pos.x, pos.y)))
        .collect();
    if hungry.is_empty() {
        return;
    }
    hungry.sort_unstable_by_key(|&(id, ..)| id);

    // Сколько в каждой куче уже обещано тем, кто к ней идёт. Это не
    // резервирование источника, которое §12.15 отверг для лома, а отказ гнать
    // троих через полбазы за одним пайком (§12.34).
    let mut taken: Vec<(Entity, i32)> = Vec::new();
    for job in &going {
        match taken.iter_mut().find(|(pile, _)| *pile == job.pile) {
            Some((_, n)) => *n += 1,
            None => taken.push((job.pile, 1)),
        }
    }
    // Проданное коту не еда (§12.50). Проверяем это здесь, а не только при
    // укусе: иначе кот ходил бы к обещанной куче каждый тик и возвращался ни
    // с чем — та самая видимая глупость, которой избегает §12.34.
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());
    let free = |item: usize| !booked.iter().any(|&(sold, _)| sold == item);

    for (_, cat_e, from) in hungry {
        // Обход строим только когда есть за чем идти: база без единого пайка
        // иначе гоняла бы BFS на каждого голодного кота каждый тик.
        let anything = stacks.iter().any(|(pile_e, _, stack)| {
            items.nutrition_of(stack.item, &techs) > 0
                && free(stack.item)
                && stack.count > claimed(&taken, pile_e)
        });
        if !anything {
            return; // еды нет ни для кого — остальным тем более
        }

        let reach = Reach::all(&map, &tiles, from);
        let found = stacks
            .iter()
            .filter(|(pile_e, _, stack)| {
                items.nutrition_of(stack.item, &techs) > 0
                    && free(stack.item)
                    && stack.count > claimed(&taken, *pile_e)
            })
            .filter_map(|(pile_e, pos, stack)| {
                // Идут не на кучу, а к **подходу** к ней: паёк лежит на
                // стеллаже, а на стеллаж не встать (§12.142).
                let (spot, d) = work_spot(&map, &tiles, &reach, (pos.x, pos.y), &[])?;
                Some((d, (pos.x, pos.y), spot, pile_e, stack.item))
            })
            // При равном расстоянии — по карте, а не по порядку сущностей:
            // обход ECS зависит от истории вставок (§11). Ключ — клетка **кучи**:
            // подход у двух куч бывает общий.
            .min_by_key(|&(d, at, ..)| (d, at.1, at.0));
        let Some((_, _, at, pile_e, item)) = found else {
            continue; // до еды не дойти — кот работает голодным, это не ошибка
        };

        match taken.iter_mut().find(|(pile, _)| *pile == pile_e) {
            Some((_, n)) => *n += 1,
            None => taken.push((pile_e, 1)),
        }
        let path = reach.path_to(at.0, at.1).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Eating { item, pile: pile_e }, Path { steps: path }));
    }
}

/// Сколько штук из кучи уже обещано тем, кто к ней идёт.
fn claimed(taken: &[(Entity, i32)], pile: Entity) -> i32 {
    taken
        .iter()
        .find(|(p, _)| *p == pile)
        .map_or(0, |&(_, n)| n)
}

/// Дошедший до кучи кот съедает штуку; кучи не оказалось — задача снимается.
///
/// Промах — легальный исход, а не ошибка: кучу мог унести носильщик или доесть
/// нацелившийся на неё сосед. Кот просто освобождается, и следующим тиком
/// раздатчик подберёт ему другую кучу — ровно как с ломом (§12.15).
///
/// **За ходку — одна штука** (§12.34): не хватило на полную сытость — кот сходит
/// ещё раз. Излишек сверх потолка пропадает: съеденное больше, чем влезает, —
/// это не запас на будущее.
pub(crate) fn work_eat(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    items: Res<ItemRules>,
    techs: Res<Techs>,
    food: Res<FoodRules>,
    mut commands: Commands,
    mut cats: Query<(Entity, &Position, &Eating, &mut Fed), Without<Path>>,
    mut stacks: Query<(&Position, &mut Stack)>,
    deals: Query<&Deal>,
    in_paws: Query<(&Haul, &Carrying)>,
) {
    // Проданный паёк коту уже не принадлежит (§12.50). Правило то же, что у
    // стройки и платы складом: с заявки товаром распоряжается сделка. Голод от
    // этого не убивает — он жжёт бодрость (§12.36), а сделку коты донесут.
    let booked = crate::trade::booked(deals.iter(), in_paws.iter());
    for (cat_e, pos, job, mut fed) in &mut cats {
        commands.entity(cat_e).remove::<Eating>();

        let Ok((pile_pos, mut stack)) = stacks.get_mut(job.pile) else {
            continue; // кучи больше нет
        };
        // Дотягивается ли кот до кучи: своя клетка или заставленный сосед
        // (§12.142) — паёк со стеллажа берут с прохода.
        if !worked_cells(&map, &tiles, (pos.x, pos.y)).contains(&(pile_pos.x, pile_pos.y))
            || stack.item != job.item
            || stack.count <= 0
        {
            continue; // куча уехала или опустела, пока кот шёл
        }
        if booked.iter().any(|&(item, _)| item == job.item) {
            continue; // паёк обещан покупателю
        }
        stack.count -= 1;
        if stack.count <= 0 {
            commands.entity(job.pile).despawn();
        }
        fed.0 = (fed.0 + items.nutrition_of(job.item, &techs)).min(food.max);
    }
}

/// Время идёт для всех: сытость тратится и во сне тоже.
///
/// Спящего голод не будит (`assign_rest` и `assign_eat` не пересекаются: спящий
/// для раздатчика еды занят), и это осознанно — кот доспит и поест
/// проснувшимся. Ушедших с базы голод не берёт вовсе, как и усталость: в поле
/// кот не симулируется, он считается (§12.22).
pub(crate) fn hunger(mut cats: Query<&mut Fed, (With<UnitId>, Without<Away>)>) {
    for mut fed in &mut cats {
        fed.0 = (fed.0 - HUNGER_PER_TICK).max(0);
    }
}
