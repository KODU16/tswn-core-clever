//! 已训练混合策略的独立推理；严格复现机制摘要 v1，不读取名字、种子或启发式分数。
use super::model_state::{ModelPayload, ModelPlayerRuntime, ModelSkills};
use super::search::{SearchCandidate, SearchTurn};
use super::*;
use std::sync::OnceLock;

pub const MODEL_ID: &str = "clever-mixed-v1-20261127-e15";
pub const CONTEXT_WIDTH: usize = 1318;
pub const ACTION_WIDTH: usize = 327;
const ENTITY_WIDTH: usize = 146;
const POOL_WIDTH: usize = ENTITY_WIDTH * 2 + 1;
const WEIGHTS: &[u8] = include_bytes!("../../models/clever-mixed-v1/weights.bin");
pub const MODEL_MANIFEST: &str = include_str!("../../models/clever-mixed-v1/manifest.json");

/// 普通、召唤物和被动技能的 1-based 注册编号转为训练列，不把编号当作数值特征。
fn skill_column(id: u32) -> Option<usize> {
    Some(match id {
        6..=30 => (id - 6) as usize,
        35 => 25,
        34 => 26,
        36 => 27,
        42 => 28,
        33 => 29,
        39 => 30,
        40 => 31,
        41 => 32,
        37 => 33,
        38 => 34,
        0..=5 => 35 + id as usize,
        31 => 41,
        32 => 42,
        _ => return None,
    })
}

/// 复现训练时的状态摘要；没有导出的 step/damage 不擅自补充到旧特征列。
fn state_summary(payload: &ModelPayload) -> (usize, f32, f32) {
    let (key, step, amount) = match payload.kind.as_str() {
        "charm" => (0, payload.charm.as_ref().map_or(0, |v| v.step), 0),
        "curse" => (1, 0, 0),
        "poison" => (2, 0, 0),
        "haste" => (3, payload.haste.as_ref().map_or(0, |v| v.step), 0),
        "slow" => (4, payload.slow.as_ref().map_or(0, |v| v.step), 0),
        "iron" => (
            5,
            payload.iron.as_ref().map_or(0, |v| v.step),
            payload.iron.as_ref().map_or(0, |v| v.protect),
        ),
        "ice" => (6, 0, 0),
        "berserk" => (7, payload.berserk.as_ref().map_or(0, |v| v.step), 0),
        "fire" => (8, 0, 0),
        "covid_infection" => (9, 0, 0),
        "lazy_infection" => (10, 0, 0),
        _ => (11, 0, 0),
    };
    (key, step as f32, amount as f32)
}

/// 只投影训练真正读取的字段，避免把未训练的新 Boss 字段或未来 RNG 混入网络。
fn entity_vector(
    id: EntityIdx,
    max_hp: i32,
    r: &ModelPlayerRuntime,
    skills: &ModelSkills,
    states: &[ModelPayload],
) -> Option<Vec<f32>> {
    let mut v = vec![
        r.hp as f32,
        r.attack as f32,
        r.defense as f32,
        r.speed as f32,
        r.agility as f32,
        r.magic as f32,
        r.resistance as f32,
        r.wisdom as f32,
        r.magic_point as f32,
        r.shield as f32,
        max_hp.max(1) as f32,
        (f64::from(r.hp) / f64::from(max_hp.max(1))) as f32,
        f32::from(r.alive),
        f32::from(r.owner != id),
        (r.at_boost_millionths as f64 / 1e6) as f32,
        r.move_state.speed_points as f32,
        f32::from(r.hide.is_some()),
        f32::from(r.assassinate.is_some()),
        f32::from(r.upgrade_active),
        f32::from(r.charge.active),
        r.charge.step as f32,
        f32::from(r.accumulate.active),
        r.protect_from.len() as f32,
        f32::from(r.protect_to.is_some()),
    ];
    v.resize(ENTITY_WIDTH, 0.0);
    for lane in &skills.lanes {
        let offset = 24 + 2 * skill_column(lane.skill_id)?;
        v[offset] = v[offset].max(lane.level as f32);
        v[offset + 1] = v[offset + 1].max(lane.build_level as f32);
    }
    for state in states {
        let (key, step, amount) = state_summary(state);
        let offset = 110 + 3 * key;
        v[offset] += 1.0;
        v[offset + 1] = v[offset + 1].max(step);
        v[offset + 2] = v[offset + 2].max(amount);
    }
    Some(v)
}

struct EncodedEntity {
    id: EntityIdx,
    team: usize,
    alive: bool,
    values: Vec<f32>,
}

