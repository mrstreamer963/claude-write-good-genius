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
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BuildRect {
    pub(crate) tile: String,
    /// [x, y, w, h] в тайлах
    pub(crate) rect: [i32; 4],
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct UnitDef {
    pub(crate) id: String,
    pub(crate) sprite: String,
    /// [x, y] в тайлах
    pub(crate) pos: [i32; 2],
}
