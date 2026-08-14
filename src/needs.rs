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
//!
//! Вторая потребность — голод — живёт в `food` (§12.36): у неё своя шкала, свой
//! порог и свой удовлетворитель. Общего у них ровно одно место, и оно здесь:
//! `tire` жжёт бодрость вдвое на пустой желудок — это и есть вся цена голода.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::BaseMap;
use crate::path::Reach;

/// Бодрости тратится за тик бодрствования, если рулсет не сказал иного.
///
/// Единица — это шкала без вариативности: ниже неё не спуститься (дроби ломают
/// детерминизм, §11), поэтому «Выносливость» умеет двигать расход только на
/// укрупнённой шкале, где база — десятки. Синтетические схемы живут на единице,
/// и выносливость там никого не различает (§12.70).
const TIRE_PER_TICK: i32 = 1;

/// Параметр, которым кот держится дольше прочих (§12.70). Имя, а не номер:
/// порядок записей `stats:` — дело рулсета.
pub(crate) const STAT_GRIT: &str = "stamina";

/// Сколько бодрости кот тратит за тик бодрствования (§12.70).
///
/// Ступень «Выносливости» вычитается из базового расхода — не прибавляется к
/// потолку. Разница смысловая: выносливый **дольше держится**, а не дольше
/// отсыпается, и пороги при этом остаются абсолютными, а сутки — одной формулой
/// на всех. Потолок от параметра потребовал бы делать `tired` и `critical`
/// долями `max`, и у каждого кота были бы свои сутки.
///
/// Ниже единицы расход не опускается никогда: кот, который не устаёт вовсе,
/// не уснёт, а бодрость — это плата за котовремя, а не необязательный налог.
fn tire_rate(needs: &NeedRules, stats: &StatRules, cat: Option<&Stats>) -> i32 {
    let base = if needs.drain > 0 {
        needs.drain
    } else {
        TIRE_PER_TICK
    };
    let step = stats
        .index_of(STAT_GRIT)
        .map_or(0, |stat| stats.step(stat, cat));
    (base - step).max(1)
}

/// Кот на нуле бодрости засыпает где стоит, отпустив задачу.
///
/// Чертёж, тему и заказ надо освободить явно: иначе работа останется «занятой»
/// спящим и её больше никто не возьмёт. Подвоз освобождать нечего — носильщика
/// у адресата больше не записано (§12.48).
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
            Option<&Treating>,
        ),
        (With<UnitId>, Without<Rest>, Without<Away>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut topics: Query<&mut Research>,
    mut orders: Query<&mut Craft>,
) {
    for (cat_e, energy, assignment, haul, researching, crafting, treating) in &cats {
        if energy.0 > 0 {
            continue;
        }
        release_work(
            &mut commands,
            cat_e,
            (assignment, haul, researching, crafting, treating),
            &mut blueprints,
            &mut topics,
            &mut orders,
        );
        // Лежанку упавший не занимает — падает где стоит, а не идёт к месту.
        commands.entity(cat_e).insert(Rest { spot: None });
    }
}

