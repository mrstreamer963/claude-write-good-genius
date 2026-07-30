//! Рулсет (YAML) — data-driven контент в стиле OpenXcom Extended.
//!
//! Порядок записей `tiles:` — это индекс палитры: он уходит в рендер, приходит
//! обратно в командах постройки и лежит в клетках карты. Переставить записи в
//! рулсете = переназначить смысл всех уже записанных индексов.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct Ruleset {
    pub(crate) grid: GridDef,
    #[serde(default)]
    pub(crate) tiles: Vec<TileDef>,
    #[serde(default)]
    pub(crate) build: Vec<BuildRect>,
    #[serde(default)]
    pub(crate) scrap: Vec<ScrapDef>,
    #[serde(default)]
    pub(crate) units: Vec<UnitDef>,
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
    /// Сколько лома нужно завезти, чтобы возвести тайл; снос возвращает столько же
    /// (§12.15 concept.md). Ноль = бесплатно, стройка начинается сразу.
    #[serde(default)]
    pub(crate) cost: i32,
    /// Сколько лома клетка хранит. Больше нуля = это склад: коты сами свозят
    /// сюда всё, что валяется на полу (§12.16). Ноль = обычный пол.
    #[serde(default)]
    pub(crate) capacity: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BuildRect {
    pub(crate) tile: String,
    /// [x, y, w, h] в тайлах
    pub(crate) rect: [i32; 4],
}

/// Стартовая куча лома на клетке пола.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ScrapDef {
    /// [x, y] в тайлах
    pub(crate) at: [i32; 2],
    pub(crate) count: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct UnitDef {
    pub(crate) id: String,
    pub(crate) sprite: String,
    /// [x, y] в тайлах
    pub(crate) pos: [i32; 2],
}
