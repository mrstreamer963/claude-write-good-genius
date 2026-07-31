//! Компоненты ECS и ресурсы мира.
//!
//! Данные без поведения: логика живёт в системах (`schedule`, `jobs`,
//! `movement`, `hauling`). Новая фича = новые компоненты здесь + отдельная
//! система там.

use bevy_ecs::prelude::*;

#[derive(Component)]
pub(crate) struct Position {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Component)]
pub(crate) struct Renderable {
    pub(crate) sprite: String,
}

#[derive(Component)]
pub(crate) struct UnitId(pub(crate) String);

/// Оставшийся маршрут. Хранится «развёрнуто»: следующий шаг — последний элемент
/// (`pop()` = следующая клетка), первый элемент — конечная цель.
#[derive(Component)]
pub(crate) struct Path {
    pub(crate) steps: Vec<(i32, i32)>,
}

/// Обратный отсчёт тиков до следующего шага.
#[derive(Component)]
pub(crate) struct MoveCooldown(pub(crate) u8);

/// Кот назначен на джоб постройки (ссылка на сущность-чертёж).
#[derive(Component)]
pub(crate) struct Assignment(pub(crate) Entity);

/// Приказ игрока «иди туда». Хранится, даже если путь сейчас не найден:
/// `retry_orders` перепроложит маршрут, как только карта изменится —
/// например, после постройки коридора.
#[derive(Component)]
pub(crate) struct Order {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Версия карты, на которой последний раз пытались проложить маршрут.
    /// Проходимость зависит только от тайлов (коты друг друга не блокируют),
    /// поэтому повтор на неизменившейся карте заведомо даст тот же результат —
    /// и пропускается, чтобы не гонять BFS каждый тик.
    pub(crate) tried_version: u64,
}

/// Чертёж — запланированная постройка тайла.
#[derive(Component)]
pub(crate) struct Blueprint {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) tile: i16,
    pub(crate) progress: i32,
    pub(crate) assignee: Option<Entity>,
    /// Что уже завезли на площадку: `(предмет, сколько)`. Пока набор не покрыл
    /// цену тайла, чертёж строителю не раздаётся — сперва носильщик (§12.15).
    pub(crate) delivered: Vec<(usize, i32)>,
    /// Носильщик, который сейчас везёт сюда лом. Один за раз: цена тайла мала,
    /// а резервировать кучи дороже, чем изредка сходить впустую.
    pub(crate) hauler: Option<Entity>,
}

/// Куча предметов на клетке пола. Тип живёт в самой куче, поэтому на одной
/// клетке спокойно лежат разные (§12.21); сливаются только одинаковые.
#[derive(Component)]
pub(crate) struct Stack {
    pub(crate) item: usize,
    pub(crate) count: i32,
}

/// Груз в лапах: **один тип за ходку** (§12.21) — иначе лапы превращаются в
/// инвентарь, а раздатчик в планировщик маршрутов. Груз сам не падает: кот
/// держит его, пока не донесёт, и потому первым берётся за следующую доставку.
#[derive(Component)]
pub(crate) struct Carrying {
    pub(crate) item: usize,
    pub(crate) count: i32,
}

/// Пометка «эту кучу — на склад»; она же задача уборки. Без пометки куча просто
/// лежит. Вешает её автоуборка или рамка игрока (§12.16).
#[derive(Component, Default)]
pub(crate) struct ToStore {
    /// Кот, который уже идёт за этой кучей. Без claim каждый освободившийся кот
    /// шёл бы к одной и той же ближайшей куче и заставал пустое место.
    pub(crate) hauler: Option<Entity>,
}

/// Куда кот несёт груз.
#[derive(Clone, Copy)]
pub(crate) enum HaulTo {
    /// Площадка: лом уходит в `Blueprint::delivered`.
    Site(Entity),
    /// Склад: конкретная клетка выбирается по дороге, а не при назначении —
    /// пока кот идёт, склад может заполниться. Внутри — куча-источник, чтобы
    /// снять с неё claim, если довезти не вышло; `None` у кота, которого
    /// отправили на склад уже с грузом.
    Store(Option<Entity>),
}

/// Задача переноса. Фаза отдельно не хранится — её задаёт наличие `Carrying`:
/// без груза кот идёт к куче, с грузом — к адресату.
#[derive(Component)]
pub(crate) struct Haul(pub(crate) HaulTo);

/// Сколько лома кот берёт за одну ходку. Компонента нет — предела нет: так
/// живут тесты механик, которым до объёма лап дела нет (§12.17).
#[derive(Component)]
pub(crate) struct Carry(pub(crate) i32);

/// Бодрость: тратится по очку за тик бодрствования, восстанавливается сном.
/// Компонента нет — кот не устаёт вовсе (§12.20).
#[derive(Component)]
pub(crate) struct Energy(pub(crate) i32);

