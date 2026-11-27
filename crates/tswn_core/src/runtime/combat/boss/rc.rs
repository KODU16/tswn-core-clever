//! RC 仅召唤“小”；“小”仅魅惑，并在受攻击时以原生雷击术反击。
use super::super::*;
use crate::namerena::{NamerenaInput, PreparedRoster};
use crate::runtime::entity::RosmonticRuntime;

pub const SMALL_SOURCE: &str = "Bascor cW1JWDuv7f@RbCl";
const SMALL_KEY: &str = "小@rc";

impl CombatRuntime {
    /// 独立身份键区分“小”和普通同名角色，不借用 Boss 免疫或召唤物技能表。
    pub(crate) fn is_rc_small(&self, actor: EntityIdx) -> bool {
        self.entities
            .get(actor)
            .is_some_and(|e| e.runtime.is_minion() && e.template.id_key_name == SMALL_KEY)
    }

    pub(crate) fn is_rc_actor(&self, actor: EntityIdx) -> bool {
        self.is_rc_small(actor)
            || self
                .entities
                .get(actor)
                .is_some_and(|e| e.template.name == "rc" && e.runtime.flags.contains(PlayerKindFlags::BOSS))
    }

    /// 专属行动接管整次出手，无目标时也不回退普通攻击。
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
        let text = if self.rng.c50() {
            "[0]使用[有点小]"
        } else {
            "[0]使用[这确实太小了]"
        };
        updates.add(RunUpdate::new(text, actor.0 as usize, actor.0 as usize, 1).without_replay_target());
        // 使用引擎等概率的三值抽样，每次召唤 1～3 只，不额外设置数量上限。
        let count = loop {
            let value = self.rng.r3();
            if value < 3 {
                break 1 + value as usize;
            }
        };
        for _ in 0..count {
            self.rc_spawn_small(actor, updates);
        }
    }

    /// 从指定完整名字构建原始属性，只将最大 HP 折半，技能替换为魅惑。
    pub(crate) fn rc_spawn_small(&mut self, actor: EntityIdx, updates: &mut RunUpdates) -> EntityIdx {
        let input = NamerenaInput::parse(SMALL_SOURCE).expect("固定召唤物名字应有效");
        let roster = PreparedRoster::build(&input, crate::namerena::eval_name::DEFAULT_EVAL_RQ).expect("固定召唤物属性应可构建");
        let s = &roster.players[0].status;
        let team = self.entities.get(actor).unwrap().runtime.team;
        let kind = self.registry.player_kind_id_by_export_name(DEFAULT_CORE_SUMMON_KIND_EXPORT).unwrap();
        let mut template = PlayerTemplate::with_kind(0, "小", kind, team, (s.max_hp / 2).max(1), s.attack);
        template.defense = s.defense;
        template.speed = s.speed;
        template.agility = s.agility;
        template.magic = s.magic;
        template.resistance = s.resistance;
        template.wisdom = s.wisdom;
        template.magic_point = s.magic_point;
        template.attr_sum = s.attr_sum;
        template.atk_sum = s.atk_sum;
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
        // 原生召唤可能重建技能与身份，生成后固定专属键、技能和反击状态。
        e.template.id_key_name = SMALL_KEY.to_owned();
        e.template.skills = SkillLoadout::from_skill_levels([(charm, 256)]);
        e.runtime.policies.damage_share = DamageSharePolicy::None;
        e.runtime.rosmontic = Some(Box::new(RosmonticRuntime::default()));
        idx
    }

    /// 每次成功进入受击链施放一次完整雷击术；反击来源不再连锁触发反击。
    pub(crate) fn rc_thunder_on_attack(&mut self, target: EntityIdx, attacker: EntityIdx, updates: &mut RunUpdates) {
        if target == attacker || !self.is_rc_small(target) {
            return;
        }
        let countering = |id| {
            self.entities
                .get(id)
                .and_then(|e| e.runtime.rosmontic.as_ref())
                .is_some_and(|s| s.countering)
        };
        if countering(target)
            || (self.is_rc_small(attacker) && countering(attacker))
            || !self.entities.get(target).is_some_and(EntityRecord::is_active)
            || !self.entities.get(attacker).is_some_and(EntityRecord::is_active)
        {
            return;
        }
        // 复活或重新施放可能重建运行态，按需恢复反击标记。
        self.entities
            .get_mut(target)
            .unwrap()
            .runtime
            .rosmontic
            .get_or_insert_with(Default::default)
            .countering = true;
        self.drain_plain_thunder_skill_into(target, attacker, updates);
        self.entities.get_mut(target).unwrap().runtime.rosmontic.as_mut().unwrap().countering = false;
    }
}
