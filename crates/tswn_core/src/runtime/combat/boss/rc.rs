//! RC 的抽卡、观察、成长与存活受击反雷共用原生行动和伤害链。
use super::super::*;
use crate::runtime::entity::RosmonticRuntime;

const SMALL_KEY: &str = "小@rc";
const BIG_KEY: &str = "大@rc";

impl CombatRuntime {
    /// 独立身份区分专属召唤物，不影响普通同名角色。
    pub(crate) fn is_rc_small(&self, actor: EntityIdx) -> bool {
        self.entities
            .get(actor)
            .is_some_and(|e| e.runtime.is_minion() && e.template.id_key_name == SMALL_KEY)
    }

    pub(crate) fn is_rc_big(&self, actor: EntityIdx) -> bool {
        self.entities
            .get(actor)
            .is_some_and(|e| e.runtime.is_minion() && e.template.id_key_name == BIG_KEY)
    }

    pub(crate) fn is_rc_boss(&self, actor: EntityIdx) -> bool {
        self.entities
            .get(actor)
            .is_some_and(|e| e.template.name == "rc" && e.runtime.flags.contains(PlayerKindFlags::BOSS))
    }

    pub(crate) fn is_rc_actor(&self, actor: EntityIdx) -> bool {
        self.is_rc_small(actor) || self.is_rc_big(actor) || self.is_rc_boss(actor)
    }

    /// 只取本体拥有、仍存活且 HP 为正的召唤物，换队不改变归属。
    pub(crate) fn rc_children(&self, owner: EntityIdx) -> Vec<EntityIdx> {
        self.entities
            .iter()
            .filter(|(_, e)| e.runtime.owner == owner && e.runtime.is_minion() && e.runtime.alive && e.runtime.hp > 0)
            .map(|(id, _)| id)
            .collect()
    }

    pub(crate) fn rc_small_count(&self, owner: EntityIdx) -> usize {
        self.rc_children(owner).into_iter().filter(|id| self.is_rc_small(*id)).count()
    }

    /// 拒绝多余区间，使技能、目标和数量严格等概率。
    fn rc_pick(&mut self, count: usize) -> usize {
        assert!(count > 0);
        // 大不限制总数，目标池可能超过一个字节；按需扩展采样区间。
        let range = if count <= 256 { 256_u64 } else { 1_u64 << 32 };
        let bound = range - range % count as u64;
        loop {
            let value = if count <= 256 {
                u64::from(self.rng.next_u8())
            } else {
                (0..4).fold(0_u64, |v, _| (v << 8) | u64::from(self.rng.next_u8()))
            };
            if value < bound {
                return (value % count as u64) as usize;
            }
        }
    }

    /// 召唤资格在施法前检查，满足资格就完整抽取，不再截断到硬上限。
    pub(crate) fn rc_draw(&mut self, actor: EntityIdx, paid: bool, updates: &mut RunUpdates) {
        if self.rc_small_count(actor) >= 3 {
            return;
        }
        let count = (if paid { 3 } else { 1 }) + self.rc_pick(3);
        let text = if paid {
            format!("[0]的同学给[0]氪了个648，抽到{count}个小")
        } else {
            format!("[0]发动[抽卡]，抽到{count}个小")
        };
        updates.add(RunUpdate::new(text, actor.0 as usize, actor.0 as usize, 1).without_replay_target());
        for _ in 0..count {
            self.rc_spawn_small(actor, updates);
        }
    }

