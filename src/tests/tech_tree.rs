//! Сторож дерева технологий (§12.134): **ни один потребитель предмета не
//! открывается раньше, чем предмет становится добываемым и понятым**.
//!
//! Ловит он не код, а **контент**: рецепт, у которого в цене вещество, ещё не
//! существующее в мире; тайл, чья цена ждёт материала со следующей ступени;
//! опечатку в `id` предмета, которую `Sim::new` глотает молча через
//! `filter_map`. Каждое из этого даёт не падение, а тихо другую игру — кнопку,
//! которой нечем воспользоваться, и ни слова о том, почему.
//!
//! Перебор законен ровно потому, что обе шкалы **только растут** (§12.18):
//! технология не забывается, известность не убывает, — значит порядок открытий
//! статичен и считается прямо из YAML, без единого тика.
//!
//! Разбираем мы **сам YAML**, а не мир `Sim::new`: сторожу нужны имена, а
//! конструктор схлопывает пропавшие `id` через `filter_map` — то есть глотает
//! как раз те опечатки, которые мы ловим.

use std::collections::{BTreeMap, BTreeSet};

use crate::ruleset::Ruleset;

const CORE: &str = include_str!("../../assets/rulesets/core.yaml");

fn shipped() -> Ruleset {
    serde_yaml::from_str(CORE).expect("рулсет читается")
}

