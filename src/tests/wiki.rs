//! Сторож справочника (§12.204): **ключ статьи обязан существовать в рулсете, а
//! ссылка — вести в написанную статью**.
//!
//! Ловит он не код, а контент, и цена ошибки тут тихая. Опечатка в ключе
//! (`tile:strorage`) не роняет ничего: значок «?» просто не появляется у
//! склада, и заметить это можно, лишь пересчитав значки глазами. Битая ссылка
//! молча превращается в обычное слово — статья ссылается в пустоту, и ни игрок,
//! ни автор об этом не узнают.
//!
//! ⚠️ Разбираем **сам YAML**, а не мир `Sim::new`, по доводу `tech_tree`
//! (§12.134): конструктор схлопывает пропавшие `id` через `filter_map`, то есть
//! глотает как раз те опечатки, которые сторож и ловит.
//!
//! ⚠️ **Список файлов здесь второй** — первый живёт в `wiki.js`, — и это
//! безопасно ровно потому, что рядом стоит проверка полноты: забудь подключить
//! файл здесь, и `every_palette_entry_has_an_article` назовёт поимённо всё, что
//! в нём написано. Обратное (файл есть в тесте, нет в игре) поймать нечем, и
//! это единственная дыра, оставленная сознательно: `include_str!` требует
//! статичных путей, а каталог из теста не читается.

use std::collections::BTreeSet;

use crate::ruleset::Ruleset;

const CORE: &str = include_str!("../../assets/rulesets/core.yaml");

/// Разделы справочника — по одному на файл, дословно `FILES` в `wiki.js`.
const WIKI: &[&str] = &[
    include_str!("../../assets/wiki/lore.md"),
    include_str!("../../assets/wiki/tiles.md"),
    include_str!("../../assets/wiki/items.md"),
    include_str!("../../assets/wiki/research.md"),
    include_str!("../../assets/wiki/recipes.md"),
    include_str!("../../assets/wiki/missions.md"),
    include_str!("../../assets/wiki/recruits.md"),
    include_str!("../../assets/wiki/factions.md"),
    include_str!("../../assets/wiki/rules.md"),
    include_str!("../../assets/wiki/skills.md"),
];

/// Правила игрока (§12.93). Палитра у них не в рулсете, а в `automation:`, и
/// ключи те же, что у `AutoRules::gates`.
const RULE_KEYS: [&str; 3] = ["sales", "crafting", "raids"];

fn shipped() -> Ruleset {
    serde_yaml::from_str(CORE).expect("рулсет читается")
}

/// Ключи всех написанных статей — `вид:id`.
fn keys() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in WIKI {
        for line in file.lines() {
            if let Some(key) = article_key(line) {
                out.insert(key);
            }
        }
    }
    out
}

/// Заголовок статьи: `## вид:id | Название`. Название необязательно.
fn article_key(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## ")?;
    let key = rest.split('|').next()?.trim();
    let (kind, id) = key.split_once(':')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
    };
    (ok(kind) && ok(id)).then(|| key.to_string())
}

/// Все ссылки `[[вид:id]]` / `[[вид:id|слово]]` с именем файла для внятного
/// отчёта: «битая ссылка» без адреса ищется потом руками по десяти файлам.
fn links() -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, file) in WIKI.iter().enumerate() {
        let mut rest = *file;
        while let Some(at) = rest.find("[[") {
            rest = &rest[at + 2..];
            let Some(end) = rest.find("]]") else { break };
            let body = &rest[..end];
            rest = &rest[end + 2..];
            let target = body.split('|').next().unwrap_or("").trim();
            if target.contains(':') {
                out.push((n, target.to_string()));
            }
        }
    }
    out
}

