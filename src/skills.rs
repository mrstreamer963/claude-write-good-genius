//! Навыки: рост от работы и от обучения (§12.17, §12.18 concept.md).
//!
//! Навык — домен работы, а не действие игрока: стройка и снос это один навык,
//! потому что и джоб у них один. Растёт навык от самой работы, поэтому набор
//! навыков задаётся рулсетом, а не полями компонента — иначе каждый новый
//! домен лез бы в компонент, снапшот и UI.
//!
//! Начисление живёт здесь одной системой: система работы только вешает маркер
//! `Worked`. Правило роста, потолок и кривая не расползаются по работам.
//!
//! **Врождённый параметр живёт здесь же** (§12.19, §12.42): он не третья
//! система, а предел, в который упираются обе, — «докуда доучишься». Растёт
//! навык от работы, а докуда он вырастет, решено при рождении кота, и потому
//! потолок опыта у каждого кота свой.
//!
//! **Обучение — вторая система этого же модуля**, потому что это работа,
//! продукт которой сам навык (§12.18). Рост от работы не умеет стартовать сам:
//! чтобы набрать опыт исследования, надо уже уметь исследовать, — а «Наука»
//! ещё и допуск, без неё кот не исследует никак, а не медленно. Кот идёт на
//! клетку с `teaches`, стоит и тикает; `Worked` и `train_skills` те же самые.

use bevy_ecs::prelude::*;

use crate::components::*;
use crate::map::BaseMap;
use crate::path::{Reach, find_path};

/// Домен «Стройка» — он же снос.
pub(crate) const SKILL_BUILD: &str = "build";

/// Домен «Вылазка»: растёт за каждый тик в поле и прибавляет отряду силы
/// (§12.23). Второй домен работы — тот самый триггер 19b, после которого
/// коты перестают быть взаимозаменяемыми.
pub(crate) const SKILL_RAID: &str = "raid";

/// Домен «Ремесло»: растёт от производства и **только ускоряет** его (§12.30).
/// Четвёртый домен работы и первый после «Стройки», у которого нет ни допуска,
/// ни парты: вход в него — сама работа.
pub(crate) const SKILL_CRAFT: &str = "craft";

/// Домен «Медицина»: растёт от лечения и **только ускоряет** его (§12.37).
/// Пятый домен работы и второй после «Ремесла», у которого нет ни допуска, ни
/// парты: без него раны заживают сами, просто дольше, — а допуск означал бы
/// базу, где выбывшего некому поднять.
pub(crate) const SKILL_MEDIC: &str = "medicine";

/// Домен «Связь»: растёт от дежурства на узле и прибавляет силы **чужой**
/// вылазке (§12.60). Шестой домен работы и единственный, чья польза достаётся
/// не тому, кто работает.
///
/// Отдельный домен, а не «Вылазка», **намеренно**: считай связь по «Вылазке» —
/// и лучшим связистом окажется лучший боец, то есть игрок ослаблял бы отряд ровно
/// тем, чем усиливает, и решение отменяло бы само себя. Врождённый предел у него
/// идёт от «Ума» (`demands`, §12.42): кого сажать к рации, решает голова, и у боя
/// это не отнимает ничего, кроме котовремени.
pub(crate) const SKILL_RELAY: &str = "relay";

/// Домен «Наука»: растёт от исследования и **работает допуском** — без уровня
/// кот за тему не берётся вовсе (§12.18, §12.26). Третий домен работы и первый,
/// в который нельзя войти без парты.
pub(crate) const SKILL_SCIENCE: &str = "science";

/// Очков опыта за тик работы. Опыт капает за тик, а не за готовый тайл: так он
/// не зависит от того, кто доделал чужой чертёж, и растёт ровно по правилу
/// «чем больше кот что-то делает, тем лучше умеет».
const XP_PER_TICK: i32 = 1;

