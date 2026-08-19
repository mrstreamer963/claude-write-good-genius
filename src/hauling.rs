//! Перенос предметов: доставка материала на площадку, уборка пола и возврат при сносе.
//!
//! Перенос — **отдельный тип задачи**, а не фаза стройки (§12.15 concept.md):
//! носильщиком и строителем могут быть разные коты, а `jobs` знает про материал
//! ровно одно — хватает его на площадке или нет.
//!
//! Кот без груза идёт к куче, с грузом — к адресату; фазу задаёт наличие
//! `Carrying`, отдельного состояния нет. Груз сам не падает: пока кот его не
//! сдал, груз в лапах, и такой кот дешевле любого другого для новой доставки.
//!
//! Тип предмета лежит в самой куче и в грузе, поэтому раздача сводит их: коту с
//! грузом годится лишь площадка, которой нужен этот тип, а пустому — лишь куча
//! нужного типа. За ходку кот везёт один тип (§12.21).
//!
//! Адресатов два (`HaulTo`): площадка, которой не хватает материала, и склад —
//! клетка с ёмкостью. Уборка (`assign_tidy`) раздаётся **после** стройки и
//! сноса: она не должна отбирать котов у настоящей работы (§12.16).
//!
//! Объём лап (`Carry`, §12.17) режет только подъём: сколько кот уже несёт, он
//! донесёт. Остаток кучи честнее оставить лежать — раздатчик пришлёт за ним
//! следующую ходку.
//!
//! **Работу делят по остатку, а не по одному коту на адресат** (§12.48).
//! Носильщиков у площадки, сделки и кучи столько, на сколько хватает остатка:
//! каждый идущий обещает ходку (не больше лап и не больше остатка), и пока
//! обещанного меньше, чем нужно, зовётся следующий. Обещания нигде не хранятся —
//! они считаются по самим котам, по их `Haul` и `Carrying`, тем же приёмом,
//! каким `assign_equip` не отправляет троих за одним комбинезоном (§12.34).
//! Отдельного claim'а поэтому нет ни у чертежа, ни у сделки, ни у пометки: его
//! пришлось бы снимать в пяти местах, откуда задачу отбирают, и однажды забыть.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::jobs::build_spot;
use crate::map::{BaseMap, DIRS};
use crate::path::Reach;

// --- доставка на площадку --------------------------------------------------

/// Адресат подвоза для раздачи.
///
/// Площадка и заказ на продажу лежат в одном списке намеренно: снабжаются они
/// одинаково, и `sale` решает ровно одно — какой `HaulTo` выдать (§12.44).
struct Needy {
    target: Entity,
    /// Куда идти; тайл этой клетки читает `build_spot`.
    at: (i32, i32),
    tile: i16,
    /// Чего не хватает — уже за вычетом груза, который сюда везут (§12.48).
    miss: Vec<(usize, i32)>,
    dest: Dest,
    /// Сколько ещё не обещано: остаток минус ходки тех, кто уже идёт. Дошёл до
    /// нуля — адресат снабжён, и новых котов к нему не зовут.
    budget: i32,
}

/// Какому адресату везём. Три, и различаются они **только двумя вещами**:
/// каким `HaulTo` кончится ходка и можно ли брать с пола (§12.69). Всё
/// остальное — расстояние, наводка, счёт обещанного — у них общее, и отдельной
/// раздачи ни один не получил (§12.44, §12.102).
///
/// Порядок вариантов значим: по нему сортируются адресаты при равной клетке,
/// а обход ECS недетерминирован (§11).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Dest {
    Site,
    Shop,
    Sale,
}

impl Dest {
    /// Брать только со склада — то есть отдавать наружу (§12.69). Внутрь базы
    /// (площадка, станок) годится и пол.
    fn storage_only(self) -> bool {
        matches!(self, Dest::Sale)
    }
}

/// Куча-источник в раздаче: сущность (её запоминает наводка), клетка, тип,
/// сколько в ней осталось необещанного и обход от неё.
type Pile = (Entity, (i32, i32), usize, i32, Reach);

/// Сколько этого типа адресату не хватает; ноль — не нужен вовсе.
fn need_of(miss: &[(usize, i32)], item: usize) -> i32 {
    miss.iter()
        .find(|&&(i, _)| i == item)
        .map_or(0, |&(_, n)| n)
}

/// Сколько лежит в куче; кучи может уже не быть — тогда ноль.
fn pile_count(stacks: &Query<(Entity, &Position, &Stack)>, pile: Entity) -> i32 {
    stacks.get(pile).map_or(0, |(_, _, s)| s.count)
}

/// Сколько из каждой кучи расписано на **уборку**: кот идёт за ней и унесёт
/// ходку. Считать это обязаны обе раздачи (§12.49): кучи у них общие, и
/// носильщик, не попавший в счёт, уносит лом из-под того, кто за ним приехал.
fn tidy_promises<'a>(
    going: impl Iterator<Item = (&'a Haul, Option<&'a Carrying>, Option<&'a Carry>)>,
    stacks: &Query<(Entity, &Position, &Stack)>,
) -> Vec<(Entity, i32)> {
    let mut promised: Vec<(Entity, i32)> = Vec::new();
    for (haul, load, carry) in going {
        let HaulTo::Store(Some(pile)) = haul.to else {
            continue;
        };
        if load.is_some() {
            continue; // из кучи уже взял — она посчитана самой собой
        }
        let trip = portion(carry, pile_count(stacks, pile));
        match promised.iter_mut().find(|(e, _)| *e == pile) {
            Some((_, n)) => *n += trip,
            None => promised.push((pile, trip)),
        }
    }
    promised
}

/// Вычесть из недостачи то, что к адресату уже несут — в лапах или обещанием.
fn less_incoming(miss: &mut Vec<(usize, i32)>, incoming: &[(usize, i32)]) {
    for &(item, count) in incoming {
        if let Some(slot) = miss.iter_mut().find(|(i, _)| *i == item) {
            slot.1 -= count;
        }
    }
    miss.retain(|&(_, n)| n > 0);
}

