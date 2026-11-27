//! 涵虚的主动技能、实际失血被动和菜的倒计时均进入原生战斗快照。
use super::super::*;
use crate::runtime::entity::RosmonticRuntime;
const VEGETABLE: &str = "菜@ReturnVoid";

impl CombatRuntime {
    pub(crate) fn is_return_void_actor(&self, id: EntityIdx) -> bool {
        self.is_rosmontic_boss(id, "ReturnVoid")
            || self
                .entities
                .get(id)
                .is_some_and(|e| e.runtime.is_minion() && e.template.id_key_name == VEGETABLE)
    }
    pub(crate) fn rv_immune(&self, id: EntityIdx) -> bool {
        self.is_rosmontic_boss(id, "ReturnVoid")
            && self
                .entities
                .get(id)
                .and_then(|e| e.runtime.rosmontic.as_ref())
                .is_some_and(|s| s.rv_dodge_turns > 0)
    }
    /// 累计实际失血；治疗不回退层数，自损和交换生命也能增加层数。
    pub(crate) fn rv_record_loss(&mut self, id: EntityIdx, loss: i32) {
        if loss <= 0 || !self.is_rosmontic_boss(id, "ReturnVoid") {
            return;
        }
        let s = self.ros_state(id);
        s.rv_loss = s.rv_loss.saturating_add(loss as u64);
        self.entities.get_mut(id).unwrap().refresh_runtime_stats_from_template();
    }
    /// 只对涵虚向更低当前 HP 的目标造成的伤害减伤；反射伤害保持两倍实际值。
    pub(crate) fn return_void_damage(&self, caster: EntityIdx, target: EntityIdx, amount: i32) -> i32 {
        if amount > 0 && caster != target && self.is_rosmontic_boss(caster, "ReturnVoid") {
            let c = self.entities.get(caster).unwrap();
            if !c.runtime.rosmontic.as_ref().is_some_and(|s| s.rv_reflect_chain)
                && self.entities.get(target).is_some_and(|t| t.runtime.hp < c.runtime.hp)
            {
                return amount / 4;
            }
        }
        amount
    }
    /// 反伤链不再触发反伤或吸血，避免两名涵虚相互反射无限递归。
    pub(crate) fn return_void_after_damage(&mut self, caster: EntityIdx, target: EntityIdx, actual: i32, u: &mut RunUpdates) {
        if actual <= 0 {
            return;
        }
        self.rv_record_loss(target, actual);
        let chain = [caster, target].into_iter().any(|id| {
            self.entities
                .get(id)
                .and_then(|e| e.runtime.rosmontic.as_ref())
                .is_some_and(|s| s.rv_reflect_chain)
        });
        if chain || caster == target {
            return;
        }
        if self.is_rosmontic_boss(caster, "ReturnVoid") && self.plain_effective_team(caster) != self.plain_effective_team(target)
        {
            let c = self.entities.get(caster).unwrap();
            if c.runtime.hp > 0 {
                // 低于半血的比例连续换算吸血，不把溢出伤害算作敌方损血。
                let ratio = (0.5 - f64::from(c.runtime.hp) / f64::from(c.template.max_hp.max(1))).max(0.0) * 2.0;
                let heal = (f64::from(actual) * ratio) as i32;
                if heal > 0 {
                    self.apply_plain_absorb_heal_into(caster, caster, heal, u);
                }
            }
        }
        let reflect = self.is_rosmontic_boss(target, "ReturnVoid") && self.ros_state(target).rv_reflect;
        if reflect && self.entities.get(caster).is_some_and(|e| e.runtime.hp > 0) {
            self.ros_state(target).rv_reflect_chain = true;
            self.rosmontic_message(target, "[0]的[天赋深不可测]反射伤害", u);
            if self.apply_plain_legacy_damage_into(target, caster, actual.saturating_mul(2), u) {
                self.drain_plain_lethal_damage_into(target, caster, u);
            }
            self.ros_state(target).rv_reflect_chain = false;
        }
    }
    /// 护符依原版效果抵挡一次死亡；不会额外触发翻面。
    pub(crate) fn rv_reraise(&mut self, id: EntityIdx, u: &mut RunUpdates) -> bool {
        if !self.is_rosmontic_boss(id, "ReturnVoid")
            || self.entities.get(id).unwrap().runtime.hp > 0
            || self.ros_state(id).rv_charms == 0
        {
            return false;
        }
        self.ros_state(id).rv_charms -= 1;
        let hp = self.rng.r16() as i32;
        let e = self.entities.get_mut(id).unwrap();
        e.runtime.hp = hp.max(1);
        e.runtime.alive = true;
        self.rosmontic_message(id, "[0]的[护身符]抵挡了死亡", u);
        true
    }
    /// 开始下一次自身行动时结束上一回合反伤，菜在第三次后续行动开始时投掷。
    pub(crate) fn begin_return_void_action(&mut self, id: EntityIdx, u: &mut RunUpdates) {
        if !self.is_rosmontic_boss(id, "ReturnVoid") {
            return;
        }
        self.ros_state(id).rv_reflect = false;
        let children: Vec<_> = self
            .entities
            .iter()
            .filter(|(_, e)| e.runtime.owner == id && e.template.id_key_name == VEGETABLE && e.runtime.hp > 0 && e.runtime.alive)
            .map(|(idx, _)| idx)
            .collect();
        for child in children {
            let s = self.ros_state(child);
            s.rv_vegetable_turns = s.rv_vegetable_turns.saturating_sub(1);
            if s.rv_vegetable_turns > 0 {
                continue;
            }
            let hp = self.entities.get(child).unwrap().runtime.hp;
            if let Some(target) = self.ros_enemies(id, 1).first().copied() {
                self.ros_message(id, target, "[0]将[菜]丢向[1]", u);
                if self.apply_plain_legacy_damage_into(id, target, hp, u) {
                    self.drain_plain_lethal_damage_into(id, target, u);
                }
            }
            self.entities.get_mut(child).unwrap().runtime.hp = 0;
            self.drain_summon_explode_self_death_into(child, u);
        }
    }
    pub(crate) fn end_return_void_action(&mut self, id: EntityIdx) {
        if !self.is_rosmontic_boss(id, "ReturnVoid") {
            return;
        }
        let s = self.ros_state(id);
        s.rv_mode_turns = s.rv_mode_turns.saturating_sub(1);
        s.rv_dodge_turns = s.rv_dodge_turns.saturating_sub(1);
        if s.rv_mode_turns == 0 {
            s.rv_mode = 0;
        }
        self.entities.get_mut(id).unwrap().refresh_runtime_stats_from_template();
    }
    /// 七个主动技能等概率选择，菜本身不执行普通行动。
    pub(crate) fn drain_return_void_action_into(&mut self, id: EntityIdx, u: &mut RunUpdates) {
        if !self.is_rosmontic_boss(id, "ReturnVoid") || !self.entities.get(id).is_some_and(EntityRecord::is_active) {
            return;
        }
        let choice = loop {
            let n = self.rng.next_u8();
            if n < 252 {
                break n % 7;
            }
        };
        self.rv_skill(id, choice, u);
    }
    pub(crate) fn rv_skill(&mut self, id: EntityIdx, choice: u8, u: &mut RunUpdates) {
        const NAMES: [&str; 7] = [
            "[0]使用[涵虚不等式]",
            "[0]使用[粗细涵伯]",
            "[0]使用[咸鱼玩家]",
            "[0]使用[送菜]",
            "[0]使用[蔺嘘不知道哦]",
            "[0]使用[涵仲恐怕不同意]",
            "[0]使用[天赋深不可测]",
        ];
        self.rosmontic_message(id, NAMES[choice as usize], u);
        match choice {
            0 => {
                let targets = self.ros_enemies(id, usize::MAX);
                if let Some(t) = targets.into_iter().min_by_key(|t| self.entities.get(*t).unwrap().runtime.hp) {
                    let a = self.entities.get(id).unwrap().runtime.hp;
                    let b = self.entities.get(t).unwrap().runtime.hp;
                    self.entities.get_mut(id).unwrap().runtime.hp = b;
                    self.entities.get_mut(t).unwrap().runtime.hp = a;
                    self.rv_record_loss(id, (a - b).max(0));
                    self.rv_record_loss(t, (b - a).max(0));
                    let s = self.ros_state(id);
                    let scale = (1.0 - f64::from(s.rv_inequalities) * 0.05).max(0.2);
                    s.rv_inequalities = s.rv_inequalities.saturating_add(1);
                    let sp = (f64::from((a - b).max(0)) * 100.0 * scale) as i32;
                    let e = self.entities.get_mut(id).unwrap();
                    e.runtime.move_state.speed_points = e.runtime.move_state.speed_points.saturating_add(sp);
                    u.add(RunUpdate::new("[0]与[1]交换了当前生命值", id.0 as usize, t.0 as usize, 1));
                    u.add(RunUpdate::new("[0]获得行动条[2]点", id.0 as usize, id.0 as usize, sp as u32));
                }
            }
            1 => {
                let e = self.entities.get(id).unwrap();
                let coarse = i64::from(e.runtime.hp) * 5 < i64::from(e.template.max_hp) || self.rng.c50();
                let s = self.ros_state(id);
                s.rv_mode = if coarse { 1 } else { 2 };
                s.rv_mode_turns = 4;
                self.rosmontic_message(id, if coarse { "[0]切换为[粗]" } else { "[0]切换为[细]" }, u);
            }
            2 => {
                let s = self.ros_state(id);
                s.rv_back = !s.rv_back;
                s.rv_face_bonus = true;
                s.rv_charms = s.rv_charms.saturating_add(1);
                let magic = s.rv_back;
                self.entities.get_mut(id).unwrap().refresh_runtime_stats_from_template();
                self.rosmontic_message(
                    id,
                    if magic {
                        "[0]咸鱼翻身为[反面]"
                    } else {
                        "[0]咸鱼翻身为[正面]"
                    },
                    u,
                );
                if let Some(t) = self.ros_enemies(id, 1).first().copied() {
                    for _ in 0..if magic { 3 } else { 1 } {
                        let atp = self.ros_atp(id, magic) * if magic { 0.7 } else { 2.0 };
                        self.ros_hit(id, t, magic, atp, false, PlainAttackOnDamage::None, u);
                    }
                }
            }
            3 => {
                let count = self.ros_enemies(id, usize::MAX).len().div_ceil(2);
                for _ in 0..count {
                    self.rv_spawn_vegetable(id, u);
                }
            }
            4 => {
                self.ros_state(id).rv_dodge_turns = 6;
            }
            5 => {
                let e = self.entities.get_mut(id).unwrap();
                let loss = e.runtime.hp / 2;
                e.runtime.hp -= loss;
                self.rv_record_loss(id, loss);
                let s = self.ros_state(id);
                s.rv_charms = s.rv_charms.saturating_add(2);
                let n = loop {
                    let x = self.rng.next_u8();
                    if x < 255 {
                        break x % 3 + 1;
                    }
                };
                for _ in 0..n {
                    if self.entities.get(id).is_some_and(EntityRecord::is_active) {
                        self.ros_default(id, u);
                    }
                }
            }
            _ => {
                self.ros_state(id).rv_reflect = true;
            }
        }
        self.entities.get_mut(id).unwrap().refresh_runtime_stats_from_template();
    }
    /// 菜的实际战斗属性为零，禁用召唤物默认分摊伤害，只保留可被攻击的实体。
    pub(crate) fn rv_spawn_vegetable(&mut self, id: EntityIdx, u: &mut RunUpdates) -> EntityIdx {
        let team = self.entities.get(id).unwrap().runtime.team;
        let kind = self.registry.player_kind_id_by_export_name(DEFAULT_CORE_SUMMON_KIND_EXPORT).unwrap();
        let mut t = PlayerTemplate::with_kind(0, "菜", kind, team, 160, 0);
        t.speed = 0;
        t.id_key_name = VEGETABLE.to_owned();
        let child = self.entities.next_spawn_idx(&t);
        self.effects.push(QueuedEffect::SpawnWithMessage {
            caster: id,
            template: t,
            message: "召唤出[1]".to_owned(),
        });
        self.drain_effects_into(u);
        let e = self.entities.get_mut(child).unwrap();
        e.template.id_key_name = VEGETABLE.to_owned();
        e.runtime.policies.damage_share = DamageSharePolicy::None;
        e.runtime.rosmontic = Some(Box::new(RosmonticRuntime {
            rv_vegetable_turns: 3,
            ..Default::default()
        }));
        child
    }
}