/// Уровень навыка кота. Навыков у кота может не быть вовсе (тесты чужих
/// механик, коты из ASCII-схем) — это нулевой уровень, а не ошибка.
pub(crate) fn level_of(rules: &SkillRules, skills: Option<&Skills>, skill: usize) -> i32 {
    rules.level(skill, skills.map_or(0, |s| s.xp_of(skill)))
}

/// Значение врождённого параметра, которым домен ограничен (§12.42); ноль —
/// домен не ограничен вовсе либо параметра у кота нет.
fn stat_value(rules: &SkillRules, stats: Option<&Stats>, skill: usize) -> i32 {
    let Some(stat) = rules.0.get(skill).and_then(|r| r.stat) else {
        return 0;
    };
    stats.map_or(0, |s| s.value_of(stat))
}

/// Докуда этот кот вырастет в домене: предел уровня по врождённому параметру
/// (§12.42). Равен потолку навыка, если домен параметром не ограничен.
pub(crate) fn level_cap_of(rules: &SkillRules, stats: Option<&Stats>, skill: usize) -> i32 {
    rules.stat_level_cap(skill, stat_value(rules, stats, skill))
}

/// Потолок опыта этого кота в домене — с учётом врождённого параметра.
///
/// Одно место, где предел превращается в число: и рост от работы, и парта
/// упираются в него, а разошлись бы они молча — кот сидел бы за партой,
/// которая ничего ему не даёт.
pub(crate) fn xp_ceiling(rules: &SkillRules, stats: Option<&Stats>, skill: usize) -> i32 {
    rules.stat_xp_cap(skill, stat_value(rules, stats, skill))
}

/// Докуда доводит парта **этого** кота: ниже из двух пределов — потолка парты
/// (§12.18) и врождённого (§12.42). Первый — свойство домена, второй — кота.
pub(crate) fn desk_cap(rules: &SkillRules, stats: Option<&Stats>, skill: usize) -> i32 {
    rules.taught_cap(skill).min(xp_ceiling(rules, stats, skill))
}

/// Клетки парт этого домена на карте — в порядке обхода, а не по расстоянию.
///
/// Отдельным выражением, потому что спрашивают о них двое и о разном: ворота —
/// «есть ли они вообще», выбор парты — «какая ближе».
fn desk_cells<'a>(
    map: &'a BaseMap,
    tiles: &'a TileRules,
    skill: usize,
) -> impl Iterator<Item = (i32, i32)> + 'a {
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| (x, y)))
        .filter(move |&(x, y)| tiles.teaches_of(map.tile_at(x, y)) == Some(skill))
}

/// Ближайшая свободная парта по **уже готовому** обходу.
///
/// Обход отдаётся снаружи, потому что снимок считает ворота по всем доменам
/// сразу: `Reach::all` — это два вектора на всю карту, и строить его на каждый
/// домен значило бы гонять BFS шесть раз на кота каждым кадром.
pub(crate) fn nearest_desk_at(
    map: &BaseMap,
    tiles: &TileRules,
    reach: &Reach,
    skill: usize,
    taken: &[(i32, i32)],
) -> Option<(i32, i32)> {
    desk_cells(map, tiles, skill)
        .filter(|cell| !taken.contains(cell))
        .filter_map(|(x, y)| reach.dist_at(x, y).map(|d| (d, (x, y))))
        .min_by_key(|&(d, _)| d)
        .map(|(_, cell)| cell)
}

/// Ближайшая свободная парта нужного домена; `None` — их нет или не дойти.
///
/// Занятые перечисляет вызывающий: занятость парты держит `Study` ученика, как
/// занятость лежанки держит `Rest` спящего (§12.20). Делить парту нельзя по той
/// же причине — иначе их число ни на что не влияет.
pub(crate) fn nearest_desk(
    map: &BaseMap,
    tiles: &TileRules,
    skill: usize,
    from: (i32, i32),
    taken: &[(i32, i32)],
) -> Option<(i32, i32)> {
    let reach = Reach::all(map, tiles, from);
    nearest_desk_at(map, tiles, &reach, skill, taken)
}

