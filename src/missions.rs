//! Вылазки: отряд уходит с базы и возвращается с добычей (§12.22, §12.23
//! concept.md).
//!
//! **Отряд выбирает игрок, поимённо** (§12.23) — единственная работа, где
//! исполнитель не раздаётся симуляцией. Причина ровно одна: от состава зависит
//! исход, а всё остальное на базе одинаково выполнимо любым котом (§12.16).
//!
//! Систем две:
//!   * `gather_squad` — держит выбранный отряд идущим к шлюзу, переживая
//!     изменения карты. Состав не трогает: заменить выбывшего некем.
//!   * `run_missions` — отправляет собравшийся отряд, крутит таймер, считает
//!     исход и возвращает котов с добычей.
//!
//! Отдельного состояния «фаза миссии» нет, как его нет у `Haul` и `Rest`: где
//! отряд, видно по компонентам. `Squad` с маршрутом — кот идёт к шлюзу,
//! `Squad` без маршрута на шлюзе — ждёт остальных, `Away` — ушёл.
//!
//! **Добыча ложится кучей на шлюз**, а не в лапы котам: кот везёт один тип за
//! ходку (§12.21), а добыча бывает набором. Дальше её разносит обычная уборка
//! (§12.16) — и то, что миссия ничего не знает про склад, тут не упрощение,
//! а прямое следствие: возврат от сноса ложится под ноги ровно так же.
//!
//! **Исход детерминирован** (`outcome`): сила отряда против сложности вылазки,
//! без броска кубика. Плата за вылазку — бодрость: та же валюта котовремени, в
//! которой измеряется и сама отправка отряда (§12.23).

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::hauling::spill;
use crate::map::BaseMap;
use crate::path::{Reach, find_path};
use crate::skills::{SKILL_RAID, level_of};

/// Все клетки-шлюзы карты, в порядке обхода: он фиксирован, значит выбор
/// шлюза детерминирован (§11).
fn gate_cells<'a>(map: &'a BaseMap, rules: &'a TileRules) -> impl Iterator<Item = (i32, i32)> + 'a {
    (0..map.height)
        .flat_map(move |y| (0..map.width).map(move |x| (x, y)))
        .filter(move |&(x, y)| rules.is_gate(map.tile_at(x, y)))
}

/// Чем кончится вылазка. Считается и на возвращении, и каждый кадр для панели:
/// прогноз и результат обязаны быть одним и тем же выражением, иначе игрок
/// увидит одно, а получит другое.
#[derive(Clone, Copy)]
pub(crate) struct Outcome {
    pub(crate) strength: i32,
    /// Какая доля добычи достанется, в процентах (0..=100).
    pub(crate) share: i32,
    /// Провал: силы не хватило даже вполовину.
    pub(crate) failed: bool,
}

/// Исход по силе отряда и сложности вылазки — **без броска кубика** (§12.23).
///
/// Детерминизм здесь не формальность: он единственное, что делает выбор отряда
/// читаемым. С кубиком игрок не отличил бы «выбрал слабый отряд» от «не
/// повезло», а вылазок за сеанс столько, что вероятность так и не проступит.
///
/// Хватило силы — вся добыча; не хватило — её доля; вдвое меньше нужного —
/// провал: ни добычи, ни сил. Нулевая сложность удаётся всегда, по общему
/// правилу нулей в рулсете (цена тайла, ёмкость склада, потолок бодрости).
pub(crate) fn outcome(danger: i32, strength: i32) -> Outcome {
    if danger <= 0 {
        return Outcome {
            strength,
            share: 100,
            failed: false,
        };
    }
    let failed = strength * 2 < danger;
    Outcome {
        strength,
        share: if failed {
            0
        } else {
            (strength * 100 / danger).min(100)
        },
        failed,
    }
}

