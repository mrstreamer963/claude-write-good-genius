//! SP / Эсперы — ядро симуляции.
//!
//! bevy_ecs-мир крутится в WebWorker (WASM). Контент грузится из YAML-рулсета
//! (стиль OpenXcom Extended). Наружу отдаём:
//!   * `map_meta()`      — размеры и палитра тайлов (один раз, для рендера);
//!   * `map_version()`   — счётчик изменений карты (для отправки base_map по требованию);
//!   * `base_map()`      — текущее состояние тайлов базы;
//!   * `add_blueprint()` — поставить чертёж (задачу); выполняют коты, по тикам;
//!   * `plan_demolish()` — ластик: отменить чертёж либо запланировать снос тайла;
//!   * `demolish()`      — мгновенный снос без котов (тесты/отладка);
//!   * `set_target()`    — приказ коту идти в тайл (движение по тикам, отменяет его задачу);
//!   * `tick()`          — один фиксированный шаг симуляции;
//!   * `snapshot()`      — рендерабельные сущности + чертежи (каждый кадр).
//!
//! Проходимость: ходить можно только по построенному полу (индекс тайла >= 0);
//! пустые клетки (-1) непроходимы. Путь ищется BFS по 4-связной сетке.
//!
//! Порядок сноса: сносимые клетки убираются с глубины наружу, а не в порядке
//! мазка игрока. Иначе снос ближней к выходу клетки отрезает доступ к дальним,
//! и комнату нельзя стереть целиком (`DemolitionFront`).
//!
//! Снос под котом разрешён — это штатная механика (дырки в перекрытиях, см.
//! `ideas.md`), а не ошибка. Кот со снесённой клетки выходит своим ходом
//! (`escape_voids`); если выйти некуда, он остаётся в пустоте и помечается
//! флагом `stuck` в снапшоте. Состояние обратимо: игрок ставит пол рядом,
//! и кот выбирается — в том числе построив этот пол сам, изнутри.
//!
//! Джобы постройки: чертёж — сущность `Blueprint`. Свободный (простаивающий) кот
//! назначается на ближайший достижимый чертёж, идёт к соседней проходимой клетке
//! и «работает» BUILD_TIME тиков, после чего тайл возводится.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;

/// Тиков между шагами юнита (при BASE_TPS=6 и периоде 1 — ~3 тайла/сек на ×1).
const MOVE_PERIOD: u8 = 1;
/// Тиков работы, чтобы возвести один тайл.
const BUILD_TIME: i32 = 12;

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

// ------------------------------------------------------------------
// Рулсет (YAML) — data-driven контент.
// ------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Ruleset {
    grid: GridDef,
    #[serde(default)]
    tiles: Vec<TileDef>,
    #[serde(default)]
    build: Vec<BuildRect>,
    #[serde(default)]
    units: Vec<UnitDef>,
}

#[derive(Debug, Deserialize, Clone)]
struct GridDef {
    width: i32,
    height: i32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
struct TileDef {
    id: String,
    #[serde(default)]
    label: String,
    color: String,
}

#[derive(Debug, Deserialize, Clone)]
struct BuildRect {
    tile: String,
    /// [x, y, w, h] в тайлах
    rect: [i32; 4],
}

#[derive(Debug, Deserialize, Clone)]
struct UnitDef {
    id: String,
    sprite: String,
    /// [x, y] в тайлах
    pos: [i32; 2],
}

// ------------------------------------------------------------------
// Компоненты и ресурсы.
// ------------------------------------------------------------------

#[derive(Component)]
struct Position {
    x: i32,
    y: i32,
}

#[derive(Component)]
struct Renderable {
    sprite: String,
}

#[derive(Component)]
struct UnitId(String);

/// Оставшийся маршрут. Хранится «развёрнуто»: следующий шаг — последний элемент
/// (`pop()` = следующая клетка), первый элемент — конечная цель.
#[derive(Component)]
struct Path {
    steps: Vec<(i32, i32)>,
}

/// Обратный отсчёт тиков до следующего шага.
#[derive(Component)]
struct MoveCooldown(u8);

/// Кот назначен на джоб постройки (ссылка на сущность-чертёж).
#[derive(Component)]
struct Assignment(Entity);

/// Приказ игрока «иди туда». Хранится, даже если путь сейчас не найден:
/// `retry_orders` перепроложит маршрут, как только карта изменится —
/// например, после постройки коридора.
#[derive(Component)]
struct Order {
    x: i32,
    y: i32,
    /// Версия карты, на которой последний раз пытались проложить маршрут.
    /// Проходимость зависит только от тайлов (коты друг друга не блокируют),
    /// поэтому повтор на неизменившейся карте заведомо даст тот же результат —
    /// и пропускается, чтобы не гонять BFS каждый тик.
    tried_version: u64,
}

/// Чертёж — запланированная постройка тайла.
#[derive(Component)]
struct Blueprint {
    x: i32,
    y: i32,
    tile: i16,
    progress: i32,
    assignee: Option<Entity>,
}

/// Тайловая карта базы. Значение ячейки — индекс в палитре тайлов, либо -1 (пусто).
#[derive(Resource)]
struct BaseMap {
    width: i32,
    height: i32,
    cells: Vec<i16>,
    version: u64,
}

impl BaseMap {
    fn empty(width: i32, height: i32) -> Self {
        BaseMap {
            width,
            height,
            cells: vec![-1; (width * height) as usize],
            version: 0,
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some((y * self.width + x) as usize)
        }
    }

    fn tile_at(&self, x: i32, y: i32) -> i16 {
        self.index(x, y).map_or(-1, |i| self.cells[i])
    }

    /// Проходима ли клетка (построен ли пол).
    fn walkable(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.cells[i] >= 0)
    }

    /// Поставить тайл (`tile` — индекс палитры, `-1` = снести). Вернёт true при изменении.
    fn set(&mut self, x: i32, y: i32, tile: i16) -> bool {
        match self.index(x, y) {
            Some(i) if self.cells[i] != tile => {
                self.cells[i] = tile;
                self.version += 1;
                true
            }
            _ => false,
        }
    }

    fn fill_rect(&mut self, rect: [i32; 4], tile: i16) {
        let [x, y, w, h] = rect;
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, tile);
            }
        }
    }
}

