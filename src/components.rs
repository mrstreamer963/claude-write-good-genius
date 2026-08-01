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

/// Кот учится у парты: стоит и тикает, а на выходе не построенная клетка, а
/// опыт в навыке (§12.18). **Шестая задача общего слоя** — как и пять
/// остальных, обязана попасть во все фильтры «кот свободен».
#[derive(Component)]
pub(crate) struct Study {
    /// Домен, которому кот учится: он же тот, которому учит парта.
    pub(crate) skill: usize,
    /// Занятая парта: клетка, к которой кот идёт или у которой уже сидит.
    /// Занятость живёт здесь, а не отдельным claim, — ровно как у лежанки
    /// (§12.20): снимается вместе с задачей, чем бы её ни сняли.
    pub(crate) spot: (i32, i32),
}

/// Тема исследования в работе. Это **разметка работы**, как чертёж: игрок
/// выбирает, что изучать, а исполнителя берёт симуляция — но только из тех, у
/// кого хватает «Науки» (§12.16, §12.26).
#[derive(Component)]
pub(crate) struct Research {
    /// Индекс записи в `research:` рулсета.
    pub(crate) def: usize,
    /// Очки работы, как у чертежа: скорость даёт навык исполнителя (§12.17).
    pub(crate) progress: i32,
    pub(crate) assignee: Option<Entity>,
    /// Клетка лаборатории, за которой идёт работа; `None` — исполнителя ещё нет.
    pub(crate) spot: Option<(i32, i32)>,
}

/// Кот занят темой (ссылка на сущность-тему) — **седьмая задача общего слоя**.
#[derive(Component)]
pub(crate) struct Researching(pub(crate) Entity);

/// Заказ в мастерской. Это **разметка работы**, как чертёж и как тема, но
/// первая **повторяемая**: рецепт крутится, пока не сделано `left` штук
/// (§12.30). Очереди заказов нет — счётчик и есть весь ответ на «сколько раз».
#[derive(Component)]
pub(crate) struct Craft {
    /// Индекс записи в `recipes:` рулсета.
    pub(crate) def: usize,
    /// Сколько штук осталось сделать, включая ту, что в работе.
    pub(crate) left: i32,
    /// Очки работы **текущей** штуки: скорость даёт навык исполнителя (§12.17).
    pub(crate) progress: i32,
    /// Материал текущей штуки уже списан со склада. Платят за штуку и в момент,
    /// когда за неё берутся, — заказ, оплаченный вперёд, заморозил бы склад под
    /// работу, которая начнётся через полтысячи тиков (§12.30).
    pub(crate) paid: bool,
    pub(crate) assignee: Option<Entity>,
    /// Клетка мастерской, за которой идёт работа; `None` — исполнителя ещё нет.
    pub(crate) spot: Option<(i32, i32)>,
}

/// Кот занят заказом (ссылка на сущность-заказ) — **восьмая задача общего слоя**.
#[derive(Component)]
pub(crate) struct Crafting(pub(crate) Entity);

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