/// Ступень технологии: `0` у темы без предисловий, иначе `1 + max(родителей)`.
///
/// Считается до неподвижной точки, а не рекурсией: цикл в дереве иначе
/// переполнил бы стек вместо того, чтобы назваться словом.
fn depths(rs: &Ruleset) -> BTreeMap<&str, u32> {
    let mut depth: BTreeMap<&str, u32> = BTreeMap::new();
    loop {
        let mut grew = false;
        for topic in &rs.research {
            let ready: Option<u32> = topic
                .requires
                .iter()
                .map(|r| depth.get(r.as_str()).copied())
                .try_fold(0, |acc, d| d.map(|d| acc.max(d + 1)));
            let Some(d) = ready else { continue };
            if depth.insert(topic.id.as_str(), d) != Some(d) {
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    depth
}

/// Ступень набора технологий: худшая из них, у пустого — ноль («сразу»).
/// `None` — набор недостижим вовсе: в нём цикл или несуществующее имя.
fn depth_of(depth: &BTreeMap<&str, u32>, techs: &[String]) -> Option<u32> {
    techs
        .iter()
        .map(|t| depth.get(t.as_str()).copied())
        .try_fold(0, |acc, d| d.map(|d| acc.max(d)))
}

/// То же для одиночного имени: пусто = «сразу».
fn depth_of_one(depth: &BTreeMap<&str, u32>, tech: &str) -> Option<u32> {
    match tech.is_empty() {
        true => Some(0),
        false => depth.get(tech).copied(),
    }
}

/// Ступень, на которой предмет **впервые появляется в мире**, и ступень, на
/// которой база его **понимает** (§12.131). Второе не раньше первого.
fn item_stages(rs: &Ruleset, depth: &BTreeMap<&str, u32>) -> BTreeMap<String, (u32, u32)> {
    let mut source: BTreeMap<String, u32> = BTreeMap::new();
    let mut note = |id: &str, at: u32| {
        let slot = source.entry(id.to_string()).or_insert(at);
        *slot = (*slot).min(at);
    };

    for s in &rs.stock {
        note(&s.item, 0); // стартовый склад — самое раннее, что бывает
    }
    for m in &rs.missions {
        for id in m.loot.keys() {
            note(id, 0); // вылазка технологиями не закрыта (§12.43)
        }
    }
    for f in &rs.factions {
        for id in f.prices.keys() {
            note(id, 0); // купить можно с первого тика
        }
    }
    for e in &rs.timeline {
        // Подарок приходит, только если база успела к технологиям события.
        if let Some(at) = depth_of(depth, &e.requires) {
            for id in e.gift.keys() {
                note(id, at);
            }
        }
    }
    for r in &rs.recipes {
        if let Some(at) = depth_of(depth, &r.requires) {
            for id in r.gives.keys() {
                note(id, at);
            }
        }
    }
    for t in &rs.research {
        if let Some(at) = depth.get(t.id.as_str()).copied() {
            for id in t.gives.keys() {
                note(id, at);
            }
        }
    }

    rs.items
        .iter()
        .filter_map(|it| {
            let born = source.get(&it.id).copied()?;
            let known = depth_of(depth, &it.requires)?;
            Some((it.id.clone(), (born, known.max(born))))
        })
        .collect()
}

/// Все места, где предмет **тратят**: `(кто, ступень, набор)`.
fn consumers(rs: &Ruleset, depth: &BTreeMap<&str, u32>) -> Vec<(String, u32, Vec<String>)> {
    let mut out: Vec<(String, u32, Vec<String>)> = Vec::new();
    for t in &rs.tiles {
        if let Some(at) = depth_of_one(depth, &t.tech) {
            out.push((
                format!("постройка «{}»", t.label),
                at,
                t.cost.keys().cloned().collect(),
            ));
        }
    }
    for r in &rs.recipes {
        if let Some(at) = depth_of(depth, &r.requires) {
            out.push((
                format!("рецепт «{}»", r.label),
                at,
                r.cost.keys().cloned().collect(),
            ));
        }
    }
    for t in &rs.research {
        if let Some(at) = depth.get(t.id.as_str()).copied() {
            let mut needs: Vec<String> = t.cost.keys().cloned().collect();
            needs.extend(t.specimen.keys().cloned());
            out.push((format!("тема «{}»", t.label), at, needs));
        }
    }
    for r in &rs.recruits {
        // Найм технологиями не закрыт вовсе (§12.43) — только известностью.
        out.push((
            format!("найм «{}»", r.label),
            0,
            r.cost.keys().cloned().collect(),
        ));
    }
    // Шаблон снаряжения: ступень у него своя у каждого предмета — та, на которой
    // вещь становится понятной (§12.114).
    for id in &rs.loadout {
        let at = rs
            .items
            .iter()
            .find(|it| &it.id == id)
            .and_then(|it| depth_of(depth, &it.requires))
            .unwrap_or(0);
        out.push((format!("шаблон снаряжения («{id}»)"), at, vec![id.clone()]));
    }
    out
}

/// Главное правило: потребитель не открывается раньше своего предмета.
#[test]
fn the_shipped_ruleset_never_asks_for_an_item_it_cannot_have() {
    let rs = shipped();
    let depth = depths(&rs);
    let stages = item_stages(&rs, &depth);

    let mut sins: Vec<String> = Vec::new();
    for (who, at, needs) in consumers(&rs, &depth) {
        for id in needs {
            let Some(&(born, known)) = stages.get(&id) else {
                continue; // предмета без источника ловит соседний тест
            };
            if at < known {
                let what = match born > at {
                    true => format!("появляется в мире только на {born}-й"),
                    false => format!("становится понятным только на {known}-й"),
                };
                sins.push(format!(
                    "{who} открывается на {at}-й ступени науки, а «{id}» {what}: \
                     игрок получит кнопку, которой нечем воспользоваться, и \
                     причины этого нигде не написано",
                ));
            }
        }
    }

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

/// У предмета, который где-то тратят, обязан быть источник: рецепт, добыча,
/// подарок, прайс, стартовый склад или выход темы.
#[test]
fn the_shipped_ruleset_gives_every_item_a_source() {
    let rs = shipped();
    let depth = depths(&rs);
    let stages = item_stages(&rs, &depth);

    let mut sins: Vec<String> = Vec::new();
    for (who, _, needs) in consumers(&rs, &depth) {
        for id in needs {
            if !stages.contains_key(&id) {
                sins.push(format!(
                    "«{id}» стоит в цене у {who}, но взяться в мире ему неоткуда: \
                     ни рецепта, ни добычи, ни подарка, ни прайса, ни стартового склада",
                ));
            }
        }
    }

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

/// Дерево без циклов и без ссылок в никуда.
///
/// Бесплатный побочный улов: сегодня такую ссылку не ловит ничто — `Sim::new`
/// её просто не заметит, и тема останется недостижимой навсегда.
#[test]
fn the_shipped_ruleset_has_a_reachable_tech_tree() {
    let rs = shipped();
    let depth = depths(&rs);

    let unreachable: Vec<&str> = rs
        .research
        .iter()
        .filter(|t| !depth.contains_key(t.id.as_str()))
        .map(|t| t.id.as_str())
        .collect();

    assert!(
        unreachable.is_empty(),
        "до этих тем не добраться — цикл в `requires` или ссылка на \
         несуществующую технологию: {unreachable:?}",
    );
}

/// Все `id` предметов, названные контентом, существуют в палитре.
///
/// `Sim::new` теряет такую строку молча (`filter_map`), и «предмет без
/// потребителей» оказывается ложным «всё в порядке».
#[test]
fn the_shipped_ruleset_names_only_items_it_has() {
    let rs = shipped();
    let known: BTreeSet<&str> = rs.items.iter().map(|it| it.id.as_str()).collect();

    let mut sins: Vec<String> = Vec::new();
    let mut check = |where_: &str, ids: Vec<&String>| {
        for id in ids {
            if !known.contains(id.as_str()) {
                sins.push(format!(
                    "{where_} называет предмет «{id}», которого нет в `items:`"
                ));
            }
        }
    };
    for t in &rs.tiles {
        check(&format!("постройка «{}»", t.label), t.cost.keys().collect());
    }
    for r in &rs.recipes {
        check(
            &format!("рецепт «{}» (вход)", r.label),
            r.cost.keys().collect(),
        );
        check(
            &format!("рецепт «{}» (выход)", r.label),
            r.gives.keys().collect(),
        );
    }
    for t in &rs.research {
        check(
            &format!("тема «{}» (цена)", t.label),
            t.cost.keys().collect(),
        );
        check(
            &format!("тема «{}» (образец)", t.label),
            t.specimen.keys().collect(),
        );
        check(
            &format!("тема «{}» (выход)", t.label),
            t.gives.keys().collect(),
        );
    }
    for m in &rs.missions {
        check(&format!("вылазка «{}»", m.label), m.loot.keys().collect());
    }
    for r in &rs.recruits {
        check(&format!("кандидат «{}»", r.label), r.cost.keys().collect());
    }
    for e in &rs.timeline {
        check(&format!("событие «{}»", e.label), e.gift.keys().collect());
    }
    for f in &rs.factions {
        check(&format!("прайс «{}»", f.label), f.prices.keys().collect());
    }
    check(
        "стартовый склад",
        rs.stock.iter().map(|s| &s.item).collect(),
    );
    check("шаблон снаряжения", rs.loadout.iter().collect());

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

/// Вторая шкала — **известность**, и мерить её тем же алгоритмом нельзя: они
/// несравнимы (§12.134). Поэтому отдельная проверка: у предмета, который
/// тратят на нулевой ступени науки, есть источник, не закрытый ни технологией,
/// ни порогом известности.
///
/// Иначе дерево технологий останется формально верным, а игрок упрётся в
/// известность — и причина будет ещё дальше от кнопки.
#[test]
fn the_shipped_ruleset_opens_early_items_without_fame() {
    let rs = shipped();
    let depth = depths(&rs);

    // Что доступно совсем сразу: стартовый склад, прайсы и вылазки без порога.
    let mut free: BTreeSet<&str> = BTreeSet::new();
    for s in &rs.stock {
        free.insert(s.item.as_str());
    }
    for f in &rs.factions {
        for id in f.prices.keys() {
            free.insert(id.as_str());
        }
    }
    for m in rs.missions.iter().filter(|m| m.requires == 0) {
        for id in m.loot.keys() {
            free.insert(id.as_str());
        }
    }
    for r in rs.recipes.iter().filter(|r| r.requires.is_empty()) {
        for id in r.gives.keys() {
            free.insert(id.as_str());
        }
    }

    let mut sins: Vec<String> = Vec::new();
    for (who, at, needs) in consumers(&rs, &depth) {
        if at > 0 {
            continue;
        }
        for id in needs {
            if !free.contains(id.as_str()) {
                sins.push(format!(
                    "{who} доступен с первого тика, а «{id}» до него ещё надо \
                     дорасти известностью: отказ окажется дальше от кнопки, чем \
                     его причина",
                ));
            }
        }
    }

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

// --- обработка артефактов (§12.138) ------------------------------------------
//
// Общий принцип: артефакт приезжает на базу → его **вскрывают** в лаборатории →
// и этим же вскрытием открывается его **разбор**. Отдельной темы «научиться
// разбирать» не бывает: вскрытие и есть тот момент, когда база увидела, как
// вещь разобрана.
//
// Правило живёт в **контенте**, а не в ядре: рецепт разбора со своей ценой,
// выходом и временем — это контент, и порождать его кодом значило бы решать за
// балансировщика (та же граница, по которой `opensOf` считается перекличкой
// палитр, а не полем рулсета). Держит правило сторож.

/// Технология разбора — это технология вскрытия того же предмета.
#[test]
fn the_shipped_ruleset_lets_you_take_apart_what_you_opened() {
    let rs = shipped();

    // Сторож, которому нечего сверять, зелен и бесполезен — та же оговорка, что
    // у `a_saved_game_continues_identically` (см. CLAUDE.md).
    assert!(
        rs.recipes.iter().any(|r| r.salvage),
        "в рулсете нет ни одного разбора — сверять нечего",
    );

    let shop = shop_tech(&rs);
    let mut sins: Vec<String> = Vec::new();
    for r in rs.recipes.iter().filter(|r| r.salvage) {
        for input in r.cost.keys() {
            let opener = rs.research.iter().find(|t| t.specimen.contains_key(input));
            let Some(opener) = opener else {
                sins.push(format!(
                    "«{}» разбирает «{input}», но вскрывать его негде: темы с \
                     таким образцом нет, и умение разбирать берётся ниоткуда",
                    r.label,
                ));
                continue;
            };
            // Ворот у разбора **ровно двое**: тема-вскрытие («умеем») и
            // мастерская («есть где»). Вскрытие сверяем поимённо, мастерскую
            // пропускаем целым списком — она стоит у каждого рецепта и
            // сторожится своим тестом ниже. Третье имя здесь — лишняя
            // ступень между «разложили в лаборатории» и «умеем разобрать»,
            // то есть отказ, который игроку не объяснить.
            let extra: Vec<&String> = r
                .requires
                .iter()
                .filter(|t| **t != opener.id && Some(t.as_str()) != shop.as_deref())
                .collect();
            if !r.requires.contains(&opener.id) || !extra.is_empty() {
                sins.push(format!(
                    "«{}» закрыт {:?}, а вскрывает «{input}» тема «{}»: база \
                     разложила вещь в лаборатории и всё ещё «не умеет её \
                     разобрать» — отказ, который игроку не объяснить",
                    r.label, r.requires, opener.label,
                ));
            }
        }
    }

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

/// Технология, открывающая **мастерскую**: её ищем по свойству тайла, а не по
/// имени. Имя `workshops` живёт в контенте, и вшитое в тест оно молча
/// разошлось бы с рулсетом на первом же переименовании.
fn shop_tech(rs: &Ruleset) -> Option<String> {
    rs.tiles
        .iter()
        .find(|t| t.shop && !t.tech.is_empty())
        .map(|t| t.tech.clone())
}

/// **Ни один рецепт не открывается раньше мастерской.**
///
/// Заказ живёт в ячейке станка (§12.96), и разбор — тот же заказ наоборот
/// (§12.114), значит место работы у всех рецептов одно. Рецепт, открытый
/// раньше станка, даёт в окне «Склад» кнопку, которой негде сработать: причина
/// у неё названа словом («нет мастерской», §12.94), но игрок услышит её от
/// механики, о которой узнал секунду назад. Знание «как» и место «где» — двое
/// ворот, и оба обязаны быть названы.
///
/// Считается по **замыканию** требований, а не по первому списку: рецепт,
/// закрытый темой, которая сама стоит за мастерской, правило уже соблюдает —
/// второе имя рядом было бы копипастой (так живёт «Производство аптечки»).
#[test]
fn the_shipped_ruleset_never_crafts_without_a_workshop() {
    let rs = shipped();
    let shop = shop_tech(&rs).expect("в рулсете есть мастерская со своей технологией");

    // Замыкание требований темы: до неподвижной точки, как `depths`, — цикл в
    // дереве иначе переполнил бы стек вместо того, чтобы назваться словом.
    let mut closure: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    loop {
        let mut grew = false;
        for topic in &rs.research {
            let mut set: BTreeSet<&str> = BTreeSet::new();
            for r in &topic.requires {
                set.insert(r.as_str());
                if let Some(inner) = closure.get(r.as_str()) {
                    set.extend(inner.iter().copied());
                }
            }
            if closure.insert(topic.id.as_str(), set.clone()) != Some(set) {
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    let sins: Vec<String> = rs
        .recipes
        .iter()
        .filter(|r| {
            !r.requires.iter().any(|t| {
                t == &shop
                    || closure
                        .get(t.as_str())
                        .is_some_and(|c| c.contains(shop.as_str()))
            })
        })
        .map(|r| {
            format!(
                "рецепт «{}» закрыт {:?}, а мастерская — «{shop}»: кнопка \
                 «{}» загорится раньше станка, на котором ей работать",
                r.label,
                r.requires,
                if r.salvage {
                    "Разобрать"
                } else {
                    "Произвести"
                },
            )
        })
        .collect();

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}

/// Три шкалы времени про одну вещь: **узнать** дороже, чем **сделать**, а
/// **сделать** дороже, чем **разобрать**.
///
/// Ломать быстрее, чем строить, — интуиция, которую нарушать нельзя молча; а
/// вскрытие, которое дешевле разового изготовления, превращает науку в
/// быстрейший способ получить вещь.
#[test]
fn the_shipped_ruleset_keeps_its_three_time_scales_apart() {
    let rs = shipped();

    let makes = |item: &str| {
        rs.recipes
            .iter()
            .filter(|r| !r.salvage && r.gives.contains_key(item))
            .map(|r| (r.label.as_str(), r.work))
            .collect::<Vec<_>>()
    };

    // Пар, которые сторож и правда сравнивает: разбор против производства и
    // тема против производства. Ноль пар — тест зелен ни о чём.
    let pairs = rs
        .recipes
        .iter()
        .filter(|r| r.salvage)
        .flat_map(|r| r.cost.keys())
        .chain(rs.research.iter().flat_map(|t| t.specimen.keys()))
        .filter(|item| !makes(item).is_empty())
        .count();
    assert!(
        pairs > 0,
        "ни одну вещь нельзя и сделать, и разобрать/вскрыть"
    );

    let mut sins: Vec<String> = Vec::new();
    for r in rs.recipes.iter().filter(|r| r.salvage) {
        for input in r.cost.keys() {
            for (label, work) in makes(input) {
                if r.work >= work {
                    sins.push(format!(
                        "«{}» ({}) не быстрее, чем «{label}» ({work}): разобрать \
                         вещь дольше, чем сделать, — этому нет объяснения",
                        r.label, r.work,
                    ));
                }
            }
        }
    }
    for t in &rs.research {
        for item in t.specimen.keys() {
            for (label, work) in makes(item) {
                if t.work <= work {
                    sins.push(format!(
                        "тема «{}» ({}) не дороже, чем «{label}» ({work}): узнать \
                         вещь быстрее, чем сделать её, — тогда наука становится \
                         кратчайшим путём к предмету",
                        t.label, t.work,
                    ));
                }
            }
        }
    }

    assert!(sins.is_empty(), "{}", sins.join("\n"));
}