/// BFS по проходимым клеткам. Возвращает маршрут в «развёрнутом» виде:
/// `[goal, .., first_step]` (без стартовой клетки), либо None если пути нет.
fn find_path(map: &BaseMap, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    if start == goal {
        return Some(Vec::new());
    }
    let (w, h) = (map.width, map.height);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let start_i = idx(start.0, start.1);

    let mut came: Vec<i32> = vec![-1; (w * h) as usize];
    came[start_i] = start_i as i32;
    let mut queue = VecDeque::new();
    queue.push_back(start);

    while let Some((cx, cy)) = queue.pop_front() {
        for (dx, dy) in DIRS {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let ni = idx(nx, ny);
            if came[ni] != -1 || !map.walkable(nx, ny) {
                continue;
            }
            came[ni] = idx(cx, cy) as i32;
            if (nx, ny) == goal {
                let mut path = Vec::new();
                let mut cur = ni;
                while cur != start_i {
                    path.push(((cur as i32) % w, (cur as i32) / w));
                    cur = came[cur] as usize;
                }
                return Some(path);
            }
            queue.push_back((nx, ny));
        }
    }
    None
}

/// Найти клетку, откуда можно выполнить чертёж (сам тайл, если проходим, либо
/// сосед), и маршрут кота до неё. Возвращает (клетка, маршрут).
///
/// `bp_tile` — что планируется на клетке; для сноса (`< 0`) стоять на самой цели
/// нельзя. Цель сноса всегда проходима, иначе сносить было бы нечего, поэтому без
/// этого правила кот выбрал бы её первой и снёс пол под собой. В пустоту кот
/// попадает действием игрока, а не в порядке штатной работы (см. §12.10 concept.md).
///
/// Снос вдобавок разбирается со стороны берега (`front`): встав на клетку глубже
/// цели, кот работал бы «из-за» дыры и после сноса остался бы по дальнюю её
/// сторону — отрезанным от остатка собственной работы. Клетка на шаг ближе к
/// берегу есть у любой цели (это её родитель в волне) и по определению уйдёт
/// позже неё.
fn path_to_build_spot(
    map: &BaseMap,
    from: (i32, i32),
    bp: (i32, i32),
    bp_tile: i16,
    front: Option<&DemolitionFront>,
) -> Option<((i32, i32), Vec<(i32, i32)>)> {
    let mut spots = Vec::new();
    if bp_tile >= 0 && map.walkable(bp.0, bp.1) {
        spots.push(bp);
    }
    for (dx, dy) in DIRS {
        let n = (bp.0 + dx, bp.1 + dy);
        if map.walkable(n.0, n.1) {
            spots.push(n);
        }
    }
    if let Some(f) = front.filter(|_| bp_tile < 0) {
        spots.sort_by_key(|&(x, y)| f.depth_at(x, y));
    }
    for spot in spots {
        if spot == from {
            return Some((spot, Vec::new()));
        }
        if let Some(path) = find_path(map, from, spot) {
            return Some((spot, path));
        }
    }
    None
}

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
struct DemolitionFront {
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
    fn new(map: &BaseMap, doomed: &[(i32, i32)], cats: &[(i32, i32)]) -> Self {
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
    fn depth_at(&self, x: i32, y: i32) -> i32 {
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
    fn is_ready(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return true;
        }
        let i = (y * self.width + x) as usize;
        self.depth[i] < 0 || self.depth[i] == self.deepest[self.zone[i] as usize]
    }
}

#[derive(Resource)]
struct SimTime {
    tick: u64,
}

// ------------------------------------------------------------------
// Системы (гоняются цепочкой каждый тик).
// ------------------------------------------------------------------

fn advance_time(mut time: ResMut<SimTime>) {
    time.tick += 1;
}

/// Назначает простаивающих котов (без задачи и без маршрута) на ближайшие
/// достижимые чертежи и отправляет их к месту постройки.
///
/// Кот с невыполнимым сейчас приказом тоже считается свободным. Иначе он бы
/// никогда не взялся строить — в том числе тот тайл, который открывает ему
/// выход из запертой комнаты. Приказ при этом не теряется: `retry_orders`
/// подхватит его, когда кот освободится от задачи.
///
/// Чертежи сноса дополнительно проходят через `DemolitionFront`: сносить можно
/// только самые глубокие клетки области, иначе снос отрезает доступ к остатку.
/// Порядок стройки не трогаем — построенный пол доступ не закрывает, а открывает.
fn assign_jobs(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut blueprints: Query<(Entity, &mut Blueprint)>,
    cats: Query<&Position, With<UnitId>>,
    free_cats: Query<(Entity, &Position), (With<UnitId>, Without<Assignment>, Without<Path>)>,
) {
    let doomed: Vec<(i32, i32)> = blueprints
        .iter()
        .filter(|(_, bp)| bp.tile < 0)
        .map(|(_, bp)| (bp.x, bp.y))
        .collect();
    let front = (!doomed.is_empty()).then(|| {
        let positions: Vec<(i32, i32)> = cats.iter().map(|p| (p.x, p.y)).collect();
        DemolitionFront::new(&map, &doomed, &positions)
    });

    let mut free: Vec<(Entity, (i32, i32))> =
        free_cats.iter().map(|(e, p)| (e, (p.x, p.y))).collect();

    for (bp_e, mut bp) in &mut blueprints {
        // Снос не на фронте своей зоны ждёт: убрать эту клетку сейчас — значит
        // отрезать доступ к тем, что глубже.
        let waiting = bp.tile < 0 && front.as_ref().is_some_and(|f| !f.is_ready(bp.x, bp.y));
        if let Some(cat) = bp.assignee {
            // Игрок мог дорисовать область уже начатого сноса — тогда джоб
            // встаёт на паузу (прогресс сохраняется), а кот освобождается.
            if waiting {
                bp.assignee = None;
                commands
                    .entity(cat)
                    .remove::<(Assignment, Path, MoveCooldown)>();
            }
            continue;
        }
        if waiting || free.is_empty() {
            continue;
        }
        let mut chosen = None;
        for (i, (cat_e, cat_pos)) in free.iter().enumerate() {
            if let Some((_spot, path)) =
                path_to_build_spot(&map, *cat_pos, (bp.x, bp.y), bp.tile, front.as_ref())
            {
                chosen = Some((i, *cat_e, path));
                break;
            }
        }
        if let Some((i, cat_e, path)) = chosen {
            free.remove(i);
            bp.assignee = Some(cat_e);
            commands.entity(cat_e).insert((
                Assignment(bp_e),
                Path { steps: path },
                MoveCooldown(0),
            ));
        }
    }
}

/// Двигает юнитов по маршруту; на прибытии снимает компоненты движения.
fn move_units(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Position, &mut Path, &mut MoveCooldown)>,
) {
    for (e, mut pos, mut path, mut cd) in &mut q {
        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
            continue;
        }
        if cd.0 > 0 {
            cd.0 -= 1;
            continue;
        }

        let next = *path.steps.last().unwrap();
        if !map.walkable(next.0, next.1) {
            let goal = *path.steps.first().unwrap();
            match find_path(&map, (pos.x, pos.y), goal) {
                Some(p) => path.steps = p,
                None => path.steps.clear(),
            }
            cd.0 = MOVE_PERIOD;
            continue;
        }

        path.steps.pop();
        pos.x = next.0;
        pos.y = next.1;
        cd.0 = MOVE_PERIOD;

        if path.steps.is_empty() {
            commands.entity(e).remove::<(Path, MoveCooldown)>();
        }
    }
}

