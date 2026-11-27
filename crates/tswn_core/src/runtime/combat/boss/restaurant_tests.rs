//! 餐厅的真实技能、免疫与快照回放回归。
use super::super::*;
use crate::runtime::entity::RestaurantIngredient;
fn battle(names: &str) -> (RuntimeRunner, EntityIdx, EntityIdx) {
    let r = RuntimeRunner::from_custom_mixed_namerena_raw(
        format!("会员制餐厅@!\n\n{names}\n\nseed:restaurant"),
        default_custom_runtime_import_config().unwrap(),
    )
    .unwrap();
    let a = r.runtime().entities.iter().find(|(_, e)| e.template.name == "会员制餐厅").unwrap().0;
    let b = r.runtime().entities.iter().find(|(_, e)| e.template.name == "alpha").unwrap().0;
    (r, a, b)
}

/// 六道各自保留配方，制作不计数，满额不抓人；吃下一道后才能补菜。
#[test]
fn restaurant_six_dishes_keep_independent_recipes_and_only_eating_counts() {
    let (mut r, a, b) = battle("alpha\nbeta\ngamma");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    for key in 25..31 {
        rt.restaurant_make_dish(a, vec![ingredient(key, None)], &mut u);
    }
    assert_eq!(rt.restaurant_state(a).dishes.len(), 5);
    assert_eq!(rt.restaurant_state(a).completed, 0);
    let snapshot = rt.restaurant_state(a).clone();
    let restored: crate::runtime::entity::RestaurantRuntime =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert_eq!(snapshot, restored);
    let before = rt.world.flat_alive().to_vec();
    let events = u.updates.len();
    rt.restaurant_cook(a, &mut u);
    rt.restaurant_make_dish(a, vec![ingredient(34, None)], &mut u);
    assert_eq!(before, rt.world.flat_alive());
    assert_eq!(events, u.updates.len());
    // 击倒非队首菜也使用它自己的配方，不会消费队首的防御菜。
    let dish = rt.restaurant_state(a).dishes[2].0;
    assert!(rt.apply_plain_legacy_damage_into(b, dish, 10000, &mut u));
    rt.drain_plain_lethal_damage_into(b, dish, &mut u);
    assert_eq!(rt.restaurant_state(a).completed, 1);
    assert_eq!(rt.restaurant_state(a).dishes.len(), 4);
    assert!(u.updates.iter().any(|e| e.message == "[1]获得[护身符]效果" && e.target == b.0 as usize));
    // 同一死亡链再次回调不能重复结算。
    rt.drain_lethal_damage_hooks_into(b, dish, &mut u);
    assert_eq!(rt.restaurant_state(a).completed, 1);
    rt.restaurant_make_dish(a, vec![ingredient(34, None)], &mut u);
    assert_eq!(rt.restaurant_state(a).dishes.len(), 5);
    assert_eq!(rt.restaurant_state(a).completed, 1);
}
/// 召唤物吃菜生效但不计数；本体吃菜达到额度则触发餐厅退场。
#[test]
fn restaurant_defeated_dish_counts_bodies_but_not_summons() {
    for summon in [true, false] {
        let (mut r, a, b) = battle("alpha");
        let rt = r.runtime_mut();
        let mut u = RunUpdates::new();
        let eater = if summon { rt.rc_spawn_small(b, &mut u) } else { b };
        rt.restaurant_state(a).limit = 1;
        rt.restaurant_make_dish(a, vec![ingredient(25, None)], &mut u);
        let dish = rt.restaurant_state(a).dish.unwrap();
        assert!(rt.apply_plain_legacy_damage_into(eater, dish, 10000, &mut u));
        rt.drain_plain_lethal_damage_into(eater, dish, &mut u);
        assert!(rt.restaurant_state(a).dish.is_none());
        assert_eq!(rt.restaurant_state(a).completed, u32::from(!summon));
        assert_eq!(rt.restaurant_state(a).stopped, !summon);
        assert!(u.updates.iter().any(|e| e.message == "[1]获得[防御]效果" && e.target == eater.0 as usize));
        let eaten = u.updates.iter().filter(|e| e.message == "[1]吃下了菜品").count();
        rt.restaurant_dish_defeated(eater, dish, &mut u);
        assert_eq!(u.updates.iter().filter(|e| e.message == "[1]吃下了菜品").count(), eaten);
    }
}

