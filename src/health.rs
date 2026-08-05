//! Здоровье: ранение, лазарет и работа медика (§12.5, §12.37 concept.md).
//!
//! Третья шкала кота после бодрости и сытости — и единственная, которая **сама
//! не тикает**. Усталость и голод берут своё за прожитый тик; здоровье не
//! убывает от жизни на базе вовсе: его роняет только вылазка, а возвращает
//! только лечение. Поэтому здесь нет ничего похожего на `tire` и `hunger`.
//!
//! **Смерти в игре нет.** Цена провала — котовремя: кот выбывает из работы на
//! сотни тиков, а не из мира. То же решение, которым §12.36 отказал голоду в
//! праве убивать, а §12.10 — необратимому наказанию в песочнице.
//!
//! Порог **один** (`hurt`), и он забирает кота жёстче, чем оба порога бодрости:
//! раненый **бросает начатое** и ложится. Уставший доделывает, потому что «устал»
//! — это про скорость, а «ранен» — про способность (§12.20, §12.33).
//!
//! Систем три:
//!   * `assign_heal` — раненый выбывает и занимает койку (а нет её — ложится
//!     где стоял); он же перекладывает лежачих на освободившиеся койки;
//!   * `assign_treat` — свободный кот берётся лечить: пациент здесь ровно та же
//!     **разметка работы**, что чертёж и тема, только выставил её не игрок, а
//!     мир (§12.16);
//!   * `heal` — тик восстановления, близнец `sleep`: скорость даёт клетка под
//!     котом, а пришедший медик прибавляет сверху.
//!
//! **Без медика раны заживают сами**, просто медленно, — ровно как сон на полу
//! против лежанки (§12.20). База без лазарета и без медиков не попадает в
//! тупик, она просто дольше собирает отряд.
//!
//! Связь «пациент ↔ медик» односторонняя по починке: `Healing::medic` — это
//! claim, как `Blueprint::assignee`, но снимать его умеет только `assign_treat`.
//! Любой, кто отбирает у кота работу (приказ игрока, истощение, собственное
//! ранение), просто снимает `Treating` — а осиротевшего пациента раздатчик
//! подберёт следующим тиком. Иначе освобождение пришлось бы вписать в каждое
//! место, где кота с работы снимают, и однажды забыть.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::{plan_spend, storage_order};
use crate::map::{BaseMap, DIRS};
use crate::needs::release_work;
use crate::path::Reach;
use crate::skills::{SKILL_MEDIC, level_of};

/// Сколько здоровья добавляет пришедший медик сверх заживления, на нулевом
/// навыке; каждый уровень «Медицины» прибавляет ещё очко — то же правило, что у
/// любой работы (`WORK_RATE` + уровень, §12.17).
const TREAT_RATE: i32 = 2;