/// Пытается проложить маршрут котам с активным приказом (`Order`), у которых
/// сейчас нет пути — например, приказ был отдан до постройки коридора.
/// Снимает `Order`, когда цель достигнута.
///
/// Коты за стройкой (`Assignment`) пропускаются: приказ не должен срывать кота
/// с начатой задачи. Он подхватится сам, как только `work_jobs` снимет задачу.
fn retry_orders(
    map: Res<BaseMap>,
    mut commands: Commands,
    mut q: Query<(Entity, &Position, &mut Order), (Without<Path>, Without<Assignment>)>,
) {
    for (e, pos, mut order) in &mut q {
        if (pos.x, pos.y) == (order.x, order.y) {
            commands.entity(e).remove::<Order>();
            continue;
        }
        // Карта не менялась с прошлой попытки — результат будет тот же.
        if order.tried_version == map.version {
            continue;
        }
        order.tried_version = map.version;
        if let Some(path) = find_path(&map, (pos.x, pos.y), (order.x, order.y)) {
            commands
                .entity(e)
                .insert((Path { steps: path }, MoveCooldown(0)));
        }
    }
}

/// Кот ничего не может сделать сам — для подсветки в UI.
///
/// Два случая: замурован в пустоте без проходимых соседей, либо его приказ
/// сейчас невыполним (второе условие — ровно то, на котором `retry_orders`
/// раз за разом не находит путь; кот за стройкой не считается застрявшим).
fn is_stuck(
    map: &BaseMap,
    pos: &Position,
    order: Option<&Order>,
    path: Option<&Path>,
    assignment: Option<&Assignment>,
) -> bool {
    let entombed = !map.walkable(pos.x, pos.y)
        && !DIRS
            .iter()
            .any(|(dx, dy)| map.walkable(pos.x + dx, pos.y + dy));
    let order_stalled = order.is_some() && path.is_none() && assignment.is_none();
    entombed || order_stalled
}

/// Цепочка систем одного тика. Вынесена отдельно, чтобы тесты гоняли ровно тот
/// же порядок, что и боевая симуляция.
///
/// `escape_voids` — последним: если приказ или джоб уже дали маршрут, он и
/// выводит кота из ямы, отдельный шаг не нужен.
fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            advance_time,
            assign_jobs,
            move_units,
            work_jobs,
            retry_orders,
            escape_voids,
        )
            .chain(),
    );
    schedule
}

/// Выводит кота со снесённой клетки на соседний пол обычным шагом.
///
/// Пересечь пустоту нельзя (`move_units` шагает только на проходимое), поэтому
/// «выход» — это всегда шаг на соседа; искать дальше смысла нет. Коты с
/// маршрутом или задачей выбираются сами: `find_path` не требует проходимости
/// стартовой клетки. Остаётся простаивающий кот — им и занимается эта система.
/// Если проходимых соседей нет, кот остаётся в пустоте (честное «замурован»)
/// и помечается флагом `stuck` в снапшоте.
fn escape_voids(
    map: Res<BaseMap>,
    mut commands: Commands,
    q: Query<(Entity, &Position), (With<UnitId>, Without<Path>)>,
) {
    for (e, pos) in &q {
        if map.walkable(pos.x, pos.y) {
            continue;
        }
        // Порядок DIRS фиксирован, значит выбор соседа детерминирован (§11).
        if let Some(step) = DIRS
            .iter()
            .map(|(dx, dy)| (pos.x + dx, pos.y + dy))
            .find(|(nx, ny)| map.walkable(*nx, *ny))
        {
            commands
                .entity(e)
                .insert((Path { steps: vec![step] }, MoveCooldown(0)));
        }
    }
}

/// Коты, добравшиеся до чертежа, «работают» до готовности; затем тайл возводится.
fn work_jobs(
    mut map: ResMut<BaseMap>,
    mut commands: Commands,
    cats: Query<(Entity, &Position, &Assignment, Option<&Path>)>,
    mut blueprints: Query<&mut Blueprint>,
) {
    for (cat_e, pos, assign, path) in &cats {
        let Ok(mut bp) = blueprints.get_mut(assign.0) else {
            commands.entity(cat_e).remove::<Assignment>();
            continue;
        };

        let in_range = (pos.x - bp.x).abs() + (pos.y - bp.y).abs() <= 1;
        if in_range {
            bp.progress += 1;
            if bp.progress >= BUILD_TIME {
                map.set(bp.x, bp.y, bp.tile);
                commands.entity(assign.0).despawn();
                commands.entity(cat_e).remove::<Assignment>();
            }
        } else if path.is_none() {
            // Дошёл, но не в радиусе (маршрут оборвался) — освобождаем джоб.
            bp.assignee = None;
            commands.entity(cat_e).remove::<Assignment>();
        }
    }
}