/// Кот отдыхает. Фаза отдельно не хранится, её задаёт наличие маршрута: с
/// `Path` кот идёт к лежанке, без — уже спит (так же устроен `Haul`, §12.15).
///
/// Это третья задача общего слоя, поэтому `Without<Rest>` обязан стоять всюду,
/// где кот считается свободным, — иначе спящего тихо перехватит работа.
#[derive(Component)]
pub(crate) struct Rest {
    /// Занятая лежанка: клетка, к которой кот идёт или на которой уже спит.
    /// `None` — кот свалился от истощения где стоял, места он не занимает
    /// (кроме случая, когда упал прямо на лежанку — это видно по позиции).
    ///
    /// Занятость лежанки живёт здесь, а не отдельным claim: она снимается
    /// вместе с задачей — просыпанием, приказом игрока, чем угодно (§12.20).
    pub(crate) spot: Option<(i32, i32)>,
}

/// Миссия — вылазка за пределы базы. Это **разметка работы**, как чертёж: игрок
/// говорит «идём туда», а исполнителей набирает симуляция (§12.16, §12.22).
#[derive(Component)]
pub(crate) struct Mission {
    /// Индекс записи в `missions:` рулсета.
    pub(crate) def: usize,
    /// Клетка шлюза: там собирается отряд и туда же он вернётся с добычей.
    /// `None` — набор ещё не начат и шлюз не выбран.
    pub(crate) gate: Option<(i32, i32)>,
    /// Тиков до возвращения. Тикает только у ушедшего отряда: сбор длится
    /// столько, сколько коты идут к шлюзу, и в срок миссии не входит.
    pub(crate) left: i32,
}

/// Кот в отряде (ссылка на сущность-миссию) — **пятая задача общего слоя**.
/// Как и остальные четыре, обязана попасть во все фильтры «кот свободен»,
/// иначе раздатчик тихо уведёт бойца из отряда (инвариант занятости).
#[derive(Component)]
pub(crate) struct Squad(pub(crate) Entity);

/// Кот ушёл с базы. Позиция остаётся последней (шлюз), но кота нет ни в
/// рендере, ни в системах базы: он не ходит, не устаёт и не выбирается из ям —
/// пока он вне базы, ими занимается авторасчёт миссии, а не симуляция (§12.22).
#[derive(Component)]
pub(crate) struct Away;

/// Перки — статичные теги из рулсета. В отличие от навыка, не растут: это
/// сложение кота, а не его мастерство (§12.17).
#[derive(Component, Default)]
pub(crate) struct Perks(pub(crate) Vec<String>);

/// Перк «Носильщик»: удваивает объём лап. Единственный перк POC.
pub(crate) const PERK_HAULER: &str = "hauler";

/// Опыт кота по доменам работы; индекс — номер записи в `skills:` рулсета.
/// Уровень не хранится: он выводится из опыта по порогам (`SkillRules`), и
/// второе место, где его можно забыть обновить, не заводится.
#[derive(Component, Default)]
pub(crate) struct Skills {
    pub(crate) xp: Vec<i32>,
}

impl Skills {
    pub(crate) fn xp_of(&self, skill: usize) -> i32 {
        self.xp.get(skill).copied().unwrap_or(0)
    }

    pub(crate) fn add_xp(&mut self, skill: usize, amount: i32, cap: i32) {
        if self.xp.len() <= skill {
            self.xp.resize(skill + 1, 0);
        }
        self.xp[skill] = (self.xp[skill] + amount).min(cap);
    }
}

/// «Кот работал в этом тике по такому-то домену». Ставит система работы, снимает
/// `train_skills`: правило роста живёт в одном месте и не копируется в каждую
/// новую работу (§12.17).
#[derive(Component)]
pub(crate) struct Worked(pub(crate) usize);

#[derive(Resource)]
pub(crate) struct SimTime {
    pub(crate) tick: u64,
}

/// Убирать ли лом с пола без приказа. Выключатель для случая, когда лом нарочно
/// оставлен у стройки (§12.16).
#[derive(Resource)]
pub(crate) struct AutoTidy(pub(crate) bool);

/// Пороги уровней по индексу домена. Пустой ресурс = навыков в мире нет, и
/// работа идёт с базовой скоростью: тесты чужих механик о навыках не знают.
#[derive(Resource, Default)]
pub(crate) struct SkillRules(pub(crate) Vec<SkillRule>);

#[derive(Default, Clone)]
pub(crate) struct SkillRule {
    pub(crate) id: String,
    pub(crate) levels: Vec<i32>,
}

impl SkillRules {
    /// Индекс домена по имени. Систем работы будет больше одной, и каждая знает
    /// имя своего домена, а не его номер: порядок записей — дело рулсета.
    pub(crate) fn index_of(&self, id: &str) -> Option<usize> {
        self.0.iter().position(|s| s.id == id)
    }

    /// Уровень навыка — сколько порогов пройдено. Потолок = число порогов.
    pub(crate) fn level(&self, skill: usize, xp: i32) -> i32 {
        self.0
            .get(skill)
            .map_or(0, |s| s.levels.iter().filter(|&&t| xp >= t).count() as i32)
    }