/// Раненый бросает работу и ложится лечиться; лежачие перебираются на койки.
///
/// Стоит **первым из раздатчиков**, даже раньше еды: остальные девять решают,
/// чем кот займётся, а этот — что он вообще ничем заняться не может. Порядок
/// раздатчиков и есть приоритет (§12.15), и выбывший обязан выпасть из пула до
/// того, как пул начнут разбирать.
///
/// Два прохода по одному списку свободных коек, как у `assign_rest` (§12.33):
///   1. **Раненые** — ложатся, отпустив всё, за что держались.
///   2. **Лежачие без койки** — переезжают, когда койка освободилась или
///      достроилась. Без этого прохода первый же раненый до постройки лазарета
///      пролежал бы на полу всю игру.
///
/// Койки нет — кот всё равно ложится: ранение выводит из строя само по себе, а
/// лазарет только ускоряет. Обратное («не лечится, пока некуда лечь») означало
/// бы кота, который ранен и продолжает работать, — то есть отсутствие механики.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_heal(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<HealthRules>,
    mut commands: Commands,
    mut lying: Query<(Entity, &UnitId, &Position, &mut Healing)>,
    wounded: Query<
        (
            Entity,
            &UnitId,
            &Position,
            &Health,
            (
                Option<&Assignment>,
                Option<&Haul>,
                Option<&Researching>,
                Option<&Crafting>,
                Option<&Treating>,
            ),
        ),
        (With<UnitId>, Without<Healing>, Without<Away>),
    >,
    mut blueprints: Query<&mut Blueprint>,
    mut topics: Query<&mut Research>,
    mut orders: Query<&mut Craft>,
) {
    if rules.max <= 0 {
        return; // ранений в мире нет
    }

    // Занято и то, к чему идут, и то, на чём лежат: кот, свалившийся прямо на
    // койку, места в `Healing` не держит, но занимает его собой — как и спящий
    // на лежанке (§12.33).
    let mut taken: Vec<(i32, i32)> = Vec::new();
    for (_, _, pos, task) in &lying {
        taken.extend(task.spot);
        if tiles.heal_of(map.tile_at(pos.x, pos.y)) > 0 {
            taken.push((pos.x, pos.y));
        }
    }
    let mut free: Vec<(i32, i32)> = (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| tiles.heal_of(map.tile_at(x, y)) > 0)
        .filter(|cell| !taken.contains(cell))
        .collect();

    // 1. Раненые выбывают. Порядок обхода задан по `id`: когда коек меньше, чем
    // раненых, «кому достанется» видно игроку, а обход ECS недетерминирован
    // (§11, §12.24).
    let mut hurt: Vec<(&str, Entity, (i32, i32))> = wounded
        .iter()
        .filter(|(_, _, _, health, _)| health.0 <= rules.hurt)
        .map(|(cat_e, id, pos, ..)| (id.0.as_str(), cat_e, (pos.x, pos.y)))
        .collect();
    hurt.sort_unstable_by_key(|&(id, ..)| id);
    for (_, cat_e, at) in hurt {
        let Ok((.., work)) = wounded.get(cat_e) else {
            continue;
        };
        release_work(
            &mut commands,
            cat_e,
            work,
            &mut blueprints,
            &mut topics,
            &mut orders,
        );
        // Койка вешается строго после освобождения задачи: `release_work`
        // снимает `Path`, а команды применяются в порядке добавления.
        let ward = claim_ward(&map, &mut free, at);
        let (spot, path) = match ward {
            Some((cell, path)) => (Some(cell), path),
            None => (None, Vec::new()),
        };
        commands.entity(cat_e).insert((
            Healing {
                spot,
                medic: None,
                kit: 0,
            },
            Path { steps: path },
            MoveCooldown(0),
        ));
    }

    // 2. Переезд лежачих: койку могли достроить или освободить, пока кот лежал
    // на полу. Тот же третий проход, что у сна (§12.33), и по той же причине —
    // «лежит и не лечится толком» игрок прочитает как зависшего кота.
    if free.is_empty() {
        return;
    }
    let mut floored: Vec<(&str, Entity, (i32, i32))> = lying
        .iter()
        .filter(|(_, _, pos, task)| {
            task.spot.is_none() && tiles.heal_of(map.tile_at(pos.x, pos.y)) <= 0
        })
        .map(|(cat_e, id, pos, _)| (id.0.as_str(), cat_e, (pos.x, pos.y)))
        .collect();
    floored.sort_unstable_by_key(|&(id, ..)| id);
    let moves: Vec<(Entity, (i32, i32), Vec<(i32, i32)>)> = floored
        .into_iter()
        .filter_map(|(_, cat_e, at)| {
            claim_ward(&map, &mut free, at).map(|(cell, path)| (cat_e, cell, path))
        })
        .collect();
    for (cat_e, cell, path) in moves {
        if let Ok((.., mut task)) = lying.get_mut(cat_e) {
            task.spot = Some(cell);
        }
        commands
            .entity(cat_e)
            .insert((Path { steps: path }, MoveCooldown(0)));
    }
}

/// Занятая котом койка и маршрут к ней.
type Ward = ((i32, i32), Vec<(i32, i32)>);

/// Забрать из `free` ближайшую достижимую койку и маршрут к ней; `None` —
/// свободных не осталось или ни до одной не дойти. Близнец `claim_bed`
/// (§12.20): выбор по расстоянию — общее правило раздачи (§12.14).
fn claim_ward(map: &BaseMap, free: &mut Vec<(i32, i32)>, from: (i32, i32)) -> Option<Ward> {
    if free.is_empty() {
        return None;
    }
    let reach = Reach::all(map, from);
    let (i, cell, _) = free
        .iter()
        .enumerate()
        .filter_map(|(i, &(x, y))| reach.dist_at(x, y).map(|d| (i, (x, y), d)))
        .min_by_key(|&(_, _, d)| d)?;
    free.remove(i);
    Some((cell, reach.path_to(cell.0, cell.1).unwrap_or_default()))
}

