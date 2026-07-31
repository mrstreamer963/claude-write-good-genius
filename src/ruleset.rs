//! Рулсет (YAML) — data-driven контент в стиле OpenXcom Extended.
//!
//! Порядок записей `tiles:` — это индекс палитры: он уходит в рендер, приходит
//! обратно в командах постройки и лежит в клетках карты. Переставить записи в
//! рулсете = переназначить смысл всех уже записанных индексов.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct Ruleset {
    pub(crate) grid: GridDef,
    #[serde(default)]
    pub(crate) items: Vec<ItemDef>,
    #[serde(default)]
    pub(crate) tiles: Vec<TileDef>,
    #[serde(default)]
    pub(crate) build: Vec<BuildRect>,
    #[serde(default)]
    pub(crate) stock: Vec<StockDef>,
    #[serde(default)]
    pub(crate) units: Vec<UnitDef>,
    #[serde(default)]
    pub(crate) skills: Vec<SkillDef>,
    #[serde(default)]
    pub(crate) perks: Vec<PerkDef>,
    #[serde(default)]
    pub(crate) missions: Vec<MissionDef>,
    #[serde(default)]
    pub(crate) recruits: Vec<RecruitDef>,
    /// Сколько лома кот уносит за одну ходку. Ноль = без предела (правило
    /// выключено), как `cost: 0` у тайла. Перк «Носильщик» удваивает (§12.17).
    #[serde(default)]
    pub(crate) carry: i32,
    #[serde(default)]
    pub(crate) energy: EnergyDef,
}

/// Усталость (§12.20). `max: 0` — механика выключена: бодрости у котов нет,
/// спать они не ходят. Так живут тесты чужих механик.
#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct EnergyDef {
    /// Полная бодрость; тратится по очку за тик бодрствования.
    #[serde(default)]
    pub(crate) max: i32,
    /// Ниже этого свободный кот идёт спать сам.
    #[serde(default)]
    pub(crate) tired: i32,
    /// Восстановление там, где лежанки нет, — цена базы без зоны отдыха.
    #[serde(default)]
    pub(crate) floor: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GridDef {
    pub(crate) width: i32,
    pub(crate) height: i32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct TileDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    pub(crate) color: String,
    /// Что нужно завезти, чтобы возвести тайл: `{ предмет: сколько }`; снос
    /// возвращает то же самое (§12.15, §12.21). Пусто = бесплатно.
    ///
    /// `BTreeMap`, а не `HashMap`: порядок обхода цены доходит до раздачи задач,
    /// а недетерминизм ломает и тесты, и модель времени (§11).
    #[serde(default)]
    pub(crate) cost: BTreeMap<String, i32>,
    /// Сколько лома клетка хранит. Больше нуля = это склад: коты сами свозят
    /// сюда всё, что валяется на полу (§12.16). Ноль = обычный пол.
    #[serde(default)]
    pub(crate) capacity: i32,
    /// Сколько бодрости кот восстанавливает здесь за тик. Больше нуля = лежанка,
    /// уставшие коты идут спать сюда (§12.20). Ноль = спать можно, но медленно.
    #[serde(default)]
    pub(crate) rest: i32,
    /// Шлюз: отсюда отряд уходит на вылазку и сюда же возвращается с добычей
    /// (§12.22). Четвёртое свойство той же схемы после `cost`/`capacity`/`rest`:
    /// комната значит что-то сверх цвета, а карта остаётся одним слоем.
    #[serde(default)]
    pub(crate) gate: bool,
    /// Парта: `id` навыка, которому учит клетка (§12.18). Пусто — не учит.
    /// Пятое свойство той же схемы: обучение — это работа, у которой есть
    /// место, а не отдельный экран.
    #[serde(default)]
    pub(crate) teaches: String,
}

