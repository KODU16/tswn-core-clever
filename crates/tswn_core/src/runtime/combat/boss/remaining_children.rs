//! 机械支援和游戏开发部伙伴均走原生召唤物死亡、伤害与归属链。
use super::super::*;
// 自动记录实际技能入口，菜品直接重用此处实现。
#[tswn_skill_capture::capture_skills(remaining_children)]
impl CombatRuntime {
    pub(super) fn remaining_child_action(&mut self, a: EntityIdx, role: u8, u: &mut RunUpdates) {
        // 掩体、地雷和支援直升机没有主动普攻。
        if matches!(role, 31 | 34 | 35) {
            return;
        }
        if role == 38 && self.rng.r127() < 70 {
            let team = self.plain_effective_team(a);
            let target = self.world.flat_alive().iter().copied().find(|id| {
                let e = self.entities.get(*id).unwrap();
                self.plain_effective_team(*id) == team && e.runtime.hp * 4 < e.template.max_hp * 3
            });
            if let Some(t) = target {
                let heal = (self.ros_atp(a, true) / 10.0).ceil() as i32;
                self.apply_plain_absorb_heal_into(a, t, heal, u);
                self.ros_cleanse(t);
                return;
            }
        }
        let Some(t) = self.select_plain_default_attack_target(a, true) else {
            return;
        };
        match role {
            30 => self.ros_default(a, u),
            32 => {
                self.ros_message(a, t, "[0]使用[强光]", u);
                self.squad_blind(a, t, u);
            }
            33 => {
                self.ros_stat(t, 1, 0.75, 0);
                self.ros_stat(t, 5, 0.75, 0);
                let atp = self.ros_atp(a, false);
                self.ros_message(a, t, "[0]发起射击", u);
                self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                let owner = self.entities.get(a).unwrap().runtime.owner;
                self.ros_advance(owner, 512);
            }
            36 => {
                let special = self.ros_state(a).locked;
                if special {
                    self.ros_state(a).locked = false;
                }
                let count = if special { 4 } else { 2 + usize::from(self.rng.next_u8() & 1) };
                let factor = if special {
                    4.0
                } else {
                    let n = 2 + (self.rng.next_u8() & 3);
                    if n == 5 {
                        self.ros_state(a).locked = true;
                    }
                    f64::from(n) * 0.5
                };
                let atp = self.ros_atp(a, true) * factor;
                for target in self.ros_enemies(a, count) {
                    self.ros_hit(a, target, false, atp, special, PlainAttackOnDamage::None, u);
                }
            }
            37 => {
                if self.rng.r127() < 40 {
                    let atp = self.ros_atp(a, true) * 1.25;
                    for target in self.ros_enemies(a, 3) {
                        let hit = self.ros_hit(a, target, false, atp, false, PlainAttackOnDamage::None, u);
                        if hit > 0 {
                            self.apply_rosmontic_burn(a, target, hit, u);
                        }
                    }
                } else {
                    let n = 3 + (self.rng.next_u8() & 1);
                    for _ in 0..n {
                        let ar = &self.entities.get(a).unwrap().runtime;
                        let tr = &self.entities.get(t).unwrap().runtime;
                        let acc = 100 + ar.agility;
                        let dodge = tr.defense + tr.agility;
                        if self.roll_plain_dodge(t, acc, dodge, false) {
                            break;
                        }
                        let atp = self.ros_atp(a, false) * 1.5;
                        self.ros_hit(a, t, true, atp, true, PlainAttackOnDamage::None, u);
                    }
                }
            }
            38 => {
                let atp = self.ros_atp(a, false) * 3.25;
                self.ros_hit(a, t, false, atp, true, PlainAttackOnDamage::None, u);
            }
            _ => {}
        }
    }
}
