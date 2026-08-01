//! DTO наружу (worker → main), сериализуются serde-wasm-bindgen.
//!
//! Это весь контракт с рендером: `map_meta` и `base_map` уходят по требованию
//! (при росте версии карты), `snapshot` — каждый кадр целиком (§12.8a).

use serde::Serialize;

use crate::ruleset::{
    ItemDef, MissionDef, PerkDef, RecipeDef, RecruitDef, ResearchDef, SkillDef, TileDef,
};

#[derive(Serialize)]
pub(crate) struct MapMeta {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) palette: Vec<TileDef>,
    /// Палитра предметов: тип в снапшоте — это её индекс (§12.21).
    pub(crate) items: Vec<ItemDef>,
    /// Палитра навыков: имена и подписи уходят один раз, как палитра тайлов, —
    /// в снапшоте от навыка остаётся только номер (§12.17).
    pub(crate) skills: Vec<SkillDef>,
    /// Подписи перков; в снапшоте у кота лежат только их `id`.
    pub(crate) perks: Vec<PerkDef>,
    /// Палитра миссий: запуск ходит по индексу в этом списке (§12.22).
    pub(crate) missions: Vec<MissionDef>,
    /// Кандидаты на найм; найм ходит по индексу в этом списке (§12.24).
    pub(crate) recruits: Vec<RecruitDef>,
    /// Темы исследования; заявка ходит по индексу в этом списке (§12.26).
    pub(crate) research: Vec<ResearchDef>,
    /// Рецепты; заказ ходит по индексу в этом списке (§12.30).
    pub(crate) recipes: Vec<RecipeDef>,
}

#[derive(Serialize)]
pub(crate) struct BaseMapDto<'a> {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) cells: &'a [i16],
}

#[derive(Serialize)]
pub(crate) struct Snapshot {
    pub(crate) tick: u64,
    pub(crate) entities: Vec<EntitySnap>,
    pub(crate) blueprints: Vec<BlueprintSnap>,
    pub(crate) stacks: Vec<StackSnap>,
    /// Миссия в работе; список, а не одна запись, — но фасад пока пускает по
    /// одной за раз (§12.22).
    pub(crate) missions: Vec<MissionSnap>,
    /// Известность базы: копится от вылазок, открывает вылазки и кандидатов.
    pub(crate) fame: i32,
    /// Кандидаты в порядке палитры из `map_meta`.
    pub(crate) recruits: Vec<RecruitSnap>,
    /// Тема в работе; список, а не одна запись, — но фасад пока пускает по
    /// одной за раз, как и вылазку (§12.26).
    pub(crate) research: Vec<ResearchSnap>,
    /// Темы в порядке палитры: можно ли взяться и если нет, то почему.
    pub(crate) topics: Vec<TopicSnap>,
    /// Заказ в работе; список, а не одна запись, — но фасад пускает по одному
    /// за раз, как вылазку и тему (§12.30).
    pub(crate) crafting: Vec<CraftSnap>,
    /// Рецепты в порядке палитры: можно ли заказать и если нет, то почему.
    pub(crate) recipes: Vec<RecipeSnap>,
    /// Изученные технологии. Только растут и только открывают (§12.24, §12.26).
    pub(crate) techs: Vec<String>,
    /// Записка: даты мира и то, что о них известно **сейчас** (§12.28).
    pub(crate) notes: Vec<NoteSnap>,
}

/// Строка записки: дата, скелет события и — если срок близко — подробности.
///
/// **Туман предзнания живёт здесь, а не в интерфейсе** (§4.6, §12.28): пока
/// детали не проступили, их в снапшоте просто нет. Спрятанный в JS текст — это
/// не «повреждённые данные», а непоказанный текст, и однажды он покажется.
#[derive(Serialize)]
pub(crate) struct NoteSnap {
    /// Индекс записи в `timeline:` рулсета.
    pub(crate) def: usize,
    pub(crate) label: String,
    /// Тик события и сколько до него осталось (отрицательное — уже позади).
    pub(crate) at: u64,
    pub(crate) left: i64,
    /// «Скелет» — виден всегда, ещё до раскрытия: игрок знает, что будет, но
    /// не знает, чем это грозит.
    pub(crate) hint: String,
    /// Детали проступили: `detail` и `requires` больше не пусты.
    pub(crate) revealed: bool,
    pub(crate) detail: String,
    /// Каких технологий ждёт событие; пусто до раскрытия и у безусловных.
    pub(crate) requires: Vec<String>,
    /// Успевает ли база прямо сейчас — считает ядро, как и доступность найма
    /// (§12.24). До раскрытия деталей это тоже деталь, поэтому всегда false.
    pub(crate) ready: bool,
    /// Событие уже прошло — и вот с каким исходом.
    pub(crate) done: bool,
    pub(crate) succeeded: bool,
}

