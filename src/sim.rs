//! WASM-интерфейс: всё, что видит воркер.
//!
//! Наружу отдаём:
//!   * `map_meta()`      — размеры и палитра тайлов (один раз, для рендера);
//!   * `map_version()`   — счётчик изменений карты (для отправки base_map по требованию);
//!   * `base_map()`      — текущее состояние тайлов базы;
//!   * `add_blueprint()` — поставить чертёж (задачу); выполняют коты, по тикам;
//!   * `plan_demolish()` — ластик: отменить чертёж либо запланировать снос тайла;
//!   * `buildable()`     — маска правила доступа для превью рамки (§12.111);
//!   * `*_rect()`        — те же инструменты на рамку; решение — на всю рамку сразу;
//!   * `mark_to_store_rect()` — пометить кучи «на склад» (или снять пометку);
//!   * `set_auto_tidy()` — убирать ли лом с пола без приказа;
//!   * `set_auto_rest()` — бросает ли кот работу на критической бодрости;
//!   * `launch()`        — отправить названных котов на вылазку;
//!   * `cancel_mission()`— распустить отряд, пока он ещё не ушёл с базы;
//!   * `hire()`          — нанять кандидата: известность открывает, склад платит;
//!   * `teach()`         — отправить кота за парту: обучение адресно (§12.18);
//!   * `start_research()`— взяться за тему: склад платит образцами, сядет допущенный;
//!   * `cancel_research()` — бросить тему (образцы не возвращаются);
//!   * `start_craft()`   — заказать штуки по рецепту: склад платит за каждую;
//!   * `cancel_craft()`  — отменить заказ (материал начатой штуки не вернётся);
//!   * `demolish()`      — мгновенный снос без котов (тесты/отладка);
//!   * `set_target()`    — приказ коту идти в тайл (движение по тикам, отменяет его задачу);
//!   * `tick()`          — один фиксированный шаг симуляции;
//!   * `snapshot()`      — сущности, чертежи и кучи лома (каждый кадр).

use bevy_ecs::prelude::*;
use wasm_bindgen::prelude::*;

use bevy_ecs::system::RunSystemOnce;

use crate::components::*;
use crate::goals::{WorldFacts, built_counts, progress_of};
use crate::hauling::{plan_spend, stored_counts};
use crate::jobs::{BUILD_WORK, Plan, may_build};
use crate::map::{BaseMap, rect_cells};
use crate::missions::{
    crew_danger, crew_force, duration, gate_cells, gate_count, guide_cut, guide_of, guide_value,
    outcome,
};
use crate::movement::{Busy, is_stuck};
use crate::path::find_path;
use crate::relay::relay_force;
use crate::ruleset::{
    EventDef, FactionDef, GoalDef, ItemDef, MissionDef, PerkDef, RecipeDef, RecruitDef,
    ResearchDef, Ruleset, SkillDef, StatDef, TileDef,
};
use crate::save::{FORMAT, SaveFile, capture, fingerprint, note, restore};
use crate::schedule::build_schedule;
use crate::seen::note_seen;
use crate::skills::{
    SKILL_RAID, SKILL_SCIENCE, desk_cap, level_cap_of, level_of, nearest_desk, xp_ceiling,
};
use crate::snapshot::{
    AutoGateNames, BaseMapDto, BlueprintSnap, CraftSnap, DealSnap, DeskSnap, EntitySnap, GoalSnap,
    MapMeta, MissionSnap, NeedSnap, NewsSnap, NodeSnap, NoteSnap, PriceSnap, RaidGates, RaidSnap,
    RecipeSnap, RecruitSnap, ResearchSnap, SaleSnap, SkillSnap, Snapshot, StackSnap, StockSnap,
    TickerSnap, TopicSnap,
};
use crate::timeline::{ready_for, revealed};

#[wasm_bindgen]
pub struct Sim {
    pub(crate) world: World,
    pub(crate) schedule: Schedule,
    pub(crate) palette: Vec<TileDef>,
    pub(crate) items: Vec<ItemDef>,
    pub(crate) skills: Vec<SkillDef>,
    pub(crate) stats: Vec<StatDef>,
    pub(crate) perks: Vec<PerkDef>,
    pub(crate) factions: Vec<FactionDef>,
    pub(crate) missions: Vec<MissionDef>,
    pub(crate) recruits: Vec<RecruitDef>,
    pub(crate) research: Vec<ResearchDef>,
    pub(crate) recipes: Vec<RecipeDef>,
    pub(crate) timeline: Vec<EventDef>,
    pub(crate) goals: Vec<GoalDef>,
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// Сколько тиков в сутках (§12.46). Лежит рядом с размерами карты и по той
    /// же причине: это описание мира для вида, а не правило — ни одна система
    /// его не читает.
    pub(crate) day: u64,
    /// Сколько тиков висит тикер новости (§12.120). Здесь по тому же доводу,
    /// что и `day`: подача, а не механика.
    pub(crate) news: u64,
    /// Отпечаток рулсета, на котором собран этот мир (§12.45). Лежит здесь, а
    /// не в ресурсах, ровно потому, что миру он не нужен: он нужен снимку —
    /// чтобы тот не загрузился в мир, собранный по другим правилам.
    pub(crate) ruleset: u64,
}

impl Sim {
    /// Собрать мир из снимка — с ошибкой обычной строкой.
    ///
    /// Отделено от `load` намеренно: `JsValue` вне wasm не собирается вовсе
    /// (паника прямо в конструкторе), поэтому отказы, которые нужно уметь
    /// проверять тестами, обязаны существовать до превращения в него.
    pub(crate) fn load_from(ruleset_yaml: &str, save_json: &str) -> Result<Sim, String> {
        let file: SaveFile =
            serde_json::from_str(save_json).map_err(|e| format!("save parse error: {e}"))?;
        if file.version != FORMAT {
            return Err(format!(
                "сохранение от другой версии игры (формат {}, нужен {FORMAT})",
                file.version
            ));
        }
        let mut sim = Sim::new(ruleset_yaml).map_err(|_| "рулсет не читается".to_string())?;
        if file.ruleset != sim.ruleset {
            return Err("сохранение снято на другом рулсете: правила изменились, \
                        и старая партия в них уже не та"
                .to_string());
        }
        restore(&mut sim.world, &file);
        Ok(sim)
    }

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

    /// Снять чертёж с клетки, освободить строителя с носильщиком и **вернуть
    /// завезённое** (§12.31).
    ///
    /// Материал ложится кучей на клетку самой площадки — тем же правилом, что
    /// возврат от сноса и добыча с вылазки: вещь появляется там, где была
    /// работа. Площадка обычно ещё пустота, и куча оказывается в яме — оттуда её
    /// штатно сдвинет `settle_stacks` (§12.15), отдельного случая не нужно.
    ///
    /// Груз **в лапах** носильщика при этом остаётся у него: ношу посреди базы
    /// не бросают, он донесёт её следующей доставкой (§12.16).
    fn cancel_blueprint(&mut self, x: i32, y: i32) -> bool {
        let Some(e) = self.blueprint_at(x, y) else {
            return false;
        };
        let bp = self.world.get::<Blueprint>(e);
        let assignee = bp.and_then(|b| b.assignee);
        let delivered: Vec<(usize, i32)> = bp.map(|b| b.delivered.clone()).unwrap_or_default();
        for (item, count) in delivered {
            self.drop_stack(x, y, item, count);
        }
        if let Some(cat) = assignee {
            self.world
                .entity_mut(cat)
                .remove::<(Assignment, Path, Stride)>();
        }
        // Носильщиков у площадки может быть несколько, и записаны они не у неё,
        // а у себя (§12.48): разворачиваем всех, кто вёз сюда.
        let mut q = self.world.query::<(Entity, &Haul)>();
        let going: Vec<Entity> = q
            .iter(&self.world)
            .filter(|(_, haul)| matches!(haul.to, HaulTo::Site(bp_e) if bp_e == e))
            .map(|(cat, _)| cat)
            .collect();
        for cat in going {
            self.world.entity_mut(cat).remove::<(Haul, Path, Stride)>();
        }
        self.world.entity_mut(e).despawn();
        true
    }

    /// Положить кучу на клетку, слив с уже лежащей там кучей того же типа.
    ///
    /// Фасадный двойник `hauling::spill`: тот работает в системе (`Commands` +
    /// `Query`), этот — по миру. Правило одно: кучи одного типа на клетке
    /// сливаются, разных — лежат рядом (§12.21).
    fn drop_stack(&mut self, x: i32, y: i32, item: usize, count: i32) {
        if count <= 0 {
            return;
        }
        let mut q = self.world.query::<(Entity, &Position, &Stack)>();
        let found = q
            .iter(&self.world)
            .find(|(_, p, s)| (p.x, p.y) == (x, y) && s.item == item)
            .map(|(e, ..)| e);
        match found.and_then(|e| self.world.get_mut::<Stack>(e)) {
            Some(mut stack) => stack.count += count,
            None => {
                self.world.spawn((Position { x, y }, Stack { item, count }));
            }
        }
    }

    /// Сколько вылазок идёт прямо сейчас (§12.59: их столько, сколько узлов
    /// связи). Слот занимает сама миссия, а не кто-то за неё. Близнецов у неё
    /// больше нет: у торговли слот стал местом (`free_post_cell`, §12.68), у
    /// производства — тоже (`free_shop_cell`, §12.96), а вылазка осталась
    /// последней, кто считает работы, а не ячейки.
    fn raids_running(&mut self) -> usize {
        let mut q = self.world.query_filtered::<Entity, With<Mission>>();
        q.iter(&self.world).count()
    }