/// Ставит свободных котов лечить раненых.
///
/// Пациент — **разметка работы**, как чертёж и как тема (§12.16): что делать,
/// видно само, а исполнителя берёт симуляция — ближайшего (§12.14). Допуска по
/// навыку здесь нет, как и у производства: «Медицина» только ускоряет, потому
/// что войти в домен иначе было бы не через что (§12.30, §12.18).
///
/// Работает медик **с соседней клетки**, как строитель у чертежа: в клетке
/// пациента не встать (§12.32), а правило соседства уже есть у стройки (§12.12).
///
/// Здесь же снимается протухший claim: медик, которого увёл приказ игрока или
/// уложило истощение, теряет только свой `Treating`, а пациент замечает потерю
/// сам. Одно место починки вместо семи мест освобождения.
pub(crate) fn assign_treat(
    map: Res<BaseMap>,
    rules: Res<HealthRules>,
    mut commands: Commands,
    mut patients: Query<(Entity, &Position, &mut Healing)>,
    medics: Query<&Treating>,
    free_cats: Query<
        (Entity, &UnitId, &Position),
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
            Without<Squad>,
            // Пленного нет на базе, и отряда за ним больше нет (§12.40):
            // фильтр по `Squad` его бы не поймал, а работа поймала бы.
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    if rules.max <= 0 {
        return;
    }

    // Медик ушёл или его увели — claim протух.
    for (patient_e, _, mut task) in &mut patients {
        let held = task
            .medic
            .is_some_and(|m| medics.get(m).is_ok_and(|t| t.0 == patient_e));
        if !held {
            task.medic = None;
        }
    }

    // Кто может взяться: свободные, в фиксированном порядке по `id`. Обход ECS
    // зависит от истории вставок, а «кого сняли с простоя» видно игроку.
    let mut idle: Vec<(&str, Entity, (i32, i32))> = free_cats
        .iter()
        .map(|(cat_e, id, pos)| (id.0.as_str(), cat_e, (pos.x, pos.y)))
        .collect();
    if idle.is_empty() {
        return;
    }
    idle.sort_unstable_by_key(|&(id, ..)| id);

    // Пациенты в порядке карты, а не сущностей: очередь к медику — это то, что
    // игрок видит, и она не должна зависеть от истории вставок (§11).
    let mut waiting: Vec<(Entity, (i32, i32))> = patients
        .iter()
        .filter(|(_, _, task)| task.medic.is_none())
        .map(|(patient_e, pos, _)| (patient_e, (pos.x, pos.y)))
        .collect();
    waiting.sort_unstable_by_key(|&(_, (x, y))| (y, x));

    for (patient_e, at) in waiting {
        let chosen = idle
            .iter()
            .enumerate()
            .filter_map(|(i, &(_, cat_e, from))| {
                let reach = Reach::all(&map, from);
                treat_spot(&map, &reach, at).map(|(spot, steps)| (steps, i, cat_e, spot, reach))
            })
            .min_by_key(|&(steps, ..)| steps);
        let Some((_, i, cat_e, spot, reach)) = chosen else {
            continue; // некому взяться или к раненому не подойти
        };

        idle.remove(i);
        if let Ok((.., mut task)) = patients.get_mut(patient_e) {
            task.medic = Some(cat_e);
        }
        let path = reach.path_to(spot.0, spot.1).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Treating(patient_e), Path { steps: path }, MoveCooldown(0)));
    }
}

/// Клетка, с которой медик работает над лежащим, и её цена в шагах.
///
/// Только соседи: сам пациент занимает свою клетку, и встать на неё нельзя
/// (§12.32). Из подходящих берём ближайшую к медику — общее правило (§12.14).
fn treat_spot(map: &BaseMap, reach: &Reach, at: (i32, i32)) -> Option<((i32, i32), i32)> {
    DIRS.iter()
        .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
        .filter(|&(x, y)| map.walkable(x, y))
        .filter_map(|c| reach.dist_at(c.0, c.1).map(|d| (c, d)))
        .min_by_key(|&(_, d)| d)
}