// ------------------------------------------------------------------
// DTO наружу (worker -> main), сериализуются serde-wasm-bindgen.
// ------------------------------------------------------------------

#[derive(Serialize)]
struct MapMeta {
    width: i32,
    height: i32,
    palette: Vec<TileDef>,
}

#[derive(Serialize)]
struct BaseMapDto<'a> {
    width: i32,
    height: i32,
    cells: &'a [i16],
}

#[derive(Serialize)]
struct Snapshot {
    tick: u64,
    entities: Vec<EntitySnap>,
    blueprints: Vec<BlueprintSnap>,
}

#[derive(Serialize)]
struct EntitySnap {
    id: String,
    sprite: String,
    x: i32,
    y: i32,
    /// Кот ничего не может сделать сам: либо замурован в пустоте без проходимых
    /// соседей, либо его приказ сейчас невыполним. Для подсветки в UI —
    /// состояние легальное (см. снос пола), но игрок должен его видеть.
    stuck: bool,
}

#[derive(Serialize)]
struct BlueprintSnap {
    x: i32,
    y: i32,
    tile: i16,
    progress: i32,
    total: i32,
}

// ------------------------------------------------------------------
// WASM-интерфейс.
// ------------------------------------------------------------------

#[wasm_bindgen]
pub struct Sim {
    world: World,
    schedule: Schedule,
    palette: Vec<TileDef>,
    width: i32,
    height: i32,
}

impl Sim {
    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    /// Сущность-чертёж на клетке (x, y), если есть.
    fn blueprint_at(&mut self, x: i32, y: i32) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &Blueprint)>();
        for (e, bp) in q.iter(&self.world) {
            if bp.x == x && bp.y == y {
                return Some(e);
            }
        }
        None
    }

    /// Снять чертёж с клетки и освободить назначенного на него кота.
    /// Отмена плана мгновенна и бесплатна — строить ещё не начинали.
    fn cancel_blueprint(&mut self, x: i32, y: i32) -> bool {
        let Some(e) = self.blueprint_at(x, y) else {
            return false;
        };
        if let Some(cat) = self.world.get::<Blueprint>(e).and_then(|bp| bp.assignee) {
            self.world
                .entity_mut(cat)
                .remove::<(Assignment, Path, MoveCooldown)>();
        }
        self.world.entity_mut(e).despawn();
        true
    }
}

#[wasm_bindgen]
impl Sim {
    /// Создать симуляцию из текста YAML-рулсета.
    #[wasm_bindgen(constructor)]
    pub fn new(ruleset_yaml: &str) -> Result<Sim, JsValue> {
        console_error_panic_hook::set_once();

        let rs: Ruleset = serde_yaml::from_str(ruleset_yaml)
            .map_err(|e| JsValue::from_str(&format!("ruleset parse error: {e}")))?;

        let (w, h) = (rs.grid.width, rs.grid.height);
        let tile_index = |id: &str| rs.tiles.iter().position(|t| t.id == id).map(|i| i as i16);

        let mut map = BaseMap::empty(w, h);
        for b in &rs.build {
            if let Some(idx) = tile_index(&b.tile) {
                map.fill_rect(b.rect, idx);
            }
        }

        let mut world = World::new();
        world.insert_resource(map);
        world.insert_resource(SimTime { tick: 0 });

        for u in &rs.units {
            world.spawn((
                UnitId(u.id.clone()),
                Renderable {
                    sprite: u.sprite.clone(),
                },
                Position {
                    x: u.pos[0],
                    y: u.pos[1],
                },
            ));
        }

        let schedule = build_schedule();
        Ok(Sim {
            world,
            schedule,
            palette: rs.tiles,
            width: w,
            height: h,
        })
    }

    /// Размеры и палитра тайлов — отдаём один раз для настройки рендера.
    pub fn map_meta(&self) -> Result<JsValue, JsValue> {
        let meta = MapMeta {
            width: self.width,
            height: self.height,
            palette: self.palette.clone(),
        };
        serde_wasm_bindgen::to_value(&meta).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Счётчик изменений карты (воркер шлёт base_map, когда он вырос).
    pub fn map_version(&self) -> f64 {
        self.world.resource::<BaseMap>().version as f64
    }

    /// Текущее состояние тайлов базы.
    pub fn base_map(&self) -> Result<JsValue, JsValue> {
        let map = self.world.resource::<BaseMap>();
        let dto = BaseMapDto {
            width: map.width,
            height: map.height,
            cells: &map.cells,
        };
        serde_wasm_bindgen::to_value(&dto).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Поставить чертёж (джоб) `tile` на клетку (x, y). Выполняют коты, по тикам.
    /// `tile = -1` — чертёж сноса: кот придёт на соседнюю клетку и уберёт пол.
    /// Вернёт true, если чертёж добавлен/обновлён.
    pub fn add_blueprint(&mut self, x: i32, y: i32, tile: i32) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let t = tile as i16;
        if self.world.resource::<BaseMap>().tile_at(x, y) == t {
            return false; // уже построено этим тайлом
        }
        if let Some(e) = self.blueprint_at(x, y) {
            let mut bp = self.world.get_mut::<Blueprint>(e).unwrap();
            if bp.tile != t {
                bp.tile = t;
                bp.progress = 0;
            }
            return true;
        }
        self.world.spawn(Blueprint {
            x,
            y,
            tile: t,
            progress: 0,
            assignee: None,
        });
        true
    }

    /// Ластик игрока: отменить чертёж, либо запланировать снос построенного тайла.
    ///
    /// Отмена плана мгновенна — строить ещё не начинали, и большая часть «ой, не
    /// туда» приходится именно на неё. Снос готового тайла идёт через ту же
    /// очередь, что и стройка: чертёж с `tile = -1`, кот приходит на соседнюю
    /// клетку и работает. Повторный ластик по клетке с чертежом сноса отменяет
    /// его. Вернёт true, если что-то изменилось.
    pub fn plan_demolish(&mut self, x: i32, y: i32) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        if self.cancel_blueprint(x, y) {
            return true;
        }
        self.add_blueprint(x, y, -1)
    }