    /// Порог следующего уровня; ноль — навык на потолке. Для полоски в UI.
    pub(crate) fn next_threshold(&self, skill: usize, xp: i32) -> i32 {
        self.0
            .get(skill)
            .and_then(|s| s.levels.iter().find(|&&t| xp < t).copied())
            .unwrap_or(0)
    }

    /// Опыт, выше которого копить незачем: последний порог.
    pub(crate) fn xp_cap(&self, skill: usize) -> i32 {
        self.0
            .get(skill)
            .and_then(|s| s.levels.last().copied())
            .unwrap_or(0)
    }
}

/// Свойства тайлов по индексу палитры. Ресурс, а не константы: и цена, и
/// ёмкость — контент из рулсета, как и сама палитра (§11).
#[derive(Resource, Default)]
pub(crate) struct TileRules(pub(crate) Vec<TileRule>);

#[derive(Default, Clone)]
pub(crate) struct TileRule {
    /// Цена постройки набором `(предмет, сколько)`, упорядоченным по имени
    /// предмета в рулсете: порядок обхода детерминирован (§11, §12.21).
    pub(crate) cost: Vec<(usize, i32)>,
    pub(crate) capacity: i32,
    pub(crate) rest: i32,
    /// Шлюз: отсюда отряд уходит на миссию и сюда же возвращается. Четвёртый
    /// случай той же схемы после `capacity`, `teaches` и `rest` — комната
    /// получает смысл сверх цвета в палитре, а не новый слой карты (§12.22).
    pub(crate) gate: bool,
}

impl TileRules {
    /// Свойства тайла. У пустоты (`< 0`) и неизвестного индекса всё по нулям:
    /// снос сам по себе материала не требует, он его возвращает.
    fn of(&self, tile: i16) -> Option<&TileRule> {
        (tile >= 0).then(|| self.0.get(tile as usize))?
    }

    /// Из чего строится тайл; пусто — бесплатно.
    pub(crate) fn cost_of(&self, tile: i16) -> &[(usize, i32)] {
        self.of(tile).map_or(&[], |r| &r.cost)
    }

    pub(crate) fn capacity_of(&self, tile: i16) -> i32 {
        self.of(tile).map_or(0, |r| r.capacity)
    }

    pub(crate) fn rest_of(&self, tile: i16) -> i32 {
        self.of(tile).map_or(0, |r| r.rest)
    }

    pub(crate) fn is_gate(&self, tile: i16) -> bool {
        self.of(tile).is_some_and(|r| r.gate)
    }
}

/// Чего площадке ещё не хватает: цена тайла минус завезённое, только недостача.
pub(crate) fn missing(rules: &TileRules, bp: &Blueprint) -> Vec<(usize, i32)> {
    rules
        .cost_of(bp.tile)
        .iter()
        .filter_map(|&(item, need)| {
            let have = delivered_of(&bp.delivered, item);
            (need > have).then_some((item, need - have))
        })
        .collect()
}

pub(crate) fn delivered_of(delivered: &[(usize, i32)], item: usize) -> i32 {
    delivered
        .iter()
        .find(|&&(i, _)| i == item)
        .map_or(0, |&(_, n)| n)
}

/// Записать доставку в набор площадки.
pub(crate) fn add_delivered(delivered: &mut Vec<(usize, i32)>, item: usize, count: i32) {
    match delivered.iter_mut().find(|(i, _)| *i == item) {
        Some((_, have)) => *have += count,
        None => delivered.push((item, count)),
    }
}

/// Миссии из рулсета по индексу записи. Пустой ресурс = вылазок в мире нет:
/// так живут тесты чужих механик, как и с пустыми `SkillRules`/`NeedRules`.
#[derive(Resource, Default)]
pub(crate) struct MissionRules(pub(crate) Vec<MissionRule>);

#[derive(Default, Clone)]
pub(crate) struct MissionRule {
    /// Сколько котов уходит. Ровно столько игрок и обязан выбрать (§12.23).
    pub(crate) squad: usize,
    /// Сколько тиков отряд отсутствует. Отсчёт идёт с ухода, а не с приказа.
    pub(crate) ticks: i32,
    /// Сложность: с ней сравнивается сила отряда (§12.23). Ноль = удаётся всегда.
    pub(crate) danger: i32,
    /// Во сколько бодрости вылазка обходится каждому коту (провал — во всю).
    pub(crate) toll: i32,
    /// Добыча набором `(предмет, сколько)`, упорядоченным по имени предмета —
    /// как цена тайла, и по той же причине: порядок детерминирован (§12.21).
    pub(crate) loot: Vec<(usize, i32)>,
}

/// Правила усталости из рулсета. `max = 0` — механика выключена целиком.
#[derive(Resource, Default)]
pub(crate) struct NeedRules {
    pub(crate) max: i32,
    /// Порог, ниже которого свободный кот уходит спать сам.
    pub(crate) tired: i32,
    /// Восстановление вне лежанки: спать можно где угодно, просто дольше.
    pub(crate) floor: i32,
}