/// Назначает свободных котов на доставку материала к чертежам, которым его не хватает.
///
/// Стоимость назначения — длина всего маршрута: `кот → куча → площадка`. Вторая
/// нога считается честным обходом от кучи, а не расстоянием от кота: иначе при
/// двух кучах кот брал бы ближнюю к себе, даже если из неё до стройки вдвое
/// дальше. Коту с грузом нога до кучи не нужна — он и оказывается дешевле всех.
///
/// Раздача жадная, как и в `assign_jobs` (§12.14): каждый раз берём самую
/// дешёвую пару (кот, чертёж) из оставшихся.
///
/// **Носильщиков у площадки столько, на сколько хватает недостачи** (§12.48).
/// Дорогой тайл — это несколько ходок, и делать их по очереди одним котом,
/// когда рядом стоят свободные, значит превращать базу в одного работника и
/// зрителей. Считается это по самим котам: гружёные вычитаются из недостачи
/// (`less_incoming`), идущие налегке — обещанной ходкой своего типа: за какой
/// кучей и за каким типом пошёл кот, записано наводкой в самой задаче (§12.49).
///
/// Чертежи сноса сюда не попадают: снос ничего не стоит, он материал возвращает.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_hauls(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    blueprints: Query<(Entity, &Blueprint)>,
    deals: Query<(Entity, &Deal)>,
    crafts: Query<(Entity, &Craft)>,
    recipes: Res<CraftRules>,
    stacks: Query<(Entity, &Position, &Stack)>,
    going: Query<(&UnitId, &Haul, Option<&Carrying>, Option<&Carry>)>,
    free_cats: Query<
        (
            Entity,
            &UnitId,
            &Position,
            Option<&Carrying>,
            Option<&Carry>,
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
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    // Что уже везут к каждому адресату — по самим носильщикам, а не по claim'у
    // на адресате (§12.48). Гружёный вычитается из недостачи своим грузом,
    // идущий налегке — обещанной ходкой; тип у обоих известен, потому что у
    // второго он записан наводкой (§12.49).
    let mut incoming: Vec<(Entity, usize, i32)> = Vec::new();
    // Идущие налегке — отдельным списком: их обещание меряется не только лапами
    // и кучей, но и нуждой адресата, а она известна только после того, как из
    // недостачи вычтен весь груз в пути. Порядок — по `id`, чтобы обрезание по
    // остатку не зависело от обхода ECS (§11).
    let mut aiming: Vec<(&str, Entity, Aim, Option<&Carry>)> = Vec::new();
    for (id, haul, load, carry) in &going {
        let target = match haul.to {
            HaulTo::Site(e) | HaulTo::Sale(e) | HaulTo::Shop(e) => e,
            HaulTo::Store(_) => continue,
        };
        match (load, haul.aim) {
            (Some(load), _) => incoming.push((target, load.item, load.count)),
            (None, Some(aim)) => aiming.push((id.0.as_str(), target, aim, carry)),
            // Наводки нет и груза нет: кот идёт к адресату, но за чем — не
            // записано. У живой раздачи такого не бывает; снимок мог приехать
            // из партии, где наводки ещё не было.
            (None, None) => {}
        }
    }
    aiming.sort_unstable_by_key(|&(id, ..)| id);
    let brought = |target: Entity| -> Vec<(usize, i32)> {
        incoming
            .iter()
            .filter(|&&(e, ..)| e == target)
            .map(|&(_, item, count)| (item, count))
            .collect()
    };

    // Адресаты, которым чего-то не хватает сверх уже обещанного.
    let mut needy: Vec<Needy> = blueprints
        .iter()
        .filter_map(|(e, bp)| {
            let mut miss = missing(&rules, bp);
            less_incoming(&mut miss, &brought(e));
            (!miss.is_empty()).then_some(Needy {
                target: e,
                at: (bp.x, bp.y),
                tile: bp.tile,
                miss,
                dest: Dest::Site,
                budget: 0,
            })
        })
        .collect();
    // **Заказ на продажу — третий адресат подвоза** (§12.44), и снабжается он
    // ровно как площадка: игрок разметил, куда деть товар, а несёт его любой
    // свободный кот (§12.16). Отдельного механизма для продажи не заводится —
    // разница только в том, что на месте товар не укладывается в тайл, а
    // превращается в счётчик контейнера, а деньги за него платит `run_trade`
    // по отгрузке (§12.68).
    needy.extend(
        deals
            .iter()
            .filter(|(_, d)| !d.buying)
            .filter_map(|(e, d)| {
                let mut miss = vec![(d.item, d.count - d.delivered)];
                less_incoming(&mut miss, &brought(e));
                (!miss.is_empty()).then(|| Needy {
                    target: e,
                    at: d.cell,
                    tile: map.tile_at(d.cell.0, d.cell.1),
                    miss,
                    dest: Dest::Sale,
                    budget: 0,
                })
            }),
    );
    // **Заказ мастерской — четвёртый адресат подвоза** (§12.102), и снабжается
    // он ровно как площадка: игрок разметил работу, а материал несёт любой
    // свободный кот (§12.16). Отдельной раздачи снова не заводится — только
    // ветка в общем списке. Разница с продажей одна: станок стоит внутри базы,
    // значит брать можно и с пола (§12.69).
    needy.extend(crafts.iter().filter_map(|(e, order)| {
        let mut miss = craft_missing(&recipes, order);
        less_incoming(&mut miss, &brought(e));
        (!miss.is_empty()).then(|| Needy {
            target: e,
            at: order.cell,
            tile: map.tile_at(order.cell.0, order.cell.1),
            miss,
            dest: Dest::Shop,
            budget: 0,
        })
    }));
    // Теперь обещания идущих налегке: каждый увезёт не больше лап, не больше
    // необещанного в куче и не больше, чем адресату нужно этого типа (§12.49).
    // Заодно копится, сколько из кучи расписано, — по этому остатку раздача
    // ниже решает, звать ли к ней ещё кота.
    //
    // Считаются **обе** раздачи разом: уборка ходит к тем же кучам, и её
    // носильщик, не попавший в счёт, уносит лом из-под приехавшего за ним
    // подвоза. Это и была большая часть промахов.
    let mut spoken_for: Vec<(Entity, i32)> =
        tidy_promises(going.iter().map(|(_, h, l, c)| (h, l, c)), &stacks);
    for (_, target, aim, carry) in aiming {
        let Some(n) = needy.iter_mut().find(|n| n.target == target) else {
            continue; // адресат уже снабжён — обещать нечего
        };
        let left = pile_count(&stacks, aim.pile) - claimed(&spoken_for, aim.pile);
        let promise = portion(carry, left.max(0)).min(need_of(&n.miss, aim.item));
        if promise <= 0 {
            continue;
        }
        less_incoming(&mut n.miss, &[(aim.item, promise)]);
        match spoken_for.iter_mut().find(|(e, _)| *e == aim.pile) {
            Some((_, n)) => *n += promise,
            None => spoken_for.push((aim.pile, promise)),
        }
    }

    // Бюджет: остаток после всех обещаний. Ушёл в ноль — адресат снабжён, и
    // звать к нему некого.
    for n in &mut needy {
        n.budget = n.miss.iter().map(|&(_, count)| count).sum::<i32>();
    }
    needy.retain(|n| n.budget > 0);
    if needy.is_empty() {
        return;
    }
    // Все три списка приходят в порядке обхода ECS, а он зависит от истории
    // вставок (§11). Жадный выбор ниже при равной длине маршрута берёт первую
    // пару, поэтому порядок протекал бы в поведение: та же база после загрузки
    // сохранения повезла бы лом иначе (§12.45). Сортируем входы — адресаты и
    // кучи по клетке, коты по `id` (как в `assign_equip`, §12.34). Тип в ключе
    // кучи нужен потому, что на одной клетке лежат кучи разных типов (§12.21),
    // а флаг продажи — потому, что сделка адресуется шлюзом, на котором может
    // стоять и чертёж.
    needy.sort_unstable_by_key(|n| (n.at.1, n.at.0, n.dest));

    let map = &*map;
    let tiles = &*rules;
    // Учтённое — то, что лежит на клетке с ёмкостью (§12.69). Наружу база
    // отдаёт только его: ворота продажи считают склад, и подвоз к посту обязан
    // брать оттуда же. Иначе правило говорит «с пола нельзя», а коты на глазах
    // носят с пола — и спорят с ним картинкой.
    let in_store = |(x, y): (i32, i32)| tiles.capacity_of(map.tile_at(x, y)) > 0;
    let mut idle: Vec<(&str, Entity, Option<(usize, i32)>, Option<&Carry>, Reach)> = free_cats
        .iter()
        .map(|(e, id, p, load, carry)| {
            (
                id.0.as_str(),
                e,
                load.map(|l| (l.item, l.count)),
                carry,
                Reach::all(map, (p.x, p.y)),
            )
        })
        .collect();
    idle.sort_unstable_by_key(|&(id, ..)| id);

    // Обходы от куч нужны только пустым котам — гружёный идёт сразу на площадку.
    // Остаток кучи считается за вычетом обещанного идущим (§12.49): расписанная
    // куча из списка уходит, и второго кота к пустому месту не зовут.
    let mut piles: Vec<Pile> = if idle.iter().any(|(_, _, load, ..)| load.is_none()) {
        stacks
            .iter()
            .filter_map(|(e, p, s)| {
                let left = s.count - claimed(&spoken_for, e);
                (left > 0).then(|| (e, (p.x, p.y), s.item, left, Reach::all(map, (p.x, p.y))))
            })
            .collect()
    } else {
        Vec::new()
    };
    piles.sort_unstable_by_key(|&(_, (x, y), item, ..)| (y, x, item));

    while !idle.is_empty() && !needy.is_empty() {
        // Взгляд на кучи внутри итерации: жадный выбор их только читает, а
        // правит остаток уже после того, как пара выбрана.
        let view: &[Pile] = &piles;
        // Куда идти первым шагом: гружёный — на площадку, пустой — к куче. Тип
        // связывает обоих: гружёный годится только той площадке, которой нужен
        // его груз, пустой идёт лишь к куче нужного типа (§12.21). У пустого
        // выбранная куча запоминается наводкой, поэтому в паре едет её индекс.
        let chosen = idle
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, _, loaded, _, reach))| {
                needy.iter().enumerate().filter_map(move |(ni, n)| {
                    let wanted = |item: usize| n.miss.iter().any(|&(i, _)| i == item);
                    if let Some((item, _)) = *loaded {
                        // Груз в лапах — неучтённое (§12.69): откуда кот его
                        // взял, уже не известно, а на складе он не лежит. К
                        // площадке с ним можно, к посту — нет. Случай редкий
                        // (ношу отобрали посреди ходки), но правило одно.
                        if !wanted(item) || n.dest.storage_only() {
                            return None;
                        }
                        let (spot, steps) = build_spot(map, reach, n.at, n.tile, None)?;
                        return Some((steps, ci, ni, spot, None));
                    }
                    view.iter()
                        .enumerate()
                        .filter(|(_, (_, _, item, ..))| wanted(*item))
                        .filter(|(_, (_, pile, ..))| !n.dest.storage_only() || in_store(*pile))
                        .filter_map(|(pi, (_, pile, _, _, from_pile))| {
                            let to_pile = reach.dist_at(pile.0, pile.1)?;
                            let (_, rest) = build_spot(map, from_pile, n.at, n.tile, None)?;
                            Some((to_pile + rest, ci, ni, *pile, Some(pi)))
                        })
                        .min_by_key(|&(steps, ..)| steps)
                })
            })
            .min_by_key(|&(steps, ..)| steps);
        // Ни одна пара не сошлась: нужного нет вовсе или до него не дойти. Коты
        // остаются свободными — `assign_jobs` найдёт им бесплатную работу.
        let Some((_, ci, ni, goal, pi)) = chosen else {
            break;
        };

        let (_, cat_e, loaded, carry, reach) = idle.remove(ci);
        let path = reach.path_to(goal.0, goal.1).unwrap_or_default();
        // Адресат остаётся в списке, пока ему есть что везти (§12.48): гружёный
        // забирает из недостачи ровно свой груз, идущий налегке — обещанную
        // ходку своего типа. Ушёл бюджет в ноль — снабжён, следующего кота
        // позовут другие. Учёт ведётся тут же, потому что команды применяются
        // после системы, и `going` только что назначенных ещё не видит.
        let n = &mut needy[ni];
        let (target_e, dest) = (n.target, n.dest);
        let (promise, aim) = match (loaded, pi) {
            (Some((item, count)), _) => {
                less_incoming(&mut n.miss, &[(item, count)]);
                (count, None)
            }
            (None, Some(pi)) => {
                let (pile_e, _, item, left, _) = piles[pi];
                let take = portion(carry, left).min(need_of(&n.miss, item));
                less_incoming(&mut n.miss, &[(item, take)]);
                piles[pi].3 -= take;
                (take, Some(Aim { pile: pile_e, item }))
            }
            // Пустой кот без кучи в паре недостижим: у пустого пара строится
            // только через кучу.
            (None, None) => (0, None),
        };
        n.budget -= promise;
        if n.budget <= 0 || n.miss.is_empty() {
            needy.remove(ni);
        }
        piles.retain(|&(_, _, _, left, _)| left > 0);
        let to = match dest {
            Dest::Sale => HaulTo::Sale(target_e),
            Dest::Site => HaulTo::Site(target_e),
            Dest::Shop => HaulTo::Shop(target_e),
        };
        commands
            .entity(cat_e)
            .insert((Haul { to, aim }, Path { steps: path }, MoveCooldown(0)));
    }
}

