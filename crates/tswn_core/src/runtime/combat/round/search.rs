use super::*;
use crate::runtime::search::{SearchCandidate, SearchTurn};

#[cfg(test)]
mod reserve_tests {
    use super::*;

    #[test]
    fn same_skill_lanes_share_stock_and_basic_type_is_fixed_by_damage() {
        // 两个同名火球槽仍可枚举，但共用账本；普攻每目标只有一个类型。
        let config = default_custom_runtime_import_config().unwrap();
        let fire = config.registry.skill_id_by_export_name(BuiltinActiveSkill::Fire.export_name()).unwrap();
        let mut field = CombatRuntime::from_template(PreparedCombatTemplate::with_registry(
            vec![
                PlayerTemplate::new(1, "actor", 0, 1000, 100)
                    .with_magic(80)
                    .with_magic_point(100)
                    .with_skill_loadout(SkillLoadout::from_skill_levels([(fire, 64), (fire, 64)])),
                PlayerTemplate::new(2, "enemy", 1, 1000, 1),
            ],
            config.registry,
        ));
        let actor = EntityIdx(0);
        for _ in 0..40 {
            field.scratch.search_turn = Some(SearchTurn::new(Some(0), None));
            field.prepare_search_action(actor, true);
        }
        assert_eq!(field.scratch.skill_reserves.len(), 1);
        assert_eq!(field.scratch.skill_reserves[0].earned(), 30.0);
        let before = field.scratch.skill_reserves[0].consumed;
        field.scratch.search_turn = Some(SearchTurn::new(None, None));
        field.prepare_search_action(actor, true);
        let turn = field.scratch.search_turn.take().unwrap();
        let selected = turn
            .candidates
            .iter()
            .position(|c| {
                matches!(&c.action,
            PreparedPlainAction::BuiltinSkill(a) if a.selected.fixed_lane == 1)
            })
            .unwrap();
        field.scratch.search_turn = Some(SearchTurn::new(Some(selected), None));
        field.prepare_search_action(actor, true);
        assert_eq!(field.scratch.skill_reserves[0].consumed, before + 1);
        for (magic, defense, resistance, mp, expected_magic) in [
            (80, 20, 20, 100, false),
            (80, 1000, 0, 100, true),
            (160, 20, 20, 0, false),
            (160, 20, 20, 15, true),
            (100, 20, 20, 100, false),
        ] {
            field.entities.get_mut(actor).unwrap().runtime.magic = magic;
            field.entities.get_mut(actor).unwrap().runtime.magic_point = mp;
            let enemy = &mut field.entities.get_mut(EntityIdx(1)).unwrap().runtime;
            enemy.defense = defense;
            enemy.resistance = resistance;
            field.scratch.search_turn = Some(SearchTurn::new(None, None));
            field.prepare_search_action(actor, true);
            let turn = field.scratch.search_turn.take().unwrap();
            let basics: Vec<_> = turn
                .candidates
                .iter()
                .filter(|c| matches!(c.action, PreparedPlainAction::BasicAttack { .. }))
                .collect();
            assert_eq!(basics.len(), 1);
            assert!(
                matches!(basics[0].action, PreparedPlainAction::BasicAttack { use_magic, .. } if use_magic == expected_magic)
            );
            assert_eq!(basics[0].cost, if expected_magic { ((magic - 100) >> 2).max(0) } else { 0 });
        }
    }