/// Что вообще бывает записью справочника: вид → все `id` его палитры.
/// `lore` палитры не имеет — это статьи о мире, ни к чему не привязанные.
fn palettes(rs: &Ruleset) -> Vec<(&'static str, Vec<String>)> {
    let ids = |xs: Vec<&String>| xs.into_iter().cloned().collect::<Vec<_>>();
    vec![
        // Внутренность объекта (`internal`, §12.163) своей статьи не получает:
        // игрок её не ставит и отдельно не видит, о ней рассказывает объект.
        (
            "tile",
            ids(rs
                .tiles
                .iter()
                .filter(|x| !x.internal)
                .map(|x| &x.id)
                .collect()),
        ),
        (
            "structure",
            ids(rs.structures.iter().map(|x| &x.id).collect()),
        ),
        ("item", ids(rs.items.iter().map(|x| &x.id).collect())),
        ("topic", ids(rs.research.iter().map(|x| &x.id).collect())),
        ("recipe", ids(rs.recipes.iter().map(|x| &x.id).collect())),
        ("raid", ids(rs.missions.iter().map(|x| &x.id).collect())),
        ("recruit", ids(rs.recruits.iter().map(|x| &x.id).collect())),
        ("faction", ids(rs.factions.iter().map(|x| &x.id).collect())),
        ("skill", ids(rs.skills.iter().map(|x| &x.id).collect())),
        ("stat", ids(rs.stats.iter().map(|x| &x.id).collect())),
        ("perk", ids(rs.perks.iter().map(|x| &x.id).collect())),
        ("rule", RULE_KEYS.iter().map(|s| (*s).to_string()).collect()),
    ]
}

/// Ключ статьи ведёт в существующую запись рулсета.
///
/// Ошибка тут тихая вдвойне: значок «?» не появляется, а статья остаётся
/// висеть в оглавлении — то есть выглядит написанной и при этом недостижима
/// оттуда, где о ней спросили бы.
#[test]
fn every_article_key_names_a_real_entry() {
    let rs = shipped();
    let pal = palettes(&rs);
    for key in keys() {
        let (kind, id) = key.split_once(':').expect("ключ вида «вид:id»");
        if kind == "lore" {
            continue;
        }
        let Some((_, ids)) = pal.iter().find(|(k, _)| *k == kind) else {
            panic!("статья «{key}»: вида «{kind}» в игре нет");
        };
        assert!(
            ids.iter().any(|x| x == id),
            "статья «{key}»: в палитре «{kind}» такого id нет"
        );
    }
}

/// Ссылка ведёт в написанную статью. Битая молча становится обычным словом —
/// то есть обещание перехода исчезает, и заметить это можно только тыкая.
#[test]
fn every_link_points_at_a_written_article() {
    let all = keys();
    for (file, target) in links() {
        assert!(
            all.contains(&target),
            "файл №{file}: ссылка на «{target}», а статьи с таким ключом нет"
        );
    }
}

/// У каждой записи боевых палитр есть статья.
///
/// Это не педантизм: значок «?» стоит **только там, где статья есть**
/// (§12.204), и запись без неё выглядит для игрока ровно так же, как запись,
/// о которой игре нечего сказать. Разница между «не написали» и «нечего
/// сказать» видна отсюда и больше ниоткуда.
///
/// Он же ловит забытый в `WIKI` файл: пропал раздел — весь его список встанет
/// здесь поимённо.
#[test]
fn every_palette_entry_has_an_article() {
    let rs = shipped();
    let all = keys();
    let mut missing = Vec::new();
    for (kind, ids) in palettes(&rs) {
        for id in ids {
            let key = format!("{kind}:{id}");
            if !all.contains(&key) {
                missing.push(key);
            }
        }
    }
    assert!(missing.is_empty(), "без статьи остались: {missing:?}");
}

/// Заголовок статьи не бывает пустым: без него окно печатает `id`, то есть
/// «sample_lore» вместо «Свойства образца», — и это выглядит поломкой, а не
/// умолчанием. Умолчание тут законно только у записей палитры, где имя даст
/// сам рулсет.
#[test]
fn a_lore_article_always_names_itself() {
    for file in WIKI {
        for line in file.lines() {
            let Some(key) = article_key(line) else {
                continue;
            };
            if !key.starts_with("lore:") {
                continue;
            }
            assert!(
                line.contains('|') && line.split('|').nth(1).is_some_and(|t| !t.trim().is_empty()),
                "статья «{key}» о мире: у неё нет заголовка, а взять его негде"
            );
        }
    }
}

/// Ключ статьи встречается один раз на весь справочник. Повтор не роняет
/// ничего: разборщик в `wiki.js` молча оставляет последнюю статью, и первая —
/// вместе с её короткой версией до маркера `---понято---` — пропадает. Так
/// «Анализатор» однажды открылся полным текстом до изучения своей темы.
#[test]
fn every_article_key_is_written_once() {
    let mut seen = BTreeSet::new();
    let twice: Vec<String> = WIKI
        .iter()
        .flat_map(|file| file.lines())
        .filter_map(article_key)
        .filter(|key| !seen.insert(key.clone()))
        .collect();
    assert!(twice.is_empty(), "статьи с повторным ключом: {twice:?}");
}
