//! 离线战斗数据采集核心（clever-collect 的后端，CPU_SIMULATION_AGENT.md 协议）。
//!
//! 在真实决策机会上分叉：每个候选动作与 baseline 共用同一组试算 seed 续局，
//! 输出结构化 JSON（states 行 / decisions 行）。只依赖原版续局与 Clever 候选
//! 枚举语义，不改变对战规则。

use super::combat::PreparedPlainAction;
use super::search::SearchTurn;
use super::{CombatRuntime, RuntimeRunner};
use serde_json::{Value, json};

/// 一次采集任务的可变配置（写进 manifest 的 configuration_sha256）。
#[derive(Debug, Clone)]
pub struct CollectConfig {
    /// 每个候选与 baseline 的续局次数。
    pub trials: usize,
    /// 单次续局的回合预算（含正式轨迹）。
    pub max_rounds: u64,
}

/// 决策机会的 RNG 边界说明（写入决策上下文与 manifest）。
pub const RNG_BOUNDARY_DESC: &str = "决策检查点位于回合调度边界（行动选择前）。行动者、行动前钩子与智慧判定 \
在试算中确定性地从检查点随机流重放；候选枚举消耗的耗蓝字节在决策点一次性结算（所有试算共用，属固定情景）。\
候选动作与 baseline 都在行动结算入口把随机流重置为派生 seed（RC4::new(seed.to_le_bytes())），\
行动内部随机性（命中/暴击/随机效果）按 seed 重新抽样，之后双方按原版策略续局。";

/// 单次试算的结局（视角阵营）：赢 / 输 / 未决；附带推进回合数与终止原因。
fn run_trial(
    checkpoint: &CombatRuntime,
    perspective: usize,
    forced: Option<usize>,
    seed: u64,
    max_rounds: u64,
) -> (i8, u64, &'static str) {
    let mut trial = checkpoint.clone();
    trial.scratch.search_turn = Some(match forced {
        // 候选分叉：搜索路径选择候选 index，行动随机流以 seed 重置。
        Some(index) => SearchTurn {
            chosen: Some(index),
            simulation_seed: Some(seed),
            ..SearchTurn::new(None, None)
        },
        // baseline：原版策略选择本行动，随机流以同一 seed 重置。
        None => SearchTurn {
            original_policy_seed: Some(seed),
            ..SearchTurn::new(None, None)
        },
    });
    let start_round = trial.round;
    // 第一动作：逐最小回合推进，直到本回合行动结算（含空批次推进与中途终局）。
    let mut winner = None;
    while winner.is_none() && trial.round - start_round < max_rounds {
        match trial.run_minimal_round_once() {
            Some(outcome) => {
                winner = outcome.winner_team;
                break;
            }
            None => {}
        }
    }
    // 第一动作之后清除分叉指令，双方按原版策略续局（规范：continuation_policy=legacy）。
    trial.scratch.search_turn = None;
    while winner.is_none() && trial.round - start_round < max_rounds {
        winner = trial.run_minimal_round_no_capture().winner_team;
    }
    let rounds = trial.round - start_round;
    match winner {
        Some(team) if team == perspective => (1, rounds, "winner"),
        Some(_) => (0, rounds, "winner"),
        None => (-1, rounds, "max_rounds"),
    }
}

/// 试算计数聚合。
#[derive(Default)]
struct TrialCounts {
    wins: usize,
    losses: usize,
    unresolved: usize,
    outcomes: Vec<i8>,
    stop_reasons: std::collections::BTreeMap<String, usize>,
    rounds_sum: u64,
    rounds_max: u64,
}

impl TrialCounts {
    fn add(&mut self, outcome: i8, rounds: u64, stop: &'static str) {
        self.outcomes.push(outcome);
        self.rounds_sum += rounds;
        self.rounds_max = self.rounds_max.max(rounds);
        *self.stop_reasons.entry(stop.to_owned()).or_insert(0) += 1;
        match outcome {
            1 => self.wins += 1,
            0 => self.losses += 1,
            _ => self.unresolved += 1,
        }
    }

    fn n(&self) -> usize { self.outcomes.len() }

    fn p_lower(&self) -> f64 { self.wins as f64 / self.n() as f64 }