/// Отпустить всё, за что кот держался: чертёж, тему, заказ и парту.
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
pub(crate) fn release_work(
    commands: &mut Commands,
    cat_e: Entity,
    work: (
        Option<&Assignment>,
        Option<&Haul>,
        Option<&Researching>,
        Option<&Crafting>,
        Option<&Treating>,
    ),
    blueprints: &mut Query<&mut Blueprint>,
    topics: &mut Query<&mut Research>,
    orders: &mut Query<&mut Craft>,
) {
    // Подвоз (`_haul`) освобождать нечего: сколько котов везут к адресату, видно
    // по ним самим, и снятого ниже `Haul` достаточно — claim'а, который надо
    // гасить, у площадки, сделки и кучи больше нет (§12.48). Груз при этом
    // остаётся в лапах: донесёт следующей ходкой (§12.15).
    let (assignment, _haul, researching, crafting, _treating) = work;
    if let Some(bp_e) = assignment.map(|a| a.0) {
        if let Ok(mut bp) = blueprints.get_mut(bp_e) {
            bp.assignee = None;
        }
    }
    if let Some(mut topic) = researching.and_then(|r| topics.get_mut(r.0).ok()) {
        topic.assignee = None;
        topic.spot = None;
    }
    if let Some(mut order) = crafting.and_then(|c| orders.get_mut(c.0).ok()) {
        order.assignee = None;
        order.spot = None;
    }
    // Пациента освобождать не нужно: `Healing::medic` — claim, который чинит
    // сам `assign_treat`, заметив, что `Treating` у медика больше нет (§12.37).
    // Одно место починки вместо освобождения в каждом, кто снимает работу.
    // `OnDuty` снимается вместе со всеми: голод и усталость уводят дежурного от
    // рации, как увели бы от чертежа. Связь на этом просто перестаёт копиться —
    // накопленное не отнимается (§12.60). Приписку (`Posted`) не трогаем: она
    // конфигурация, а не задача, и раздатчик вернёт кота на узел сам — иначе
    // повторилась бы дыра парты, где поевший ученик не возвращается никогда.
    commands.entity(cat_e).remove::<(
        Assignment,
        Haul,
        Study,
        Researching,
        Crafting,
        Equipping,
        Eating,
        Treating,
        OnDuty,
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
            Option<&Treating>,
        ),
        (
            With<UnitId>,
            Without<Rest>,
            Without<Away>,
            // Раненого сюда не пускаем: он уже лежит, и увести его на лежанку
            // значит отменить лечение ради сна (§12.37).
            Without<Healing>,
            Or<(
                With<Assignment>,
                With<Haul>,
                With<Study>,
                With<Researching>,
                With<Crafting>,
                With<Equipping>,
                With<Eating>,
                With<Treating>,
                With<Squad>,
                With<Path>,
            )>,
        ),
    >,
    fallen: Query<
        (Entity, &Position, &Energy, &Rest),
        (With<UnitId>, Without<Path>, Without<Away>, Without<Healing>),
    >,
    mut blueprints: Query<&mut Blueprint>,
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
        for (cat_e, pos, energy, assignment, haul, researching, crafting, treating) in &busy_cats {
            if energy.0 > needs.critical || free.is_empty() {
                continue;
            }
            let Some(bed) = claim_bed(&map, &mut free, (pos.x, pos.y)) else {
                continue; // до свободной лежанки не добраться — работает дальше
            };
            release_work(
                &mut commands,
                cat_e,
                (assignment, haul, researching, crafting, treating),
                &mut blueprints,
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

/// Отправляет кота, которому нечем заняться, на свободную лежанку — дремать
/// (§12.52).
///
/// Стоит **последним** из раздатчиков, и это не вкусовщина: «коту ничего не
/// досталось» становится фактом только после того, как своё разобрали все
/// остальные. Раздать дремоту раньше значило бы уводить кота от работы, которую
/// ему через два вызова и предложили бы.
///
/// Задачи он тут не получает — только маршрут. Дремота это отсутствие задачи
/// плюс место для сна под лапами, и заведись у неё компонент, пришлось бы
/// вписывать его в одиннадцать фильтров занятости ради состояния, из которого
/// кота обязан выдёргивать кто угодно.
///
/// **Занятой считается и клетка под котом, и та, куда он идёт.** Иначе двое
/// свободных выберут одну лежанку, придут на неё вдвоём, и `spread_units`
/// отправит второго обратно в поиск — качели на ровном месте (§12.39).
/// Спящий держит свою `Rest::spot` по той же причине, что и всегда (§12.20).
pub(crate) fn assign_nap(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut commands: Commands,
    everyone: Query<(&Position, Option<&Path>, Option<&Rest>), (With<UnitId>, Without<Away>)>,
    idle: Query<
        (Entity, &UnitId, &Position, &Energy),
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
            Without<Away>,
            Without<Path>,
            // Приказ игрока — тоже дело, даже когда он невыполним: увести кота
            // с того места, куда его послали, значит объяснить игроку, что его
            // решение отменила усталость, которой ещё нет (§12.15).
            Without<Order>,
        ),
    >,
) {
    if needs.max <= 0 || idle.is_empty() {
        return;
    }
    let mut taken: Vec<(i32, i32)> = Vec::new();
    // Клетки, на которых кот **остановиться** не может: под остановившимся
    // соседом (§12.32). Идущий сюда не попадает — сквозь него проходят.
    let mut standing: Vec<(i32, i32)> = Vec::new();
    for (pos, path, rest) in &everyone {
        taken.push((pos.x, pos.y));
        taken.extend(path.and_then(|p| p.steps.first()).copied());
        taken.extend(rest.and_then(|r| r.spot));
        if path.is_none() {
            standing.push((pos.x, pos.y));
        }
    }
    let mut free: Vec<(i32, i32)> = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| rules.rest_of(map.tile_at(x, y)) > 0)
        .filter(|cell| !taken.contains(cell))
        .collect();
    if free.is_empty() {
        return;
    }

    // Порядок обхода — по `id` кота: лежанок может не хватить, и «кому
    // досталась» обязано быть видно игроку, а обход ECS недетерминирован (§11).
    let mut cats: Vec<(&str, Entity, (i32, i32))> = idle
        .iter()
        // Полному дремать нечего, а стоящий на месте для сна уже дремлет:
        // послать его на соседнюю лежанку значило бы гонять кота по спальне.
        .filter(|(_, _, pos, energy)| {
            energy.0 < needs.max && rules.rest_of(map.tile_at(pos.x, pos.y)) <= 0
        })
        .map(|(e, id, pos, _)| (id.0.as_str(), e, (pos.x, pos.y)))
        .collect();
    cats.sort_unstable();
    for (_, cat_e, at) in cats {
        if free.is_empty() {
            return;
        }
        // Идём к лежанке **по шагу за раз**, а не целым маршрутом, и это не
        // мелочь. Кот с маршрутом невидим раздатчикам (`Without<Path>`,
        // инвариант 5), а дорога через полбазы длится десятки тиков: выдай
        // маршрут целиком — и освободившийся чертёж простоял бы всё это время,
        // пока бригада бредёт в спальню. Так кот свободен через тик, а идёт с
        // обычной скоростью: `MoveCooldown(1)` — тот же `MOVE_PERIOD`, который
        // `move_units` ставит после каждого шага.
        let Some((_, path)) = claim_bed(&map, &mut free, at) else {
            continue;
        };
        // ...но **останавливаться там, где стоять нельзя, нельзя и на шаг**:
        // на полке (§12.35) и под остановившимся котом (§12.32). Оба правила
        // разбираются после факта, и кот, замерший на такой клетке хотя бы на
        // тик, будет с неё согнан — а следующим тиком раздача пошлёт его туда
        // же. Это те самые качели, которые ловились на боевой партии между
        // стеллажами. Поэтому шаг растягивается до ближайшей клетки, на которой
        // можно замереть: `steps` лежит с конца, значит нужен её хвост.
        let stop_at = path.iter().rposition(|&cell| {
            !rules.is_solid(map.tile_at(cell.0, cell.1)) && !standing.contains(&cell)
        });
        let steps = match stop_at {
            Some(i) => path[i..].to_vec(),
            None => continue, // замереть по дороге негде — идти нет смысла
        };
        if !steps.is_empty() {
            commands
                .entity(cat_e)
                .insert((Path { steps }, MoveCooldown(1)));
        }
    }
}

/// Скорость и потолок сна на клетке под котом (§12.52).
///
/// Скорость: лежанка — свою, всё остальное — общий `floor` из рулсета. Ноль не
/// берём никогда: спать можно где угодно, вопрос только в том, сколько это
/// займёт, — иначе кот, уснувший на голом полу рулсета без `floor`, не
/// проснулся бы вовсе.
///
/// Потолок: докуда здесь высыпаются. Ноль — до полной, по общему правилу нулей
/// в рулсете (цена тайла, ёмкость склада, сложность вылазки), и ровно поэтому
/// мир без потолков ведёт себя как до §12.52.
fn sleeping_at(rules: &TileRules, needs: &NeedRules, tile: i16) -> (i32, i32) {
    let bed = rules.rest_of(tile);
    let ceiling = match bed > 0 {
        true => rules.wake_of(tile),
        false => needs.floor_wake,
    };
    let ceiling = match ceiling > 0 {
        true => ceiling.min(needs.max),
        false => needs.max,
    };
    (bed.max(needs.floor).max(1), ceiling)
}

/// Спящие коты восстанавливают бодрость и просыпаются на **потолке места**.
///
/// До §12.52 потолок был один — полная бодрость, — и кот занимал лежанку до
/// последнего очка. Теперь докуда высыпаются, решает клетка: лежанка — до
/// своего `wake`, пол — до общего `floor_wake`. Всё, что выше, — уже не сон, а
/// дремота (`doze`): она никого не держит и никем не защищена.
///
/// Бодрость при этом потолком **не режется**, режется только сон: снятый `Rest`
/// возвращает кота и раздатчикам, и игроку, а добирать до полной он будет,
/// пока base не найдёт ему дела.
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
        let (rate, ceiling) = sleeping_at(&rules, &needs, map.tile_at(pos.x, pos.y));
        energy.0 = (energy.0 + rate).min(needs.max);
        if energy.0 >= ceiling {
            commands.entity(cat_e).remove::<Rest>();
        }
    }
}