/// Ворота обучения: куда сажать — или почему нельзя.
///
/// **Одно выражение на фасад и на снимок** (инвариант 14). У `Sim::teach` пять
/// отказов, и один из них — «свободная парта есть, но до неё не дойти» — виду
/// невыразим вовсе: считать достижимость в JS значит завести второй экземпляр
/// правила, который однажды покажет живой кнопку, отклонённую фасадом.
///
/// Наружу едет **тег**, а слово по нему подбирает вид (§12.53) — ровно то же
/// деление, что у `missions::Phase` (§12.168): ядро не занимается подписями.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Desk {
    /// Сажать сюда.
    Open((i32, i32)),
    /// Домену за партой не учат вовсе («Стройка»): он растёт только работой.
    Untaught,
    /// Кота нет на базе — учить некого (§12.22).
    Away,
    /// Парта его уже ничему не научит: дальше только практика.
    Topped,
    /// Парт этого домена на базе нет ни одной.
    Missing,
    /// Парты есть, но все заняты или до них не дойти.
    Taken,
}

impl Desk {
    /// Тег для снимка; у открытых ворот его нет — пустая строка и есть «можно».
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Desk::Open(_) => "",
            Desk::Untaught => "untaught",
            Desk::Away => "away",
            Desk::Topped => "topped",
            Desk::Missing => "nodesk",
            Desk::Taken => "taken",
        }
    }
}

/// Порядок проверок значим и повторяет порядок отказов, который был у
/// `Sim::teach` россыпью: сперва про домен, потом про кота, потом про парты.
/// Обратный порядок сказал бы «парт нет» про домен, которому и не учат.
///
/// **«Парт нет» и «все заняты» — разные ответы**: первый чинится стройкой,
/// второй ожиданием, и слить их в одно «нельзя» значило бы отказ без причины.
pub(crate) fn desk_gate(
    map: &BaseMap,
    tiles: &TileRules,
    rules: &SkillRules,
    reach: &Reach,
    skills: Option<&Skills>,
    stats: Option<&Stats>,
    skill: usize,
    away: bool,
    taken: &[(i32, i32)],
) -> Desk {
    if rules.taught_cap(skill) <= 0 {
        return Desk::Untaught;
    }
    if away {
        return Desk::Away;
    }
    // Предел у каждого кота свой: парта доводит до `taught`, но не выше
    // врождённого (§12.42). Доученного она не берёт — отправленный за неё кот
    // встал бы с неё в тот же тик, а игрок прочёл бы это как поломку.
    if skills.map_or(0, |s| s.xp_of(skill)) >= desk_cap(rules, stats, skill) {
        return Desk::Topped;
    }
    if desk_cells(map, tiles, skill).next().is_none() {
        return Desk::Missing;
    }
    nearest_desk_at(map, tiles, reach, skill, taken).map_or(Desk::Taken, Desk::Open)
}

