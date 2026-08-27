//! Поиск пути: BFS по 4-связной сетке проходимых клеток.

use std::collections::VecDeque;

use crate::components::TileRules;
use crate::map::{BaseMap, DIRS};

/// Дерево кратчайших путей: BFS от клетки по проходимым соседям.
///
/// Один обход отвечает сразу на два вопроса — «далеко ли» и «как дойти» — и
/// сразу для всех целей. На этом стоит выбор ближайшего чертежа в `assign_jobs`:
/// сравнить десяток мест работы одним обходом дешевле, чем звать BFS на каждое.
///
/// Проходимость **стартовой** клетки не требуется — на этом свойстве держится
/// выход кота из ямы (шаг наружу из пустоты) и, с §12.142, сход с полки,
/// на которой кот остался от старого сохранения. Менять осторожно.
pub(crate) struct Reach {
    width: i32,
    start: usize,
    /// Предыдущая клетка на кратчайшем пути; -1 — клетка не достигнута.
    came: Vec<i32>,
    /// Число шагов от старта; -1 — клетка не достигнута.
    dist: Vec<i32>,
}

impl Reach {
    /// Обход всей достижимой области.
    pub(crate) fn all(map: &BaseMap, rules: &TileRules, start: (i32, i32)) -> Self {
        Self::explore(map, rules, start, None)
    }

    /// Обход с ранним выходом: как только цель достигнута, дальше не идём.
    pub(crate) fn to(
        map: &BaseMap,
        rules: &TileRules,
        start: (i32, i32),
        goal: (i32, i32),
    ) -> Self {
        Self::explore(map, rules, start, Some(goal))
    }

    fn explore(
        map: &BaseMap,
        rules: &TileRules,
        start: (i32, i32),
        stop_at: Option<(i32, i32)>,
    ) -> Self {
        let (w, h) = (map.width, map.height);
        let start_i = (start.1 * w + start.0) as usize;
        let mut r = Reach {
            width: w,
            start: start_i,
            came: vec![-1; (w * h) as usize],
            dist: vec![-1; (w * h) as usize],
        };
        r.came[start_i] = start_i as i32;
        r.dist[start_i] = 0;
        if stop_at == Some(start) {
            return r;
        }

        let mut queue = VecDeque::new();
        queue.push_back(start);
        while let Some((cx, cy)) = queue.pop_front() {
            let ci = (cy * w + cx) as usize;
            for (dx, dy) in DIRS {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let ni = (ny * w + nx) as usize;
                if r.came[ni] != -1 || !map.walkable(rules, nx, ny) {
                    continue;
                }
                r.came[ni] = ci as i32;
                r.dist[ni] = r.dist[ci] + 1;
                if stop_at == Some((nx, ny)) {
                    return r;
                }
                queue.push_back((nx, ny));
            }
        }
        r
    }

    /// Число шагов до клетки; `None` — недостижима (или вне карты).
    pub(crate) fn dist_at(&self, x: i32, y: i32) -> Option<i32> {
        if x < 0 || y < 0 || x >= self.width {
            return None;
        }
        self.dist
            .get((y * self.width + x) as usize)
            .copied()
            .filter(|&d| d >= 0)
    }

    /// Маршрут в «развёрнутом» виде: `[goal, .., first_step]` (без стартовой
    /// клетки); для цели, равной старту, — пустой.
    pub(crate) fn path_to(&self, x: i32, y: i32) -> Option<Vec<(i32, i32)>> {
        self.dist_at(x, y)?;
        let mut path = Vec::new();
        let mut cur = (y * self.width + x) as usize;
        while cur != self.start {
            path.push(((cur as i32) % self.width, (cur as i32) / self.width));
            cur = self.came[cur] as usize;
        }
        Some(path)
    }
}

/// BFS по проходимым клеткам. Возвращает маршрут в «развёрнутом» виде:
/// `[goal, .., first_step]` (без стартовой клетки), либо None если пути нет.
pub(crate) fn find_path(
    map: &BaseMap,
    rules: &TileRules,
    start: (i32, i32),
    goal: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    Reach::to(map, rules, start, goal).path_to(goal.0, goal.1)
}