/// Дремота: кот, которому нечем заняться, добирает бодрость там же, где спал
/// (§12.52).
///
/// Своего компонента у неё нет и не должно быть, и это главное в решении:
/// дремота — это **отсутствие задачи** плюс место для сна под лапами. Отсюда
/// само собой следует всё, ради чего она заводилась:
///   * её видит любой раздатчик (все они фильтруют `Without<Rest>`, а `Rest`
///     тут нет) — появился чертёж, и кот встал;
///   * её не защищает §12.51 — приказ и заявка поднимают кота сразу;
///   * лежанку она не занимает: `assign_rest` считает занятыми места спящих, а
///     дремлющий не спит. Кто спит по нужде, тот и получит кровать, а
///     дремлющего сгонит с неё `spread_units`, когда сосед придёт (§12.32).
///
/// Работает только там, где спят по-настоящему (`rest > 0`): дремать посреди
/// коридора значило бы, что зона отдыха нужна лишь для скорости. Прибавка идёт
/// **сверх** `tire`, который у бодрствующего кота никто не отменял, — то есть
/// чистыми на лежанке набегает на очко меньше ставки. Второй арифметики для
/// этого не заводим: кот именно что бодрствует, просто лёжа.
pub(crate) fn doze(
    map: Res<BaseMap>,
    rules: Res<TileRules>,
    needs: Res<NeedRules>,
    mut cats: Query<
        (&Position, &mut Energy),
        (
            With<UnitId>,
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
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    for (pos, mut energy) in &mut cats {
        let tile = map.tile_at(pos.x, pos.y);
        if rules.rest_of(tile) <= 0 || energy.0 >= needs.max {
            continue;
        }
        let (rate, _) = sleeping_at(&rules, &needs, tile);
        energy.0 = (energy.0 + rate).min(needs.max);
    }
}

/// Бодрствование стоит бодрости — одинаково за работу, ходьбу и простой.
/// Кроме тех, кто на миссии: вне базы усталость не считается вовсе (§12.22).
///
/// **На пустой желудок она горит в `starve` раз быстрее** (§12.36). Это вся
/// цена голода: он стоит сил, а не жизни, — и берёт их существующей шкалой,
/// а не второй полоской рядом. Отсюда же и отсутствие у голода второго порога:
/// с работы кота уводит бодрость, дошедшая до критической, а не сам голод.
///
/// Спящего это не касается — у него нет `Energy` в убыль вовсе: голодный, из
/// чьего сна не выйти, был бы наказанием без выхода, а §12.20 такие отверг.
pub(crate) fn tire(
    food: Res<FoodRules>,
    needs: Res<NeedRules>,
    stats: Res<StatRules>,
    mut cats: Query<
        (&mut Energy, Option<&Fed>, Option<&Stats>),
        (With<UnitId>, Without<Rest>, Without<Away>),
    >,
) {
    for (mut energy, fed, cat) in &mut cats {
        let starving = fed.is_some_and(|f| f.0 <= 0);
        let rate = tire_rate(&needs, &stats, cat) * if starving { food.starve.max(1) } else { 1 };
        energy.0 = (energy.0 - rate).max(0);
    }
}