/// Сажает **приписанного** к парте кота, как только тот освободился (§12.84).
///
/// Раздачи здесь нет, как нет её и у `assign_relay`: система перебирает не
/// свободных котов, а приписки игрока (`Enrolled`). Никого не приписали — за
/// партами пусто, и это законное состояние.
///
/// **Стоит сразу за нуждами и экипировкой, до всех работ по базе.** Приписка,
/// уступающая чертежу, не значит ничего: работа на базе есть всегда, и
/// разбуженного ученика уводил бы первый же подвоз — та же причина, по которой
/// §12.34 поставила экипировку впереди стройки. Но нужды идут раньше: голодный
/// и раненый ученик сперва человек, а потом ученик.
///
/// **Доучившегося приписка отпускает здесь же.** Кот, которому парта больше
/// ничего не даёт, иначе ходил бы к ней вечно и вечно вставал бы с неё, — а
/// игрок читал бы это как кота, который «завис». Предел тот же двойной, что и
/// у самой учёбы (`desk_cap`): расходись они — приписка звала бы за парту,
/// с которой `study` поднимает в тот же тик.
///
/// Свободной парты может не быть: тогда кот просто работает и попробует
/// следующим тиком. Занятость парты держит `Study` ученика, как везде (§12.20),
/// поэтому отдельного реестра тут нет.
///
/// Ничью решает `id` кота, как во всех раздатчиках (инвариант 9): порядок
/// обхода сущностей ECS зависит от истории вставок и недетерминирован.
pub(crate) fn assign_study(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<SkillRules>,
    mut commands: Commands,
    students: Query<&Study>,
    mut records: Query<&mut Record>,
    free_cats: Query<
        (
            Entity,
            &UnitId,
            &Position,
            &Enrolled,
            Option<&Skills>,
            Option<&Stats>,
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
            Without<Squad>,
            Without<OnDuty>,
            Without<Away>,
            Without<Path>,
        ),
    >,
) {
    let mut taken: Vec<(i32, i32)> = students.iter().map(|s| s.spot).collect();

    let mut idle: Vec<(&str, Entity, (i32, i32), usize, i32, i32)> = free_cats
        .iter()
        .map(|(e, id, p, enrolled, skills, stats)| {
            let skill = enrolled.skill;
            (
                id.0.as_str(),
                e,
                (p.x, p.y),
                skill,
                skills.map_or(0, |s| s.xp_of(skill)),
                desk_cap(&rules, stats, skill),
            )
        })
        .collect();
    idle.sort_unstable_by_key(|&(id, ..)| id);

    for (_, cat_e, at, skill, xp, cap) in idle {
        if xp >= cap {
            // Вторая из двух непересекающихся веток исчерпания приписки: этого
            // кота от парты уже увели (сон, рана, приказ), `Study` с него снят,
            // а потолка он достиг. Забудь её — и отметка в личном деле не
            // встанет ровно у того, кого увели на последнем очке.
            if let Ok(mut record) = records.get_mut(cat_e) {
                record.note_schooled(skill);
            }
            commands.entity(cat_e).remove::<Enrolled>();
            continue;
        }
        let Some(spot) = nearest_desk(&map, &tiles, skill, at, &taken) else {
            continue; // парт нет, все заняты или не дойти — попробуем следующим тиком
        };
        taken.push(spot);
        let path = find_path(&map, &tiles, at, spot).unwrap_or_default();
        commands
            .entity(cat_e)
            .insert((Study { skill, spot }, Path { steps: path }));
    }
}

