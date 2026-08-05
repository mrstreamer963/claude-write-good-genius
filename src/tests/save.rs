//! Сохранение партии (§12.45): сторож полноты, переигровка после загрузки и
//! ремап ссылок на сущности.

use std::collections::BTreeSet;

use bevy_ecs::prelude::Entity;

use crate::components::*;
use crate::map::BaseMap;
use crate::save::{FORMAT, SAVED, SKIPPED, short_name};
use crate::sim::Sim;

const CORE: &str = include_str!("../../assets/rulesets/core.yaml");

/// Мир словами — всё, что видно снаружи, без ссылок на сущности.
///
/// Номера сущностей сравнивать нельзя намеренно: `bevy_ecs` переиспользует
/// освободившиеся индексы, поэтому у живого мира и у загруженного они разойдутся
/// заведомо, ничего при этом не говоря о состоянии. Ссылки проверяет отдельный
/// тест (`claims_survive_a_save`), а этот отвечает на другой вопрос — тот же ли
/// это мир и так же ли он дальше живёт.
fn state_of(sim: &Sim) -> String {
    let w = &sim.world;
    let mut out = vec![
        format!("tick={}", w.resource::<SimTime>().tick),
        format!("map={:?}", w.resource::<BaseMap>().cells),
        format!("fame={}", w.resource::<Fame>().0),
        format!("money={}", w.resource::<Money>().0),
        format!("standing={:?}", w.resource::<Standing>().0),
        format!("techs={:?}", w.resource::<Techs>().0),
        format!(
            "chronicle={:?}",
            w.resource::<Chronicle>()
                .0
                .iter()
                .map(|h| (h.def, h.ready))
                .collect::<Vec<_>>()
        ),
    ];

    let mut lines: Vec<String> = Vec::new();
    for e in w.iter_entities() {
        let mut p: Vec<String> = Vec::new();
        if let Some(u) = e.get::<UnitId>() {
            p.push(format!("cat={}", u.0));
        }
        if let Some(pos) = e.get::<Position>() {
            p.push(format!("at={},{}", pos.x, pos.y));
        }
        if let Some(c) = e.get::<Energy>() {
            p.push(format!("energy={}", c.0));
        }
        if let Some(c) = e.get::<Fed>() {
            p.push(format!("fed={}", c.0));
        }
        if let Some(c) = e.get::<Health>() {
            p.push(format!("health={}", c.0));
        }
        if let Some(s) = e.get::<Skills>() {
            p.push(format!("xp={:?}", s.xp));
        }
        if let Some(g) = e.get::<Gear>() {
            p.push(format!("gear={:?}", g.0));
        }
        if let Some(c) = e.get::<Carrying>() {
            p.push(format!("carrying={},{}", c.item, c.count));
        }
        if let Some(path) = e.get::<Path>() {
            p.push(format!("path={:?}", path.steps));
        }
        if let Some(o) = e.get::<Order>() {
            p.push(format!("order={},{}", o.x, o.y));
        }
        if let Some(s) = e.get::<Stack>() {
            p.push(format!("stack={},{}", s.item, s.count));
        }
        if e.contains::<ToStore>() {
            p.push("tostore".into());
        }
        if let Some(b) = e.get::<Blueprint>() {
            p.push(format!(
                "bp={},{},{},{},{:?}",
                b.x, b.y, b.tile, b.progress, b.delivered
            ));
        }
        if let Some(r) = e.get::<Research>() {
            p.push(format!("topic={},{}", r.def, r.progress));
        }
        if let Some(c) = e.get::<Craft>() {
            p.push(format!(
                "craft={},{},{},{}",
                c.def, c.left, c.progress, c.paid
            ));
        }
        if let Some(d) = e.get::<Deal>() {
            p.push(format!(
                "deal={},{},{},{},{},{}",
                d.faction, d.item, d.count, d.unit, d.left, d.delivered
            ));
        }
        if let Some(m) = e.get::<Mission>() {
            p.push(format!("mission={},{:?},{}", m.def, m.gate, m.left));
        }
        // Вид задачи, но не её адресат: «строит» переживает загрузку, а номер
        // чертежа — нет и не должен.
        for (has, name) in [
            (e.contains::<Assignment>(), "job:build"),
            (e.contains::<Haul>(), "job:haul"),
            (e.contains::<Rest>(), "job:rest"),
            (e.contains::<Study>(), "job:study"),
            (e.contains::<Researching>(), "job:research"),
            (e.contains::<Crafting>(), "job:craft"),
            (e.contains::<Equipping>(), "job:equip"),
            (e.contains::<Eating>(), "job:eat"),
            (e.contains::<Healing>(), "job:heal"),
            (e.contains::<Treating>(), "job:treat"),
            (e.contains::<Squad>(), "job:squad"),
            (e.contains::<Away>(), "away"),
            (e.contains::<Captive>(), "captive"),
        ] {
            if has {
                p.push(name.into());
            }
        }
        if !p.is_empty() {
            lines.push(p.join(" "));
        }
    }
    // Сортировка, а не порядок обхода: тот зависит от истории вставок и у двух
    // миров совпасть не обязан (§11).
    lines.sort();
    out.extend(lines);
    out.join("\n")
}

