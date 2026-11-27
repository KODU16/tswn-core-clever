//! 紫、AL1S 与兔狐小队的逐槽行动选择和公共状态。
use super::super::*;
impl CombatRuntime {
    /// 支援只在自己的战斗队伍内找人，避免两边同名 Boss 串队。
    pub(super) fn squad_member(&self, actor: EntityIdx, name: &str) -> Option<EntityIdx> {
        let team = self.plain_effective_team(actor);
        // 同队可同时放入两种难度，部署与复活只关联对应版本。
        let original = self.entities.get(actor).is_some_and(|e| e.template.id_key_name.ends_with("_@!"));
        self.entities
            .iter()
            .find(|(id, e)| {
                self.plain_effective_team(*id) == team
                    && e.template.name == name
                    && e.template.id_key_name.ends_with("_@!") == original
                    && e.runtime.flags.contains(PlayerKindFlags::BOSS)
            })
            .map(|(id, _)| id)
    }
    pub(super) fn remaining_children(&self, actor: EntityIdx, role: u8) -> Vec<EntityIdx> {
        self.entities
            .iter()
            .filter(|(_, e)| {
                e.runtime.owner == actor && e.runtime.hp > 0 && e.runtime.rosmontic.as_ref().is_some_and(|s| s.child_kind == role)
            })
            .map(|(id, _)| id)
            .collect()
    }
    pub(crate) fn ros_power(&mut self, actor: EntityIdx, factor: f64, add: f64) {
        let s = self.ros_state(actor);
        let old = if s.power_bits == 0 {
            1.0
        } else {
            f64::from_bits(s.power_bits)
        };
        s.power_bits = (old * factor + add).to_bits();
        self.entities.get_mut(actor).unwrap().refresh_runtime_stats_from_template();
    }
    pub(super) fn ros_attract(&mut self, actor: EntityIdx, factor: f64) {
        let s = self.ros_state(actor);
        let old = if s.attract_scale_bits == 0 {
            1.0
        } else {
            f64::from_bits(s.attract_scale_bits)
        };
        s.attract_scale_bits = (old * factor).to_bits();
        self.entities.get_mut(actor).unwrap().refresh_runtime_stats_from_template();
    }
    pub(crate) fn ros_advance(&mut self, actor: EntityIdx, points: i32) {
        self.entities.get_mut(actor).unwrap().runtime.move_state.speed_points += points;
    }
    pub(super) fn squad_blind(&mut self, actor: EntityIdx, target: EntityIdx, updates: &mut RunUpdates) -> bool {
        // 闪光本身也需要命中；目盲期间不能再向对手施加目盲。
        if self.miss_squad_blinded_attack(actor, target, updates) {
            return false;
        }
        // 队长的 FOXTROT 用攻击对敏捷与速度，其余闪光用魔法对抗性。
        let physical = self.is_rosmontic_boss(actor, "fox1");
        let r = &self.entities.get(actor).unwrap().runtime;
        let a = if physical { r.attack } else { r.magic };
        if self.ros_immune(target, "blindly") {
            return false;
        }
        let r = &self.entities.get(target).unwrap().runtime;
        if self.roll_plain_dodge(target, a, r.agility + if physical { r.speed } else { r.resistance }, false) {
            return false;
        }
        let duration = self.entities.get(actor).and_then(|e| e.runtime.rosmontic.as_ref()).map_or(3, |s| {
            if s.squad_blind_duration == 0 {
                3
            } else {
                s.squad_blind_duration
            }
        });
        let s = self.ros_state(target);
        s.blind_turns = s.blind_turns.saturating_add(duration);

        self.entities.get_mut(target).unwrap().refresh_runtime_stats_from_template();
        self.ros_message(actor, target, "[1]陷入[目盲]", updates);
        true
    }
    pub(super) fn squad_taunt(&mut self, actor: EntityIdx) {
        self.ros_state(actor).taunt_turns = 4;
        self.entities.get_mut(actor).unwrap().refresh_runtime_stats_from_template();
    }
    /// 最后一批拥有专属普攻，不能退回通用的技能攻击。
    pub(super) fn remaining_default(&mut self, actor: EntityIdx, updates: &mut RunUpdates) {
        let name = self.entities.get(actor).unwrap().template.name.clone();
        if name == "AL1S" {
            let (phase, energy) = {
                let s = self.ros_state(actor);
                (s.phase, s.energy)
            };
            if phase == 2 {
                let e = self.entities.get(actor).unwrap();
                let heal = (f64::from(e.template.max_hp - e.runtime.hp) * 0.6).ceil() as i32;
                self.apply_plain_absorb_heal_into(actor, actor, heal, updates);
                return;
            }
            self.rosmontic_message(actor, "[0]发射[光之剑]", updates);
            let atp = self.ros_atp(actor, true) * (1.5 + 0.5 * f64::from(energy));
            for t in self.ros_enemies(actor, 2) {
                self.ros_hit(actor, t, true, atp, false, PlainAttackOnDamage::None, updates);
            }
            self.ros_state(actor).energy = (energy + 1).min(2);
            return;
        }
        let Some(mut t) = self.select_plain_default_attack_target(actor, true) else {
            return;
        };
        match name.as_str() {
            "fox4" => {
                let old = self.ros_state(actor).charge_target;
                if let Some(id) = old.filter(|id| self.entities.get(*id).is_some_and(|e| e.runtime.hp > 0)) {
                    t = id;
                    self.ros_state(actor).aim += 1;
                } else {
                    let s = self.ros_state(actor);
                    s.charge_target = Some(t);
                    s.aim = 0;
                }
                let aim = self.ros_state(actor).aim;
                if aim == 0 {
                    self.ros_message(actor, t, "[0]瞄准了[1]", updates);
                    self.ros_advance(actor, 1024);
                } else {
                    self.ros_message(actor, t, "[0]发起[精准狙击]", updates);
                    self.ros_stat(t, 1, 0.5, 0);
                    let atp = self.ros_atp(actor, false) * f64::from(aim);
                    self.ros_hit(actor, t, false, atp, true, PlainAttackOnDamage::None, updates);
                }
            }
            "ribbit4" => {
                let yin = self.ros_state(actor).conceal;
                let locked = self
                    .entities
                    .get(t)
                    .unwrap()
                    .runtime
                    .rosmontic
                    .as_ref()
                    .is_some_and(|s| s.locked_turns > 0);
                let atp = self.ros_atp(actor, false) * (0.75 + 0.5 * f64::from(yin)) * if locked { 1.5 } else { 1.0 };
                self.ros_message(actor, t, "[0]发起[狙击]", updates);
                self.ros_hit(actor, t, false, atp, yin > 2, PlainAttackOnDamage::None, updates);
            }
            "fox3" => {
                let shieldless = self.ros_state(actor).shieldless;
                let (def, agi) = {
                    let e = self.entities.get(t).unwrap();
                    (e.runtime.defense, e.runtime.agility)
                };
                if shieldless {
                    let r = &mut self.entities.get_mut(t).unwrap().runtime;
                    r.defense = 0;
                    r.agility = 0;
                }
                let atp = self.ros_atp(actor, false);
                self.ros_message(
                    actor,
                    t,
                    if shieldless {
                        "[0]使用[双连击]"
                    } else {
                        "[0]发起攻击"
                    },
                    updates,
                );
                for _ in 0..if shieldless { 2 } else { 1 } {
                    self.ros_hit(actor, t, false, atp, false, PlainAttackOnDamage::None, updates);
                }
                let r = &mut self.entities.get_mut(t).unwrap().runtime;
                r.defense = def;
                r.agility = agi;
            }
            "fox1" => {
                let old = self
                    .entities
                    .get(t)
                    .unwrap()
                    .runtime
                    .rosmontic
                    .as_ref()
                    .is_some_and(|s| s.blind_turns > 0);
                if self.squad_blind(actor, t, updates) {
                    let atp = self.ros_atp(actor, false) * if old { 1.8 } else { 1.2 };
                    self.ros_hit(actor, t, false, atp, false, PlainAttackOnDamage::None, updates);
                } else {
                    self.ros_advance(actor, 512);
                }
            }
            _ => self.ros_default(actor, updates),
        }
    }
    /// 原版按顺序逐槽抽样；主动技的条件在扣 MP 前检查。
    pub(super) fn remaining_action(&mut self, actor: EntityIdx, updates: &mut RunUpdates) -> bool {
        let e = self.entities.get(actor).unwrap();
        let role = e.runtime.rosmontic.as_ref().map_or(0, |s| s.child_kind);
        if role >= 30 {
            self.remaining_child_action(actor, role, updates);
            return true;
        }
        if !e.runtime.flags.contains(PlayerKindFlags::BOSS) {
            return false;
        }
        let name = e.template.name.clone();
        let order: &[usize] = match name.as_str() {
            "yukari" => &[1, 2, 3, 0, 4],
            "AL1S" => &[5, 1, 4, 3, 2],
            "fox1" => &[1, 0, 5, 2],
            "fox2" => &[2, 1, 3, 0],
            "fox3" => &[1, 0],
            "fox4" => &[1, 0],
            "ribbit1" => &[0, 2, 1],
            "ribbit2" => &[3, 2, 1, 0],
            "ribbit3" => &[0, 1, 2],
            "ribbit4" => &[1],
            _ => return false,
        };
        if let Some(skill) = self.ros_state(actor).charged_skill.take() {
            self.remaining_cast(actor, skill, true, updates);
            return true;
        }
        let cost = 8 + i32::from(self.rng.next_u8() & 15);
        if self.entities.get(actor).unwrap().runtime.magic_point >= cost {
            for &skill in order {
                if !self.remaining_allowed(actor, skill) {
                    continue;
                }
                let level = self.ros_state(actor).levels[skill];
                if level <= 0 || self.rng.r127() as i32 >= level {
                    continue;
                }
                self.entities.get_mut(actor).unwrap().runtime.magic_point -= cost;
                self.remaining_cast(actor, skill, false, updates);
                return true;
            }
        }
        self.remaining_default(actor, updates);
        true
    }
    pub(super) fn remaining_allowed(&self, actor: EntityIdx, skill: usize) -> bool {
        let e = self.entities.get(actor).unwrap();
        let s = e.runtime.rosmontic.as_ref().unwrap();
        match (e.template.name.as_str(), skill) {
            ("yukari", 1) => s.barrier_turns == 0,
            ("yukari", 3) => s.boundary < 2,
            ("yukari", 4) => s.boundary >= 2 && s.death_boundary,
            ("AL1S", 1 | 2) => s.phase == 3,
            ("AL1S", 3) => s.phase != 2 && s.energy >= 2,
            ("AL1S", 4) => s.phase == 2 && self.remaining_children(actor, 30).len() >= 2,
            ("AL1S", 5) => s.phase == 2,
            ("fox1", 1) => {
                s.last_stand <= 1
                    && (2..=4).any(|n| {
                        self.squad_member(actor, &format!("fox{n}")).is_some_and(|id| {
                            let e = self.entities.get(id).unwrap();
                            e.runtime.hp <= 0 && e.runtime.corpse == RuntimeCorpseKind::None
                        })
                    })
            }
            ("fox1", 2) => {
                s.levels[1] == 0
                    && (2..=4).all(|n| {
                        self.squad_member(actor, &format!("fox{n}"))
                            .is_none_or(|id| self.entities.get(id).unwrap().runtime.hp <= 0)
                    })
            }
            ("fox2", 1) => s.summon_count < 4 && self.remaining_children(actor, 34).is_empty(),
            ("fox2", 3) => !self.select_squad_heal(actor).is_empty(),
            ("fox3", 1) => !s.shieldless,
            ("fox4", 0) => self.remaining_children(actor, 33).is_empty(),
            ("fox4", 1) => s.charge_target.is_some_and(|id| self.entities.get(id).is_some_and(|e| e.runtime.hp > 0)),
            ("ribbit1", 1) => self.remaining_children(actor, 32).is_empty(),
            ("ribbit3", 0) => s.locked,
            ("ribbit3", 2) => !self.remaining_children(actor, 31).is_empty(),
            _ => true,
        }
    }
    pub(super) fn select_squad_heal(&self, actor: EntityIdx) -> Vec<EntityIdx> {
        let team = self.plain_effective_team(actor);
        self.world
            .flat_alive()
            .iter()
            .copied()
            .filter(|id| {
                let e = self.entities.get(*id).unwrap();
                self.plain_effective_team(*id) == team && !e.runtime.is_combat_minion() && e.runtime.hp < e.template.max_hp
            })
            .collect()
    }
}