    fn p_upper(&self) -> f64 { (self.wins + self.unresolved) as f64 / self.n() as f64 }

    fn to_json(&self) -> Value {
        json!({
            "wins": self.wins,
            "losses": self.losses,
            "unresolved": self.unresolved,
            "n_trials": self.n(),
            "p_win_lower": self.p_lower(),
            "p_win_upper": self.p_upper(),
            "outcomes": self.outcomes,
            "stop_reason_counts": self.stop_reasons,
            "rounds_mean": if self.n() == 0 { 0.0 } else { self.rounds_sum as f64 / self.n() as f64 },
            "rounds_max": self.rounds_max,
        })
    }
}

/// 把候选动作转成结构化 canonical_action（候选顺序稳定，与枚举顺序一致）。
fn canonical_action(turn: &SearchTurn, index: usize, runtime: &CombatRuntime) -> Option<Value> {
    let candidate = &turn.candidates[index];
    let actor = turn.actor?;
    match &candidate.action {
        PreparedPlainAction::BasicAttack {
            target,
            use_magic,
            amount,
        } => Some(json!({
            "action_kind": "basic_attack",
            "target_entity_ids": [format!("e{}", target.0)],
            "use_magic": use_magic,
            "amount": amount,
            "magic_cost": candidate.cost,
            "skill_export_name": Value::Null,
            "skill_slot_key": Value::Null,
            "fixed_lane": Value::Null,
            "skill_level": Value::Null,
        })),
        PreparedPlainAction::BuiltinSkill(action) => {
            let lane = action.selected.fixed_lane;
            let skills = &runtime.entities.get(actor)?.template.skills;
            // 普通玩家的 lane 即 pbb 槽位 key（普通技能导入按 key 建 lane）；
            // 特殊技能（召唤/附身/武器）无 pbb 槽位语义，slot_key 记 null。
            let skill_id = skills.skills()[lane];
            let slot_key = runtime
                .registry
                .skill(skill_id)
                .filter(|spec| spec.export_name.starts_with("core.skill."))
                .map(|_| json!(lane))
                .unwrap_or(Value::Null);
            let level = skills.level_at(lane);
            Some(json!({
                "action_kind": "builtin_skill",
                "skill_export_name": action.selected.skill.export_name(),
                "skill_slot_key": slot_key,
                "fixed_lane": lane,
                "skill_level": level,
                "target_entity_ids": action.targets.iter().map(|t| format!("e{}", t.0)).collect::<Vec<_>>(),
                "use_magic": Value::Null,
                "amount": Value::Null,
                "magic_cost": candidate.cost,
            }))
        }
        // 强制行动/专属脚本等不应出现在候选表；出现则跳过该候选并计数。
        _ => None,
    }
}

/// 一个决策机会的完整采集结果（states 行 + decisions 行 + 统计）。
#[derive(Debug)]
pub struct CollectedDecision {
    pub state_id: String,
    pub decision_id: String,
    pub state_line: Value,
    pub decision_line: Value,
    pub rollouts: u64,
    pub candidates: usize,
    pub unresolved: usize,
    pub skipped_candidates: usize,
}

/// 一场战斗的采集结果。
#[derive(Debug)]
pub struct CollectedBattle {
    pub decisions: Vec<CollectedDecision>,
    pub audit_line: Value,
    pub skip_counts: std::collections::BTreeMap<&'static str, u64>,
}