/// Держит выбранный игроком отряд идущим к шлюзу.
///
/// Состав здесь не трогается: его назначил `Sim::launch` в момент заявки, и
/// заменить выбывшего некем — это выбор игрока, а не раздача работы (§12.23).
/// Система нужна ровно для того, чтобы маршрут переживал изменения карты:
/// шлюз снесли, кота выбросило из ямы, боец проснулся после истощения.
/// Тот же случай, что `retry_orders` у приказа игрока.
pub(crate) fn gather_squad(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    crew: Query<
        (Entity, &Squad, &Position, Option<&Path>, Option<&Away>),
        // Спящего не трогаем: маршрут разбудил бы его, а истощение — не повод
        // гнать кота дальше. Проснётся — эта же система его и подберёт. Идущего
        // за снаряжением — тем более: одетым он уйдёт сильнее (§12.29, §12.34).
        // Раненого — тоже: в отряд его не берут вовсе, а если ранило уже
        // назначенного, отряд ждёт, пока тот встанет (§12.23, §12.37).
        (Without<Rest>, Without<Equipping>, Without<Healing>),
    >,
) {
    if missions.is_empty() {
        return;
    }
    let map = &*map;

    for (mission_e, mut mission) in &mut missions {
        // Кто в отряде и где он. Пустой маршрут считается пройденным:
        // `move_units` снимет его только следующим тиком.
        let mut left_base = false;
        let squad: Vec<(Entity, (i32, i32), bool)> = crew
            .iter()
            .filter(|(_, s, ..)| s.0 == mission_e)
            .inspect(|(.., away)| left_base |= away.is_some())
            .map(|(e, _, p, path, _)| {
                let walking = path.is_some_and(|p| !p.steps.is_empty());
                (e, (p.x, p.y), walking)
            })
            .collect();
        // Отряд ушёл — шлюз больше не пересматривается: вернутся коты туда,
        // откуда ушли, даже если гараж успели снести.
        if left_base {
            continue;
        }

        // Шлюз мог уйти под ластиком, пока отряд собирался, — выбираем заново.
        if mission
            .gate
            .is_some_and(|(x, y)| !tiles.is_gate(map.tile_at(x, y)))
        {
            mission.gate = None;
        }
        if mission.gate.is_none() {
            let at: Vec<(i32, i32)> = squad.iter().map(|&(_, at, _)| at).collect();
            mission.gate = pick_gate(map, &tiles, &at);
        }
        let Some(gate) = mission.gate else {
            continue; // шлюза нет или до него не добраться всем разом
        };

        for &(cat_e, at, walking) in &squad {
            if walking || at == gate {
                continue;
            }
            if let Some(steps) = find_path(map, at, gate) {
                commands
                    .entity(cat_e)
                    .insert((Path { steps }, MoveCooldown(0)));
            }
        }
    }
}

/// Шлюз, к которому отряду суммарно ближе всего идти.
///
/// Клетки, до которых дойдут не все, отбрасываются: состав фиксирован, и шлюз,
/// отрезанный от одного из бойцов, значит вылазку, которая никогда не тронется.
/// Ничьи разрешает порядок обхода карты, то есть детерминированно.
pub(crate) fn pick_gate(map: &BaseMap, tiles: &TileRules, at: &[(i32, i32)]) -> Option<(i32, i32)> {
    let reaches: Vec<Reach> = at.iter().map(|&p| Reach::all(map, p)).collect();
    gate_cells(map, tiles)
        .filter_map(|(x, y)| {
            let mut total = 0;
            for r in &reaches {
                total += r.dist_at(x, y)?;
            }
            Some((total, (x, y)))
        })
        .min_by_key(|&(total, _)| total)
        .map(|(_, cell)| cell)
}

/// Есть ли на базе силы прийти за пленным — считается **до** того, как его
/// оставят (§12.40).
///
/// Плен обратим по определению: он стоит котовремени, а не кота (§12.37). Но
/// обратимость держится не на обещании, а на этой проверке: если базе некем
/// снарядить самый маленький спасательный отряд, отряд тащит раненого сам, и
/// плена не случается вовсе. Иначе провал на трёх котах запирал бы игру
/// насмерть — та самая необратимость, которой §12.10 избегает.
///
/// Считаются вылазки, доступные **прямо сейчас**: известность только растёт
/// (§12.24), но ждать её, сидя в плену, кот будет столько же, сколько никогда.
fn rescue_is_possible(rules: &MissionRules, fame: i32, on_base: usize) -> bool {
    rules
        .0
        .iter()
        .any(|r| r.rescue && r.requires <= fame && r.squad <= on_base)
}