/// Сторож снимка: каждый тип мира либо сохраняется, либо пропущен осознанно.
///
/// Забытый компонент — самая тихая поломка этого формата: мир после загрузки
/// просто немного другой, и не падает ничего. Ловится он здесь, потому что
/// `bevy_ecs` держит реестр: компоненты и ресурсы лежат в нём вместе
/// (`Components::register_resource` пишет в тот же список), а системы
/// регистрируют всё, что запрашивают, при инициализации расписания.
///
/// Проверка **двусторонняя**, и это возможно потому, что после пары тиков в
/// реестре оказывается весь мир целиком: расписание инициализирует системы, а
/// те регистрируют каждый запрошенный тип, даже если такой сущности сейчас
/// нет. Вторая сторона ловит мёртвую запись — тип, переименованный или
/// удалённый из `components.rs`, но забытый в списке.
#[test]
fn every_component_is_either_saved_or_skipped() {
    let mut sim = Sim::new(CORE).expect("рулсет");
    sim.tick_n(2);

    let listed: BTreeSet<&str> = SAVED
        .iter()
        .copied()
        .chain(SKIPPED.iter().map(|(name, _)| *name))
        .collect();

    let registered: BTreeSet<&str> = sim
        .world
        .components()
        .iter()
        .map(|info| info.name())
        .filter(|name| name.starts_with("sp_sim::"))
        .map(short_name)
        .collect();

    let unlisted: Vec<&&str> = registered.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "в мире появились типы, которых нет ни в SAVED, ни в SKIPPED: {unlisted:?}.\n\
         Допиши их в `src/save.rs`: сохраняется — в SAVED, не сохраняется — в SKIPPED \
         с причиной. Иначе снимок молча потеряет их, и мир после загрузки будет другим."
    );

    let dead: Vec<&&str> = listed.difference(&registered).collect();
    assert!(
        dead.is_empty(),
        "в списках `src/save.rs` перечислены типы, которых в мире нет: {dead:?}.\n\
         Похоже, их переименовали или удалили — убери и из списка."
    );
}

/// Списки не пересекаются: тип не может одновременно сохраняться и не
/// сохраняться, а пересечение означало бы, что одну из двух записей забыли
/// убрать.
#[test]
fn the_two_lists_do_not_overlap() {
    let saved: BTreeSet<&str> = SAVED.iter().copied().collect();
    assert_eq!(saved.len(), SAVED.len(), "дубль в SAVED");

    let skipped: BTreeSet<&str> = SKIPPED.iter().map(|(name, _)| *name).collect();
    assert_eq!(skipped.len(), SKIPPED.len(), "дубль в SKIPPED");

    let both: Vec<&&str> = saved.intersection(&skipped).collect();
    assert!(both.is_empty(), "тип и сохраняется, и пропущен: {both:?}");
}

/// Загруженная партия — тот же мир, и дальше он живёт так же.
///
/// Сверка идёт **на каждом тике**, а не в конце: конечное состояние маскирует
/// расхождение — коты досыпают, кучи доезжают, и разошедшиеся миры сходятся
/// обратно. Тот же приём, что в `demolish_job_is_done_from_a_neighbour`.
#[test]
fn a_saved_game_continues_identically() {
    let mut live = Sim::new(CORE).expect("рулсет");
    let sample = 2; // индекс `sample` в палитре предметов
    let bed = 4; // индекс `bed` в палитре тайлов: технологией не закрыт

    // Мир должен успеть обрасти всем, что бывает в партии: снимок, снятый с
    // пустой базы, проверяет только карту и трёх котов.
    live.put_item(4, 3, sample, 10);
    assert!(live.teach("excellent", "science"), "ученик за партой");
    live.tick_n(400);
    assert!(live.start_research(0), "тема взята: наука в работе");
    assert!(
        live.add_blueprint_rect(9, 6, 2, 2, bed),
        "и стройка размечена"
    );
    // Первая вылазка забирает двоих: у базы остаётся один кот за партой, а
    // ушедшие дают `Away`, `Squad` и саму миссию — то, чего на базе не увидеть.
    assert!(
        live.launch(0, vec!["sp2".to_string(), "sp3".to_string()]),
        "и отряд ушёл на вылазку",
    );
    live.tick_n(200);

    // Тест зелен только если сравнивать есть что: без этого он проверяет
    // равенство двух почти пустых строк (наступали, см. CLAUDE.md).
    let before = state_of(&live);
    for mark in ["cat=excellent", "topic=", "bp=", "job:", "away"] {
        assert!(
            before.contains(mark),
            "в мире для сверки нет «{mark}», сверять нечего:\n{before}",
        );
    }

    let json = live.save().expect("снимок");
    let mut loaded = Sim::load_from(CORE, &json).expect("загрузка");
    assert_eq!(
        state_of(&loaded),
        before,
        "загруженный мир отличается сразу"
    );

    for step in 1..=300 {
        live.tick();
        loaded.tick();
        assert_eq!(
            state_of(&loaded),
            state_of(&live),
            "миры разошлись на {step}-м тике после загрузки"
        );
    }
}

