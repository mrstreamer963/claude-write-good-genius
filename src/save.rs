//! Сохранение партии: полный снимок мира (§12.45).
//!
//! Сохраняется **то, что изменилось с начала партии**; правила не сохраняются,
//! потому что они и есть рулсет — их целиком пересобирает `Sim::new`. Отсюда
//! ровно две части снимка: тайловая карта плюс ресурсы состояния, и компоненты
//! на сущностях.
//!
//! Формат привязан к рулсету **отпечатком** (`fingerprint`): индексы палитр
//! (тайлы, предметы, навыки, фракции, миссии, рецепты, темы) лежат в снимке
//! числами, а имена — только в YAML, поэтому правка рулсета молча меняет смысл
//! старого снимка. Несовпадение = отказ грузить, а не «поехавший» мир.
//!
//! Систем модуль не заводит: сохранение не происходит каждый тик, у него нет
//! ни сущности-владельца, ни расписания — только данные и арифметика в момент
//! события. Это то же исключение из «фича = данные + система», что у
//! шкал-ворот (§12.43).
//!
//! # Чем держится полнота
//!
//! Главный риск формата: новый компонент не попадёт в снимок, и мир после
//! загрузки будет тихо другим. Это природа пропущенного `Without<…>`
//! (инвариант 7), но там хотя бы ломается поведение, а тут не ломается ничего.
//! Поэтому у снимка есть сторож — `every_component_is_either_saved_or_skipped`:
//! он сверяет реестр `bevy_ecs` (компоненты и ресурсы лежат в нём вместе) с
//! двумя списками ниже. Тип, не попавший ни в один, роняет тест.
//!
//! Списки — единственное место, где перечислено «что вообще есть в мире»;
//! обновлять их вместе с `components.rs`, а не вместо.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::*;
use crate::map::BaseMap;

/// Версия формата снимка. Растёт, когда старый файл перестаёт читаться новым
/// кодом; несовпадение — отказ грузить, а не попытка догадаться.
///
/// # ⚠️ Поднимать руками, и никто об этом не напомнит
///
/// **Меняешь любой DTO ниже — подними число.** Сторож
/// (`every_component_is_either_saved_or_skipped`) следит за составом мира, но
/// про форму снимка не знает ничего, и на несовпадение схемы отказа не будет.
///
/// Молчание тут не случайность, а плата за компактность: у `EntityDto` на
/// каждом поле `#[serde(default)]` (у кота нет чертежа, у кучи нет бодрости),
/// а лишние поля serde игнорирует по умолчанию. Значит **снимок с разъехавшейся
/// схемой загрузится молча**: пропавшее поле станет `None`. Проверено — если
/// переименовать в файле `energy`, коты приезжают без бодрости вовсе, то есть
/// не устают никогда, и не падает ничего.
///
/// Пока схему правят вместе с этим числом, дыра не выстрелит. Если надоест
/// помнить — чинится тем же приёмом, что и сторож состава: тест считает
/// отпечаток имён полей всех DTO и сверяет с константой рядом, а расхождение
/// требует поднять `FORMAT`. На POC решено не заводить (§12.45).
pub(crate) const FORMAT: u32 = 29;

/// Что уходит в снимок. Порядок — как в `components.rs`: сперва компоненты,
/// потом ресурсы состояния.
///
/// Список — оснастка сторожа, а не рабочие данные: `capture` и `restore`
/// перечисляют компоненты кодом, и в боевую сборку он не едет.
#[cfg(test)]
pub(crate) const SAVED: &[&str] = &[
    // Кот и его тело.
    "Position",
    "Renderable",
    "UnitId",
    "Path",
    "Stride",
    "Carry",
    "Energy",
    "Fed",
    "Health",
    "Skills",
    "Stats",
    "Perks",
    "Gear",
    "Carrying",
    // Двенадцать задач общего слоя плюс приказ игрока и приписка к узлу.
    "Order",
    "Assignment",
    "Haul",
    "Rest",
    "Study",
    "Researching",
    "Crafting",
    "Equipping",
    "Eating",
    "Healing",
    "Treating",
    "Squad",
    "OnDuty",
    // Приписка к узлу — конфигурация, а не задача (§12.60), но состояние мира:
    // без неё загруженная партия забыла бы, кого игрок посадил на связь.
    "Posted",
    // Состав отряда на узле (§12.61) — той же природы: конфигурация, но
    // состояние мира. Без неё загруженная партия распустила бы все отряды.
    "Enlisted",
    // Приписка к парте (§12.84) — третья конфигурация того же рода: без неё
    // загруженная партия забыла бы, кого игрок отправил учиться, и ученик,
    // спавший в момент сохранения, за парту больше не вернулся бы.
    "Enrolled",
    // Состояния кота, которые не задачи.
    "Away",
    "Captive",
    // Объекты мира: разметка работы, кучи и сделки.
    "Blueprint",
    "Structure",
    "Stack",
    "ToStore",
    "Research",
    "Craft",
    "Deal",
    "Mission",
    // Ресурсы состояния. Карта живёт в `map.rs`, остальные — в `components.rs`.
    "BaseMap",
    "SimTime",
    "AutoTidy",
    "AutoRest",
    // Пороги автопроизводства — правило игрока, а не правило рулсета (§12.65):
    // без них загруженная партия забыла бы, что база держит запас деталей.
    "Stocking",
    // Автовылазки — то же правило игрока, только про узел связи (§12.67).
    "AutoRaids",
    // Автопродажа излишков — третье правило того же рода (§12.87). Забыть его
    // тише всех: партия грузится, база копит лом, и не происходит ничего.
    "Selling",
    // Закладки игрока в окне «Склад» (§12.100). Не правила — сами по себе они
    // ничего не делают, — но состояние партии: игрок решил, что держать на
    // виду, и после загрузки это решение обязано быть на месте. Держать их в JS
    // уже пробовали с «Убирать сам», и зеркало врало (§12.96).
    "Favorites",
    "Tickers",
    "Fame",
    "Money",
    // Отметки по условиям связки (§12.159): с какого тика держится каждое.
    // Не журнал и не правило — состояние мира, и без него поздравление
    // соврало бы датами.
    "GoalHolds",
    "Standing",
    "Techs",
    // Что база уже видела своими глазами (§12.131). Без этого загруженная
    // партия спрятала бы строки склада, которые игрок уже читал, — то есть
    // забыла бы половину своей истории тише, чем любой другой пропуск.
    "Seen",
    "Chronicle",
    "Goals",
    "Raids",
    "Crafted",
    "Earned",
    // Лента новостей (§12.120). В снимке она ради **базовой линии**: без неё
    // загруженная партия объявила бы новостью всё, что у базы и так открыто, —
    // и стопка тикеров встретила бы игрока на первом же тике.
    "News",
    "Trace",
];

