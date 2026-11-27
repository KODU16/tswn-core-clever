//! 会员制餐厅两回合做菜喂菜，材料记录保留真实技能来源，虚拟厨师不加入行动队列。
use super::super::*;
use super::restaurant_methods::METHODS;
use crate::runtime::entity::{RestaurantIngredient, RestaurantRuntime};
const ID: &str = "会员制餐厅";
const DISH: &str = "restaurant-dish";
const CHEF: &str = "restaurant-chef";
// 普攻只从实际日志入库，不加入无日志时的原版三十五技能随机池。
pub(super) const PLAIN_ATTACK: u8 = 35;
pub(super) const SKILL_NAMES: [&str; 35] = [
    "火球术",
    "冰冻术",
    "雷击术",
    "地裂术",
    "吸血攻击",
    "投毒",
    "连击",
    "会心一击",
    "瘟疫",
    "生命之轮",
    "狂暴术",
    "魅惑",
    "加速术",
    "减速术",
    "诅咒",
    "治愈魔法",
    "苏生术",
    "净化",
    "铁壁",
    "蓄力",
    "聚气",
    "背刺",
    "血祭",
    "分身",
    "幻术",
    "防御",
    "守护",
    "伤害反弹",
    "护身符",
    "护盾",
    "反击",
    "吞噬",
    "亡灵复活",
    "垂死",
    "隐匿",
];
/// 应用技能造成的字段变化，未改变的捕获上下文不回写原角色。
fn merge_skill_changes(before: &serde_json::Value, after: &serde_json::Value, current: &mut serde_json::Value) {
    if before == after {
        return;
    }
    if let (Some(a), Some(b), Some(c)) = (before.as_object(), after.as_object(), current.as_object_mut()) {
        for (key, value) in b {
            if let (Some(old), Some(now)) = (a.get(key), c.get_mut(key)) {
                merge_skill_changes(old, value, now);
            } else {
                c.insert(key.clone(), value.clone());
            }
        }
    } else {
        *current = after.clone();
    }
}
impl CombatRuntime {
    /// 准备模板覆盖占位身份后重建餐厅索引，每场战斗重新计算额度。
    pub(crate) fn restaurant_init(&mut self) {
        self.restaurant_bosses = self
            .entities
            .iter()
            .filter_map(|(id, e)| (e.template.name == ID && e.runtime.flags.contains(PlayerKindFlags::BOSS)).then_some(id))
            .collect();
        for id in self.restaurant_bosses.clone() {
            let team = self.entities.get(id).unwrap().runtime.team;
            let count = self
                .entities
                .iter()
                .filter(|(_, e)| e.runtime.team != team && e.runtime.alive && !e.runtime.is_combat_minion())
                .count();
            self.restaurant_state(id).limit = (count as u32).saturating_mul(10);
            self.entities.get_mut(id).unwrap().states.immune_to_effects = true;
        }
    }
    pub(crate) fn is_restaurant(&self, id: EntityIdx) -> bool { self.is_rosmontic_boss(id, ID) }
    pub(crate) fn is_restaurant_actor(&self, id: EntityIdx) -> bool {
        self.is_restaurant(id)
            || self
                .entities
                .get(id)
                .is_some_and(|e| e.template.id_key_name == DISH || e.template.id_key_name == CHEF)
    }
    pub(crate) fn restaurant_state(&mut self, id: EntityIdx) -> &mut RestaurantRuntime {
        self.ros_state(id).restaurant.get_or_insert_with(Default::default)
    }
    /// 任意直接改血、改队、属性或专属增益都在回合边界恢复；菜的进度独立保存。
    pub(crate) fn restaurant_restore(&mut self, id: EntityIdx) -> bool {
        if !self.is_restaurant(id)
            || self
                .entities
                .get(id)
                .and_then(|e| e.runtime.rosmontic.as_ref())
                .and_then(|s| s.restaurant.as_ref())
                .is_some_and(|s| s.stopped)
        {
            return false;
        }
        let e = self.entities.get_mut(id).unwrap();
        e.runtime.hp = 114;
        e.runtime.alive = true;
        e.runtime.team = e.template.team;
        e.runtime.shield = 0;
        e.states.clear_score_profile_for_reuse();
        e.states.immune_to_effects = true;
        let s = e.runtime.rosmontic.as_mut().unwrap();
        let restaurant = s.restaurant.take();
        **s = Default::default();
        s.restaurant = restaurant;
        e.refresh_runtime_stats_from_template();
        true
    }
    /// 输出中的虚拟厨师统一投影为餐厅，内部伤害结算和施法属性保持独立。
    pub(crate) fn restaurant_hide_chef(&self, updates: &mut RunUpdates) {
        if self.restaurant_bosses.is_empty() {
            return;
        }
        for (chef, entity) in self.entities.iter().filter(|(_, e)| e.template.id_key_name == CHEF) {
            let chef = chef.0 as usize;
            let owner = entity.runtime.root_owner.0 as usize;
            for event in &mut updates.updates {
                if event.caster == chef {
                    event.caster = owner;
                }
                if event.target == chef {
                    event.target = owner;
                }
                for target in &mut event.targets {
                    if *target == chef {
                        *target = owner;
                    }
                }
                if let Some(targets) = &mut event.animation_targets {
                    for target in targets.iter_mut() {
                        if *target == chef {
                            *target = owner;
                        }
                    }
                }
                // 虚拟厨师自身的血量变化不能显示成餐厅受伤或治疗。
                if let Some(changes) = &mut event.hp_changes {
                    *changes = changes.iter().copied().filter(|hp| hp.player != chef).collect();
                }
            }
        }
    }
    /// 机制与连锁反馈完成后再精简喂菜日志，避免改变反击等依赖事件的判定。
    pub(crate) fn restaurant_present_food_results(&self, updates: &mut RunUpdates) {
        if self.restaurant_bosses.is_empty() {
            return;
        }
        let events = std::mem::take(&mut updates.updates);
        updates.updates = events
            .into_iter()
            .filter_map(|mut event| {
                let Some(recipient) = event.restaurant_recipient.take() else {
                    return Some(event);
                };
                if event.dialogue
                    || event.skill_name.is_some()
                    || super::super::skill_capture::captured_skill_name(&event.message).is_some()
                {
                    return None;
                }
                let text = if event.target == recipient && event.message.contains("[1]") {
                    // 换血等结果也只保留食客身份，不显示原施法者。
                    event.message.replace("[0]", "菜品")
                } else if event.has_caster && event.caster == recipient && event.message.contains("[0]") {
                    event.message.replace("[0]", "[1]")
                } else {
                    return None;
                };
                event.message = text.into();
                event.has_caster = false;
                event.has_target = true;
                event.hide_replay_target = false;
                event.target = recipient;
                event.targets.retain(|id| *id == recipient);
                event.animation_targets = None;
                Some(event)
            })
            .collect();
    }
    /// 收集真正发生的技能日志，不把餐厅、菜与喂菜衍生事件再作为材料。
    pub(crate) fn restaurant_finish_round(&mut self, u: &RunUpdates) {
        if self.restaurant_bosses.is_empty() {
            return;
        }
        let mut found = Vec::new();
        for event in &u.updates {
            let source = EntityIdx(event.caster as u32);
            if event.has_caster && !self.is_restaurant_actor(source) && event.skill_name.as_deref() == Some("普攻") {
                found.push(RestaurantIngredient {
                    label: "普攻".to_owned(),
                    skill: Some(PLAIN_ATTACK),
                    invocation: None,
                    source: None,
                    person: None,
                });
                continue;
            }
            if !event.has_caster
                || self.is_restaurant_actor(source)
                || (event.skill_call.is_none()
                    && !(event.message.starts_with("[0]使用[")
                        || event.message.starts_with("[0]发动[")
                        || event.message.starts_with("[0]发起[")
                        || event.message.starts_with("[0]开始[")
                        || event.message.starts_with("[0][")))
            {
                continue;
            }
            if let (Some(name), Some(call)) = (&event.skill_name, &event.skill_call) {
                // 原版技能沿既有目标化入口处理自身增益与被动；专属同名技能仍保留原实现。
                if call.entry.starts_with("standard_")
                    && let Some(key) = SKILL_NAMES.iter().position(|n| *n == name)
                {
                    found.push(RestaurantIngredient {
                        label: name.clone(),
                        skill: Some(key as u8),
                        invocation: None,
                        source: Some(source),
                        person: None,
                    });
                    continue;
                }
                found.push(RestaurantIngredient {
                    label: name.clone(),
                    skill: None,
                    invocation: Some(call.clone()),
                    source: Some(source),
                    person: None,
                });
                continue;
            }
            for (key, label) in SKILL_NAMES.iter().enumerate() {
                if event.message.contains(&format!("[{label}]")) {
                    found.push(RestaurantIngredient {
                        label: (*label).to_owned(),
                        skill: Some(key as u8),
                        invocation: None,
                        source: Some(source),
                        person: None,
                    });
                }
            }
        }
        for id in self.restaurant_bosses.clone() {
            let s = self.restaurant_state(id);
            if !s.feeding {
                for item in &found {
                    // 同一入口只更新最近的发动上下文，不把每次属性变化复制成新材料。
                    if let Some(old) = s.history.iter_mut().find(|old| {
                        old.source == item.source
                            && old.label == item.label
                            && old.skill == item.skill
                            && old.invocation.as_ref().map(|c| (&c.entry, &c.arguments))
                                == item.invocation.as_ref().map(|c| (&c.entry, &c.arguments))
                    }) {
                        *old = item.clone();
                    } else {
                        s.history.push(item.clone());
                    }
                }
            }
            s.feeding = false;
            self.restaurant_restore(id);
        }
    }
    /// 拒绝采样余数，包含 122 种方法在内的所有候选均等概率。
    pub(crate) fn restaurant_pick(&mut self, n: usize) -> usize {
        assert!(n > 0);
        let range = 1_u64 << 32;
        let bound = range - range % n as u64;
        loop {
            let v = (0..4).fold(0_u64, |v, _| (v << 8) | u64::from(self.rng.next_u8()));
            if v < bound {
                return (v % n as u64) as usize;
            }
        }
    }
    pub(crate) fn drain_restaurant_action_into(&mut self, id: EntityIdx, u: &mut RunUpdates) {
        if !self.is_restaurant(id) {
            return;
        }
        // 被击倒的菜不能喂出或计数，下次自身行动直接重新烹饪。
        if let Some(dish) = self.restaurant_state(id).dish
            && !self.entities.get(dish).is_some_and(|e| e.runtime.alive && e.runtime.hp > 0)
        {
            let state = self.restaurant_state(id);
            state.dish = None;
            state.ingredients.clear();
        }
        if self.restaurant_state(id).dish.is_some() {
            self.restaurant_feed(id, u);
        } else {
            self.restaurant_cook(id, u);
        }
    }
    /// 烹饪会移除入选敌人，但始终留下一个非召唤本体；不能移除者也不会作为人物材料。
    pub(crate) fn restaurant_cook(&mut self, id: EntityIdx, u: &mut RunUpdates) {
        // 烹饪不是攻击，连平常不可选中的本体也计入敌方人数与保留判定。
        let team = self.plain_effective_team(id);
        let enemies: Vec<_> = self
            .world
            .flat_alive()
            .iter()
            .copied()
            .filter(|t| self.plain_effective_team(*t) != team)
            .collect();
        if enemies.is_empty() {
            return;
        }
        let roots: Vec<_> = enemies
            .iter()
            .copied()
            .filter(|t| !self.entities.get(*t).unwrap().runtime.is_combat_minion())
            .collect();
        let history = self.restaurant_state(id).history.clone();
        let mut items = Vec::new();
        if roots.len() == 1 && history.is_empty() {
            let a = self.restaurant_pick(35);
            let mut b = self.restaurant_pick(34);
            if b >= a {
                b += 1;
            }
            for key in [a, b] {
                items.push(RestaurantIngredient {
                    label: SKILL_NAMES[key].to_owned(),
                    skill: Some(key as u8),
                    invocation: None,
                    source: None,
                    person: None,
                });
            }
        } else {
            // 优先留下没有召唤归属的本体，抓取其他本体不会连带消灭最后一名。
            let keep = roots
                .iter()
                .copied()
                .find(|t| {
                    let e = self.entities.get(*t).unwrap();
                    e.runtime.owner == *t || e.runtime.root_owner == *t
                })
                .or_else(|| roots.first().copied());
            let mut pool = history;
            for t in enemies {
                if Some(t) != keep && !self.is_restaurant(t) {
                    pool.push(RestaurantIngredient {
                        label: self.entities.get(t).unwrap().template.display_name.clone(),
                        skill: None,
                        invocation: None,
                        source: None,
                        person: Some(t),
                    });
                }
            }
            let count = 2 + self.restaurant_pick(2);
            for _ in 0..count {
                if pool.is_empty() {
                    // 食材耗尽时复用已取出的材料，不凭空加入原版技能。
                    // 唯一的随机原版两技能入口是上面的单本体无日志分支。
                    if let Some(item) = items.last().cloned() {
                        items.push(item);
                    }
                    break;
                }
                let index = self.restaurant_pick(pool.len());
                let item = pool.remove(index);
                if let Some(person) = item.person {
                    let team = self.entities.get(person).unwrap().runtime.team;
                    let e = self.entities.get_mut(person).unwrap();
                    e.runtime.hp = 0;
                    e.runtime.alive = false;
                    self.ros_message(id, person, "[1]被烹饪消失了", u);
                    self.mark_dead_with_linked_minions_into(person, team, u);
                    // 移除本体可能连带移除召唤物，不能再次把同一个实体做进菜。
                    pool.retain(|p| {
                        p.person
                            .is_none_or(|t| self.entities.get(t).is_some_and(|e| e.runtime.hp > 0 && e.runtime.alive))
                    });
                }
                items.push(item);
            }
        }
        self.restaurant_make_dish(id, items, u);
    }
    /// 三项食材用两个独立方法连接，整个菜名作为一个结构化高亮 token。
    pub(crate) fn restaurant_make_dish(&mut self, id: EntityIdx, items: Vec<RestaurantIngredient>, u: &mut RunUpdates) {
        let mut name = String::new();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                name.push_str(METHODS[self.restaurant_pick(METHODS.len())]);
            }
            name.push_str(&item.label);
        }
        let kind = self.registry.player_kind_id_by_export_name(DEFAULT_CORE_SUMMON_KIND_EXPORT).unwrap();
        let team = self.entities.get(id).unwrap().runtime.team;
        let mut t = PlayerTemplate::with_kind(0, name.clone(), kind, team, 100, 0);
        t.id_key_name = DISH.to_owned();
        t.speed = 0;
        let dish = self.entities.next_spawn_idx(&t);
        // 入场只更新菜品状态，烹饪日志统一由下面的一句给出。
        self.effects.push(QueuedEffect::SpawnSilent { caster: id, template: t });
        self.drain_effects_into(u);
        let e = self.entities.get_mut(dish).unwrap();
        e.template.id_key_name = DISH.to_owned();
        e.runtime.policies.damage_share = DamageSharePolicy::None;
        let s = self.restaurant_state(id);
        s.ingredients = items;
        s.dish = Some(dish);
        u.add(RunUpdate::new(
            format!("[0]开始烹饪，做出[{name}]"),
            id.0 as usize,
            dish.0 as usize,
            1,
        ));
    }
    /// 厨师有完整 300/90 模板但不进入世界存活表、选敌池或调度器。
    pub(crate) fn restaurant_chef(&mut self, id: EntityIdx) -> EntityIdx {
        if let Some(c) = self.restaurant_state(id).chef {
            return c;
        }
        let team = self.entities.get(id).unwrap().runtime.team;
        // 内部结算身份沿用餐厅名称，快照与日志仍单独过滤其实体编号。
        let name = self.entities.get(id).unwrap().template.name.clone();
        let mut t = PlayerTemplate::new(0, name, team, 300, 54);
        t.defense = 54;
        t.magic = 54;
        t.resistance = 54;
        t.agility = 54;
        t.wisdom = 54;
        t.speed = 214;
        t.id_key_name = CHEF.to_owned();
        // 虚拟实体也关联餐厅的输入队伍，快照导出才能映射其归属。
        let c = self.entities.spawn_from_template_with_owner(t, &self.registry, Some(id), Some(id));
        // 通用召唤生成会重建模板身份，生成后恢复内部标记供统一隐藏。
        let entity = self.entities.get_mut(c).unwrap();
        entity.template.id_key_name = CHEF.to_owned();
        entity.runtime.alive = false;
        self.restaurant_state(id).chef = Some(c);
        c
    }
    /// 真实施法者即便已经成菜或死亡，仍以该实体属性结算；只临时恢复发动所需的存活标记。
    pub(crate) fn restaurant_cast(
        &mut self,
        id: EntityIdx,
        source: Option<EntityIdx>,
        key: u8,
        target: EntityIdx,
        u: &mut RunUpdates,
    ) {
        if self.is_restaurant(target) {
            self.ros_message(id, target, "[1]免疫了菜品效果", u);
            return;
        }
        if key == PLAIN_ATTACK {
            // 普攻菜固定由 300/90 的虚拟厨师发动，不使用食材原攻击者的属性。
            let caster = self.restaurant_chef(id);
            let e = self.entities.get_mut(caster).unwrap();
            let (hp, alive) = (e.runtime.hp, e.runtime.alive);
            e.runtime.hp = hp.max(1);
            e.runtime.alive = true;
            self.drain_plain_default_attack_into(caster, target, false, u);
            let e = self.entities.get_mut(caster).unwrap();
            e.runtime.hp = hp;
            e.runtime.alive = alive;
            self.restaurant_hide_chef(u);
            return;
        }
        if key >= 25 {
            self.restaurant_passive(target, key, u);
            return;
        }
        let caster = source
            .filter(|s| self.entities.get(*s).is_some())
            .unwrap_or_else(|| self.restaurant_chef(id));
        let Some(skill) = BuiltinActiveSkill::from_legacy_key(key as usize) else {
            return;
        };
        let (hp, alive, skills) = {
            let e = self.entities.get(caster).unwrap();
            (e.runtime.hp, e.runtime.alive, e.template.skills.clone())
        };
        let sid = self.registry.skill_id_by_export_name(skill.export_name()).unwrap();
        let lane = skills.skills().iter().position(|s| *s == sid);
        {
            let e = self.entities.get_mut(caster).unwrap();
            e.runtime.hp = e.runtime.hp.max(1);
            e.runtime.alive = true;
            if lane.is_none() {
                let mut levels: Vec<_> = skills.skills().iter().copied().zip(skills.levels().iter().copied()).collect();
                levels.push((sid, 256));
                e.template.skills = SkillLoadout::from_skill_levels(levels);
            }
        }
        let lane = lane.unwrap_or(skills.len());
        // 自身类技能将受益者改为被喂者；其余技能锁定这次喂菜的目标，不重新选择。
        if matches!(
            skill,
            BuiltinActiveSkill::Iron
                | BuiltinActiveSkill::Charge
                | BuiltinActiveSkill::Accumulate
                | BuiltinActiveSkill::Clone
                | BuiltinActiveSkill::Shadow
                | BuiltinActiveSkill::Summon
        ) {
            self.restaurant_self_skill(caster, target, skill, u);
        } else {
            let prepared = PreparedBuiltinSkillAction {
                selected: SelectedBuiltinSkill { skill, fixed_lane: lane },
                targets: smallvec::smallvec![target],
            };
            self.drain_plain_builtin_skill_into(caster, prepared, u);
            // 背刺原本需先潜伏，这里一次菜效果完成其攻击。
            if skill == BuiltinActiveSkill::Assassinate && self.entities.get(caster).unwrap().runtime.assassinate.is_some() {
                let p = PreparedBuiltinSkillAction {
                    selected: SelectedBuiltinSkill { skill, fixed_lane: lane },
                    targets: smallvec::smallvec![target],
                };
                self.drain_plain_builtin_skill_into(caster, p, u);
            }
        }
        let e = self.entities.get_mut(caster).unwrap();
        e.template.skills = skills;
        if !alive || hp <= 0 {
            e.runtime.hp = hp;
            e.runtime.alive = alive;
        }
        self.restaurant_hide_chef(u);
    }
    /// 专属技能按原函数重放，原施法者死亡时临时恢复发动上下文。
    pub(crate) fn restaurant_replay(
        &mut self,
        call: &crate::runtime::skill_call::SkillCall,
        target: EntityIdx,
        u: &mut RunUpdates,
    ) {
        let Some(e) = self.entities.get(call.caster) else {
            return;
        };
        let saved = e.runtime.clone();
        // 保存的上下文固定多阶段招式；数值则读取原施法者当前属性。
        let mut execution = *call.caster_state.clone();
        execution.hp = saved.hp.max(1);
        execution.alive = true;
        execution.attack = saved.attack;
        execution.magic = saved.magic;
        execution.wisdom = saved.wisdom;
        execution.defense = saved.defense;
        execution.resistance = saved.resistance;
        execution.agility = saved.agility;
        execution.speed = saved.speed;
        let before = serde_json::to_value(&execution).expect("技能上下文可序列化");
        self.entities.get_mut(call.caster).unwrap().runtime = execution;
        let previous = self.scratch.replay_skill_target.replace((call.caster, target));
        self.replay_captured_skill(call, target, u);
        self.scratch.replay_skill_target = previous;
        // 只合并本次技能真正修改的字段，旧阶段上下文不会覆盖当前其他状态。
        let after = serde_json::to_value(&self.entities.get(call.caster).unwrap().runtime).expect("技能结果可序列化");
        let mut restored = serde_json::to_value(&saved).expect("原角色可序列化");
        merge_skill_changes(&before, &after, &mut restored);
        let mut restored: PlayerRuntime = serde_json::from_value(restored).expect("技能结果保持原字段类型");
        if !saved.alive || saved.hp <= 0 {
            restored.hp = saved.hp;
            restored.alive = saved.alive;
        }
        self.entities.get_mut(call.caster).unwrap().runtime = restored;
    }
    /// 被动技能没有攻击动作；按用户确认给被喂者启用对应原生被动。
    fn restaurant_passive(&mut self, target: EntityIdx, key: u8, u: &mut RunUpdates) {
        let exports = [
            DEFAULT_CORE_DEFEND_SKILL_EXPORT,
            DEFAULT_CORE_PROTECT_SKILL_EXPORT,
            DEFAULT_CORE_REFLECT_SKILL_EXPORT,
            DEFAULT_CORE_RERAISE_SKILL_EXPORT,
            DEFAULT_CORE_SHIELD_SKILL_EXPORT,
            DEFAULT_CORE_COUNTER_SKILL_EXPORT,
            DEFAULT_CORE_MERGE_SKILL_EXPORT,
            DEFAULT_CORE_ZOMBIE_SKILL_EXPORT,
            DEFAULT_CORE_UPGRADE_SKILL_EXPORT,
            DEFAULT_CORE_HIDE_SKILL_EXPORT,
        ];
        let Some(export) = exports.get(key.saturating_sub(25) as usize) else {
            return;
        };
        let sid = self.registry.skill_id_by_export_name(export).unwrap();
        let e = self.entities.get_mut(target).unwrap();
        let mut levels: Vec<_> = e
            .template
            .skills
            .skills()
            .iter()
            .copied()
            .zip(e.template.skills.levels().iter().copied())
            .collect();
        if let Some((_, level)) = levels.iter_mut().find(|(s, _)| *s == sid) {
            *level = (*level).max(256);
        } else {
            levels.push((sid, 256));
        }
        e.template.skills = SkillLoadout::from_skill_levels(levels);
        e.template.skills.prepare_hook_cache(&self.registry);
        u.add(RunUpdate::new(
            format!("[1]获得[{}]效果", SKILL_NAMES[key as usize]),
            target.0 as usize,
            target.0 as usize,
            1,
        ));
    }
    /// 自身增益的作用对象明确为食客，避免把效果误加到原发动者。
    fn restaurant_self_skill(&mut self, caster: EntityIdx, target: EntityIdx, skill: BuiltinActiveSkill, u: &mut RunUpdates) {
        match skill {
            BuiltinActiveSkill::Iron => self.drain_plain_iron_skill_into(target, u),
            BuiltinActiveSkill::Charge => self.drain_plain_charge_skill_into(target, u),
            BuiltinActiveSkill::Accumulate => self.drain_plain_accumulate_skill_into(target, u),
            _ => {
                let sid = self.registry.skill_id_by_export_name(skill.export_name()).unwrap();
                let saved = self.entities.get(target).unwrap().template.skills.clone();
                let mut levels: Vec<_> = saved.skills().iter().copied().zip(saved.levels().iter().copied()).collect();
                let lane = if let Some(i) = levels.iter().position(|(s, _)| *s == sid) {
                    i
                } else {
                    levels.push((sid, 256));
                    levels.len() - 1
                };
                self.entities.get_mut(target).unwrap().template.skills = SkillLoadout::from_skill_levels(levels);
                self.drain_plain_builtin_skill_into(
                    target,
                    PreparedBuiltinSkillAction {
                        selected: SelectedBuiltinSkill { skill, fixed_lane: lane },
                        targets: smallvec::smallvec![target],
                    },
                    u,
                );
                self.entities.get_mut(target).unwrap().template.skills = saved;
            }
        }
        let _ = caster;
    }
    /// 成功分身才增加十次完整循环，日志重放和失败的分身均不计数。
    pub(crate) fn restaurant_cloned(&mut self, actor: EntityIdx) {
        for id in self.restaurant_bosses.clone() {
            if self.plain_effective_team(actor) != self.plain_effective_team(id) {
                let s = self.restaurant_state(id);
                s.limit = s.limit.saturating_add(10);
            }
        }
    }
    /// 完成一次喂菜后消耗菜；达到初始敌方人数十倍的完整循环数再自爆退场。
    pub(crate) fn restaurant_feed(&mut self, id: EntityIdx, u: &mut RunUpdates) {
        let Some(dish) = self.restaurant_state(id).dish.take() else {
            return;
        };
        let items = std::mem::take(&mut self.restaurant_state(id).ingredients);
        // 直接调用喂菜入口时同样拒绝已经被击倒的菜品。
        if !self.entities.get(dish).is_some_and(|e| e.runtime.alive && e.runtime.hp > 0) {
            return;
        }
        let team = self.plain_effective_team(id);
        let targets: Vec<_> = self
            .world
            .flat_alive()
            .iter()
            .copied()
            .filter(|t| self.plain_effective_team(*t) != team)
            .collect();
        if targets.is_empty() {
            self.restaurant_state(id).dish = Some(dish);
            self.restaurant_state(id).ingredients = items;
            return;
        }
        let targets: Vec<_> = targets
            .into_iter()
            .filter(|t| !self.entities.get(*t).unwrap().runtime.is_combat_minion())
            .collect();
        if targets.is_empty() {
            self.restaurant_state(id).dish = Some(dish);
            self.restaurant_state(id).ingredients = items;
            return;
        }
        let target = targets[self.restaurant_pick(targets.len())];
        // 每份菜使用新的 300/90 虚拟施法状态，不继承上一份菜交换的血量或增益。
        if let Some(chef) = self.restaurant_state(id).chef {
            let e = self.entities.get_mut(chef).unwrap();
            e.runtime = PlayerRuntime::from_template(&e.template, &self.registry, id, id);
            e.runtime.alive = false;
            e.states.clear_score_profile_for_reuse();
        }
        self.restaurant_state(id).feeding = true;
        self.ros_message(id, target, "[0]将菜喂给[1]", u);
        let mut skills: Vec<_> = items.into_iter().filter(|x| x.skill.is_some() || x.invocation.is_some()).collect();
        if skills.is_empty() {
            let n = 1 + self.restaurant_pick(5);
            let mut keys: Vec<_> = (0..35_u8).collect();
            for _ in 0..n {
                let p = self.restaurant_pick(keys.len());
                let key = keys.remove(p);
                skills.push(RestaurantIngredient {
                    label: SKILL_NAMES[key as usize].to_owned(),
                    skill: Some(key),
                    invocation: None,
                    source: None,
                    person: None,
                });
            }
        }
        let result_start = u.updates.len();
        for item in skills {
            if !self.entities.get(target).is_some_and(|e| e.runtime.hp > 0 && e.runtime.alive) {
                break;
            }
            if let Some(call) = item.invocation {
                self.restaurant_replay(&call, target, u);
            } else if let Some(key) = item.skill {
                self.restaurant_cast(id, item.source, key, target, u);
            }
        }
        // 延后到回合收尾才裁切显示，保留本轮机制读取的原始技能消息。
        for event in u.updates.iter_mut().skip(result_start) {
            event.restaurant_recipient = Some(target.0 as usize);
        }
        let team = self.entities.get(dish).unwrap().runtime.team;
        let e = self.entities.get_mut(dish).unwrap();
        e.runtime.hp = 0;
        e.runtime.alive = false;
        self.world.mark_dead(dish, team);
        let s = self.restaurant_state(id);
        s.completed = s.completed.saturating_add(1);
        let done = s.completed >= s.limit;
        if done
            && self
                .world
                .flat_alive()
                .iter()
                .any(|t| self.plain_effective_team(*t) != self.plain_effective_team(id))
        {
            self.restaurant_state(id).stopped = true;
            self.rosmontic_message(id, "[0]发动[尖锐爆鸣]，击倒了自己", u);
            self.entities.get_mut(id).unwrap().runtime.hp = 0;
            self.drain_plain_lethal_damage_into(id, id, u);
        }
    }
}