    /// Мгновенно снести тайл вместе с чертежом, без участия котов.
    ///
    /// Ластик игрока ходит через `plan_demolish`; этот путь оставлен для тестов
    /// и отладки. Вернёт true при изменении.
    pub fn demolish(&mut self, x: i32, y: i32) -> bool {
        let mut changed = self.cancel_blueprint(x, y);
        if self.world.resource_mut::<BaseMap>().set(x, y, -1) {
            changed = true;
        }
        changed
    }

    /// Приказ коту `unit_id` идти в тайл (x, y). Отменяет его джоб постройки, если был.
    /// Приказ сохраняется, даже если путь пока не найден — маршрут будет
    /// перепроложен автоматически, как только карта изменится (см. `retry_orders`).
    /// Вернёт true, если приказ принят (цель проходима), false — если цель не тайл-пол
    /// или юнит не найден.
    pub fn set_target(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        let mut found = None;
        {
            let mut q = self.world.query::<(Entity, &UnitId, &Position)>();
            for (e, id, p) in q.iter(&self.world) {
                if id.0 == unit_id {
                    found = Some((e, p.x, p.y));
                    break;
                }
            }
        }
        let Some((entity, sx, sy)) = found else {
            return false;
        };

        if !self.world.resource::<BaseMap>().walkable(x, y) {
            return false;
        }
        let map_version = self.world.resource::<BaseMap>().version;
        let path = find_path(self.world.resource::<BaseMap>(), (sx, sy), (x, y));

        // Снять текущую задачу постройки (освободить чертёж).
        if let Some(bp_e) = self.world.get::<Assignment>(entity).map(|a| a.0) {
            if let Some(mut bp) = self.world.get_mut::<Blueprint>(bp_e) {
                bp.assignee = None;
            }
            self.world.entity_mut(entity).remove::<Assignment>();
        }

        // Приказ сохраняется даже без пути прямо сейчас — `retry_orders`
        // перепроложит маршрут при следующем изменении карты (например, после
        // постройки коридора, открывающего доступ к цели).
        self.world.entity_mut(entity).insert(Order {
            x,
            y,
            tried_version: map_version,
        });
        match path {
            Some(p) => {
                self.world
                    .entity_mut(entity)
                    .insert((Path { steps: p }, MoveCooldown(0)));
            }
            None => {
                self.world.entity_mut(entity).remove::<(Path, MoveCooldown)>();
            }
        }
        true
    }

