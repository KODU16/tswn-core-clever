//! 涵虚技能通过实际战斗实体验证，覆盖状态重算与伤害边界。
/// 真正跑完战斗，确认快速续局、回放和重建使用相同随机流。
#[test]
fn rv_battles_replay_and_fast_execution_agree() {
    for seed in [1, 19, 37] {
        let raw = format!("ReturnVoid@!\n\nalpha\nbeta\ngamma\n\nseed:{seed}");
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
/// 两个涵虚同时反伤时只反射一次；致死仍能消耗护符。
#[test]
fn rv_reflection_chain_stops_and_charms_save_lethal_hits() {
    let raw = "ReturnVoid@!\n\nReturnVoid@!\n\nseed:double-rv".to_owned();
    let mut r = RuntimeRunner::from_custom_mixed_namerena_raw(raw, default_custom_runtime_import_config().unwrap()).unwrap();
    let rt = r.runtime_mut();
    let ids: Vec<_> = rt.entities.iter().map(|(i, _)| i).collect();
    let (a, b) = (ids[0], ids[1]);
    let mut u = RunUpdates::new();
    rt.ros_state(a).rv_reflect = true;
    rt.ros_state(b).rv_reflect = true;
    rt.apply_plain_legacy_damage_into(a, b, 10, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 340);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 350);
    rt.ros_state(a).rv_charms = 1;
    rt.entities.get_mut(a).unwrap().runtime.hp = 2;
    if rt.apply_plain_legacy_damage_into(b, a, 100, &mut u) {
        rt.drain_plain_lethal_damage_into(b, a, &mut u);
    }
    assert!(rt.entities.get(a).unwrap().runtime.hp > 0);
    assert!(rt.entities.get(a).unwrap().runtime.alive);
    assert_eq!(rt.ros_state(a).rv_charms, 0);
}

use super::super::*;
fn battle() -> (RuntimeRunner, EntityIdx, EntityIdx) {
    let r = RuntimeRunner::from_custom_mixed_namerena_raw(
        "ReturnVoid@!\n\nalpha\n\nseed:rv".to_owned(),
        default_custom_runtime_import_config().unwrap(),
    )
    .unwrap();
    let a = r.runtime().entities.iter().find(|(_, e)| e.template.name == "ReturnVoid").unwrap().0;
    let b = r.runtime().entities.iter().find(|(_, e)| e.template.name == "alpha").unwrap().0;
    (r, a, b)
}
/// 身份、初始面向与指定图标在真实解析路径生效。
#[test]
fn rv_identity_icon_and_stats() {
    let (r, a, _) = battle();
    let e = r.runtime().entities.get(a).unwrap();
    assert_eq!(e.template.display_name, "涵虚");
    assert_eq!([e.runtime.hp, e.runtime.attack, e.runtime.speed], [360, 106, 266]);
    assert!(!e.runtime.rosmontic.as_ref().unwrap().rv_back);
    assert_eq!(
        crate::namerena::icon_render::render_icon_vec_from_name("ReturnVoid@!"),
        crate::namerena::icon_render::render_icon_vec_from_name("涵虚@涵虚")
    );
}
/// 受伤累计跨治疗保留；永久加速参与实际调度。
#[test]
fn rv_loss_layers_and_refresh() {
    let (mut r, a, b) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.apply_plain_legacy_damage_into(b, a, 35, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 106);
    rt.apply_plain_legacy_damage_into(b, a, 1, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 111);
    assert_eq!(rt.entities.get(a).unwrap().effective_speed(), 271);
    rt.entities.get_mut(a).unwrap().runtime.hp = 360;
    rt.apply_plain_legacy_damage_into(b, a, 36, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 116);
    rt.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 116);
}
/// 首次不等式按 100% 发放，后续衰减到 20%，交换不触发反伤。
#[test]
fn rv_swap_sp_decay_and_floor() {
    let (mut r, a, b) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    for (times, expected) in [(0, 10000), (1, 9500), (80, 2000)] {
        rt.entities.get_mut(a).unwrap().runtime.hp = 300;
        rt.entities.get_mut(b).unwrap().runtime.hp = 200;
        rt.entities.get_mut(a).unwrap().runtime.move_state.speed_points = 0;
        rt.ros_state(a).rv_inequalities = times;
        rt.rv_skill(a, 0, &mut u);
        assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 200);
        assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 300);
        assert_eq!(rt.entities.get(a).unwrap().runtime.move_state.speed_points, expected);
    }
}
/// 低血目标减伤、实际损血吸血和反伤递归阻断可同时工作。
#[test]
fn rv_damage_lifesteal_reflection() {
    let (mut r, a, b) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.entities.get_mut(b).unwrap().runtime.hp = 40;
    rt.apply_plain_legacy_damage_into(a, b, 100, &mut u);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 15);
    rt.entities.get_mut(a).unwrap().runtime.hp = 36;
    rt.entities.get_mut(b).unwrap().runtime.hp = 100;
    rt.apply_plain_legacy_damage_into(a, b, 20, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 52);
    rt.entities.get_mut(a).unwrap().runtime.hp = 100;
    rt.entities.get_mut(b).unwrap().runtime.hp = 100;
    rt.ros_state(a).rv_reflect = true;
    rt.apply_plain_legacy_damage_into(b, a, 10, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 90);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 80);
    rt.begin_return_void_action(a, &mut u);
    assert!(!rt.ros_state(a).rv_reflect);
}
/// 翻面增益不相互叠加；主动自损按当前生命的一半扣除并获得护符。
#[test]
fn rv_faces_charms_and_self_loss() {
    let (mut r, a, _) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.rv_skill(a, 2, &mut u);
    assert!(rt.ros_state(a).rv_back);
    assert_eq!(rt.entities.get(a).unwrap().runtime.magic, 136);
    rt.rv_skill(a, 2, &mut u);
    assert!(!rt.ros_state(a).rv_back);
    assert_eq!(rt.entities.get(a).unwrap().runtime.magic, 106);
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 136);
    rt.entities.get_mut(a).unwrap().runtime.hp = 200;
    rt.rv_skill(a, 5, &mut u);
    // 普攻会触发低血吸血，所以最终生命不少于自损后的 100。
    assert!(rt.entities.get(a).unwrap().runtime.hp >= 100);
    assert_eq!(rt.ros_state(a).rv_loss, 100);
    assert_eq!(rt.ros_state(a).rv_charms, 4);
    rt.entities.get_mut(a).unwrap().runtime.hp = 0;
    assert!(rt.rv_reraise(a, &mut u));
    assert_eq!(rt.ros_state(a).rv_charms, 3);
    assert!(rt.entities.get(a).unwrap().runtime.hp > 0);
}
/// 临时属性按自身回合到期，净化的伤害与背刺在持续期内均被回避。
#[test]
fn rv_timed_buffs_and_immunity() {
    let (mut r, a, b) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.entities.get_mut(a).unwrap().runtime.hp = 60;
    rt.rv_skill(a, 1, &mut u);
    assert_eq!(rt.ros_state(a).rv_mode, 1);
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 159);
    for _ in 0..4 {
        rt.end_return_void_action(a);
    }
    assert_eq!(rt.entities.get(a).unwrap().runtime.attack, 106);
    rt.rv_skill(a, 4, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.agility, 156);
    assert!(rt.status_immune(a, "assassinate"));
    rt.apply_disperse_attack_damage_into(b, a, 40, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 60);
    for _ in 0..6 {
        rt.end_return_void_action(a);
    }
    assert!(!rt.rv_immune(a));
    assert_eq!(rt.entities.get(a).unwrap().runtime.agility, 106);
}
/// 菜无普攻，存活三次本体后续行动后按实际剩余生命投掷。
#[test]
fn rv_vegetable_countdown_and_snapshot() {
    let (mut r, a, b) = battle();
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let child = rt.rv_spawn_vegetable(a, &mut u);
    assert!(rt.is_return_void_actor(child));
    assert_eq!(rt.entities.get(child).unwrap().runtime.hp, 160);
    rt.entities.get_mut(child).unwrap().runtime.hp = 80;
    rt.entities.get_mut(a).unwrap().runtime.hp = 10;
    rt.entities.get_mut(b).unwrap().runtime.hp = 200;
    rt.begin_return_void_action(a, &mut u);
    rt.begin_return_void_action(a, &mut u);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 200);
    let saved = rt.ros_state(child).clone();
    let bytes = serde_json::to_vec(&saved).unwrap();
    let restored: crate::runtime::entity::RosmonticRuntime = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved, restored);
    rt.begin_return_void_action(a, &mut u);
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, 120);
    assert_eq!(rt.entities.get(child).unwrap().runtime.hp, 0);
}