/// 均值、最大值和计数涵盖全部实体，不按名字排序或截断召唤物。
fn pool(entities: &[&EncodedEntity]) -> Vec<f32> {
    let mut result = vec![0.0; POOL_WIDTH];
    if entities.is_empty() {
        return result;
    }
    result[0] = entities.len() as f32;
    result[1 + ENTITY_WIDTH..].fill(f32::NEG_INFINITY);
    for entity in entities {
        for (i, &value) in entity.values.iter().enumerate() {
            result[1 + i] += value;
            result[1 + ENTITY_WIDTH + i] = result[1 + ENTITY_WIDTH + i].max(value);
        }
    }
    for value in &mut result[1..1 + ENTITY_WIDTH] {
        *value /= entities.len() as f32;
    }
    result
}

#[derive(serde::Deserialize)]
struct PolicyAction {
    key: usize,
    cost: i32,
    level: u32,
    amount: i32,
    targets: Vec<EntityIdx>,
}

fn encode_context(entities: &[EncodedEntity], actor: EntityIdx, team: usize) -> Option<Vec<f32>> {
    let mut context = entities.iter().find(|e| e.id == actor)?.values.clone();
    for (ally, living) in [(true, true), (false, true), (true, false), (false, false)] {
        let group: Vec<_> = entities.iter().filter(|e| (e.team == team) == ally && e.alive == living).collect();
        context.extend(pool(&group));
    }
    Some(context)
}

fn encode_action(entities: &[EncodedEntity], actor: EntityIdx, team: usize, action: &PolicyAction) -> Option<Vec<f32>> {
    if action.key >= 28 {
        return None;
    }
    let targets: Vec<_> = action
        .targets
        .iter()
        .map(|id| entities.iter().find(|e| e.id == *id))
        .collect::<Option<_>>()?;
    let mut values = vec![0.0; 28];
    values[action.key] = 1.0;
    values.extend([
        action.cost as f32,
        action.level as f32,
        action.amount as f32,
        targets.iter().filter(|e| e.id == actor).count() as f32,
        targets.iter().filter(|e| e.team == team).count() as f32,
        targets.iter().filter(|e| e.team != team).count() as f32,
    ]);
    values.extend(pool(&targets));
    Some(values)
}

fn action_input(candidate: &SearchCandidate, level: u32) -> Option<PolicyAction> {
    let (key, amount, targets) = match &candidate.action {
        PreparedPlainAction::BasicAttack {
            target,
            use_magic,
            amount,
        } => (usize::from(*use_magic), *amount, vec![*target]),
        PreparedPlainAction::BuiltinSkill(action) => {
            let index = BuiltinActiveSkill::CORE.iter().position(|skill| *skill == action.selected.skill)?;
            (index + 2, 0, action.targets.to_vec())
        }
        _ => return None,
    };
    Some(PolicyAction {
        key,
        cost: candidate.cost,
        level,
        amount,
        targets,
    })
}

/// 行动者用候选准备时快照；其他实体及状态保持回合边界，与本次训练数据一致。
pub(crate) fn score_turn(runtime: &CombatRuntime, turn: &SearchTurn) -> Option<Vec<f32>> {
    let actor = turn.actor?;
    let team = turn.team?;
    let actor_runtime = turn.actor_runtime.as_ref()?;
    let mut entities = Vec::new();
    for (id, e) in runtime.entities.iter() {
        let projected = ModelPlayerRuntime::from(if id == actor { actor_runtime } else { &e.runtime });
        let skills = ModelSkills::from(&e.template.skills);
        let states: Vec<_> = e.states.entries().iter().map(|entry| ModelPayload::from(&entry.payload)).collect();
        entities.push(EncodedEntity {
            id,
            team: projected.team,
            alive: projected.alive,
            values: entity_vector(id, e.template.max_hp, &projected, &skills, &states)?,
        });
    }
    let context = encode_context(&entities, actor, team)?;
    let mut actions = Vec::new();
    for (index, candidate) in turn.candidates.iter().enumerate() {
        let action = action_input(candidate, *turn.candidate_levels.get(index)?)?;
        actions.push(encode_action(&entities, actor, team, &action)?);
    }
    score_features(&context, &actions)
}

fn weights() -> &'static [f32] {
    static DECODED: OnceLock<Vec<f32>> = OnceLock::new();
    DECODED.get_or_init(|| WEIGHTS.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect())
}
fn normalize(values: &[f32], mean: &[f32], std: &[f32]) -> Vec<f32> {
    values
        .iter()
        .zip(mean)
        .zip(std)
        .map(|((&v, &m), &s)| ((v - m) / s).clamp(-10.0, 10.0))
        .collect()
}
fn linear(input: &[f32], weights: &[f32], bias: &[f32]) -> Vec<f32> {
    weights
        .chunks_exact(input.len())
        .zip(bias)
        .map(|(row, &b)| row.iter().zip(input).fold(b, |sum, (&w, &x)| sum + w * x))
        .collect()
}
fn silu(values: &mut [f32]) {
    for v in values {
        *v /= 1.0 + (-*v).exp();
    }
}

