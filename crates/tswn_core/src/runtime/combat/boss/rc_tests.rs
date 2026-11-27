//! RC 技能、永久观察、成长、伤害后反雷与真实回放回归。
use super::super::*;

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

/// 所有属性断言均使用玩家可见口径。
fn visible(rt: &CombatRuntime, id: EntityIdx) -> [i32; 7] {
    let r = &rt.entities.get(id).unwrap().runtime;
    [
        r.attack + 36,
        r.defense + 36,
        r.speed - 124,
        r.agility + 36,
        r.magic + 36,
        r.resistance + 36,
        r.wisdom + 36,
    ]
}

fn thunder_count(u: &RunUpdates) -> usize { u.updates.iter().filter(|x| x.message == "[0]使用[雷击术]").count() }

/// 从两只起手完整抽卡，能够超过旧上限；达到三只后两种召唤都不可用。
#[test]
fn rc_draw_ranges_and_precast_threshold_replace_hard_cap() {
    for paid in [false, true] {
        let mut counts = [false; 3];
        for seed in 0..64 {
            let (mut r, rc, _) = battle();
            let rt = r.runtime_mut();
            rt.rng = crate::rc4::RC4::new(format!("draw-{seed}").as_bytes(), 1);
            let mut u = RunUpdates::new();
            rt.rc_spawn_small(rc, &mut u);
            rt.rc_spawn_small(rc, &mut u);
            u = RunUpdates::new();
            rt.rc_draw(rc, paid, &mut u);
            let added = rt.rc_small_count(rc) - 2;
            let min = if paid { 3 } else { 1 };
            assert!((min..=min + 2).contains(&added));
            counts[added - min] = true;
            assert!(u.updates.iter().any(|x| x.message.contains(if paid { "648" } else { "抽卡" })));
            let len = rt.entities.len();
            rt.rc_draw(rc, false, &mut u);
            rt.rc_draw(rc, true, &mut u);
            assert_eq!(rt.entities.len(), len);
        }
        assert_eq!(counts, [true; 3]);
    }
}