    #[test]
    fn ordered_shares_multiplier_storage_and_legal_bursts() {
        // 两个64熟练度技能按1/2、1/4累积；4倍封顶归一后均为1/2。
        let config = default_custom_runtime_import_config().unwrap();
        let fire = config.registry.skill_id_by_export_name(BuiltinActiveSkill::Fire.export_name()).unwrap();
        let ice = config.registry.skill_id_by_export_name(BuiltinActiveSkill::Ice.export_name()).unwrap();
        for multiplier in [1, 4] {
            let mut runtime = CombatRuntime::from_template(PreparedCombatTemplate::with_registry(
                vec![
                    PlayerTemplate::new(1, "actor", 0, 1000, 1)
                        .with_magic_point(100)
                        .with_skill_loadout(SkillLoadout::from_skill_levels([(fire, 64), (ice, 64)])),
                    PlayerTemplate::new(2, "enemy", 1, 1000, 1),
                ],
                config.registry.clone(),
            ));
            runtime.scratch.clever_skill_multiplier_micros = multiplier * 1_000_000;
            let actor = EntityIdx(0);
            for _ in 0..40 {
                runtime.scratch.search_turn = Some(SearchTurn::new(Some(0), None));
                runtime.prepare_search_action(actor, true);
            }
            let stock = &runtime.scratch.skill_reserves;
            assert_eq!(stock.len(), 2);
            assert_eq!(stock[0].earned(), 20.0);
            assert_eq!(stock[1].earned(), if multiplier == 1 { 10.0 } else { 20.0 });
            // v6允许延迟发行；不能再把全部累计额度误当成已发次数。
            assert!(stock.iter().all(|r| r.available() >= 8));
            let before = runtime.scratch.skill_reserves.clone();
            runtime.entities.get_mut(actor).unwrap().runtime.magic_point = 0;
            runtime.scratch.search_turn = Some(SearchTurn::new(Some(0), None));
            runtime.prepare_search_action(actor, true);
            assert_eq!(runtime.scratch.skill_reserves, before);
            runtime.entities.get_mut(actor).unwrap().runtime.magic_point = 100;
            for _ in 0..8 {
                // 候选预览不能给正式账本增发；随后实际选择冰冻，连续消耗已有库存。
                let mut preview = runtime.clone();
                preview.scratch.search_turn = Some(SearchTurn::new(None, None));
                preview.prepare_search_action(actor, true);
                let turn = preview.scratch.search_turn.take().unwrap();
                let selected = turn
                    .candidates
                    .iter()
                    .position(|c| {
                        matches!(&c.action,
                    PreparedPlainAction::BuiltinSkill(a) if a.selected.skill == BuiltinActiveSkill::Ice)
                    })
                    .unwrap();
                runtime.scratch.search_turn = Some(SearchTurn::new(Some(selected), None));
                assert!(matches!(
                    runtime.prepare_search_action(actor, true),
                    Some(PreparedPlainAction::BuiltinSkill(_))
                ));
                runtime.entities.get_mut(actor).unwrap().runtime.magic_point = 100;
            }
            assert_eq!(runtime.scratch.skill_reserves[1].consumed, 8);
            for r in &runtime.scratch.skill_reserves {
                assert!((r.initial_credit() + r.earned() - r.issued as f64 - r.credit()).abs() < 1e-10);
                assert!(r.consumed <= r.issued);
            }
            // 整场复位清空额度与倍率；分支克隆具有独立账本。
            let mut clone = runtime.clone();
            clone.scratch.clear();
            assert!(clone.scratch.skill_reserves.is_empty());
            assert_eq!(clone.scratch.clever_skill_multiplier_micros, 0);
            assert!(!runtime.scratch.skill_reserves.is_empty());
        }
        for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0, 101.0] {
            assert!(crate::runtime::skill_reserve::multiplier_micros(invalid).is_err());
        }
    }
}