// --- уборка ----------------------------------------------------------------

/// Держит пометки уборки в согласии с картой.
///
/// При включённой автоуборке помечает всё, что валяется вне склада. Снимает
/// пометку с куч **на складе** в любом режиме: без этого доставленный лом
/// сразу становился бы задачей на самого себя и коты гоняли бы его по кругу.
pub(crate) fn mark_loose_scrap(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    auto: Res<AutoTidy>,
    mut commands: Commands,
    stacks: Query<(Entity, &Position, Option<&ToStore>), With<Stack>>,
) {
    for (e, pos, mark) in &stacks {
        let in_store = rules.capacity_of(map.tile_at(pos.x, pos.y)) > 0;
        match (in_store, mark.is_some()) {
            (true, true) => {
                commands.entity(e).remove::<ToStore>();
            }
            (false, false) if auto.0 => {
                commands.entity(e).insert(ToStore);
            }
            _ => {}
        }
    }
}

/// Раздаёт уборку: гружёные коты несут груз на склад, пустые идут за помеченными
/// кучами.
///
/// Стоит **после** `assign_jobs` в цепочке — уборка не отбирает котов у стройки
/// и сноса. Здесь, в отличие от `assign_hauls`, вторая нога маршрута (куча →
/// склад) не оптимизируется: склад обычно одна комната, а платить пришлось бы
/// обходом на каждую кучу каждый тик, пока на полу есть мусор.
///
/// **Кучу разбирают столько котов, на сколько её хватает** (§12.48). Куча в три
/// десятка — это восемь ходок, и делать их по очереди одним котом, пока рядом
/// стоят свободные, значит превращать базу в одного работника и зрителей; а
/// падает такая куча ровно там, где игрок её видит, — с вылазки и от каравана.
/// Сколько из кучи уже обещано идущим, считается по ним самим, как в
/// `assign_equip` (§12.34), и ограничено ещё и местом на складе: кот, которому
/// некуда сдать груз, встанет с ломом в лапах, а лапы игроку не видны (§12.16).
///
/// **Куча в ячейке торгового поста разбирается вперёд всех прочих** (§12.98):
/// расстояние решает только внутри очереди. Привезённое покупкой ложится кучей
/// прямо в ячейку (§12.68), а `free_post_cell` считает ячейку с кучей занятой, —
/// значит неубранный товар держит торговый слот, за который игрок платил
/// постройкой поста. У всех остальных куч цена ходки одинакова, и там §12.14
/// («берём ближайшего») цел целиком.
pub(crate) fn assign_tidy(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    marks: Query<(Entity, &Position), With<ToStore>>,
    going: Query<(&Haul, Option<&Carrying>, Option<&Carry>)>,
    stacks: Query<(Entity, &Position, &Stack)>,
    free_cats: Query<
        (
            Entity,
            &UnitId,
            &Position,
            Option<&Carrying>,
            Option<&Carry>,
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
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    if free_cats.is_empty() {
        return;
    }
    let map = &*map;
    let stock = stock_grid(map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
    // Склада нет или он забит — уборки не будет вовсе. Без этой проверки коты
    // ходили бы к кучам и возвращались ни с чем, тик за тиком.
    if !any_store_room(map, &rules, &stock) {
        return;
    }

    // Гружёные коты: каждому — ближайший склад со свободным местом. Драться за
    // клетку им незачем, ёмкость проверяется ещё раз при сдаче.
    let mut empty: Vec<(&str, Entity, Option<&Carry>, Reach)> = Vec::new();
    for (cat_e, id, pos, load, carry) in &free_cats {
        let reach = Reach::all(map, (pos.x, pos.y));
        if load.is_none() {
            empty.push((id.0.as_str(), cat_e, carry, reach));
            continue;
        }
        if let Some((cell, _)) = nearest_store(map, &rules, &reach, &stock) {
            let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
            commands.entity(cat_e).insert((
                Haul {
                    to: HaulTo::Store(None),
                    aim: None,
                },
                Path { steps: path },
                MoveCooldown(0),
            ));
        }
    }

    // Сколько из каждой кучи уже обещано тем, кто к ней идёт, и сколько места
    // осталось на складе. Место — общий бюджет на всю раздачу: кот, которому
    // некуда сдать, встал бы с грузом в лапах, а лапы игроку не видны (§12.16).
    let mut promised: Vec<(Entity, i32)> = Vec::new();
    let mut room: i32 = (0..stock.len())
        .map(|i| (rules.capacity_of(map.cells[i]) - stock[i]).max(0))
        .sum();
    for (haul, load, carry) in &going {
        // Груз в лапах места на складе уже ждёт, но в `stock` его нет — он не
        // лежит на полу. Из кучи он при этом **уже взят**, поэтому обещанием не
        // считается: обещает только тот, кто за ней ещё идёт.
        if let Some(load) = load {
            if matches!(haul.to, HaulTo::Store(_)) {
                room -= load.count;
            }
            continue;
        }
        // Кучи у уборки и подвоза общие, поэтому считаются обе раздачи разом
        // (§12.49): носильщик подвоза, не попавший в счёт, уносит лом из-под
        // приехавшего за ним уборщика — и наоборот.
        let pile_e = match (haul.to, haul.aim) {
            (HaulTo::Store(Some(pile)), _) => pile,
            (_, Some(aim)) => aim.pile,
            _ => continue,
        };
        let trip = portion(carry, pile_count(&stacks, pile_e));
        match promised.iter_mut().find(|(e, _)| *e == pile_e) {
            Some((_, n)) => *n += trip,
            None => promised.push((pile_e, trip)),
        }
    }

    // Пустые коты: жадно разбираем пары (кот, помеченная куча) от ближней.
    // Куча остаётся в списке, пока в ней есть необещанное, — тогда её разбирают
    // несколько котов разом (§12.48). Четвёртое поле — очередь (§12.98): куча в
    // ячейке торгового поста разбирается вперёд всех прочих.
    let mut open: Vec<(Entity, (i32, i32), i32, u8)> = marks
        .iter()
        .filter_map(|(e, p)| {
            let count = stacks.get(e).map_or(0, |(_, _, s)| s.count);
            let left = count - claimed(&promised, e);
            let rank = u8::from(!rules.is_trade_post(map.tile_at(p.x, p.y)));
            (left > 0).then_some((e, (p.x, p.y), left, rank))
        })
        .collect();
    // Порядок обхода ECS в поведение протекать не должен (§11): при равном
    // расстоянии жадный выбор берёт первую пару. Коты — по `id`, кучи — по
    // клетке; помеченных куч разных типов на одной клетке бывает несколько
    // (§12.21), поэтому в ключе ещё и сущность.
    empty.sort_unstable_by_key(|&(id, ..)| id);
    open.sort_unstable_by_key(|&(e, (x, y), ..)| (y, x, e.index()));

    while !empty.is_empty() && !open.is_empty() && room > 0 {
        let chosen = empty
            .iter()
            .enumerate()
            .flat_map(|(ci, (_, _, _, reach))| {
                open.iter()
                    .enumerate()
                    .filter_map(move |(oi, &(_, xy, _, rank))| {
                        reach.dist_at(xy.0, xy.1).map(|d| (rank, d, ci, oi, xy))
                    })
            })
            .min_by_key(|&(rank, steps, ..)| (rank, steps));
        let Some((_, _, ci, oi, goal)) = chosen else {
            break;
        };

        let (_, cat_e, carry, reach) = empty.remove(ci);
        let (pile_e, _, left, _) = open[oi];
        let trip = portion(carry, left.min(room));
        open[oi].2 -= trip;
        room -= trip;
        if open[oi].2 <= 0 {
            open.remove(oi);
        }
        let path = reach.path_to(goal.0, goal.1).unwrap_or_default();
        commands.entity(cat_e).insert((
            Haul {
                to: HaulTo::Store(Some(pile_e)),
                aim: None,
            },
            Path { steps: path },
            MoveCooldown(0),
        ));
    }
}

/// Сколько из кучи уже обещано идущим к ней.
fn claimed(promised: &[(Entity, i32)], pile: Entity) -> i32 {
    promised
        .iter()
        .find(|&&(e, _)| e == pile)
        .map_or(0, |&(_, n)| n)
}

// --- работа носильщика -----------------------------------------------------

/// Носильщики, добравшиеся до цели: набирают лом на куче и сдают его адресату.
///
/// Отпустить задачу (снять `Haul`, обнулить claim) — штатный исход, а не
/// ошибка: кучу могли разобрать раньше, склад — заполниться, маршрут —
/// оборваться. Раздатчик на следующем тике попробует снова, груз при этом
/// остаётся на коте.
#[allow(clippy::too_many_arguments)]
pub(crate) fn work_hauls(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    mut commands: Commands,
    cats: Query<(
        Entity,
        &Position,
        &Haul,
        Option<&Carrying>,
        Option<&Path>,
        Option<&Carry>,
    )>,
    mut blueprints: Query<&mut Blueprint>,
    mut deals: Query<&mut Deal>,
    mut crafts: Query<&mut Craft>,
    recipes: Res<CraftRules>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    // Что уже везут к каждому адресату. Носильщиков у него теперь несколько
    // (§12.48), и берущий с кучи обязан вычесть чужой груз: иначе второй кот
    // поднимет то же самое, а на площадке этого уже никто не ждёт — и груз
    // осядет в лапах, где игроку его не видно и не разметить (§12.16).
    let incoming: Vec<(Entity, usize, i32)> = cats
        .iter()
        .filter_map(|(_, _, haul, load, ..)| {
            let target = match haul.to {
                HaulTo::Site(e) | HaulTo::Sale(e) | HaulTo::Shop(e) => e,
                HaulTo::Store(_) => return None,
            };
            load.map(|l| (target, l.item, l.count))
        })
        .collect();
    let brought = |target: Entity| -> Vec<(usize, i32)> {
        incoming
            .iter()
            .filter(|&&(e, ..)| e == target)
            .map(|&(_, item, count)| (item, count))
            .collect()
    };

    for (cat_e, pos, haul, load, path, carry) in &cats {
        match haul.to {
            HaulTo::Site(bp_e) => {
                // Чертёж отменили, пока кот был в пути.
                let Ok(mut bp) = blueprints.get_mut(bp_e) else {
                    commands.entity(cat_e).remove::<Haul>();
                    continue;
                };
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let miss = missing(&rules, &bp);

                let Some(load) = load else {
                    // Пришёл к куче: берём тот тип, которого площадке не хватает,
                    // и ровно столько, сколько не хватает и влезает в лапы. Какая
                    // именно куча под ногами — решается здесь, а не при раздаче.
                    // Чужой груз в пути вычитается: везут его уже без нас.
                    let mut left = miss.clone();
                    less_incoming(&mut left, &brought(bp_e));
                    // И забронированное под продажу — тоже не наше (§12.50):
                    // с момента заявки товаром распоряжается сделка, а базе
                    // остаётся свободный остаток.
                    for slot in left.iter_mut() {
                        let free = crate::trade::free_to_spend(
                            stacks.iter().map(|(_, _, s)| s),
                            deals.iter(),
                            cats.iter().filter_map(|(_, _, h, l, ..)| l.map(|l| (h, l))),
                            slot.0,
                        );
                        slot.1 = slot.1.min(free.max(0));
                    }
                    left.retain(|&(_, n)| n > 0);
                    let taken =
                        take_needed(&mut commands, &mut stacks, (pos.x, pos.y), &left, carry);
                    let Some((item, taken)) = taken else {
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    let reach = Reach::all(&map, (pos.x, pos.y));
                    match build_spot(&map, &reach, (bp.x, bp.y), bp.tile, None) {
                        Some((spot, _)) => {
                            let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
                            // Наводка отработала: дальше кота считают по грузу,
                            // и оставленная она посчиталась бы вторым разом.
                            commands.entity(cat_e).insert((
                                Haul {
                                    to: haul.to,
                                    aim: None,
                                },
                                Carrying { item, count: taken },
                                Path { steps: path },
                                MoveCooldown(0),
                            ));
                        }
                        // Площадка стала недостижима, пока кот шёл за грузом:
                        // груз при нём, задача отпущена.
                        None => {
                            commands
                                .entity(cat_e)
                                .insert(Carrying { item, count: taken })
                                .remove::<Haul>();
                        }
                    }
                    continue;
                };

                // Пришёл на площадку — сдаём груз (излишек уносит с собой).
                if (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1 {
                    let need = miss
                        .iter()
                        .find(|&&(i, _)| i == load.item)
                        .map_or(0, |&(_, n)| n);
                    let given = load.count.min(need);
                    add_delivered(&mut bp.delivered, load.item, given);
                    keep_rest(&mut commands, cat_e, load.item, load.count - given);
                }
                commands.entity(cat_e).remove::<Haul>();
            }
            // Мастерская — зеркало площадки, и отличие ровно одно: недостачу
            // считает `craft_missing` (цена рецепта на все оставшиеся штуки), а
            // не цена тайла (§12.102). Даже адрес тот же: `build_spot` ведёт к
            // соседней клетке, потому что на станке стоять незачем.
            HaulTo::Shop(order_e) => {
                // Заказ отменили или станок снесли, пока кот был в пути.
                let Ok(mut order) = crafts.get_mut(order_e) else {
                    commands.entity(cat_e).remove::<Haul>();
                    continue;
                };
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let miss = craft_missing(&recipes, &order);
                let cell = order.cell;

                let Some(load) = load else {
                    // Пришёл к куче — берём то, чего станку не хватает. Чужой
                    // груз в пути вычитается (§12.48), забронированное под
                    // продажу — тоже не наше (§12.50).
                    let mut left = miss.clone();
                    less_incoming(&mut left, &brought(order_e));
                    for slot in left.iter_mut() {
                        let free = crate::trade::free_to_spend(
                            stacks.iter().map(|(_, _, s)| s),
                            deals.iter(),
                            cats.iter().filter_map(|(_, _, h, l, ..)| l.map(|l| (h, l))),
                            slot.0,
                        );
                        slot.1 = slot.1.min(free.max(0));
                    }
                    left.retain(|&(_, n)| n > 0);
                    let taken =
                        take_needed(&mut commands, &mut stacks, (pos.x, pos.y), &left, carry);
                    let Some((item, taken)) = taken else {
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    let reach = Reach::all(&map, (pos.x, pos.y));
                    let tile = map.tile_at(cell.0, cell.1);
                    match build_spot(&map, &reach, cell, tile, None) {
                        Some((spot, _)) => {
                            let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
                            commands.entity(cat_e).insert((
                                Haul {
                                    to: haul.to,
                                    aim: None,
                                },
                                Carrying { item, count: taken },
                                Path { steps: path },
                                MoveCooldown(0),
                            ));
                        }
                        None => {
                            commands
                                .entity(cat_e)
                                .insert(Carrying { item, count: taken })
                                .remove::<Haul>();
                        }
                    }
                    continue;
                };

                // Пришёл к станку — сдаём груз (излишек уносит с собой).
                if (pos.x - cell.0).abs() + (pos.y - cell.1).abs() <= 1 {
                    let need = miss
                        .iter()
                        .find(|&&(i, _)| i == load.item)
                        .map_or(0, |&(_, n)| n);
                    let given = load.count.min(need);
                    add_delivered(&mut order.delivered, load.item, given);
                    keep_rest(&mut commands, cat_e, load.item, load.count - given);
                }
                commands.entity(cat_e).remove::<Haul>();
            }
            // Продажа — зеркало площадки, и отличий ровно два: на месте товар
            // не укладывается в тайл, а исчезает из мира, и вместо `delivered`
            // растут деньги (§12.44).
            HaulTo::Sale(deal_e) => {
                let Ok(mut deal) = deals.get_mut(deal_e) else {
                    commands.entity(cat_e).remove::<Haul>();
                    continue;
                };
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let left = deal.count - deal.delivered;

                let Some(load) = load else {
                    let mut miss = vec![(deal.item, left)];
                    less_incoming(&mut miss, &brought(deal_e));
                    let taken =
                        take_needed(&mut commands, &mut stacks, (pos.x, pos.y), &miss, carry);
                    let Some((item, taken)) = taken else {
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };
                    let reach = Reach::all(&map, (pos.x, pos.y));
                    let tile = map.tile_at(deal.cell.0, deal.cell.1);
                    match build_spot(&map, &reach, deal.cell, tile, None) {
                        Some((spot, _)) => {
                            let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
                            commands.entity(cat_e).insert((
                                Haul {
                                    to: haul.to,
                                    aim: None,
                                },
                                Carrying { item, count: taken },
                                Path { steps: path },
                                MoveCooldown(0),
                            ));
                        }
                        None => {
                            commands
                                .entity(cat_e)
                                .insert(Carrying { item, count: taken })
                                .remove::<Haul>();
                        }
                    }
                    continue;
                };

                // Донёс до ячейки — товар ложится в контейнер (§12.68). **Он не
                // становится кучей ни на тик**: ячейка ёмкости не имеет, и
                // `mark_loose_scrap` тут же повёз бы проданное обратно на склад.
                // Содержимое живёт счётчиком на самой сделке.
                //
                // Денег здесь нет: за отгруженный контейнер платит `run_trade`
                // разом, там же, где списывается покупка (§12.68). Второго
                // места, где считаются деньги, быть не должно.
                if (pos.x - deal.cell.0).abs() + (pos.y - deal.cell.1).abs() <= 1 {
                    let given = load.count.min(left).max(0);
                    if load.item == deal.item {
                        deal.delivered += given;
                        keep_rest(&mut commands, cat_e, load.item, load.count - given);
                    }
                }
                // Сделка здесь **не закрывается**: набитый контейнер ещё должен
                // уехать, и срок ему отсчитывает `run_trade`.
                commands.entity(cat_e).remove::<Haul>();
            }

            HaulTo::Store(pile_e) => {
                if path.is_some() {
                    continue; // ещё в дороге
                }
                let Some(load) = load else {
                    // Куда нести, решается **до** подъёма: клетка склада могла
                    // заполниться, пока кот шёл. Складов нет или все полны —
                    // куча остаётся лежать на виду, а не переезжает в лапы,
                    // откуда игрок её уже не достанет.
                    let reach = Reach::all(&map, (pos.x, pos.y));
                    let stock =
                        stock_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                    let Some((cell, _)) = nearest_store(&map, &rules, &reach, &stock) else {
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };

                    // Берём ровно столько, сколько влезет в адресат и в лапы:
                    // остаток честнее оставить на полу, чем таскать по базе.
                    // Берём именно ту кучу, за которой шли: на клетке могут
                    // лежать разные типы, а помечена была одна (§12.21).
                    let free = portion(carry, free_space(&map, &rules, &stock, cell));
                    let taken =
                        take_from_pile(&mut commands, &mut stacks, pile_e, (pos.x, pos.y), free);
                    let Some((item, taken)) = taken else {
                        commands.entity(cat_e).remove::<Haul>();
                        continue;
                    };
                    let path = reach.path_to(cell.0, cell.1).unwrap_or_default();
                    commands.entity(cat_e).insert((
                        Carrying { item, count: taken },
                        Path { steps: path },
                        MoveCooldown(0),
                    ));
                    continue;
                };

                // Пришёл на склад — сдаём, сколько влезло.
                let stock = stock_grid(&map, stacks.iter().map(|(_, p, s)| ((p.x, p.y), s.count)));
                let free = free_space(&map, &rules, &stock, (pos.x, pos.y));
                if free > 0 {
                    let given = load.count.min(free);
                    spill(&mut commands, &mut stacks, (pos.x, pos.y), load.item, given);
                    keep_rest(&mut commands, cat_e, load.item, load.count - given);
                }
                commands.entity(cat_e).remove::<Haul>();
            }
        }
    }
}

/// Сколько кот унесёт за эту ходку: `Carry` — предел лап, его отсутствие (и
/// ноль) читается как «без предела», по правилу нулей у тайлов (§12.17).
fn portion(carry: Option<&Carry>, want: i32) -> i32 {
    match carry {
        Some(&Carry(cap)) if cap > 0 => want.min(cap),
        _ => want,
    }
}

/// Оставить коту остаток груза; пусто — снять `Carrying`.
fn keep_rest(commands: &mut Commands, cat: Entity, item: usize, left: i32) {
    if left > 0 {
        commands.entity(cat).insert(Carrying { item, count: left });
    } else {
        commands.entity(cat).remove::<Carrying>();
    }
}

/// Взять из конкретной кучи, если она ещё есть и лежит под котом. Кучи может
/// уже не быть — её разобрали раньше; это штатный конец задачи, а не ошибка.
fn take_from_pile(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    pile: Option<Entity>,
    at: (i32, i32),
    want: i32,
) -> Option<(usize, i32)> {
    let (stack_e, pos, mut stack) = pile.and_then(|e| stacks.get_mut(e).ok())?;
    if (pos.x, pos.y) != at {
        return None;
    }
    let taken = want.min(stack.count);
    if taken <= 0 {
        return None;
    }
    stack.count -= taken;
    let item = stack.item;
    if stack.count <= 0 {
        commands.entity(stack_e).despawn();
    }
    Some((item, taken))
}

/// Взять из-под ног тот тип, которого не хватает площадке, — сколько не хватает
/// и сколько влезает в лапы. Перебор идёт по недостаче, а не по кучам: порядок
/// цены детерминирован (§12.21), порядок сущностей — нет.
fn take_needed(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    at: (i32, i32),
    miss: &[(usize, i32)],
    carry: Option<&Carry>,
) -> Option<(usize, i32)> {
    for &(item, need) in miss {
        let found = stacks
            .iter()
            .find(|(_, p, s)| (p.x, p.y) == at && s.item == item && s.count > 0)
            .map(|(e, ..)| e);
        if let Some(taken) = take_from_pile(commands, stacks, found, at, portion(carry, need)) {
            return Some(taken);
        }
    }
    None
}

// --- склад и кучи ----------------------------------------------------------

/// Кучи, лежащие **на складе**, в порядке обхода карты.
///
/// Порядок задан явно, а не порядком сущностей: обход ECS зависит от истории
/// вставок, а любой недетерминизм ломает и тесты, и модель времени (§11).
/// Лежащее на полу сюда не попадает — платит склад, а не то, что валяется
/// (§12.24).
pub(crate) fn storage_order<I>(
    map: &BaseMap,
    rules: &TileRules,
    piles: I,
) -> Vec<(Entity, usize, i32)>
where
    I: IntoIterator<Item = (Entity, (i32, i32), usize, i32)>,
{
    let mut shelved: Vec<(i32, i32, Entity, usize, i32)> = piles
        .into_iter()
        .filter(|&(_, (x, y), _, count)| count > 0 && rules.capacity_of(map.tile_at(x, y)) > 0)
        .map(|(e, (x, y), item, count)| (y, x, e, item, count))
        .collect();
    shelved.sort_unstable_by_key(|&(y, x, ..)| (y, x));
    shelved
        .into_iter()
        .map(|(_, _, e, i, n)| (e, i, n))
        .collect()
}

/// Сколько снять с каждой кучи, чтобы покрыть набор; `None` — не покрывается.
///
/// **Либо весь набор, либо ничего**: половинчатая оплата оставила бы игрока и
/// без предметов, и без покупки. Правило живёт здесь одно на всех, кто платит
/// складом, — найм и науку (фасад) и производство (система): две арифметики
/// списания однажды разойдутся на порядке обхода куч (§12.30).
pub(crate) fn plan_spend(
    piles: &[(Entity, usize, i32)],
    cost: &[(usize, i32)],
    reserved: &[(usize, i32)],
) -> Option<Vec<(Entity, i32)>> {
    let mut takes: Vec<(Entity, i32)> = Vec::new();
    for &(item, need) in cost {
        // Забронированное под продажу базе уже не принадлежит (§12.50):
        // проверяем это до раскладки по кучам, потому что правило про сумму,
        // а не про конкретную кучу.
        let booked = reserved
            .iter()
            .find(|&&(i, _)| i == item)
            .map_or(0, |&(_, n)| n);
        let shelved: i32 = piles
            .iter()
            .filter(|&&(_, pile_item, _)| pile_item == item)
            .map(|&(_, _, count)| count)
            .sum();
        if shelved - booked < need {
            return None;
        }
        let mut left = need;
        for &(pile_e, pile_item, count) in piles {
            if left <= 0 {
                break;
            }
            if pile_item != item {
                continue;
            }
            let taken = left.min(count);
            left -= taken;
            takes.push((pile_e, taken));
        }
        if left > 0 {
            return None; // на складе не хватает — не снимаем ничего
        }
    }
    Some(takes)
}

/// Сколько предметов каждого типа лежит **на складе** — на клетках с ёмкостью.
///
/// Склад — это то, чем платят (§12.24), и потому единственная величина, которую
/// нельзя считать дважды: разойдись она с `plan_spend`, и панель показывала бы
/// живую кнопку, которую фасад отклоняет молча (§12.53). Отсюда свободная
/// функция вместо метода: её зовут и фасад (`Sim::stock`, у которого на руках
/// `&mut World`), и система целей (у которой `Query`).
///
/// Пол и лапы сюда **не входят** намеренно: они у базы есть, но заплатить ими
/// нельзя, пока их не убрали.
/// Длина результата **не задаётся снаружи намеренно**: вектор растёт под
/// встреченные типы. Параметр «сколько типов в палитре» был бы вторым источником
/// правды, и звавшие эту функцию фасад и система однажды передали бы разное —
/// один длину `ItemRules`, другой длину своей копии палитры. Тогда цель
/// закрывалась бы по одному счёту, а панель показывала бы другой; так и вышло
/// при первой сборке. Читать результат — только через `.get()`.
pub(crate) fn stored_counts<'a>(
    piles: impl Iterator<Item = (&'a Position, &'a Stack)>,
    map: &BaseMap,
    rules: &TileRules,
) -> Vec<i32> {
    let mut stored: Vec<i32> = Vec::new();
    for (p, s) in piles {
        if rules.capacity_of(map.tile_at(p.x, p.y)) > 0 {
            if stored.len() <= s.item {
                stored.resize(s.item + 1, 0);
            }
            stored[s.item] += s.count;
        }
    }
    stored
}

/// Сколько предметов каждого типа есть у базы **вообще**: кучи где угодно плюс
/// груз в лапах.
///
/// Это другой вопрос, чем `stored_counts`, и путать их нельзя. Тот отвечает
/// «чем база может заплатить» (склад, §12.24), этот — «сколько у неё этого
/// добра». Второй нужен там, где важно наличие, а не платёжеспособность:
/// ворота продажи (§12.44 — везут обычным подвозом, и куча на полу годится) и
/// порог автопроизводства (§12.65 — готовое лежит под ногами мастера, и склад
/// узнаёт о нём только после уборки).
///
/// Ушедших с базы это не касается: заявка на вылазку роняет ношу под ноги
/// (§12.38), так что чужого добра за шлюзом не бывает. Обещанное покупателю
/// **не вычитается здесь** — бронь считает `trade::booked`, и складывать два
/// правила в одну функцию значит потерять возможность спросить их порознь.
///
/// Длина результата, как и у `stored_counts`, растёт под встреченные типы:
/// читать только через `.get()`.
pub(crate) fn on_base_counts<'a>(
    piles: impl Iterator<Item = &'a Stack>,
    loads: impl Iterator<Item = &'a Carrying>,
) -> Vec<i32> {
    let mut have: Vec<i32> = Vec::new();
    let mut add = |item: usize, count: i32| {
        if have.len() <= item {
            have.resize(item + 1, 0);
        }
        have[item] += count;
    };
    for s in piles {
        add(s.item, s.count);
    }
    for load in loads {
        add(load.item, load.count);
    }
    have
}

/// Сколько всего добра лежит на каждой клетке — сеткой, чтобы не искать
/// линейно. Тип здесь не важен: склад типо-агностичен, ёмкость считает штуки
/// (§12.21).
fn stock_grid(map: &BaseMap, piles: impl Iterator<Item = ((i32, i32), i32)>) -> Vec<i32> {
    let mut grid = vec![0; (map.width * map.height) as usize];
    for ((x, y), count) in piles {
        if let Some(i) = map.index(x, y) {
            grid[i] += count;
        }
    }
    grid
}

/// Сколько ещё влезет на клетку. Склад — это тайл с ёмкостью, отдельного
/// слоя зон нет (§12.16); у обычного пола ёмкость нулевая.
fn free_space(map: &BaseMap, rules: &TileRules, stock: &[i32], at: (i32, i32)) -> i32 {
    map.index(at.0, at.1)
        .map_or(0, |i| rules.capacity_of(map.cells[i]) - stock[i])
}

/// Ближайшая клетка склада, куда влезет хоть сколько-то лома, и путь до неё
/// в шагах.
fn nearest_store(
    map: &BaseMap,
    rules: &TileRules,
    reach: &Reach,
    stock: &[i32],
) -> Option<((i32, i32), i32)> {
    let mut best: Option<((i32, i32), i32)> = None;
    for y in 0..map.height {
        for x in 0..map.width {
            if free_space(map, rules, stock, (x, y)) <= 0 {
                continue;
            }
            let Some(d) = reach.dist_at(x, y) else {
                continue;
            };
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some(((x, y), d));
            }
        }
    }
    best
}

/// Есть ли на базе хоть одно место под груз. Дешёвая проверка без обхода: без
/// неё коты ходили бы к кучам и возвращались ни с чем, пока склада нет.
fn any_store_room(map: &BaseMap, rules: &TileRules, stock: &[i32]) -> bool {
    (0..stock.len()).any(|i| rules.capacity_of(map.cells[i]) - stock[i] > 0)
}

/// Приводит кучи в порядок: снимает их с пустоты и сливает те, что оказались
/// на одной клетке.
///
/// **Пустота.** Кот сносит клетку, стоя на шаг ближе к берегу, возврат ложится
/// под него — и следующая волна убирает эту клетку вместе с кучей. Сдвиг на
/// соседний пол — тот же ответ, что и у кота в яме (§12.10), и по той же
/// причине: пустоту не пересечь, значит куча в ней недостижима. Разница в том,
/// что кот виден флагом `stuck`, а «лом провалился в дыру» читался бы как
/// молчаливая пропажа материала. Шаг ровно один: куча возникает только под
/// котом, а он стоит на полу. Соседа нет — куча остаётся в яме, обратимо.
///
/// **Двойники.** `spill` не видит куч, заспавненных в этом же тике через
/// `Commands`, поэтому два кота, сдавшие одинаковый груз на одну клетку, создают
/// две кучи — сливаем; живут двойники не дольше тика. Кучи **разных** типов на
/// одной клетке двойниками не считаются и остаются лежать рядом (§12.21).
pub(crate) fn settle_stacks(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut stacks: Query<(Entity, &mut Position, &mut Stack, Option<&ToStore>)>,
) {
    // Первая куча своего типа на клетке остаётся, остальные вливаются в неё.
    let mut keepers: Vec<(((i32, i32), usize), Entity, bool)> = Vec::new();
    let mut moves: Vec<(Entity, (i32, i32))> = Vec::new();
    let mut merges: Vec<(Entity, Entity, i32, bool)> = Vec::new();

    for (e, pos, stack, mark) in stacks.iter() {
        let Some(home) = home_of(&map, (pos.x, pos.y)) else {
            continue; // замуровано вместе с ямой — обратимо
        };
        let home = (home, stack.item);
        match keepers.iter().find(|(cell, ..)| *cell == home) {
            // Пометка переживает слияние: иначе жест игрока молча пропадал бы.
            Some(&(_, keeper, kept_mark)) => {
                merges.push((e, keeper, stack.count, mark.is_some() && !kept_mark))
            }
            None => {
                keepers.push((home, e, mark.is_some()));
                if (pos.x, pos.y) != home.0 {
                    moves.push((e, home.0));
                }
            }
        }
    }

    for (e, to) in moves {
        if let Ok((_, mut pos, ..)) = stacks.get_mut(e) {
            pos.x = to.0;
            pos.y = to.1;
        }
    }
    for (e, keeper, count, pass_mark) in merges {
        if let Ok((_, _, mut stack, _)) = stacks.get_mut(keeper) {
            stack.count += count;
        }
        if pass_mark {
            commands.entity(keeper).insert(ToStore);
        }
        commands.entity(e).despawn();
    }
}

/// Клетка, на которой куче место: своя, если проходима, иначе первый проходимый
/// сосед. Порядок `DIRS` фиксирован, значит выбор детерминирован (§11).
fn home_of(map: &BaseMap, at: (i32, i32)) -> Option<(i32, i32)> {
    if map.walkable(at.0, at.1) {
        return Some(at);
    }
    DIRS.iter()
        .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
        .find(|&(nx, ny)| map.walkable(nx, ny))
}

/// Положить лом на клетку, слив с уже лежащей там кучей.
pub(crate) fn spill(
    commands: &mut Commands,
    stacks: &mut Query<(Entity, &Position, &mut Stack)>,
    at: (i32, i32),
    item: usize,
    count: i32,
) {
    if let Some((_, _, mut stack)) = stacks
        .iter_mut()
        .find(|(_, p, s)| (p.x, p.y) == at && s.item == item)
    {
        stack.count += count;
        return;
    }
    commands.spawn((Position { x: at.0, y: at.1 }, Stack { item, count }));
}
