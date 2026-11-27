//! 最后一批 Boss 的指令、减伤和分阶段复活，反击标记阻断递归。
use super::super::*;
impl CombatRuntime {
    /// 机械和伙伴使用各自的强免疫名单，不把召唤物伪装成 Boss。
    pub(crate) fn remaining_child_immune(&mut self, target: EntityIdx, key: &str) -> bool {
        let role = self
            .entities
            .get(target)
            .and_then(|e| e.runtime.rosmontic.as_ref())
            .map_or(0, |s| s.child_kind);
        let listed = match role {
            31 | 32 => matches!(
                key,
                "berserk"
                    | "charm"
                    | "ice"
                    | "poison"
                    | "piston"
                    | "curse"
                    | "fire"
                    | "disperse"
                    | "half"
                    | "exchange"
                    | "blindly"
            ),
            33 => matches!(
                key,
                "berserk" | "charm" | "ice" | "poison" | "piston" | "curse" | "disperse" | "half" | "exchange"
            ),
            34 => matches!(
                key,
                "berserk" | "charm" | "poison" | "piston" | "curse" | "half" | "exchange" | "blindly"
            ),
            35 => matches!(
                key,
                "berserk" | "charm" | "ice" | "poison" | "piston" | "curse" | "half" | "exchange" | "blindly"
            ),
            36..=38 => matches!(key, "charm" | "berserk" | "disperse"),
            _ => false,
        };
        listed && self.rng.next_u8() < 240
    }
    pub(super) fn begin_remaining_action(&mut self, a: EntityIdx, u: &mut RunUpdates) {
        if self.is_rosmontic_boss(a, "yukari") {
            self.ros_state(a).gap = true;
        }
        if self.is_rosmontic_boss(a, "ribbit4") {
            let s = self.ros_state(a);
            s.conceal = s.conceal.saturating_add(1);
        }
        if self.is_rosmontic_boss(a, "ribbit3") && self.rng.next_u8() & 1 == 0 {
            let e = self.entities.get(a).unwrap();
            if e.runtime.hp * 2 < e.template.max_hp {
                let heal = e.template.max_hp / 5;
                self.apply_plain_absorb_heal_into(a, a, heal, u);
            }
        }
        if self.is_rosmontic_boss(a, "fox1") && self.ros_state(a).deployed {
            let heli = self.entities.iter().any(|(_, e)| {
                e.runtime.hp > 0
                    && e.runtime.rosmontic.as_ref().is_some_and(|s| s.child_kind == 31)
                    && self.plain_effective_team(e.runtime.owner) != self.plain_effective_team(a)
            });
            if heli {
                if let Some(t) = self.squad_member(a, "fox2") {
                    if self.ros_state(t).levels[0] == 0 {
                        self.ros_state(t).levels[0] = 1024;
                        self.ros_advance(t, 1024);
                    }
                }
            }
            if let Some(t) = self.squad_member(a, "fox3") {
                let s = self.ros_state(t);
                if !s.shieldless && s.levels[0] == 5 && s.levels[1] <= 40 {
                    s.levels[0] = 378;
                }
            }
            if let Some(t) = self.squad_member(a, "fox4") {
                let locked = self
                    .ros_state(t)
                    .charge_target
                    .is_some_and(|id| self.entities.get(id).is_some_and(|e| e.runtime.hp > 0));
                if locked {
                    self.ros_power(t, 1.0, 0.1);
                    self.ros_attract(t, 1.1);
                } else {
                    self.ros_advance(t, 1024);
                }
            }
        }
        if self.is_rosmontic_boss(a, "ribbit1") && self.ros_state(a).deployed {
            let mut flags = self.ros_state(a).commands;
            let e = self.entities.get(a).unwrap();
            if e.runtime.hp * 2 < e.template.max_hp && flags & 1 == 0 {
                flags |= 1;
                self.ros_stat(a, 2, 2.0, 0);
                self.ros_stat(a, 1, 0.0, 0);
            }
            if let Some(t) = self.squad_member(a, "ribbit2") {
                let e = self.entities.get(t).unwrap();
                let high = e.runtime.hp * 2 > e.template.max_hp;
                if high && self.ros_state(t).levels[2] == 0 {
                    self.ros_state(t).levels[2] = 55;
                    if let Some(r3) = self.squad_member(a, "ribbit3") {
                        self.ros_advance(r3, 1024);
                    }
                }
                if !high && flags & 2 == 0 {
                    flags |= 2;
                    self.ros_state(t).levels[2] = 0;
                    self.ros_state(t).levels[1] = 300;
                    self.ros_stat(t, 1, 2.0, 0);
                    self.ros_stat(t, 5, 2.0, 0);
                }
            }
            if let Some(t) = self.squad_member(a, "ribbit3") {
                if !self.remaining_children(t, 31).is_empty() {
                    self.ros_advance(t, 1024);
                } else if self.ros_state(t).levels[1] > 0
                    && flags & 4 == 0
                    && self
                        .squad_member(a, "ribbit2")
                        .is_some_and(|r2| self.entities.get(r2).unwrap().runtime.rosmontic.as_ref().is_some_and(|s| s.plate))
                {
                    flags |= 4;
                    self.ros_state(t).locked = true;
                    self.ros_advance(t, -1024);
                }
            }
            if let Some(t) = self.squad_member(a, "ribbit4") {
                if self.ros_state(t).conceal > 1 && flags & 8 == 0 {
                    flags |= 8;
                    self.ros_stat(t, 2, 0.8, 0);
                    self.ros_stat(t, 0, 2.4, 0);
                    self.ros_state(t).conceal += 2;
                }
            }
            self.ros_state(a).commands = flags;
        }
    }
    pub(super) fn end_remaining_action(&mut self, a: EntityIdx) {
        if self.is_rosmontic_boss(a, "yukari") && self.ros_state(a).barrier_turns == 1 {
            self.ros_attract(a, 1.0 / 1.12);
        }
        let Some(s) = self.entities.get_mut(a).and_then(|e| e.runtime.rosmontic.as_mut()) else {
            return;
        };
        let mut refresh = false;
        for turns in [&mut s.blind_turns, &mut s.locked_turns, &mut s.anger_turns, &mut s.taunt_turns] {
            if *turns > 0 {
                *turns -= 1;
                if *turns == 0 {
                    refresh = true;
                }
            }
        }
        if s.blind_turns == 0 {
            s.blind_divisor = 0;
        }
        if s.locked_turns == 0 {
            s.locked_divisor = 0;
        }
        if refresh {
            self.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
        }
    }
    /// 预防御阶段处理隐蔽、首次回避、地雷引爆与柚子的柜子。
    pub(crate) fn remaining_pre_defend(&mut self, t: EntityIdx, v: &mut RuntimeDefendValue, u: &mut RunUpdates) {
        if v.atp().is_none() {
            return;
        }
        if self.is_rosmontic_boss(t, "ribbit1") && !self.ros_state(t).resolve_used {
            self.ros_state(t).resolve_used = true;
            self.ros_stat(t, 2, 1.3, 0);
            self.ros_stat(t, 3, 1.3, 0);
            v.set_atp(0.0);
            self.rosmontic_message(t, "[0]回避首次攻击", u);
        }
        if self.is_rosmontic_boss(t, "ribbit4") {
            let yin = self.ros_state(t).conceal;
            if u32::from(self.rng.next_u8()) < 96 + u32::from(yin) * 16 {
                v.set_atp(0.0);
                self.rosmontic_message(t, "[0]在[隐蔽]中避开攻击", u);
            }
        }
        let role = self.entities.get(t).unwrap().runtime.rosmontic.as_ref().map_or(0, |s| s.child_kind);
        if role == 35 && !self.ros_state(t).countering {
            self.ros_state(t).countering = true;
            v.set_atp(0.0);
            self.rosmontic_message(t, "[0]被引爆", u);
            self.ros_hit(t, v.caster(), false, 19200.0, false, PlainAttackOnDamage::None, u);
            self.entities.get_mut(t).unwrap().runtime.hp = 0;
            self.drain_plain_lethal_damage_into(t, t, u);
        }
        if role == 36 && self.entities.get(t).unwrap().runtime.hp < 300 && !self.ros_state(t).resolve_used {
            self.ros_state(t).resolve_used = true;
            self.ros_attract(t, 1.0 / 128.0);
            self.entities.get_mut(t).unwrap().runtime.shield = 320;
            self.ros_cleanse(t);
            v.set_atp(0.0);
            self.rosmontic_message(t, "[0]躲进柜子并获得护盾", u);
        }
    }
    /// 以最终伤害执行常驻减伤，死亡前完成反击与阶段阻断。
    pub(crate) fn remaining_defend(&mut self, t: EntityIdx, v: &mut RuntimeDefendValue, u: &mut RunUpdates) -> bool {
        let Some(damage) = v.damage() else { return false };
        let a = v.caster();
        if self.is_rosmontic_boss(t, "yukari") {
            if self.ros_state(t).gap {
                self.ros_state(t).gap = false;
                self.ros_advance(t, -256);
                v.set_damage(0);
                self.rosmontic_message(t, "[0]遁入[隙间]", u);
                return true;
            }
            if self.ros_state(t).barrier_turns > 0 && damage > 0 {
                if damage <= 240 {
                    v.set_damage(1);
                } else {
                    v.set_damage(damage - 240);
                    self.ros_state(t).barrier_turns = 0;
                    self.ros_attract(t, 1.0 / 1.12);
                }
                return false;
            }
        }
        if self.is_rosmontic_boss(t, "AL1S") {
            let phase = self.ros_state(t).phase;
            v.set_damage(damage / if phase == 2 { 4 } else { 2 });
        }
        if self.is_rosmontic_boss(t, "ribbit4") {
            self.ros_state(t).conceal = 0;
        }
        if self.is_rosmontic_boss(t, "fox3") && !self.ros_state(t).shieldless {
            let n = (self.rng.next_u8() & 7) + if self.ros_state(t).taunt_turns > 0 { 2 } else { 0 };
            if n > 4 {
                v.set_damage(0);
                return true;
            }
        }
        if self.is_rosmontic_boss(t, "ribbit3") {
            if let Some(heli) = self.remaining_children(t, 31).first().copied() {
                v.set_damage(0);
                self.ros_message(t, heli, "[1]代为承受伤害", u);
                self.ros_hit(a, heli, false, f64::from(damage) * 128.0, true, PlainAttackOnDamage::None, u);
                return true;
            }
        }
        let counter_active = self
            .entities
            .get(a)
            .and_then(|e| e.runtime.rosmontic.as_ref())
            .is_some_and(|s| s.countering);
        if self.is_rosmontic_boss(t, "ribbit2") {
            let (plate, anger, counter) = {
                let s = self.ros_state(t);
                (s.plate, s.anger_turns > 0, s.countering)
            };
            if damage > 0 && !counter && !counter_active && (anger || self.rng.next_u8() & 3 == 0) {
                self.ros_state(t).countering = true;
                self.rosmontic_message(t, "[0]进行[反制速射]", u);
                self.ros_hit(t, a, false, 14400.0, false, PlainAttackOnDamage::None, u);
                self.ros_state(t).countering = false;
            }
            v.set_damage(if self.entities.get(a).is_some_and(|e| e.runtime.hp <= 0) {
                0
            } else {
                damage * if plate { 25 } else { 75 } / 100
            });
        }
        if self.is_rosmontic_boss(t, "fox1") {
            let hp = self.entities.get(t).unwrap().runtime.hp;
            let (used, stand) = {
                let s = self.ros_state(t);
                (s.resolve_used, s.last_stand)
            };
            if damage >= hp && damage > 0 && !used {
                let s = self.ros_state(t);
                s.resolve_used = true;
                s.last_stand = 4;
                s.countering = true;
                v.set_damage(0);
                self.ros_advance(t, 4096);
                self.ros_power(t, 2.0, 0.0);
                self.rosmontic_message(t, "[0]发动[死线反击]", u);
                if !counter_active {
                    self.ros_hit(t, a, false, 19200.0, false, PlainAttackOnDamage::None, u);
                }
                self.ros_state(t).countering = false;
                return true;
            }
            if stand > 1 || (stand == 1 && self.ros_state(t).levels[1] == 0) {
                self.ros_state(t).last_stand = stand - 1;
                v.set_damage(0);
                return true;
            }
        }
        false
    }
    /// AL1S 两次转阶段均先回满生命，最后阶段才进入真正死亡链。
    pub(crate) fn remaining_revive(&mut self, t: EntityIdx, u: &mut RunUpdates) -> bool {
        if !self.is_rosmontic_boss(t, "AL1S") || self.entities.get(t).unwrap().runtime.hp > 0 {
            return false;
        }
        let phase = self.ros_state(t).phase;
        if phase >= 3 {
            return false;
        }
        self.ros_state(t).phase = phase + 1;
        let max = self.entities.get(t).unwrap().template.max_hp;
        self.entities.get_mut(t).unwrap().runtime.hp = max;
        self.ros_cleanse(t);
        if phase == 1 {
            self.ros_advance(t, 6144);
            self.rosmontic_message(t, "[0]进入[无名司祭]阶段", u);
        } else {
            let children = self.remaining_children(t, 30);
            for c in children {
                self.entities.get_mut(c).unwrap().runtime.hp = 0;
                self.drain_plain_lethal_damage_into(t, c, u);
            }
            self.entities.get_mut(t).unwrap().runtime.shield = 400;
            self.ros_advance(t, 2048);
            self.rosmontic_message(t, "[0]恢复[勇者]人格", u);
        }
        true
    }
}