#[test]
fn small_has_72_hp_visible_36_stats_and_only_charms() {
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let small = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let e = rt.entities.get(small).unwrap();
    assert_eq!([e.runtime.hp, e.template.max_hp], [72; 2]);
    assert_eq!(visible(rt, small), [36; 7]);
    assert_eq!(e.runtime.owner, rc);
    assert!(e.runtime.attract() >= 1.0e12);
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

/// 反雷要求实际扣血且存活，RC 本体（包括敌方 RC）被明确排除。
#[test]
fn small_retaliates_only_after_surviving_positive_damage_and_never_against_rc() {
    for (amount, expected) in [(0, 0), (1, 1), (72, 0), (100, 0)] {
        let (mut r, rc, enemy) = battle();
        let rt = r.runtime_mut();
        let small = rt.rc_spawn_small(rc, &mut RunUpdates::new());
        let mut u = RunUpdates::new();
        rt.apply_plain_attack_damage_with_covid_and_on_damage_into(enemy, small, amount, None, PlainAttackOnDamage::None, &mut u);
        assert_eq!(thunder_count(&u), expected, "damage={amount}");
    }
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let small = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    rt.entities.get_mut(small).unwrap().runtime.shield = 10000;
    let mut u = RunUpdates::new();
    rt.drain_plain_attack_after_dodge_into(
        enemy,
        small,
        false,
        100.0,
        DamageKind::Default,
        None,
        PlainAttackOnDamage::None,
        &mut u,
    );
    assert_eq!(rt.entities.get(small).unwrap().runtime.hp, 72);
    assert_eq!(thunder_count(&u), 0);
    rt.entities.get_mut(small).unwrap().runtime.shield = 0;
    // 模拟状态重建，第一次有效受伤仍能恢复反雷标记。
    rt.entities.get_mut(small).unwrap().runtime.rosmontic = None;
    let mut u = RunUpdates::new();
    rt.apply_plain_attack_damage_with_covid_and_on_damage_into(rc, small, 1, None, PlainAttackOnDamage::None, &mut u);
    assert_eq!(thunder_count(&u), 0);
    let mut u = RunUpdates::new();
    rt.apply_plain_attack_damage_with_covid_and_on_damage_into(enemy, small, 1, None, PlainAttackOnDamage::None, &mut u);
    assert_eq!(thunder_count(&u), 1);
    assert!(!rt.ros_state(small).countering);
}

#[test]
fn small_counter_chain_does_not_recurse() {
    let (mut r, rc, _) = battle();
    let rt = r.runtime_mut();
    let a = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let b = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let mut u = RunUpdates::new();
    rt.rc_thunder_on_attack(a, b, &mut u);
    assert_eq!(thunder_count(&u), 1);
    assert!(!rt.ros_state(a).countering);
}

/// 观察层数随快照保留，驱散、净化均不减少；临时增益刷新不叠乘。
#[test]
fn rc_observation_stacks_survive_cleanse_and_buff_expires() {
    let (mut r, rc, _) = battle();
    let rt = r.runtime_mut();
    let a = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let b = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let base = visible(rt, rc);
    let mut u = RunUpdates::new();
    rt.rc_observe(rc, a, &mut u);
    assert!(u.updates.iter().any(|x| matches!(x.message.as_ref(), "这确实太小了" | "有点小")));
    assert_eq!(visible(rt, rc), base.map(|v| (f64::from(v) * 1.2) as i32));
    rt.rc_observe(rc, b, &mut u);
    assert_eq!(visible(rt, rc), base.map(|v| (f64::from(v) * 1.2) as i32));
    rt.end_rosmontic_action(rc, &mut u);
    assert_eq!(rt.ros_state(rc).rc_boost_turns, 2);
    rt.end_rosmontic_action(rc, &mut u);
    assert_eq!(rt.ros_state(rc).rc_boost_turns, 1);
    rt.end_rosmontic_action(rc, &mut u);
    assert_eq!(visible(rt, rc), base);
    rt.dispel_rosmontic_into(a, &mut u);
    rt.ros_cleanse(a);
    rt.apply_disperse_hit_into(rc, a, &mut u);
    assert_eq!(rt.ros_state(a).rc_observations, 1);
    let clone = rt.clone();
    assert_eq!(clone.entities, rt.entities);
    assert_eq!(
        r.player_snapshot(a.0 as usize)
            .unwrap()
            .status_labels
            .iter()
            .find(|x| x.contains("观察次数"))
            .unwrap(),
        "观察次数 ×1"
    );
}

/// 死亡、零血和不属于本体的对象不能被观察；没有目标时不产生层数或台词。
#[test]
fn rc_observe_rejects_dead_zero_hp_and_foreign_targets() {
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let a = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let b = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let c = rt.rc_spawn_small(enemy, &mut RunUpdates::new());
    rt.entities.get_mut(a).unwrap().runtime.alive = false;
    rt.entities.get_mut(b).unwrap().runtime.hp = 0;
    let mut u = RunUpdates::new();
    for id in [a, b, c, enemy] {
        rt.rc_observe(rc, id, &mut u);
    }
    assert!(u.updates.is_empty());
    assert!(rt.rc_children(rc).is_empty());
    for id in [a, b, c] {
        assert_eq!(rt.ros_state(id).rc_observations, 0);
    }
}

#[test]
fn rc_second_observation_transforms_hp_stats_and_big_action_sequence() {
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let a = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    rt.entities.get_mut(a).unwrap().runtime.hp = 25;
    let mut u = RunUpdates::new();
    rt.rc_observe(rc, a, &mut u);
    rt.rc_observe(rc, a, &mut u);
    assert!(rt.is_rc_big(a));
    assert!(!rt.is_rc_small(a));
    assert_eq!(rt.rc_small_count(rc), 0);
    assert_eq!(rt.entities.get(a).unwrap().template.max_hp, 144);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 50);
    assert_eq!(visible(rt, a), [72; 7]);
    rt.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
    assert_eq!(visible(rt, a), [72; 7]);
    assert_eq!(rt.ros_state(a).rc_observations, 2);
    assert!(
        u.updates
            .iter()
            .any(|x| matches!(x.message.as_ref(), "还是大了" | "我觉得大了" | "其实我不太欣赏的来小"))
    );
    let mut u = RunUpdates::new();
    rt.apply_plain_attack_damage_with_covid_and_on_damage_into(enemy, a, 1, None, PlainAttackOnDamage::None, &mut u);
    assert_eq!(thunder_count(&u), 0);
    // 留足靶子 HP，独立验证两个完整的蓄力／雷击周期。
    rt.entities.get_mut(enemy).unwrap().runtime.hp = 100000;
    for charge in [true, false, true, false] {
        let mut u = RunUpdates::new();
        rt.drain_rc_action_into(a, &mut u);
        assert_eq!(u.updates.iter().any(|x| x.message.contains("开始[蓄力]")), charge);
        assert_eq!(thunder_count(&u), usize::from(!charge));
    }
}