/// 纯 f32 推理复现 PyTorch eval：标准化、SiLU、LayerNorm 与情景/动作交互。
/// 返回排序 logits，不伪装成实际胜场或 Monte Carlo 样本。
pub fn score_features(context: &[f32], actions: &[Vec<f32>]) -> Option<Vec<f32>> {
    if context.len() != CONTEXT_WIDTH
        || context.iter().any(|v| !v.is_finite())
        || actions.iter().any(|a| a.len() != ACTION_WIDTH || a.iter().any(|v| !v.is_finite()))
    {
        return None;
    }
    let w = weights();
    let x = normalize(context, &w[..1318], &w[1318..2636]);
    let mut state = linear(&x, &w[3290..340698], &w[340698..340954]);
    silu(&mut state);
    let mean = state.iter().sum::<f32>() / 256.0;
    let variance = state.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 256.0;
    for i in 0..256 {
        state[i] = (state[i] - mean) / (variance + 1e-5).sqrt() * w[340954 + i] + w[341210 + i];
    }
    let mut state = linear(&state, &w[341466..374234], &w[374234..374362]);
    silu(&mut state);
    let mut scores = Vec::new();
    for input in actions {
        let a = normalize(input, &w[2636..2963], &w[2963..3290]);
        let mut a = linear(&a, &w[374362..416218], &w[416218..416346]);
        silu(&mut a);
        let mut a = linear(&a, &w[416346..432730], &w[432730..432858]);
        silu(&mut a);
        let mut interaction = state.clone();
        interaction.extend(&a);
        interaction.extend(state.iter().zip(&a).map(|(s, a)| s * a));
        let mut q = linear(&interaction, &w[432858..482010], &w[482010..482138]);
        silu(&mut q);
        scores.push(linear(&q, &w[482138..482266], &w[482266..482267])[0]);
    }
    scores.iter().all(|v| v.is_finite()).then_some(scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Deserialize)]
    struct SampleEntity {
        id: EntityIdx,
        max_hp: i32,
        runtime: ModelPlayerRuntime,
        skills: ModelSkills,
        states: Vec<ModelPayload>,
    }
    #[derive(serde::Deserialize)]
    struct Sample {
        actor: EntityIdx,
        team: usize,
        actor_runtime: ModelPlayerRuntime,
        entities: Vec<SampleEntity>,
        actions: Vec<PolicyAction>,
        context: Vec<f32>,
        action_features: Vec<Vec<f32>>,
        expected_logits: Vec<f32>,
    }

    #[test]
    fn real_decisions_match_python_features_and_pytorch_logits() {
        // 三个真实决策场景包含不同队伍规模与召唤物，不冻结源码或配置形状。
        let samples: Vec<Sample> = serde_json::from_str(include_str!("clever_policy/real-decisions.json")).unwrap();
        for sample in samples {
            let entities: Vec<_> = sample
                .entities
                .iter()
                .map(|e| {
                    let r = if e.id == sample.actor {
                        &sample.actor_runtime
                    } else {
                        &e.runtime
                    };
                    EncodedEntity {
                        id: e.id,
                        team: r.team,
                        alive: r.alive,
                        values: entity_vector(e.id, e.max_hp, r, &e.skills, &e.states).unwrap(),
                    }
                })
                .collect();
            let context = encode_context(&entities, sample.actor, sample.team).unwrap();
            for (a, b) in context.iter().zip(&sample.context) {
                assert!((a - b).abs() < 1e-4, "context {a} != {b}");
            }
            let actions: Vec<_> = sample
                .actions
                .iter()
                .map(|a| encode_action(&entities, sample.actor, sample.team, a).unwrap())
                .collect();
            for (actual, expected) in actions.iter().zip(&sample.action_features) {
                for (a, b) in actual.iter().zip(expected) {
                    assert!((a - b).abs() < 1e-4);
                }
            }
            let scores = score_features(&context, &actions).unwrap();
            for (a, b) in scores.iter().zip(&sample.expected_logits) {
                assert!((a - b).abs() < 2e-4, "logit {a} != {b}");
            }
            let best = |v: &[f32]| (0..v.len()).max_by(|&a, &b| v[a].total_cmp(&v[b]).then(b.cmp(&a))).unwrap();
            assert_eq!(best(&scores), best(&sample.expected_logits));
        }
    }
}
