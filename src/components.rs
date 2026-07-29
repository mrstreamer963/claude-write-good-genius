//! Компоненты ECS и ресурс времени.
//!
//! Данные без поведения: логика живёт в системах (`schedule`, `jobs`,
//! `movement`). Новая фича = новые компоненты здесь + отдельная система там.

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
}

#[derive(Resource)]
pub(crate) struct SimTime {
    pub(crate) tick: u64,
}
