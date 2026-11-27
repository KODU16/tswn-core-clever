//! 紫的境界、AL1S 的程式与兔狐小队主动技能。
use super::super::*;
impl CombatRuntime {
    pub(super) fn remaining_cast(&mut self, a: EntityIdx, skill: usize, release: bool, u: &mut RunUpdates) {
        let name = self.entities.get(a).unwrap().template.name.clone();
        if name == "yukari" {
            self.cast_yukari(a, skill, release, u);
            return;
        }
        if name == "AL1S" {
            self.cast_al1s(a, skill, release, u);
            return;
        }
        match (name.as_str(), skill) {
            ("fox1" | "ribbit1", 0) => self.squad_deploy(a, name == "fox1", u),
            ("fox1", 1) => {
                self.ros_state(a).levels[1] = 0;
                self.rosmontic_message(a, "[0]使用[破釜沉舟]", u);
                for n in 2..=4 {
                    if let Some(t) = self.squad_member(a, &format!("fox{n}")) {
                        if self.entities.get(t).unwrap().runtime.hp > 0 {
                            continue;
                        }
                        let e = self.entities.get_mut(t).unwrap();
                        if e.runtime.corpse != RuntimeCorpseKind::None {
                            continue;
                        }
                        e.runtime.hp = [246, 280, 234][n - 2];
                        e.runtime.alive = true;
                        let team = e.runtime.team;
                        self.world.revive_round_actor(t);
                        self.world.revive_alive(t, team);
                        self.ros_message(a, t, "[1]重新加入战斗", u);
                        self.ros_stat(t, [3, 2, 0][n - 2], [3.0, 1.5, 2.0][n - 2], 0);
                        self.ros_advance(t, 2048);
                    }
                }
            }
            ("fox1", 2) => {
                if !release {
                    let s = self.ros_state(a);
                    s.charged_skill = Some(2);
                    s.break_on_damage = true;
                    self.ros_advance(a, -1024);
                    self.rosmontic_message(a, "[0]准备[液压弹]", u);
                } else {
                    self.ros_state(a).levels[2] = 0;
                    self.ros_state(a).break_on_damage = false;
                    let atp = self.ros_atp(a, true) * 20.0;
                    self.rosmontic_message(a, "[0]发射[液压弹]", u);
                    for t in self.ros_enemies(a, 100) {
                        self.ros_hit(a, t, true, atp, true, PlainAttackOnDamage::None, u);
                    }
                }
            }
            ("fox1", 5) => {
                self.ros_state(a).levels[5] = 0;
                let n = 2 + usize::from(self.rng.next_u8() % 3);
                let f = 1.0 + f64::from(self.rng.next_u8() & 1);
                let atp = self.ros_atp(a, true) * f;
                self.rosmontic_message(a, "[0]投掷[破片炸弹]", u);
                for t in self.ros_enemies(a, n) {
                    let def = self.entities.get(t).unwrap().runtime.defense;
                    self.entities.get_mut(t).unwrap().runtime.defense = 0;
                    self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                    self.entities.get_mut(t).unwrap().runtime.defense = def;
                }
            }
            ("fox2", 0) => {
                self.ros_state(a).levels[0] = 0;
                self.rosmontic_message(a, "[0]使用[EMP]，机械单位停机", u);
                let all: Vec<_> = self
                    .entities
                    .iter()
                    .filter(|(_, e)| {
                        e.runtime.hp > 0 && e.runtime.rosmontic.as_ref().is_some_and(|s| (31..=35).contains(&s.child_kind))
                    })
                    .map(|(id, _)| id)
                    .collect();
                for t in all {
                    self.entities.get_mut(t).unwrap().runtime.hp = 0;
                    self.drain_plain_lethal_damage_into(a, t, u);
                }
                self.ros_advance(a, 1024);
            }
            ("fox2", 1) => {
                self.ros_state(a).summon_count += 1;
                self.pcb_spawn(a, 34, "移动式掩体", [240, 100, 100, 1, 1, 1, 10, 50], -204800, 327680.0, u);
            }
            ("fox2", 2) => {
                self.rosmontic_message(a, "[0]使用[后勤整备]", u);
                let team = self.plain_effective_team(a);
                let targets: Vec<_> = self
                    .world
                    .flat_alive()
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.plain_effective_team(*id) == team && !self.entities.get(*id).unwrap().runtime.is_combat_minion()
                    })
                    .take(10)
                    .collect();
                for t in targets {
                    self.entities.get_mut(t).unwrap().runtime.shield += 80;
                }
            }
            ("fox2", 3) => {
                if let Some(t) = self.select_squad_heal(a).into_iter().max_by_key(|id| {
                    let e = self.entities.get(*id).unwrap();
                    e.template.max_hp - e.runtime.hp
                }) {
                    let heal = (self.ros_atp(a, true) / 24.0).ceil() as i32;
                    self.ros_state(a).levels[3] = (self.ros_state(a).levels[3] + 1) / 2;
                    self.ros_message(a, t, "[0]为[1]准备[稻荷寿司]", u);
                    self.apply_plain_absorb_heal_into(a, t, heal, u);
                    self.ros_cleanse(t);
                    self.ros_advance(a, 1024);
                    let e = self.entities.get(t).unwrap();
                    if e.runtime.hp == e.template.max_hp {
                        self.ros_power(t, 1.0, 0.5);
                    }
                }
            }
            ("fox3", 0) => {
                self.ros_state(a).levels[0] = 0;
                self.ros_state(a).shieldless = true;
                self.ros_stat(a, 1, 0.5, 0);
                self.ros_stat(a, 2, 1.5, 0);
                self.ros_stat(a, 3, 5.0, 0);
                self.ros_power(a, 1.5, 0.0);
                self.ros_advance(a, 2048);
                self.rosmontic_message(a, "[0][弃盾]，切换突击姿态", u);
            }
            ("fox3", 1) => {
                self.ros_state(a).levels[1] /= 2;
                self.squad_taunt(a);
                self.ros_advance(a, -1024);
                self.rosmontic_message(a, "[0]使用[闪光盾]", u);
                for t in self.ros_enemies(a, 4) {
                    self.squad_blind(a, t, u);
                }
            }
            ("fox4", 0) => {
                self.pcb_spawn(a, 33, "反侦察无人机", [160, 70, 15, 240, 240, 15, 15, 50], 2048, 16384.0, u);
            }
            ("fox4", 1) => {
                let Some(target) = self.ros_state(a).charge_target else {
                    return;
                };
                self.ros_state(a).aim += 1;
                let aim = self.ros_state(a).aim;
                let mut targets = self.ros_enemies(a, 3);
                if !targets.contains(&target) {
                    if targets.is_empty() {
                        targets.push(target);
                    } else {
                        targets[0] = target;
                    }
                }
                let atp = self.ros_atp(a, false) * f64::from(aim);
                self.rosmontic_message(a, "[0]使用[贯穿狙击]", u);
                for t in targets {
                    self.ros_hit(a, t, false, atp, true, PlainAttackOnDamage::None, u);
                }
            }
            ("ribbit1", 1) => {
                self.pcb_spawn(a, 32, "自走式闪光无人机", [60, 10, 10, 200, 200, 10, 10, 60], 2048, 8192.0, u);
            }
            ("ribbit1", 2) => {
                let level = self.ros_state(a).levels[2];
                self.ros_state(a).levels[2] = (level - 16).max(16);
                self.pcb_spawn(a, 35, "阔剑地雷", [10, 200, 1, 1, 200, 1, 1, 50], 0, 32768.0, u);
            }
            ("ribbit2", 0) => {
                self.ros_state(a).levels[0] = 0;
                self.ros_state(a).anger_turns = 5;
                self.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
                self.rosmontic_message(a, "[0]使用[铁锹迫击炮]并进入[愤怒]", u);
                let atp = self.ros_atp(a, false) / 37.0;
                for t in self.ros_enemies(a, 1) {
                    self.ros_hit(a, t, false, atp, true, PlainAttackOnDamage::None, u);
                }
            }
            ("ribbit2", 1) => {
                self.ros_state(a).levels[1] = 0;
                self.ros_state(a).plate = true;
                self.ros_stat(a, 1, 5.0, 0);
                self.ros_power(a, 0.6, 0.0);
                self.rosmontic_message(a, "[0]装备[特殊防弹板]", u);
            }
            ("ribbit2", 2) => {
                self.ros_state(a).levels[2] -= 55;
                self.rosmontic_message(a, "[0]发射[信号弹]", u);
                for n in 1..=4 {
                    if let Some(t) = self.squad_member(a, &format!("ribbit{n}")) {
                        if n == 2 {
                            self.squad_taunt(t);
                            self.ros_advance(t, 512);
                        } else {
                            self.ros_stat(t, 2, 1.1, 0);
                            self.ros_stat(t, 3, 1.1, 0);
                            self.ros_advance(t, if n == 1 { 2024 } else { 2048 });
                        }
                    }
                }
            }
            ("ribbit2", 3) => {
                let def = self.entities.get(a).unwrap().runtime.defense;
                self.entities.get_mut(a).unwrap().runtime.defense = 0;
                self.rosmontic_message(a, "[0]发起[压制射击]", u);
                let atp = self.ros_atp(a, false);
                for t in self.ros_enemies(a, 1) {
                    for _ in 0..2 {
                        self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                    }
                }
                self.entities.get_mut(a).unwrap().runtime.defense = def;
            }
            ("ribbit3", 0 | 2) => {
                let full = skill == 0;
                if full {
                    self.ros_state(a).locked = false;
                    self.ros_state(a).power_bits = 1.0f64.to_bits();
                    self.entities.get_mut(a).unwrap().refresh_runtime_stats_from_template();
                }
                self.rosmontic_message(
                    a,
                    if full {
                        "[0]使用[全弹发射]"
                    } else {
                        "[0]呼叫[钢铁之雨]"
                    },
                    u,
                );
                let atp = self.ros_atp(a, true) * if full { 3.5 } else { 1.6 };
                for t in self.ros_enemies(a, if full { 5 } else { 4 }) {
                    let hit = self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                    if !full && hit > 0 {
                        self.apply_rosmontic_burn(a, t, hit, u);
                    }
                }
            }
            ("ribbit3", 1) => {
                self.ros_state(a).levels[1] = 0;
                self.pcb_spawn(a, 31, "支援直升机", [800, 100, 120, 90, 50, 10, 20, 50], 0, 49152.0, u);
            }
            ("ribbit4", 1) => {
                let smart = self.ros_state(a).smart;
                let target = self.ros_enemies(a, 999).into_iter().find(|id| {
                    let e = self.entities.get(*id).unwrap();
                    !smart
                        || (!e.runtime.is_combat_minion()
                            && e.runtime.hp >= 100
                            && e.runtime.rosmontic.as_ref().is_none_or(|s| s.locked_turns == 0))
                });
                if let Some(t) = target {
                    let s = self.ros_state(t);
                    s.locked_turns = if s.locked_turns == 0 {
                        3
                    } else {
                        s.locked_turns.saturating_add(4)
                    };
                    s.locked_divisor = if s.locked_divisor == 0 {
                        10
                    } else {
                        s.locked_divisor.saturating_add(5)
                    };
                    self.entities.get_mut(t).unwrap().refresh_runtime_stats_from_template();
                    self.ros_advance(a, 1024);
                    self.ros_message(a, t, "[0]使用[胆怯者的观测]，[1]被[锁定]", u);
                }
            }
            _ => {}
        }
    }
    /// 部署永久增益只执行一次；成员按同队身份检索，不依赖召唤顺序。
    fn squad_deploy(&mut self, a: EntityIdx, fox: bool, u: &mut RunUpdates) {
        self.ros_state(a).levels[0] = 0;
        self.ros_state(a).deployed = true;
        self.rosmontic_message(
            a,
            if fox {
                "[0]使用[战术部署]"
            } else {
                "[0]使用[战术规划]"
            },
            u,
        );
        for n in 1..=4 {
            if let Some(t) = self.squad_member(a, &format!("{}{n}", if fox { "fox" } else { "ribbit" })) {
                if fox {
                    self.ros_stat(t, 3, 1.5, 0);
                    match n {
                        1 => {
                            self.ros_stat(t, 0, 1.2, 0);
                            self.ros_power(t, 1.5, 0.0);
                            self.ros_attract(t, 0.25);
                        }
                        2 => {
                            self.ros_stat(t, 4, 2.0, 0);
                            self.ros_stat(t, 5, 2.0, 0);
                            self.ros_attract(t, 0.5);
                            self.ros_advance(t, 2048);
                        }
                        3 => {
                            self.ros_stat(t, 1, 2.5, 0);
                            self.ros_advance(t, 4096);
                        }
                        _ => {
                            self.ros_stat(t, 0, 3.0, 0);
                            self.ros_advance(t, 2048);
                        }
                    }
                } else {
                    match n {
                        1 => {
                            self.ros_power(t, 1.5, 0.0);
                            self.ros_stat(t, 3, 1.5, 0);
                            self.ros_stat(t, 2, 1.25, 0);
                        }
                        2 => {
                            for i in [1, 2, 5] {
                                self.ros_stat(t, i, 1.5, 0);
                            }
                            self.ros_advance(t, 4096);
                        }
                        3 => self.ros_stat(t, 0, 3.0, 0),
                        _ => {
                            self.ros_power(t, 1.5, 0.0);
                            self.ros_stat(t, 2, 0.75, 0);
                            self.ros_stat(t, 3, 4.0, 0);
                            self.ros_advance(t, 1024);
                        }
                    }
                }
            }
        }
    }
    fn cast_yukari(&mut self, a: EntityIdx, skill: usize, release: bool, u: &mut RunUpdates) {
        match skill {
            0 => {
                if !release {
                    let s = self.ros_state(a);
                    s.charged_skill = Some(0);
                    s.break_on_damage = true;
                    self.ros_advance(a, 256);
                    self.rosmontic_message(a, "[0]准备[废线「废弃车站下车之旅」]", u);
                } else {
                    self.ros_state(a).levels[0] = 0;
                    self.ros_state(a).break_on_damage = false;
                    let atp = self.ros_atp(a, false) * 8.0;
                    for t in self.ros_enemies(a, 1) {
                        self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                    }
                }
            }
            1 => {
                self.ros_state(a).barrier_turns = 2;
                self.ros_attract(a, 1.12);
                self.rosmontic_message(a, "[0]展开[四重结界]", u);
            }
            2 => {
                self.rosmontic_message(a, "[0]使用[飞光虫之巢]", u);
                for t in self.ros_enemies(a, 1) {
                    for i in 0..5 {
                        let ar = &self.entities.get(a).unwrap().runtime;
                        let tr = &self.entities.get(t).unwrap().runtime;
                        let accuracy = 100 + ar.agility;
                        let dodge = tr.resistance + tr.agility;
                        if self.roll_plain_dodge(t, accuracy, dodge, false) {
                            break;
                        }
                        let atp = self.ros_atp(a, true) * 0.2 * (1.0 + 0.9 * f64::from(i));
                        self.ros_hit(a, t, true, atp, true, PlainAttackOnDamage::None, u);
                    }
                }
            }
            _ => {
                if skill == 3 {
                    self.ros_state(a).boundary += 1;
                    if self.rng.next_u8() & 7 == 0 {
                        self.ros_state(a).death_boundary = true;
                    }
                }
                self.rosmontic_message(
                    a,
                    if skill == 3 {
                        "[0]使用[境符「波与粒的境界」]"
                    } else {
                        "[0]使用[境符「生与死的境界」]"
                    },
                    u,
                );
                let atp = self.ros_atp(a, true) * if skill == 3 { 0.4 } else { 2.0 };
                for t in self.world.flat_alive().to_vec() {
                    if self.is_rosmontic_boss(t, "yukari") {
                        continue;
                    }
                    for _ in 0..if skill == 3 { 4 } else { 1 } {
                        self.ros_hit(a, t, false, atp, false, PlainAttackOnDamage::None, u);
                    }
                }
            }
        }
    }
    fn cast_al1s(&mut self, a: EntityIdx, skill: usize, release: bool, u: &mut RunUpdates) {
        match skill {
            1 => {
                self.ros_state(a).levels[1] = 0;
                self.rosmontic_message(a, "[0]使用[伙伴招募]", u);
                for (role, name, attrs) in [
                    (36, "花冈柚子", [600, 110, 120, 65, 75, 70, 120, 100]),
                    (37, "才羽桃井", [750, 90, 75, 90, 90, 70, 75, 90]),
                    (38, "才羽绿", [750, 70, 75, 90, 90, 90, 75, 90]),
                ] {
                    self.pcb_spawn(a, role, name, attrs, 2048, 32768.0, u);
                }
            }
            2 => {
                self.ros_state(a).levels[2] = 0;
                self.rosmontic_message(a, "[0]使用[强化魔法]", u);
                let mut targets = vec![a];
                for role in 36..=38 {
                    targets.extend(self.remaining_children(a, role));
                }
                for t in targets {
                    let role = self.ros_state(t).child_kind;
                    match role {
                        36 => {
                            self.ros_stat(t, 0, 1.5, 0);
                            self.ros_power(t, 2.0, 0.0);
                        }
                        37 => self.ros_stat(t, 2, 2.5, 0),
                        38 => {
                            self.ros_stat(t, 3, 7.5, 0);
                            self.ros_attract(t, 0.5);
                        }
                        _ => {
                            self.ros_stat(t, 1, 5.0, 0);
                            self.ros_stat(t, 5, 5.0, 0);
                            self.ros_power(t, 0.5, 0.0);
                            self.ros_attract(t, 2.0);
                        }
                    }
                    self.ros_advance(t, 2048);
                }
            }
            3 => {
                if !release {
                    self.ros_state(a).charged_skill = Some(3);
                    self.rosmontic_message(a, "[0]为[光炮]填充魔力", u);
                } else if self.ros_state(a).phase == 2 {
                    self.ros_advance(a, 2048);
                } else {
                    let energy = self.ros_state(a).energy;
                    self.ros_state(a).energy = 0;
                    let atp = self.ros_atp(a, true) * 2.0 * (1.0 + f64::from(energy));
                    self.rosmontic_message(a, "[0]发射[光炮]", u);
                    for t in self.ros_enemies(a, 6) {
                        self.ros_hit(a, t, true, atp, false, PlainAttackOnDamage::None, u);
                    }
                }
            }
            4 => {
                self.rosmontic_message(a, "[0]执行[侵略／防卫程式]", u);
                // 所有 Type.F 执行同一个程式，依据 AL1S 自身血量选分支。
                let e = self.entities.get(a).unwrap();
                let high = e.runtime.hp * 2 >= e.template.max_hp;
                let attack = (self.rng.next_u8() & 3 != 0) == high;
                for t in self.remaining_children(a, 30) {
                    if attack {
                        self.ros_stat(t, 0, 1.0, if high { 100 } else { 75 });
                        self.ros_stat(t, 2, 1.0, if high { 40 } else { 30 });
                        self.ros_power(t, 1.0, if high { 0.4 } else { 0.3 });
                    } else {
                        for i in [1, 5] {
                            self.ros_stat(t, i, 1.0, if high { 75 } else { 100 });
                        }
                        self.ros_state(t).attract_scale_bits = 1.5f64.to_bits();
                        self.entities.get_mut(t).unwrap().refresh_runtime_stats_from_template();
                    }
                }
            }
            5 => {
                let n = self.ros_state(a).summon_count;
                self.ros_state(a).summon_count = (n + 1).min(5);
                for _ in 0..n {
                    self.pcb_spawn(a, 30, "Type.F", [200, 50, 50, 50, 50, 50, 50, 50], 2048, 40960.0, u);
                }
            }
            _ => {}
        }
    }
}