/// Учеников за партой — держит и доводит до порога.
///
/// Раздатчика у обучения нет: **оно адресно** (§12.18). Правило §12.16 «игрок
/// размечает работу, исполнителя берёт симуляция» не нарушается, а получает
/// вторую границу после вылазки: обучение — не работа над базой, а решение о
/// судьбе конкретного кота, тот же случай, что приказ «иди туда».
///
/// Поэтому система делает ровно две вещи: держит ученика идущим к парте,
/// переживая изменения карты (как `gather_squad` держит отряд), и вешает
/// `Worked`, пока тот сидит. Опыт начисляет `train_skills` в конце цепочки —
/// одно правило роста на работу и на учёбу.
///
/// Дойдя до `taught`, кот встаёт сам: парта — вход в домен, дальше только
/// практика, иначе она заменяет работу и «чем больше делает, тем лучше»
/// перестаёт что-либо значить.
pub(crate) fn study(
    map: Res<BaseMap>,
    tiles: Res<TileRules>,
    rules: Res<SkillRules>,
    mut commands: Commands,
    mut students: Query<(
        Entity,
        &Position,
        &mut Study,
        Option<&Path>,
        Option<&Skills>,
        Option<&Stats>,
        Option<&mut Record>,
    )>,
) {
    let taken: Vec<(i32, i32)> = students.iter().map(|(_, _, s, ..)| s.spot).collect();

    for (cat_e, pos, mut task, path, skills, stats, record) in &mut students {
        // Доучился: дальше парта не помогает, и держать за ней кота — значит
        // молча отнимать у базы работника. Предел здесь двойной: докуда доводит
        // парта (§12.18) и докуда пускает врождённый параметр (§12.42) —
        // тупому коту парта помогает меньше, а не дольше.
        //
        // Вместе с задачей снимается и приписка (§12.84): она значит «вернись за
        // парту, когда освободишься», а возвращаться уже незачем — иначе кот
        // ходил бы к ней вечно и вечно вставал бы с неё.
        if skills.map_or(0, |s| s.xp_of(task.skill)) >= desk_cap(&rules, stats, task.skill) {
            // Первая из двух веток исчерпания: досидел за партой сам. Отметка
            // идемпотентна, поэтому обе ветки безопасно зовут одно и то же.
            if let Some(mut record) = record {
                record.note_schooled(task.skill);
            }
            commands.entity(cat_e).remove::<(Study, Enrolled)>();
            continue;
        }

        // Парту могли снести, пока кот шёл: ищем другую свободную, а нет её —
        // учёба кончилась. Занятой считаем и свою, поэтому сравниваем с целью.
        if tiles.teaches_of(map.tile_at(task.spot.0, task.spot.1)) != Some(task.skill) {
            let others: Vec<(i32, i32)> =
                taken.iter().copied().filter(|&c| c != task.spot).collect();
            let Some(spot) = nearest_desk(&map, &tiles, task.skill, (pos.x, pos.y), &others) else {
                commands.entity(cat_e).remove::<(Study, Path, Stride)>();
                continue;
            };
            task.spot = spot;
            // Старый маршрут вёл к снесённой парте: оставить его — значит
            // сперва сходить туда, где учиться уже нечему.
            match find_path(&map, &tiles, (pos.x, pos.y), spot) {
                Some(steps) if !steps.is_empty() => {
                    commands.entity(cat_e).insert(Path { steps });
                }
                _ => {
                    commands.entity(cat_e).remove::<(Path, Stride)>();
                }
            }
            continue;
        }

        if path.is_some() {
            continue; // ещё идёт
        }
        if (pos.x, pos.y) == task.spot {
            commands.entity(cat_e).insert(Worked(task.skill));
        } else if let Some(steps) = find_path(&map, &tiles, (pos.x, pos.y), task.spot) {
            // Маршрут оборвался — кота выбросило из ямы или парту перенесли.
            commands.entity(cat_e).insert(Path { steps });
        }
    }
}

/// Превращает маркеры «работал в этом тике» в опыт и снимает их.
///
/// Стоит в конце цепочки, а не сразу за `work_jobs`: маркер успевает поставить
/// любая система работы, в том числе будущая.
pub(crate) fn train_skills(
    rules: Res<SkillRules>,
    time: Res<SimTime>,
    mut commands: Commands,
    mut cats: Query<(Entity, &Worked, Option<&mut Skills>, Option<&Stats>)>,
) {
    for (cat_e, worked, skills, stats) in &mut cats {
        // След маркера, переживающий тик: `Worked` снимается здесь же, а панель
        // кота собирается уже после цепочки и без следа не знала бы, какой из
        // доменов показывать (§12.17). Пишется он тут, потому что тут же
        // единственное место, где работа превращается в опыт.
        commands.entity(cat_e).insert(Trained {
            skill: worked.0,
            at: time.tick,
        });
        // Нет порогов — расти нечему: домена нет в рулсете либо он без уровней.
        // Потолок у каждого кота свой: врождённый параметр режет опыт, а не
        // показанный уровень (§12.42), — иначе кот копил бы очки, которые
        // никогда ни во что не превратятся.
        let cap = xp_ceiling(&rules, stats, worked.0);
        match skills {
            Some(mut skills) => skills.add_xp(worked.0, XP_PER_TICK, cap),
            None => {
                let mut fresh = Skills::default();
                fresh.add_xp(worked.0, XP_PER_TICK, cap);
                commands.entity(cat_e).insert(fresh);
            }
        }
        commands.entity(cat_e).remove::<Worked>();
    }
}
