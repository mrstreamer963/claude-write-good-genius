//! Порядок сноса: волна от берега вглубь сносимой области.
//!
//! Сносимые клетки убираются с глубины наружу, а не в порядке мазка игрока.
//! Иначе снос ближней к выходу клетки отрезает доступ к дальним, и комнату
//! нельзя стереть целиком (§12.12 concept.md).

use std::collections::VecDeque;

use crate::map::{BaseMap, DIRS};

/// Волна сноса: как глубоко каждая клетка сидит в сносимой области и из какого
/// источника до неё дошла волна. Определяет, какие клетки можно сносить сейчас.
///
/// Источники волны («берег») — проходимые клетки, которые остаются: пол без
/// чертежа сноса. Сносим всегда самую глубокую клетку зоны. Это не эвристика,
/// а свойство BFS: кратчайший путь к клетке глубины `d` состоит из клеток
/// глубин `0..d`, значит клетка максимальной глубины не лежит внутри маршрута
/// ни к одной другой сносимой клетке — и её снос никому не перекрывает доступ.
/// Обратный порядок (сносить ближнее к берегу) ровно этим и ломался: комната
/// стиралась частично, а остаток становился недостижим.
///
/// Область, которую сносят целиком, берега не имеет — там волну задают коты
/// внутри неё: снос идёт от них наружу и заканчивается островком пола под
/// котом (снести клетку под собой кот не может, см. §12.11 concept.md).
///
/// Зона — клетка-источник волны. У каждого источника свой фронт: комната с
/// двумя дверями и несвязные области сносятся параллельно и не мешают друг
/// другу — маршрут волны от источника до клетки целиком лежит внутри его зоны.
pub(crate) struct DemolitionFront {
    width: i32,
    height: i32,
    /// Глубина клетки: 0 — берег, -1 — волна не дошла.
    depth: Vec<i32>,
    /// Индекс клетки-источника волны.
    zone: Vec<i32>,
    /// Глубина самой глубокой сносимой клетки зоны (индексируется зоной).
    deepest: Vec<i32>,
}

impl DemolitionFront {
    /// `doomed` — клетки с чертежом сноса, `cats` — позиции всех котов.
    pub(crate) fn new(map: &BaseMap, doomed: &[(i32, i32)], cats: &[(i32, i32)]) -> Self {
        let n = (map.width * map.height) as usize;
        let mut is_doomed = vec![false; n];
        for &(x, y) in doomed {
            if let Some(i) = map.index(x, y) {
                is_doomed[i] = true;
            }
        }

        let mut front = DemolitionFront {
            width: map.width,
            height: map.height,
            depth: vec![-1; n],
            zone: vec![-1; n],
            deepest: vec![-1; n],
        };
        let mut queue = VecDeque::new();

        // Волна от берега — остающегося пола — вглубь сносимой области.
        for (i, (&cell, &doomed)) in map.cells.iter().zip(&is_doomed).enumerate() {
            if cell >= 0 && !doomed {
                front.seed(i, &mut queue);
            }
        }
        front.spread(map, &mut queue);

        // Куда волна не дошла — область без берега; там источники это коты.
        for i in cats.iter().filter_map(|&(x, y)| map.index(x, y)) {
            if map.cells[i] >= 0 && front.depth[i] < 0 {
                front.seed(i, &mut queue);
            }
        }
        front.spread(map, &mut queue);

        for (i, &doomed) in is_doomed.iter().enumerate() {
            if doomed && front.depth[i] >= 0 {
                let z = front.zone[i] as usize;
                front.deepest[z] = front.deepest[z].max(front.depth[i]);
            }
        }
        front
    }

    fn seed(&mut self, i: usize, queue: &mut VecDeque<usize>) {
        self.depth[i] = 0;
        self.zone[i] = i as i32;
        queue.push_back(i);
    }

    fn spread(&mut self, map: &BaseMap, queue: &mut VecDeque<usize>) {
        while let Some(ci) = queue.pop_front() {
            let (cx, cy) = ((ci as i32) % self.width, (ci as i32) / self.width);
            for (dx, dy) in DIRS {
                let (nx, ny) = (cx + dx, cy + dy);
                if !map.walkable(nx, ny) {
                    continue;
                }
                let ni = (ny * self.width + nx) as usize;
                if self.depth[ni] >= 0 {
                    continue;
                }
                self.depth[ni] = self.depth[ci] + 1;
                self.zone[ni] = self.zone[ci];
                queue.push_back(ni);
            }
        }
    }

    /// Глубина клетки; недостижимая волной — «бесконечно глубокая», чтобы при
    /// выборе места работы такая клетка оказывалась последней в очереди.
    pub(crate) fn depth_at(&self, x: i32, y: i32) -> i32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return i32::MAX;
        }
        match self.depth[(y * self.width + x) as usize] {
            -1 => i32::MAX,
            d => d,
        }
    }

    /// Пора ли сносить эту клетку: глубже неё в её зоне сносить нечего.
    ///
    /// Клетка, до которой волна не дошла (ни берега, ни кота в её области),
    /// не мешает никому — её никто и не достанет, гейт для неё пропускается.
    pub(crate) fn is_ready(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return true;
        }
        let i = (y * self.width + x) as usize;
        self.depth[i] < 0 || self.depth[i] == self.deepest[self.zone[i] as usize]
    }
}
