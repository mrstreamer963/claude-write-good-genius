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
    /// Сколько лома уже завезли на площадку. Пока меньше цены тайла, чертёж
    /// строителю не раздаётся — сперва носильщик (§12.15).
    pub(crate) delivered: i32,
    /// Носильщик, который сейчас везёт сюда лом. Один за раз: цена тайла мала,
    /// а резервировать кучи дороже, чем изредка сходить впустую.
    pub(crate) hauler: Option<Entity>,
}

/// Куча лома на клетке пола. Лом — единственный материал POC: в нём и цена
/// постройки, и возврат со сноса (§12.15).
#[derive(Component)]
pub(crate) struct Stack {
    pub(crate) count: i32,
}

/// Лом на руках у кота. Груз сам не падает: кот держит его, пока не донесёт, и
/// поэтому первым берётся за следующую доставку — ему не нужно идти к куче.
#[derive(Component)]
pub(crate) struct Carrying(pub(crate) i32);

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

#[derive(Default, Clone, Copy)]
pub(crate) struct TileRule {
    pub(crate) cost: i32,
    pub(crate) capacity: i32,
}

impl TileRules {
    /// Свойства тайла. У пустоты (`< 0`) и неизвестного индекса всё по нулям:
    /// снос сам по себе материала не требует, он его возвращает.
    fn of(&self, tile: i16) -> TileRule {
        if tile < 0 {
            TileRule::default()
        } else {
            self.0.get(tile as usize).copied().unwrap_or_default()
        }
    }

    pub(crate) fn cost_of(&self, tile: i16) -> i32 {
        self.of(tile).cost
    }

    pub(crate) fn capacity_of(&self, tile: i16) -> i32 {
        self.of(tile).capacity
    }
}
