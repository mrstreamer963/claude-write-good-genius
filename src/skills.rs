//! Навыки: рост от работы (§12.17 concept.md).
//!
//! Навык — домен работы, а не действие игрока: стройка и снос это один навык,
//! потому что и джоб у них один. Растёт навык от самой работы, поэтому набор
//! навыков задаётся рулсетом, а не полями компонента — иначе каждый новый
//! домен (наука, §12.18) лез бы в компонент, снапшот и UI.
//!
//! Начисление живёт здесь одной системой: система работы только вешает маркер
//! `Worked`. Правило роста, потолок и кривая не расползаются по работам.

use bevy_ecs::prelude::*;

use crate::components::*;

/// Домен «Стройка» — он же снос.
pub(crate) const SKILL_BUILD: &str = "build";

/// Домен «Вылазка»: растёт за каждый тик в поле и прибавляет отряду силы
/// (§12.23). Второй домен работы — тот самый триггер 19b, после которого
/// коты перестают быть взаимозаменяемыми.
pub(crate) const SKILL_RAID: &str = "raid";

/// Очков опыта за тик работы. Опыт капает за тик, а не за готовый тайл: так он
/// не зависит от того, кто доделал чужой чертёж, и растёт ровно по правилу
/// «чем больше кот что-то делает, тем лучше умеет».
const XP_PER_TICK: i32 = 1;

/// Уровень навыка кота. Навыков у кота может не быть вовсе (тесты чужих
/// механик, коты из ASCII-схем) — это нулевой уровень, а не ошибка.
pub(crate) fn level_of(rules: &SkillRules, skills: Option<&Skills>, skill: usize) -> i32 {
    rules.level(skill, skills.map_or(0, |s| s.xp_of(skill)))
}

/// Превращает маркеры «работал в этом тике» в опыт и снимает их.
///
/// Стоит в конце цепочки, а не сразу за `work_jobs`: маркер успевает поставить
/// любая система работы, в том числе будущая.
pub(crate) fn train_skills(
    rules: Res<SkillRules>,
    mut commands: Commands,
    mut cats: Query<(Entity, &Worked, Option<&mut Skills>)>,
) {
    for (cat_e, worked, skills) in &mut cats {
        // Нет порогов — расти нечему: домена нет в рулсете либо он без уровней.
        let cap = rules.xp_cap(worked.0);
        match skills {
            Some(mut skills) => skills.add_xp(worked.0, XP_PER_TICK, cap),
            None => {
                let mut fresh = Skills::default();
                fresh.add_xp(worked.0, XP_PER_TICK, cap);
                commands.entity(cat_e).insert(fresh);
            }
        }
        commands.entity(cat_e).remove::<Worked>();
    }
}