#[test]
fn observing_big_gives_rc_native_berserk_for_two_actions() {
    let (mut r, rc, _) = battle();
    let rt = r.runtime_mut();
    let a = rt.rc_spawn_small(rc, &mut RunUpdates::new());
    let mut u = RunUpdates::new();
    for _ in 0..3 {
        rt.rc_observe(rc, a, &mut u);
    }
    assert_eq!(rt.ros_state(a).rc_observations, 3);
    assert!(rt.has_plain_berserk_state(rc));
    assert!(!rt.has_plain_berserk_state(a));
    assert!(u.updates.iter().any(|x| x.message == "一个两个的怎么都是大雷，我愤怒了"));
    assert!(matches!(
        rt.prepare_plain_action(rc, true, PlainSkillPreActionOutcome::default()),
        Some(PreparedPlainAction::ForcedAttack { .. })
    ));
    rt.drain_plain_berserk_forced_action_state_into(rc, &mut u);
    assert!(rt.has_plain_berserk_state(rc));
    rt.drain_plain_berserk_forced_action_state_into(rc, &mut u);
    assert!(!rt.has_plain_berserk_state(rc));
}

#[test]
fn rc_heals_lowest_current_hp_small_only_and_can_draw_after_growth_or_death() {
    let (mut r, rc, enemy) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let a = rt.rc_spawn_small(rc, &mut u);
    let b = rt.rc_spawn_small(rc, &mut u);
    let c = rt.rc_spawn_small(rc, &mut u);
    rt.entities.get_mut(a).unwrap().runtime.hp = 30;
    rt.entities.get_mut(b).unwrap().runtime.hp = 10;
    rt.rc_observe(rc, c, &mut u);
    rt.rc_observe(rc, c, &mut u);
    rt.entities.get_mut(c).unwrap().runtime.hp = 1;
    rt.rc_heal_small(rc, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 30);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 72);
    assert_eq!(rt.entities.get(c).unwrap().runtime.hp, 1);
    // 魅惑换队仍计原归属，成长为大则释放小的召唤资格。
    rt.entities.get_mut(a).unwrap().runtime.team = rt.entities.get(enemy).unwrap().runtime.team;
    assert_eq!(rt.rc_small_count(rc), 2);
    rt.rc_draw(rc, true, &mut u);
    assert!(rt.rc_small_count(rc) >= 5);
    let ids = rt.rc_children(rc);
    for id in ids {
        if rt.is_rc_small(id) {
            rt.entities.get_mut(id).unwrap().runtime.hp = 0;
        }
    }
    assert_eq!(rt.rc_small_count(rc), 0);
    rt.rc_draw(rc, false, &mut u);
    assert!((1..=3).contains(&rt.rc_small_count(rc)));
}

/// 火球、净化的独立伤害入口遵守相同条件，纯毒伤和 RC 攻击不触发。
#[test]
fn rc_small_fire_disperse_and_poison_damage_follow_retaliation_rules() {
    for kind in 0..3 {
        for (amount, expected) in [(0, 0), (1, usize::from(kind != 2)), (72, 0)] {
            for from_rc in [false, true] {
                let (mut r, rc, enemy) = battle();
                let rt = r.runtime_mut();
                let small = rt.rc_spawn_small(rc, &mut RunUpdates::new());
                let caster = if from_rc { rc } else { enemy };
                let mut u = RunUpdates::new();
                match kind {
                    0 => {
                        rt.apply_plain_legacy_damage_with_fire_into(caster, small, amount, Some(12345), &mut u);
                    }
                    1 => {
                        rt.apply_disperse_attack_damage_into(caster, small, amount, &mut u);
                    }
                    _ => {
                        rt.apply_poison_tick_damage_into(caster, small, amount, &mut u);
                    }
                }
                assert_eq!(
                    thunder_count(&u),
                    if from_rc { 0 } else { expected },
                    "kind={kind}, damage={amount}, rc={from_rc}"
                );
            }
        }
    }
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
