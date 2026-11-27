//! 小队展开、部署、阶段转换及机械支援的真实状态回归。
use super::super::*;
fn battle(raw: &str) -> RuntimeRunner {
    RuntimeRunner::from_custom_mixed_namerena_raw(raw.to_owned(), default_custom_runtime_import_config().unwrap()).unwrap()
}
fn id(r: &RuntimeRunner, name: &str) -> EntityIdx {
    r.runtime().entities.iter().find(|(_, e)| e.template.name == name).unwrap().0
}
#[test]
fn squad_aliases_create_four_teammates_without_alias_entity() {
    let r = battle("fox@!\n\nribbit@!\n\nseed:squad");
    assert_eq!(r.runtime().entities.len(), 8);
    for prefix in ["fox", "ribbit"] {
        let first = id(&r, &format!("{prefix}1"));
        let team = r.runtime().plain_effective_team(first);
        for n in 1..=4 {
            let member = id(&r, &format!("{prefix}{n}"));
            assert_eq!(r.runtime().plain_effective_team(member), team);
            assert!(r.runtime().entities.get(member).unwrap().runtime.flags.contains(PlayerKindFlags::BOSS));
        }
    }
    assert_ne!(
        r.runtime().plain_effective_team(id(&r, "fox1")),
        r.runtime().plain_effective_team(id(&r, "ribbit1"))
    );
}
#[test]
fn deployment_changes_members_only_once_and_survives_refresh() {
    let mut r = battle("fox@!\n\nribbit@!");
    let a = id(&r, "fox1");
    let b = id(&r, "fox4");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let base = rt.entities.get(b).unwrap().runtime.attack;
    rt.remaining_cast(a, 0, false, &mut u);
    assert_eq!(rt.entities.get(b).unwrap().runtime.attack, base * 3);
    rt.entities.get_mut(b).unwrap().refresh_runtime_stats_from_template();
    assert_eq!(rt.entities.get(b).unwrap().runtime.attack, base * 3);
    assert_eq!(rt.ros_state(a).levels[0], 0);
}
#[test]
fn al1s_revives_twice_and_shuts_down_type_f() {
    let mut r = battle("AL1S@!\n\nalpha");
    let a = id(&r, "AL1S");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.entities.get_mut(a).unwrap().runtime.hp = 0;
    assert!(rt.remaining_revive(a, &mut u));
    assert_eq!(rt.ros_state(a).phase, 2);
    rt.remaining_cast(a, 5, false, &mut u);
    let children = rt.remaining_children(a, 30);
    assert_eq!(children.len(), 1);
    rt.entities.get_mut(a).unwrap().runtime.hp = 0;
    assert!(rt.remaining_revive(a, &mut u));
    assert_eq!(rt.ros_state(a).phase, 3);
    assert!(rt.remaining_children(a, 30).is_empty());
    assert_eq!(rt.entities.get(a).unwrap().runtime.shield, 400);
    rt.remaining_cast(a, 1, false, &mut u);
    for role in 36..=38 {
        assert_eq!(rt.remaining_children(a, role).len(), 1);
    }
    rt.entities.get_mut(a).unwrap().runtime.hp = 0;
    assert!(!rt.remaining_revive(a, &mut u));
}
#[test]
fn squad_support_creates_owned_machines_and_shields() {
    let mut r = battle("fox@!\n\nribbit@!");
    let f = id(&r, "fox2");
    let rb = id(&r, "ribbit3");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.remaining_cast(f, 1, false, &mut u);
    assert_eq!(rt.remaining_children(f, 34).len(), 1);
    rt.remaining_cast(f, 2, false, &mut u);
    assert_eq!(rt.entities.get(f).unwrap().runtime.shield, 80);
    rt.remaining_cast(rb, 1, false, &mut u);
    assert_eq!(rt.remaining_children(rb, 31).len(), 1);
}
/// 整场推进验证交叉反击和召唤不会挂起，快照也能继续运行。
#[test]
fn remaining_bosses_finish_seeded_battles() {
    for name in ["yukari", "AL1S", "fox", "ribbit"] {
        for seed in [2, 19, 37] {
            let mut r = battle(&format!("{name}@!\n\nalpha\nbeta\ngamma\n\nseed:{seed}"));
            let mut copy = r.clone();
            let full = r.run_until_winner(20000);
            let fast = copy.run_to_completion(20000);
            assert!(!full.guard_exhausted, "{name}, seed={seed}");
            assert_eq!(full.winner_team, fast.winner_team);
            assert_eq!(r.runtime().entities, copy.runtime().entities);
        }
    }
}
/// 旧成员输入也补齐整队，重复别名、显式成员不会叠出多份。
#[test]
fn member_inputs_complete_squads_and_expansion_is_idempotent() {
    for raw in ["fox4@!\n\nalpha", "ribbit2@!\n\nalpha", "fox@!\nfox2@!\nfox@!\n\nalpha"] {
        let r = battle(raw);
        assert_eq!(r.runtime().entities.len(), 5);
        let input = crate::namerena::NamerenaInput::parse(raw).unwrap();
        assert_eq!(input.groups[0].len(), 4);
    }
}
#[test]
fn yukari_gap_barrier_charge_and_unlock_progress() {
    let mut r = battle("yukari@!\n\nalpha");
    let a = id(&r, "yukari");
    let t = id(&r, "alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.begin_remaining_action(a, &mut u);
    let mut v = RuntimeDefendValue::Damage {
        value: 100,
        caster: t,
        target: a,
        kind: DamageKind::Default,
    };
    assert!(rt.remaining_defend(a, &mut v, &mut u));
    assert_eq!(v.damage(), Some(0));
    assert!(!rt.ros_state(a).gap);
    rt.remaining_cast(a, 1, false, &mut u);
    v.set_damage(240);
    rt.remaining_defend(a, &mut v, &mut u);
    assert_eq!(v.damage(), Some(1));
    v.set_damage(300);
    rt.remaining_defend(a, &mut v, &mut u);
    assert_eq!(v.damage(), Some(60));
    assert_eq!(rt.ros_state(a).barrier_turns, 0);
    rt.remaining_cast(a, 0, false, &mut u);
    assert_eq!(rt.ros_state(a).charged_skill, Some(0));
    rt.remaining_action(a, &mut u);
    assert_eq!(rt.ros_state(a).levels[0], 0);
}
#[test]
fn helicopter_shares_only_while_alive_and_emp_stops_machines() {
    let mut r = battle("fox@!\n\nribbit@!");
    let f = id(&r, "fox2");
    let b = id(&r, "ribbit3");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.remaining_cast(b, 1, false, &mut u);
    let heli = rt.remaining_children(b, 31)[0];
    let hp = rt.entities.get(heli).unwrap().runtime.hp;
    let mut v = RuntimeDefendValue::Damage {
        value: 80,
        caster: f,
        target: b,
        kind: DamageKind::Default,
    };
    assert!(rt.remaining_defend(b, &mut v, &mut u));
    assert_eq!(v.damage(), Some(0));
    assert!(rt.entities.get(heli).unwrap().runtime.hp < hp);
    rt.remaining_cast(f, 0, false, &mut u);
    assert!(rt.remaining_children(b, 31).is_empty());
    v.set_damage(80);
    assert!(!rt.remaining_defend(b, &mut v, &mut u));
    assert_eq!(v.damage(), Some(80));
}
#[test]
fn fox_last_stand_revives_members_and_mechanical_resummon() {
    let mut r = battle("fox@!\n\nalpha");
    let a = id(&r, "fox1");
    let f2 = id(&r, "fox2");
    let f4 = id(&r, "fox4");
    let t = id(&r, "alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let mut v = RuntimeDefendValue::Damage {
        value: 10000,
        caster: t,
        target: a,
        kind: DamageKind::Default,
    };
    assert!(rt.remaining_defend(a, &mut v, &mut u));
    assert_eq!(v.damage(), Some(0));
    assert_eq!(rt.ros_state(a).last_stand, 4);
    rt.entities.get_mut(f2).unwrap().runtime.hp = 0;
    rt.drain_plain_lethal_damage_into(t, f2, &mut u);
    rt.ros_state(a).last_stand = 1;
    rt.remaining_cast(a, 1, false, &mut u);
    assert_eq!(rt.entities.get(f2).unwrap().runtime.hp, 246);
    assert!(rt.entities.get(f2).unwrap().runtime.alive);
    rt.remaining_cast(f4, 0, false, &mut u);
    let child = rt.remaining_children(f4, 33)[0];
    rt.entities.get_mut(child).unwrap().runtime.hp = 0;
    rt.drain_plain_lethal_damage_into(t, child, &mut u);
    assert!(rt.remaining_allowed(f4, 0));
    rt.remaining_cast(f4, 0, false, &mut u);
    assert_eq!(rt.remaining_children(f4, 33).len(), 1);
}
#[test]
fn ribbit_breakthrough_keeps_zero_defense_after_refresh() {
    let mut r = battle("ribbit@!\n\nalpha");
    let a = id(&r, "ribbit1");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.remaining_cast(a, 0, false, &mut u);
    // 使用生命比例触发突破，兼容普通与备份不同的生命上限。
    let hp = rt.entities.get(a).unwrap().template.max_hp / 4;
    rt.entities.get_mut(a).unwrap().runtime.hp = hp;
    rt.begin_remaining_action(a, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.defense, 0);
    rt.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
    assert_eq!(rt.entities.get(a).unwrap().runtime.defense, 0);
}

/// 目盲不跳过行动：普攻、法术、必中和瘟疫均落空，仍可蓄力。
#[test]
fn squad_blind_misses_attacks_but_allows_support_and_expires() {
    let mut r = battle("fox_@!\n\nalpha\nbeta");
    let f = id(&r, "fox1");
    let a = id(&r, "alpha");
    let b = id(&r, "beta");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let agility = rt.entities.get(a).unwrap().runtime.agility;
    for _ in 0..256 {
        if rt.squad_blind(f, a, &mut u) {
            break;
        }
    }
    assert_eq!(rt.ros_state(a).blind_turns, 3);
    assert!(!rt.entities.get(a).unwrap().states.is_frozen());
    assert_eq!(rt.entities.get(a).unwrap().runtime.agility, agility);
    let hp = rt.entities.get(b).unwrap().runtime.hp;
    for magic in [false, true] {
        assert_eq!(rt.ros_hit(a, b, magic, 10000.0, false, PlainAttackOnDamage::None, &mut u), 0);
        assert_eq!(rt.ros_hit(a, b, magic, 10000.0, true, PlainAttackOnDamage::None, &mut u), 0);
    }
    rt.drain_plain_half_skill_into(a, b, &mut u);
    rt.drain_plain_disperse_skill_into(a, b, &mut u);
    rt.drain_plain_thunder_skill_into(a, b, &mut u);
    let team = rt.plain_effective_team(b);
    rt.drain_plain_charm_skill_into(a, b, &mut u);
    assert_eq!(rt.plain_effective_team(b), team);
    assert!(rt.magic_attack_dodged(a, b));
    assert_eq!(rt.entities.get(b).unwrap().runtime.hp, hp);
    rt.drain_plain_charge_skill_into(a, &mut u);
    assert!(rt.entities.get(a).unwrap().runtime.charge.active);
    for expected in [2, 1, 0] {
        rt.end_remaining_action(a);
        assert_eq!(rt.ros_state(a).blind_turns, expected);
    }
    assert!(rt.ros_hit(a, b, false, 10000.0, true, PlainAttackOnDamage::None, &mut u) > 0);
}

/// 隐藏备份仍是完整 Boss 队伍，保留原数值、图标与部署归属。
#[test]
fn original_squad_aliases_preserve_stats_and_isolate_same_team_members() {
    for (prefix, hp) in [("fox", [660, 670, 730, 550]), ("ribbit", [670, 720, 540, 530])] {
        let mut r = battle(&format!("{prefix}@!\n{prefix}_@!\n{prefix}2_@!\n\nalpha"));
        assert_eq!(r.runtime().entities.len(), 9);
        let backups: Vec<_> = r
            .runtime()
            .entities
            .iter()
            .filter(|(_, e)| e.template.id_key_name.ends_with("_@!"))
            .map(|(id, e)| (id, e.template.max_hp))
            .collect();
        assert_eq!(backups.iter().map(|(_, hp)| *hp).collect::<Vec<_>>(), hp);
        for (id, _) in &backups {
            assert_eq!(r.runtime().squad_power_scale(*id), 1.0);
        }
        let leader = backups[0].0;
        let regular = id(&r, &format!("{prefix}1"));
        let rt = r.runtime_mut();
        assert_eq!(rt.squad_member(leader, &format!("{prefix}2")), Some(backups[1].0));
        assert_ne!(rt.squad_member(regular, &format!("{prefix}2")), Some(backups[1].0));
        let mut u = RunUpdates::new();
        rt.remaining_cast(leader, 0, false, &mut u);
        assert!(!rt.ros_state(regular).deployed);
        let icon = crate::namerena::icon_render::render_icon_vec_from_name(&format!("{prefix}_@!"));
        assert_eq!(
            icon,
            crate::namerena::icon_render::render_icon_vec_from_name(&format!("{prefix}1@!"))
        );
    }
}
/// 目盲回合仍产生真实行动，持续数在自己的行动结束后减少。
#[test]
fn blinded_player_still_takes_a_normal_turn() {
    let mut r = battle("alpha\n\nbeta");
    let a = id(&r, "alpha");
    let b = id(&r, "beta");
    let rt = r.runtime_mut();
    let hp = rt.entities.get(b).unwrap().runtime.hp;
    let e = rt.entities.get_mut(a).unwrap();
    e.template.skills = SkillLoadout::default();
    e.runtime.move_state.speed_points = 1000000;
    rt.ros_state(a).blind_turns = 1;
    let out = r.run_round();
    assert_eq!(out.action.unwrap().actor, a);
    assert_eq!(r.runtime().entities.get(b).unwrap().runtime.hp, hp);
    assert_eq!(
        r.runtime().entities.get(a).unwrap().runtime.rosmontic.as_ref().unwrap().blind_turns,
        0
    );
}