/// 在检查点（回合调度边界）上采集一个决策机会。
#[allow(clippy::too_many_arguments)]
fn capture_decision(
    trials: usize,
    max_rounds: u64,
    battle_index: u64,
    split: &str,
    checkpoint: &CombatRuntime,
    runner: &RuntimeRunner,
    turn: SearchTurn,
    boundary: &Option<(Value, u64)>,
    decision_n: usize,
) -> Result<CollectedDecision, String> {
    let actor = turn.actor.ok_or("决策缺少行动者")?;
    let team = turn.team.ok_or("决策缺少有效阵营")?;
    let actor_entity = checkpoint.entities.get(actor).ok_or("行动者实体缺失")?;
    let actor_id = format!("e{}", actor.0);
    // 视角映射：actor 归属的输入组与双方输入组到运行时队伍的映射。
    let input_groups = runner.input_groups();
    let actor_input_team = input_groups
        .iter()
        .position(|group| group.iter().any(|id| *id == actor))
        .map(|i| json!(i))
        .unwrap_or(Value::Null);
    let team_mapping: Vec<Value> = input_groups
        .iter()
        .enumerate()
        .map(|(i, group)| {
            json!({
                "input_team_index": i,
                "entity_ids": group.iter().map(|id| format!("e{}", id.0)).collect::<Vec<_>>(),
                "runtime_teams": group.iter().map(|id| checkpoint.entities.get(*id).map(|e| e.runtime.team)).collect::<Vec<_>>(),
            })
        })
        .collect();

    // 结构化候选表（含已结算的耗蓝等前置条件）。
    let mut context_candidates: Vec<Value> = Vec::new();
    let mut valid_indices: Vec<usize> = Vec::new();
    let mut skipped_candidates = 0usize;
    for (i, candidate) in turn.candidates.iter().enumerate() {
        match canonical_action(&turn, i, checkpoint) {
            Some(action) => {
                valid_indices.push(i);
                context_candidates.push(json!({
                    "candidate_index": i,
                    "canonical_action": action,
                    "pre_resolved_magic_cost": candidate.cost,
                }));
            }
            None => skipped_candidates += 1,
        }
    }

    let state_id = format!("s{battle_index:08}-{decision_n:02}");
    let decision_id = format!("d{battle_index:08}-{decision_n:02}");
    let battle_id = format!("b{battle_index:08}");
    let seed_set_id = format!("ss{battle_index:08}-{decision_n:02}");
    let decision_round = checkpoint.round;

    let state_line = json!({
        "schema_version": 1,
        "state_id": state_id,
        "battle_id": battle_id,
        "split": split,
        "clean_round_index": boundary.as_ref().map(|(_, r)| *r).unwrap_or(0),
        "decision_round_index": decision_round,
        "round_boundary_state": boundary.as_ref().map(|(s, _)| s.clone()).unwrap_or(Value::Null),
        "decision_context": {
            "actor_entity_id": actor_id,
            "actor_runtime_snapshot": serde_json::to_value(&actor_entity.runtime).unwrap_or(Value::Null),
            "effective_team": team,
            "candidates": context_candidates,
            "pre_resolved_preconditions": "候选的 magic_cost 在决策点一次性结算（固定情景，全部试算共用）",
            "rng_boundary": RNG_BOUNDARY_DESC,
            "state_changes_since_boundary": "边界到行动选择点之间：行动者解析、行动前钩子、智慧判定与候选枚举按检查点随机流确定性重放",
        },
        "features": Value::Null,
        "feature_schema_id": Value::Null,
        "feature_overflow": Value::Null,
    });

    // 试算 seed 列表：baseline 与全部候选共用同一组 seed（配对样本）。
    let seeds: Vec<u64> = (0..trials)
        .map(|k| sha256_u64(&format!("clever-data-v1:trial:{seed_set_id}:k={k}")))
        .collect();
    let mut baseline = TrialCounts::default();
    for &seed in &seeds {
        let (outcome, rounds, stop) = run_trial(checkpoint, team, None, seed, max_rounds);
        baseline.add(outcome, rounds, stop);
    }
    let mut actions = Vec::new();
    let mut unresolved_total = baseline.unresolved;
    for &i in &valid_indices {
        let mut counts = TrialCounts::default();
        for &seed in &seeds {
            let (outcome, rounds, stop) = run_trial(checkpoint, team, Some(i), seed, max_rounds);
            counts.add(outcome, rounds, stop);
        }
        unresolved_total += counts.unresolved;
        // 收益区间：动作胜率区间减 baseline 胜率区间；配对均值只在两侧终局的 k 上计算。
        let mut paired_sum = 0.0;
        let mut paired_n = 0usize;
        for k in 0..trials {
            let a = counts.outcomes[k];
            let b = baseline.outcomes[k];
            if a >= 0 && b >= 0 {
                paired_sum += f64::from(a - b);
                paired_n += 1;
            }
        }
        let paired_mean = if paired_n > 0 { paired_sum / paired_n as f64 } else { 0.0 };
        let mut paired_var = 0.0;
        if paired_n > 1 {
            for k in 0..trials {
                let a = counts.outcomes[k];
                let b = baseline.outcomes[k];
                if a >= 0 && b >= 0 {
                    let d = f64::from(a - b) - paired_mean;
                    paired_var += d * d;
                }
            }
        }
        let paired_se = if paired_n > 1 {
            (paired_var / (paired_n as f64 * (paired_n - 1) as f64)).sqrt()
        } else {
            0.0
        };
        actions.push(json!({
            "action_id": format!("a{battle_index:08}-{decision_n:02}-{i:02}"),
            "candidate_index": i,
            "canonical_action": canonical_action(&turn, i, checkpoint),
            "wins": counts.wins,
            "losses": counts.losses,
            "unresolved": counts.unresolved,
            "n_trials": counts.n(),
            "outcomes": counts.outcomes,
            "p_win_lower": counts.p_lower(),
            "p_win_upper": counts.p_upper(),
            "advantage_lower": counts.p_lower() - baseline.p_upper(),
            "advantage_upper": counts.p_upper() - baseline.p_lower(),
            "paired_completed": paired_n,
            "paired_mean": paired_mean,
            "paired_se": paired_se,
            "paired_unresolved_action": counts.unresolved,
            "paired_unresolved_baseline": baseline.unresolved,
            "stop_reason_counts": counts.stop_reasons,
            "rounds_mean": if counts.n() == 0 { 0.0 } else { counts.rounds_sum as f64 / counts.n() as f64 },
            "rounds_max": counts.rounds_max,
        }));
    }

    let decision_line = json!({
        "schema_version": 1,
        "decision_id": decision_id,
        "state_id": state_id,
        "battle_id": battle_id,
        "split": split,
        "actor_entity_id": actor_id,
        "actor_original_input_team": actor_input_team,
        "perspective_effective_team": team,
        "input_to_runtime_team_mapping": team_mapping,
        "sampling_bucket": "uniform-decision-opportunity",
        "inclusion_probability": 1.0,
        "seed_set_id": seed_set_id,
        "trial_seed_derivation": "sha256(\"clever-data-v1:trial:\"+seed_set_id+\":k=\"+k) 前 8 字节小端，k=0..n_trials-1；baseline 与全部动作共用同一列表",
        "baseline": baseline.to_json(),
        "actions": actions,
    });

    Ok(CollectedDecision {
        state_id,
        decision_id,
        state_line,
        decision_line,
        rollouts: ((valid_indices.len() + 1) * trials) as u64,
        candidates: valid_indices.len(),
        unresolved: unresolved_total,
        skipped_candidates,
    })
}