/// Заказ в работе: сколько штук осталось, сколько сделано у текущей и кто занят.
///
/// Отдельное от темы DTO, потому что у заказа есть то, чего у темы нет и быть не
/// может, — **счётчик штук** (§12.30). Полоска показывает текущую штуку, а не
/// весь заказ: работа считается поштучно, и «40% от пяти» игрок прочтёт неверно.
#[derive(Serialize)]
pub(crate) struct CraftSnap {
    /// Индекс записи в палитре рецептов из `map_meta`.
    pub(crate) def: usize,
    /// Сколько штук осталось, включая ту, что в работе.
    pub(crate) left: i32,
    pub(crate) progress: i32,
    pub(crate) total: i32,
    /// Материал текущей штуки списан: заказ работает, а не ждёт склада.
    pub(crate) paid: bool,
    /// Кто у верстака; пусто — исполнитель ещё не нашёлся.
    pub(crate) unit: String,
}

/// Можно ли заказать рецепт прямо сейчас — и если нет, то почему.
///
/// Считает ядро, а не интерфейс, по той же причине, что и у тем (§12.26).
/// `affordable` здесь — **подсказка, а не запрет**: заказ без материала ядро
/// примет, он просто будет ждать, как чертёж без лома (§12.30).
#[derive(Serialize)]
pub(crate) struct RecipeSnap {
    /// Технологии рецепта изучены: он вообще существует.
    pub(crate) unlocked: bool,
    /// На складе хватает на одну штуку прямо сейчас.
    pub(crate) affordable: bool,
    /// И есть мастерская, в которой работать.
    pub(crate) shop: bool,
}

/// Тема в работе: сколько сделано и кто этим занят.
#[derive(Serialize)]
pub(crate) struct ResearchSnap {
    /// Индекс записи в палитре тем из `map_meta`.
    pub(crate) def: usize,
    pub(crate) progress: i32,
    pub(crate) total: i32,
    /// Кто сейчас за темой; пусто — исполнитель ещё не нашёлся.
    pub(crate) unit: String,
}

/// Можно ли взяться за тему прямо сейчас — и если нет, то почему.
///
/// Считает ядро, а не интерфейс, по той же причине, что и у кандидатов
/// (§12.24): «есть ли на складе образцы» и «хватает ли кому-то „Науки“» — это
/// знание ядра, и дублировать его в JS значит однажды показать кнопку, которую
/// ядро отклонит.
#[derive(Serialize)]
pub(crate) struct TopicSnap {
    /// Технология уже изучена — второй раз тему не берут.
    pub(crate) known: bool,
    /// Предыдущие технологии на месте: тема вообще существует.
    pub(crate) unlocked: bool,
    /// На складе есть чем заплатить.
    pub(crate) affordable: bool,
    /// На базе есть кот с нужным уровнем «Науки» — это допуск (§12.18).
    pub(crate) staffed: bool,
    /// И есть лаборатория, в которой работать.
    pub(crate) lab: bool,
}

/// Можно ли нанять кандидата прямо сейчас — и если нет, то почему.
///
/// Считает ядро, а не интерфейс: «хватает ли на складе» — это знание о том, что
/// склад и есть казна (§12.24), и дублировать его в JS значит однажды показать
/// кнопку, которую ядро отклонит.
#[derive(Serialize)]
pub(crate) struct RecruitSnap {
    /// Уже на базе — нанять второй раз нельзя, коты уникальны (§4.2).
    pub(crate) hired: bool,
    /// Известности хватает: кандидат откликается.
    pub(crate) unlocked: bool,
    /// И на складе есть чем заплатить.
    pub(crate) affordable: bool,
}