/// Миссия — вылазка за пределы базы. Порядок записей `missions:` — индекс,
/// который уходит в интерфейс и приходит обратно в команде запуска.
///
/// Кого послать, рулсет не описывает — это выбор игрока (§12.23).
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct MissionDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    /// Сколько котов уходит; ровно столько игрок и обязан выбрать.
    pub(crate) squad: usize,
    /// Сколько тиков длится вылазка. Отсчёт идёт с ухода отряда, а не с приказа:
    /// сбор — это уже видимая игроку беготня, и считать её дважды незачем.
    pub(crate) ticks: i32,
    /// Сложность. Сравнивается с силой отряда (по коту — 1 плюс его уровень
    /// «Вылазки»): хватило — вся добыча, не хватило — доля, вдвое меньше
    /// нужного — провал (§12.23). Ноль = вылазка удаётся всегда.
    #[serde(default)]
    pub(crate) danger: i32,
    /// Во сколько бодрости обходится вылазка каждому коту. Списывается разом
    /// при возвращении: сама вылазка не симулируется, она считается (§12.22).
    /// Провал забирает всю бодрость — коты падают у шлюза.
    #[serde(default)]
    pub(crate) toll: i32,
    /// Добыча при полном успехе: `{ предмет: сколько }`. Частичный успех даёт
    /// свою долю от каждой позиции. `BTreeMap` по той же причине, что и у цены
    /// тайла, — порядок обхода должен быть детерминирован (§12.21).
    #[serde(default)]
    pub(crate) loot: BTreeMap<String, i32>,
    /// Сколько известности приносит полный успех; доля добычи — та же доля
    /// известности. Провал не приносит ничего, но и не отнимает (§12.24).
    #[serde(default)]
    pub(crate) fame: i32,
    /// Сколько известности нужно, чтобы взяться. Ворота, а не подсказка:
    /// заявку ниже порога ядро отклоняет.
    #[serde(default)]
    pub(crate) requires: i32,
}

/// Кандидат на найм. Коты уникальны (§4.2), поэтому это не «юнит такого-то
/// типа», а конкретный кот со своей биографией: нанимается один раз и приходит
/// уже умеющим — ровно тот «навык из найма», который предсказал §12.18.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct RecruitDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    pub(crate) sprite: String,
    /// Сколько известности нужно, чтобы он вообще откликнулся.
    #[serde(default)]
    pub(crate) requires: i32,
    /// Чем платим: `{ предмет: сколько }` со склада. Известность не тратится —
    /// она открывает, а платят предметами (§12.24).
    #[serde(default)]
    pub(crate) cost: BTreeMap<String, i32>,
    /// С каким опытом приходит: `{ навык: очки }`. Уровень выводится из опыта
    /// по порогам, второго места для него не заводится (§12.17).
    #[serde(default)]
    pub(crate) skills: BTreeMap<String, i32>,
    #[serde(default)]
    pub(crate) perks: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BuildRect {
    pub(crate) tile: String,
    /// [x, y, w, h] в тайлах
    pub(crate) rect: [i32; 4],
}

/// Стартовая куча на клетке пола.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct StockDef {
    /// [x, y] в тайлах
    pub(crate) at: [i32; 2],
    pub(crate) item: String,
    pub(crate) count: i32,
}

/// Предмет. Порядок записей `items:` — индекс, который лежит в кучах, в лапах
/// и в ценах тайлов и уходит в снапшот (§12.21).
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct ItemDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    pub(crate) color: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct UnitDef {
    pub(crate) id: String,
    pub(crate) sprite: String,
    /// [x, y] в тайлах
    pub(crate) pos: [i32; 2],
    /// Статичные теги-перки: свойства сложения, которые даны коту и не растут
    /// (§12.17). Код знает по имени только те, для которых у него есть эффект.
    #[serde(default)]
    pub(crate) perks: Vec<String>,
}

/// Перк: коту он выдаётся по `id` в `units:`, здесь лежит только подпись для
/// интерфейса. Эффект перка знает код — как и у `cost`/`capacity` у тайлов.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct PerkDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
}

/// Навык — домен работы, а не действие игрока: стройка и снос это один навык,
/// потому что и джоб у них один (§12.17).
///
/// Порядок записей `skills:` — индекс домена, как порядок `tiles:` для палитры:
/// он лежит в `Skills::xp` и уходит в снапшот. Код обращается к навыку по `id`,
/// поэтому записи можно переставлять, но не переименовывать.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate) struct SkillDef {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    /// Пороги опыта: сколько всего очков нужно на 1-й, 2-й, … уровень. Длина
    /// списка — потолок навыка, выше него опыт не копится.
    #[serde(default)]
    pub(crate) levels: Vec<i32>,
    /// До какого уровня доводит парта (§12.18). Ноль — домену не учат вовсе:
    /// так живёт «Стройка», доступная с первого тика. Обучение — вход в домен,
    /// мастерство — из домена, поэтому потолок парты ниже потолка навыка.
    #[serde(default)]
    pub(crate) taught: i32,
}
