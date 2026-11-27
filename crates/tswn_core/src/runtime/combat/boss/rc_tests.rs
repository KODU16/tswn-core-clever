//! RC 的真实召唤、指定名字属性、专属行动与受击雷击回归。
use super::super::*;
use super::rc::SMALL_SOURCE;

fn battle() -> (RuntimeRunner, EntityIdx, EntityIdx) {
    let r = RuntimeRunner::from_custom_mixed_namerena_raw(
        "rc@!\n\nalpha\n\nseed:rc-test".to_owned(),
        default_custom_runtime_import_config().unwrap(),
    )
    .unwrap();
    let rc = r.runtime().entities.iter().find(|(_, e)| e.template.name == "rc").unwrap().0;
    let enemy = r.runtime().entities.iter().find(|(_, e)| e.template.name == "alpha").unwrap().0;
    (r, rc, enemy)
}

#[test]
fn rc_uses_boss_stats_low_attract_and_requested_icon() {
    let (r, rc, _) = battle();
    let e = r.runtime().entities.get(rc).unwrap();
    assert_eq!(
        [
            e.runtime.hp,
            e.runtime.attack,
            e.runtime.defense,
            e.runtime.speed,
            e.runtime.agility,
            e.runtime.magic,
            e.runtime.resistance,
            e.runtime.wisdom
        ],
        [360, 106, 106, 266, 106, 106, 106, 106]
    );
    assert!(e.runtime.attract() > 0.0 && e.runtime.attract() < 0.001);
    assert!(e.runtime.flags.contains(PlayerKindFlags::BOSS));
    assert!(e.template.skills.levels().iter().all(|&level| level == 0));
    assert_eq!(
        crate::namerena::icon_render::render_icon_vec_from_name("rc@!"),
        crate::namerena::icon_render::render_icon_vec_from_name("RbCl@RbCl")
    );
}

#[test]
fn rc_summons_one_to_three_with_both_skill_texts() {
    let (mut r, rc, _) = battle();
    let rt = r.runtime_mut();
    let mut counts = [false; 3];
    let mut texts = [false; 2];
    for _ in 0..64 {
        let before = rt.entities.len();
        let mut u = RunUpdates::new();
        rt.drain_rc_action_into(rc, &mut u);
        let count = rt.entities.len() - before;
        assert!((1..=3).contains(&count));
        counts[count - 1] = true;
        texts[0] |= u.updates.iter().any(|x| x.message == "[0]使用[有点小]");
        texts[1] |= u.updates.iter().any(|x| x.message == "[0]使用[这确实太小了]");
    }
    assert_eq!(counts, [true; 3]);
    assert_eq!(texts, [true; 2]);
}

#[test]
fn small_inherits_named_stats_with_half_hp_and_only_charms() {
    let (mut r, rc, enemy) = battle();
    let input = crate::namerena::NamerenaInput::parse(SMALL_SOURCE).unwrap();
    let source = crate::namerena::PreparedRoster::build(&input, crate::namerena::eval_name::DEFAULT_EVAL_RQ).unwrap();
    let s = &source.players[0].status;
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let small = rt.rc_spawn_small(rc, &mut u);
    let e = rt.entities.get(small).unwrap();
    assert_eq!([e.runtime.hp, e.template.max_hp], [s.max_hp / 2; 2]);
    assert_eq!(
        [
            e.runtime.attack,
            e.runtime.defense,
            e.runtime.speed,
            e.runtime.agility,
            e.runtime.magic,
            e.runtime.resistance,
            e.runtime.wisdom,
            e.runtime.magic_point
        ],
        [
            s.attack,
            s.defense,
            s.speed,
            s.agility,
            s.magic,
            s.resistance,
            s.wisdom,
            s.magic_point
        ]
    );
    assert_eq!(e.runtime.owner, rc);
    assert!(e.runtime.attract() >= 1.0e12);
    assert_eq!(e.template.skills.skills().len(), 1);
    let charm = rt.registry.skill_id_by_export_name(BuiltinActiveSkill::Charm.export_name()).unwrap();
    assert_eq!(e.template.skills.skills(), &[charm]);
    let before = rt.entities.get(enemy).unwrap().runtime.hp;
    let mut u = RunUpdates::new();
    rt.drain_rc_action_into(small, &mut u);
    assert!(u.updates.iter().any(|x| x.message.contains("魅惑")));
    assert_eq!(rt.entities.get(enemy).unwrap().runtime.hp, before);
    for actor in [rc, small] {
        assert!(matches!(
            rt.prepare_plain_action(actor, true, PlainSkillPreActionOutcome::default()),
            Some(PreparedPlainAction::BossSkill(PreparedBossSkillAction {
                selected: crate::namerena::BossActionSkill::Rc,
                ..
            }))
        ));
    }
}

#[test]
fn small_reacts_once_to_shielded_and_lethal_attacks_without_counter_loop() {
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let small = rt.rc_spawn_small(rc, &mut u);
    rt.entities.get_mut(small).unwrap().runtime.shield = 10000;
    // 模拟复活重建运行态后第一次受击，仍应正常反击。
    rt.entities.get_mut(small).unwrap().runtime.rosmontic = None;
    let mut u = RunUpdates::new();
    let mut v = RuntimeDefendValue::Damage {
        value: 0,
        caster: enemy,
        target: small,
        kind: DamageKind::Default,
    };
    rt.drain_post_defend_hooks_into(small, &mut u, &mut v);
    assert_eq!(u.updates.iter().filter(|x| x.message == "[0]使用[雷击术]").count(), 1);
    let other = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let mut u = RunUpdates::new();
    rt.rc_thunder_on_attack(small, other, &mut u);
    assert_eq!(u.updates.iter().filter(|x| x.message == "[0]使用[雷击术]").count(), 1);
    assert!(!rt.entities.get(small).unwrap().runtime.rosmontic.as_ref().unwrap().countering);
    rt.entities.get_mut(small).unwrap().runtime.shield = 0;
    rt.entities.get_mut(enemy).unwrap().runtime.hp = 100000;
    let mut u = RunUpdates::new();
    rt.drain_plain_attack_after_dodge_into(
        enemy,
        small,
        false,
        1.0e12,
        DamageKind::Default,
        None,
        PlainAttackOnDamage::None,
        &mut u,
    );
    assert_eq!(u.updates.iter().filter(|x| x.message == "[0]使用[雷击术]").count(), 1);
    assert_eq!(rt.entities.get(small).unwrap().runtime.hp, 0);
}

/// 快速续局、真实回放与同输入重建使用同一套召唤和反击随机流。
#[test]
fn rc_battles_match_captured_and_fast_execution() {
    for seed in [1, 19, 37] {
        let raw = format!("rc@!\n\nalpha\nbeta\ngamma\n\nseed:{seed}");
        let config = default_custom_runtime_import_config().unwrap();
        let mut captured = RuntimeRunner::from_custom_mixed_namerena_raw(raw.clone(), config.clone()).unwrap();
        let mut fast = captured.clone();
        let mut replay = RuntimeRunner::from_custom_mixed_namerena_raw(raw, config).unwrap();
        let a = captured.run_until_winner(20000);
        let b = fast.run_to_completion(20000);
        let c = replay.run_to_completion(20000);
        assert!(!a.guard_exhausted, "seed={seed}");
        assert_eq!(a.winner_team, b.winner_team);
        assert_eq!(b.winner_team, c.winner_team);
        assert_eq!(captured.runtime().entities, fast.runtime().entities);
        assert_eq!(fast.runtime().entities, replay.runtime().entities);
    }
}