#[derive(Serialize)]
pub(crate) struct EntitySnap {
    pub(crate) id: String,
    pub(crate) sprite: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Кот ничего не может сделать сам: либо замурован в пустоте без проходимых
    /// соседей, либо его приказ сейчас невыполним. Для подсветки в UI —
    /// состояние легальное (см. снос пола), но игрок должен его видеть.
    pub(crate) stuck: bool,
    /// Сколько кот несёт (0 — пустой) и чего именно (-1 — пустой).
    pub(crate) carrying: i32,
    pub(crate) carrying_item: i32,
    /// Сколько лома кот берёт за ходку (0 — без предела).
    pub(crate) carry_max: i32,
    /// Навыки в порядке палитры из `map_meta`. Рост, которого не видно, не
    /// существует, — панель выбранного кота единственное место, где он заметен.
    pub(crate) skills: Vec<SkillSnap>,
    /// Статичные перки: они не растут, но объясняют игроку, почему этот кот
    /// таскает больше соседа (§12.17).
    pub(crate) perks: Vec<String>,
    /// Что надето — индексы палитры предметов из `map_meta` (§12.29). Сила
    /// снаряжения входит в силу отряда, поэтому пропавший со склада комбинезон
    /// обязан быть виден на коте: иначе прогноз вылазки меняется «сам собой».
    pub(crate) gear: Vec<usize>,
    /// Бодрость и её потолок; `energy_max = 0` — усталость выключена рулсетом.
    pub(crate) energy: i32,
    pub(crate) energy_max: i32,
    /// Кот спит здесь и сейчас (в пути к лежанке — ещё нет): состояние
    /// легальное и обратимое, но игрок должен видеть, почему тот не работает.
    pub(crate) sleeping: bool,
    /// Кот сидит за партой. Учёба — потраченное котовремя и единственная её
    /// цена (§12.18), поэтому она обязана быть видна там же, где сон.
    pub(crate) studying: bool,
    /// Кот ушёл с базы на миссию. Позиция при этом остаётся на шлюзе, поэтому
    /// рисовать его на карте нельзя — там его нет (§12.22).
    pub(crate) away: bool,
}

/// Миссия: где собирается отряд, кто в нём и сколько осталось идти.
#[derive(Serialize)]
pub(crate) struct MissionSnap {
    /// Индекс записи в палитре миссий из `map_meta`.
    pub(crate) def: usize,
    /// Клетка шлюза; `-1`, пока отряд не начали набирать (шлюз не выбран).
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Тиков до возвращения и вся длительность — для полоски. Пока отряд не
    /// ушёл, `left == total`: сбор в срок вылазки не входит.
    pub(crate) left: i32,
    pub(crate) total: i32,
    /// Кто в отряде и сколько нужно всего.
    pub(crate) squad: Vec<String>,
    pub(crate) size: usize,
    /// Отряд ушёл с базы: отозвать его уже нельзя.
    pub(crate) away: bool,
    /// Прогноз исхода — тем же выражением, которым он посчитается на
    /// возвращении (§12.23). Пока отряд на базе, это ещё и предупреждение:
    /// увидел «провал» — успел отозвать.
    pub(crate) strength: i32,
    pub(crate) danger: i32,
    /// Какая доля добычи достанется, в процентах.
    pub(crate) share: i32,
    pub(crate) failed: bool,
}

#[derive(Serialize)]
pub(crate) struct SkillSnap {
    pub(crate) level: i32,
    pub(crate) xp: i32,
    /// Порог следующего уровня; 0 — навык на потолке.
    pub(crate) next: i32,
}

#[derive(Serialize)]
pub(crate) struct BlueprintSnap {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) tile: i16,
    pub(crate) progress: i32,
    pub(crate) total: i32,
    /// Сколько всего штук нужно площадке и сколько уже завезли: пока
    /// `delivered < need`, стройка не начата и чертёж ждёт материал.
    pub(crate) need: i32,
    pub(crate) delivered: i32,
}

/// Куча предметов на полу; `item` — индекс палитры из `map_meta`.
#[derive(Serialize)]
pub(crate) struct StackSnap {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) item: usize,
    pub(crate) count: i32,
    /// Помечена «на склад» — за ней придёт свободный кот. При включённой
    /// автоуборке помечено всё, что лежит вне склада.
    pub(crate) marked: bool,
}