/// Отправляет собравшийся отряд, крутит таймер и возвращает котов с добычей.
///
/// Стоит после `move_units`: кот, шагнувший на шлюз в этом тике, засчитывается
/// сразу, — и до `settle_stacks`, чтобы добыча, вывалившаяся в свежую яму,
/// съехала на пол тем же тиком (§12.15).
///
/// **Провал может оставить кота в плену** (§12.40), а вылазка с `rescue` —
/// привести пленных обратно. И то, и другое — работа с составом отряда, а не с
/// добычей, поэтому живёт здесь же: другого места, где известно, чем кончилась
/// вылазка, нет.
pub(crate) fn run_missions(
    rules: Res<MissionRules>,
    skill_rules: Res<SkillRules>,
    items: Res<ItemRules>,
    mut fame: ResMut<Fame>,
    mut commands: Commands,
    mut missions: Query<(Entity, &mut Mission)>,
    mut crew: Query<(
        Entity,
        &Squad,
        &UnitId,
        &Position,
        Option<&Path>,
        Option<&Away>,
        Option<&Skills>,
        Option<&mut Energy>,
        Option<&Gear>,
        Option<&mut Health>,
    )>,
    // Кто остаётся дома: по нему считается, есть ли кому прийти за пленным.
    // Ушедшие сюда не попадают — у них `Away`, — поэтому возвращающийся отряд
    // прибавляется отдельно.
    home: Query<(), (With<UnitId>, Without<Away>)>,
    // Пленные — те, за кем идёт вылазка с `rescue`. Только сущности: позицию
    // им ставит `Commands`, иначе этот запрос конфликтовал бы с `crew` за
    // `Position` и bevy уронил бы систему на старте.
    captives: Query<Entity, With<Captive>>,
    mut stacks: Query<(Entity, &Position, &mut Stack)>,
) {
    let raid = skill_rules.index_of(SKILL_RAID);
    for (mission_e, mut mission) in &mut missions {
        let Some(rule) = rules.0.get(mission.def) else {
            continue;
        };
        let Some(gate) = mission.gate else {
            continue; // шлюз ещё не выбран
        };

        let squad: Vec<(Entity, (i32, i32), bool, bool, i32)> = crew
            .iter()
            .filter(|(_, s, ..)| s.0 == mission_e)
            .map(|(e, _, _, p, path, away, skills, _, gear, _)| {
                let walking = path.is_some_and(|p| !p.steps.is_empty());
                // Вклад кота в силу отряда: сам он стоит единицу, навык —
                // сверху, надетое — ещё сверху. Нулевой навык поэтому не значит
                // «бесполезен», а снаряжение — второе слагаемое, которое растёт
                // не от навыка и не упирается в его потолок (§12.29).
                let force = 1
                    + raid.map_or(0, |s| level_of(&skill_rules, skills, s))
                    + items.force_of_gear(gear);
                (e, (p.x, p.y), walking, away.is_some(), force)
            })
            .collect();

        // Отряд в поле. База о нём ничего не знает: ни усталости, ни маршрутов —
        // вылазка считается разом по возвращении, а не симулируется (§12.22).
        if squad.iter().any(|&(.., away, _)| away) {
            // Опыт капает за тик в поле, как и на любой другой работе (§12.17):
            // начисляет его `train_skills` в конце цепочки, здесь только маркер.
            if let Some(skill) = raid {
                for &(cat_e, ..) in &squad {
                    commands.entity(cat_e).insert(Worked(skill));
                }
            }
            mission.left -= 1;
            if mission.left > 0 {
                continue;
            }

            // Исход считаем по силе **на возвращении**: за вылазку навык вырос,
            // и отнимать этот рост у самой вылазки было бы странно.
            let force = squad.iter().map(|&(.., f)| f).sum();
            let out = outcome(rule.danger, force);

            // Кого не смогли унести (§12.40). Провал бьёт всех одинаково, и
            // здоровье до вычета упорядочено так же, как после, — поэтому
            // выбирать можно здесь, не дожидаясь урона: тяжелее всех тому, кто
            // и шёл самым битым. Ничьи разрешает `id`, а не порядок сущностей
            // в ECS: «кого оставили» игрок увидит и запомнит (§12.24).
            //
            // Своя же вылазка за пленным пленных не оставляет никогда, иначе
            // неудачное спасение плодит второго пленника и база уходит в
            // спираль. И некому идти — тоже не оставляет: см. `rescue_is_possible`.
            let on_base = home.iter().count() + squad.len() - 1;
            let captive =
                if out.failed && !rule.rescue && rescue_is_possible(&rules, fame.0, on_base) {
                    let mut left_behind: Vec<(i32, &str, Entity)> = crew
                        .iter()
                        .filter(|(_, s, ..)| s.0 == mission_e)
                        .map(|(e, _, id, _, _, _, _, _, _, health)| {
                            (health.map_or(i32::MAX, |h| h.0), id.0.as_str(), e)
                        })
                        .collect();
                    left_behind.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
                    left_behind.first().map(|&(.., e)| e)
                } else {
                    None
                };

            // Успешное спасение возвращает **всех**, кого нашли: делить своих
            // на «этого забрали, а за тем сходите ещё раз» — это не сложность,
            // а вторая ходка за тем же самым. Частичный успех тоже возвращает:
            // доля считается в добыче, а кот либо дома, либо нет (§12.40).
            if rule.rescue && !out.failed {
                for cat_e in &captives {
                    commands
                        .entity(cat_e)
                        .remove::<(Away, Captive)>()
                        .insert(Position {
                            x: gate.0,
                            y: gate.1,
                        });
                }
            }

            for &(cat_e, ..) in &squad {
                // Пленный остаётся в поле: `Away` при нём, отряда больше нет —
                // миссия сейчас исчезнет, и ссылка на неё повисла бы.
                match Some(cat_e) == captive {
                    true => {
                        commands.entity(cat_e).remove::<Squad>().insert(Captive);
                    }
                    false => {
                        commands.entity(cat_e).remove::<(Away, Squad)>();
                    }
                }
                // Плата за вылазку — котовремя: та же валюта, в которой
                // измеряется и сама отправка отряда. Провал забирает всё, и
                // коты валятся у шлюза — `collapse_exhausted` подберёт их.
                //
                // Раны считаются **той же долей**, что и добыча: полный успех не
                // царапает никого, полсилы стоят половины `harm`, провал — всего
                // (§12.37). Отдельной формулы для урона нет намеренно: две
                // арифметики исхода разошлись бы, а прогноз в панели показывал
                // бы игроку не то, что случится (§12.23).
                if let Ok((.., energy, _, health)) = crew.get_mut(cat_e) {
                    if let Some(mut energy) = energy {
                        let toll = if out.failed { energy.0 } else { rule.toll };
                        energy.0 = (energy.0 - toll).max(0);
                    }
                    if let Some(mut health) = health {
                        health.0 = (health.0 - rule.harm * (100 - out.share) / 100).max(0);
                    }
                }
                // Провал ломает снаряжение: до него он стоил только бодрости, а
                // она восстанавливается бесплатно — то есть заведомо провальная
                // вылазка была способом качать «Вылазку» за одно лишь время
                // (§12.29). Успех не изнашивает: износ за каждый выход
                // превратил бы петлю «добыча → сила» в оброк. Комплект наберётся
                // заново, как только на складе снова будет из чего.
                if out.failed {
                    commands.entity(cat_e).remove::<Gear>();
                }
            }
            // Добыча ложится кучей на шлюз — ровно как возврат от сноса ложится
            // под ноги сносильщику. Развозит её обычная уборка (§12.16).
            for &(item, count) in &rule.loot {
                let got = count * out.share / 100;
                if got > 0 {
                    spill(&mut commands, &mut stacks, gate, item, got);
                }
            }
            // Известность идёт той же долей, что и добыча: слухи расходятся по
            // сделанному, а не по задуманному. Провал не приносит ничего — но и
            // не отнимает: ворота, которые закрываются, читаются как поломка
            // (§12.24).
            fame.0 += rule.fame * out.share / 100;
            commands.entity(mission_e).despawn();
            continue;
        }

        // Уходим, только когда отряд в полном составе стоит на шлюзе: недобор
        // здесь — это не «пошли вдвоём вместо троих», а «ещё идут».
        let ready = squad.len() == rule.squad
            && squad
                .iter()
                .all(|&(_, at, walking, ..)| at == gate && !walking);
        if ready {
            for &(cat_e, ..) in &squad {
                commands.entity(cat_e).insert(Away);
            }
        }
    }
}