    /// 可用技能等概率抽取；小仅魅惑，大固定交替蓄力和雷击。
    pub(crate) fn drain_rc_action_into(&mut self, actor: EntityIdx, updates: &mut RunUpdates) {
        if !self.entities.get(actor).is_some_and(EntityRecord::is_active) {
            return;
        }
        if self.is_rc_small(actor) {
            if let Some(target) = self.select_plain_charm_targets(actor, true).first().copied() {
                self.drain_plain_charm_skill_into(actor, target, updates);
            }
            return;
        }
        if self.is_rc_big(actor) {
            let s = self.ros_state(actor);
            let charge = s.actions % 2 == 0;
            s.actions = s.actions.wrapping_add(1);
            if charge {
                self.drain_plain_charge_skill_into(actor, updates);
            } else if let Some(target) = self.select_plain_default_attack_target(actor, true) {
                self.drain_plain_thunder_skill_into(actor, target, updates);
                self.entities.get_mut(actor).unwrap().clear_charge_runtime();
            }
            return;
        }
        let children = self.rc_children(actor);
        let small_count = self.rc_small_count(actor);
        let mut choices = Vec::new();
        if small_count < 3 {
            choices.extend([0, 1]);
        }
        if !children.is_empty() {
            choices.push(2);
        }
        if small_count > 0 {
            choices.push(3);
        }
        if choices.is_empty() {
            return;
        }
        match choices[self.rc_pick(choices.len())] {
            0 => self.rc_draw(actor, false, updates),
            1 => self.rc_draw(actor, true, updates),
            2 => {
                let target = children[self.rc_pick(children.len())];
                self.rc_observe(actor, target, updates);
            }
            _ => self.rc_heal_small(actor, updates),
        }
    }

    /// 外显七围固定 36：内部六围为 0，速度保留行动条偏移 160。
    pub(crate) fn rc_spawn_small(&mut self, actor: EntityIdx, updates: &mut RunUpdates) -> EntityIdx {
        let team = self.entities.get(actor).unwrap().runtime.team;
        let kind = self.registry.player_kind_id_by_export_name(DEFAULT_CORE_SUMMON_KIND_EXPORT).unwrap();
        let mut template = PlayerTemplate::with_kind(0, "小", kind, team, 72, 0);
        template.speed = 160;
        template.attract_bits = 1.0e12_f64.to_bits();
        template.id_key_name = SMALL_KEY.to_owned();
        let idx = self.entities.next_spawn_idx(&template);
        self.effects.push(QueuedEffect::SpawnWithMessage {
            caster: actor,
            template,
            message: "召唤出[1]".to_owned(),
        });
        self.drain_effects_into(updates);
        let charm = self.registry.skill_id_by_export_name(BuiltinActiveSkill::Charm.export_name()).unwrap();
        let e = self.entities.get_mut(idx).unwrap();
        // 原生生成可能重建身份和技能，随后固定专属状态，禁用伤害分摊。
        e.template.id_key_name = SMALL_KEY.to_owned();
        e.template.skills = SkillLoadout::from_skill_levels([(charm, 256)]);
        e.runtime.policies.damage_share = DamageSharePolicy::None;
        e.runtime.rosmontic = Some(Box::new(RosmonticRuntime::default()));
        idx
    }