    /// Вылазка по этому заказу, если она уже идёт: двух одинаковых не бывает
    /// (§12.59). По нему же её и отменяют, потому что порядок обхода сущностей
    /// ECS недетерминирован (§11). У заказа мастерской ключ с §12.96 другой —
    /// клетка (`order_at`): заказов на один рецепт бывает несколько.
    fn mission_of(&mut self, def: usize) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &Mission)>();
        q.iter(&self.world)
            .find(|(_, m)| m.def == def)
            .map(|(e, _)| e)
    }

    /// Есть ли кого спасать: хоть один кот остался в плену (§12.40).
    fn has_captive(&mut self) -> bool {
        let mut q = self.world.query_filtered::<Entity, With<Captive>>();
        q.iter(&self.world).next().is_some()
    }

    /// Открыта ли вылазка и есть ли у неё цель — то, что решает ядро, а не
    /// интерфейс (§12.24).
    ///
    /// Ровно те же две проверки, что и в `launch`: известность как ворота и
    /// «есть ли кого спасать» у вылазки за своим. Третьего экземпляра этих
    /// правил в JS быть не должно — он однажды разойдётся с ядром и покажет
    /// кнопку, которую заявка отклонит.
    ///
    /// Состав отряда здесь не проверяется: кого выбрал игрок, знает только
    /// интерфейс, и спрашивать об этом снапшот каждый кадр незачем.
    pub(crate) fn raid_gates(&mut self, def: usize) -> RaidGates {
        let Some(rule) = self.world.resource::<MissionRules>().0.get(def).cloned() else {
            return RaidGates::default();
        };
        RaidGates {
            unlocked: self.world.resource::<Fame>().0 >= rule.requires,
            welcome: self.world.resource::<Standing>().covers(&rule.needs),
            possible: !rule.rescue || self.has_captive(),
            // Медленный край срока — свойство заказа, а не отряда (§12.71): тем
            // же `duration`, каким срок замёрзнет на уходе. Нужен там, где
            // отряда ещё нет и считать не на кого.
            span_slow: duration(&rule, rule.squad),
        }
    }

    /// Открыт ли кандидат — то же выражение, что и в снимке (`RecruitSnap`).
    ///
    /// «Открыт» здесь значит **«его можно позвать»**, а не «его можно оплатить»:
    /// известность и доверие фракции — это ворота, а пустой склад — «пока
    /// нечем». Ровно так же делит причины и панель, пряча нанятого и запертого и
    /// оставляя остальных с причиной словом (§12.94).
    ///
    /// Нанятый в счёт не идёт **намеренно**: найм закрыл бы кандидата, и лента
    /// объявила бы новостью то, что игрок только что сделал сам.
    pub(crate) fn recruit_is_open(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<RecruitRules>().0.get(def).cloned() else {
            return false;
        };
        self.world.resource::<Fame>().0 >= rule.requires
            && self.world.resource::<Standing>().covers(&rule.needs)
    }

    /// Нанят ли кандидат — тем же вопросом, что и `hire`: спрашиваем у самого
    /// кота в мире, а не у отдельного списка нанятых.
    pub(crate) fn recruit_is_hired(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<RecruitRules>().0.get(def).cloned() else {
            return false;
        };
        let mut units = self.world.query::<&UnitId>();
        units.iter(&self.world).any(|u| u.0 == rule.id)
    }

    /// Открыта ли тема: предыдущие технологии на месте **и образец, если он ей
    /// нужен, база хоть раз видела** (§12.143).
    ///
    /// Ворот двое, потому что их двое и у окна: тема, ждущая находки, в списке
    /// не показывается вовсе — до первой вылазки за «Наукой» пусто, и дверь
    /// спрятана (§12.94). Считай их лента иначе, и новость «новая тема»
    /// пришла бы нулевым тиком, когда показывать ещё нечего, — то есть
    /// разошлась бы с экраном ровно так, как запрещает инвариант 14.
    ///
    /// Изученность в счёт не идёт по тому же доводу, что и найм: тема, доведённая
    /// до конца, закрылась бы новостью о собственной победе. Обе шкалы только
    /// растут (технологии не забываются, §12.18; `Seen` не забывает предмет,
    /// §12.131), значит тема умеет открыться и не умеет закрыться — и это
    /// правда о мире, а не упущение.
    pub(crate) fn topic_is_open(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<ResearchRules>().0.get(def).cloned() else {
            return false;
        };
        if !self.world.resource::<Techs>().covers(&rule.requires) {
            return false;
        }
        let seen = self.world.resource::<Seen>();
        rule.specimen.iter().all(|&(item, _)| seen.saw(item))
    }

    /// Открыт ли рецепт — то же выражение, что и `unlocked` в снимке
    /// (`RecipeSnap`): технология изучена.
    ///
    /// Мастерская и склад в счёт не идут по тому же доводу, что лаборатория у
    /// темы: это «пока нечем», а не «пока нельзя» — и чинится оно стройкой,
    /// про которую окно «Склад» и так говорит красной строкой (§12.100).
    ///
    /// Закрыться рецепт не умеет: технологии не забываются (§12.18). См. шапку
    /// `NewsKind`.
    pub(crate) fn recipe_is_open(&mut self, def: usize) -> bool {
        let Some(rule) = self.world.resource::<CraftRules>().0.get(def).cloned() else {
            return false;
        };
        self.world.resource::<Techs>().covers(&rule.requires)
    }

    /// Открыт ли тайл палитры — то же выражение, что и ворота разметки
    /// (`tech_allows`, §12.27), и то же, по которому вид решает, показывать ли
    /// кнопку (§12.126).
    ///
    /// Цена и место в счёт не идут по тому же доводу, что мастерская у рецепта:
    /// «нечем платить» — это «пока нечем», а не «пока нельзя». Закрыться тайл
    /// не умеет: технологии не забываются (§12.18). См. шапку `NewsKind`.
    pub(crate) fn tile_is_open(&mut self, def: usize) -> bool {
        if def >= self.world.resource::<TileRules>().0.len() {
            return false;
        }
        self.tech_allows(def as i16)
    }

    /// Открыт ли заказ вылазки **для ленты** — то есть виден ли он игроку в
    /// штабе. Ровно те же ворота, по которым решает `hiddenRaid` в виде: рост
    /// известности (§12.79) и «есть ли кого спасать» у вылазки за своим.
    ///
    /// **Доверие фракции сюда не входит, и это не упущение.** Заказ, закрытый
    /// репутацией, из списка не пропадает (§12.79): он остаётся карточкой с
    /// причиной словом — «Полиция +30, у базы −40», — потому что это цель, к
    /// которой игрок идёт. Считай ленту по всем трём воротам разом, и она
    /// разойдётся с экраном в обе стороны: «Логово рейдеров» появляется в
    /// списке по одной известности **молча** (заказчик пока не жалует), а
    /// просевшая репутация объявляет «заказ больше не откликается» про
    /// карточку, которая на экране осталась, — та же ошибка, что новость про
    /// уже нанятого кота.
    ///
    /// Правило общее: **лента говорит о появлении и пропаже, а не о воротах.**
    /// Ворота называет словом сама карточка (§12.53).
    pub(crate) fn raid_is_open(&mut self, def: usize) -> bool {
        let rescue = self
            .world
            .resource::<MissionRules>()
            .0
            .get(def)
            .is_some_and(|m| m.rescue);
        let g = self.raid_gates(def);
        g.unlocked && (!rescue || g.possible)
    }

    /// Открыт ли предмет — **то же выражение, каким `syncStockWindow` решает,
    /// показывать ли строку** (§12.131): база его видела (`Seen`) или умеет
    /// сделать открытым рецептом. Второй экземпляр этого правила в JS уже есть
    /// и не может не быть (строку прячет вид), поэтому расходиться им нельзя:
    /// новость обязана называть появление строки, а не то, что за ней стоит.
    ///
    /// Умение при этом объявляет **рецепт** («теперь умеем делать …»), и второй
    /// новости о том же в тот же тик не бывает — см. `note_news`.
    pub(crate) fn item_is_open(&mut self, def: usize) -> bool {
        if self.world.resource::<Seen>().saw(def) {
            return true;
        }
        let rules = self.world.resource::<CraftRules>().0.clone();
        let techs = self.world.resource::<Techs>();
        rules
            .iter()
            .any(|r| r.gives.iter().any(|&(i, _)| i == def) && techs.covers(&r.requires))
    }

    /// Наблюдатель ленты новостей (§12.120): что открылось и что закрылось за
    /// этот тик.
    ///
    /// Стоит в `Sim::tick` **после** цепочки, а не системой в ней, по той же
    /// причине, по которой там же стоят автовылазка и автопродажа (§12.67,
    /// §12.87): ворота считает фасад (`raid_gates`, `recruit_is_open`,
    /// `topic_is_open`), а второй их экземпляр однажды разойдётся с первым —
    /// инвариант 14. После цепочки — потому что новость читает **сложившийся**
    /// тик, дословно `check_goals` (§12.58).
    ///
    /// Крючков в местах, где ворота проверяются (`launch`, `hire`,
    /// `start_research`), нет ни одного и заводить их нельзя: ворота открывает
    /// не команда игрока, а выросшая шкала, — и десять крючков вместо одного
    /// наблюдателя это девять мест, где однажды забудут.
    fn note_news(&mut self) {
        let at = self.world.resource::<SimTime>().tick;
        let counts = [
            self.world.resource::<MissionRules>().0.len(),
            self.world.resource::<RecruitRules>().0.len(),
            self.world.resource::<ResearchRules>().0.len(),
            self.world.resource::<CraftRules>().0.len(),
            self.world.resource::<TileRules>().0.len(),
            self.items.len(),
        ];
        // Базовая линия снимается молча: партия начинается с открытой первой
        // ступени, и объявлять её новостью не о чем. У загруженной партии линия
        // приезжает из снимка, поэтому здесь не пересчитывается.
        let first = !self.world.resource::<News>().started;
        for kind in NewsKind::ALL {
            for def in 0..counts[kind.index()] {
                // **Нанятый — не кандидат вовсе** (§12.120), поэтому наблюдатель
                // не смотрит на него совсем: ни ворот, ни новости, ни отметки
                // состояния. `recruit_is_open` про найм не знает намеренно
                // (иначе найм закрывал бы кандидата сам), но отвечает он на
                // вопрос, которого больше нет: кот на базе, и ни репутация, ни
                // известность про него ничего не решают, а в списке найма его
                // не показывают (§12.94). Спроси тут ворота — и упавшая
                // репутация пострадавшего (§12.43) стала бы новостью про того,
                // кого игрок уже нанял.
                if kind == NewsKind::Recruit && self.recruit_is_hired(def) {
                    continue;
                }
                let open = match kind {
                    NewsKind::Raid => self.raid_is_open(def),
                    NewsKind::Recruit => self.recruit_is_open(def),
                    NewsKind::Topic => self.topic_is_open(def),
                    NewsKind::Recipe => self.recipe_is_open(def),
                    NewsKind::Tile => self.tile_is_open(def),
                    // Ворота предмета — **то же выражение, каким окно «Склад»
                    // решает, показывать ли строку** (§12.131, §12.136): он
                    // «открыт», если побывал в мире базы (`Seen`) **или** его
                    // умеет сделать открытый рецепт. Второе не украшение: по
                    // §12.131 открытый рецепт делает строку видимой заранее —
                    // кнопка «Произвести» живёт в строке выхода, — и без него
                    // лента объявляла «новый ресурс — Аптечка» в тот момент,
                    // когда первая аптечка легла под ноги мастеру, то есть про
                    // строку, которая на экране стояла с нулём уже давно
                    // (§12.145). Закрытий тут не бывает по обеим сторонам:
                    // `Seen` только растёт, технологии не забываются (§12.18).
                    NewsKind::Item => self.item_is_open(def),
                };
                let was = self.world.resource::<News>().was_open(kind, def);
                // **Открывшийся рецепт объявляет себя сам** (§12.145): он
                // открывает и свой выход, и в тот же тик рядом встали бы две
                // строки об одном — «теперь умеем делать: Аптечка» и «новый
                // ресурс: Аптечка», обе ведущие в «Склад». Состояние при этом
                // отмечается молча (`set_open` ниже), поэтому приезд самой вещи
                // новостью уже не станет: строка на экране была.
                let dup = kind == NewsKind::Item && open && !self.world.resource::<Seen>().saw(def);
                let mut news = self.world.resource_mut::<News>();
                if !first && open != was && !dup {
                    news.push(kind, def, open, at);
                }
                news.set_open(kind, def, open);
            }
        }
        self.world.resource_mut::<News>().started = true;
    }

    /// Отряд миссии: кто в нём и ушёл ли он уже с базы.
    fn crew_of(&mut self, mission: Entity) -> Vec<(Entity, bool)> {
        let mut q = self.world.query::<(Entity, &Squad, Option<&Away>)>();
        q.iter(&self.world)
            .filter(|(_, squad, _)| squad.0 == mission)
            .map(|(e, _, away)| (e, away.is_some()))
            .collect()
    }

    /// Снять миссию и освободить весь её отряд. Зовётся и по кнопке «Отозвать»,
    /// и когда игрок уводит бойца приказом: состав выбран поимённо, заменить
    /// выбывшего некем, а молча зависшая вылазка хуже честного роспуска.
    fn disband(&mut self, mission: Entity) {
        for (cat_e, _) in self.crew_of(mission) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Squad, Path, Stride)>();
        }
        self.world.entity_mut(mission).despawn();
    }

    /// Что лежит на складе: кучи на клетках с ёмкостью, в порядке обхода карты.
    ///
    /// Порядок задан явно, а не порядком сущностей: обход ECS зависит от истории
    /// вставок, а любой недетерминизм ломает и тесты, и модель времени (§11).
    fn storage_piles(&mut self) -> Vec<(Entity, usize, i32)> {
        let mut q = self.world.query::<(Entity, &Position, &Stack)>();
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        let mut piles: Vec<(i32, i32, Entity, usize, i32)> = q
            .iter(&self.world)
            .filter(|(_, p, _)| rules.capacity_of(map.tile_at(p.x, p.y)) > 0)
            .map(|(e, p, s)| (p.y, p.x, e, s.item, s.count))
            .collect();
        piles.sort_unstable_by_key(|&(y, x, ..)| (y, x));
        piles.into_iter().map(|(_, _, e, i, n)| (e, i, n)).collect()
    }

    /// Сколько предмета лежит на складе — им и платят (§12.24).
    fn in_storage(&mut self, item: usize) -> i32 {
        self.storage_piles()
            .iter()
            .filter(|&&(_, i, _)| i == item)
            .map(|&(_, _, n)| n)
            .sum()
    }

    /// Имущество базы по типам, разложенное на три числа (§12.53).
    ///
    /// Одним числом это врало бы: платит **склад** (§12.24), из складского ещё
    /// вычитается обещанное покупателю (§12.50), а кучи на полу и груз в лапах
    /// у базы есть, но заплатить ими нельзя, пока их не убрали. Игрок, который
    /// видит сумму всего, не понимает, почему найм отклонён.
    ///
    /// Считается **здесь**, а не в панели: то же правило вторым экземпляром в
    /// JS однажды разойдётся с `plan_spend` (§12.26).
    ///
    /// **Бронь берётся сырой (`owed`), а груз под сделку числится складским**, а
    /// не валяющимся (§12.69). Причина не в бухгалтерии, а в том, что игрок
    /// видит: товар для продажи кот берёт со склада, и при обычном счёте он на
    /// время ходки перетекал из главного числа в серое — то прыгало вверх, то
    /// падало обратно, да ещё и утверждало про уже проданное, что оно «валяется
    /// и годится на стройку». Здесь эти штуки остаются там, где были: учтёнными.
    /// Главное число тогда падает **один раз, в момент заявки**, и держится до
    /// отгрузки.
    ///
    /// Арифметика от перекладывания не меняется: `(склад + лапы под сделку) −
    /// owed` — это ровно `in_storage − booked`, то есть те же ворота, что у
    /// `Sim::trade` и у `plan_spend`.
    pub(crate) fn stock(&mut self) -> Vec<StockSnap> {
        let booked = self.owed_for_sale();
        let mut loose = vec![0; self.items.len()];
        let mut stored = vec![0; self.items.len()];
        {
            let mut q = self.world.query::<(&Position, &Stack)>();
            let map = self.world.resource::<BaseMap>();
            let rules = self.world.resource::<TileRules>();
            // Складское считает `stored_counts` — одно место на фасад и на цели
            // (§12.53). Пол остаётся здесь: он нужен только панели.
            // `stored_counts` растёт под встреченные типы и короче палитры,
            // если чего-то на складе нет вовсе.
            for (item, n) in stored_counts(q.iter(&self.world), map, rules)
                .into_iter()
                .enumerate()
            {
                if let Some(slot) = stored.get_mut(item) {
                    *slot += n;
                }
            }
            for (p, s) in q.iter(&self.world) {
                if rules.capacity_of(map.tile_at(p.x, p.y)) == 0
                    && let Some(n) = loose.get_mut(s.item)
                {
                    *n += s.count;
                }
            }
            // Груз в лапах — тоже имущество базы, и считать его надо: без него
            // счётчик проседает, пока кот несёт кучу, а это читается как
            // потеря материала. Куда его записать, решает адресат: везомое к
            // посту уже учтено и забронировано, всё прочее поднято с пола или
            // идёт на стройку — то есть неучтённое.
            let mut paws = self.world.query::<(&Carrying, Option<&Haul>)>();
            for (load, haul) in paws.iter(&self.world) {
                let sold = haul.is_some_and(|h| matches!(h.to, HaulTo::Sale(_)));
                let side = match sold {
                    true => &mut stored,
                    false => &mut loose,
                };
                if let Some(n) = side.get_mut(load.item) {
                    *n += load.count;
                }
            }
        }
        let seen = self.world.resource::<Seen>();
        let items = self.world.resource::<ItemRules>();
        let techs = self.world.resource::<Techs>();
        (0..self.items.len())
            .map(|item| {
                let owed = booked
                    .iter()
                    .find(|&&(i, _)| i == item)
                    .map_or(0, |&(_, n)| n);
                StockSnap {
                    seen: seen.saw(item),
                    understood: items.understood(item, techs),
                    stored: stored[item],
                    loose: loose[item],
                    booked: owed,
                    // `stored + loose` — это ровно `on_base_counts`: обе ветки
                    // выше раскладывают по этим двум числам **все** кучи и
                    // **все** лапы, а куда именно — решает адресат. Минус
                    // `owed` — и получается то самое, что меряет порог
                    // производства (`crafting::pieces_needed`, §12.65).
                    on_base: stored[item] + loose[item] - owed,
                }
            })
            .collect()
    }

    /// Цели партии так, как их видит панель (§12.58).
    ///
    /// Считается **здесь**, а не в JS, по той же причине, что и разбивка склада
    /// (§12.53): счётчик «74 / 100» обязан считаться тем же выражением, каким
    /// цель засчитывается (`goals::progress_of`). Второй экземпляр правила в
    /// панели однажды разойдётся с системой, и игрок увидит полную полоску у
    /// незакрытой цели.
    pub(crate) fn goals(&mut self) -> Vec<GoalSnap> {
        let rules = self.world.resource::<GoalRules>().0.clone();
        let stored = {
            let mut q = self.world.query::<(&Position, &Stack)>();
            let map = self.world.resource::<BaseMap>();
            let tiles = self.world.resource::<TileRules>();
            stored_counts(q.iter(&self.world), map, tiles)
        };
        let built = built_counts(self.world.resource::<BaseMap>());
        let cats = self.world.query::<&UnitId>().iter(&self.world).count() as i32;
        let facts = WorldFacts {
            techs: self.world.resource::<Techs>(),
            raids: self.world.resource::<Raids>(),
            crafted: self.world.resource::<Crafted>(),
            earned: self.world.resource::<Earned>(),
            cats,
            stored,
            built,
        };
        let taken = self.world.resource::<Goals>();
        rules
            .iter()
            .enumerate()
            .filter_map(|(def, rule)| {
                let done = taken.taken(def);
                // Скрытая и невзятая наружу не уходит вовсе: прятать её в JS
                // значит объявить её в devtools (§12.28).
                if rule.hidden && done.is_none() {
                    return None;
                }
                let (have, need) = progress_of(&rule.test, &facts);
                Some(GoalSnap {
                    def,
                    done: done.is_some(),
                    at: done.map_or(0, |t| t.at),
                    have,
                    need,
                    hidden: rule.hidden,
                })
            })
            .collect()
    }

    /// Хватает ли на складе на весь набор — **за вычетом брони** (§12.50).
    ///
    /// Считает то же, что и `plan_spend` при самой оплате: разойдись они, и
    /// панель показывала бы живую кнопку, которую фасад отклоняет молча.
    fn storage_covers(&mut self, cost: &[(usize, i32)]) -> bool {
        let booked = self.booked_for_sale();
        cost.iter().all(|&(item, need)| {
            let reserved = booked
                .iter()
                .find(|&&(i, _)| i == item)
                .map_or(0, |&(_, n)| n);
            self.in_storage(item) - reserved >= need
        })
    }

    /// Списать набор со склада. Либо снимается всё, либо ничего: половинчатая
    /// оплата оставила бы игрока и без предметов, и без кота.
    ///
    /// Сам расчёт живёт в `plan_spend` и общий с производством, которое платит
    /// из системы (§12.30): две арифметики списания однажды разошлись бы на
    /// порядке обхода куч.
    fn spend_from_storage(&mut self, cost: &[(usize, i32)]) -> bool {
        let piles = self.storage_piles();
        let booked = self.booked_for_sale();
        let Some(takes) = plan_spend(&piles, cost, &booked) else {
            return false;
        };
        for (pile_e, taken) in takes {
            let count = self.world.get::<Stack>(pile_e).map_or(0, |s| s.count);
            if taken >= count {
                self.world.entity_mut(pile_e).despawn();
            } else if let Some(mut stack) = self.world.get_mut::<Stack>(pile_e) {
                stack.count -= taken;
            }
        }
        true
    }

    /// Снять с кота текущую задачу, освободив всё, что она держала.
    ///
    /// Одно место на всех, кто задачу отбирает — приказ игрока и заявка на
    /// вылазку. Забыть освободить чертёж можно ровно однажды, и площадка после
    /// этого навсегда останется за котом, который давно занят другим (§12.15).
    fn release_task(&mut self, cat: Entity) {
        if let Some(bp_e) = self.world.get::<Assignment>(cat).map(|a| a.0)
            && let Some(mut bp) = self.world.get_mut::<Blueprint>(bp_e)
        {
            bp.assignee = None;
        }
        // Подвоз освобождать нечего: носильщиков у адресата столько, сколько
        // котов к нему идёт, и записаны они у себя, а не у него (§12.48). Снять
        // с кота `Haul` (ниже) — и есть всё освобождение.
        if let Some(topic_e) = self.world.get::<Researching>(cat).map(|r| r.0)
            && let Some(mut topic) = self.world.get_mut::<Research>(topic_e)
        {
            topic.assignee = None;
        }
        // У заказа освобождаем **только исполнителя**: ячейка станка — свойство
        // самого заказа, а не задачи кота (§12.96), и снятая здесь потеряла бы
        // станок вместе с оплаченным материалом.
        if let Some(order_e) = self.world.get::<Crafting>(cat).map(|c| c.0)
            && let Some(mut order) = self.world.get_mut::<Craft>(order_e)
        {
            order.assignee = None;
        }
        // Груз кот при этом не бросает: донесёт, когда снова возьмётся за
        // доставку (§12.15). Сон и учёба снимаются — это осознанные действия
        // (§12.20, §12.18); парту при этом отпускает сам снятый `Study`, а
        // пациента — `assign_treat`, заметив пропажу `Treating` (§12.37).
        //
        // А вот **лечение приказом не отменяется**: раненый не «занят делом»,
        // которое можно бросить, он не может работать. Игрок волен увести его
        // куда угодно — лечиться кот будет и там, просто без койки (§12.37).
        // `OnDuty` снимается приказом наравне со всеми: «иди туда» уводит кота
        // и от рации. **Приписку (`Posted`) приказ не снимает** (§12.60) — она
        // конфигурация, и кот вернётся на узел, как только освободится, ровно
        // как тема исследования переживает уход учёного спать. Без отдельной
        // команды отмены это дало бы кота-йо-йо, поэтому она и есть (`unpost`).
        self.world.entity_mut(cat).remove::<(
            Assignment,
            Haul,
            Rest,
            Study,
            Researching,
            Crafting,
            Equipping,
            Eating,
            Treating,
            OnDuty,
            Path,
            Stride,
        )>();
    }

    /// Карта с наложенными чертежами — то, по чему считает правило доступа
    /// (§12.111): разметка обещает тайл, и обещанное правило видеть обязано.
    fn plan(&mut self) -> Plan {
        let mut q = self.world.query::<&Blueprint>();
        let planned: Vec<(i32, i32, i16)> = q
            .iter(&self.world)
            .map(|bp| (bp.x, bp.y, bp.tile))
            .collect();
        Plan::of(self.world.resource::<BaseMap>(), planned.into_iter())
    }

    /// Открыта ли постройка этого тайла: технология изучена или не нужна.
    fn tech_allows(&self, tile: i16) -> bool {
        let rules = self.world.resource::<TileRules>();
        match rules.tech_of(tile) {
            Some(tech) => self.world.resource::<Techs>().knows(tech),
            None => true,
        }
    }

    /// Сущность темы по индексу записи; `None` — эту тему сейчас не изучают.
    ///
    /// Спрашивают **по `def`**, а не «идёт ли хоть какая-то тема» (§12.132):
    /// тем теперь столько, сколько лабораторий. Ключ при этом однозначен, в
    /// отличие от заказа: двух тем на один `def` не бывает никогда — тема
    /// одноразова и необратима (§12.18), и `cancel_research` поэтому осталась
    /// адресуемой темой, а не клеткой.
    fn topic_of(&mut self, def: usize) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &Research)>();
        q.iter(&self.world)
            .find(|(_, t)| t.def == def)
            .map(|(e, _)| e)
    }

    /// Есть ли на базе клетка лаборатории — без неё работать негде.
    fn has_lab(&mut self) -> bool {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .any(|(x, y)| rules.is_lab(map.tile_at(x, y)))
    }

    /// Есть ли на базе кот с таким уровнем «Науки» — это допуск, а не скорость
    /// (§12.18): без него тему не возьмёт никто и никогда.
    fn has_scientist(&mut self, level: i32) -> bool {
        self.count_scientists(level, false) > 0
    }

    /// То же, но **только среди тех, кто на базе** (§12.135): ушедший в поле не
    /// возьмётся за тему, и `assign_research` его не видит (`Without<Away>`).
    ///
    /// Два вопроса, а не один: «есть ли у базы такой кот» решает заявку —
    /// отказывать из-за того, что учёный сейчас в поле, значило бы отменять
    /// решение игрока по временной причине, — а «есть ли он **сейчас**»
    /// объясняет, почему заведённая тема стоит. Второе без первого читается как
    /// «допуска нет», хотя он есть.
    pub(crate) fn has_scientist_home(&mut self, level: i32) -> bool {
        self.count_scientists(level, true) > 0
    }

    fn count_scientists(&mut self, level: i32, home_only: bool) -> usize {
        let Some(science) = self.world.resource::<SkillRules>().index_of(SKILL_SCIENCE) else {
            // Домена нет вовсе — тему без допуска возьмёт кто угодно, а с
            // допуском не возьмёт никто.
            return match level <= 0 {
                true => 1,
                false => 0,
            };
        };
        let mut q = self
            .world
            .query_filtered::<(Option<&Skills>, Option<&Away>), With<UnitId>>();
        let rules = self.world.resource::<SkillRules>();
        q.iter(&self.world)
            .filter(|(_, away)| !(home_only && away.is_some()))
            .filter(|(skills, _)| level_of(rules, *skills, science) >= level)
            .count()
    }

    /// Клетки торговых постов в порядке обхода карты — он фиксирован, значит
    /// выбор ячейки детерминирован (§11), как и выбор узла связи в
    /// `relay_cells`.
    fn post_cells(&mut self) -> Vec<(i32, i32)> {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .filter(|&(x, y)| rules.is_trade_post(map.tile_at(x, y)))
            .collect()
    }

    /// Сколько на базе торговых постов — **счёт, а не факт** (§12.55).
    ///
    /// С §12.68 пост — **место, а не лицензия**: ячейка занята сделкой от клика
    /// до вывоза. Но счётность осталась ровно та же, и второй пост по-прежнему
    /// даёт второе окно, — иначе он остаётся декорацией, как бездонный склад до
    /// того, как `capacity` дал складской комнате смысл (§12.16). Рабочим местом
    /// пост при этом не стал: коты к нему **возят**, а возить не значит работать.
    fn trade_posts(&mut self) -> usize {
        self.post_cells().len()
    }

    /// Свободная ячейка под новую сделку — первая по обходу карты, или `None`.
    ///
    /// Ячейка занята, если на ней уже стоит сделка **или** на ней лежит куча:
    /// привезённое покупателю освобождает пост только вывозом (§12.68), и это
    /// то самое давление на логистику, ради которого гараж и заведён. Ленивый
    /// подвоз затыкает торговлю — обратимо и своими силами.
    fn free_post_cell(&mut self) -> Option<(i32, i32)> {
        let taken: Vec<(i32, i32)> = {
            let mut q = self.world.query::<&Deal>();
            q.iter(&self.world).map(|d| d.cell).collect()
        };
        let piles: Vec<(i32, i32)> = {
            let mut q = self.world.query_filtered::<&Position, With<Stack>>();
            q.iter(&self.world).map(|p| (p.x, p.y)).collect()
        };
        self.post_cells()
            .into_iter()
            .find(|c| !taken.contains(c) && !piles.contains(c))
    }

    /// Сколько влезает в контейнер этой клетки; ноль — без предела (§12.90).
    ///
    /// Спрашивается **по клетке, а не вообще**: посты могут быть разными, и
    /// предел у сделки — свойство той ячейки, в которую она ляжет.
    fn lot_at(&mut self, (x, y): (i32, i32)) -> i32 {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        rules.lot_of(map.tile_at(x, y))
    }

    /// Открыта ли автоматика — по правилу на каждое: `(сбыт, производство,
    /// вылазки)`, §12.93.
    ///
    /// Одно место на снимок и на тесты: снапшот на хосте не собрать, а три
    /// почти одинаковых флага рядом — приглашение перепутать их местами, и
    /// заметить это можно было бы только по молча не работающей строке.
    pub(crate) fn auto_gates_open(&self) -> (bool, bool, bool) {
        let gates = self.world.resource::<AutoRules>();
        let techs = self.world.resource::<Techs>();
        (
            gates.sales_open(techs),
            gates.crafting_open(techs),
            gates.raids_open(techs),
        )
    }

    /// Предел одной сделки прямо сейчас — контейнер той ячейки, которую займёт
    /// следующая заявка (§12.90). Ноль — предела нет.
    ///
    /// Свободной ячейки может не быть вовсе; тогда берём первый пост, чтобы
    /// интерфейсу было чем подписать кнопку. Это **не** ворота: ворота считает
    /// `trade` по той клетке, которую и займёт, — второй их экземпляр в JS
    /// однажды разойдётся с фасадом (§12.26).
    fn post_lot(&mut self) -> i32 {
        let cell = self
            .free_post_cell()
            .or_else(|| self.post_cells().into_iter().next());
        cell.map_or(0, |c| self.lot_at(c))
    }

    /// Сколько на базе гаражей — потолок одновременных вылазок (§12.152).
    ///
    /// Третье применение «комната = слот» после мастерской и поста. До §12.152
    /// счёт вела рация (§12.59) — чистая лицензия, за которой никто не работает
    /// и к которой ничего не возят, — а гараж был просто дверью, выбираемой
    /// автоматом. Отряд принадлежит двери: слот, состав и шлюз съехались на одну
    /// клетку, ту самую, которую игрок и нажимает.
    fn gate_nodes(&mut self) -> usize {
        self.gate_cells_of().len()
    }

    /// Клетки гаражей в порядке обхода карты — он фиксирован, значит и номер
    /// отряда, и очередь заявок детерминированы (§11). Список один на фасад и
    /// на систему: считает его `missions::gate_cells`.
    pub(crate) fn gate_cells_of(&mut self) -> Vec<(i32, i32)> {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        gate_cells(map, rules).collect()
    }

    /// Клетки раций в порядке обхода карты. С §12.152 они больше не считают
    /// слоты и не держат состав — только связь (§12.60), — но список нужен
    /// снимку: панель говорит, сколько раций есть и сколько из них с дежурным.
    pub(crate) fn relay_cells(&mut self) -> Vec<(i32, i32)> {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        crate::relay::relay_cells(map, rules).collect()
    }

    /// Гараж, за которым сейчас нет вылазки. Занятость держит сама `Mission`
    /// (`Mission::gate`), отдельного реестра нет — ровно как ячейку станка
    /// держит `Craft::cell`, а лежанку `Rest::spot` (§12.55, §12.96, §12.152).
    ///
    /// **Куча добычи на гараже занятостью не считается** (§12.152). Проверка
    /// «клетка пуста» есть ровно у торгового поста (§12.98) и заведена под его
    /// контейнер; здесь она заперла бы вылазки насмерть при полном складе —
    /// уборке некуда везти, а команды «выбросить» в игре нет.
    fn free_gate_cell(&mut self) -> Option<(i32, i32)> {
        let taken: Vec<(i32, i32)> = {
            let mut q = self.world.query::<&Mission>();
            q.iter(&self.world).map(|m| m.gate).collect()
        };
        self.gate_cells_of()
            .into_iter()
            .find(|cell| !taken.contains(cell))
    }

    /// Гараж ли в этой клетке.
    fn is_gate_at(&mut self, x: i32, y: i32) -> bool {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        rules.is_gate(map.tile_at(x, y))
    }

    /// Рация ли в этой клетке.
    fn is_relay_at(&mut self, x: i32, y: i32) -> bool {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        rules.is_relay_node(map.tile_at(x, y))
    }

    /// Свободен ли **этот** гараж: вылазки за ним сейчас нет.
    fn node_is_free(&mut self, x: i32, y: i32) -> bool {
        let mut q = self.world.query::<&Mission>();
        !q.iter(&self.world).any(|m| m.gate == (x, y))
    }

    /// Постоянный состав отряда этого узла, по `id` (§12.61).
    ///
    /// Порядок — по `id` кота, а не по обходу сущностей: тот зависит от истории
    /// вставок и недетерминирован (§11, инвариант 9), а состав едет и в панель,
    /// и в `launch`, где длина списка сверяется с заказом.
    fn roster_of(&mut self, x: i32, y: i32) -> Vec<String> {
        let mut q = self.world.query::<(&UnitId, &Enlisted)>();
        let mut crew: Vec<String> = q
            .iter(&self.world)
            .filter(|(_, e)| e.spot == (x, y))
            .map(|(id, _)| id.0.clone())
            .collect();
        crew.sort_unstable();
        crew
    }

    /// Клетки мастерских в порядке обхода карты — он фиксирован, значит выбор
    /// ячейки детерминирован (§11), как у постов и узлов связи.
    fn shop_cells(&mut self) -> Vec<(i32, i32)> {
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        (0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .filter(|&(x, y)| rules.is_shop(map.tile_at(x, y)))
            .collect()
    }

    /// Сколько на базе мастерских. Тоже счёт: станок — слот заказа (§12.55).
    fn shops(&mut self) -> usize {
        self.shop_cells().len()
    }

    /// Свободная ячейка лаборатории под новую тему — первая по обходу карты.
    ///
    /// Близнец `free_shop_cell` (§12.132): с §12.132 тема, как заказ и сделка,
    /// живёт **в ячейке** и держит её до последнего очка работы. Считает это
    /// одно место на фасад и снимок — `research::free_lab`; правила-порога у
    /// науки нет, так что третьего зова не появится.
    fn free_lab_cell(&mut self) -> Option<(i32, i32)> {
        let taken: Vec<(i32, i32)> = {
            let mut q = self.world.query::<&Research>();
            q.iter(&self.world).map(|t| t.cell).collect()
        };
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        crate::research::free_lab(map, rules, &taken)
    }

    /// Свободная ячейка под новый заказ — первая по обходу карты, или `None`.
    ///
    /// Близнец `free_post_cell` (§12.68): с §12.96 заказ живёт **в ячейке**, а
    /// не ищет себе станок, и занимает её от заявки до последней штуки. Кучи,
    /// в отличие от поста, ячейку не занимают: готовое ложится под ноги мастеру
    /// и уезжает уборкой, а работать он продолжает там же (§12.30).
    ///
    /// Считает это одно место на фасад, правило и снимок — `crafting::free_shop`.
    fn free_shop_cell(&mut self) -> Option<(i32, i32)> {
        let taken: Vec<(i32, i32)> = {
            let mut q = self.world.query::<&Craft>();
            q.iter(&self.world).map(|c| c.cell).collect()
        };
        let map = self.world.resource::<BaseMap>();
        let rules = self.world.resource::<TileRules>();
        crate::crafting::free_shop(map, rules, &taken)
    }

    /// Ячейка, в которую встанет **ручной** заказ: свободная, а если свободных
    /// нет — занятая неоплаченным заказом правила, вместе с ним самим.
    ///
    /// **Приказ игрока удаляет приказ автопроизводства** (§12.97). Иначе за
    /// починку храповика платили бы кнопки рецептов: правило теперь занимает все
    /// станки, и `RecipeSnap::shop` гас бы почти всегда — а решение игрока
    /// важнее правила, тем более что правило доберёт своё, как только ячейка
    /// освободится. Оплаченный заказ не вытесняется никогда: материал за штуку
    /// уже списан (§12.26).
    ///
    /// Вытесняется **последний по карте** — симметрия срезанию с конца в
    /// `plan_craft`; порядок обхода ECS в выбор не просачивается (§11).
    ///
    /// Считает это одно место на фасад и снимок, как `crafting::free_shop` и
    /// `trade::quote`: разойдись они — кнопка гасла бы там, где клик сработал бы.
    fn spare_shop_cell(&mut self) -> Option<((i32, i32), Option<Entity>)> {
        if let Some(cell) = self.free_shop_cell() {
            return Some((cell, None));
        }
        let mut q = self.world.query::<(Entity, &Craft)>();
        let mut spare: Vec<(Entity, (i32, i32))> = q
            .iter(&self.world)
            .filter(|(_, c)| c.auto && c.delivered.is_empty())
            .map(|(e, c)| (e, c.cell))
            .collect();
        spare.sort_unstable_by_key(|&(_, (x, y))| (y, x));
        spare.pop().map(|(e, cell)| (cell, Some(e)))
    }

    /// Заказ, стоящий в этой ячейке. **Ключ у заказа — клетка, а не рецепт**
    /// (§12.96): заказов на один рецепт бывает несколько, а в одной ячейке —
    /// ровно один.
    fn order_at(&mut self, x: i32, y: i32) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &Craft)>();
        q.iter(&self.world)
            .find(|(_, c)| c.cell == (x, y))
            .map(|(e, _)| e)
    }

    /// Заказы на этот рецепт, отсортированные по клетке: обход ECS зависит от
    /// истории вставок (§11), а «первый по карте» — нет.
    fn orders_of(&mut self, def: usize) -> Vec<Entity> {
        let mut q = self.world.query::<(Entity, &Craft)>();
        let mut found: Vec<(Entity, (i32, i32))> = q
            .iter(&self.world)
            .filter(|(_, c)| c.def == def)
            .map(|(e, c)| (e, c.cell))
            .collect();
        found.sort_unstable_by_key(|&(_, (x, y))| (y, x));
        found.into_iter().map(|(e, _)| e).collect()
    }

    /// Клетки, занятые учениками: и та, за которой сидят, и та, к которой идут.
    fn taken_desks(&mut self) -> Vec<(i32, i32)> {
        let mut q = self.world.query::<&Study>();
        q.iter(&self.world).map(|s| s.spot).collect()
    }
}