/// Что на коте надето: индексы предметов палитры, в порядке шаблона (§12.29).
///
/// Не инвентарь: предмет надет либо нет, класть в него нечего (§12.21). Компонента
/// нет — кот не экипирован вовсе, и это нормальное состояние, а не недостача.
#[derive(Component)]
pub(crate) struct Gear(pub(crate) Vec<usize>);

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
    /// До какого уровня доводит парта. Ноль — домен не преподаётся вовсе: так
    /// живёт «Стройка», которой учиться незачем (§12.18).
    pub(crate) taught: i32,
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

    /// Опыт, на котором парта останавливается: порог уровня `taught`.
    ///
    /// Ноль — домену не учат (парта не поможет), и это не то же самое, что
    /// потолок навыка: обучение — вход в домен, мастерство — из домена
    /// (§12.18). Уровень выше длины списка порогов упирается в последний.
    pub(crate) fn taught_cap(&self, skill: usize) -> i32 {
        let Some(rule) = self.0.get(skill).filter(|r| r.taught > 0) else {
            return 0;
        };
        let last = rule.levels.len().saturating_sub(1);
        rule.levels
            .get((rule.taught as usize - 1).min(last))
            .copied()
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
    /// Парта: какому домену учит эта клетка (индекс в `skills:`). `None` —
    /// не учит ничему. Пятый случай той же схемы (§12.18).
    pub(crate) teaches: Option<usize>,
    /// Лаборатория: здесь идёт работа над темой. Шестой случай той же схемы —
    /// у комнаты появляется смысл сверх цвета, а карта остаётся одним слоем.
    pub(crate) lab: bool,
    /// Мастерская: здесь идёт работа над заказом. Седьмой случай той же схемы
    /// (§12.30).
    pub(crate) shop: bool,
    /// Технология, открывающая постройку; пусто — доступен сразу (§12.27).
    pub(crate) tech: String,
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

    /// Какому домену учит клетка; `None` — обычный пол.
    pub(crate) fn teaches_of(&self, tile: i16) -> Option<usize> {
        self.of(tile).and_then(|r| r.teaches)
    }

    pub(crate) fn is_lab(&self, tile: i16) -> bool {
        self.of(tile).is_some_and(|r| r.lab)
    }

    pub(crate) fn is_shop(&self, tile: i16) -> bool {
        self.of(tile).is_some_and(|r| r.shop)
    }

    /// Технология, без которой тайл не построить; `None` — доступен сразу.
    /// У сноса (`tile < 0`) ворот нет: разбирать можно что угодно и всегда.
    pub(crate) fn tech_of(&self, tile: i16) -> Option<&str> {
        self.of(tile)
            .map(|r| r.tech.as_str())
            .filter(|t| !t.is_empty())
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

/// Свойства предметов по индексу палитры `items:`. Ресурс той же природы, что и
/// `TileRules`: предмет значит что-то сверх цвета, потому что у него есть
/// свойство, — а не потому, что код знает его по имени (§12.29).
#[derive(Resource, Default)]
pub(crate) struct ItemRules(pub(crate) Vec<ItemRule>);

#[derive(Default, Clone)]
pub(crate) struct ItemRule {
    /// Сколько силы предмет добавляет отряду, если надет. Ноль — обычный
    /// материал: надеть его нельзя, и в шаблон он не попадает.
    pub(crate) force: i32,
}

impl ItemRules {
    /// Сила предмета; неизвестный индекс — ноль, как у тайла вне палитры.
    pub(crate) fn force_of(&self, item: usize) -> i32 {
        self.0.get(item).map_or(0, |r| r.force)
    }

    /// Сила всего надетого на коте. Компонента нет — кот не экипирован, и это
    /// ноль, а не отсутствие данных.
    pub(crate) fn force_of_gear(&self, gear: Option<&Gear>) -> i32 {
        gear.map_or(0, |g| g.0.iter().map(|&i| self.force_of(i)).sum())
    }
}

/// Шаблон снаряжения: какие предметы кот носит (§4.2, §12.29). Один на всех и
/// задан рулсетом — редактор шаблонов это интерфейс поверх выбора, которого на
/// POC нет. Пустой ресурс = снаряжения в мире нет: так живут тесты чужих механик.
#[derive(Resource, Default)]
pub(crate) struct LoadoutRules(pub(crate) Vec<usize>);

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
    /// Известности за полный успех; доля добычи — та же доля известности.
    pub(crate) fame: i32,
    /// Порог известности, ниже которого за вылазку не берутся (§12.24).
    pub(crate) requires: i32,
    /// Добыча набором `(предмет, сколько)`, упорядоченным по имени предмета —
    /// как цена тайла, и по той же причине: порядок детерминирован (§12.21).
    pub(crate) loot: Vec<(usize, i32)>,
}

/// Известность базы: копится от вылазок и открывает доступ к следующим.
///
/// **Только растёт и только открывает** (§12.24). Не валюта: платят предметами.
/// Ворота, которые могут закрыться, читаются как поломка — игрок не поймёт,
/// почему доступная вчера вылазка сегодня исчезла.
#[derive(Resource, Default)]
pub(crate) struct Fame(pub(crate) i32);

/// Темы исследования из рулсета по индексу записи. Пустой ресурс = изучать
/// нечего: так живут тесты чужих механик.
#[derive(Resource, Default)]
pub(crate) struct ResearchRules(pub(crate) Vec<ResearchRule>);

#[derive(Default, Clone)]
pub(crate) struct ResearchRule {
    /// Он же id технологии на выходе: у темы и результата одно имя, второе
    /// значило бы два списка, которые однажды разойдутся.
    pub(crate) id: String,
    /// Уровень «Науки», ниже которого кот за тему не берётся вовсе. Это
    /// **допуск**, а не скорость (§12.18): без навыка нельзя, а не медленно.
    pub(crate) level: i32,
    /// Очки работы, как у чертежа: скорость даёт навык исполнителя (§12.17).
    pub(crate) work: i32,
    /// Чем платим — набор `(предмет, сколько)` со склада, как у найма (§12.24).
    pub(crate) cost: Vec<(usize, i32)>,
    /// Технологии, без которых темы не существует: дерево в §4.3.
    pub(crate) requires: Vec<String>,
}

/// Рецепты из рулсета по индексу записи. Пустой ресурс = производства в мире
/// нет: так живут тесты чужих механик.
#[derive(Resource, Default)]
pub(crate) struct CraftRules(pub(crate) Vec<CraftRule>);

#[derive(Default, Clone)]
pub(crate) struct CraftRule {
    /// Очки работы на одну штуку, как у чертежа (§12.17).
    pub(crate) work: i32,
    /// Чем платим за штуку — набор `(предмет, сколько)` со склада, упорядоченный
    /// по имени предмета, как цена тайла (§12.21).
    pub(crate) cost: Vec<(usize, i32)>,
    /// Что выходит: набор `(предмет, сколько)`. Ложится кучей под ноги мастеру,
    /// как добыча на шлюз, — дальше её разносит обычная уборка (§12.30).
    pub(crate) gives: Vec<(usize, i32)>,
    /// Технологии, без которых рецепта не существует: ворота те же, что у тайла
    /// (§12.27). Допуска по навыку у производства нет — навык только ускоряет.
    pub(crate) requires: Vec<String>,
}

/// Изученные технологии. **Только растёт и только открывает** — та же шкала-
/// ворота, что и известность (§12.24): технология, которая может пропасть,
/// закрыла бы уже доступное, и игрок прочтёт это как баг.
#[derive(Resource, Default)]
pub(crate) struct Techs(pub(crate) Vec<String>);

impl Techs {
    pub(crate) fn knows(&self, tech: &str) -> bool {
        self.0.iter().any(|t| t == tech)
    }

    /// Известны ли все технологии набора; пустой набор известен всегда.
    pub(crate) fn covers(&self, techs: &[String]) -> bool {
        techs.iter().all(|t| self.knows(t))
    }
}

/// События таймлайна по индексу записи. Пустой ресурс = мир ничего не делает
/// сам: так живут тесты чужих механик.
#[derive(Resource, Default)]
pub(crate) struct TimelineRules(pub(crate) Vec<EventRule>);

/// Имени у правила нет: событие адресуется индексом записи, как миссия и тема,
/// а `id` из рулсета — это ключ контента, и второе его хранилище было бы
/// ещё одним местом, где он может разойтись.
#[derive(Default, Clone)]
pub(crate) struct EventRule {
    /// Тик, на котором событие происходит: дата — это номер тика (§12.28).
    pub(crate) at: u64,
    /// За сколько тиков до срока проступают детали («туман предзнания», §4.6).
    pub(crate) reveal: u64,
    /// Технологии, к которым база должна успеть; пусто — событие безусловно.
    pub(crate) requires: Vec<String>,
    /// Что ляжет кучей на шлюз, если база готова.
    pub(crate) gift: Vec<(usize, i32)>,
    pub(crate) fame: i32,
    /// Чего стоит неготовность — бодрости каждому коту на базе.
    pub(crate) toll: i32,
}

/// Что уже случилось: журнал сработавших событий, в порядке дат.
///
/// **Только растёт**, как известность и технологии (§12.24, §12.27): прошедшее
/// событие не отменяется. Он же признак «сработало» — второго списка не
/// заводим, иначе однажды разойдутся.
#[derive(Resource, Default)]
pub(crate) struct Chronicle(pub(crate) Vec<Happened>);

#[derive(Clone, Copy)]
pub(crate) struct Happened {
    /// Индекс записи в `timeline:`.
    pub(crate) def: usize,
    /// Была ли база готова — этим и различаются два исхода.
    pub(crate) ready: bool,
}

impl Chronicle {
    pub(crate) fn happened(&self, def: usize) -> Option<Happened> {
        self.0.iter().copied().find(|h| h.def == def)
    }
}

/// Кандидаты на найм по индексу записи в рулсете. Пустой ресурс = нанимать
/// некого: так живут тесты чужих механик.
#[derive(Resource, Default)]
pub(crate) struct RecruitRules(pub(crate) Vec<RecruitRule>);

#[derive(Default, Clone)]
pub(crate) struct RecruitRule {
    /// Он же `UnitId` будущего кота: нанять дважды одного и того же нельзя,
    /// и проверяется это по самому коту, а не по отдельному списку нанятых.
    pub(crate) id: String,
    pub(crate) sprite: String,
    pub(crate) requires: i32,
    /// Чем платим — набор `(предмет, сколько)`, упорядоченный по имени
    /// предмета, как цена тайла и добыча (§12.21).
    pub(crate) cost: Vec<(usize, i32)>,
    /// Стартовый опыт: `(домен, очки)`.
    pub(crate) skills: Vec<(usize, i32)>,
    pub(crate) perks: Vec<String>,
}

/// Свойства кота, общие для всех и заданные рулсетом.
///
/// Ресурс, а не разовое чтение при старте: новичок с найма собирается по тем же
/// правилам, что и стартовая тройка, и второе место для этих чисел означало бы
/// кота, у которого лапы другого размера (§12.24).
#[derive(Resource, Default)]
pub(crate) struct UnitRules {
    /// Базовый объём лап; перк «Носильщик» удваивает. Ноль = предела нет.
    pub(crate) carry: i32,
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