/// 采集一场战斗：正式轨迹只用原版策略，最多 4 个决策机会。
pub fn collect_battle(
    groups: &[Vec<String>],
    seed_line: &str,
    battle_index: u64,
    split: &str,
    config: &CollectConfig,
) -> Result<CollectedBattle, String> {
    let mut skip_counts: std::collections::BTreeMap<&'static str, u64> = std::collections::BTreeMap::new();
    let mut runner = RuntimeRunner::new_from_groups_with_seed(groups, &[seed_line.to_owned()]).map_err(|e| e.to_string())?;
    let mut decisions = Vec::new();
    let mut boundary: Option<(Value, u64)> = match runner.model_state() {
        Ok(state) => Some((serde_json::to_value(&state).map_err(|e| e.to_string())?, 0)),
        Err(_) => None,
    };
    loop {
        if runner.runtime().world.winner_team().is_some() || runner.runtime().round >= config.max_rounds {
            break;
        }
        if decisions.len() < 4 {
            // 检查点 = 回合调度边界（行动选择前）；探测不改动正式对局。
            let checkpoint = runner.runtime().clone();
            match checkpoint.collect_decision_preview() {
                Ok(turn) => {
                    // 决策边界状态：效果队列排空时可在当前边界导出，否则沿用最近一次成功边界。
                    if let Ok(state) = runner.model_state() {
                        let round = runner.runtime().round;
                        boundary = Some((serde_json::to_value(&state).map_err(|e| e.to_string())?, round));
                    }
                    let captured = capture_decision(
                        config.trials,
                        config.max_rounds,
                        battle_index,
                        split,
                        &checkpoint,
                        &runner,
                        turn,
                        &boundary,
                        decisions.len(),
                    )?;
                    decisions.push(captured);
                }
                Err(reason) => {
                    *skip_counts.entry(reason).or_insert(0) += 1;
                }
            }
        }
        let outcome = runner.runtime_mut().run_minimal_round_no_capture();
        if outcome.winner_team.is_some() {
            break;
        }
    }
    let audit_line = json!({
        "battle_index": battle_index,
        "battle_id": format!("b{battle_index:08}"),
        "split": split,
        "full_names": groups.iter().cloned().collect::<Vec<_>>(),
        "battle_seed": seed_line,
        "winner_team": runner.runtime().world.winner_team(),
        "rounds": runner.runtime().round,
        "decisions_taken": decisions.len(),
    });
    Ok(CollectedBattle {
        decisions,
        audit_line,
        skip_counts,
    })
}

