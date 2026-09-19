//! Поиск пути: дерево кратчайших **по времени** маршрутов по 4-связной сетке
//! проходимых клеток (Дейкстра; §12.249).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::components::TileRules;
use crate::map::{BaseMap, DIRS};

/// Дерево кратчайших путей от клетки по проходимым соседям.
///
/// Один обход отвечает сразу на два вопроса — «далеко ли» и «как дойти» — и
/// сразу для всех целей. На этом стоит выбор ближайшего чертежа в `assign_jobs`:
/// сравнить десяток мест работы одним обходом дешевле, чем звать поиск на каждое.
///
/// **Расстояние меряется в тиках, а не в шагах** (§12.249): цена входа в
/// клетку — `BaseMap::step_cost`, то же выражение, которым `move_units`
/// растягивает шаг, и в него входит завал под лапами (§12.35). До §12.249 это
/// был BFS по шагам: кот шёл сквозь полный склад напрямик, а «ближайший кот» у
/// раздатчиков считался тем, кто придёт позже. Кучу, к которой кот послан, он
/// по-прежнему достигает — цена стоит на входе в клетку, и цель от неё не
/// освобождена, — обходит он только кучи **по дороге**. Ничья решается
/// индексом клетки: обход детерминирован (§11).
///
/// Проходимость **стартовой** клетки не требуется — на этом свойстве держится
/// выход кота из ямы (шаг наружу из пустоты) и, с §12.142, сход с полки,
/// на которой кот остался от старого сохранения. Менять осторожно.
pub(crate) struct Reach {
    width: i32,
    start: usize,
    /// Предыдущая клетка на кратчайшем пути; -1 — клетка не достигнута.
    came: Vec<i32>,
    /// Тиков пути от старта; -1 — клетка не достигнута.
    dist: Vec<i32>,
}

impl Reach {
    /// Обход всей достижимой области.
    pub(crate) fn all(map: &BaseMap, rules: &TileRules, start: (i32, i32)) -> Self {
        Self::explore(map, rules, start, None)
    }

    /// Обход с ранним выходом: как только цель **снята с кучи** (её цена уже
    /// окончательна), дальше не идём.
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
        let stop_i = stop_at.map(|(x, y)| (y * w + x) as usize);
        let mut r = Reach {
            width: w,
            start: start_i,
            came: vec![-1; (w * h) as usize],
            dist: vec![-1; (w * h) as usize],
        };
        r.came[start_i] = start_i as i32;
        r.dist[start_i] = 0;
        if stop_i == Some(start_i) {
            return r;
        }

        // Куча по (цена, индекс): при равной цене первой снимается меньшая
        // клетка — ничья решается картой, а не порядком вставки.
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((0i32, start_i)));
        let mut done = vec![false; (w * h) as usize];
        while let Some(Reverse((cost, ci))) = heap.pop() {
            if done[ci] {
                continue;
            }
            done[ci] = true;
            if stop_i == Some(ci) {
                return r;
            }
            let (cx, cy) = ((ci as i32) % w, (ci as i32) / w);
            for (dx, dy) in DIRS {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let ni = (ny * w + nx) as usize;
                if done[ni] || !map.walkable(rules, nx, ny) {
                    continue;
                }
                let next = cost + i32::from(map.step_cost(nx, ny));
                if r.dist[ni] != -1 && r.dist[ni] <= next {
                    continue;
                }
                r.came[ni] = ci as i32;
                r.dist[ni] = next;
                heap.push(Reverse((next, ni)));
            }
        }
        r
    }

    /// Тиков пути до клетки; `None` — недостижима (или вне карты).
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

/// Кратчайший по времени маршрут в «развёрнутом» виде: `[goal, .., first_step]`
/// (без стартовой клетки), либо None если пути нет.
pub(crate) fn find_path(
    map: &BaseMap,
    rules: &TileRules,
    start: (i32, i32),
    goal: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    Reach::to(map, rules, start, goal).path_to(goal.0, goal.1)
}