/// Какую аптечку израсходовать и что для этого снять со склада (§12.47).
///
/// Перебор идёт по палитре предметов, а не по кучам: какую именно возьмут,
/// когда аптечек несколько, должно быть предсказуемо (§11). Считает и списывает
/// это общий `plan_spend` — та же арифметика, что у найма, науки и заказа.
fn plan_kit(
    piles: &[(Entity, usize, i32)],
    items: &ItemRules,
) -> Option<(i32, Vec<(Entity, i32)>)> {
    items
        .medkits()
        .find_map(|(item, mends)| plan_spend(piles, &[(item, 1)]).map(|takes| (mends, takes)))
}

/// Лежачие коты заживают; дошедший медик ускоряет, а на полной шкале кот встаёт.
///
/// Близнец `sleep` (§12.20): скорость даёт клетка под котом — койка свою, всё
/// остальное общий `mend` из рулсета, — и ноль не берётся никогда, иначе кот,
/// раненный до постройки лазарета, не встал бы вовсе.
///
/// Медик прибавляет `TREAT_RATE` плюс уровень «Медицины» и растит его сам: здесь
/// только маркер `Worked`, опыт начисляет `train_skills` в конце цепочки
/// (§12.17). Пока медик идёт — он не лечит: дорога это дорога.
///
/// Здесь же тратится аптечка (§12.47) — **в момент, когда лечение действительно
/// началось**, а не при назначении медика: оплаченное вперёд заморозило бы склад
/// под работу, которая может не начаться (тот же довод, что у заказа, §12.30).
/// Аптечка **ускоряет, а не открывает**: не нашлось — лечение идёт как раньше.
pub(crate) fn heal(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<HealthRules>,
    skill_rules: Res<SkillRules>,
    items: Res<ItemRules>,
    mut commands: Commands,
    mut patients: Query<(Entity, &Position, &mut Health, &mut Healing, Option<&Path>)>,
    medics: Query<(Entity, &Position, &Treating, Option<&Path>, Option<&Skills>)>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    if rules.max <= 0 {
        return;
    }
    let medicine = skill_rules.index_of(SKILL_MEDIC);

    // Кто из медиков уже стоит у пациента: пришедший лечит, идущий — нет.
    let arrived: Vec<(Entity, Entity, (i32, i32), i32)> = medics
        .iter()
        .filter(|(_, _, _, path, _)| path.is_none())
        .map(|(medic_e, pos, task, _, skills)| {
            let level = medicine.map_or(0, |s| level_of(&skill_rules, skills, s));
            (medic_e, task.0, (pos.x, pos.y), level)
        })
        .collect();

    for (cat_e, pos, mut health, mut task, path) in &mut patients {
        if path.is_some() {
            continue; // ещё идёт к койке
        }
        let mut rate = tiles
            .heal_of(map.tile_at(pos.x, pos.y))
            .max(rules.mend)
            .max(1);
        // Считается только свой медик и только дошедший: соседняя клетка — то же
        // условие, по которому строитель работает у чертежа (§12.12).
        let own = task.medic;
        let helper = arrived.iter().find(|&&(medic_e, patient_e, at, _)| {
            Some(medic_e) == own
                && patient_e == cat_e
                && (at.0 - pos.x).abs() + (at.1 - pos.y).abs() <= 1
        });
        if let Some(&(medic_e, _, _, level)) = helper {
            rate += TREAT_RATE + level;
            if let Some(skill) = medicine {
                commands.entity(medic_e).insert(Worked(skill));
            }
            // Аптечка тратится один раз на раненого: признак «уже потратили» —
            // сам `kit`, второго флага не заводим. Списание общее с наймом,
            // наукой и заказом (§12.47).
            if task.kit == 0 {
                let piles = storage_order(
                    &map,
                    &tiles,
                    stacks
                        .iter()
                        .map(|(e, p, s)| (e, (p.x, p.y), s.item, s.count)),
                );
                if let Some((mends, takes)) = plan_kit(&piles, &items) {
                    for (pile_e, taken) in takes {
                        if let Ok((_, _, mut stack)) = stacks.get_mut(pile_e) {
                            stack.count -= taken;
                            if stack.count <= 0 {
                                commands.entity(pile_e).despawn();
                            }
                        }
                    }
                    task.kit = mends;
                }
            }
        }
        // Перевязка помогает и после ухода медика: аптечку уже израсходовали.
        rate += task.kit;

        health.0 = (health.0 + rate).min(rules.max);
        if health.0 >= rules.max {
            commands.entity(cat_e).remove::<Healing>();
            if let Some(medic_e) = task.medic {
                commands.entity(medic_e).remove::<Treating>();
            }
        }
    }
}