impl CombatRuntime {
    pub(super) fn prepare_search_action(&mut self, actor: EntityIdx, smart: bool) -> Option<PreparedPlainAction> {
        let runtime = &self.entities.get(actor)?.runtime;
        // 特殊技能权重和 Boss 继续使用原版路径，不改变其抽样语义。
        if self.is_rc_actor(actor)
            || cfg!(feature = "cards")
            || runtime.flags.contains(PlayerKindFlags::BOSS)
            || self.entities.get(actor)?.template.skills.uses_effective_skill_weights()
        {
            self.scratch.search_turn = None;
            return self.prepare_plain_action(actor, smart, PlainSkillPreActionOutcome::default());
        }
        let (mp, attack, magic) = (runtime.magic_point, runtime.attack, runtime.magic);
        let team = self.plain_effective_team(actor);
        let cost = i32::from(self.rng.next_u8() & 15) + 8;
        let mut candidates = Vec::new();
        for &target in self.world.flat_alive() {
            if self.is_quilt(target) || self.entities.get(target)?.runtime.team == team {
                continue;
            }
            // 普攻类型不交给模型学习：按目标防抗比较简单伤害估计，MP不足只能物攻。
            let defender = &self.entities.get(target)?.runtime;
            let magic_cost = ((magic - attack) >> 2).max(0);
            let physical_damage =
                f64::from(attack.max(0)) * (f64::from(attack) + 64.0).max(0.0) / (f64::from(defender.defense) + 64.0).max(1.0);
            let magic_damage =
                f64::from(magic.max(0)) * (f64::from(magic) + 64.0).max(0.0) / (f64::from(defender.resistance) + 64.0).max(1.0);
            let use_magic = mp >= magic_cost && magic_damage > physical_damage;
            candidates.push(SearchCandidate {
                action: PreparedPlainAction::BasicAttack {
                    target,
                    use_magic,
                    amount: if use_magic { magic } else { attack },
                },
                cost: if use_magic { magic_cost } else { 0 },
            });
        }
        if mp >= cost {
            let skills = &self.entities.get(actor)?.template.skills;
            let entries: Vec<_> = skills
                .active_order()
                .iter()
                .filter_map(|&lane| Some((lane, self.builtin_active_skill(skills.skills()[lane])?, skills.level_at(lane)?)))
                .collect();
            let mut carry = 1.0;
            let mut legal = Vec::new();
            let mut chances = Vec::new();
            for (fixed_lane, skill, level) in entries {
                // 原版扫描份额 q_i=p_i*前项全失败率；不可用技能不占前置份额。
                if level == 0 || !self.plain_action_skill_allowed(actor, skill, level, smart) {
                    continue;
                }
                let targets = self.search_skill_targets(actor, skill, smart);
                if targets.is_empty() {
                    continue;
                }
                // 武器保留各自原版概率，不把其特殊判定误写成普通熟练度份额。
                if matches!(
                    skill,
                    BuiltinActiveSkill::WeaponS11 | BuiltinActiveSkill::WeaponDeathNote | BuiltinActiveSkill::WeaponXuanYue
                ) {
                    if !self.plain_action_skill_probability(actor, skill, level, smart) {
                        continue;
                    }
                } else {
                    let probability = f64::from(level.min(128)) / 128.0;
                    let share = carry * probability;
                    carry *= 1.0 - probability;
                    chances.push((skill.export_name(), share));
                }
                legal.push((fixed_lane, skill, targets));
            }
            // 先汇总同名槽份额并统一发行，再枚举共享库存下的全部合法目标。
            self.prepare_skill_reserves(actor, &chances);
            for (fixed_lane, skill, targets) in legal {
                if !matches!(
                    skill,
                    BuiltinActiveSkill::WeaponS11 | BuiltinActiveSkill::WeaponDeathNote | BuiltinActiveSkill::WeaponXuanYue
                ) && !self.has_skill_reserve(actor, skill)
                {
                    continue;
                }
                let selected = SelectedBuiltinSkill { skill, fixed_lane };
                for targets in targets {
                    candidates.push(SearchCandidate {
                        action: PreparedPlainAction::BuiltinSkill(PreparedBuiltinSkillAction { selected, targets }),
                        cost,
                    });
                }
            }
        }
        // 保存候选准备点的行动者数值，复现训练时的决策快照而不读取 RNG。
        let actor_runtime = self.entities.get(actor)?.runtime.clone();
        let skills = &self.entities.get(actor)?.template.skills;
        let candidate_levels = candidates
            .iter()
            .map(|candidate| match &candidate.action {
                PreparedPlainAction::BuiltinSkill(action) => skills.level_at(action.selected.fixed_lane).unwrap_or(0),
                _ => 0,
            })
            .collect();
        let turn = self.scratch.search_turn.as_mut()?;
        turn.actor_runtime = Some(actor_runtime);
        turn.candidate_levels = candidate_levels;
        turn.actor = Some(actor);
        turn.team = Some(team);
        turn.candidates = candidates;
        let selected = turn.chosen.and_then(|index| turn.candidates.get(index)).cloned();
        if let Some(seed) = turn.simulation_seed {
            self.rng = RC4::new(&seed.to_le_bytes(), 1);
        }
        if let Some(selected) = selected {
            if let PreparedPlainAction::BuiltinSkill(action) = &selected.action
                && !matches!(
                    action.selected.skill,
                    BuiltinActiveSkill::WeaponS11 | BuiltinActiveSkill::WeaponDeathNote | BuiltinActiveSkill::WeaponXuanYue
                )
            {
                self.consume_skill_reserve(actor, action.selected.skill);
            }
            self.entities.get_mut(actor)?.runtime.magic_point -= selected.cost;
            return Some(selected.action);
        }
        None
    }

