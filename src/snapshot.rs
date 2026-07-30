//! DTO наружу (worker → main), сериализуются serde-wasm-bindgen.
//!
//! Это весь контракт с рендером: `map_meta` и `base_map` уходят по требованию
//! (при росте версии карты), `snapshot` — каждый кадр целиком (§12.8a).

use serde::Serialize;

use crate::ruleset::{PerkDef, SkillDef, TileDef};

#[derive(Serialize)]
pub(crate) struct MapMeta {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) palette: Vec<TileDef>,
    /// Палитра навыков: имена и подписи уходят один раз, как палитра тайлов, —
    /// в снапшоте от навыка остаётся только номер (§12.17).
    pub(crate) skills: Vec<SkillDef>,
    /// Подписи перков; в снапшоте у кота лежат только их `id`.
    pub(crate) perks: Vec<PerkDef>,
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
    /// Сколько лома кот несёт (0 — пустой).
    pub(crate) carrying: i32,
    /// Сколько лома кот берёт за ходку (0 — без предела).
    pub(crate) carry_max: i32,
    /// Навыки в порядке палитры из `map_meta`. Рост, которого не видно, не
    /// существует, — панель выбранного кота единственное место, где он заметен.
    pub(crate) skills: Vec<SkillSnap>,
    /// Статичные перки: они не растут, но объясняют игроку, почему этот кот
    /// таскает больше соседа (§12.17).
    pub(crate) perks: Vec<String>,
    /// Бодрость и её потолок; `energy_max = 0` — усталость выключена рулсетом.
    pub(crate) energy: i32,
    pub(crate) energy_max: i32,
    /// Кот спит здесь и сейчас (в пути к лежанке — ещё нет): состояние
    /// легальное и обратимое, но игрок должен видеть, почему тот не работает.
    pub(crate) sleeping: bool,
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
    /// Сколько лома нужно площадке и сколько уже завезли: пока `delivered < need`,
    /// стройка не начата и чертёж рисуется как ждущий материал.
    pub(crate) need: i32,
    pub(crate) delivered: i32,
}

/// Куча лома на полу.
#[derive(Serialize)]
pub(crate) struct StackSnap {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) count: i32,
    /// Помечена «на склад» — за ней придёт свободный кот. При включённой
    /// автоуборке помечено всё, что лежит вне склада.
    pub(crate) marked: bool,
}