/// Собрать кота по правилам рулсета.
///
/// Одно место на стартовую тройку и на любого новичка с найма (§12.24): иначе
/// нанятый однажды приедет с лапами другого размера или без бодрости, и понять
/// это можно будет только по странному поведению.
///
/// Перк превращается в числа здесь, один раз: расти ему всё равно некуда
/// (§12.17). Опыт — стартовый багаж кандидата, у стартовой тройки он пуст.
fn spawn_cat(
    world: &mut World,
    id: &str,
    sprite: &str,
    at: (i32, i32),
    perks: &[String],
    skills: &[(usize, i32)],
    stats: &[(usize, i32)],
) -> Entity {
    let carry = world.resource::<UnitRules>().carry;
    let energy_max = world.resource::<NeedRules>().max;
    let fed_max = world.resource::<FoodRules>().max;
    let health_max = world.resource::<HealthRules>().max;
    // Врождённое собирается первым: потолок стартового опыта зависит от него
    // (§12.42), а не наоборот.
    let born = {
        let mut born = Stats::default();
        for &(stat, value) in stats {
            born.set(stat, value);
        }
        born
    };
    let caps: Vec<(usize, i32)> = {
        let rules = world.resource::<SkillRules>();
        skills
            .iter()
            .map(|&(s, _)| (s, xp_ceiling(rules, Some(&born), s)))
            .collect()
    };

    let hauler = perks.iter().any(|p| p == PERK_HAULER);
    let mut cat = world.spawn((
        UnitId(id.to_string()),
        Renderable {
            sprite: sprite.to_string(),
        },
        Position { x: at.0, y: at.1 },
        Perks(perks.to_vec()),
    ));
    // Ни одного параметра — компоненты тоже нет: так живут коты из ASCII-схем,
    // в мире которых палитры параметров не существует. А вот нулевое значение
    // — это уже свойство кота, и оно видно в карточке (§12.42).
    if !born.0.is_empty() {
        cat.insert(born);
    }
    if carry > 0 {
        cat.insert(Carry(carry * if hauler { 2 } else { 1 }));
    }
    if energy_max > 0 {
        cat.insert(Energy(energy_max));
    }
    // Новичок приходит сытым, как и бодрым: голодать он начнёт на общих
    // основаниях, а не с порога (§12.36).
    if fed_max > 0 {
        cat.insert(Fed(fed_max));
    }
    // И целым: раны приходят только из поля, а нанятый в поле ещё не был
    // (§12.37).
    if health_max > 0 {
        cat.insert(Health(health_max));
    }
    if !skills.is_empty() {
        let mut xp = Skills::default();
        for (&(skill, amount), &(_, cap)) in skills.iter().zip(&caps) {
            xp.add_xp(skill, amount, cap.max(amount));
        }
        cat.insert(xp);
    }
    cat.id()
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
        let item_index = |id: &str| rs.items.iter().position(|i| i.id == id);

        let mut map = BaseMap::empty(w, h);
        for b in &rs.build {
            if let Some(idx) = tile_index(&b.tile) {
                map.fill_rect(b.rect, idx);
            }
        }

        let mut world = World::new();
        world.insert_resource(map);
        world.insert_resource(SimTime { tick: 0 });
        // Навыки заводим до тайлов: парта ссылается на домен по имени, а в
        // правилах остаётся его индекс — как цена ссылается на предмет.
        let skill_index = |id: &str| rs.skills.iter().position(|s| s.id == id);
        // Параметр навык тоже держит индексом: имена живут в рулсете, по коду
        // ходят номера палитры (§12.42).
        let stat_index = |id: &str| rs.stats.iter().position(|s| s.id == id);
        // Ступени параметров (§12.70): что параметр делает **прямо сейчас**, в
        // отличие от `demands` у навыка, который задаёт его потолок (§12.42).
        world.insert_resource(StatRules(
            rs.stats
                .iter()
                .map(|s| StatRule {
                    id: s.id.clone(),
                    steps: s.steps.clone(),
                })
                .collect(),
        ));
        world.insert_resource(SkillRules(
            rs.skills
                .iter()
                .map(|s| SkillRule {
                    id: s.id.clone(),
                    levels: s.levels.clone(),
                    taught: s.taught,
                    stat: stat_index(&s.stat),
                    demands: s.demands.clone(),
                })
                .collect(),
        ));
        // Цена из рулсета — имена предметов; в правилах остаются индексы палитры.
        // Порядок пар задан `BTreeMap` (по имени), то есть детерминирован (§12.21).
        world.insert_resource(TileRules(
            rs.tiles
                .iter()
                .map(|t| TileRule {
                    cost: t
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    capacity: t.capacity,
                    rest: t.rest,
                    wake: t.wake,
                    heal: t.heal,
                    gate: t.gate,
                    teaches: skill_index(&t.teaches),
                    lab: t.lab,
                    shop: t.shop,
                    solid: t.solid,
                    trade: t.trade,
                    lot: t.lot,
                    relay: t.relay,
                    comms: t.comms,
                    tech: t.tech.clone(),
                    quiet: t.quiet,
                    noisy: t.noisy,
                    clean: t.clean,
                    dirty: t.dirty,
                })
                .collect(),
        ));
        world.insert_resource(ResearchRules(
            rs.research
                .iter()
                .map(|r| ResearchRule {
                    id: r.id.clone(),
                    level: r.level,
                    work: r.work,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    specimen: r
                        .specimen
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    gives: r
                        .gives
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    requires: r.requires.clone(),
                })
                .collect(),
        ));
        world.insert_resource(CraftRules(
            rs.recipes
                .iter()
                .map(|r| CraftRule {
                    id: r.id.clone(),
                    work: r.work,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    gives: r
                        .gives
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    requires: r.requires.clone(),
                    salvage: r.salvage,
                })
                .collect(),
        ));
        world.insert_resource(Techs::default());
        world.insert_resource(TimelineRules(
            rs.timeline
                .iter()
                .map(|e| EventRule {
                    at: e.at,
                    reveal: e.reveal,
                    requires: e.requires.clone(),
                    gift: e
                        .gift
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    fame: e.fame,
                    toll: e.toll,
                })
                .collect(),
        ));
        world.insert_resource(Chronicle::default());
        // Цели партии (§12.58). Индексы палитр — те же, что везде: тайл, миссия
        // и рецепт адресуются номером записи, технология — именем.
        let tile_index = |id: &str| rs.tiles.iter().position(|t| t.id == id);
        let mission_index = |id: &str| rs.missions.iter().position(|m| m.id == id);
        let recipe_index = |id: &str| rs.recipes.iter().position(|r| r.id == id);
        world.insert_resource(GoalRules(
            rs.goals
                .iter()
                .filter_map(|g| {
                    // Условие — ровно одно из полей. Порядок веток здесь и есть
                    // приоритет при случайно заполненных двух: описан он в
                    // `GoalDef`, а спорную запись ловит `every_goal_is_reachable`.
                    let test = if let Some(id) = &g.tile {
                        GoalTest::Tile(tile_index(id)?, g.count.max(1))
                    } else if !g.stored.is_empty() {
                        GoalTest::Stored(
                            g.stored
                                .iter()
                                .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                                .collect(),
                        )
                    } else if let Some(id) = &g.tech {
                        GoalTest::Tech(id.clone())
                    } else if g.cats > 0 {
                        GoalTest::Cats(g.cats)
                    } else if let Some(id) = &g.raid {
                        GoalTest::Raid(mission_index(id)?)
                    } else if let Some(id) = &g.craft {
                        GoalTest::Craft(recipe_index(id)?)
                    } else if g.earned > 0 {
                        GoalTest::Earned(g.earned)
                    } else {
                        return None; // цель без условия — не цель
                    };
                    Some(GoalRule {
                        test,
                        hidden: g.hidden,
                    })
                })
                .collect(),
        ));
        world.insert_resource(Goals::default());
        world.insert_resource(Raids::default());
        world.insert_resource(Crafted::default());
        world.insert_resource(Earned::default());
        // Фракции — такая же палитра, как предметы и параметры: в правилах от
        // фракции остаётся индекс записи (§12.43).
        let faction_index = |id: &str| rs.factions.iter().position(|f| f.id == id);
        world.insert_resource(FactionRules(
            rs.factions
                .iter()
                .map(|f| FactionRule {
                    span: f.span,
                    lead: f.lead,
                    spread: f.spread,
                    favor: f.favor,
                    period: f.period,
                    // Прайс упорядочен по индексу предмета, а не по имени: он
                    // доходит до раздачи задач и до цены на кнопке, а
                    // недетерминированный обход ломает и тесты, и модель
                    // времени (§11, §12.21).
                    prices: {
                        let mut list: Vec<(usize, PriceLine)> = f
                            .prices
                            .iter()
                            .filter_map(|(id, def)| {
                                item_index(id).map(|i| {
                                    // `needs` упорядочен по индексу фракции —
                                    // дословно `needs` у вылазки и кандидата.
                                    let mut needs: Vec<(usize, i32)> = def
                                        .needs
                                        .iter()
                                        .filter_map(|(id, &n)| faction_index(id).map(|f| (f, n)))
                                        .collect();
                                    needs.sort_unstable_by_key(|&(f, _)| f);
                                    let line = PriceLine {
                                        phases: def.phases.clone(),
                                        requires: def.requires,
                                        needs,
                                    };
                                    (i, line)
                                })
                            })
                            .collect();
                        list.sort_unstable_by_key(|(i, _)| *i);
                        list
                    },
                })
                .collect(),
        ));
        world.insert_resource(Standing::default());
        world.insert_resource(Money::default());
        world.insert_resource(MissionRules(
            rs.missions
                .iter()
                .map(|m| MissionRule {
                    squad: m.squad.bounds().0,
                    squad_max: m.squad.bounds().1,
                    stealth: m.stealth,
                    travel: m.travel,
                    work: m.work,
                    danger: m.danger,
                    toll: m.toll,
                    harm: m.harm,
                    loot: m
                        .loot
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    fame: m.fame,
                    requires: m.requires,
                    rescue: m.rescue,
                    patron: faction_index(&m.patron),
                    against: faction_index(&m.against),
                    standing: m.standing,
                    needs: m
                        .needs
                        .iter()
                        .filter_map(|(id, &n)| faction_index(id).map(|f| (f, n)))
                        .collect(),
                })
                .collect(),
        ));
        world.insert_resource(RecruitRules(
            rs.recruits
                .iter()
                .map(|r| RecruitRule {
                    id: r.id.clone(),
                    sprite: r.sprite.clone(),
                    requires: r.requires,
                    cost: r
                        .cost
                        .iter()
                        .filter_map(|(id, &n)| item_index(id).map(|i| (i, n)))
                        .collect(),
                    skills: r
                        .skills
                        .iter()
                        .filter_map(|(id, &xp)| skill_index(id).map(|i| (i, xp)))
                        .collect(),
                    stats: r
                        .stats
                        .iter()
                        .filter_map(|(id, &v)| stat_index(id).map(|i| (i, v)))
                        .collect(),
                    perks: r.perks.clone(),
                    needs: r
                        .needs
                        .iter()
                        .filter_map(|(id, &n)| faction_index(id).map(|f| (f, n)))
                        .collect(),
                })
                .collect(),
        ));
        world.insert_resource(Fame::default());
        // Снаряжение — свойство предмета, а шаблон ссылается на предметы по
        // имени: в правилах, как и в цене тайла, остаются индексы палитры.
        world.insert_resource(ItemRules(
            rs.items
                .iter()
                .map(|i| ItemRule {
                    force: i.force,
                    nutrition: i.nutrition,
                    mends: i.mends,
                    requires: i.requires.clone(),
                })
                .collect(),
        ));
        world.insert_resource(LoadoutRules(
            rs.loadout.iter().filter_map(|id| item_index(id)).collect(),
        ));
        world.insert_resource(UnitRules { carry: rs.carry });
        world.insert_resource(AutoTidy(true));
        world.insert_resource(AutoRest(true));
        // Порогов автопроизводства в новой партии нет ни одного: правило — это
        // решение игрока, а не стартовая настройка (§12.65).
        world.insert_resource(Stocking::default());
        // И автовылазок тоже: узел, только что построенный, никуда сам не ходит
        // (§12.67). Правило — решение игрока, а не свойство рации.
        world.insert_resource(AutoRaids::default());
        // И автопродажи: база, которая с первого тика сбывает «излишки», сама
        // решала бы, что излишек, — а сделка не отменяется (§12.44, §12.87).
        world.insert_resource(Selling::default());
        // Закладки игрока в окне «Склад» (§12.100): что стоит наверху списка и
        // что вынесено лентой на главный экран. Тикеры пустые — новая база сама
        // ничего на ленту не выносит, и та прячется, как любая пустая панель.
        // А вот избранное начинается не с нуля (§12.112): им же отобраны строки
        // шапки, и пустой список показал бы там всю палитру. Кто в нём стоит —
        // контент (`favorite:` у предмета), а не число в коде.
        world.insert_resource(Favorites(
            rs.items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.favorite)
                .map(|(i, _)| i)
                .collect(),
        ));
        world.insert_resource(Tickers::default());
        // Что база уже видела своими глазами (§12.131). Пустой заводится, а
        // наполняется ниже — одним прогоном наблюдателя по собранному миру:
        // стартовый склад база видела заведомо, и ждать первого тика тут
        // нельзя. Партия открывается **на паузе**, то есть тика может не быть
        // ещё долго, и шапка успела бы сказать «ресурсы не выбраны» про
        // выбранные рулсетом.
        world.insert_resource(Seen(vec![false; rs.items.len()]));
        // Лента новостей (§12.120). Базовой линии у неё пока нет: снимет её
        // первый же `note_news`, и стартовая доступность новостью не станет.
        world.insert_resource(News::default());
        // Ворота автоматики — контент, а не состояние (§12.93): пустое имя
        // технологии значит «правило доступно сразу», и так живут все
        // синтетические миры тестов.
        world.insert_resource(AutoRules {
            sales: rs.automation.sales.clone(),
            crafting: rs.automation.crafting.clone(),
            raids: rs.automation.raids.clone(),
        });
        world.insert_resource(Trace::default());
        world.insert_resource(NeedRules {
            max: rs.energy.max,
            drain: rs.energy.drain,
            walk: rs.energy.walk,
            idle: rs.energy.idle,
            tired: rs.energy.tired,
            critical: rs.energy.critical,
            floor: rs.energy.floor,
            floor_wake: rs.energy.floor_wake,
        });
        world.insert_resource(FoodRules {
            max: rs.food.max,
            hungry: rs.food.hungry,
            starve: rs.food.starve,
        });
        world.insert_resource(HealthRules {
            max: rs.health.max,
            hurt: rs.health.hurt,
            mend: rs.health.mend,
        });

        // Стартовый запас. Стартовая застройка (`build`) при этом бесплатна —
        // это уже существующая база, а не работа котов.
        for s in &rs.stock {
            let Some(item) = item_index(&s.item) else {
                continue; // предмета нет в палитре — запись контента мимо
            };
            world.spawn((
                Position {
                    x: s.at[0],
                    y: s.at[1],
                },
                Stack {
                    item,
                    count: s.count,
                },
            ));
        }

        for u in &rs.units {
            let stats: Vec<(usize, i32)> = u
                .stats
                .iter()
                .filter_map(|(id, &v)| stat_index(id).map(|i| (i, v)))
                .collect();
            spawn_cat(
                &mut world,
                &u.id,
                &u.sprite,
                (u.pos[0], u.pos[1]),
                &u.perks,
                &[],
                &stats,
            );
        }

        // Стартовый склад база видела заведомо (§12.131). Прогоняем наблюдателя
        // сразу, а не ждём первого тика: партия открывается на паузе, и тика
        // может не быть ещё долго. Тот же наблюдатель, что в цепочке, — второй
        // экземпляр этого обхода разошёлся бы с первым на первой же правке.
        let _ = world.run_system_once(note_seen);

        let schedule = build_schedule();
        Ok(Sim {
            world,
            schedule,
            palette: rs.tiles,
            items: rs.items,
            skills: rs.skills,
            stats: rs.stats,
            perks: rs.perks,
            factions: rs.factions,
            missions: rs.missions,
            recruits: rs.recruits,
            research: rs.research,
            recipes: rs.recipes,
            timeline: rs.timeline,
            goals: rs.goals,
            width: w,
            height: h,
            day: rs.day,
            news: rs.news,
            ruleset: fingerprint(ruleset_yaml),
        })
    }

    /// Снять снимок партии (§12.45). Наружу — JSON-текст: главный поток кладёт
    /// его в `localStorage` или отдаёт файлом, воркеру хранить нечем.
    pub fn save(&self) -> Result<String, JsValue> {
        let file = capture(&self.world, self.ruleset);
        serde_json::to_string(&file)
            .map_err(|e| JsValue::from_str(&format!("save serialize error: {e}")))
    }

    /// Собрать мир из снимка. Правила берутся из рулсета — в снимке их нет и
    /// быть не должно (§12.45), — поэтому текст YAML нужен и здесь.
    ///
    /// Два отказа, и оба явные: чужая версия формата и чужой рулсет. Индексы
    /// палитр лежат в снимке числами, а имена — только в YAML, поэтому загрузка
    /// снимка от другого контента дала бы не ошибку, а тихо другой мир.
    pub fn load(ruleset_yaml: &str, save_json: &str) -> Result<Sim, JsValue> {
        Sim::load_from(ruleset_yaml, save_json).map_err(|e| JsValue::from_str(&e))
    }

    /// Отладочный журнал команд — «как игрок дошёл до такого состояния»
    /// (§12.45). Партию он не восстанавливает и источником правды не является.
    pub fn trace(&self) -> String {
        self.world
            .resource::<Trace>()
            .0
            .iter()
            .map(|e| format!("{}\t{}", e.tick, e.cmd))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Размеры и палитра тайлов — отдаём один раз для настройки рендера.
    pub fn map_meta(&self) -> Result<JsValue, JsValue> {
        let meta = MapMeta {
            width: self.width,
            height: self.height,
            day: self.day,
            news: self.news,
            palette: self.palette.clone(),
            items: self.items.clone(),
            skills: self.skills.clone(),
            stats: self.stats.clone(),
            // Какой параметр ведёт отряд — знание ядра, и наружу оно уходит
            // номером (§12.71): панель называет реакцию по имени и показывает
            // её значение, а искать `reflex` строкой в JS — второй экземпляр
            // правила.
            guide_stat: self
                .world
                .resource::<StatRules>()
                .index_of(crate::missions::STAT_GUIDE)
                .map_or(-1, |i| i as i32),
            // Тем же номером и по той же причине едет домен силы в поле.
            raid_skill: self
                .world
                .resource::<SkillRules>()
                .index_of(crate::skills::SKILL_RAID)
                .map_or(-1, |i| i as i32),
            // Ярлыки тем-ворот (§12.93): вид обязан назвать отказ по имени темы,
            // а не «нужна какая-то наука». Ищем тему по `id` из рулсета — имён
            // технологий в ядре нет, они контент.
            auto_gates: {
                let gates = self.world.resource::<AutoRules>();
                let name = |id: &str| {
                    if id.is_empty() {
                        return String::new();
                    }
                    self.research
                        .iter()
                        .find(|t| t.id == id)
                        .map_or_else(|| id.to_string(), |t| t.label.clone())
                };
                AutoGateNames {
                    sales: name(&gates.sales.clone()),
                    crafting: name(&gates.crafting.clone()),
                    raids: name(&gates.raids.clone()),
                }
            },
            perks: self.perks.clone(),
            factions: self.factions.clone(),
            missions: self.missions.clone(),
            recruits: self.recruits.clone(),
            research: self.research.clone(),
            recipes: self.recipes.clone(),
            goals: self.goals.clone(),
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

    /// Маска ворот разметки (§12.111, §12.157) для превью: по байту на клетку, `1` —
    /// тайл здесь поставить можно, `0` — нельзя. **Пустой вектор значит
    /// «правило к этому тайлу не применимо»**, и рендер не красит ничего.
    ///
    /// Считает ядро, а не вид: второй экземпляр правила в JS однажды разойдётся
    /// с этим и покажет клетку, которую фасад отклонит (§12.53). Зовётся маска
    /// каждым кадром — она зависит от чертежей, а те меняются между кадрами, —
    /// и стоит это прохода по карте, то есть меньше самого снапшота.
    ///
    /// `(rx, ry, rw, rh)` — рамка, которую игрок тянет прямо сейчас; нулевая
    /// ширина значит «рамки нет». Клетки внутри неё считаются **по порядку и с
    /// накоплением**, ровно как их разметит `add_blueprint_rect`: каждая
    /// принятая меняет ответ для следующих. Без этого превью обещало бы девять
    /// полок из мазка три на три, а разметилось бы восемь — то самое молчаливое
    /// расхождение, ради которого маска и заводится.
    pub fn buildable(&mut self, tile: i32, rx: i32, ry: i32, rw: i32, rh: i32) -> Vec<u8> {
        let t = tile as i16;
        if !self.gated_by_placement(t) {
            return Vec::new();
        }
        let mut plan = self.plan();
        let rules = self.world.resource::<TileRules>();
        let mut mask = vec![0u8; (self.width * self.height) as usize];
        let mut done = vec![false; mask.len()];
        for (cx, cy) in rect_cells(rx, ry, rw, rh) {
            let ok = may_build(&plan, rules, (cx, cy), t);
            if ok {
                plan.set(cx, cy, t);
            }
            if let Some(i) = self.cell_index(cx, cy) {
                mask[i] = u8::from(ok);
                done[i] = true;
            }
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let i = (y * self.width + x) as usize;
                if !done[i] {
                    mask[i] = u8::from(may_build(&plan, rules, (x, y), t));
                }
            }
        }
        mask
    }

    /// Подчиняется ли тайл воротам разметки вообще: полка (§12.111) или клетка
    /// со свойством зонирования (§12.157). Одно выражение на маску, потому что
    /// «правило неприменимо» — это её пустой вектор, и ошибиться здесь значит
    /// либо не показать креста, либо считать маску всей карты на каждый пол.
    fn gated_by_placement(&self, tile: i16) -> bool {
        let rules = self.world.resource::<TileRules>();
        rules.is_solid(tile)
            || rules.is_quiet(tile)
            || rules.is_noisy(tile)
            || rules.is_clean(tile)
            || rules.is_dirty(tile)
    }

    /// Номер клетки в маске; `None` — за картой.
    fn cell_index(&self, x: i32, y: i32) -> Option<usize> {
        self.in_bounds(x, y).then(|| (y * self.width + x) as usize)
    }

    /// Поставить чертёж (джоб) `tile` на клетку (x, y). Выполняют коты, по тикам.
    /// `tile = -1` — чертёж сноса: кот придёт на соседнюю клетку и уберёт пол.
    /// Вернёт true, если чертёж добавлен/обновлён.
    pub fn add_blueprint(&mut self, x: i32, y: i32, tile: i32) -> bool {
        note(&mut self.world, format!("build {x} {y} {tile}"));
        if !self.in_bounds(x, y) {
            return false;
        }
        let t = tile as i16;
        // Технология — ворота палитры (§12.27). Проверяем в момент разметки, а
        // не в момент работы: чертёж, который никто никогда не возьмёт, игрок
        // прочтёт как поломку, а не как «сперва изучите». У сноса ворот нет —
        // разбирать можно что угодно и всегда.
        if !self.tech_allows(t) {
            return false;
        }
        // Правило доступа (§12.111) и зонирование (§12.157) — вторые ворота
        // разметки и по тому же доводу: у полки обязан быть проход, а лежанка
        // вплотную к цеху — это спальня, в которой не спят. Считается по плану,
        // то есть с учётом уже размеченного, — иначе одна рамка соберёт монолит.
        let plan = self.plan();
        if !may_build(&plan, self.world.resource::<TileRules>(), (x, y), t) {
            return false;
        }
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
            delivered: Vec::new(),
        });
        true
    }

    /// Поставить чертежи `tile` на весь прямоугольник — один жест рамкой.
    /// Вернёт true, если хоть одна клетка изменилась.
    pub fn add_blueprint_rect(&mut self, x: i32, y: i32, w: i32, h: i32, tile: i32) -> bool {
        note(
            &mut self.world,
            format!("build_rect {x} {y} {w} {h} {tile}"),
        );
        let mut changed = false;
        for (cx, cy) in rect_cells(x, y, w, h) {
            changed |= self.add_blueprint(cx, cy, tile);
        }
        changed
    }

    /// Ластик игрока: отменить чертёж, либо запланировать снос построенного тайла.
    ///
    /// Отмена плана мгновенна — строить ещё не начинали, и большая часть «ой, не
    /// туда» приходится именно на неё. Снос готового тайла идёт через ту же
    /// очередь, что и стройка: чертёж с `tile = -1`, кот приходит на соседнюю
    /// клетку и работает. Повторный ластик по клетке с чертежом сноса отменяет
    /// его. Вернёт true, если что-то изменилось.
    pub fn plan_demolish(&mut self, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("erase {x} {y}"));
        self.plan_demolish_rect(x, y, 1, 1)
    }

    /// Ластик по прямоугольнику. Решение принимается на всю рамку сразу: сперва
    /// снимаем чертежи, и только если снимать было нечего — планируем снос пола.
    ///
    /// Потайловый переключатель на рамке дал бы кашу: там, где часть клеток уже
    /// запланирована, один жест половину снял бы, а половину поставил, и результат
    /// зависел бы от того, что игрок делал раньше. Порядок «сначала отмена» ещё и
    /// безопаснее — отмена ничего не разрушает.
    pub fn plan_demolish_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        note(&mut self.world, format!("erase_rect {x} {y} {w} {h}"));
        let cells: Vec<(i32, i32)> = rect_cells(x, y, w, h).collect();
        let mut cancelled = false;
        for &(cx, cy) in &cells {
            cancelled |= self.cancel_blueprint(cx, cy);
        }
        if cancelled {
            return true;
        }
        let mut planned = false;
        for &(cx, cy) in &cells {
            planned |= self.add_blueprint(cx, cy, -1);
        }
        planned
    }

    /// Убирать ли лом с пола без приказа игрока.
    ///
    /// Выключение снимает все пометки и разворачивает котов, которые шли за
    /// кучей: иначе переключатель выглядел бы сломанным — коты продолжали бы
    /// разбирать помеченное. Кот, уже несущий груз, свою ходку доводит до
    /// конца: бросать лом посреди базы хуже, чем донести (§12.16).
    pub fn set_auto_tidy(&mut self, on: bool) {
        note(&mut self.world, format!("auto_tidy {on}"));
        self.world.resource_mut::<AutoTidy>().0 = on;
        if on {
            return;
        }

        let mut marked = self.world.query_filtered::<Entity, With<ToStore>>();
        for e in marked.iter(&self.world).collect::<Vec<_>>() {
            self.world.entity_mut(e).remove::<ToStore>();
        }

        let mut q = self.world.query::<(Entity, &Haul, Option<&Carrying>)>();
        let going: Vec<Entity> = q
            .iter(&self.world)
            .filter(|(_, haul, load)| matches!(haul.to, HaulTo::Store(_)) && load.is_none())
            .map(|(e, ..)| e)
            .collect();
        for e in going {
            self.world.entity_mut(e).remove::<(Haul, Path, Stride)>();
        }
    }

    /// Бросает ли кот начатое, когда бодрость ушла ниже критического порога
    /// (§12.33). Выключено — коты работают до нуля и валятся где стоят, как до
    /// второго порога.
    ///
    /// В отличие от `set_auto_tidy`, выключение **никого не будит**: уже
    /// спящий кот — это состояние, а не пометка игрока, и снимать её значило бы
    /// поднимать котов на нуле бодрости, чтобы они тут же упали снова.
    pub fn set_auto_rest(&mut self, on: bool) {
        note(&mut self.world, format!("auto_rest {on}"));
        self.world.resource_mut::<AutoRest>().0 = on;
    }

    /// Пометить кучи под рамкой «на склад» — или снять пометку.
    ///
    /// Решение принимается на всю рамку сразу, по правилу ластика (§12.13):
    /// есть под рамкой хоть одна помеченная куча — снимаем все пометки, нет —
    /// помечаем все. Кота выбирать не нужно: как и чертёж, это разметка работы,
    /// а возьмёт её любой свободный.
    ///
    /// Вернёт true, если что-то изменилось.
    pub fn mark_to_store_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        note(&mut self.world, format!("store_rect {x} {y} {w} {h}"));
        let cells: Vec<(i32, i32)> = rect_cells(x, y, w, h).collect();
        let mut q = self
            .world
            .query::<(Entity, &Position, Option<&ToStore>, &Stack)>();
        let under: Vec<(Entity, bool)> = q
            .iter(&self.world)
            .filter(|(_, p, ..)| cells.contains(&(p.x, p.y)))
            .map(|(e, _, mark, _)| (e, mark.is_some()))
            .collect();
        if under.is_empty() {
            return false;
        }

        let unmark = under.iter().any(|&(_, marked)| marked);
        for (e, marked) in under {
            match (unmark, marked) {
                (true, true) => {
                    self.world.entity_mut(e).remove::<ToStore>();
                }
                (false, false) => {
                    self.world.entity_mut(e).insert(ToStore);
                }
                _ => {}
            }
        }
        true
    }

    /// Отправить названных котов на миссию `def` (индекс в палитре `map_meta`).
    ///
    /// **Отряд выбирает игрок, поимённо** — единственная работа, где исполнитель
    /// не раздаётся симуляцией (§12.23). Причина одна: от состава зависит исход.
    /// Недокомплект разрешён (§12.113): минимум вилки — рекомендация, а не
    /// допуск, и платит за него сам игрок — долей добычи и сроком. Безусловный
    /// минимум остался у вылазки за своим: им меряется обратимость плена.
    ///
    /// Заявка снимает с выбранных текущие задачи — как приказ игрока (§12.15):
    /// решение отправить кота в поле весомее начатой им стройки. **Кроме сна**
    /// (§12.51): пока включено «Беречь себя», спящий боец досыпает своё, а
    /// отряд его ждёт — ровно так же, как ждёт истощённого (§12.23). Иначе
    /// выключатель отменял бы только второй порог, а вылазка поднимала бы кота
    /// с лежанки в обход обоих.
    ///
    /// **Вылазок идёт столько, сколько узлов связи** (§12.59): узел — это слот,
    /// как мастерская для заказа и пост для сделки (§12.55). Ноль узлов — ноль
    /// вылазок, исключения «одна всегда бесплатна» нет: она сделала бы первый
    /// узел бесполезным, а правило — правилом с оговоркой.
    ///
    /// **Двух вылазок по одному заказу не бывает.** Отменять их пришлось бы по
    /// номеру в списке, а порядок обхода сущностей ECS недетерминирован (§11) —
    /// та же причина, по которой заказ отменяется по рецепту (§12.55). Читается
    /// это и без кода: заказ у фракции уже взят.
    ///
    /// Вернёт false, если миссии нет, все узлы заняты, эта вылазка уже идёт,
    /// состав не тот или до общего шлюза дойдут не все.
    ///
    /// **Наружу эта форма не ходит** (§12.61): в интерфейсе состав живёт на узле,
    /// и `launch_node` — единственная кнопка. Здесь она осталась примитивом, на
    /// котором стоят тесты механики: «этот отряд, любой свободный узел».
    #[cfg(test)]
    pub(crate) fn launch(&mut self, def: usize, units: Vec<String>) -> bool {
        note(&mut self.world, format!("launch {def} {}", units.join(",")));
        self.launch_at(def, units, None)
    }

    /// Отправить в вылазку отряд, приписанный к гаражу `(x, y)` (§12.61,
    /// §12.152).
    ///
    /// **Гараж заменяет выбор отряда**, и это вся суть этапа: состав хранится на
    /// клетке (`Enlisted`), а не собирается кликами перед каждым уходом. Отсюда и
    /// адресация по клетке, а не по отряду: гаражей может быть несколько, и
    /// кнопка вылазки обязана знать, чей отряд идёт. Отвергнуто «оба способа
    /// разом» (выделенные коты перекрывают приписку): два источника правды о
    /// том, кто идёт, — и игрок не знает, какой сработает.
    ///
    /// Слот берётся **именно этот**, а не первый свободный: гараж, который игрок
    /// нажал, и дверь, из которой отряд уйдёт, обязаны совпадать. До §12.152
    /// слот брала рация, а дверь подбирал `pick_gate` — и совпадать им было
    /// нечем.
    ///
    /// Вернёт false, если в клетке не гараж, гараж уже занят вылазкой или его
    /// отряд не подходит заказу — всё то же, что и у `launch`.
    pub fn launch_node(&mut self, def: usize, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("launch_node {def} {x} {y}"));
        if !self.is_gate_at(x, y) || !self.node_is_free(x, y) {
            return false;
        }
        // **Идут все или никто** (§12.148). Отряд, собранный наполовину, ушёл
        // бы просевшей долей и вернулся бы с ранами, а половина, оставшаяся
        // дома, ждала бы его без дела: цена решения размазана по двум местам, и
        // ни в одном не видна целиком. Ворота здесь ровно те же, что у правила
        // автовылазки (`squad_is_fit`), — второй их экземпляр разошёлся бы с
        // ним, и одна и та же бригада уходила бы по кнопке и стояла по правилу.
        //
        // Отказ называется словом в штабе (`NodeSnap::fit`), а чинится он двумя
        // способами и оба — решение игрока: подождать или вычеркнуть неготового
        // с узла.
        if !self.squad_is_fit(x, y) {
            return false;
        }
        let units = self.roster_of(x, y);
        if !self.launch_at(def, units, Some((x, y))) {
            return false;
        }
        // Отправили этот узел **на другой** заказ — правило автовылазки
        // усыплено (§12.72, §12.77). Оно повторяет решение игрока, а игрок
        // только что принял другое: правило, пережившее явный приказ, вернуло
        // бы отряд «сам собой» тем же тиком, каким тот дошёл до базы, — то есть
        // коты вернулись бы с назначенной вылазки и тут же исчезли снова.
        //
        // Но именно усыплено, а не стёрто: разовое дело кончится, а круг
        // останется тем же, и заново искать его среди заказов значило бы
        // принимать то же решение второй раз (§12.77).
        //
        // Сравнение с `def` здесь не деталь, а вся граница: правило само уходит
        // через эту же кнопку (`run_auto_raids`), но всегда со **своим**
        // заказом, — значит усыпить себя оно не может. Ручная отправка на тот
        // же заказ правила тоже не трогает: другого решения игрок не принимал.
        if self
            .world
            .resource::<AutoRaids>()
            .of(x, y)
            .is_some_and(|rule| rule != def)
        {
            self.world.resource_mut::<AutoRaids>().set_on(x, y, false);
        }
        true
    }

    /// Кто из списка уйдёт прямо сейчас — сущность и клетка (§12.70).
    ///
    /// **Одно место на кнопку и на панель.** Цена решения обязана быть видна до
    /// нажатия (сколько лап уйдёт → срок и доля), а второй экземпляр правила
    /// «кто готов» однажды разошёлся бы с фасадом, и панель обещала бы состав,
    /// которого вылазка не увидит, — та же болезнь, из-за которой свободу
    /// ячейки поста считает одно выражение (§12.68).
    ///
    /// Дубликаты отсекаются по сущности: «три раза excellent» — это один кот,
    /// а не отряд.
    fn ready_crew(&mut self, units: &[String]) -> Vec<(Entity, (i32, i32))> {
        let spare = self.world.resource::<AutoRest>().0;
        let hurt = self.world.resource::<HealthRules>().hurt;
        let mut crew: Vec<(Entity, (i32, i32))> = Vec::new();
        let mut q = self.world.query::<(
            Entity,
            &UnitId,
            &Position,
            Option<&Away>,
            Option<&Health>,
            Option<&Rest>,
        )>();
        for id in units {
            let found = q
                .iter(&self.world)
                .find(|(_, u, _, away, health, rest)| {
                    u.0 == *id
                        && away.is_none()
                        && !(spare && rest.is_some())
                        && health.is_none_or(|h: &Health| h.0 > hurt)
                })
                .map(|(e, _, p, ..)| (e, (p.x, p.y)));
            if let Some(cat) = found
                && !crew.iter().any(|&(e, _)| e == cat.0)
            {
                crew.push(cat);
            }
        }
        crew
    }

    /// Срок каждого заказа для отряда этого узла (§12.70) — цена до нажатия.
    /// Тем же выражением, каким срок замёрзнет на уходе.
    fn node_spans(&mut self, x: i32, y: i32) -> Vec<i32> {
        let paws = self.ready_roster_of(x, y).len();
        let rules = self.world.resource::<MissionRules>();
        rules.0.iter().map(|r| duration(r, paws)).collect()
    }

    /// Опасность каждого заказа для отряда этого узла (§12.70): уже урезанная
    /// проводником, тем же выражением, что и на возвращении.
    fn node_dangers(&mut self, x: i32, y: i32) -> Vec<i32> {
        let guide = self.node_guide_step(x, y);
        // Число лап нужно разведке (§12.113): у неё опасность растёт с составом,
        // и прогноз обязан показать этот рост до нажатия.
        let paws = self.ready_roster_of(x, y).len();
        let rules = self.world.resource::<MissionRules>();
        rules
            .0
            .iter()
            .map(|r| crew_danger(r, guide, paws))
            .collect()
    }

    /// Вклад каждого готового кота в силу отряда, в порядке `ready_roster_of`
    /// (§12.71): сам кот стоит единицу, уровень «Вылазки» — сверху, надетое —
    /// ещё сверху. Ровно то же выражение, что в `run_missions` и в прогнозе
    /// идущей вылазки, — второй экземпляр в JS однажды пообещал бы игроку не ту
    /// силу, с которой отряд уйдёт (инвариант 14).
    fn node_forces(&mut self, x: i32, y: i32) -> Vec<i32> {
        let roster = self.roster_of(x, y);
        let crew = self.ready_crew(&roster);
        let raid = self.world.resource::<SkillRules>().index_of(SKILL_RAID);
        let skill_rules = self.world.resource::<SkillRules>();
        let items = self.world.resource::<ItemRules>();
        crew.iter()
            .map(|&(e, _)| {
                1 + raid.map_or(0, |s| level_of(skill_rules, self.world.get::<Skills>(e), s))
                    + items.force_of_gear(self.world.get::<Gear>(e))
            })
            .collect()
    }

    /// Прогноз исхода по каждому заказу для отряда этого узла (§12.71): доля
    /// добычи и провал, теми же `outcome`/`raid_danger`, какими они посчитаются
    /// на возвращении. Связь в силу здесь не входит намеренно: она копится за
    /// время вылазки и на уходе равна нулю (§12.60).
    fn node_outcomes(&mut self, x: i32, y: i32) -> (Vec<i32>, Vec<bool>) {
        let forces = self.node_forces(x, y);
        let dangers = self.node_dangers(x, y);
        // Свёртка вкладов — по заказу, а не по отряду (§12.113): на разведке
        // сила это лучший, а не сумма, и заказы стоят в одном списке.
        let rules = self.world.resource::<MissionRules>();
        let out: Vec<crate::missions::Outcome> = rules
            .0
            .iter()
            .zip(&dangers)
            .map(|(r, &d)| outcome(d, crew_force(r, forces.iter().copied())))
            .collect();
        (
            out.iter().map(|o| o.share).collect(),
            out.iter().map(|o| o.failed).collect(),
        )
    }

    /// Ступень проводника среди готовых уйти — лучшая «Реакция» (§12.70).
    fn node_guide_step(&mut self, x: i32, y: i32) -> i32 {
        let roster = self.roster_of(x, y);
        let crew = self.ready_crew(&roster);
        let stats = self.world.resource::<StatRules>();
        crew.iter()
            .map(|&(e, _)| guide_of(stats, self.world.get::<Stats>(e)))
            .max()
            .unwrap_or(0)
    }

    /// Кто ведёт отряд узла: кот с лучшей ступенью, ничья — по `id` (§11).
    fn node_guide(&mut self, x: i32, y: i32) -> String {
        let step = self.node_guide_step(x, y);
        if step <= 0 {
            return String::new();
        }
        let roster = self.roster_of(x, y);
        let crew = self.ready_crew(&roster);
        let stats = self.world.resource::<StatRules>();
        // Ничью по ступени решает **сырая реакция**, и только потом `id`
        // (§11, §12.70): ступень грубая, у реакции 5 и 7 она одна, и назвать
        // ведущим того, у кого реакция ниже, — заведомо неверное объяснение
        // при верной механике.
        crew.iter()
            .filter(|&&(e, _)| guide_of(stats, self.world.get::<Stats>(e)) == step)
            .filter_map(|&(e, _)| {
                let raw = guide_value(stats, self.world.get::<Stats>(e));
                self.world.get::<UnitId>(e).map(|u| (-raw, u.0.clone()))
            })
            .min()
            .map(|(_, id)| id)
            .unwrap_or_default()
    }

    /// Кто из отряда узла уйдёт прямо сейчас, по `id` (§12.70) — для панели.
    /// Порядок тот же, что у состава: игрок читает оба списка рядом.
    fn ready_roster_of(&mut self, x: i32, y: i32) -> Vec<String> {
        let roster = self.roster_of(x, y);
        let ready = self.ready_crew(&roster);
        ready
            .into_iter()
            .filter_map(|(e, _)| self.world.get::<UnitId>(e).map(|u| u.0.clone()))
            .collect()
    }

    fn launch_at(&mut self, def: usize, units: Vec<String>, node: Option<(i32, i32)>) -> bool {
        let Some(rule) = self.world.resource::<MissionRules>().0.get(def).cloned() else {
            return false;
        };
        // Больше предела бригада не уводит: вилку задаёт заказ, а не приписка
        // (§12.70). Отказ здесь молчаливый и лечится вычёркиванием кота с узла —
        // подрезать список самим значило бы решать за игрока, кто останется.
        if units.len() > rule.squad_max {
            return false;
        }
        if self.raids_running() >= self.gate_nodes() || self.mission_of(def).is_some() {
            return false;
        }
        // Известность — ворота: за дело, о котором ещё не слышали, не берутся,
        // сколько бы сильным ни был отряд (§12.24).
        if self.world.resource::<Fame>().0 < rule.requires {
            return false;
        }
        // Вторые ворота: заказчик должен с базой разговаривать (§12.43).
        // Известность решает, дорос ли ты вообще; репутация — станут ли с тобой
        // говорить именно эти. Отказ молчаливый, как и на нехватку известности:
        // причину игрок читает в панели, где её называет словом `RaidSnap`.
        if !self.world.resource::<Standing>().covers(&rule.needs) {
            return false;
        }
        // За своим идут, только пока есть за кем (§12.40). Вылазка с `rescue`
        // без пленного — это вылазка без цели: добычи у неё нет, а вернуть ей
        // некого. Пленных в отряд при этом не берут — но отдельной проверки на
        // это нет и не нужно: пленный `Away`, а ушедших список уже отсекает.
        if rule.rescue && !self.has_captive() {
            return false;
        }

        // **Идут все, кто готов; неготовые остаются дома** (§12.70). До §12.70
        // список требовалось иметь ровно по составу, и любой неготовый отменял
        // вылазку целиком — то есть один невыносливый кот держал свою бригаду на
        // приколе всё время своего сна. Ожидание при этом не ротация, а простой:
        // подменить его в бригаде некем.
        //
        // Не готов — это три случая, и все они видны игроку в строке отряда:
        //   * `Away` — кот в поле или в плену, его вообще нет на базе;
        //   * ниже `hurt` — выбывший, и это вся цена провала (§12.37); отправить
        //     его добирать урон значило бы её отменить;
        //   * спит, **пока включено «Беречь себя»** — будить его заявка не имеет
        //     права (§12.51), а ждать его теперь незачем. С выключенным
        //     «Беречь себя» игрок сказал котов не жалеть: спящего поднимают и
        //     уводят, как и до §12.70, — простоя от этого не возникает, он идёт
        //     к шлюзу сразу.
        //
        // Дубликаты в списке отсекаются по сущности: «три раза excellent» — это
        // один кот, а не отряд.
        let crew = self.ready_crew(&units);
        // **Недокомплект — решение игрока, а не отказ** (§12.113). Минимум вилки
        // перестал быть допуском: цена меньшего состава уже посчитана дважды —
        // линейной силой в `outcome` и делением срока на лапы в `duration`, — а
        // третьим её считать значило бы завести пятую арифметику исхода
        // (инвариант 14) поверх уже посчитанного.
        //
        // Исключение одно: **вылазка за своим**. Её минимумом меряется
        // обратимость плена (`rescue_is_possible`), и отпустить туда кого
        // угодно значило бы обещать возврат, не проверив его.
        let need = if rule.rescue { rule.squad } else { 1 };
        if crew.len() < need {
            return false;
        }

        // Гараж, который держит этот слот, — он же дверь и дом отряда
        // (§12.152). Свободный берётся в порядке обхода карты, то есть
        // детерминированно (§11); это путь для теста и для `launch`, а игрок
        // называет клетку сам.
        //
        // До §12.152 здесь звался `pick_gate`: слот брался у рации, а шлюз
        // подбирался ближайший ко всем сразу — то есть дверь выбирал алгоритм.
        // Проверки «до шлюза доберутся все» вместе с ним не стало намеренно:
        // отряд приписан к своему гаражу заранее, и подменять его другим на
        // основании того, что кто-то заперт, значит отменять решение игрока
        // молча. Не дошедший до двери держит сбор — это видно и чинится.
        let gate = match node {
            Some(spot) => spot,
            None => match self.free_gate_cell() {
                Some(spot) => spot,
                None => return false,
            },
        };
        let mission_e = self.world.spawn(Mission {
            def,
            gate,
            // Срок пока неизвестен: он зависит от того, сколько лап дойдёт до
            // шлюза, и замерзает в момент ухода (§12.70).
            left: 0,
            span: 0,
            covered: 0,
        });
        let mission_e = mission_e.id();
        // Спящих в `crew` уже нет — их отсеял отбор состава (§12.70), и будить
        // тут некого. Заснуть по дороге к шлюзу кот всё ещё может: тогда его
        // ждёт `gather_squad`, который спящего не трогает, потому что истощение
        // из отряда не выводит (§12.23, §12.51).
        for (cat_e, from) in crew {
            self.release_task(cat_e);
            // Ношу кот кладёт под ноги — прямо здесь и сейчас (§12.38). Уехать
            // с ней в поле значит вынуть лом из мира на сотни тиков: сумма
            // сходится, но кучу не видно, её не взять на стройку и не
            // разметить (§12.16) — тот же случай, что вещь, оставшаяся в
            // пустоте (§12.15). Роняем **до** маршрута: к шлюзу кот идёт налегке.
            //
            // Это единственное исключение из «ношу посреди базы не бросают», и
            // граница у него ровная: приказ игрока груз не роняет (кот вернётся
            // к работе и донесёт), а заявка на вылазку убирает кота с базы
            // надолго. Куча ложится там, где он вёз, то есть по дороге к
            // площадке, ради которой лом и подняли, — оттуда её подхватит
            // следующий носильщик.
            if let Some(load) = self.world.get::<Carrying>(cat_e) {
                let (item, count) = (load.item, load.count);
                self.drop_stack(from.0, from.1, item, count);
                self.world.entity_mut(cat_e).remove::<Carrying>();
            }
            self.world.entity_mut(cat_e).insert(Squad(mission_e));
            {
                let steps = find_path(
                    self.world.resource::<BaseMap>(),
                    self.world.resource::<TileRules>(),
                    from,
                    gate,
                )
                .unwrap_or_default();
                self.world.entity_mut(cat_e).insert(Path { steps });
            }
        }
        true
    }

    /// Распустить отряд и снять миссию.
    ///
    /// Работает, только пока отряд на базе: ушедших не отзывают — что там
    /// происходит, симуляция не знает, вылазка считается разом по возвращении.
    /// Адресуется **по заказу**, а не по номеру в списке: вылазок теперь идёт
    /// столько, сколько узлов связи (§12.59), а порядок обхода сущностей ECS
    /// недетерминирован — по номеру отзывался бы то один отряд, то другой. Двух
    /// вылазок по одному заказу не бывает, заказ их и различает (§12.55).
    ///
    /// Отзыв **усыпляет правило автовылазки** этого узла (§12.77): без этого
    /// «Отозвать» у автоматического отряда не значит ничего — правило заводит
    /// заявку заново тем же тиком, и кнопка читается как сломанная. Стирать
    /// правило тоже нельзя: отзывают ради разового дела на базе, а круг после
    /// него тот же самый.
    ///
    /// Вернёт false, если такой вылазки нет или отряд уже ушёл.
    pub fn cancel_mission(&mut self, def: usize) -> bool {
        note(&mut self.world, format!("cancel_mission {def}"));
        let Some(mission_e) = self.mission_of(def) else {
            return false;
        };
        if self.crew_of(mission_e).iter().any(|&(_, away)| away) {
            return false;
        }
        let node = self.world.get::<Mission>(mission_e).map(|m| m.gate);
        if let Some((x, y)) = node {
            self.world.resource_mut::<AutoRaids>().set_on(x, y, false);
        }
        self.disband(mission_e);
        true
    }

    /// Нанять кандидата `def` (индекс в палитре из `map_meta`).
    ///
    /// **Известность открывает, платят предметами** (§12.24). Одна шкала и
    /// ворота, и валюта была бы ловушкой: наняв кота, игрок обнаружил бы, что
    /// у него закрылась вылазка, — и прочитал бы это как поломку.
    ///
    /// Платит **склад**: то, что валяется на полу, ещё не сосчитано, а склад —
    /// учтённое имущество базы. Это третий смысл клетки с `capacity` после
    /// ёмкости и места назначения, и первая причина убираться не из аккуратности.
    ///
    /// Новичок появляется у шлюза: гараж — точка контакта базы с миром (§12.22).
    /// Вернёт false, если кандидата нет, известности не хватает, он уже нанят,
    /// на складе нет цены или на базе нет шлюза.
    pub fn hire(&mut self, def: usize) -> bool {
        note(&mut self.world, format!("hire {def}"));
        let Some(rule) = self.world.resource::<RecruitRules>().0.get(def).cloned() else {
            return false;
        };
        if self.world.resource::<Fame>().0 < rule.requires {
            return false;
        }
        // Вторые ворота: кандидата присылают те, кто базе доверяет (§12.43).
        // Репутация при этом **не тратится** — она открывает, а платит склад:
        // найм это покупка, а не поступок, и списывать за него доверие значило
        // бы сделать репутацию валютой, то есть ловушку §12.24.
        if !self.world.resource::<Standing>().covers(&rule.needs) {
            return false;
        }
        // Нанят ли — спрашиваем у самого кота: список нанятых был бы вторым
        // источником правды, который однажды разойдётся с миром.
        let mut units = self.world.query::<&UnitId>();
        if units.iter(&self.world).any(|u| u.0 == rule.id) {
            return false;
        }

        // Первый гараж по обходу карты — детерминированно (§11), и этого
        // достаточно: новичок просто приходит, отряда за ним нет.
        let Some(gate) = self.gate_cells_of().into_iter().next() else {
            return false; // шлюза нет — новичку неоткуда взяться
        };
        if !self.spend_from_storage(&rule.cost) {
            return false;
        }
        spawn_cat(
            &mut self.world,
            &rule.id,
            &rule.sprite,
            gate,
            &rule.perks,
            &rule.skills,
            &rule.stats,
        );
        true
    }

    /// Взяться за тему `def` (индекс в палитре из `map_meta`).
    ///
    /// **Тема — разметка работы, а исполнителя берёт симуляция** (§12.16): игрок
    /// решает, что изучать, а сядет за это ближайший кот, которому хватает
    /// «Науки». Уровень — допуск (§12.18), а не скорость: без него не медленнее,
    /// а никак.
    ///
    /// Платят **образцами со склада**, разом при заявке, — как за найм (§12.24):
    /// работа котов начнётся потом, а решение принято сейчас.
    ///
    /// **У темы-вскрытия плата другая** (§12.133): вместо `cost` у неё
    /// `specimen`, и это не плата, а материал — коты везут его в лабораторию
    /// ногами, как на станок (§12.102). Ворот у него **двое** (§12.139): шкала
    /// `Seen` («предмет хоть раз побывал на базе», §12.131) открывает саму тему,
    /// а склад — заявку. Списания при этом нет: образец приезжает подвозом.
    ///
    /// Одного `Seen`, как было до §12.139, мало: база, чьи коты разобрали по
    /// себе все привезённые комбинезоны, заводила тему, которая **не ждёт
    /// образец, а стоит намертво**, — и держала при этом ячейку лаборатории и
    /// учёного. «Ждёт материал» законно у заказа (§12.30), потому что материал
    /// на базе есть; надетое же со склада не вернётся никогда.
    ///
    /// Тема одна за раз: на POC второй некого посадить, а очередь тем — механика,
    /// которую не на чем проверить. Вернёт false, если темы нет, она уже изучена
    /// или идёт, не хватает предыдущих технологий, образец ещё не попадался
    /// базе, на базе нет лаборатории, некому взяться или на складе нечем
    /// заплатить.
    pub fn start_research(&mut self, def: usize) -> bool {
        note(&mut self.world, format!("research {def}"));
        let Some(rule) = self.world.resource::<ResearchRules>().0.get(def).cloned() else {
            return false;
        };
        if self.topic_of(def).is_some() {
            return false;
        }
        let techs = self.world.resource::<Techs>();
        // Уже изученное не изучают снова, а без предыдущих технологий темы
        // просто не существует — это дерево из §4.3.
        if techs.knows(&rule.id) || !techs.covers(&rule.requires) {
            return false;
        }
        // Образец база должна была **повидать** (§12.131, §12.133) — это ворота
        // самой темы: невиданное не вскрывают, и в реестре такая тема стоит
        // витриной (§12.137).
        {
            let seen = self.world.resource::<Seen>();
            if !rule.specimen.iter().all(|&(item, _)| seen.saw(item)) {
                return false;
            }
        }
        // …а лежать он обязан **на складе прямо сейчас** (§12.139): везут его
        // ногами и берут со складской кучи (§12.130), поэтому тема, заведённая
        // над надетыми по котам комбинезонами, не ждёт образец, а стоит
        // намертво — держа ячейку лаборатории и учёного. Спрашиваем тем же
        // выражением, что и плату (`storage_covers`): списания тут нет,
        // материал приедет подвозом, но обещать работу без материала нельзя.
        if !self.storage_covers(&rule.specimen) {
            return false;
        }
        // Свободная ячейка лаборатории (§12.132) — не «есть ли лаборатория
        // вообще»: тем теперь идёт столько, сколько комнат, и вторая тема при
        // одной комнате отклоняется, а не встаёт в очередь.
        let Some(cell) = self.free_lab_cell() else {
            return false;
        };
        // Некому взяться — отказываем **до** оплаты: тема, за которую заплачено
        // образцами и которую никто не возьмёт, читается как потерянный ресурс,
        // а не как «подождите, пока кто-нибудь доучится».
        if !self.has_scientist(rule.level) {
            return false;
        }
        if !self.spend_from_storage(&rule.cost) {
            return false;
        }
        self.world.spawn(Research {
            def,
            progress: 0,
            assignee: None,
            cell,
            delivered: Vec::new(),
        });
        true
    }

    /// Бросить тему `def` и освободить исполнителя.
    ///
    /// Адресуется **темой, а не клеткой** (§12.132), в отличие от заказа
    /// (§12.96): двух тем на один `def` не бывает никогда, потому что тема
    /// одноразова и необратима (§12.18), — значит ключ однозначен, и заводить
    /// второй незачем.
    ///
    /// Плата (`cost`) при этом **не возвращается**: образцы уже разобрали на
    /// опыты — та же цена поспешной разметки, что и у отменённого чертежа.
    /// А вот **завезённый образец** (`specimen`, §12.133) роняется кучей на
    /// клетку: он ещё лежит в лаборатории целым, и материал не горит никогда
    /// (инвариант 8). Вернёт false, если такой темы не изучают.
    pub fn cancel_research(&mut self, def: usize) -> bool {
        note(&mut self.world, format!("cancel_research {def}"));
        let Some(topic_e) = self.topic_of(def) else {
            return false;
        };
        if let Some(cat_e) = self.world.get::<Research>(topic_e).and_then(|t| t.assignee) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Researching, Path, Stride)>();
        }
        // Завезённый образец возвращается кучей на клетку лаборатории (§12.133,
        // §12.31) — дословно как материал со снятого заказа. Плата (`cost`) при
        // этом не возвращается: её разобрали на опыты. Разница ровно та же, что
        // между платой и материалом у мастерской (§12.102).
        let spill = self
            .world
            .get::<Research>(topic_e)
            .map(|t| (t.cell, t.delivered.clone()))
            .unwrap_or_default();
        self.world.entity_mut(topic_e).despawn();
        for (item, count) in spill.1 {
            if count > 0 {
                self.drop_stack(spill.0.0, spill.0.1, item, count);
            }
        }
        true
    }

    /// Заказать `count` штук по рецепту `def` (индекс в палитре из `map_meta`).
    ///
    /// **Заказ — разметка работы, как чертёж** (§12.16): игрок решает, что и
    /// сколько сделать, а к верстаку встанет ближайший свободный кот. Допуска по
    /// навыку здесь нет — рецепт открывает технология, «Ремесло» лишь ускоряет
    /// (§12.30).
    ///
    /// **Вперёд не платят.** Материал списывается за штуку и в тот момент, когда
    /// за неё берутся: заказ на десять штук, оплаченный сразу, заморозил бы склад
    /// под работу, которая начнётся через полтысячи тиков. Поэтому и пустой склад
    /// заявку не отклоняет — заказ ждёт материала, как чертёж ждёт лом (§12.15).
    ///
    /// **Заказ живёт в ячейке станка** (§12.96) — дословно как сделка живёт в
    /// ячейке торгового поста (§12.68). Ячейку выбирает **ядро**, первую
    /// свободную по обходу карты, а не игрок мышью: §12.16 держится тем, что
    /// разметка не зависит ни от исполнителя, ни от того, какую комнату игрок
    /// нашёл курсором. Слот по-прежнему занимает сам заказ, а не кот за ним.
    ///
    /// **Отсюда и параллельность одного рецепта:** повторная заявка занимает
    /// следующий свободный станок, и пятнадцать деталей на трёх мастерских
    /// делаются втрое быстрее, а не по очереди. Свободных ячеек не осталось —
    /// штуки добавляются к первому по карте заказу того же рецепта, ровно как
    /// было до §12.96; на базе с одной мастерской поведение не изменилось вовсе.
    ///
    /// **Приказ игрока удаляет приказ автопроизводства** (§12.97): свободной
    /// ячейки нет — заявка забирает её у неоплаченного заказа правила
    /// (`spare_shop_cell`), и только если вытеснять некого, добирает свой. Заказ,
    /// которого коснулся игрок, становится ручным: правило его больше не ведёт,
    /// но в счёт порога он идёт — это тот же будущий запас.
    ///
    /// Вернёт false, если рецепта нет, счётчик неположителен, не хватает
    /// технологий или ячейки нет, вытеснять некого и добавить штуки некуда.
    pub fn start_craft(&mut self, def: usize, count: i32) -> bool {
        note(&mut self.world, format!("craft {def} {count}"));
        let Some(rule) = self.world.resource::<CraftRules>().0.get(def).cloned() else {
            return false;
        };
        if count <= 0 {
            return false;
        }
        if !self.world.resource::<Techs>().covers(&rule.requires) {
            return false;
        }
        // **Заказать можно только то, на что есть материал** (§12.129, до него
        // §12.114d — про один разбор). Заявка режется наличным: сколько склад
        // потянет, столько и закажут.
        //
        // До §12.129 обычный заказ ждал материала законно (§12.30): лом придёт
        // сам, с уборки и с вылазок. Но ждущий заказ **держит станок** (§12.96)
        // и **расписывает на себя будущий лом** (§12.128) — то есть отнимает
        // ровно то, чем игрок расплатился бы за следующий заказ, и делает это
        // молча. Правило-порог этим воротам не подчиняется и подчиняться не
        // должно: оно как раз и говорит «дозакажи, когда появится».
        let count = count.min(self.craft_room(&rule.cost));
        if count <= 0 {
            return false;
        }
        // Ячейка есть — заказ встаёт в неё, и это главное, ради чего §12.96 и
        // писалась: второй заказ на тот же рецепт занимает второй станок, а не
        // ждёт первого. Свободной нет — забираем у неоплаченного заказа правила
        // (§12.97): вытеснение стоит **перед** добором, чтобы три клика по трём
        // станкам дали три заказа, а не один разбухший.
        let Some((cell, evicted)) = self.spare_shop_cell() else {
            // Ячеек не осталось и вытеснять некого. Добавляем штук к своему
            // первому по карте заказу — так работала любая повторная заявка до
            // §12.96, и на базе с одной мастерской это по-прежнему обычная
            // ветка. Заказ правила, доживший сюда, оплачен: переворачиваем его
            // в ручной, а не отбираем — начатая штука доделается, а вести её
            // дальше будет игрок.
            let Some(order_e) = self.orders_of(def).into_iter().next() else {
                return false; // и добавить некуда: все станки заняты чужим
            };
            let Some(mut order) = self.world.get_mut::<Craft>(order_e) else {
                return false;
            };
            order.left += count;
            order.auto = false;
            return true;
        };
        if let Some(order_e) = evicted {
            // Кота с вытесненного заказа отпускаем сами: заказа сейчас не
            // станет, а `Crafting` на коте пережил бы его на тик.
            if let Some(cat_e) = self.world.get::<Craft>(order_e).and_then(|c| c.assignee) {
                self.world
                    .entity_mut(cat_e)
                    .remove::<(Crafting, Path, Stride)>();
            }
            self.world.entity_mut(order_e).despawn();
        }
        self.world.spawn(Craft {
            def,
            left: count,
            progress: 0,
            delivered: Vec::new(),
            assignee: None,
            cell,
            auto: false,
        });
        true
    }

    /// Сколько штук база потянет прямо сейчас (§12.114d, §12.129): **учтённое** —
    /// склад минус бронь продажи, — минус обещанное открытым заказам.
    ///
    /// Считается по складу, потому что станок так и снабжается (§12.130):
    /// материал к нему возят ногами и берут **только со складских куч**, как к
    /// торговому посту. Валяющееся не пропадает — его увезёт уборка, и число
    /// вырастет само.
    ///
    /// Это то же число, которое игрок видит главным в строке окна (§12.53), и
    /// то же, каким платят найм, наука и аптечка: «чем можно платить» в игре
    /// один ответ, а не два.
    ///
    /// **Надетое и везомое в счёт не идут** (`Gear` и `Carrying` — не кучи), и
    /// это не побочность запроса: заказ на разбор, считающий надетое, раздевал
    /// бы отряд перед вылазкой. Стережёт `a_worn_suit_is_not_salvage_material`.
    ///
    /// Обещанное вычитается по **всем** заказам, а не только по разборам: тот,
    /// что стоит и ждёт лом, съест его, когда лом появится, — и два разбора по
    /// одному комбинезону иначе прошли бы оба.
    fn craft_room(&mut self, cost: &[(usize, i32)]) -> i32 {
        let rules = CraftRules(self.world.resource::<CraftRules>().0.clone());
        let mut orders = self.world.query::<&Craft>();
        let promised: Vec<(usize, i32)> =
            orders.iter(&self.world).fold(Vec::new(), |mut acc, order| {
                for (item, need) in craft_missing(&rules, order) {
                    match acc.iter_mut().find(|(i, _)| *i == item) {
                        Some((_, n)) => *n += need,
                        None => acc.push((item, need)),
                    }
                }
                acc
            });
        let booked = self.booked_for_sale();
        let at = |set: &[(usize, i32)], item: usize| {
            set.iter().find(|&&(i, _)| i == item).map_or(0, |&(_, n)| n)
        };
        cost.iter()
            .map(|&(item, per)| {
                // Тот же счёт, каким подвоз решает, что может привезти на
                // станок (§12.102, §12.130): складские кучи, минус чужие
                // обещания.
                let free = self.in_storage(item) - at(&booked, item) - at(&promised, item);
                if per > 0 { free / per } else { i32::MAX }
            })
            .min()
            .unwrap_or(0)
            .max(0)
    }

    /// Держать на базе не меньше `min` штук того, что даёт рецепт `def`
    /// (§12.65). Ноль снимает порог.
    ///
    /// **Это правило, а не заказ** (§12.64): команда только пишет число, а мир
    /// приводит в соответствие `plan_craft` — каждым тиком, а не один раз.
    /// Отсюда и то, что снятие порога ничего здесь не отменяет: заказ, который
    /// правило вело, оно же и уберёт, доделав оплаченную штуку.
    ///
    /// Порог живёт на **рецепте**, а не на предмете: два рецепта могут давать
    /// одно и то же, и выбор между ними — решение игрока, а не «первый по
    /// палитре».
    ///
    /// Вернёт false, если такого рецепта нет или число отрицательное. Пустой
    /// склад, занятый станок и неоткрытая технология отказом **не** являются:
    /// правило ждёт, как ждёт заказ без материала (§12.30).
    pub fn set_stock(&mut self, def: usize, min: i32) -> bool {
        note(&mut self.world, format!("stock {def} {min}"));
        if min < 0 || self.world.resource::<CraftRules>().0.len() <= def {
            return false;
        }
        // Порога у разбора нет (§12.114). Правило «держать ткани до N» рвало бы
        // надетые комбинезоны без спроса, а решение необратимо, — поэтому вид
        // такой полоски не рисует, а фасад отказывает: команда приезжает и из
        // старого трейса, где вид был другим.
        if self.world.resource::<CraftRules>().0[def].salvage {
            return false;
        }
        // Ворота автоматики (§12.93). Проверяются только при **постановке**:
        // снятие обязано проходить всегда, иначе правило, поставленное до
        // изучения, стало бы несбрасываемым.
        if min > 0
            && !self
                .world
                .resource::<AutoRules>()
                .crafting_open(self.world.resource::<Techs>())
        {
            return false;
        }
        self.world.resource_mut::<Stocking>().set(def, min);
        true
    }

    /// Велеть узлу `(x, y)` ходить на заказ `def` самому — или снять правило
    /// (`def < 0`), §12.67.
    ///
    /// **Это правило, а не заявка**, ровно как порог у производства (§12.64):
    /// команда пишет заказ на клетку, а в поле отряд отправляет `run_auto_raids`
    /// — каждым тиком, пока правило стоит. Отсюда и отсутствие пары «отменить»:
    /// снятое правило новых вылазок не заводит, но **идущую не отзывает** —
    /// отряд уже в поле, и отзыва оттуда не бывает вовсе (§12.22).
    ///
    /// Правило висит на **гараже**, а не на отряде: состав и так приписан к
    /// клетке (§12.61), а слот вылазки — это сам гараж (§12.152). Один гараж —
    /// один заказ: очередь заказов это уже план, который база выполняет за
    /// игрока, а не повторение его решения (§12.64).
    ///
    /// Вернёт false, если в клетке не гараж или такого заказа нет. Отряд не
    /// в сборе, закрытые ворота и раненый боец отказом **не** являются: правило
    /// ждёт готовности, как порог ждёт материала (§12.30).
    pub fn set_auto_raid(&mut self, def: i32, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("auto_raid {def} {x} {y}"));
        if !self.is_gate_at(x, y) {
            return false;
        }
        // Ворота автоматики (§12.93); снятие (`def < 0`) проходит всегда.
        if def >= 0
            && !self
                .world
                .resource::<AutoRules>()
                .raids_open(self.world.resource::<Techs>())
        {
            return false;
        }
        if def < 0 {
            self.world.resource_mut::<AutoRaids>().clear(x, y);
            return true;
        }
        let def = def as usize;
        if self.world.resource::<MissionRules>().0.len() <= def {
            return false;
        }
        self.world.resource_mut::<AutoRaids>().set(x, y, def);
        true
    }

    /// Приостановить правило узла `(x, y)` или вернуть его в строй (§12.77).
    ///
    /// **Пауза — не снятие**, и в этом весь смысл: отряд привёз гору лома, её
    /// надо разгрести, а через сотню тиков вернуться к тому же кругу. Снятое
    /// правило пришлось бы ставить заново — то есть искать заказ среди
    /// карточек и принимать решение, которого игрок не менял. Усыплённое помнит
    /// заказ и будится тем же тумблером.
    ///
    /// Усыпляет правило не только игрок: то же делают явный приказ на **другой**
    /// заказ (`launch_node`) и отзыв собравшегося отряда (`cancel_mission`).
    /// Оба — решение взамен ближайшего круга, а не отказ от рутины.
    ///
    /// Идущую вылазку пауза **не отзывает**, ровно как и снятие: отряд уже в
    /// поле, а оттуда не отзывают вовсе (§12.22).
    ///
    /// Вернёт false, если в клетке не гараж или правила на нём нет: будить
    /// нечего, а заводить правило умеет только `set_auto_raid`.
    pub fn set_auto_raid_on(&mut self, x: i32, y: i32, on: bool) -> bool {
        note(&mut self.world, format!("auto_raid_on {x} {y} {on}"));
        if !self.is_gate_at(x, y) {
            return false;
        }
        self.world.resource_mut::<AutoRaids>().set_on(x, y, on)
    }

    /// Правило автовылазки: повторить за игроком клик по кнопке заказа (§12.67).
    ///
    /// **Стоит в фасаде, а не в цепочке систем** — и это единственное правило,
    /// которое там стоит. Причина не в удобстве: заявка на вылазку это не
    /// изменение данных, а оркестровка на `&mut World` (снять задачи, уронить
    /// ношу, проложить маршруты, занять именной слот), и живёт она в
    /// `launch_at`. Система, повторившая бы её, стала бы **вторым** способом
    /// уйти в поле — то есть ровно тем, чего §12.64 велит не заводить. Поэтому
    /// правило зовёт ту же кнопку, что и игрок, и стоит там же, где стоял бы он:
    /// перед тиком.
    ///
    /// **Отряд уходит, только когда он готов целиком**: все на базе, никто не
    /// ранен, никто не спит и ни у кого бодрость не ниже порога усталости.
    /// `launch_node` принял бы и спящего (§12.51: отряд его подождёт), но
    /// правило повторяется каждый тик — и заявка, поданная над спящим, заняла бы
    /// узел на всё время его сна. Игрок в этом месте подождал бы сам, поэтому
    /// ждёт и правило.
    ///
    /// Ворот здесь нет ни одной: известность, репутацию, занятый слот и уже
    /// идущий заказ проверяет `launch_at` — второй экземпляр этих проверок
    /// однажды разошёлся бы с кнопкой (§12.24).
    ///
    /// Правило на снесённом гараже **убирает за собой само**: двери нет — нет и
    /// строки. Это та же оговорка, из-за которой §12.65 пришлось убирать заказ
    /// снятого порога: у быстрого выхода обязана быть уборка.
    fn run_auto_raids(&mut self) {
        let rules = self.world.resource::<AutoRaids>().0.clone();
        if rules.is_empty() {
            return;
        }
        for (x, y, def, on) in rules {
            if !self.is_gate_at(x, y) {
                self.world.resource_mut::<AutoRaids>().clear(x, y);
                continue;
            }
            // Усыплённое правило помнит заказ, но заявок не заводит (§12.77).
            if !on {
                continue;
            }
            if !self.node_is_free(x, y) || !self.squad_is_fit(x, y) {
                continue;
            }
            // Правило берётся за заказ, **только когда прогноз обещает полную
            // долю** (§12.117). До §12.117 здесь стоял минимум вилки (§12.113),
            // и он мерил не то: справится ли отряд, считает `outcome`, а не
            // число лап. Довод §12.113 никуда не делся — недобор это разовое
            // осознанное решение игрока, — но выражен он теперь тем же числом,
            // которое игрок видит в штабе до нажатия.
            if !self.auto_outcome_full(x, y, def) {
                continue;
            }
            self.launch_node(def, x, y);
        }
    }

    /// Прогноз исхода заказа для готовых лап узла: `(доля, провал)` (§12.117).
    ///
    /// Та же пара, что уже едет в штаб (`NodeSnap::shares`/`fails`), и тот же
    /// `outcome`, каким исход посчитается на возвращении (инвариант 14).
    pub(crate) fn auto_forecast(&mut self, x: i32, y: i32, def: usize) -> (i32, bool) {
        let (shares, fails) = self.node_outcomes(x, y);
        (
            shares.get(def).copied().unwrap_or(0),
            fails.get(def).copied().unwrap_or(true),
        )
    }

    /// Возьмётся ли правило за этот заказ прямо сейчас: **только полная доля**
    /// (§12.117).
    ///
    /// До §12.117 воротами был минимум вилки (§12.113), и он мерил не то. Число
    /// в вилке — про срок и подачу заказа, а «справится ли» считает `outcome`,
    /// поэтому проверка состава промахивалась в обе стороны: прокачанного
    /// одиночку с прогнозом «100 %, не провал» не пускала, а троих новичков на
    /// логово рейдеров пускала — в провал, каждый круг, с потерей снаряжения и
    /// пленом. То есть ворота не защищали ровно от того случая, ради которого
    /// стояли.
    ///
    /// Порог — **полная доля**, а не «не провал»: правило повторяет решение без
    /// присмотра, и частичная добыча тут не риск, на который игрок согласился, а
    /// тихая утечка каждый круг. Согласиться на неё по-прежнему можно — руками
    /// из штаба, где доля написана до нажатия (§12.71).
    ///
    /// **Одно выражение на правило и на снимок** (§12.116): игрок видит причину
    /// простоя ровно тогда, когда правило и правда стоит из-за неё.
    pub(crate) fn auto_outcome_full(&mut self, x: i32, y: i32, def: usize) -> bool {
        let (share, failed) = self.auto_forecast(x, y, def);
        !failed && share >= 100
    }

    /// Готов ли отряд узла уйти прямо сейчас: **все** на базе, целы и не спят.
    ///
    /// Состав здесь **не считается** — его длину сверит `launch_node`: «сколько
    /// нужно» знает заказ, а правило про заказ ничего не решает.
    ///
    /// «Все» — это половина запрета некомплекта у автовылазки (§12.70, §12.113);
    /// вторая половина — минимум вилки, который `run_auto_raids` спрашивает сам,
    /// потому что `launch_node` спрашивать перестал. Игрок,
    /// нажимая кнопку, соглашается на просевшую долю осознанно и разово: он
    /// видит цену в строке отряда до нажатия. Автомат принимал бы это решение за
    /// него на каждом круге и гонял бы полупустые отряды — то есть делал бы
    /// выбор, которого у него не просили. Ровно поэтому правило ждёт полного
    /// состава, хотя кнопка ждать перестала.
    pub(crate) fn squad_is_fit(&mut self, x: i32, y: i32) -> bool {
        let hurt = self.world.resource::<HealthRules>().hurt;
        let tired = self.world.resource::<NeedRules>().tired;
        let mut q = self.world.query::<(
            &Enlisted,
            Option<&Away>,
            Option<&Health>,
            Option<&Rest>,
            Option<&Energy>,
        )>();
        let mut crew = 0;
        for (spot, away, health, rest, energy) in q.iter(&self.world) {
            if spot.spot != (x, y) {
                continue;
            }
            crew += 1;
            let fit = away.is_none()
                && health.is_none_or(|h: &Health| h.0 > hurt)
                && rest.is_none()
                // Порога усталости может не быть вовсе (`NeedRules` пуст в
                // синтетических схемах) — тогда бодрость никого не держит.
                && energy.is_none_or(|e: &Energy| e.0 > tired);
            if !fit {
                return false;
            }
        }
        crew > 0
    }

    /// Отменить заказ на рецепт `def`, освободить мастера и станок.
    ///
    /// Отменяют **по клетке** (§12.96). Довод тот же, по которому до §12.96
    /// отменяли по рецепту: порядок обхода ECS недетерминирован, а номер строки
    /// уедет под курсором, как только закроется соседний заказ. Но рецепт
    /// заказы больше не различает — их на него бывает несколько, — а ячейка
    /// различает полностью: двух заказов в одной не бывает.
    ///
    /// **Кнопка живёт в панели этой самой клетки** (§12.95): решение про заказ
    /// на конкретном станке принимают там, где написано, что за заказ и докуда
    /// он дошёл. §12.80 это не нарушает — он запрещает панели клетки решения,
    /// **чью цену она не может показать**, а тут цена написана строкой выше.
    ///
    /// Материал уже начатой штуки **не возвращается** — та же цена поспешной
    /// разметки, что у брошенной темы и у отменённого чертежа с завезённым ломом
    /// (§12.26). Неоплаченные штуки не стоили ничего и просто исчезают.
    ///
    /// **Заказ, который ведёт правило, этой командой не отменяется** (§12.65):
    /// правило завело бы его обратно тем же тиком, и отмена оказалась бы
    /// командой, которая ничего не делает. Отменяют такой заказ снятием порога
    /// (`set_stock(def, 0)`) — источник у разметки один, и отменять надо его.
    ///
    /// Вернёт false, если в этой клетке заказа нет или он не ручной.
    pub fn cancel_craft(&mut self, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("cancel_craft {x} {y}"));
        let Some(order_e) = self.order_at(x, y) else {
            return false;
        };
        if self.world.get::<Craft>(order_e).is_some_and(|o| o.auto) {
            return false;
        }
        if let Some(cat_e) = self.world.get::<Craft>(order_e).and_then(|o| o.assignee) {
            self.world
                .entity_mut(cat_e)
                .remove::<(Crafting, Path, Stride)>();
        }
        // Привезённое на станок возвращается кучей на его клетку (§12.102,
        // §12.31): материал не исчезает никогда, а дальше его увозит обычная
        // уборка. До §12.102 возвращать было нечего — материал списывался со
        // склада, и отмена его молча сжигала.
        let spill = self
            .world
            .get::<Craft>(order_e)
            .map(|o| (o.cell, o.delivered.clone()))
            .unwrap_or_default();
        self.world.entity_mut(order_e).despawn();
        for (item, count) in spill.1 {
            if count > 0 {
                self.drop_stack(spill.0.0, spill.0.1, item, count);
            }
        }
        true
    }

    /// Заключить сделку с фракцией: купить или продать `count` штук предмета
    /// `item` (§12.44).
    ///
    /// **Курс фиксируется здесь и сейчас** и дальше живёт в самой сделке. Цена
    /// обязана быть видна до клика, а показать в панели одно и списать другое
    /// запрещает та же дисциплина, что и у прогноза вылазки (§12.23): курс
    /// считает `trade::quote`, и это единственное место, где он считается.
    ///
    /// **Покупка платится сразу и целиком**, а товар едет `lead` тиков — за это
    /// время расписание успеет уйти, и в том и смысл: решение принимается
    /// вперёд. **Продажа не платится вовсе**, пока коты не набьют контейнер:
    /// деньги приходят разом по отгрузке, и считает их `run_trade` (§12.68).
    ///
    /// Сделок идёт столько, сколько ячеек у постов (§12.68), и **отменить их
    /// нельзя** — команды для этого нет намеренно: иначе это бесплатный опцион.
    ///
    /// Отсюда же вторые ворота: **продать можно только то, что на складе есть**
    /// (§12.50). Слот торговли один и не отменяется, поэтому заявка на пять
    /// ломов при трёх занимала бы его навсегда — коты донесли бы три, сделка не
    /// закрылась бы никогда, и торговля кончилась бы на этом.
    ///
    /// Считается **склад, а не всё добро базы** (§12.69): учтённым база платит
    /// и торгует, неучтённое (пол, лапы, ячейки постов) годится только на
    /// стройку внутри базы. До §12.69 продажа брала откуда угодно — и у игрока
    /// появлялся третий смысл слова «есть», не выводимый из двух чисел в шапке.
    /// **Из складского вычитается забронированное** соседними сделками: с
    /// §12.68 постов много, и ворота, считающие каждую заявку в одиночку,
    /// пропускали бы продажу одного и того же лома с двух постов сразу.
    ///
    /// Вернёт false, если счётчик неположителен, свободной ячейки поста нет,
    /// фракция этим не торгует, на покупку не хватает денег или на продажу не
    /// хватает товара.
    pub fn trade(&mut self, faction: usize, item: usize, count: i32, buying: bool) -> bool {
        note(
            &mut self.world,
            format!("trade {faction} {item} {count} {buying}"),
        );
        if count <= 0 {
            return false;
        }
        // Сделок столько, сколько постов (§12.55), и с §12.68 счёт этот стал
        // местом: сделка занимает **ячейку** — от клика до вывоза. Отмены у
        // сделки по-прежнему нет (§12.44), но теперь занятый пост объясняет
        // себя сам и разгребается котами, а не только таймером.
        let Some(cell) = self.free_post_cell() else {
            return false;
        };
        // **В контейнер влезает столько, сколько влезает** (§12.90). Без этого
        // пост ограничивал бы число сделок, но не объём: одна заявка на пятьсот
        // штук проезжала бы через единственную ячейку, и второй пост терял
        // смысл — счётность §12.55 держалась бы только на терпении игрока,
        // который кликает по пять. Предел берётся у **той клетки**, которую
        // сделка займёт: посты бывают разные.
        let lot = self.lot_at(cell);
        if lot > 0 && count > lot {
            return false;
        }
        let Some(unit) = crate::trade::quote(
            self.world.resource::<FactionRules>(),
            self.world.resource::<Standing>(),
            faction,
            item,
            self.world.resource::<SimTime>().tick,
            buying,
        ) else {
            return false; // фракция этим предметом не торгует
        };
        // **Ворота на товар закрывают только покупку** (§12.150). Ассортимент —
        // это что продавец предлагает: дорос ли ты до этой вещи (известность) и
        // станут ли с тобой о ней говорить (репутация). Сдать ему лом можно
        // всегда — репутация умеет падать (§12.43), и ворота на продажу однажды
        // сломали бы уже поставленное правило автопродажи.
        if buying {
            let gates = crate::trade::offer_gates(
                self.world.resource::<FactionRules>(),
                self.world.resource::<Fame>(),
                self.world.resource::<Standing>(),
                faction,
                item,
            );
            match gates {
                Some(g) if g.unlocked && g.welcome => {}
                _ => return false,
            }
        }
        let lead = self
            .world
            .resource::<FactionRules>()
            .0
            .get(faction)
            .map_or(0, |f| f.lead);
        if buying {
            let total = unit * count;
            if self.world.resource::<Money>().0 < total {
                return false;
            }
            self.world.resource_mut::<Money>().0 -= total;
        } else {
            // **Продаётся учтённое — то же, чем платят** (§12.69): склад минус
            // бронь. Пол, лапы и ячейки постов сюда не входят: они годятся на
            // стройку внутри базы, но наружу база распоряжается только тем, что
            // убрано. Одно выражение на плату и на торговлю — и третьего смысла
            // слова «есть» у игрока больше нет.
            //
            // Форма брони здесь ровно одна и та же, что у платы (`booked`, со
            // скидкой на лапы): товар уезжает **со склада**, значит кот, взявший
            // его, склад уже уменьшил. Развилки `owed`/`booked`, на которой
            // ядро дважды ошиблось, для продажи больше не существует (§12.50).
            let reserved = self
                .booked_for_sale()
                .iter()
                .find(|&&(i, _)| i == item)
                .map_or(0, |&(_, n)| n);
            if self.in_storage(item) - reserved < count {
                return false; // на складе нечего продать — слот не занимаем
            }
        }
        self.world.spawn(Deal {
            faction,
            item,
            count,
            unit,
            buying,
            // У продажи таймер стоит на нуле до тех пор, пока контейнер не
            // набьют целиком: заводит его `run_trade` (§12.68).
            left: if buying { lead.max(1) } else { 0 },
            delivered: 0,
            cell,
        });
        true
    }

    /// Продавать этой фракции всё, что сверх `keep` штук предмета (§12.87).
    /// Ноль снимает правило.
    ///
    /// **Это правило, а не сделка** (§12.64): команда только пишет число, а
    /// заявки подаёт `run_auto_sales` — каждым тиком, пока есть излишек. Замыкает
    /// набор автоматики: «убирать сам», «беречь себя», порог производства
    /// (§12.65) и автовылазка (§12.67) уже были, а торговля до сих пор делалась
    /// руками на каждый мешок лома.
    ///
    /// **Правило на предмет только одно, а покупатель — его поле** (§12.88).
    /// Адресата называет игрок: «продать тому, кто даёт больше» ядру вычислить
    /// нетрудно — курс чистая функция тика (§12.44), — но такое правило
    /// обыгрывало бы игрока на его же расписании и торговало бы с той стороной,
    /// от которой он держится подальше (§12.43). А вот **двух правил на один
    /// предмет не бывает**: на паре «фракция + предмет» их выходило по два на
    /// лом, и излишек молча доставался первому по палитре. Поэтому команда
    /// перезаписывает правило предмета целиком, вместе с покупателем.
    ///
    /// Ноль — это «правила нет», а не «продавать всё до нуля», ровно как у
    /// порога производства. Второе выразить нечем намеренно: сделка не
    /// отменяется (§12.44), и база, продавшая себя в ноль, не откатывается.
    ///
    /// Вернёт false, если фракция этим предметом не торгует или число
    /// отрицательное. Занятые ячейки и пустой склад отказом **не** являются:
    /// правило ждёт излишка, как порог ждёт материала (§12.30).
    pub fn set_sale(&mut self, faction: usize, item: usize, keep: i32) -> bool {
        note(&mut self.world, format!("sale {faction} {item} {keep}"));
        if keep < 0 {
            return false;
        }
        // Ворота автоматики (§12.93); снятие (`keep == 0`) проходит всегда.
        if keep > 0
            && !self
                .world
                .resource::<AutoRules>()
                .sales_open(self.world.resource::<Techs>())
        {
            return false;
        }
        // Ворота ровно те же, по которым `quote` вернул бы `None`: чем фракция
        // не торгует, на то и порога нет — иначе правило молча не работало бы.
        if self
            .world
            .resource::<FactionRules>()
            .0
            .get(faction)
            .and_then(|rule| rule.price_of(item))
            .is_none()
        {
            return false;
        }
        self.world
            .resource_mut::<Selling>()
            .set(Surplus::Sell(faction), item, keep);
        true
    }

    /// Отдавать в разбор всё, что сверх `keep` штук предмета (§12.115). Ноль
    /// снимает правило.
    ///
    /// **Это второй адресат того же правила, а не второе правило** (§12.115):
    /// команда пишет в тот же слот, что и `set_sale`, поэтому «разбирать
    /// сверх N» стирает «продавать сверх K» вместе с покупателем — ровно как
    /// смена покупателя стирает прежнего (§12.88). Иначе два правила мерили бы
    /// один и тот же излишек одного склада, и предмет сверх обоих чисел
    /// доставался бы тому, кто раньше вызван в `Sim::tick`.
    ///
    /// Ворота — технология **производства** (§12.93), а не сбыта: разбор это
    /// заказ мастерской, и правило открывается тем же, чем порог у станка.
    ///
    /// Вернёт false, если предмет никакой рецепт не разбирает (зеркало ворот
    /// `set_sale`: чем сторона не торгует, на то и порога нет) или число
    /// отрицательное. Пустой склад и занятые станки отказом **не** являются:
    /// правило ждёт излишка, как порог ждёт материала (§12.30).
    pub fn set_salvage_rule(&mut self, item: usize, keep: i32) -> bool {
        note(&mut self.world, format!("salvage {item} {keep}"));
        if keep < 0 {
            return false;
        }
        // Ворота автоматики (§12.93); снятие (`keep == 0`) проходит всегда.
        if keep > 0
            && !self
                .world
                .resource::<AutoRules>()
                .crafting_open(self.world.resource::<Techs>())
        {
            return false;
        }
        if keep > 0 && self.salvage_recipe(item).is_none() {
            return false;
        }
        self.world
            .resource_mut::<Selling>()
            .set(Surplus::Salvage, item, keep);
        true
    }

    /// Рецепт, который разбирает этот предмет, — **первый по палитре**
    /// (§12.115).
    ///
    /// Разборов на один предмет бывает несколько (одну вещь можно раскроить
    /// по-разному), но правило у предмета одно, и выбирать рецепт правилу
    /// нечем: «какой выгоднее» — это решение игрока, а не арифметика (§12.43,
    /// §12.88). Поэтому берём первый и говорим об этом вслух: порядок записей
    /// в рулсете это контент, и автор ставит основной разбор первым. Руками
    /// заказать любой другой по-прежнему можно — правило заменяет клик, а не
    /// запрещает его.
    fn salvage_recipe(&self, item: usize) -> Option<usize> {
        self.world
            .resource::<CraftRules>()
            .0
            .iter()
            .position(|r| r.salvage && r.cost.iter().any(|&(i, per)| i == item && per > 0))
    }

    /// Избранное: поднять строку предмета наверх списка в окне «Склад»
    /// (§12.100).
    ///
    /// **Ворот нет вовсе**, и это не недосмотр: порядок строк — не механика.
    /// Закрепить можно и то, чем не торгует никто (аптечку под порогом
    /// производства), потому что избранное отвечает на «где эта строка», а не
    /// на «с кем по ней торгуют». За второе отвечает `set_ticker`, и вот у
    /// него сторона обязательна.
    pub fn set_favorite(&mut self, item: usize, on: bool) -> bool {
        note(&mut self.world, format!("favorite {item} {on}"));
        self.world.resource_mut::<Favorites>().set(item, on);
        true
    }

    /// Тикер: вынести предмет лентой на главный экран и назвать сторону, с
    /// которой по нему торгуют в один клик (§12.100).
    ///
    /// **Ворота одни, и те же, что у порога автопродажи** (§12.87): сторона
    /// обязана торговать этим предметом. Иначе в ленте встала бы строка с
    /// кнопками, которые фасад отклонит, — а молчащая кнопка читается как
    /// поломка (§12.53). Ворот автоматики (`AutoRules`) здесь нет: тикер не
    /// правило, он ничего не делает сам.
    ///
    /// Снятие (`on == false`) ворот не спрашивает — по тому же доводу, по
    /// которому их не спрашивает снятие правила (§12.93): запертая отмена
    /// оставила бы тикер несбрасываемым.
    pub fn set_ticker(&mut self, item: usize, faction: usize, on: bool) -> bool {
        note(&mut self.world, format!("ticker {item} {faction} {on}"));
        if !on {
            self.world
                .resource_mut::<Tickers>()
                .set(item, faction, false);
            return true;
        }
        if self
            .world
            .resource::<FactionRules>()
            .0
            .get(faction)
            .and_then(|rule| rule.price_of(item))
            .is_none()
        {
            return false;
        }
        self.world
            .resource_mut::<Tickers>()
            .set(item, faction, true);
        true
    }

    /// Правило автопродажи: повторить за игроком клик по кнопке «Продать»
    /// (§12.87, §12.88).
    ///
    /// **Стоит в фасаде, а не в цепочке систем**, и это второй случай после
    /// автовылазки (§12.67). Довод здесь свой и сильнее: у заявки на продажу
    /// трое ворот — свободная ячейка поста (`free_post_cell`), прайс фракции и
    /// складское за вычетом брони, — и все три уже посчитаны в `trade`. Система
    /// считала бы их второй раз, а разошедшийся счёт открыл бы сделку, которую
    /// **нечем закрыть**: отмены у сделки нет (§12.44), и ошибка правила стала
    /// бы вечно занятой ячейкой. Поэтому правило зовёт ту же кнопку, что и
    /// игрок, и стоит там же, где стоял бы он, — перед тиком.
    ///
    /// **Считает оно ровно одно — сколько лишнего, — и меряет это складом**
    /// (§12.91): `in_storage` минус `booked_for_sale`, то самое выражение,
    /// которым отвечает `trade`. Одно число, а не два: «сколько сверх порога» и
    /// «сколько можно выставить» здесь один и тот же вопрос.
    ///
    /// До §12.91 порог считал **всё добро базы** (кучи и лапы) по аналогии с
    /// порогом производства, и это была ошибка: правило меряло одним, а тратило
    /// другим. База с четырьмя сотнями на складе и тремя на полу считалась
    /// стоящей на семистах — и при пороге в пятьсот правило сливало склад до
    /// двухсот, оставляя базе то, чем она наружу распоряжаться не может
    /// (§12.69). Порог обязан меряться в той валюте, которой правило платит.
    ///
    /// С порогом производства это не расходится, а дополняет его: тот **кладёт**
    /// на базу (готовое ложится под ноги мастеру, и склад узнаёт о нём после
    /// уборки), а этот **снимает** со склада. Каждый меряет тот запас, который
    /// сам двигает.
    ///
    /// **Уборки за собой у правила нет** — и это единственное правило, у
    /// которого её нет вовсе (ср. §12.65, где снятый порог обязан убрать свой
    /// заказ). Причина та же, по которой правило живёт в фасаде: сделка
    /// необратима, снятое правило открытую сделку не отзывает, а закроет её
    /// обычная отгрузка.
    fn run_auto_sales(&mut self) {
        let rules = self.world.resource::<Selling>().0.clone();
        if rules.is_empty() {
            return;
        }
        // Правило на предмет одно (§12.88), поэтому спорить за излишек тут
        // некому — но порядок всё равно значим: два **разных** предмета делят
        // последнюю свободную ячейку поста, и решает это сортировка (§11).
        for (item, dest, keep) in rules {
            // Разбор — второй адресат того же правила (§12.115), и уходит он
            // своей дорогой: у него не пост и не сделка, а заказ мастерской.
            if dest == Surplus::Salvage {
                self.run_auto_salvage(item, keep);
                continue;
            }
            let Surplus::Sell(faction) = dest else {
                continue;
            };
            // Ячейка кончилась — дальше по списку идти незачем: `trade` отклонил
            // бы всех подряд, а трейс наполнился бы отказами (§12.45).
            let Some(cell) = self.free_post_cell() else {
                break;
            };
            // **Излишек считается по складу** (§12.91) — тем же выражением,
            // которым ответит `trade`: учтённое минус бронь соседних сделок.
            // Пол и лапы сюда не входят: наружу база отдаёт только убранное
            // (§12.69), и порог, считающий неубранное своим, сливал бы склад
            // ниже собственного числа.
            let booked = self
                .booked_for_sale()
                .iter()
                .find(|&&(i, _)| i == item)
                .map_or(0, |&(_, n)| n);
            let mut count = self.in_storage(item) - booked - keep;
            // **Правило грузит контейнер, а не вагон** (§12.90): больше, чем
            // влезает, `trade` не примет, а молча отклонённая заявка означала бы
            // правило, которое при большом излишке не работает вовсе. Остаток
            // подождёт свободной ячейки — своей или следующего поста, и это та
            // самая причина строить второй.
            let lot = self.lot_at(cell);
            if lot > 0 {
                count = count.min(lot);
            }
            if count <= 0 {
                continue; // на складе нет ничего сверх порога
            }
            self.trade(faction, item, count, false);
        }
    }

    /// Отдать излишек предмета в разбор (§12.115) — заводя обычный заказ той же
    /// кнопкой, которой его завёл бы игрок.
    ///
    /// Правило зовёт кнопку, а не системy, по доводу §12.87: у `start_craft`
    /// свои ворота (склад, свободная ячейка станка, срез по наличию §12.114d),
    /// и второй их экземпляр однажды завёл бы заказ, которого фасад не примет.
    ///
    /// **Порциями, как `plan_craft`** (§12.97): по одному заказу за тик, пятёрка
    /// или остаток. Один заказ на весь излишек занял бы один станок и сползался
    /// бы в одну ячейку, а порция расходится по свободным.
    ///
    /// **Разбор уступает шаблону снаряжения** (§12.115): комбинезон это ещё и
    /// вещь, за которой кот идёт сам (§12.29), и два автомата на один дефицит
    /// оставили бы отряд голым. Поэтому из излишка вычитается по штуке на
    /// каждого неодетого кота на базе — число, а не запрет: одетой базе разбор
    /// не мешает ничем.
    fn run_auto_salvage(&mut self, item: usize, keep: i32) {
        let Some(def) = self.salvage_recipe(item) else {
            return; // рецепт унесли из рулсета — правило молчит, а не падает
        };
        // Свободное считается **тем же выражением, что и ворота кнопки**
        // (§12.129): склад минус бронь продажи минус обещанное открытым
        // заказам. Голый склад врал бы дважды за одну ходку — материал уходит
        // из кучи только когда носильщик до неё дошёл, а шаг длится тики
        // (§12.140), — и правило заказывало бы поверх собственного заказа,
        // сливая запас ниже своего же порога.
        let free = self.craft_room(&[(item, 1)]);
        let surplus = free - keep - self.unequipped_need(item);
        if surplus <= 0 {
            return;
        }
        // Размеры те же, что у кнопки и у порога производства: правило
        // повторяет клик игрока, значит и порции у него те же (§12.97).
        let count = surplus.min(crate::crafting::RULE_BATCH);
        self.start_craft(def, count);
    }

    /// Сколько штук предмета база обязана придержать под шаблон снаряжения
    /// (§12.115): по одной на каждого кота **на базе**, у кого этой вещи ещё
    /// нет. Предмет не из шаблона — ноль.
    ///
    /// Ушедших не считаем: они уже одеты или уже нет, и склад им сейчас не
    /// поможет (§12.22). Считается по `Gear`, то есть по надетому, — везомое к
    /// коту в счёт не идёт, и это осознанный перебор в пользу отряда: лишняя
    /// придержанная штука дешевле голого бойца.
    fn unequipped_need(&mut self, item: usize) -> i32 {
        if !self.world.resource::<LoadoutRules>().0.contains(&item) {
            return 0;
        }
        let mut cats = self
            .world
            .query_filtered::<Option<&Gear>, (With<UnitId>, Without<Away>)>();
        cats.iter(&self.world)
            .filter(|gear| !gear.is_some_and(|g| g.0.contains(&item)))
            .count() as i32
    }

    /// Сколько открытые продажи ещё **должны** — по предметам, **без скидки на
    /// лапы** (§12.50).
    ///
    /// Половина брони, и половина осмысленная сама по себе: её спрашивает тот,
    /// кто считает добро базы **вместе с лапами**, — а это ровно шапка (§12.69),
    /// где груз под сделку числится учтённым. Скидка на лапы там засчитала бы
    /// носильщика дважды: раз как «есть на базе», раз как «уже не обещано». Тем
    /// и отличается от `booked_for_sale`: та отвечает на вопрос «сколько нельзя
    /// брать **из куч**», и лапы из неё вычтены потому, что из куч это уже
    /// взято. Порог автопродажи с §12.91 берёт вторую: он меряет склад.
    pub(crate) fn owed_for_sale(&mut self) -> Vec<(usize, i32)> {
        let mut deals = self.world.query::<&Deal>();
        let mut out: Vec<(usize, i32)> = Vec::new();
        for deal in deals.iter(&self.world).filter(|d| !d.buying) {
            let left = deal.count - deal.delivered;
            if left <= 0 {
                continue;
            }
            match out.iter_mut().find(|(item, _)| *item == deal.item) {
                Some((_, n)) => *n += left,
                None => out.push((deal.item, left)),
            }
        }
        out
    }

    /// Что забронировано под открытую продажу **в кучах** (§12.50). Один расчёт
    /// на всех, кто снимает предметы с базы, — здесь он собирается из мира.
    ///
    /// Это `owed_for_sale` минус то, что уже в лапах: из куч эти штуки взяты, и
    /// запирать их второй раз значило бы держать базе лишнее. Кто считает лапы
    /// наравне с кучами — берёт `owed_for_sale`, а не эту.
    pub(crate) fn booked_for_sale(&mut self) -> Vec<(usize, i32)> {
        let mut out = self.owed_for_sale();
        let mut paws = self.world.query::<(&Haul, &Carrying)>();
        let carried: Vec<(HaulTo, usize, i32)> = paws
            .iter(&self.world)
            .map(|(h, c)| (h.to, c.item, c.count))
            .collect();
        for (to, item, count) in carried {
            if !matches!(to, HaulTo::Sale(_)) {
                continue;
            }
            if let Some(slot) = out.iter_mut().find(|(i, _)| *i == item) {
                slot.1 -= count;
            }
        }
        out.retain(|&(_, n)| n > 0);
        out
    }

    /// Отправить кота `unit_id` учиться домену `skill_id`.
    ///
    /// **Обучение адресно** (§12.18) — как приказ «иди туда» и как заявка на
    /// вылазку: правило §12.16 «игрок размечает работу, исполнителя берёт
    /// симуляция» здесь не нарушается, а получает вторую границу. Обучение —
    /// не работа над базой, а решение о судьбе конкретного кота: это он
    /// перестанет строить на ближайшие пару сотен тиков.
    ///
    /// Команда снимает с кота текущую задачу и распускает его вылазку — ровно
    /// как приказ игрока (§12.23). Вернёт false, если кота нет, он в поле,
    /// домену не учат, коту уже нечему учиться за партой или парты нет.
    ///
    /// **С §12.84 это ещё и приписка** (`Enrolled`), а не только разовая
    /// посадка: ученик, которого увели голод, сон или рана, возвращается за
    /// парту сам — сажает его обратно `assign_study`. До §12.84 он не
    /// возвращался никогда, и игрок узнавал об этом только по не растущему
    /// уровню. Сажать сразу здесь, а не ждать раздатчика, всё равно нужно:
    /// иначе свободный кот трогался бы с места на тик позже, чем нажали, — а
    /// заодно здесь же и отказ, когда все парты заняты (молчащая кнопка
    /// читается как сломанная, §12.53).
    pub fn teach(&mut self, unit_id: &str, skill_id: &str) -> bool {
        note(&mut self.world, format!("teach {unit_id} {skill_id}"));
        let rules = self.world.resource::<SkillRules>();
        let Some(skill) = rules.index_of(skill_id) else {
            return false;
        };
        // Ноль — домену не учат вовсе («Стройка»): парта ему не поможет.
        if rules.taught_cap(skill) <= 0 {
            return false;
        }

        let mut found = None;
        {
            let mut q = self.world.query::<(
                Entity,
                &UnitId,
                &Position,
                Option<&Skills>,
                Option<&Stats>,
                Option<&Away>,
            )>();
            let rules = self.world.resource::<SkillRules>();
            for (e, id, p, skills, stats, away) in q.iter(&self.world) {
                if id.0 == unit_id && away.is_none() {
                    // Предел у каждого кота свой: парта доводит до `taught`, но
                    // не выше врождённого (§12.42).
                    found = Some((
                        e,
                        (p.x, p.y),
                        skills.map_or(0, |s| s.xp_of(skill)),
                        desk_cap(rules, stats, skill),
                    ));
                    break;
                }
            }
        }
        // Кота нет на базе — учить некого: его позиция это шлюз, с которого он
        // ушёл (§12.22).
        let Some((cat_e, from, xp, cap)) = found else {
            return false;
        };
        // Парта — вход в домен, а не тренажёр: доученного она не берёт. И не
        // берёт того, кому парта уже ничего не даст: отправленный за неё кот
        // встал бы с неё в тот же тик, а игрок прочёл бы это как поломку.
        if xp >= cap {
            return false;
        }

        let taken = self.taken_desks();
        let Some(spot) = nearest_desk(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            skill,
            from,
            &taken,
        ) else {
            return false; // парт нет, все заняты или до них не добраться
        };

        self.release_task(cat_e);
        if let Some(mission_e) = self.world.get::<Squad>(cat_e).map(|s| s.0) {
            self.disband(mission_e);
        }
        // Старый приказ «иди туда» снимается: обучение — такое же адресное
        // распоряжение этим котом, и два противоречащих друг другу висеть не
        // должны. Иначе кот, доучившись, ушёл бы «доисполнять» забытый приказ.
        let path = find_path(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            from,
            spot,
        )
        .unwrap_or_default();
        self.world.entity_mut(cat_e).remove::<Order>().insert((
            Enrolled { skill },
            Study { skill, spot },
            Path { steps: path },
        ));
        true
    }

    /// Если в клетке парта и коту есть чему за ней научиться — записать его
    /// именно за эту парту (§12.85). Иначе `false`, и приказ идёт как обычно.
    ///
    /// Парта **именно эта**, а не ближайшая свободная, как у `teach`: игрок
    /// ткнул в клетку, и посадить его за другую значило бы ответить не на тот
    /// жест. Занятую парту это отклоняет — тогда «иди туда» остаётся приказом,
    /// и кот просто дойдёт: два кота за одной партой не сидят (§12.20).
    fn teach_at(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        let skill = {
            let map = self.world.resource::<BaseMap>();
            let tiles = self.world.resource::<TileRules>();
            match tiles.teaches_of(map.tile_at(x, y)) {
                Some(skill) => skill,
                None => return false,
            }
        };
        if self.taken_desks().contains(&(x, y)) {
            return false;
        }
        let Some(cat_e) = self.unit_on_base(unit_id) else {
            return false;
        };
        // Доучившемуся парта не поможет: приказ остаётся приказом, кот дойдёт и
        // займётся своими делами. Молчаливой записи, которая ничего не даёт,
        // тут быть не должно — причину игрок читает на кнопке (§12.84).
        let (xp, cap) = {
            let skills = self
                .world
                .get::<Skills>(cat_e)
                .map_or(0, |s| s.xp_of(skill));
            let stats = self.world.get::<Stats>(cat_e);
            let rules = self.world.resource::<SkillRules>();
            (skills, desk_cap(rules, stats, skill))
        };
        if xp >= cap {
            return false;
        }
        let id = unit_id.to_string();
        note(&mut self.world, format!("teach_at {id} {x} {y}"));

        let from = self
            .world
            .get::<Position>(cat_e)
            .map_or((0, 0), |p| (p.x, p.y));
        self.release_task(cat_e);
        if let Some(mission_e) = self.world.get::<Squad>(cat_e).map(|s| s.0) {
            self.disband(mission_e);
        }
        let path = find_path(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            from,
            (x, y),
        )
        .unwrap_or_default();
        self.world.entity_mut(cat_e).remove::<Order>().insert((
            Enrolled { skill },
            Study {
                skill,
                spot: (x, y),
            },
            Path { steps: path },
        ));
        true
    }

    /// Снять кота с учёбы — и приписку, и текущую задачу (§12.84).
    ///
    /// Зеркало `unpost_relay`, и снимать задачу тут обязательно по той же
    /// причине: иначе кот досидел бы за партой до потолка, и игрок решил бы,
    /// что кнопка не сработала.
    ///
    /// **Приказ «иди туда» приписку не снимает** — как не снимает приписку к
    /// рации (§12.60). Один день он её снимал, и это было прямой поломкой:
    /// клетка парты — ровно то место, куда игрок кликает, чтобы отправить кота
    /// учиться, а клик по клетке это приказ. Кот доходил до парты и уходил
    /// работать, потому что тем же кликом учёбу и отменили. Отмена обязана быть
    /// названной кнопкой, а не побочным действием чего-то другого.
    ///
    /// Вернёт false, если кота нет или он ничему не приписан.
    pub fn unteach(&mut self, unit_id: &str) -> bool {
        note(&mut self.world, format!("unteach {unit_id}"));
        let found = {
            let mut q = self.world.query::<(Entity, &UnitId)>();
            q.iter(&self.world)
                .find(|(_, id)| id.0 == unit_id)
                .map(|(e, _)| e)
        };
        let Some(cat_e) = found else {
            return false;
        };
        if self.world.get::<Enrolled>(cat_e).is_none() {
            return false;
        }
        self.world
            .entity_mut(cat_e)
            .remove::<(Enrolled, Study, Path, Stride)>();
        true
    }

    /// Приписать кота к узлу связи (§12.60): пока идёт вылазка этого узла, он
    /// садится к рации и держит связь.
    ///
    /// **Приписка — конфигурация, а не задача.** Приписанный работает как все;
    /// на узел его сажает раздатчик, и только когда там нужна связь. Голод и сон
    /// снимают дежурство, но не приписку — кот вернётся сам. Разовая посадка
    /// повторила бы дыру парты: игрок кликнул, кот поел и больше не вернулся, а
    /// пропажу связи видно только просевшим бонусом на возвращении.
    ///
    /// **Узел держит одного связиста**, как парта и лежанка: новая приписка
    /// снимает прежнюю молча — и саму приписку, и текущее дежурство. Отказывать
    /// было бы хуже: кнопка молчала бы при занятом узле, а молчащая кнопка
    /// читается как сломанная (§12.53). Это зеркало правила «кот числится не
    /// более чем в одном отряде» (§12.61), только с другой стороны: там одна
    /// приписка на кота, здесь — одна на узел.
    ///
    /// С §12.76 сам по себе к рации никто не садится, поэтому «уступает» тут
    /// всегда такой же приписанный, а не самосевший.
    ///
    /// Вернёт false, если кота нет, он не на базе или в клетке не узел связи.
    pub fn post_relay(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("post_relay {unit_id} {x} {y}"));
        if !self.is_relay_at(x, y) {
            return false;
        }
        let Some(cat_e) = self.unit_on_base(unit_id) else {
            return false;
        };
        // Прежний связист этого узла уступает: решение игрока свежее. Идущему к
        // рации снимаем и маршрут — иначе он дошагает до чужого места и встанет
        // там без дела, как это делает `unpost_relay`.
        let others: Vec<(Entity, bool)> = {
            let mut q = self
                .world
                .query::<(Entity, Option<&OnDuty>, Option<&Posted>)>();
            q.iter(&self.world)
                .filter(|(e, duty, post)| {
                    *e != cat_e
                        && (duty.is_some_and(|d| d.spot == (x, y))
                            || post.is_some_and(|p| p.spot == (x, y)))
                })
                .map(|(e, duty, _)| (e, duty.is_some()))
                .collect()
        };
        for (e, on_duty) in others {
            let mut cat = self.world.entity_mut(e);
            cat.remove::<(Posted, OnDuty)>();
            if on_duty {
                cat.remove::<(Path, Stride)>();
            }
        }
        self.world.entity_mut(cat_e).insert(Posted { spot: (x, y) });
        true
    }

    /// Снять приписку к узлу — и вместе с ней текущее дежурство (§12.60).
    ///
    /// Снимать дежурство обязательно: иначе связист досидит до конца вылазки, и
    /// игрок решит, что кнопка не сработала. Это единственное место, где отмена
    /// трогает мир, а не только конфигурацию.
    ///
    /// Во время вылазки отменять **можно**: связь считается за тик, накопленное
    /// уже накоплено, и бесплатного опциона тут нет — в отличие от сделки
    /// (§12.44) и отзыва ушедшего отряда.
    pub fn unpost_relay(&mut self, unit_id: &str) -> bool {
        note(&mut self.world, format!("unpost_relay {unit_id}"));
        let found = {
            let mut q = self.world.query::<(Entity, &UnitId)>();
            q.iter(&self.world)
                .find(|(_, id)| id.0 == unit_id)
                .map(|(e, _)| e)
        };
        let Some(cat_e) = found else {
            return false;
        };
        if self.world.get::<Posted>(cat_e).is_none() {
            return false;
        }
        self.world
            .entity_mut(cat_e)
            .remove::<(Posted, OnDuty, Path, Stride)>();
        true
    }

    /// Зачислить кота в отряд гаража (§12.61, §12.152).
    ///
    /// Состав хранится на клетке и **переживает вылазку**: вернувшийся отряд
    /// остаётся отрядом, и второй раз его собирать не надо. Это конфигурация, а
    /// не задача, — как приписка связиста (§12.60): зачисленный работает как все,
    /// пока не подана заявка.
    ///
    /// Кот числится **не более чем в одном отряде**: зачисление ко второму
    /// гаражу снимает первое молча. Отказывать было бы хуже — игрок не видит,
    /// где кот числился раньше, и кнопка молчала бы без объяснения (§12.53).
    ///
    /// Вернёт false, если в клетке не гараж, кота нет на базе или он уже подан в
    /// заявку (`Squad`): состав вышедшего отряда не переигрывают — для этого
    /// есть отзыв (`cancel_mission`).
    pub fn enlist(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        note(&mut self.world, format!("enlist {unit_id} {x} {y}"));
        if !self.is_gate_at(x, y) {
            return false;
        }
        let Some(cat_e) = self.unit_on_base(unit_id) else {
            return false;
        };
        if self.world.get::<Squad>(cat_e).is_some() {
            return false;
        }
        self.world
            .entity_mut(cat_e)
            .insert(Enlisted { spot: (x, y) });
        true
    }

    /// Вычеркнуть кота из отряда узла (§12.61).
    ///
    /// Мир это не трогает: `Enlisted` — конфигурация, задачи за ней нет, и
    /// вычёркивать посреди вылазки нечего (ушедшего держит `Squad`). Тем
    /// `dismiss` и отличается от `unpost_relay`, который обязан снять ещё и
    /// дежурство.
    ///
    /// Вернёт false, если кота нет или он ни в каком отряде не числился.
    pub fn dismiss(&mut self, unit_id: &str) -> bool {
        note(&mut self.world, format!("dismiss {unit_id}"));
        let found = {
            let mut q = self.world.query::<(Entity, &UnitId)>();
            q.iter(&self.world)
                .find(|(_, id)| id.0 == unit_id)
                .map(|(e, _)| e)
        };
        let Some(cat_e) = found else {
            return false;
        };
        if self.world.get::<Enlisted>(cat_e).is_none() {
            return false;
        }
        self.world.entity_mut(cat_e).remove::<Enlisted>();
        true
    }

    /// Кот на базе по `id`: ушедших и пленных не берём — их тут нет (§12.40).
    fn unit_on_base(&mut self, unit_id: &str) -> Option<Entity> {
        let mut q = self.world.query::<(Entity, &UnitId, Option<&Away>)>();
        q.iter(&self.world)
            .find(|(_, id, away)| id.0 == unit_id && away.is_none())
            .map(|(e, ..)| e)
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
    ///
    /// **Клетка с ролью отвечает на приказ своей ролью** (§12.85): послать кота
    /// на парту и значит записать его учиться, отдельного шага для этого нет.
    /// Это и есть жест, которым игрок пользуется, — «кликнул кота, кликнул
    /// парту», — а «иди туда» на парте не значит ничего: кот постоит и уйдёт
    /// работать. Кнопка в тулбаре остаётся вторым путём для тех, кто не хочет
    /// искать клетку глазами.
    pub fn set_target(&mut self, unit_id: &str, x: i32, y: i32) -> bool {
        // Парта и рация разбираются до всего остального: это не приказ с
        // довеском, а другие команды, и `note` о них пишут они сами.
        if self.teach_at(unit_id, x, y) {
            return true;
        }
        // Рация — второй случай того же правила (§12.85): послать кота на узел
        // связи и значит приписать его к нему. Садится он не сразу, а когда
        // отсюда уйдёт вылазка (§12.76), — поэтому приписка тут и уместна:
        // «иди туда» на рации не значит ровно ничего, кот постоит и уйдёт.
        {
            if self.is_relay_at(x, y) && self.post_relay(unit_id, x, y) {
                return true;
            }
        }
        note(&mut self.world, format!("move {unit_id} {x} {y}"));
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
        // Кота нет на базе — приказывать некому: его позиция это шлюз, с
        // которого он ушёл, а сам он вернётся туда же (§12.22).
        if self.world.get::<Away>(entity).is_some() {
            return false;
        }

        if !self
            .world
            .resource::<BaseMap>()
            .walkable(self.world.resource::<TileRules>(), x, y)
        {
            return false;
        }
        let map_version = self.world.resource::<BaseMap>().version;
        let path = find_path(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
            (sx, sy),
            (x, y),
        );

        // Приказ забирает кота у любой задачи — стройки, переноса, учёбы
        // (§12.15, §12.20). Груз он при этом не бросает: донесёт, когда снова
        // возьмётся за доставку.
        //
        // **Кроме сна, пока включено «Беречь себя»** (§12.51): спящий досыпает
        // своё, а приказ ждёт его — `Order` остаётся висеть, и маршрут проложит
        // `retry_orders` тем же тиком, каким кот проснулся. Тот же выключатель,
        // что и у вылазки, и та же причина: сон — это состояние, а не пометка
        // игрока (§12.33). Выключенный — будит, как будил всегда.
        let asleep =
            self.world.resource::<AutoRest>().0 && self.world.get::<Rest>(entity).is_some();
        if !asleep {
            self.release_task(entity);
        }

        // А вылазку приказ **распускает целиком**: состав выбран поимённо,
        // заменить выбывшего некем, и отряд, который никогда не соберётся,
        // хуже честного роспуска (§12.23). Ушедшего это не касается — такого
        // кота нет в мире базы, и приказ ему отклонён выше.
        if let Some(mission_e) = self.world.get::<Squad>(entity).map(|s| s.0) {
            self.disband(mission_e);
        }

        // Приказ сохраняется даже без пути прямо сейчас — `retry_orders`
        // перепроложит маршрут при следующем изменении карты (например, после
        // постройки коридора, открывающего доступ к цели).
        // Отметку ставим только на неудаче: приказ с найденным маршрутом ещё
        // ничего не пробовал безуспешно, и если маршрут потом потеряется (сон,
        // рана, вылазка), `retry_orders` обязан проложить его заново тем же
        // тиком, не дожидаясь смены карты.
        self.world.entity_mut(entity).insert(Order {
            x,
            y,
            tried_version: path.is_none().then_some(map_version),
        });
        // Спящему маршрут не трогаем вовсе: `Path` при `Rest` значит «идёт к
        // лежанке» (§12.20), и выданный к цели он остановил бы сам сон, а
        // снятый — оставил бы кота стоять на полдороге. Приказ подхватит
        // `retry_orders`, как только `Rest` снимется.
        if asleep {
            return true;
        }
        match path {
            Some(p) => {
                self.world.entity_mut(entity).insert(Path { steps: p });
            }
            None => {
                self.world.entity_mut(entity).remove::<(Path, Stride)>();
            }
        }
        true
    }

    /// Один фиксированный шаг симуляции.
    pub fn tick(&mut self) {
        // Правило автовылазки — **до** цепочки и в фасаде, а не в ней (§12.67):
        // оно повторяет клик игрока по кнопке заказа, а кнопка живёт здесь.
        // Перед тиком, потому что игрок нажал бы её между тиками, и заявка
        // обязана застать `gather_squad` этого же тика — иначе отряд теряет тик
        // на ровном месте.
        self.run_auto_raids();
        // Автопродажа — там же и по той же причине (§12.87): она повторяет клик
        // по кнопке «Продать», а у кнопки трое ворот, второй экземпляр которых
        // открыл бы неотменяемую сделку. Перед тиком, чтобы заявку застал
        // `assign_hauls` этого же тика — иначе носильщик теряет тик впустую.
        self.run_auto_sales();
        self.schedule.run(&mut self.world);
        // Лента новостей — **после** цепочки (§12.120): новость читает
        // сложившийся тик и мир не меняет, дословно `check_goals`.
        self.note_news();
    }

    /// Рендерабельные сущности + чертежи (для PixiJS).
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        let tick = self.world.resource::<SimTime>().tick;

        // Площадки сноса: `Busy` знает, что кот занят чертежом, но не тем, во что
        // тот обернётся, — а «строит» и «разбирает» игрок читает по-разному
        // (§12.41). Собирается до котов, потому что чертёж читают по `Assignment`.
        let doomed: std::collections::HashSet<Entity> = {
            let mut q = self.world.query::<(Entity, &Blueprint)>();
            q.iter(&self.world)
                .filter(|(_, bp)| bp.tile < 0)
                .map(|(e, _)| e)
                .collect()
        };

        // Палитра параметров живёт рядом с палитрой тайлов, а не в мире: у кота
        // лежат только значения, и длину списка задаёт рулсет (§12.42).
        let stat_count = self.stats.len();
        let mut entities = Vec::new();
        {
            // Задачи кота собраны вложенным кортежем: их ровно столько, сколько
            // берёт `Busy::of`, и растут они вместе — а плоский запрос упёрся бы
            // в предел арности `QueryData`.
            let mut q = self.world.query::<(
                &UnitId,
                &Renderable,
                &Position,
                Option<&Carrying>,
                Option<&Carry>,
                Option<&Skills>,
                Option<&Stats>,
                Option<&Perks>,
                Option<&Energy>,
                Option<&Fed>,
                Option<&Gear>,
                Option<&Health>,
                Option<&Captive>,
                Option<&Posted>,
                (
                    Option<&Order>,
                    Option<&Path>,
                    Option<&Assignment>,
                    Option<&Haul>,
                    Option<&Rest>,
                    Option<&Study>,
                    Option<&Researching>,
                    Option<&Crafting>,
                    Option<&Equipping>,
                    Option<&Eating>,
                    Option<&Healing>,
                    Option<&Treating>,
                    Option<&Squad>,
                    Option<&OnDuty>,
                    Option<&Away>,
                ),
            )>();
            // Зачисление в отряд узла спрашивается отдельным запросом, а не
            // шестнадцатым полем в кортеже: тот уже упёрся в предел арности
            // `QueryData` и потому собран вложенным.
            let crews: Vec<(String, (i32, i32))> = {
                let mut q = self.world.query::<(&UnitId, &Enlisted)>();
                q.iter(&self.world)
                    .map(|(id, e)| (id.0.clone(), e.spot))
                    .collect()
            };
            // Шаг, который кот делает прямо сейчас (§12.140), — тем же отдельным
            // запросом и по той же причине: кортеж выше уже упёрся в предел
            // арности `QueryData`.
            let strides: Vec<(String, ((i32, i32), u8, u8))> = {
                let mut q = self.world.query::<(&UnitId, &Stride)>();
                q.iter(&self.world)
                    .map(|(id, s)| (id.0.clone(), (s.to, s.left, s.span)))
                    .collect()
            };
            // Приписка к парте (§12.84) — тем же отдельным запросом и по той же
            // причине, что и зачисление в отряд.
            let enrolled: Vec<(String, usize)> = {
                let mut q = self.world.query::<(&UnitId, &Enrolled)>();
                q.iter(&self.world)
                    .map(|(id, e)| (id.0.clone(), e.skill))
                    .collect()
            };
            let map = self.world.resource::<BaseMap>();
            let rules = self.world.resource::<SkillRules>();
            let tiles = self.world.resource::<TileRules>();
            let needs = self.world.resource::<NeedRules>();
            let food = self.world.resource::<FoodRules>();
            let hurts = self.world.resource::<HealthRules>();
            // Каков этот кот **в поле** (§12.71). Раньше это знал только узел, и
            // только про уже зачисленных (`NodeSnap::forces`), — про кота,
            // которого игрок ещё не взял, окно не знало ничего, и состав
            // подбирался перебором. Считается здесь, потому что это свойство
            // кота, а не узла: то же число объясняет карточку кота на карте.
            let raid = self.world.resource::<SkillRules>().index_of(SKILL_RAID);
            let items = self.world.resource::<ItemRules>();
            let stat_rules = self.world.resource::<StatRules>();
            for (
                id,
                r,
                p,
                load,
                carry,
                skills,
                stats,
                perks,
                energy,
                fed,
                gear,
                health,
                captive,
                post,
                tasks,
            ) in q.iter(&self.world)
            {
                let (
                    order,
                    path,
                    assignment,
                    haul,
                    rest,
                    study,
                    researching,
                    crafting,
                    equipping,
                    eating,
                    healing,
                    treating,
                    squad,
                    duty,
                    away,
                ) = tasks;
                // Место для сна под лапами — то, из чего `Busy::of` соберёт
                // «дремлет», если задач у кота не нашлось (§12.52). Считается
                // здесь, потому что карта и правила тайлов в `Busy` не ходят.
                //
                // Полную бодрость сюда **не подмешиваем**, хотя `doze` на ней и
                // останавливается: кот на потолке теряет очко в `tire` и
                // добирает его обратно следующим тиком, так что подпись мигала
                // бы «дремлет / без дела» каждый кадр.
                let bed = tiles.rest_of(map.tile_at(p.x, p.y)) > 0;
                // Слагаемые силы в поле (§12.71): считаются здесь, чтобы сама
                // сила ниже сложилась **из них**, а не рядом с ними.
                let raid_skill = raid.map_or(0, |s| level_of(rules, skills, s));
                let gear_force = items.force_of_gear(gear);
                let busy = Busy::of(
                    order,
                    path,
                    assignment,
                    haul,
                    rest,
                    study,
                    researching,
                    crafting,
                    equipping,
                    eating,
                    healing,
                    treating,
                    squad,
                    duty,
                    away,
                    bed,
                );
                // Шаг ищется по `id` тем же перебором, что и отряд ниже: котов
                // на базе десяток, и словарь ради этого не заводится.
                let stride = strides
                    .iter()
                    .find(|(who, _)| *who == id.0)
                    .map(|(_, s)| *s);
                entities.push(EntitySnap {
                    id: id.0.clone(),
                    sprite: r.sprite.clone(),
                    x: p.x,
                    y: p.y,
                    stuck: is_stuck(map, tiles, p, busy),
                    away: away.is_some(),
                    // Пленный тоже `away` — на карте его нет. Но пропавший кот
                    // обязан быть объясним: «ушёл на вылазку» видно в панели
                    // миссии, а «остался там» — только здесь (§12.40).
                    captive: captive.is_some(),
                    // Куда уже послан (§12.86): панель клетки этим отличает
                    // «пойдут» от «уже идёт».
                    order_x: order.map_or(-1, |o| o.x),
                    order_y: order.map_or(-1, |o| o.y),
                    post_x: post.map_or(-1, |p| p.spot.0),
                    post_y: post.map_or(-1, |p| p.spot.1),
                    // В каком отряде числится (§12.61). Отдельно от дежурства:
                    // это разные решения игрока об одном и том же узле — «идёт
                    // отсюда» и «сидит здесь».
                    crew_x: crews
                        .iter()
                        .find(|(who, _)| *who == id.0)
                        .map_or(-1, |(_, s)| s.0),
                    crew_y: crews
                        .iter()
                        .find(|(who, _)| *who == id.0)
                        .map_or(-1, |(_, s)| s.1),
                    energy: energy.map_or(0, |e| e.0),
                    energy_max: needs.max,
                    energy_tired: needs.tired,
                    energy_critical: needs.critical,
                    fed: fed.map_or(0, |f| f.0),
                    fed_max: food.max,
                    // Порог голода уходит наружу вместе со шкалой: без него
                    // панель не отличит «наелся минуту назад» от «уже идёт
                    // есть», а второй экземпляр числа в JS однажды разойдётся
                    // с рулсетом (§12.26 — считает ядро, а не интерфейс).
                    fed_hungry: food.hungry,
                    health: health.map_or(0, |h| h.0),
                    health_max: hurts.max,
                    health_hurt: hurts.hurt,
                    // Чем занят — разобрано в `Busy` вместе с самой занятостью
                    // (§12.41); здесь чертёж только уточняется до сноса.
                    job: match busy.job {
                        "build" if assignment.is_some_and(|a| doomed.contains(&a.0)) => "demolish",
                        job => job,
                    },
                    moving: busy.moving,
                    // Стоящий кот «идёт» в свою же клетку: нулевой `step_span`
                    // и есть признак, что рисовать нечего.
                    to_x: stride.map_or(p.x, |(to, _, _)| to.0),
                    to_y: stride.map_or(p.y, |(to, _, _)| to.1),
                    step_left: stride.map_or(0, |(_, left, _)| left),
                    step_span: stride.map_or(0, |(_, _, span)| span),
                    carrying: load.map_or(0, |c| c.count),
                    carrying_item: load.map_or(-1, |c| c.item as i32),
                    carry_max: carry.map_or(0, |c| c.0),
                    skills: (0..rules.0.len())
                        .map(|i| {
                            let xp = skills.map_or(0, |s| s.xp_of(i));
                            SkillSnap {
                                level: rules.level(i, xp),
                                xp,
                                next: rules.next_threshold(i, xp),
                                cap: level_cap_of(rules, stats, i),
                                desk: desk_cap(rules, stats, i),
                            }
                        })
                        .collect(),
                    perks: perks.map(|p| p.0.clone()).unwrap_or_default(),
                    // Врождённое: полоска опыта, упёршаяся в предел, объясняется
                    // только здесь — без этой строки игрок читает застрявший
                    // навык как поломку, а не как свойство кота (§12.42).
                    stats: (0..stat_count)
                        .map(|i| stats.map_or(0, |s| s.value_of(i)))
                        .collect(),
                    // Надетое видно в панели кота: снаряжение молча прибавляет
                    // отряду силы, и без этого игрок не свяжет пропавший со
                    // склада комбинезон с выросшим прогнозом вылазки (§12.29).
                    gear: gear.map(|g| g.0.clone()).unwrap_or_default(),
                    // Приписка, а не задача: `study` в `job` уже есть, но он
                    // молчит про кота, которого увёл сон (§12.84).
                    study: enrolled
                        .iter()
                        .find(|(who, _)| who == &id.0)
                        .map_or(-1, |(_, skill)| *skill as i32),
                    // Тем же выражением, каким сила отряда сложится на уходе
                    // (§12.23, инвариант 14): сам кот стоит единицу, уровень
                    // «Вылазки» — сверху, надетое — ещё сверху. Складывать их
                    // в JS значило бы завести второй экземпляр правила силы,
                    // поэтому наружу едет **и** сумма, и слагаемые, а сумма
                    // считается из них же: «+4 силы» само по себе не говорит,
                    // опытный это боец или одетый новичок, а разойтись с
                    // подписью она теперь не может.
                    raid_force: raid_skill + gear_force + 1,
                    raid_skill,
                    gear_force,
                    // Проводник: ступень — чтобы сравнить кандидата с нынешним
                    // ведущим (считается по лучшему, а не по сумме), процент —
                    // чтобы панель называла следствие реакции, а не её шкалу
                    // (§12.71). Какая из ступеней делает проводника, знает ядро:
                    // искать `reflex` по имени в JS — второй экземпляр правила.
                    guide_step: guide_of(stat_rules, stats),
                    guide_cut: guide_cut(guide_of(stat_rules, stats)),
                });
            }
        }

        let mut blueprints = Vec::new();
        {
            let mut q = self.world.query::<&Blueprint>();
            let rules = self.world.resource::<TileRules>();
            for bp in q.iter(&self.world) {
                blueprints.push(BlueprintSnap {
                    x: bp.x,
                    y: bp.y,
                    tile: bp.tile,
                    progress: bp.progress,
                    total: BUILD_WORK,
                    // Полоска подвоза показывает набор целиком: сколько всего
                    // штук нужно и сколько уже лежит на площадке.
                    need: rules.cost_of(bp.tile).iter().map(|&(_, n)| n).sum(),
                    delivered: bp.delivered.iter().map(|&(_, n)| n).sum(),
                    // А панель клетки называет недостачу поимённо: цена
                    // упорядочена по имени предмета (инвариант 12), значит и
                    // список детерминирован.
                    missing: rules
                        .cost_of(bp.tile)
                        .iter()
                        .filter_map(|&(item, n)| {
                            let got = bp
                                .delivered
                                .iter()
                                .find(|&&(i, _)| i == item)
                                .map(|&(_, c)| c)
                                .unwrap_or(0);
                            (n - got > 0).then_some(NeedSnap {
                                item,
                                left: n - got,
                            })
                        })
                        .collect(),
                });
            }
        }

        let mut stacks = Vec::new();
        {
            let mut q = self.world.query::<(&Position, &Stack, Option<&ToStore>)>();
            for (p, s, mark) in q.iter(&self.world) {
                stacks.push(StackSnap {
                    x: p.x,
                    y: p.y,
                    item: s.item,
                    count: s.count,
                    marked: mark.is_some(),
                });
            }
        }

        // Имущество базы по типам, разложенное на «чем можно платить» и «что
        // ещё надо убрать» (§12.53). Одним числом это врало бы: платит склад,
        // а игрок читал бы сумму всего, что валяется по базе, и не понимал,
        // почему найм отклонён.
        let stock = self.stock();

        let mut missions = Vec::new();
        {
            let raid = self.world.resource::<SkillRules>().index_of(SKILL_RAID);
            let mut crew = self.world.query::<(
                &UnitId,
                &Squad,
                Option<&Away>,
                Option<&Skills>,
                Option<&Gear>,
                Option<&Rest>,
                Option<&Stats>,
            )>();
            let skill_rules = self.world.resource::<SkillRules>();
            let stat_rules = self.world.resource::<StatRules>();
            let items = self.world.resource::<ItemRules>();
            // Вклад кота в силу отряда считается ровно как в `run_missions`:
            // сам он стоит единицу, уровень «Вылазки» — сверху, надетое — ещё
            // сверху. Прогноз и результат обязаны быть одним выражением (§12.23).
            let members: Vec<(Entity, String, bool, i32, bool, i32, i32)> = crew
                .iter(&self.world)
                .map(|(id, squad, away, skills, gear, rest, stats)| {
                    let force = 1
                        + raid.map_or(0, |s| level_of(skill_rules, skills, s))
                        + items.force_of_gear(gear);
                    (
                        squad.0,
                        id.0.clone(),
                        away.is_some(),
                        force,
                        rest.is_some(),
                        guide_of(stat_rules, stats),
                        guide_value(stat_rules, stats),
                    )
                })
                .collect();
            // Кто прямо сейчас держит связь: дошедший дежурный, без маршрута
            // (§12.60). Ровно тот же признак, по которому копит `run_missions`.
            let manned: Vec<(i32, i32)> = {
                let mut q = self.world.query_filtered::<&OnDuty, Without<Path>>();
                q.iter(&self.world).map(|d| d.spot).collect()
            };
            let mut q = self.world.query::<(Entity, &Mission)>();
            let rules = self.world.resource::<MissionRules>();
            for (e, m) in q.iter(&self.world) {
                let rule = rules.0.get(m.def);
                let mine = || members.iter().filter(move |&&(owner, ..)| owner == e);
                // Опасность, какой её встретит **этот** отряд: проводник режет
                // её тем же выражением, что и на возвращении (§12.70,
                // инвариант 14). Наружу едет уже урезанная — игрок должен
                // видеть то, с чем коты столкнутся, а исходную рядом показывает
                // панель по `danger_base`.
                let base = rule.map_or(0, |r| r.danger);
                let guide = mine().map(|&(.., g, _)| g).max().unwrap_or(0);
                let paws = mine().count();
                let danger = rule.map_or(0, |r| crew_danger(r, guide, paws));
                // Связь входит в **ту же** силу, что и отряд, и считается тем же
                // выражением, что на возвращении (§12.60, инвариант 14). Число
                // это «что будет, если связь оборвётся прямо сейчас»: она копится
                // за тик, поэтому прогноз честно растёт вместе с ней.
                // Полный срок до ухода ещё не посчитан (`span` = 0): прогноз
                // берёт его по тому составу, который сейчас в отряде, — тем же
                // выражением, каким срок замёрзнет на уходе (§12.70).
                let span = match m.span {
                    0 => rule.map_or(0, |r| duration(r, paws)),
                    frozen => frozen,
                };
                let comms = relay_force(m.covered, span);
                let raw = mine().map(|&(.., force, _, _, _)| force);
                let force: i32 = match rule {
                    Some(r) => crew_force(r, raw),
                    None => raw.sum(),
                } + comms;
                let out = outcome(danger, force);
                missions.push(MissionSnap {
                    def: m.def,
                    x: m.gate.0,
                    y: m.gate.1,
                    left: m.left,
                    total: span,
                    squad: mine().map(|(_, id, ..)| id.clone()).collect(),
                    size: rule.map_or(0, |r| r.squad),
                    away: mine().any(|&(_, _, away, ..)| away),
                    // Ждать отряд может только на базе: за шлюзом не спят.
                    resting: mine().any(|&(.., resting, _, _)| resting),
                    strength: out.strength,
                    danger,
                    danger_base: base,
                    // Проводник — кот с лучшей ступенью; ничью решает `id`,
                    // потому что порядок обхода ECS недетерминирован (§11).
                    // Ничью решает сырая реакция, потом `id` — то же правило,
                    // что у `node_guide` (§12.70).
                    guide: mine()
                        .filter(|&&(.., g, _)| g > 0 && g == guide)
                        .map(|(_, id, .., raw)| (-raw, id.clone()))
                        .min()
                        .map(|(_, id)| id)
                        .unwrap_or_default(),
                    share: out.share,
                    failed: out.failed,
                    patron: rule.and_then(|r| r.patron).map_or(-1, |f| f as i32),
                    against: rule.and_then(|r| r.against).map_or(-1, |f| f as i32),
                    // Той же долей, что и добыча: прогноз и результат — одно
                    // выражение, иначе игрок увидит одно, а получит другое.
                    standing: rule.map_or(0, |r| r.standing) * out.share / 100,
                    // Вылазка за своим возвращает не добычу, а кота, и панель
                    // обязана говорить об этом иначе: «добыча 50 %» под именем
                    // спасательной вылазки читается как «половина кота» (§12.40).
                    rescue: rule.is_some_and(|r| r.rescue),
                    comms,
                    manned: !manned.is_empty(),
                });
            }
        }

        let raids: Vec<RaidSnap> = {
            let count = self.world.resource::<MissionRules>().0.len();
            (0..count).map(|def| self.raid_gates(def)).collect()
        };

        let fame = self.world.resource::<Fame>().0;
        // Репутация уходит наружу целиком, в порядке палитры: чего не видно,
        // того для игрока нет, а решение о стороне обязано быть читаемым.
        let standing: Vec<i32> = (0..self.factions.len())
            .map(|f| self.world.resource::<Standing>().value_of(f))
            .collect();
        let money = self.world.resource::<Money>().0;
        let deals: Vec<DealSnap> = {
            let mut q = self.world.query::<&Deal>();
            let mut out: Vec<DealSnap> = q
                .iter(&self.world)
                .map(|d| DealSnap {
                    faction: d.faction,
                    item: d.item,
                    count: d.count,
                    unit: d.unit,
                    buying: d.buying,
                    left: d.left,
                    delivered: d.delivered,
                    x: d.cell.0,
                    y: d.cell.1,
                })
                .collect();
            // По клетке, а не по обходу ECS: тот порядок зависит от истории
            // вставок, и закрывшаяся сделка переставляла соседние карточки под
            // курсором игрока. Правило то же, по которому сортируются кучи и
            // места в раздатчиках (инвариант 9), — «места по клетке», row-major.
            out.sort_by_key(|d| (d.y, d.x));
            out
        };
        // Курсы считаются тем же `quote`, которым посчитается заказ, — двух
        // арифметик цены быть не должно (§12.44).
        let prices: Vec<PriceSnap> = {
            let tick = self.world.resource::<SimTime>().tick;
            let factions = self.world.resource::<FactionRules>();
            let standing = self.world.resource::<Standing>();
            let fame = self.world.resource::<Fame>();
            let mut out = Vec::new();
            for (f, rule) in factions.0.iter().enumerate() {
                let ahead = crate::trade::phase_left(rule, tick);
                for (item, _) in &rule.prices {
                    let at = |t: u64, buying: bool| {
                        crate::trade::quote(factions, standing, f, *item, t, buying).unwrap_or(0)
                    };
                    let next_tick = tick + ahead.unwrap_or(0);
                    // Прошлая фаза — **шагом вперёд, а не назад** (§12.100):
                    // назад тик уходит в минус на первых сутках партии, а
                    // расписание кольцевое, и «на длину цикла минус одна фаза
                    // вперёд» — это тот же индекс, что «на одну назад».
                    // Длина цикла своя у каждого предмета: `quote` берёт её из
                    // его же списка фаз.
                    let phases = rule
                        .price_of(*item)
                        .map_or(1, |line| line.phases.len().max(1));
                    // Ворота считает то же выражение, что и фасад: разойдись
                    // они, витрина показала бы живой кнопку, которую
                    // `Sim::trade` отклонит (инвариант 14).
                    let gates = crate::trade::offer_gates(factions, fame, standing, f, *item);
                    let prev_tick = tick + rule.period * (phases as u64 - 1);
                    out.push(PriceSnap {
                        faction: f,
                        item: *item,
                        buy: at(tick, true),
                        sell: at(tick, false),
                        next_buy: at(next_tick, true),
                        next_sell: at(next_tick, false),
                        next_in: ahead.unwrap_or(0),
                        prev_buy: at(prev_tick, true),
                        prev_sell: at(prev_tick, false),
                        unlocked: gates.as_ref().is_none_or(|g| g.unlocked),
                        welcome: gates.as_ref().is_none_or(|g| g.welcome),
                    });
                }
            }
            out
        };
        // Правила автопродажи: по одному на предмет, и покупатель — поле
        // правила (§12.88). Отдельным списком, а не полем в строке курса: строк
        // у предмета столько, сколько сторон им торгует, а правило одно.
        let sales: Vec<SaleSnap> = self
            .world
            .resource::<Selling>()
            .0
            .iter()
            .map(|&(item, dest, keep)| SaleSnap {
                item,
                // Адресат правила: сторона или разбор (§12.115). Наружу едет
                // тем же способом, каким лежит в снимке партии, — `None` и есть
                // «в мастерскую», а не «покупатель неизвестен».
                faction: match dest {
                    Surplus::Sell(faction) => Some(faction),
                    Surplus::Salvage => None,
                },
                keep,
            })
            .collect();
        // Закладки игрока (§12.100). Порядок держит ядро — оба списка
        // отсортированы по предмету, — потому что от него зависит порядок строк
        // на экране, а обход ECS недетерминирован (§11).
        let favorites: Vec<usize> = self.world.resource::<Favorites>().0.clone();
        let tickers: Vec<TickerSnap> = self
            .world
            .resource::<Tickers>()
            .0
            .iter()
            .map(|&(item, faction)| TickerSnap { item, faction })
            .collect();
        // Постов **сколько есть**, а свободен ли хоть один — считает ядро
        // (§12.26): «сделок меньше, чем постов» второй раз в JS однажды
        // разойдётся с `trade`, и игрок увидит кнопку, которую фасад отклонит.
        let posts = self.trade_posts() as i32;
        // Свободна ли ячейка — считает ядро тем же выражением, которым это
        // решит `trade` (§12.68): «сделок меньше, чем постов» в JS разошлось бы
        // с фасадом на первой же неубранной куче, и игрок увидел бы живую
        // кнопку, которую фасад отклоняет молча.
        let post_free = self.free_post_cell().is_some();
        // Открыта ли автоматика (§12.93) — тем же выражением, которым это решают
        // сами команды: три почти одинаковых поля напрашиваются на путаницу,
        // поэтому считает их одно место, общее со тестами (как `raid_gates`).
        let auto_open = self.auto_gates_open();
        // Предел одной сделки — контейнер той ячейки, которую займёт заявка
        // (§12.90). Считает ядро тем же выражением, что и `trade`: разойдись
        // они, и Shift на кнопке обещал бы объём, который фасад отклоняет.
        let post_lot = self.post_lot();
        // У раций наружу едет только счёт: с §12.152 они не держат ни слота, ни
        // состава, и поузловых ворот у них не осталось.
        let relays = self.relay_cells().len() as i32;
        // Дверь наружу — и она же слот, и она же отряд (§12.152). Без единого
        // гаража уйти некуда, и заявка отклоняется молча: причину обязано
        // назвать ядро (§12.53).
        let gates = gate_count(
            self.world.resource::<BaseMap>(),
            self.world.resource::<TileRules>(),
        ) as i32;
        // Что связь даёт прямо сейчас — сумма вкладов дошедших дежурных
        // (§12.152). Тем же `duty_gain`, каким копит `run_missions`.
        let comms_now: i32 = {
            let mut q = self
                .world
                .query_filtered::<(&OnDuty, Option<&Skills>), Without<Path>>();
            let map = self.world.resource::<BaseMap>();
            let tiles = self.world.resource::<TileRules>();
            let skills = self.world.resource::<SkillRules>();
            q.iter(&self.world)
                .map(|(d, sk)| crate::relay::duty_gain(tiles, map, skills, sk, d.spot))
                .sum()
        };
        // Отряды поимённо: с §12.152 у каждого гаража свой состав, и панель
        // обязана называть его словом — иначе кнопка вылазки берёт отряд ниоткуда.
        let nodes: Vec<NodeSnap> = {
            let cells = self.gate_cells_of();
            cells
                .into_iter()
                .map(|(x, y)| {
                    let forces = self.node_forces(x, y);
                    let (shares, fails) = self.node_outcomes(x, y);
                    // Почему правило стоит (§12.116). Два разных ответа, и
                    // считает их ядро теми же выражениями, что и сам
                    // `run_auto_raids`: «состав не в сборе» чинится временем,
                    // «людей меньше минимума» — решением игрока.
                    let auto = self.world.resource::<AutoRaids>().of(x, y);
                    let (auto_share, auto_fail) =
                        auto.map_or((0, false), |def| self.auto_forecast(x, y, def));
                    // Собран ли отряд целиком (§12.148). Ворота у кнопки и у
                    // правила одни, поэтому и поле одно — `auto_fit` разошёлся
                    // бы с кнопкой на первом же спящем.
                    let fit = self.squad_is_fit(x, y);
                    NodeSnap {
                        x,
                        y,
                        crew: self.roster_of(x, y),
                        ready: self.ready_roster_of(x, y),
                        spans: self.node_spans(x, y),
                        dangers: self.node_dangers(x, y),
                        force: forces.iter().sum(),
                        forces,
                        shares,
                        fails,
                        guide: self.node_guide(x, y),
                        busy: !self.node_is_free(x, y),
                        auto: auto.map_or(-1, |def| def as i32),
                        auto_on: self.world.resource::<AutoRaids>().is_on(x, y),
                        auto_share,
                        auto_fail,
                        fit,
                    }
                })
                .collect()
        };
        let mut recruits = Vec::new();
        {
            let rules = self.world.resource::<RecruitRules>().0.clone();
            let hired: Vec<String> = {
                let mut q = self.world.query::<&UnitId>();
                q.iter(&self.world).map(|u| u.0.clone()).collect()
            };
            for rule in &rules {
                recruits.push(RecruitSnap {
                    hired: hired.contains(&rule.id),
                    unlocked: fame >= rule.requires,
                    welcome: self.world.resource::<Standing>().covers(&rule.needs),
                    affordable: self.storage_covers(&rule.cost),
                });
            }
        }

        // Есть ли **сейчас на базе** кот с допуском каждой темы (§12.135).
        // Считается до обхода тем, потому что запрос по миру, а не по теме.
        let home_science: Vec<usize> = {
            let rules = self.world.resource::<ResearchRules>().0.clone();
            rules
                .iter()
                .enumerate()
                .filter(|(_, r)| self.has_scientist_home(r.level))
                .map(|(def, _)| def)
                .collect()
        };
        let mut research = Vec::new();
        {
            let mut q = self.world.query::<&Research>();
            let names: Vec<(Entity, String)> = {
                let mut cats = self.world.query::<(Entity, &UnitId)>();
                cats.iter(&self.world)
                    .map(|(e, u)| (e, u.0.clone()))
                    .collect()
            };
            let rules = self.world.resource::<ResearchRules>();
            for topic in q.iter(&self.world) {
                let need = topic_missing(rules, topic);
                research.push(ResearchSnap {
                    def: topic.def,
                    progress: topic.progress,
                    total: rules.0.get(topic.def).map_or(0, |r| r.work),
                    unit: topic
                        .assignee
                        .and_then(|e| names.iter().find(|&&(cat, _)| cat == e))
                        .map(|(_, id)| id.clone())
                        .unwrap_or_default(),
                    x: topic.cell.0,
                    y: topic.cell.1,
                    home: home_science.contains(&topic.def),
                    owed: need
                        .into_iter()
                        .map(|(item, left)| NeedSnap { item, left })
                        .collect(),
                });
            }
        }
        // Порядок — **по клетке** (§12.132): обход ECS зависит от истории
        // вставок (§11), и до §12.132 это скрывала единственность темы. Без
        // сортировки карточки тем переставлялись бы под курсором.
        research.sort_by_key(|r| (r.y, r.x));

        let techs = self.world.resource::<Techs>().0.clone();
        let has_lab = self.has_lab();
        // Свободная ячейка — **тем же выражением, что и заявка** (§12.132):
        // «есть лаборатория» и «есть свободная» это два разных отказа, и
        // второй экземпляр этой арифметики в JS однажды показал бы живую
        // кнопку, которую фасад отклоняет (инвариант 14).
        let lab_free = self.free_lab_cell().is_some();
        let running: Vec<usize> = {
            let mut q = self.world.query::<&Research>();
            q.iter(&self.world).map(|t| t.def).collect()
        };
        let mut topics = Vec::new();
        {
            let rules = self.world.resource::<ResearchRules>().0.clone();
            for (def, rule) in rules.iter().enumerate() {
                let known = self.world.resource::<Techs>().knows(&rule.id);
                let unlocked = self.world.resource::<Techs>().covers(&rule.requires);
                // Образец — ворота по **шкале `Seen`** (§12.131, §12.133), а не
                // по складу: тема открыта и ждёт, пока его привезут.
                let sighted = {
                    let seen = self.world.resource::<Seen>();
                    rule.specimen.iter().all(|&(item, _)| seen.saw(item))
                };
                topics.push(TopicSnap {
                    known,
                    unlocked,
                    affordable: self.storage_covers(&rule.cost),
                    staffed: self.has_scientist(rule.level),
                    lab: has_lab,
                    lab_free,
                    busy: running.contains(&def),
                    sighted,
                    // Тем же выражением, что и `affordable` (§12.139): образец
                    // берут со складской кучи, значит спрашивать надо склад.
                    stocked: self.storage_covers(&rule.specimen),
                    specimen: rule.specimen.iter().map(|&(item, _)| item).collect(),
                });
            }
        }

        let mut crafting = Vec::new();
        {
            let mut q = self.world.query::<&Craft>();
            let names: Vec<(Entity, String)> = {
                let mut cats = self.world.query::<(Entity, &UnitId)>();
                cats.iter(&self.world)
                    .map(|(e, u)| (e, u.0.clone()))
                    .collect()
            };
            let rules = self.world.resource::<CraftRules>();
            for order in q.iter(&self.world) {
                crafting.push(CraftSnap {
                    def: order.def,
                    left: order.left,
                    progress: order.progress,
                    total: rules.0.get(order.def).map_or(0, |r| r.work),
                    supplied: craft_supplied(rules, order),
                    delivered: order.delivered.iter().map(|&(_, n)| n).sum(),
                    need: rules.0.get(order.def).map_or(0, |r| {
                        r.cost.iter().map(|&(_, per)| per * order.left.max(0)).sum()
                    }) + order.delivered.iter().map(|&(_, n)| n).sum::<i32>(),
                    // Та же недостача, но по типам (§12.128): её читает число
                    // «расписано» в строке склада, и считается она тем же
                    // выражением, каким подвоз решает, что нести.
                    owed: craft_missing(rules, order)
                        .into_iter()
                        .map(|(item, left)| NeedSnap { item, left })
                        .collect(),
                    // Ячейка, в которой стоит заказ. С §12.96 она у него есть
                    // всегда — заказ рождается в станке, а не ищет его, — и
                    // заглушки `-1` больше нет. Едет наружу по той же причине,
                    // что и шлюз миссии: мастерских несколько, и панель клетки
                    // обязана отличать занятую от свободной, а с §12.96 ещё и
                    // держит единственную кнопку «Отменить» (§12.95).
                    x: order.cell.0,
                    y: order.cell.1,
                    auto: order.auto,
                    unit: order
                        .assignee
                        .and_then(|e| names.iter().find(|&&(cat, _)| cat == e))
                        .map(|(_, id)| id.clone())
                        .unwrap_or_default(),
                });
            }
        }
        // Порядок обхода сущностей ECS зависит от истории вставок и
        // недетерминирован (§11): без сортировки список заказов в панели
        // перетасовывался бы сам собой, а у мира из снимка порядок был бы свой.
        // Ключ — **клетка**, как у сделок (§12.81): рецепт заказы больше не
        // различает (§12.96), а закрывшийся сосед переставлял бы строки под
        // курсором игрока.
        crafting.sort_by_key(|c| (c.y, c.x));

        let shops = self.shops();
        let shop_spare = self.spare_shop_cell().is_some();
        let mut recipes = Vec::new();
        {
            let rules = self.world.resource::<CraftRules>().0.clone();
            let running: Vec<usize> = crafting.iter().map(|c| c.def).collect();
            for (def, rule) in rules.iter().enumerate() {
                recipes.push(RecipeSnap {
                    unlocked: self.world.resource::<Techs>().covers(&rule.requires),
                    // Есть ли куда поставить заказ: станок, свободный или
                    // отбираемый у правила (§12.97), **или** уже размеченный
                    // заказ на этот рецепт — тогда заявка добавит штук и станка
                    // не займёт (§12.55). Считает это `spare_shop_cell`, то же
                    // выражение, которым отвечает `start_craft`.
                    shop: shop_spare || running.contains(&def),
                    // Разбор ждать не умеет (§12.114d): сколько трофеев есть,
                    // столько и разберут. Второго экземпляра этой арифметики в
                    // виде быть не должно — он показал бы живой кнопку,
                    // которую фасад отклонит.
                    // Считается **у любого рецепта** (§12.129): ворота теперь
                    // общие, и кнопка «Произвести» гасится тем же числом, каким
                    // режется заявка.
                    room: self.craft_room(&rule.cost),
                });
            }
        }

        // Пороги автопроизводства — длиной ровно в палитру рецептов: ресурс
        // растёт под тот рецепт, которому порог задали, и короче палитры, пока
        // остальные нули (§12.65).
        let stocking = {
            let rule = self.world.resource::<Stocking>();
            (0..self.recipes.len()).map(|d| rule.min_of(d)).collect()
        };

        // Записка. Что игрок знает о будущем — решает ядро: до `reveal` детали
        // в снапшот просто не кладутся (§4.6, §12.28).
        let mut notes = Vec::new();
        {
            let rules = self.world.resource::<TimelineRules>().0.clone();
            for (def, rule) in rules.iter().enumerate() {
                let happened = self.world.resource::<Chronicle>().happened(def);
                let open = revealed(rule, tick);
                let ready = open && ready_for(rule, self.world.resource::<Techs>());
                notes.push(NoteSnap {
                    def,
                    label: self.timeline[def].label.clone(),
                    at: rule.at,
                    left: rule.at as i64 - tick as i64,
                    hint: self.timeline[def].hint.clone(),
                    revealed: open,
                    detail: if open {
                        self.timeline[def].detail.clone()
                    } else {
                        String::new()
                    },
                    requires: if open {
                        rule.requires.clone()
                    } else {
                        Vec::new()
                    },
                    ready,
                    done: happened.is_some(),
                    succeeded: happened.is_some_and(|h| h.ready),
                });
            }
        }

        // Парты по доменам (§12.84): сколько их всего и сколько свободно.
        // Считается здесь, а не в JS, по той же причине, что и `post_free`:
        // «свободна» значит «её не держит ничей `Study`», а занятость — знание
        // ядра. Панели это нужно ровно для того, чтобы отказ кнопки «Учить» был
        // назван словом, а не молчанием (§12.53).
        let desks: Vec<DeskSnap> = {
            let taken: Vec<(i32, i32)> = self.taken_desks();
            let map = self.world.resource::<BaseMap>();
            let tiles = self.world.resource::<TileRules>();
            let cells: Vec<Option<usize>> = (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .map(|(x, y)| tiles.teaches_of(map.tile_at(x, y)))
                .collect();
            let spots: Vec<((i32, i32), Option<usize>)> = (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .zip(cells)
                .collect();
            (0..self.world.resource::<SkillRules>().0.len())
                .map(|i| {
                    let mine = spots.iter().filter(|(_, s)| *s == Some(i));
                    DeskSnap {
                        total: mine.clone().count() as i32,
                        free: mine.filter(|(c, _)| !taken.contains(c)).count() as i32,
                    }
                })
                .collect()
        };

        let goals_required = self.world.resource::<GoalRules>().required();
        // Лента едет целиком: она ограничена `NEWS_MAX`, а какая новость ещё
        // свежая — решает вид по `tick` и `news` из `meta` (§12.120).
        let news: Vec<NewsSnap> = self
            .world
            .resource::<News>()
            .feed
            .iter()
            .map(|n| NewsSnap {
                kind: match n.kind {
                    NewsKind::Raid => "raid",
                    NewsKind::Recruit => "recruit",
                    NewsKind::Topic => "topic",
                    NewsKind::Recipe => "recipe",
                    NewsKind::Tile => "tile",
                    NewsKind::Item => "item",
                },
                def: n.def,
                opened: n.opened,
                at: n.at,
            })
            .collect();
        let goals = self.goals();

        serde_wasm_bindgen::to_value(&Snapshot {
            tick,
            entities,
            blueprints,
            stacks,
            stock,
            missions,
            raids,
            relays,
            gates,
            comms_now,
            nodes,
            desks,
            fame,
            standing,
            money,
            deals,
            prices,
            sales,
            favorites,
            tickers,
            posts,
            post_free,
            post_lot,
            shops: shops as i32,
            auto_sales: auto_open.0,
            auto_crafting: auto_open.1,
            auto_raids: auto_open.2,
            auto_tidy: self.world.resource::<AutoTidy>().0,
            recruits,
            research,
            topics,
            crafting,
            recipes,
            stocking,
            techs,
            notes,
            goals,
            goals_required,
            news,
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
