//! Тайловая карта базы: проходимость и версия.
//!
//! Проходимость: ходить можно только по построенному полу (индекс тайла >= 0),
//! который вдобавок не заставлен доверху (`solid`, §12.142). Пустые клетки (-1)
//! и стеллажи непроходимы и пересечь их нельзя.

use bevy_ecs::prelude::Resource;

use crate::components::TileRules;

/// Порядок обхода соседей. Фиксирован: от него зависит детерминизм выбора
/// клетки в системах (§11 concept.md).
pub(crate) const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Тайловая карта базы. Значение ячейки — индекс в палитре тайлов, либо -1 (пусто).
#[derive(Resource)]
pub(crate) struct BaseMap {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) cells: Vec<i16>,
    /// Счётчик изменений — единственный сигнал «карта изменилась». На нём висят
    /// гейт повторов приказов (`retry_orders`) и отправка карты в рендер,
    /// поэтому тайлы менять только через `set`: иначе версия не вырастет и оба
    /// механизма молча заглохнут.
    pub(crate) version: u64,
}

impl BaseMap {
    pub(crate) fn empty(width: i32, height: i32) -> Self {
        BaseMap {
            width,
            height,
            cells: vec![-1; (width * height) as usize],
            version: 0,
        }
    }

    pub(crate) fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some((y * self.width + x) as usize)
        }
    }

    pub(crate) fn tile_at(&self, x: i32, y: i32) -> i16 {
        self.index(x, y).map_or(-1, |i| self.cells[i])
    }

    /// Проходима ли клетка: построен пол **и** он не заставлен доверху.
    ///
    /// `solid` (§12.142) — это мебель до потолка: сквозь неё не ходят и на ней
    /// не стоят. Раньше правило касалось только остановки и разбиралось после
    /// факта (`clear_solids`), отчего коты ходили сквозь стеллажи, а к середине
    /// монолита полок подходили через соседние полки (§12.35, §12.111).
    ///
    /// Правила приезжают параметром, а не кэшируются палитрой внутри карты:
    /// `solid` — это контент, и тесты двигают его уже после сборки мира. Кэш
    /// разошёлся бы молча, а параметр компилятор перечислит сам.
    ///
    /// Хранению это не мешает: полка с `capacity` остаётся местом, куда кладут
    /// груз, — просто кладут его с соседней клетки (инвариант 8).
    pub(crate) fn walkable(&self, rules: &TileRules, x: i32, y: i32) -> bool {
        self.index(x, y)
            .is_some_and(|i| self.cells[i] >= 0 && !rules.is_solid(self.cells[i]))
    }

    /// Есть ли на клетке пол вообще — без оглядки на `solid`.
    ///
    /// Не то же, что `walkable`: полка непроходима, но она не пустота, и
    /// оставленное на ней не пропадает (инвариант 8, `settle_stacks`).
    pub(crate) fn has_floor(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.cells[i] >= 0)
    }

    /// Поставить тайл (`tile` — индекс палитры, `-1` = снести). Вернёт true при изменении.
    pub(crate) fn set(&mut self, x: i32, y: i32, tile: i16) -> bool {
        match self.index(x, y) {
            Some(i) if self.cells[i] != tile => {
                self.cells[i] = tile;
                self.version += 1;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn fill_rect(&mut self, rect: [i32; 4], tile: i16) {
        let [x, y, w, h] = rect;
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, tile);
            }
        }
    }
}

/// Клетки прямоугольника [x, y, w, h] построчно. Нулевые и отрицательные
/// размеры дают пустой обход, выход за карту отсекают вызываемые методы.
pub(crate) fn rect_cells(x: i32, y: i32, w: i32, h: i32) -> impl Iterator<Item = (i32, i32)> {
    (0..h).flat_map(move |dy| (0..w).map(move |dx| (x + dx, y + dy)))
}