    /// 枚举全部合法单体目标；群攻覆盖全部首目标，后续顺序有界，不穷举排列。
    pub(crate) fn search_skill_targets(
        &mut self,
        actor: EntityIdx,
        skill: BuiltinActiveSkill,
        smart: bool,
    ) -> Vec<PreparedTargetList> {
        use BuiltinActiveSkill::*;
        let team = self.plain_effective_team(actor);
        let actor_runtime = &self.entities.get(actor).unwrap().runtime;
        let actor_hp = actor_runtime.hp;
        if matches!(skill, Assassinate) && actor_runtime.assassinate.is_some() {
            return vec![PreparedTargetList::new()];
        }
        if matches!(skill, Shadow | Iron | Clone | Charge | Accumulate | Summon) {
            return vec![smallvec::smallvec![actor]];
        }
        if matches!(skill, WeaponS11 | WeaponDeathNote | WeaponXuanYue) {
            return self
                .prepare_plain_builtin_skill_action(actor, smart, SelectedBuiltinSkill { skill, fixed_lane: 0 })
                .map(|action| vec![action.targets])
                .unwrap_or_default();
        }
        let mut targets: Vec<_> = if skill == Revive {
            self.world
                .team_roster(team)
                .unwrap_or_default()
                .iter()
                .copied()
                .filter(|&id| {
                    self.entities.get(id).is_some_and(|entity| {
                        !entity.runtime.alive
                            && !entity.runtime.is_combat_minion()
                            && entity.runtime.corpse == RuntimeCorpseKind::None
                    })
                })
                .collect()
        } else {
            self.world
                .flat_alive()
                .iter()
                .filter(|target| !self.is_quilt(**target))
                .copied()
                .filter(|&id| {
                    let entity = self.entities.get(id).unwrap();
                    match skill {
                        Heal => entity.runtime.team == team && entity.runtime.hp < entity.template.max_hp,
                        Haste => entity.runtime.team == team,
                        // 蓄力的生命之轮还会夺取行动条，即使不回血也可能有战术价值。
                        Exchange => entity.runtime.team != team && (entity.runtime.hp > actor_hp || actor_runtime.charge.active),
                        RinickYuriControl => entity.runtime.team != team && entity.states.entry(PLAIN_CHARM_STATE_KEY).is_none(),
                        _ => entity.runtime.team != team,
                    }
                })
                .collect()
        };
        if !matches!(skill, Quake | Rapid | RinickIceAge) {
            return targets.into_iter().map(|target| smallvec::smallvec![target]).collect();
        }
        let limit = match skill {
            // 连击的执行器最多使用三个目标，额外目标不会产生不同动作。
            Rapid => 3,
            _ => {
                if smart {
                    6
                } else {
                    5
                }
            }
        };
        // 低血量优先与高输出优先两种后续队列；每个敌人都可作为首目标。
        targets.sort_by_key(|&id| (self.entities.get(id).unwrap().runtime.hp, id));
        let mut power_order = targets.clone();
        power_order.sort_by_key(|&id| {
            (
                std::cmp::Reverse(
                    self.entities
                        .get(id)
                        .unwrap()
                        .runtime
                        .attack
                        .max(self.entities.get(id).unwrap().runtime.magic),
                ),
                id,
            )
        });
        let mut lists = Vec::new();
        for &first in &targets {
            for order in [&targets, &power_order] {
                let mut list = smallvec::smallvec![first];
                list.extend(order.iter().copied().filter(|&id| id != first).take(limit - 1));
                if !lists.contains(&list) {
                    lists.push(list);
                }
            }
        }
        lists
    }
}