    /// Один фиксированный шаг симуляции.
    pub fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }

    /// Рендерабельные сущности + чертежи (для PixiJS).
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        let tick = self.world.resource::<SimTime>().tick;

        let mut entities = Vec::new();
        {
            let mut q = self.world.query::<(
                &UnitId,
                &Renderable,
                &Position,
                Option<&Order>,
                Option<&Path>,
                Option<&Assignment>,
            )>();
            let map = self.world.resource::<BaseMap>();
            for (id, r, p, order, path, assignment) in q.iter(&self.world) {
                entities.push(EntitySnap {
                    id: id.0.clone(),
                    sprite: r.sprite.clone(),
                    x: p.x,
                    y: p.y,
                    stuck: is_stuck(map, p, order, path, assignment),
                });
            }
        }

        let mut blueprints = Vec::new();
        {
            let mut q = self.world.query::<&Blueprint>();
            for bp in q.iter(&self.world) {
                blueprints.push(BlueprintSnap {
                    x: bp.x,
                    y: bp.y,
                    tile: bp.tile,
                    progress: bp.progress,
                    total: BUILD_TIME,
                });
            }
        }

        serde_wasm_bindgen::to_value(&Snapshot {
            tick,
            entities,
            blueprints,
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

// ------------------------------------------------------------------
// Тесты. Гоняют ту же цепочку систем (`build_schedule`), что и боевая
// симуляция, поэтому проверяют реальные взаимодействия, а не их копию.
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Строки ASCII-схемы из файла `src/test_maps/*.map` (через `include_str!`
    /// на вызывающей стороне) — для схем, которые `rustfmt` иначе схлопывает
    /// в одну строку и делает нечитаемыми.
    fn rows_from(text: &'static str) -> Vec<&'static str> {
        text.lines().filter(|l| !l.is_empty()).collect()
    }

    /// Собирает `Sim` из ASCII-схемы, минуя YAML: `#` — пустота (непроходимо),
    /// `.` — пол, любая другая буква — пол с котом под этим id.
    fn sim_from(rows: &[&str]) -> Sim {
        let height = rows.len() as i32;
        let width = rows[0].len() as i32;
        let mut map = BaseMap::empty(width, height);
        let mut world = World::new();

        for (y, row) in rows.iter().enumerate() {
            assert_eq!(row.len() as i32, width, "строки схемы разной длины");
            for (x, ch) in row.chars().enumerate() {
                let (x, y) = (x as i32, y as i32);
                if ch == '#' {
                    continue;
                }
                map.set(x, y, 0);
                if ch != '.' {
                    world.spawn((
                        UnitId(ch.to_string()),
                        Renderable {
                            sprite: "cat".to_string(),
                        },
                        Position { x, y },
                    ));
                }
            }
        }

        world.insert_resource(map);
        world.insert_resource(SimTime { tick: 0 });
        Sim {
            world,
            schedule: build_schedule(),
            palette: vec![TileDef {
                id: "floor".to_string(),
                label: "Пол".to_string(),
                color: "#000000".to_string(),
            }],
            width,
            height,
        }
    }

    impl Sim {
        fn tick_n(&mut self, n: usize) {
            for _ in 0..n {
                self.tick();
            }
        }

        fn pos_of(&mut self, unit: &str) -> (i32, i32) {
            let mut q = self.world.query::<(&UnitId, &Position)>();
            q.iter(&self.world)
                .find(|(id, _)| id.0 == unit)
                .map(|(_, p)| (p.x, p.y))
                .expect("кот не найден")
        }

        fn stuck_of(&mut self, unit: &str) -> bool {
            let mut q = self.world.query::<(
                &UnitId,
                &Position,
                Option<&Order>,
                Option<&Path>,
                Option<&Assignment>,
            )>();
            let map = self.world.resource::<BaseMap>();
            q.iter(&self.world)
                .find(|(id, ..)| id.0 == unit)
                .map(|(_, p, o, path, a)| is_stuck(map, p, o, path, a))
                .expect("кот не найден")
        }

        /// `tried_version` приказа, если приказ ещё висит.
        fn order_tried_version(&mut self, unit: &str) -> Option<u64> {
            let mut q = self.world.query::<(&UnitId, Option<&Order>)>();
            q.iter(&self.world)
                .find(|(id, _)| id.0 == unit)
                .and_then(|(_, o)| o.map(|o| o.tried_version))
        }

        fn has_assignment(&mut self, unit: &str) -> bool {
            let mut q = self.world.query::<(&UnitId, Option<&Assignment>)>();
            q.iter(&self.world)
                .find(|(id, _)| id.0 == unit)
                .map(|(_, a)| a.is_some())
                .unwrap_or(false)
        }

        fn has_path(&mut self, unit: &str) -> bool {
            let mut q = self.world.query::<(&UnitId, Option<&Path>)>();
            q.iter(&self.world)
                .find(|(id, _)| id.0 == unit)
                .map(|(_, p)| p.is_some())
                .unwrap_or(false)
        }

        fn tile(&self, x: i32, y: i32) -> i16 {
            self.world.resource::<BaseMap>().tile_at(x, y)
        }

        fn map_ver(&self) -> u64 {
            self.world.resource::<BaseMap>().version
        }

        /// Изменить тайл в обход чертежей — для проверки реакции на смену карты.
        fn force_tile(&mut self, x: i32, y: i32, tile: i16) {
            self.world.resource_mut::<BaseMap>().set(x, y, tile);
        }

        /// Мазок ластиком по прямоугольнику [x, y, w, h] — как ведёт мышью игрок.
        fn plan_demolish_rect(&mut self, rect: [i32; 4]) {
            let [x, y, w, h] = rect;
            for dy in 0..h {
                for dx in 0..w {
                    self.plan_demolish(x + dx, y + dy);
                }
            }
        }

        /// Сколько клеток пола осталось в прямоугольнике.
        fn floors_left(&self, rect: [i32; 4]) -> i32 {
            let [x, y, w, h] = rect;
            let map = self.world.resource::<BaseMap>();
            (0..h)
                .flat_map(|dy| (0..w).map(move |dx| (dx, dy)))
                .filter(|(dx, dy)| map.tile_at(x + dx, y + dy) >= 0)
                .count() as i32
        }
    }

    // --- пути -------------------------------------------------------------

    #[test]
    fn path_crosses_connected_floor() {
        let sim = sim_from(&["#####", "#...#", "#####"]);
        let map = sim.world.resource::<BaseMap>();
        let path = find_path(map, (1, 1), (3, 1)).expect("путь должен быть");
        // Маршрут развёрнут: последний элемент — следующий шаг, первый — цель.
        assert_eq!(path.first(), Some(&(3, 1)));
        assert_eq!(path.last(), Some(&(2, 1)));
    }

    #[test]
    fn no_path_across_gap() {
        let sim = sim_from(&["#####", "#.#.#", "#####"]);
        let map = sim.world.resource::<BaseMap>();
        assert!(find_path(map, (1, 1), (3, 1)).is_none());
    }

    /// Свойство, на котором держится выход из ямы: старт может быть непроходим.
    /// Кот на снесённой клетке всё ещё умеет проложить маршрут наружу.
    #[test]
    fn path_starts_from_unwalkable_tile() {
        let sim = sim_from(&["#####", "#...#", "#####"]);
        let mut sim = sim;
        sim.force_tile(1, 1, -1);
        let map = sim.world.resource::<BaseMap>();
        assert!(!map.walkable(1, 1));
        assert!(find_path(map, (1, 1), (3, 1)).is_some());
    }

    // --- снос под котом ---------------------------------------------------

    #[test]
    fn cat_steps_out_of_demolished_tile() {
        let mut sim = sim_from(&["#####", "#a..#", "#####"]);
        assert!(sim.demolish(1, 1));
        sim.tick_n(4);
        assert_eq!(sim.pos_of("a"), (2, 1), "кот должен уйти на соседний пол");
        assert!(!sim.stuck_of("a"));
    }

    #[test]
    fn entombed_cat_stays_and_is_flagged() {
        // Одиночная клетка пола: снести её — и шагнуть некуда.
        let mut sim = sim_from(&["###", "#a#", "###"]);
        assert!(sim.demolish(1, 1));
        sim.tick_n(10);
        assert_eq!(sim.pos_of("a"), (1, 1), "выхода нет — кот остаётся");
        assert!(sim.stuck_of("a"), "замурованный кот должен помечаться stuck");
    }

    #[test]
    fn entombed_cat_recovers_when_floor_returns() {
        let mut sim = sim_from(&["###", "#a#", "###"]);
        sim.demolish(1, 1);
        sim.tick_n(5);
        assert!(sim.stuck_of("a"));

        sim.force_tile(1, 0, 0); // игрок вернул пол рядом
        sim.tick_n(6);
        assert_eq!(sim.pos_of("a"), (1, 0), "кот должен выбраться на новый пол");
        assert!(!sim.stuck_of("a"));
    }

    // --- приказы ----------------------------------------------------------

    /// Регрессия errors.md №1: приказ, отданный до появления прохода, должен
    /// исполниться сам, как только карта откроется.
    #[test]
    fn order_resumes_after_map_opens() {
        let mut sim = sim_from(&["#######", "#a.#..#", "#######"]);
        assert!(sim.set_target("a", 5, 1), "приказ на проходимую клетку принят");
        sim.tick_n(10);
        assert_eq!(sim.pos_of("a"), (1, 1), "пути нет — кот стоит");
        assert!(sim.stuck_of("a"), "невыполнимый приказ помечается stuck");

        sim.force_tile(3, 1, 0); // построили перемычку
        sim.tick_n(30);
        assert_eq!(sim.pos_of("a"), (5, 1), "после открытия прохода кот дошёл");
        assert!(!sim.stuck_of("a"));
    }

    /// Регрессия errors.md №2: кот с невыполнимым приказом обязан оставаться
    /// работоспособным. Чертёж в (0,1) достижим только изнутри левой комнаты,
    /// так что построить его может только запертый кот — и только если приказ
    /// не исключает его из назначения на джобы.
    #[test]
    fn trapped_cat_with_stalled_order_still_builds() {
        let mut sim = sim_from(&["#######", "#a.#.b#", "#######"]);
        sim.set_target("a", 5, 1); // за стеной — недостижимо
        sim.tick_n(5);
        assert!(sim.stuck_of("a"));

        assert!(sim.add_blueprint(0, 1, 0));
        sim.tick_n(200);

        assert_eq!(sim.tile(0, 1), 0, "запертый кот должен построить тайл");
        assert_eq!(sim.pos_of("a"), (1, 1), "строит из своей комнаты");
    }

    /// Приказ не должен срывать кота с начатой стройки: сначала джоб, потом
    /// приказ. Проверяем, что открытие прохода посреди работы не бросает стройку.
    #[test]
    fn order_does_not_abandon_active_build() {
        let mut sim = sim_from(&["#######", "#a.#.b#", "#######"]);
        sim.set_target("a", 5, 1);
        sim.add_blueprint(0, 1, 0);
        sim.tick_n(6);
        assert!(sim.has_assignment("a"), "кот должен взяться за чертёж");

        assert!(!sim.has_path("a"), "кот на месте, работает");

        sim.force_tile(3, 1, 0); // проход открылся прямо во время работы
        sim.tick_n(1);
        // Ключевая проверка: приказ стал выполним, но маршрут выдавать нельзя —
        // иначе кот уйдёт со стройки. `Assignment` при этом не снимается, так
        // что проверять надо именно отсутствие маршрута.
        assert!(
            !sim.has_path("a"),
            "приказ не должен срывать кота с начатой стройки"
        );
        assert!(sim.has_assignment("a"));

        sim.tick_n(200);
        assert_eq!(sim.tile(0, 1), 0, "стройка доведена до конца");
        assert_eq!(sim.pos_of("a"), (5, 1), "после стройки приказ исполнен");
    }

    /// Гейтинг повторов опирается на `tried_version`: она обязана отмечать
    /// версию карты, на которой была попытка, и обновляться при её смене.
    ///
    /// Внимание: сам факт «лишних BFS нет» отсюда не виден — без гейта состояние
    /// было бы точно таким же. Это свойство производительности, и тестом оно не
    /// закрыто; закрыт зато опасный случай — что гейт задушит нужный повтор
    /// (см. `order_resumes_after_map_opens`).
    #[test]
    fn order_retry_tracks_map_version() {
        let mut sim = sim_from(&["#######", "#a.#..#", "#######"]);
        sim.set_target("a", 5, 1);

        let v = sim.map_ver();
        assert_eq!(
            sim.order_tried_version("a"),
            Some(v),
            "первая попытка отмечена текущей версией карты"
        );

        sim.tick_n(20);
        assert_eq!(
            sim.order_tried_version("a"),
            Some(v),
            "карта не менялась — отметка попытки остаётся прежней"
        );

        sim.force_tile(3, 1, 0);
        let v2 = sim.map_ver();
        assert_ne!(v, v2, "карта изменилась");
        sim.tick_n(1);
        assert_eq!(
            sim.order_tried_version("a"),
            Some(v2),
            "после смены карты попытка повторяется"
        );
    }

    /// Приказ переживает серию перестроек: открыли проход, тут же закрыли,
    /// открыли снова — и он всё равно исполняется. Ловит ошибки в бухгалтерии
    /// `tried_version`, из-за которых приказ мог бы «залипнуть» после отката.
    #[test]
    fn order_survives_repeated_map_changes() {
        let mut sim = sim_from(&["#######", "#a.#..#", "#######"]);
        sim.set_target("a", 5, 1);
        sim.tick_n(5);
        assert_eq!(sim.pos_of("a"), (1, 1), "пути нет — кот стоит");

        sim.force_tile(3, 1, 0); // открыли
        sim.tick_n(2);
        sim.force_tile(3, 1, -1); // и снова закрыли
        sim.tick_n(20);
        assert_ne!(sim.pos_of("a"), (5, 1), "проход закрыт — цель недостижима");
        assert!(sim.stuck_of("a"));

        sim.force_tile(3, 1, 0); // открыли окончательно
        sim.tick_n(40);
        assert_eq!(sim.pos_of("a"), (5, 1), "приказ дожил и исполнился");
        assert!(!sim.stuck_of("a"));
    }

    // --- снос через очередь -----------------------------------------------

    /// Ключевое правило сноса: кот работает с соседней клетки и не сносит пол
    /// под собой. Цель сноса всегда проходима, поэтому без правила он выбрал бы
    /// её как место работы и уронил себя в яму.
    #[test]
    fn demolish_job_is_done_from_a_neighbour() {
        let mut sim = sim_from(&["#####", "#a..#", "#####"]);
        assert!(sim.plan_demolish(3, 1), "снос построенного тайла ставится в очередь");
        assert_eq!(sim.tile(3, 1), 0, "мгновенно ничего не сносится");

        // Проверяем каждый тик, а не только итог: если кот встанет на цель и
        // снесёт пол под собой, `escape_voids` тут же выведет его на соседнюю
        // клетку — и по конечному состоянию ошибка будет неотличима от нормы.
        for _ in 0..200 {
            sim.tick_n(1);
            assert_ne!(sim.pos_of("a"), (3, 1), "кот встал на клетку, которую сносит");
        }

        assert_eq!(sim.tile(3, 1), -1, "тайл снесён котом");
        assert!(!sim.stuck_of("a"), "кот не уронил себя в яму");
    }

    /// Отмена ещё не начатого плана — мгновенная и бесплатная.
    #[test]
    fn cancelling_a_blueprint_is_instant() {
        let mut sim = sim_from(&["#####", "#a.##", "#####"]);
        assert!(sim.add_blueprint(3, 1, 0));
        assert!(sim.plan_demolish(3, 1), "ластик снимает чертёж");
        sim.tick_n(50);
        assert_eq!(sim.tile(3, 1), -1, "чертёж снят, строить нечего");
    }

    /// Повторный ластик по клетке с чертежом сноса отменяет сам снос.
    #[test]
    fn second_erase_cancels_pending_demolish() {
        let mut sim = sim_from(&["#####", "#a..#", "#####"]);
        sim.plan_demolish(3, 1);
        assert!(sim.plan_demolish(3, 1), "второй ластик снимает чертёж сноса");
        sim.tick_n(100);
        assert_eq!(sim.tile(3, 1), 0, "снос отменён — пол на месте");
    }

    /// Сносить пустую клетку нечего — чертёж не ставится.
    #[test]
    fn erasing_empty_cell_does_nothing() {
        let mut sim = sim_from(&["###", "#a#", "###"]);
        assert!(!sim.plan_demolish(0, 0));
    }

    /// Одинокий тайл без проходимых соседей снести некому: подойти неоткуда.
    /// Осознанное следствие правила «работать с соседней клетки».
    #[test]
    fn lone_tile_cannot_be_demolished_by_a_job() {
        let mut sim = sim_from(&["#####", "#a.##", "#####"]);
        sim.force_tile(2, 1, -1); // отрезали одинокий тайл под котом
        assert!(sim.plan_demolish(1, 1), "чертёж ставится");
        sim.tick_n(200);
        assert_eq!(sim.tile(1, 1), 0, "подойти неоткуда — тайл остаётся");
    }

    // --- порядок сноса ----------------------------------------------------

    /// Главная регрессия: комната, стёртая одним мазком, должна исчезнуть
    /// целиком. Раньше чертежи разбирались в порядке мазка, кот убирал клетку
    /// у двери раньше глубины — и остаток комнаты становился недостижим.
    ///
    /// Схема в `test_maps/whole_room.map`: единственная дверь в комнату — на
    /// строке 2 (`###.###`), клетка (3, 2).
    #[test]
    fn whole_room_is_erased_completely() {
        let mut sim = sim_from(&rows_from(include_str!("test_maps/whole_room.map")));
        sim.plan_demolish_rect([1, 3, 5, 3]);
        sim.tick_n(1000);

        assert_eq!(sim.floors_left([1, 3, 5, 3]), 0, "комната стёрта целиком");
        assert_eq!(sim.tile(3, 2), 0, "дверь не трогали — она осталась");
        assert!(!sim.stuck_of("a"), "кот не заперся внутри");
    }

    /// Направление сноса: сначала дальняя от берега клетка, ближняя — последней.
    #[test]
    fn demolition_retreats_from_the_deep_end() {
        let mut sim = sim_from(&["######", "#a...#", "######"]);
        sim.plan_demolish_rect([2, 1, 3, 1]);

        sim.tick_n(20);
        assert_eq!(sim.tile(4, 1), -1, "первой уходит дальняя клетка");
        assert_eq!(sim.tile(2, 1), 0, "ближняя к коту ещё на месте");

        sim.tick_n(200);
        assert_eq!(sim.floors_left([2, 1, 3, 1]), 0, "остальное снесено следом");
        assert_eq!(sim.pos_of("a"), (1, 1), "кот отступил на берег");
    }

    /// Мазок рисуется не мгновенно: сим тикает, пока игрок ведёт мышь, и кот
    /// успевает взяться за первую клетку. Дорисованная глубина обязана
    /// перебить уже начатый ближний снос, иначе он отрежет её.
    #[test]
    fn extending_the_area_pauses_the_started_job() {
        let mut sim = sim_from(&["######", "#a...#", "######"]);
        sim.plan_demolish(2, 1);
        sim.tick_n(6);
        assert!(sim.has_assignment("a"), "кот взялся за единственный снос");

        sim.plan_demolish(3, 1);
        sim.plan_demolish(4, 1);

        sim.tick_n(24);
        // Прогресса по (2,1) оставалось меньше, чем ушло тиков: не будь паузы,
        // кот бы её давно доломал.
        assert_eq!(sim.tile(2, 1), 0, "начатый ближний снос встал на паузу");
        assert_eq!(sim.tile(4, 1), -1, "глубина ушла первой");

        sim.tick_n(300);
        assert_eq!(sim.floors_left([2, 1, 3, 1]), 0, "в итоге снесено всё");
    }

    /// Два кота с разных концов коридора: у каждого своя зона и свой фронт,
    /// оба работают одновременно и сходятся к середине. Ошибка в выборе места
    /// работы разрубила бы коридор пополам и заперла бы кота не с той стороны.
    #[test]
    fn two_cats_erase_a_corridor_from_both_ends() {
        let mut sim = sim_from(&["##########", "#a......b#", "##########"]);
        sim.plan_demolish_rect([2, 1, 6, 1]);
        sim.tick_n(400);

        assert_eq!(sim.floors_left([2, 1, 6, 1]), 0, "коридор стёрт целиком");
        assert_eq!(sim.pos_of("a"), (1, 1), "каждый отступил на свой берег");
        assert_eq!(sim.pos_of("b"), (8, 1));
    }

    /// У области, которую сносят целиком, берега нет — волну задаёт кот внутри.
    /// Снос идёт от него наружу и упирается в клетку под ним: снести её некому
    /// (§12.11), поэтому островок пола под котом — ожидаемый остаток. Где
    /// именно он окажется, не фиксируем: кот ходит, и вместе с ним едет
    /// источник волны.
    #[test]
    fn fully_erased_area_leaves_the_island_under_the_cat() {
        let mut sim = sim_from(&["#####", "#a..#", "#####"]);
        sim.plan_demolish_rect([1, 1, 3, 1]);
        sim.tick_n(300);

        assert_eq!(sim.floors_left([1, 1, 3, 1]), 1, "остался ровно один тайл");
        let (x, y) = sim.pos_of("a");
        assert_eq!(sim.tile(x, y), 0, "и это клетка под котом");
        assert!(!sim.stuck_of("a"), "кот стоит на полу, а не в пустоте");
    }

    /// Мгновенный снос остаётся доступен как отладочный путь.
    #[test]
    fn instant_demolish_still_available() {
        let mut sim = sim_from(&["#####", "#a..#", "#####"]);
        assert!(sim.demolish(3, 1));
        assert_eq!(sim.tile(3, 1), -1, "снос без котов и без тиков");
    }
}