/// 用引擎名字构造 API 重新生成一个名字的属性与技能（audit-names 用）。
pub fn audit_name_record(full_name: &str) -> Result<Value, String> {
    let (groups, _) = RuntimeRunner::split_namerena_into_groups(full_name.to_owned());
    let runner = RuntimeRunner::new_from_groups_with_seed(&groups, &["audit-names".to_owned()])
        .map_err(|e| format!("构造 {full_name} 失败：{e}"))?;
    let runtime = runner.runtime();
    let entity = runtime.entities.iter().next().map(|(_, e)| e).ok_or("空实体")?;
    let template = &entity.template;
    let attrs = [
        template.max_hp,
        template.attack,
        template.magic,
        template.wisdom,
        template.speed,
        template.defense,
        template.resistance,
        template.agility,
    ];
    let sum: i32 = attrs.iter().sum();
    let skills = &template.skills;
    let lanes: Vec<Value> = (0..40)
        .map(|lane| {
            // 普通玩家的 lane 即 pbb 槽位 key；特殊技能（武器/召唤身份）无 pbb 槽位语义。
            let slot_key = skills
                .skills()
                .get(lane)
                .and_then(|&skill_id| runtime.registry.skill(skill_id))
                .filter(|spec| spec.export_name.starts_with("core.skill."))
                .map(|_| json!(lane))
                .unwrap_or(Value::Null);
            json!({
                "lane": lane,
                "slot_key": slot_key,
                "level": skills.level_at(lane),
            })
        })
        .collect();
    Ok(json!({
        "full_name": full_name,
        "attrs": attrs,
        "attr_sum_engine": template.attr_sum,
        "attr_sum_computed": sum,
        "attr_props": attrs.iter().map(|a| f64::from(*a) / f64::from(sum)).collect::<Vec<_>>(),
        "skills_lanes": lanes,
        "active_order": skills.active_order(),
    }))
}

/// pbb_skill_0..15 槽内技能 ID 与引擎导出名/内置主动技能的映射表。
pub fn skill_mapping_table() -> Value {
    let mut slots = Vec::new();
    for key in 0..40usize {
        let active = super::BuiltinActiveSkill::from_legacy_key(key).map(|skill| skill.export_name().to_owned());
        slots.push(json!({
            "slot_key": key,
            "export_name": crate::namerena::skill_name_for_export(key),
            "builtin_active_export_name": active,
        }));
    }
    json!({
        "schema_version": 1,
        "note": "pbb_skill_0..15 的槽内技能 ID 即本表的 slot_key（0..34 为命名技能，35..39 为非技能槽）；同一技能可占多槽，逐槽熟练度见 pbb_freq_0..15。builtin_active_export_name 对应战斗决策 canonical_action.skill_export_name。",
        "slots": slots,
    })
}

/// 字符串 sha256 前 8 字节（小端）作为 u64（试算 seed 派生，规范固定公式）。
fn sha256_u64(text: &str) -> u64 {
    use sha2::Digest;
    let digest: [u8; 32] = sha2::Sha256::digest(text.as_bytes()).into();
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

/// 采集协议的数据格式版本。
pub const COLLECT_SCHEMA_VERSION: u64 = 1;