/// Что в снимок не уходит — и почему. Причина обязательна: список без причин
/// через полгода неотличим от списка забытого. Как и `SAVED` — оснастка
/// сторожа.
#[cfg(test)]
pub(crate) const SKIPPED: &[(&str, &str)] = &[
    (
        "Worked",
        "живёт внутри одного тика: вешает система работы, снимает `train_skills` \
         в конце цепочки — между тиками его не существует (§12.17)",
    ),
    // Ниже — ресурсы-правила: разобранный YAML. Сохранять их значит завести
    // второй источник правды рядом с рулсетом, который к тому же разойдётся с
    // ним при первой же правке контента.
    ("SkillRules", "правила: пересобирает `Sim::new` из рулсета"),
    ("StatRules", "правила: пересобирает `Sim::new` из рулсета"),
    ("TileRules", "правила: пересобирает `Sim::new` из рулсета"),
    (
        "StructureRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    ("ItemRules", "правила: пересобирает `Sim::new` из рулсета"),
    (
        "LoadoutRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    (
        "MissionRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    (
        "FactionRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    (
        "ResearchRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    ("CraftRules", "правила: пересобирает `Sim::new` из рулсета"),
    (
        "TimelineRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    ("GoalRules", "правила: пересобирает `Sim::new` из рулсета"),
    (
        "AutoRules",
        "правила: пересобирает `Sim::new` из рулсета — это имена технологий, \
         открывающих автоматику (§12.93), а не состояние партии",
    ),
    (
        "RecruitRules",
        "правила: пересобирает `Sim::new` из рулсета",
    ),
    ("UnitRules", "правила: пересобирает `Sim::new` из рулсета"),
    ("NeedRules", "правила: пересобирает `Sim::new` из рулсета"),
    ("FoodRules", "правила: пересобирает `Sim::new` из рулсета"),
    ("HealthRules", "правила: пересобирает `Sim::new` из рулсета"),
];

/// Потолок отладочного журнала. Команд игрока за партию — сотни, а не сотни
/// тысяч, но журнал единственное, что росло бы неограниченно, поэтому предел
/// есть: старое вытесняется, «как игрок дошёл» остаётся про последнее.
pub(crate) const TRACE_MAX: usize = 5000;

// ── Файл сохранения ───────────────────────────────────────────────────────

/// Снимок партии целиком.
#[derive(Serialize, Deserialize)]
pub(crate) struct SaveFile {
    pub(crate) version: u32,
    /// Отпечаток рулсета, при котором снимок снят (`fingerprint`).
    pub(crate) ruleset: u64,
    pub(crate) map: MapDto,
    pub(crate) state: StateDto,
    /// Сущности в порядке возрастания их `Entity::index()`. Порядок значим:
    /// ссылки внутри — это номера в этом списке, и он же задаёт порядок,
    /// в котором сущности пересоздаются при загрузке.
    pub(crate) entities: Vec<EntityDto>,
    pub(crate) trace: Vec<TraceDto>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct MapDto {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) cells: Vec<i16>,
    pub(crate) version: u64,
}

/// Ресурсы состояния — всё, что меняется по ходу партии и не выводится из
/// рулсета.
#[derive(Serialize, Deserialize)]
pub(crate) struct StateDto {
    pub(crate) tick: u64,
    pub(crate) auto_tidy: bool,
    pub(crate) auto_rest: bool,
    /// Пороги автопроизводства по индексу рецепта (§12.65).
    #[serde(default)]
    pub(crate) stocking: Vec<i32>,
    /// Автовылазки: клетка узла, заказ, на который он ходит сам, и работает ли
    /// правило прямо сейчас (§12.67, §12.77). Усыплённое правило — такое же
    /// решение игрока, как активное, и партия обязана его помнить: иначе
    /// загрузка молча возвращает отряд в поле.
    #[serde(default)]
    pub(crate) auto_raids: Vec<(i32, i32, usize, bool)>,
    /// Правило излишка: предмет, **куда** его девать и сколько штук база
    /// придерживает (§12.87, §12.88, §12.115).
    ///
    /// Адресат едет как `Option<фракция>`: `None` — разбор. Не число со
    /// значением-маркером (`usize::MAX`) намеренно — маркер разобрался бы у
    /// старого снимка молча и продал бы лом несуществующей стороне.
    ///
    /// Порядок полей уже менялся (было «фракция, предмет»), а теперь сменился и
    /// тип: старый снимок разобрался бы **молча и наизнанку**. Это и есть тот
    /// случай, ради которого `FORMAT` поднимают руками.
    #[serde(default)]
    pub(crate) selling: Vec<(usize, Option<usize>, i32)>,
    /// Избранные предметы: их строки стоят наверху окна «Склад» (§12.100).
    #[serde(default)]
    pub(crate) favorites: Vec<usize>,
    /// Тикеры: предмет и сторона, с которой по нему торгуют в один клик
    /// (§12.100). Пара, а не тройка: порога у тикера нет — он не правило.
    #[serde(default)]
    pub(crate) tickers: Vec<(usize, usize)>,
    pub(crate) fame: i32,
    pub(crate) money: i32,
    pub(crate) standing: Vec<i32>,
    pub(crate) techs: Vec<String>,
    /// Что база уже видела (§12.131). Позиционно по палитре `items:`.
    ///
    /// `default` тут безопасен ровно потому, что `restore` не доверяет ему
    /// молча: пустую линию он тут же добирает по самому миру и по закладкам —
    /// иначе старый снимок спрятал бы весь склад до первого `note_seen`.
    #[serde(default)]
    pub(crate) seen: Vec<bool>,
    /// Журнал сработавших событий: `(индекс события, была ли база готова)`.
    pub(crate) chronicle: Vec<(usize, bool)>,
    /// Взятые цели: `(индекс цели, тик взятия)` (§12.58).
    #[serde(default)]
    pub(crate) goals: Vec<(usize, u64, Vec<u64>)>,
    /// Живые отметки: с какого тика держится каждое условие каждой невзятой цели
    /// (§12.159). Без них загруженная партия сказала бы в поздравлении, что всё
    /// сошлось в день загрузки.
    #[serde(default)]
    pub(crate) goal_holds: Vec<Vec<Option<u64>>>,
    /// Три журнала совершённого. Без них цель, уже взятая игроком, после
    /// загрузки открылась бы заново — а `Goals` их и так переживёт, так что
    /// расхождение было бы тихим: панель права, а условие «ещё не было».
    #[serde(default)]
    pub(crate) raids: Vec<usize>,
    #[serde(default)]
    pub(crate) crafted: Vec<usize>,
    #[serde(default)]
    pub(crate) earned: i32,
    /// Лента новостей (§12.120): базовая линия «что было открыто» и сама лента.
    /// Линия — по виду и записи палитры, в порядке `NewsKind::ALL`.
    ///
    /// ⚠️ **Новый вид новости — это подъём `FORMAT`** (так `Tile` довела его до
    /// 21 — §12.126, а `Item` до 24 — §12.136): состав строк здесь позиционный, и снимок с четырьмя видами
    /// разобрался бы молча, объявив новостью всю палитру разом.
    #[serde(default)]
    pub(crate) news_open: Vec<Vec<bool>>,
    /// `(вид, запись, открылось ли, тик)`.
    #[serde(default)]
    pub(crate) news: Vec<(usize, usize, bool, u64)>,
    /// Базовая линия уже снята. Отдельным флагом, а не «`news_open` не пуст»:
    /// мир без заказов, кандидатов и тем — законный (все синтетические такие), и
    /// без флага он снимал бы линию заново каждый тик.
    #[serde(default)]
    pub(crate) news_started: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TraceDto {
    pub(crate) tick: u64,
    pub(crate) cmd: String,
}

/// Одна сущность: набор её компонентов. Пустые поля в файл не пишутся —
/// у кота нет чертежа, у кучи нет бодрости, и таких «нет» большинство.
#[derive(Serialize, Deserialize, Default)]
pub(crate) struct EntityDto {
    // Кот и его тело.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pos: Option<(i32, i32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sprite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<Vec<(i32, i32)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Шаг, который кот делает прямо сейчас (§12.140): куда, сколько тиков
    /// осталось, сколько занимает целиком. До §12.140 здесь стоял обратный
    /// отсчёт `MoveCooldown`.
    pub(crate) stride: Option<((i32, i32), u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) carry: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) energy: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) health: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) skills: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stats: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) perks: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gear: Option<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) carrying: Option<(usize, i32)>,

    // Приказ игрока и одиннадцать задач общего слоя.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<(i32, i32, Option<u64>)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) assignment: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) haul: Option<HaulDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) aim: Option<AimDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) rest: Option<RestDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) study: Option<StudyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) researching: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) crafting: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) equipping: Option<(usize, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) eating: Option<(usize, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) healing: Option<HealingDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) treating: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) squad: Option<u32>,
    /// Дежурство на узле связи и приписка к нему (§12.60). Обе — клетки, а не
    /// ссылки на сущности: узел это тайл карты, и раскладывать вторым проходом
    /// тут нечего.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) on_duty: Option<(i32, i32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) posted: Option<(i32, i32)>,
    /// В отряде какого узла кот числится (§12.61) — тоже клетка, а не ссылка.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) enlisted: Option<(i32, i32)>,
    /// Чему кот приписан учиться (§12.84) — домен, а не клетка: парту он
    /// выбирает сам, любую свободную.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) enrolled: Option<usize>,

    // Состояния кота, которые не задачи.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) away: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) captive: bool,

    // Объекты мира.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) blueprint: Option<BlueprintDto>,
    /// Объект-штамп (§12.160). Ссылок на сущности в нём нет: клетки объект
    /// считает сам по якорю и повороту, а чертежи его не знают — связь идёт
    /// в обратную сторону, через карту.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) structure: Option<StructureDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stack: Option<(usize, i32)>,
    /// Пометка «на склад» — чистый флаг: носильщика она не держит (§12.48).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) to_store: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) research: Option<ResearchDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) craft: Option<CraftDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deal: Option<DealDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mission: Option<MissionDto>,
}