/// Ссылки на сущности переживают загрузку: claim остаётся за тем же котом.
///
/// В снимке `Entity` не сохранишь — это индекс с поколением, у нового мира они
/// свои. Ссылки едут номерами в списке сущностей и раскладываются обратно вторым
/// проходом, и вот это здесь и проверяется: не «ссылка есть», а «ведёт туда же».
/// Промах ремапа не уронил бы ничего — площадка просто осталась бы за чужим
/// котом, а тема за несуществующим.
#[test]
fn claims_survive_a_save() {
    let mut live = Sim::new(CORE).expect("рулсет");
    let sample = 2;
    let bed = 4;

    live.put_item(4, 3, sample, 10);
    assert!(live.teach("excellent", "science"));
    live.tick_n(800); // доучился и встал из-за парты
    assert!(live.start_research(0));
    live.tick_n(60); // и сел за тему — теперь она за ним
    assert!(live.add_blueprint_rect(9, 6, 2, 2, bed));
    assert!(live.launch(0, vec!["sp2".to_string(), "sp3".to_string()]));
    live.tick_n(120);

    let before = links_of(&live);
    // Сверять есть что: без claim'ов тест сравнил бы два пустых списка.
    assert!(
        before.iter().any(|l| l.starts_with("squad"))
            && before.iter().any(|l| l.starts_with("topic")),
        "в мире нет claim'ов для сверки: {before:?}",
    );

    let json = live.save().expect("снимок");
    let loaded = Sim::load_from(CORE, &json).expect("загрузка");
    assert_eq!(links_of(&loaded), before, "claim'ы после загрузки поехали");
}

/// Все ссылки мира, разрешённые до устойчивых имён: кот — по `UnitId`,
/// площадка — по клетке, тема и миссия — по индексу в рулсете. Номера сущностей
/// сюда не попадают намеренно: они у двух миров разные по определению.
fn links_of(sim: &Sim) -> Vec<String> {
    let w = &sim.world;
    let name = |e: Entity| {
        w.get::<UnitId>(e)
            .map(|u| u.0.clone())
            .unwrap_or_else(|| "?".to_string())
    };
    let spot = |e: Entity| {
        w.get::<Blueprint>(e)
            .map(|b| format!("{},{}", b.x, b.y))
            .unwrap_or_else(|| "?".to_string())
    };

    let mut out: Vec<String> = Vec::new();
    for e in w.iter_entities() {
        let who = e.get::<UnitId>().map(|u| u.0.clone());
        if let (Some(who), Some(a)) = (&who, e.get::<Assignment>()) {
            out.push(format!("build {who} -> {}", spot(a.0)));
        }
        if let (Some(who), Some(h)) = (&who, e.get::<Haul>()) {
            let to = match h.to {
                HaulTo::Site(t) => format!("site {}", spot(t)),
                HaulTo::Sale(t) => {
                    format!("sale {}", w.get::<Deal>(t).map_or(-1, |d| d.item as i32))
                }
                HaulTo::Store(t) => format!("store {}", t.map_or("-".into(), name)),
            };
            out.push(format!("haul {who} -> {to}"));
            // Наводка — тоже ссылка на сущность, и она обязана пережить круг
            // снимка (§12.49): по ней раздатчик считает обещанное.
            if let Some(aim) = h.aim {
                let at = w
                    .get::<Position>(aim.pile)
                    .map(|p| format!("{},{}", p.x, p.y))
                    .unwrap_or_else(|| "?".to_string());
                out.push(format!("aim {who} -> {at} {}", aim.item));
            }
        }
        if let (Some(who), Some(r)) = (&who, e.get::<Researching>()) {
            out.push(format!(
                "topic {who} -> {}",
                w.get::<Research>(r.0).map_or(-1, |t| t.def as i32)
            ));
        }
        if let (Some(who), Some(c)) = (&who, e.get::<Crafting>()) {
            out.push(format!(
                "craft {who} -> {}",
                w.get::<Craft>(c.0).map_or(-1, |t| t.def as i32)
            ));
        }
        if let (Some(who), Some(s)) = (&who, e.get::<Squad>()) {
            out.push(format!(
                "squad {who} -> {}",
                w.get::<Mission>(s.0).map_or(-1, |m| m.def as i32)
            ));
        }
        if let (Some(who), Some(t)) = (&who, e.get::<Treating>()) {
            out.push(format!("treat {who} -> {}", name(t.0)));
        }
        if let (Some(who), Some(h)) = (&who, e.get::<Healing>()) {
            if let Some(medic) = h.medic {
                out.push(format!("medic {who} <- {}", name(medic)));
            }
        }
        if let (Some(who), Some(q)) = (&who, e.get::<Equipping>()) {
            out.push(format!(
                "equip {who} -> {}",
                w.get::<Stack>(q.pile).map_or(-1, |s| s.item as i32)
            ));
        }
        if let (Some(who), Some(q)) = (&who, e.get::<Eating>()) {
            out.push(format!(
                "eat {who} -> {}",
                w.get::<Stack>(q.pile).map_or(-1, |s| s.item as i32)
            ));
        }
        if let (Some(who), Some(s)) = (&who, e.get::<Study>()) {
            out.push(format!("study {who} -> {},{:?}", s.skill, s.spot));
        }
        if let Some(b) = e.get::<Blueprint>() {
            if let Some(a) = b.assignee {
                out.push(format!("site {},{} <- {}", b.x, b.y, name(a)));
            }
        }
        if let Some(r) = e.get::<Research>() {
            if let Some(a) = r.assignee {
                out.push(format!("topic {} <- {}", r.def, name(a)));
            }
        }
        if let Some(c) = e.get::<Craft>() {
            if let Some(a) = c.assignee {
                out.push(format!("order {} <- {}", c.def, name(a)));
            }
        }
    }
    out.sort();
    out
}