    /// 观察层数独立保存，驱散、净化和普通状态清理均不修改它。
    pub(crate) fn rc_observe(&mut self, actor: EntityIdx, target: EntityIdx, updates: &mut RunUpdates) {
        if !self.rc_children(actor).contains(&target) {
            return;
        }
        let small = self.is_rc_small(target);
        let big = self.is_rc_big(target);
        let s = self.ros_state(target);
        let previous = s.rc_observations;
        s.rc_observations = s.rc_observations.saturating_add(1);
        updates.add(RunUpdate::new("[0]观察了[1]", actor.0 as usize, target.0 as usize, 1));
        if small && previous == 0 {
            // 施法回合末减一次，保留随后两个自身回合；重复触发刷新而不叠乘。
            self.ros_state(actor).rc_boost_turns = 3;
            self.entities.get_mut(actor).unwrap().refresh_runtime_stats_from_template();
            let quote = if self.rng.c50() { "这确实太小了" } else { "有点小" };
            self.boss_dialogue(actor, quote, updates);
        } else if small && previous == 1 {
            // 七围按外显口径翻倍，持久倍率在状态重建后仍保留。
            for i in 0..7 {
                self.ros_stat(target, i, 2.0, if i == 2 { -124 } else { 36 });
            }
            let thunder = self.registry.skill_id_by_export_name(BuiltinActiveSkill::Thunder.export_name()).unwrap();
            let charge = self.registry.skill_id_by_export_name(BuiltinActiveSkill::Charge.export_name()).unwrap();
            let e = self.entities.get_mut(target).unwrap();
            // 成长后的技能表同步为蓄力和雷击，供吞噬等读取技能的机制使用。
            e.template.skills = SkillLoadout::from_skill_levels([(charge, 256), (thunder, 256)]);
            e.template.name = "大".to_owned();
            e.template.display_name = "大".to_owned();
            e.template.id_key_name = BIG_KEY.to_owned();
            e.template.max_hp *= 2;
            e.runtime.hp *= 2;
            e.runtime.rosmontic.as_mut().unwrap().actions = 0;
            updates.add(RunUpdate::new("[1]变成了[大]", actor.0 as usize, target.0 as usize, 1));
            let quote = if self.rng.c50() { "还是大了" } else { "我觉得大了" };
            self.boss_dialogue(actor, quote, updates);
        } else if big {
            // 使用原生狂暴而非愤怒威力状态，接下来两次行动由原生流程接管。
            let e = self.entities.get_mut(actor).unwrap();
            if !e.states.set_payload(PLAIN_BERSERK_STATE_KEY, StatePayload::Berserk { step: 2 }) {
                e.states.add_entry(StateEntry::berserk(PLAIN_BERSERK_STATE_KEY, 2));
            }
            updates.add(RunUpdate::new("[0]进入[狂暴]状态", actor.0 as usize, actor.0 as usize, 2));
            self.boss_dialogue(actor, "一个两个的怎么都是大雷，我愤怒了", updates);
        }
    }

    /// 以当前 HP 最低为准，治疗存活的小；不治疗大、尸体或其他 RC 的召唤物。
    pub(crate) fn rc_heal_small(&mut self, actor: EntityIdx, updates: &mut RunUpdates) {
        let Some(target) = self
            .rc_children(actor)
            .into_iter()
            .filter(|id| self.is_rc_small(*id))
            .min_by_key(|id| self.entities.get(*id).unwrap().runtime.hp)
        else {
            return;
        };
        let e = self.entities.get_mut(target).unwrap();
        let healed = e.template.max_hp - e.runtime.hp;
        e.runtime.hp = e.template.max_hp;
        updates.add(RunUpdate::new(
            "[0]使用[我突然有点担心小]",
            actor.0 as usize,
            target.0 as usize,
            1,
        ));
        updates.add(RunUpdate::new(
            "[1]回复体力[2]点",
            actor.0 as usize,
            target.0 as usize,
            healed.max(0) as u32,
        ));
    }

    /// 仅实际扣血后且仍存活的小反雷；任意 RC 本体的攻击与反雷链均排除。
    pub(crate) fn rc_thunder_on_attack(&mut self, target: EntityIdx, attacker: EntityIdx, updates: &mut RunUpdates) {
        if target == attacker || !self.is_rc_small(target) || self.is_rc_boss(attacker) {
            return;
        }
        let countering = |id| {
            self.entities
                .get(id)
                .and_then(|e| e.runtime.rosmontic.as_ref())
                .is_some_and(|s| s.countering)
        };
        if countering(target)
            || countering(attacker)
            || !self.entities.get(target).is_some_and(|e| e.runtime.alive && e.runtime.hp > 0)
            || !self.entities.get(attacker).is_some_and(EntityRecord::is_active)
        {
            return;
        }
        self.ros_state(target).countering = true;
        self.drain_plain_thunder_skill_into(target, attacker, updates);
        self.ros_state(target).countering = false;
    }
}