/// Куда кот несёт груз. Ссылки — номера сущностей в списке снимка.
#[derive(Serialize, Deserialize)]
pub(crate) enum HaulDto {
    Site(u32),
    Sale(u32),
    Shop(u32),
    Lab(u32),
    Store(Option<u32>),
}

/// Наводка носильщика: за какой кучей и за каким типом он пошёл (§12.49).
/// Куча — номер сущности, как и все ссылки снимка.
#[derive(Serialize, Deserialize)]
pub(crate) struct AimDto {
    pub(crate) pile: u32,
    pub(crate) item: usize,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RestDto {
    pub(crate) spot: Option<(i32, i32)>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StudyDto {
    pub(crate) skill: usize,
    pub(crate) spot: (i32, i32),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct HealingDto {
    pub(crate) spot: Option<(i32, i32)>,
    pub(crate) medic: Option<u32>,
    pub(crate) kit: i32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StructureDto {
    pub(crate) def: usize,
    pub(crate) anchor: (i32, i32),
    pub(crate) rot: u8,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BlueprintDto {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) tile: i16,
    pub(crate) progress: i32,
    pub(crate) assignee: Option<u32>,
    pub(crate) delivered: Vec<(usize, i32)>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ResearchDto {
    pub(crate) def: usize,
    pub(crate) progress: i32,
    /// Учёные над темой (§12.163). Список, а не один: `#[serde(default)]` тут
    /// уместен — снимок формата до §12.163 разберётся в тему без исполнителей,
    /// и раздатчик посадит их заново тем же тиком. Это не то же, что `cell`
    /// ниже: там пропавшее поле молча убивало оплаченную тему.
    #[serde(default)]
    pub(crate) assignees: Vec<u32>,
    /// Ячейка лаборатории, в которой стоит тема (§12.132).
    ///
    /// ⚠️ **`#[serde(default)]` тут ставить нельзя.** Снимок формата до §12.132
    /// разобрался бы молча, положив все темы в (0, 0) — клетку, которая почти
    /// наверняка не лаборатория, — и раздатчик снёс бы их первым же тиком как
    /// «лабораторию снесли». Оплаченная тема пропадала бы без единого слова.
    /// Отсутствие `default` даёт честный отказ разбора, а `FORMAT` — причину.
    pub(crate) cell: (i32, i32),
    /// Что уже привезли на вскрытие (§12.133).
    #[serde(default)]
    pub(crate) delivered: Vec<(usize, i32)>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CraftDto {
    pub(crate) def: usize,
    pub(crate) left: i32,
    pub(crate) progress: i32,
    /// Что уже завезли на станок (§12.102). До него здесь был `paid: bool`:
    /// материал списывался со склада мгновенно, и хранить было нечего.
    #[serde(default)]
    pub(crate) delivered: Vec<(usize, i32)>,
    pub(crate) assignee: Option<u32>,
    /// Ячейка станка, в которой стоит заказ (§12.96). Не `Option`: заказ
    /// рождается в ней и держит её до последней штуки, как сделка — ячейку
    /// поста.
    pub(crate) cell: (i32, i32),
    /// Заказ ведёт правило-порог (§12.65). Без него загруженная партия отдала бы
    /// автозаказ игроку: правило перестало бы его вести, а «Отменить» появилась
    /// бы там, где её быть не должно.
    #[serde(default)]
    pub(crate) auto: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DealDto {
    pub(crate) faction: usize,
    pub(crate) item: usize,
    pub(crate) count: i32,
    pub(crate) unit: i32,
    pub(crate) buying: bool,
    pub(crate) left: i32,
    pub(crate) delivered: i32,
    pub(crate) cell: (i32, i32),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct MissionDto {
    pub(crate) def: usize,
    /// Клетка гаража: и дверь, и слот вылазки (§12.152). До §12.152 полей было
    /// два — `gate` (шлюз, подобранный автоматом, оттого `Option`) и `node`
    /// (рация, державшая слот), — и §12.152 схлопнула их в одно. Старый снимок
    /// разобрался бы **молча и наполовину**: `node` пропал бы, а `gate` приехал
    /// бы `None`, то есть отряд без двери. Это и есть тот случай, ради которого
    /// `FORMAT` поднимают руками, — поднят до 26.
    pub(crate) gate: (i32, i32),
    pub(crate) left: i32,
    /// Набежавшая связь (§12.60). Состояние вылазки, а не правило: без него
    /// загруженная партия теряла бы накопленный бонус.
    #[serde(default)]
    pub(crate) covered: i32,
    /// Замороженный на уходе срок вылазки (§12.70). Состояние, а не правило:
    /// он посчитан по числу дошедших лап, и из рулсета его уже не вывести.
    #[serde(default)]
    pub(crate) span: i32,
}

// ── Снять снимок ──────────────────────────────────────────────────────────

/// Собрать снимок мира.
///
/// Сущности обходятся **в порядке `Entity::index()`**, а не в порядке архетипов:
/// обход ECS зависит от истории вставок, а файл обязан получаться один и тот же
/// (§11). Этот же порядок задаёт номера ссылок и порядок пересоздания при
/// загрузке.
pub(crate) fn capture(world: &World, ruleset: u64) -> SaveFile {
    let mut ids: Vec<Entity> = world.iter_entities().map(|e| e.id()).collect();
    ids.sort_by_key(|e| e.index());
    let number: HashMap<Entity, u32> = ids
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, i as u32))
        .collect();
    let at = |e: Entity| number.get(&e).copied();

    let map = world.resource::<BaseMap>();
    let chronicle = world.resource::<Chronicle>();

    let entities = ids
        .iter()
        .map(|&id| {
            let e = world.entity(id);
            EntityDto {
                pos: e.get::<Position>().map(|p| (p.x, p.y)),
                sprite: e.get::<Renderable>().map(|r| r.sprite.clone()),
                unit: e.get::<UnitId>().map(|u| u.0.clone()),
                path: e.get::<Path>().map(|p| p.steps.clone()),
                stride: e.get::<Stride>().map(|s| (s.to, s.left, s.span)),
                carry: e.get::<Carry>().map(|c| c.0),
                energy: e.get::<Energy>().map(|c| c.0),
                fed: e.get::<Fed>().map(|c| c.0),
                health: e.get::<Health>().map(|c| c.0),
                skills: e.get::<Skills>().map(|s| s.xp.clone()),
                stats: e.get::<Stats>().map(|s| s.0.clone()),
                perks: e.get::<Perks>().map(|p| p.0.clone()),
                gear: e.get::<Gear>().map(|g| g.0.clone()),
                carrying: e.get::<Carrying>().map(|c| (c.item, c.count)),

                order: e.get::<Order>().map(|o| (o.x, o.y, o.tried_version)),
                assignment: e.get::<Assignment>().and_then(|a| at(a.0)),
                haul: e.get::<Haul>().map(|h| match h.to {
                    HaulTo::Site(t) => HaulDto::Site(at(t).unwrap_or(u32::MAX)),
                    HaulTo::Sale(t) => HaulDto::Sale(at(t).unwrap_or(u32::MAX)),
                    HaulTo::Shop(t) => HaulDto::Shop(at(t).unwrap_or(u32::MAX)),
                    HaulTo::Lab(t) => HaulDto::Lab(at(t).unwrap_or(u32::MAX)),
                    HaulTo::Store(t) => HaulDto::Store(t.and_then(at)),
                }),
                aim: e
                    .get::<Haul>()
                    .and_then(|h| h.aim)
                    .and_then(|a| at(a.pile).map(|pile| AimDto { pile, item: a.item })),
                rest: e.get::<Rest>().map(|r| RestDto { spot: r.spot }),
                study: e.get::<Study>().map(|s| StudyDto {
                    skill: s.skill,
                    spot: s.spot,
                }),
                researching: e.get::<Researching>().and_then(|r| at(r.0)),
                crafting: e.get::<Crafting>().and_then(|c| at(c.0)),
                equipping: e
                    .get::<Equipping>()
                    .map(|q| (q.item, at(q.pile).unwrap_or(u32::MAX))),
                eating: e
                    .get::<Eating>()
                    .map(|q| (q.item, at(q.pile).unwrap_or(u32::MAX))),
                healing: e.get::<Healing>().map(|h| HealingDto {
                    spot: h.spot,
                    medic: h.medic.and_then(at),
                    kit: h.kit,
                }),
                treating: e.get::<Treating>().and_then(|t| at(t.0)),
                squad: e.get::<Squad>().and_then(|s| at(s.0)),
                on_duty: e.get::<OnDuty>().map(|d| d.spot),
                posted: e.get::<Posted>().map(|p| p.spot),
                enlisted: e.get::<Enlisted>().map(|c| c.spot),
                enrolled: e.get::<Enrolled>().map(|c| c.skill),

                away: e.contains::<Away>(),
                captive: e.contains::<Captive>(),

                blueprint: e.get::<Blueprint>().map(|b| BlueprintDto {
                    x: b.x,
                    y: b.y,
                    tile: b.tile,
                    progress: b.progress,
                    assignee: b.assignee.and_then(at),
                    delivered: b.delivered.clone(),
                }),
                structure: e.get::<Structure>().map(|s| StructureDto {
                    def: s.def,
                    anchor: s.anchor,
                    rot: s.rot,
                }),
                stack: e.get::<Stack>().map(|s| (s.item, s.count)),
                to_store: e.contains::<ToStore>(),
                research: e.get::<Research>().map(|r| ResearchDto {
                    def: r.def,
                    progress: r.progress,
                    assignees: r.assignees.iter().filter_map(|&e| at(e)).collect(),
                    cell: r.cell,
                    delivered: r.delivered.clone(),
                }),
                craft: e.get::<Craft>().map(|c| CraftDto {
                    def: c.def,
                    left: c.left,
                    progress: c.progress,
                    delivered: c.delivered.clone(),
                    assignee: c.assignee.and_then(at),
                    cell: c.cell,
                    auto: c.auto,
                }),
                deal: e.get::<Deal>().map(|d| DealDto {
                    faction: d.faction,
                    item: d.item,
                    count: d.count,
                    unit: d.unit,
                    buying: d.buying,
                    left: d.left,
                    delivered: d.delivered,
                    cell: d.cell,
                }),
                mission: e.get::<Mission>().map(|m| MissionDto {
                    def: m.def,
                    gate: m.gate,
                    left: m.left,
                    span: m.span,
                    covered: m.covered,
                }),
            }
        })
        .collect();

    SaveFile {
        version: FORMAT,
        ruleset,
        map: MapDto {
            width: map.width,
            height: map.height,
            cells: map.cells.clone(),
            version: map.version,
        },
        state: StateDto {
            tick: world.resource::<SimTime>().tick,
            auto_tidy: world.resource::<AutoTidy>().0,
            auto_rest: world.resource::<AutoRest>().0,
            stocking: world.resource::<Stocking>().0.clone(),
            auto_raids: world.resource::<AutoRaids>().0.clone(),
            selling: world
                .resource::<Selling>()
                .0
                .iter()
                .map(|&(item, dest, keep)| {
                    let side = match dest {
                        Surplus::Sell(faction) => Some(faction),
                        Surplus::Salvage => None,
                    };
                    (item, side, keep)
                })
                .collect(),
            favorites: world.resource::<Favorites>().0.clone(),
            tickers: world.resource::<Tickers>().0.clone(),
            fame: world.resource::<Fame>().0,
            money: world.resource::<Money>().0,
            standing: world.resource::<Standing>().0.clone(),
            techs: world.resource::<Techs>().0.clone(),
            seen: world.resource::<Seen>().0.clone(),
            chronicle: chronicle.0.iter().map(|h| (h.def, h.ready)).collect(),
            goals: world
                .resource::<Goals>()
                .0
                .iter()
                .map(|t| (t.def, t.at, t.parts.clone()))
                .collect(),
            goal_holds: world.resource::<GoalHolds>().0.clone(),
            raids: world.resource::<Raids>().0.clone(),
            crafted: world.resource::<Crafted>().0.clone(),
            earned: world.resource::<Earned>().0,
            news_open: world.resource::<News>().open.clone(),
            news: world
                .resource::<News>()
                .feed
                .iter()
                .map(|n| (n.kind.index(), n.def, n.opened, n.at))
                .collect(),
            news_started: world.resource::<News>().started,
        },
        entities,
        trace: world
            .resource::<Trace>()
            .0
            .iter()
            .map(|e| TraceDto {
                tick: e.tick,
                cmd: e.cmd.clone(),
            })
            .collect(),
    }
}

// ── Восстановить мир ──────────────────────────────────────────────────────

/// Разложить снимок в мир, собранный `Sim::new` из того же рулсета.
///
/// Стартовая застройка и стартовые коты к этому моменту уже есть — их сносим:
/// снимок описывает мир целиком, а не поверх чего-то.
///
/// Загрузка в два прохода. Сначала сущности создаются пустыми, чтобы номер из
/// файла получил свой `Entity`; только потом вставляются компоненты, потому что
/// ссылаться они могут на кого угодно, в том числе вперёд.
/// Поднять шкалу «что база видела» (§12.131).
///
/// Читает не только `state.seen`, но и сам снимок — кучи, лапы и надетое, — а
/// заодно закладки игрока. Причина в том, что `default` у поля снисходителен:
/// снимок формата до §12.131 везёт пустую линию, и партия, у которой в шапке
/// закреплён паёк, а на складе лежат тонны лома, встретила бы игрока пустым
/// складом. Добор по миру делает это невозможным: то, что база держит в лапах,
/// она заведомо видела.
///
/// Из `file`, а не из мира: компоненты сущностей вставляются **после** этого
/// места, и мир на момент вызова ещё пуст.
///
/// Закладка — второй источник и не менее надёжный: закрепить строку можно
/// только у предмета, чья строка была на экране.
fn restore_seen(world: &mut World, file: &SaveFile) {
    let s = &file.state;
    let mut seen = world.resource_mut::<Seen>();
    for (item, &saw) in s.seen.iter().enumerate() {
        if saw {
            seen.mark(item);
        }
    }
    for dto in &file.entities {
        if let Some((item, count)) = dto.stack
            && count > 0
        {
            seen.mark(item);
        }
        if let Some((item, count)) = dto.carrying
            && count > 0
        {
            seen.mark(item);
        }
        if let Some(gear) = &dto.gear {
            for &item in gear {
                seen.mark(item);
            }
        }
    }
    for &item in &s.favorites {
        seen.mark(item);
    }
    for &(item, _) in &s.tickers {
        seen.mark(item);
    }
}

pub(crate) fn restore(world: &mut World, file: &SaveFile) {
    let old: Vec<Entity> = world.iter_entities().map(|e| e.id()).collect();
    for id in old {
        world.despawn(id);
    }

    let ids: Vec<Entity> = (0..file.entities.len())
        .map(|_| world.spawn_empty().id())
        .collect();
    let at = |n: u32| ids.get(n as usize).copied();

    {
        let mut map = world.resource_mut::<BaseMap>();
        map.width = file.map.width;
        map.height = file.map.height;
        map.cells = file.map.cells.clone();
        // Версия карты — единственный сигнал «карта изменилась» (инвариант 3).
        // Ставим её заведомо новой, а не из файла: рендер по ту сторону
        // протокола помнит версию **прошлого** мира, и совпадение номеров
        // означало бы, что карту ему не пришлют вовсе.
        map.version = file.map.version + 1;
    }

    let s = &file.state;
    world.resource_mut::<SimTime>().tick = s.tick;
    world.resource_mut::<AutoTidy>().0 = s.auto_tidy;
    world.resource_mut::<AutoRest>().0 = s.auto_rest;
    world.resource_mut::<Stocking>().0 = s.stocking.clone();
    world.resource_mut::<AutoRaids>().0 = s.auto_raids.clone();
    world.resource_mut::<Selling>().0 = s
        .selling
        .iter()
        .map(|&(item, side, keep)| {
            let dest = side.map_or(Surplus::Salvage, Surplus::Sell);
            (item, dest, keep)
        })
        .collect();
    world.resource_mut::<Favorites>().0 = s.favorites.clone();
    world.resource_mut::<Tickers>().0 = s.tickers.clone();
    world.resource_mut::<Fame>().0 = s.fame;
    world.resource_mut::<Money>().0 = s.money;
    world.resource_mut::<Standing>().0 = s.standing.clone();
    world.resource_mut::<Techs>().0 = s.techs.clone();
    restore_seen(world, file);
    world.resource_mut::<Chronicle>().0 = s
        .chronicle
        .iter()
        .map(|&(def, ready)| Happened { def, ready })
        .collect();
    world.resource_mut::<Goals>().0 = s
        .goals
        .iter()
        .map(|(def, at, parts)| Taken {
            def: *def,
            at: *at,
            parts: parts.clone(),
        })
        .collect();
    world.resource_mut::<GoalHolds>().0 = s.goal_holds.clone();
    world.resource_mut::<Raids>().0 = s.raids.clone();
    world.resource_mut::<Crafted>().0 = s.crafted.clone();
    world.resource_mut::<Earned>().0 = s.earned;
    {
        let mut news = world.resource_mut::<News>();
        news.open = s.news_open.clone();
        news.started = s.news_started;
        news.feed = s
            .news
            .iter()
            .filter_map(|&(kind, def, opened, at)| {
                NewsKind::from_index(kind).map(|kind| Note {
                    kind,
                    def,
                    opened,
                    at,
                })
            })
            .collect();
    }
    world.resource_mut::<Trace>().0 = file
        .trace
        .iter()
        .map(|t| TraceEntry {
            tick: t.tick,
            cmd: t.cmd.clone(),
        })
        .collect();

    for (dto, &id) in file.entities.iter().zip(&ids) {
        let mut e = world.entity_mut(id);

        if let Some((x, y)) = dto.pos {
            e.insert(Position { x, y });
        }
        if let Some(sprite) = &dto.sprite {
            e.insert(Renderable {
                sprite: sprite.clone(),
            });
        }
        if let Some(unit) = &dto.unit {
            e.insert(UnitId(unit.clone()));
        }
        if let Some(steps) = &dto.path {
            e.insert(Path {
                steps: steps.clone(),
            });
        }
        if let Some((to, left, span)) = dto.stride {
            e.insert(Stride { to, left, span });
        }
        if let Some(c) = dto.carry {
            e.insert(Carry(c));
        }
        if let Some(c) = dto.energy {
            e.insert(Energy(c));
        }
        if let Some(c) = dto.fed {
            e.insert(Fed(c));
        }
        if let Some(c) = dto.health {
            e.insert(Health(c));
        }
        if let Some(xp) = &dto.skills {
            e.insert(Skills { xp: xp.clone() });
        }
        if let Some(v) = &dto.stats {
            e.insert(Stats(v.clone()));
        }
        if let Some(v) = &dto.perks {
            e.insert(Perks(v.clone()));
        }
        if let Some(v) = &dto.gear {
            e.insert(Gear(v.clone()));
        }
        if let Some((item, count)) = dto.carrying {
            e.insert(Carrying { item, count });
        }

        if let Some((x, y, tried_version)) = dto.order {
            e.insert(Order {
                x,
                y,
                tried_version,
            });
        }
        if let Some(n) = dto.assignment.and_then(at) {
            e.insert(Assignment(n));
        }
        if let Some(h) = &dto.haul {
            let to = match *h {
                HaulDto::Site(n) => at(n).map(HaulTo::Site),
                HaulDto::Sale(n) => at(n).map(HaulTo::Sale),
                HaulDto::Shop(n) => at(n).map(HaulTo::Shop),
                HaulDto::Lab(n) => at(n).map(HaulTo::Lab),
                HaulDto::Store(n) => Some(HaulTo::Store(n.and_then(at))),
            };
            // Наводка без кучи — не беда: кот дойдёт и возьмёт что под ногами,
            // а раздатчик пересчитает обещания (§12.49).
            let aim = dto
                .aim
                .as_ref()
                .and_then(|a| at(a.pile).map(|pile| Aim { pile, item: a.item }));
            if let Some(to) = to {
                e.insert(Haul { to, aim });
            }
        }
        if let Some(r) = &dto.rest {
            e.insert(Rest { spot: r.spot });
        }
        if let Some(s) = &dto.study {
            e.insert(Study {
                skill: s.skill,
                spot: s.spot,
            });
        }
        if let Some(n) = dto.researching.and_then(at) {
            e.insert(Researching(n));
        }
        if let Some(n) = dto.crafting.and_then(at) {
            e.insert(Crafting(n));
        }
        if let Some((item, pile)) = dto.equipping
            && let Some(pile) = at(pile)
        {
            e.insert(Equipping { item, pile });
        }
        if let Some((item, pile)) = dto.eating
            && let Some(pile) = at(pile)
        {
            e.insert(Eating { item, pile });
        }
        if let Some(h) = &dto.healing {
            e.insert(Healing {
                spot: h.spot,
                medic: h.medic.and_then(at),
                kit: h.kit,
            });
        }
        if let Some(n) = dto.treating.and_then(at) {
            e.insert(Treating(n));
        }
        if let Some(n) = dto.squad.and_then(at) {
            e.insert(Squad(n));
        }
        if let Some(spot) = dto.on_duty {
            e.insert(OnDuty { spot });
        }
        if let Some(spot) = dto.posted {
            e.insert(Posted { spot });
        }
        if let Some(skill) = dto.enrolled {
            e.insert(Enrolled { skill });
        }
        if let Some(spot) = dto.enlisted {
            e.insert(Enlisted { spot });
        }

        if dto.away {
            e.insert(Away);
        }
        if dto.captive {
            e.insert(Captive);
        }

        if let Some(b) = &dto.blueprint {
            e.insert(Blueprint {
                x: b.x,
                y: b.y,
                tile: b.tile,
                progress: b.progress,
                assignee: b.assignee.and_then(at),
                delivered: b.delivered.clone(),
            });
        }
        if let Some(s) = &dto.structure {
            e.insert(Structure {
                def: s.def,
                anchor: s.anchor,
                rot: s.rot,
            });
        }
        if let Some((item, count)) = dto.stack {
            e.insert(Stack { item, count });
        }
        if dto.to_store {
            e.insert(ToStore);
        }
        if let Some(r) = &dto.research {
            e.insert(Research {
                def: r.def,
                progress: r.progress,
                assignees: r.assignees.iter().filter_map(|&n| at(n)).collect(),
                cell: r.cell,
                delivered: r.delivered.clone(),
            });
        }
        if let Some(c) = &dto.craft {
            e.insert(Craft {
                def: c.def,
                left: c.left,
                progress: c.progress,
                delivered: c.delivered.clone(),
                assignee: c.assignee.and_then(at),
                cell: c.cell,
                auto: c.auto,
            });
        }
        if let Some(d) = &dto.deal {
            e.insert(Deal {
                faction: d.faction,
                item: d.item,
                count: d.count,
                unit: d.unit,
                buying: d.buying,
                left: d.left,
                delivered: d.delivered,
                cell: d.cell,
            });
        }
        if let Some(m) = &dto.mission {
            e.insert(Mission {
                covered: m.covered,
                def: m.def,
                gate: m.gate,
                left: m.left,
                span: m.span,
            });
        }
    }
}

/// Записать команду игрока в отладочный журнал (§12.45).
///
/// Зовётся из фасада, и только оттуда: команда игрока — это вызов фасада, а
/// второе место записи означало бы журнал, расходящийся с тем, что игрок делал.
pub(crate) fn note(world: &mut World, cmd: impl Into<String>) {
    let tick = world.resource::<SimTime>().tick;
    let mut trace = world.resource_mut::<Trace>();
    if trace.0.len() >= TRACE_MAX {
        trace.0.pop_front();
    }
    trace.0.push_back(TraceEntry {
        tick,
        cmd: cmd.into(),
    });
}

/// Короткое имя типа из полного пути `bevy_ecs` (`sp_sim::components::Position`
/// → `Position`). Нужно только сторожу — реестр он читает сам.
#[cfg(test)]
pub(crate) fn short_name(full: &str) -> &str {
    full.rsplit("::").next().unwrap_or(full)
}

/// Отпечаток рулсета — FNV-1a по тексту YAML.
///
/// Своя реализация вместо зависимости: хэш обязан быть **детерминирован между
/// запусками** (§11), а `DefaultHasher` этого не обещает и от версии к версии
/// меняется. Криптостойкость здесь не нужна — отпечаток ловит правку контента,
/// а не подделку.
pub(crate) fn fingerprint(ruleset_yaml: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ruleset_yaml.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}