/// Снимок с другого рулсета не грузится, и отказ явный.
///
/// Индексы палитр лежат в снимке числами, а имена — только в YAML: без этой
/// проверки правка контента дала бы не ошибку, а тихо другой мир — кот с чужим
/// навыком, цена из чужого тайла. Отказ здесь честнее молчания.
#[test]
fn a_save_from_another_ruleset_is_refused() {
    let mut live = Sim::new(CORE).expect("рулсет");
    live.tick_n(50);
    let json = live.save().expect("снимок");

    // Правка контента, какой она и бывает: цифра в прайсе.
    let modded = CORE.replacen("squad: 2", "squad: 3", 1);
    assert_ne!(modded, CORE, "рулсет для проверки должен отличаться");

    assert!(
        Sim::load_from(&modded, &json).is_err(),
        "снимок загрузился в мир с другими правилами",
    );
    assert!(Sim::load_from(CORE, &json).is_ok(), "а на своём — грузится");
}

/// Снимок чужой версии формата не грузится.
///
/// `FORMAT` поднимают руками при каждой правке DTO, и это единственное, что
/// стоит между «схема разъехалась» и молча другим миром (см. комментарий у
/// самой константы). Проверяем, что отказ вообще работает.
#[test]
fn a_save_from_another_format_is_refused() {
    let mut live = Sim::new(CORE).expect("рулсет");
    live.tick_n(20);
    let json = live.save().expect("снимок");

    let older = json.replacen(&format!("\"version\":{FORMAT}"), "\"version\":0", 1);
    assert_ne!(older, json, "версия должна была найтись в файле");

    assert!(
        Sim::load_from(CORE, &older).is_err(),
        "снимок чужого формата загрузился",
    );
    assert!(Sim::load_from(CORE, &json).is_ok(), "а свой — грузится");
}

/// Журнал помнит, что делал игрок, и переживает сохранение.
///
/// Партию он не восстанавливает (её восстанавливает снимок), поэтому проверяем
/// ровно его назначение: команды на месте и после загрузки никуда не делись.
#[test]
fn the_trace_remembers_what_the_player_did() {
    let mut live = Sim::new(CORE).expect("рулсет");
    let bed = 4;
    live.tick_n(10);
    live.add_blueprint_rect(9, 6, 2, 2, bed);
    live.set_auto_tidy(false);
    live.teach("excellent", "science");

    let trace = live.trace();
    for mark in [
        "build_rect 9 6 2 2 4",
        "auto_tidy false",
        "teach excellent science",
    ] {
        assert!(trace.contains(mark), "в журнале нет «{mark}»:\n{trace}");
    }

    let json = live.save().expect("снимок");
    let loaded = Sim::load_from(CORE, &json).expect("загрузка");
    assert_eq!(loaded.trace(), trace, "журнал не пережил сохранение");
}

/// У каждого пропуска есть причина. Список без причин через полгода
/// неотличим от списка забытого.
#[test]
fn every_skip_has_a_reason() {
    for (name, reason) in SKIPPED {
        assert!(!reason.trim().is_empty(), "пропуск без причины: {name}");
    }
}