/// 任意专属技能名称保存同源入口，改名不改变其结算，原施法者死亡仍能重放。
#[test]
fn restaurant_generic_call_uses_original_skill_and_current_stats() {
    let (mut runner, restaurant, target) = battle("alpha\ncirno@!");
    let rt = runner.runtime_mut();
    let source = rt.entities.iter().find(|(_, e)| e.template.name == "cirno").unwrap().0;
    let mut events = RunUpdates::new();
    rt.cirno_absolute_zero(source, &mut events);
    let event = events.updates.iter_mut().find(|e| e.skill_call.is_some()).unwrap();
    assert!(event.skill_call.as_ref().unwrap().entry.ends_with("cirno_absolute_zero"));
    event.skill_name = Some("任意新名称".to_owned());
    rt.restaurant_finish_round(&events);
    let item = rt
        .restaurant_state(restaurant)
        .history
        .iter()
        .find(|e| e.label == "任意新名称")
        .unwrap()
        .clone();
    let call = item.invocation.unwrap();
    assert_eq!(call.caster, source);
    // 直接入口与菜品使用同一随机流及当前属性，食客不重新随机挑选。
    rt.entities.get_mut(target).unwrap().runtime.hp = 100000;
    rt.entities.get_mut(target).unwrap().template.max_hp = 100000;
    rt.entities.get_mut(source).unwrap().runtime.magic = 200;
    let mut expected = rt.clone();
    expected.scratch.replay_skill_target = Some((source, target));
    rt.restaurant_make_dish(
        restaurant,
        vec![RestaurantIngredient {
            label: "任意新名称".to_owned(),
            skill: None,
            invocation: Some(call.clone()),
            source: Some(source),
            person: None,
        }],
        &mut RunUpdates::new(),
    );
    // 建菜会消费随机流，比较前再次同步两侧随机数。
    expected.rng = rt.rng.clone();
    expected.entities.get_mut(target).unwrap().runtime.hp = 100000;
    expected.cirno_absolute_zero(source, &mut RunUpdates::new());
    rt.restaurant_replay(&call, target, &mut RunUpdates::new());
    assert_eq!(
        rt.entities.get(target).unwrap().runtime.hp,
        expected.entities.get(target).unwrap().runtime.hp
    );
    rt.entities.get_mut(source).unwrap().runtime.hp = 0;
    rt.entities.get_mut(source).unwrap().runtime.alive = false;
    rt.restaurant_replay(&call, target, &mut RunUpdates::new());
    assert!(!rt.entities.get(source).unwrap().runtime.alive);
}
/// 观察等没有括号的日志也携带调用凭据，喂菜衍生调用不会再次入库。
#[test]
fn restaurant_generic_capture_records_observe_but_excludes_own_actions() {
    let (mut runner, restaurant, _) = battle("alpha\nrc@!");
    let rt = runner.runtime_mut();
    let rc = rt.entities.iter().find(|(_, e)| e.template.name == "rc").unwrap().0;
    let mut updates = RunUpdates::new();
    let small = rt.rc_spawn_small(rc, &mut updates);
    rt.rc_observe(rc, small, &mut updates);
    rt.restaurant_finish_round(&updates);
    assert!(
        rt.restaurant_state(restaurant)
            .history
            .iter()
            .any(|i| i.label == "观察" && i.invocation.is_some())
    );
    let n = rt.restaurant_state(restaurant).history.len();
    let mut updates = RunUpdates::new();
    rt.restaurant_cook(restaurant, &mut updates);
    rt.restaurant_finish_round(&updates);
    assert_eq!(rt.restaurant_state(restaurant).history.len(), n);
}
/// 特殊句式按实际入口捕获；治疗菜品始终给被喂的本体，不再治疗原召唤物。
#[test]
fn restaurant_generic_capture_handles_snap_and_healing() {
    let (mut runner, restaurant, target) = battle("alpha\nrc@!\nthanos@!");
    let rt = runner.runtime_mut();
    let thanos = rt.entities.iter().find(|(_, e)| e.template.name == "thanos").unwrap().0;
    let rc = rt.entities.iter().find(|(_, e)| e.template.name == "rc").unwrap().0;
    let mut events = RunUpdates::new();
    rt.drain_plain_thanos_snap_into(thanos, Default::default(), &mut events);
    rt.restaurant_finish_round(&events);
    assert!(
        rt.restaurant_state(restaurant)
            .history
            .iter()
            .any(|i| i.label == "响指" && i.invocation.is_some())
    );
    let mut events = RunUpdates::new();
    let small = rt.rc_spawn_small(rc, &mut events);
    rt.entities.get_mut(small).unwrap().runtime.hp = 1;
    rt.rc_heal_small(rc, &mut events);
    rt.restaurant_finish_round(&events);
    let call = rt
        .restaurant_state(restaurant)
        .history
        .iter()
        .find(|i| i.label == "我突然有点担心小")
        .unwrap()
        .invocation
        .clone()
        .unwrap();
    rt.entities.get_mut(target).unwrap().runtime.hp = 1;
    rt.entities.get_mut(target).unwrap().runtime.alive = true;
    rt.restaurant_replay(&call, target, &mut RunUpdates::new());
    let e = rt.entities.get(target).unwrap();
    assert_eq!(e.runtime.hp, e.template.max_hp);
}
fn ingredient(key: u8, source: Option<EntityIdx>) -> RestaurantIngredient {
    RestaurantIngredient {
        label: super::restaurant::SKILL_NAMES[key as usize].to_owned(),
        skill: Some(key),
        invocation: None,
        source,
        person: None,
    }
}
/// 普攻以固定名称入库，不保留原攻击者属性，喂出伤害与虚拟厨师直接普攻相同。
#[test]
fn restaurant_plain_attack_is_an_ingredient_cast_by_chef() {
    let (mut runner, restaurant, target) = battle("alpha");
    let rt = runner.runtime_mut();
    let mut events = RunUpdates::new();
    rt.drain_plain_default_attack_into(target, restaurant, false, &mut events);
    rt.restaurant_finish_round(&events);
    let item = rt
        .restaurant_state(restaurant)
        .history
        .iter()
        .find(|i| i.label == "普攻")
        .unwrap()
        .clone();
    assert_eq!(item.skill, Some(super::restaurant::PLAIN_ATTACK));
    assert!(item.source.is_none() && item.invocation.is_none());
    rt.restaurant_cook(restaurant, &mut events);
    assert!(rt.restaurant_state(restaurant).ingredients.iter().all(|i| i.label == "普攻"));
    let chef = rt.restaurant_chef(restaurant);
    rt.entities.get_mut(target).unwrap().runtime.hp = 10000;
    rt.entities.get_mut(target).unwrap().runtime.attack = 100000;
    rt.entities.get_mut(target).unwrap().template.max_hp = 10000;
    let mut expected = rt.clone();
    expected.entities.get_mut(chef).unwrap().runtime.alive = true;
    expected.drain_plain_default_attack_into(chef, target, false, &mut RunUpdates::new());
    events = RunUpdates::new();
    rt.restaurant_cast(restaurant, Some(target), super::restaurant::PLAIN_ATTACK, target, &mut events);
    assert_eq!(
        rt.entities.get(target).unwrap().runtime.hp,
        expected.entities.get(target).unwrap().runtime.hp
    );
    assert!(
        events
            .updates
            .iter()
            .any(|e| e.message == "[0]发起攻击" && e.caster == restaurant.0 as usize)
    );
    assert!(!rt.entities.get(chef).unwrap().runtime.alive);
    assert_eq!(rt.entities.get(chef).unwrap().runtime.attack, 54);
}
/// 虚拟施法身份不进入标准快照，日志中的所有角色引用归餐厅，血量变化不能冒充餐厅。
#[test]
fn restaurant_chef_is_hidden_from_snapshots_and_events() {
    let (mut runner, restaurant, target) = battle("alpha");
    let count = runner.player_snapshots().len();
    let chef = runner.runtime_mut().restaurant_chef(restaurant);
    assert!(runner.player_snapshot(chef.0 as usize).is_none());
    assert_eq!(runner.player_snapshots().len(), count);
    let rt = runner.runtime_mut();
    let mut events = RunUpdates::new();
    rt.restaurant_cast(restaurant, None, 0, target, &mut events);
    let mut chained = RunUpdate::new("[0]影响[1]", chef.0 as usize, chef.0 as usize, 1);
    chained.targets.extend([chef.0 as usize, target.0 as usize]);
    chained.animation_targets = Some(vec![chef.0 as usize, target.0 as usize].into_boxed_slice());
    chained.hp_changes = Some(
        vec![
            crate::runtime::update::RunUpdateHp {
                player: chef.0 as usize,
                before: 300,
                after: 280,
            },
            crate::runtime::update::RunUpdateHp {
                player: target.0 as usize,
                before: 100,
                after: 80,
            },
        ]
        .into_boxed_slice(),
    );
    events.add(chained);
    rt.restaurant_hide_chef(&mut events);
    for e in &events.updates {
        assert_ne!(e.caster, chef.0 as usize);
        assert_ne!(e.target, chef.0 as usize);
        assert!(!e.targets.contains(&(chef.0 as usize)));
        assert!(e.animation_targets.as_ref().is_none_or(|ids| !ids.contains(&(chef.0 as usize))));
        assert!(
            e.hp_changes
                .as_ref()
                .is_none_or(|changes| changes.iter().all(|hp| hp.player != chef.0 as usize))
        );
    }
    let hp = events.updates.last().unwrap().hp_changes.as_ref().unwrap();
    assert_eq!(hp.len(), 1);
    assert_eq!(hp[0].player, target.0 as usize);
}
/// 烹饪无重复端菜提示，普攻、主动技能与自身增益的喂菜只展示食客结果。
#[test]
fn restaurant_food_logs_only_show_recipient_results() {
    for key in [0, 11, 18, 25, super::restaurant::PLAIN_ATTACK] {
        let (mut runner, restaurant, target) = battle("alpha");
        let rt = runner.runtime_mut();
        rt.entities.get_mut(target).unwrap().runtime.hp = 10000;
        rt.entities.get_mut(target).unwrap().template.max_hp = 10000;
        let item = RestaurantIngredient {
            label: if key == super::restaurant::PLAIN_ATTACK {
                "普攻".to_owned()
            } else {
                super::restaurant::SKILL_NAMES[key as usize].to_owned()
            },
            skill: Some(key),
            invocation: None,
            source: None,
            person: None,
        };
        let mut cook = RunUpdates::new();
        rt.restaurant_make_dish(restaurant, vec![item], &mut cook);
        assert!(!cook.updates.iter().any(|e| e.message.contains("端出")));
        assert_eq!(cook.updates.iter().filter(|e| e.message.contains("开始烹饪")).count(), 1);
        let mut feed = RunUpdates::new();
        rt.restaurant_feed(restaurant, &mut feed);
        rt.restaurant_present_food_results(&mut feed);
        let results: Vec<_> = feed
            .updates
            .iter()
            .filter(|e| e.message != "[0]将菜喂给[1]" && e.update_type != crate::runtime::update::UpdateType::NextLine)
            .collect();
        assert!(!results.is_empty(), "key={key}");
        for event in results {
            assert!(!event.message.contains("[0]"), "key={key}: {}", event.message);
            assert!(!event.message.contains("发起攻击"), "key={key}");
            assert!(!event.has_caster);
            assert_eq!(event.target, target.0 as usize);
            assert!(event.message.contains("[1]"), "key={key}: {}", event.message);
        }
    }
}
/// 指定外显值对应真实内部属性，餐厅仍在敌方选敌池中。
#[test]
fn restaurant_stats_immunity_and_targetability() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let e = rt.entities.get(a).unwrap();
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
        [114, -25, 9, 160, -17, -17, -28, -26]
    );
    let mut u = RunUpdates::new();
    assert!(rt.world.flat_alive().contains(&a));
    rt.apply_plain_legacy_damage_into(b, a, 999, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 114);
    rt.apply_poison_tick_damage_into(b, a, 999, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 114);
    assert!(rt.status_immune(a, "charm"));
    assert!(rt.status_immune(a, "ice"));
    assert!(
        !rt.entities
            .get_mut(a)
            .unwrap()
            .states
            .add_entry(StateEntry::berserk(PLAIN_BERSERK_STATE_KEY, 2))
    );
    rt.entities.get_mut(a).unwrap().runtime.hp = 0;
    rt.drain_die_hooks_into(a, &mut u);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 114);
    assert_eq!(rt.restaurant_state(a).limit, 10);
}
/// 冷启动明确抽两种不同原版技能，来源均为空，之后才建立虚拟厨师。
#[test]
fn restaurant_cold_start_and_chef_stats() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.restaurant_cook(a, &mut u);
    let s = rt.restaurant_state(a).clone();
    assert_eq!(s.ingredients.len(), 2);
    assert!(
        s.ingredients
            .iter()
            .all(|i| i.skill.is_some_and(|k| k < 35) && i.source.is_none() && i.person.is_none())
    );
    assert_ne!(s.ingredients[0].skill, s.ingredients[1].skill);
    assert!(rt.entities.get(b).unwrap().runtime.alive);
    let c = rt.restaurant_chef(a);
    let e = rt.entities.get(c).unwrap();
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
        [300, 54, 54, 214, 54, 54, 54, 54]
    );
    assert!(!rt.world.flat_alive().contains(&c));
}
/// 三材料由两个方法连接，读写快照完整保存配方及真实来源。
#[test]
fn restaurant_three_ingredients_and_snapshot() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let items = vec![ingredient(0, Some(b)), ingredient(1, Some(b)), ingredient(2, None)];
    rt.restaurant_make_dish(a, items.clone(), &mut u);
    let s = rt.restaurant_state(a).clone();
    let name = &rt.entities.get(s.dish.unwrap()).unwrap().template.name;
    assert!(name.starts_with("火球术") && name.ends_with("雷击术"));
    let middle = name.strip_prefix("火球术").unwrap().strip_suffix("雷击术").unwrap();
    let (method1, method2) = middle.split_once("冰冻术").unwrap();
    assert!(super::restaurant_methods::METHODS.contains(&method1));
    assert!(super::restaurant_methods::METHODS.contains(&method2));
    let bytes = serde_json::to_vec(&s).unwrap();
    let restored: crate::runtime::entity::RestaurantRuntime = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(s, restored);
    assert_eq!(s.ingredients, items);
}
/// 材料只来自真实技能发动日志，餐厅自身日志与喂菜生成的攻击不回流。
#[test]
fn restaurant_records_actual_caster_and_ignores_own_output() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    u.add(RunUpdate::new("[0]使用[净化]", b.0 as usize, a.0 as usize, 1));
    u.add(RunUpdate::new("[0]使用[魅惑]", a.0 as usize, b.0 as usize, 1));
    rt.restaurant_finish_round(&u);
    assert_eq!(rt.restaurant_state(a).history, vec![ingredient(17, Some(b))]);
    rt.restaurant_state(a).feeding = true;
    let mut u = RunUpdates::new();
    u.add(RunUpdate::new("[0]使用[火球术]", b.0 as usize, a.0 as usize, 1));
    rt.restaurant_finish_round(&u);
    assert_eq!(rt.restaurant_state(a).history.len(), 1);
}
/// 反复抓人也保留非召唤物本体，不能把最后一名做掉。
#[test]
fn restaurant_preserves_an_enemy_body() {
    for seed in 0..24 {
        let (mut r, a, _) = battle("alpha\nbeta\ngamma");
        let rt = r.runtime_mut();
        rt.rng = crate::rc4::RC4::new(format!("cook-{seed}").as_bytes(), 1);
        let mut u = RunUpdates::new();
        for _ in 0..3 {
            rt.restaurant_cook(a, &mut u);
        }
        let count = rt
            .world
            .flat_alive()
            .iter()
            .filter(|t| {
                rt.plain_effective_team(**t) != rt.plain_effective_team(a)
                    && !rt.entities.get(**t).unwrap().runtime.is_combat_minion()
            })
            .count();
        assert!(count >= 1);
    }
}
/// 喂菜仅选敌方本体，即使召唤物嘲讽无限高也不会选中。
#[test]
fn restaurant_feeds_bodies_not_summons() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    let small = rt.rc_spawn_small(b, &mut u);
    rt.restaurant_make_dish(a, vec![ingredient(29, None), ingredient(25, None)], &mut u);
    u = RunUpdates::new();
    rt.restaurant_feed(a, &mut u);
    let feed = u.updates.iter().find(|e| e.message == "[0]将菜喂给[1]").unwrap();
    assert_eq!(feed.target, b.0 as usize);
    assert_ne!(feed.target, small.0 as usize);
    assert_eq!(rt.restaurant_state(a).completed, 1);
}
/// 没有击倒者的直接移除不制造食客，下次行动重新做菜。
#[test]
fn restaurant_defeated_dish_does_not_count_as_served() {
    let (mut runner, restaurant, _) = battle("alpha");
    let rt = runner.runtime_mut();
    let mut events = RunUpdates::new();
    rt.restaurant_make_dish(restaurant, vec![ingredient(25, None)], &mut events);
    let dish = rt.restaurant_state(restaurant).dish.unwrap();
    assert_eq!(rt.entities.get(dish).unwrap().runtime.hp, 100);
    assert_eq!(rt.entities.get(dish).unwrap().template.max_hp, 100);
    rt.kill_entity_without_damage_into(dish, &mut events);
    rt.restaurant_feed(restaurant, &mut events);
    assert_eq!(rt.restaurant_state(restaurant).completed, 0);
    assert!(rt.restaurant_state(restaurant).dish.is_none());
    assert!(!events.updates.iter().any(|e| e.message == "[0]将菜喂给[1]"));
    rt.drain_restaurant_action_into(restaurant, &mut events);
    let next = rt.restaurant_state(restaurant).dish.unwrap();
    assert_ne!(next, dish);
    rt.kill_entity_without_damage_into(next, &mut events);
    rt.drain_restaurant_action_into(restaurant, &mut events);
    assert_eq!(rt.restaurant_state(restaurant).completed, 0);
    let next = rt.restaurant_state(restaurant).dish.unwrap();
    assert!(rt.entities.get(next).unwrap().runtime.alive);
}
/// 原版所有三十五项均可实际喂给角色，包含被动赋予和需技能槽的分身/交换。
#[test]
fn restaurant_can_serve_all_35_skills() {
    for key in 0..35_u8 {
        let (mut r, a, b) = battle("alpha");
        let rt = r.runtime_mut();
        let mut u = RunUpdates::new();
        rt.restaurant_cast(a, None, key, b, &mut u);
        assert!(rt.entities.get(a).unwrap().runtime.alive, "key={key}");
    }
}
/// 各 Boss 的直接伤害、换号和状态路径都不能绕过餐厅免疫。
#[test]
fn restaurant_survives_every_boss_until_own_exit() {
    for name in crate::namerena::BOSS_NAMES {
        if name == "会员制餐厅" {
            continue;
        }
        let (mut runner, restaurant, _) = battle(&format!("alpha\n{name}@!"));
        for _ in 0..400 {
            if runner.runtime_mut().restaurant_state(restaurant).stopped {
                break;
            }
            runner.runtime_mut().run_minimal_round();
            let rt = runner.runtime_mut();
            if rt.restaurant_state(restaurant).stopped {
                break;
            }
            let e = rt.entities.get(restaurant).unwrap();
            assert_eq!(e.runtime.hp, 114, "boss={name}");
            assert!(e.runtime.alive, "boss={name}");
            assert_eq!(e.template.name, "会员制餐厅", "boss={name}");
        }
    }
}
/// 真正生成分身后增加十次，克隆也是可被喂菜的本体。
#[test]
fn restaurant_actual_clone_extends_budget_and_is_body() {
    let (mut r, restaurant, target) = battle("alpha");
    let rt = r.runtime_mut();
    let mut updates = RunUpdates::new();
    rt.restaurant_cast(restaurant, None, 23, target, &mut updates);
    assert_eq!(rt.restaurant_state(restaurant).limit, 20);
    let clone = rt
        .world
        .flat_alive()
        .iter()
        .copied()
        .find(|id| *id != target && *id != restaurant)
        .unwrap();
    assert!(!rt.entities.get(clone).unwrap().runtime.is_combat_minion());
    assert!(rt.entities.get(clone).unwrap().runtime.is_minion());
}
/// 十次完整烹饪喂菜后自退场，分身的成功通知每次增加十次。
#[test]
fn restaurant_cycle_limit_and_clone_extension() {
    let (mut r, a, b) = battle("alpha");
    let rt = r.runtime_mut();
    let mut u = RunUpdates::new();
    rt.restaurant_cloned(b);
    assert_eq!(rt.restaurant_state(a).limit, 20);
    rt.restaurant_cloned(a);
    assert_eq!(rt.restaurant_state(a).limit, 20);
    rt.restaurant_state(a).limit = 2;
    for _ in 0..2 {
        rt.restaurant_make_dish(a, vec![ingredient(25, None), ingredient(29, None)], &mut u);
        rt.restaurant_feed(a, &mut u);
    }
    assert!(rt.restaurant_state(a).stopped);
    assert_eq!(rt.entities.get(a).unwrap().runtime.hp, 0);
    assert!(!rt.entities.get(a).unwrap().runtime.alive);
    assert!(rt.entities.get(b).unwrap().runtime.alive);
    assert!(u.updates.iter().any(|e| e.message.contains("尖锐爆鸣")));
}
/// 有帧和无帧使用同样的日志食材与随机流，不能让胜率模拟变成另一个 Boss。
#[test]
fn restaurant_capture_and_fast_execution_match() {
    for seed in [1, 7, 19] {
        let raw = format!("会员制餐厅@!\n\nalpha\nbeta\n\nseed:{seed}");
        let mut a = RuntimeRunner::from_custom_mixed_namerena_raw(raw, default_custom_runtime_import_config().unwrap()).unwrap();
        let mut b = a.clone();
        let x = a.run_until_winner(20000);
        let y = b.run_to_completion(20000);
        assert!(!x.guard_exhausted, "seed={seed}");
        assert_eq!(x.winner_team, y.winner_team);
        for (id, left) in a.runtime().entities.iter() {
            let right = b.runtime().entities.get(id).unwrap();
            assert_eq!(left.template, right.template, "seed={seed}, entity={id:?} template");
            let mut left_runtime = left.runtime.clone();
            let mut right_runtime = right.runtime.clone();
            // 全局批次号不属于战斗结果，同一反击链在两次独立运行中编号不同。
            left_runtime.counter.last_updates_id = None;
            right_runtime.counter.last_updates_id = None;
            left_runtime.counter_xuan_yue.last_updates_id = None;
            right_runtime.counter_xuan_yue.last_updates_id = None;
            assert_eq!(left_runtime, right_runtime, "seed={seed}, entity={id:?} runtime");
            assert_eq!(left.states, right.states, "seed={seed}, entity={id:?} states");
        }
    }
}
