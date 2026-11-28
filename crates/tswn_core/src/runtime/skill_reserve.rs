//! 对齐即时对战 v6 的主动技能储备：同名合并、倍率归一和库存关联批量发放。
use super::*;
use serde::{Deserialize, Serialize};

pub const RESERVE_VERSION: u32 = 6;

/// 接受0.1～100倍，保存六位小数，拒绝NaN、无穷和越界。
pub fn multiplier_micros(value: f64) -> Result<u32, &'static str> {
    if !value.is_finite() || !(0.1..=100.0).contains(&value) {
        return Err("技能倍率须为0.1～100之间的数字");
    }
    Ok((value * 1_000_000.0).round() as u32)
}

/// 按实体及技能导出名共享库存；浮点用位模式保存，保证快照精确往返。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReserve {
    pub actor: EntityIdx,
    pub skill: String,
    pub initial_credit_bits: u64,
    /// 未发行额度可以超过1，不能截断成小数或自动兑换全部额度。
    pub credit_bits: u64,
    pub earned_bits: u64,
    pub issued: u64,
    pub consumed: u64,
}

impl SkillReserve {
    pub fn available(&self) -> u64 { self.issued - self.consumed }
    pub fn credit(&self) -> f64 { f64::from_bits(self.credit_bits) }
    pub fn earned(&self) -> f64 { f64::from_bits(self.earned_bits) }
    pub fn initial_credit(&self) -> f64 { f64::from_bits(self.initial_credit_bits) }
}

/// 与平台 prepare 相同：先合并同名份额，再逐项封顶、归一，最后统一发放。
fn prepare_reserves(
    reserves: &mut Vec<SkillReserve>,
    actor: EntityIdx,
    chances: &[(&str, f64)],
    multiplier: f64,
    mut roll: impl FnMut() -> f64,
) {
    let mut merged: Vec<(&str, f64)> = Vec::new();
    for &(skill, share) in chances {
        if let Some((_, total)) = merged.iter_mut().find(|(key, _)| *key == skill) {
            *total += share;
        } else {
            merged.push((skill, share));
        }
    }
    let mut eligible = Vec::new();
    let mut speeds = Vec::new();
    for (skill, share) in merged {
        let index = if let Some(index) = reserves.iter().position(|r| r.actor == actor && r.skill == skill) {
            index
        } else {
            // 与平台一致，仅首次合法时抽相位；零份额的合法技能也保留待发额度。
            let phase = roll().clamp(0.0, 1.0 - f64::EPSILON / 2.0);
            reserves.push(SkillReserve {
                actor,
                skill: skill.to_owned(),
                initial_credit_bits: phase.to_bits(),
                credit_bits: phase.to_bits(),
                earned_bits: 0.0f64.to_bits(),
                issued: 0,
                consumed: 0,
            });
            reserves.len() - 1
        };
        eligible.push(index);
        speeds.push((share * multiplier).clamp(0.0, 1.0));
    }
    let total: f64 = speeds.iter().sum();
    for (&index, speed) in eligible.iter().zip(speeds) {
        let q = if total > 1.0 { speed / total } else { speed };
        let r = &mut reserves[index];
        r.credit_bits = (r.credit() + q).to_bits();
        r.earned_bits = (r.earned() + q).to_bits();
    }
    // 库存统计包括当前不可用的技能；只发当前合法项，不清除其他库存或额度。
    let inventory: u64 = reserves.iter().filter(|r| r.actor == actor).map(SkillReserve::available).sum();
    let issue_limit = (eligible.len() as u64).saturating_sub(inventory).max(1);
    for _ in 0..issue_limit {
        let peak = eligible.iter().map(|&i| reserves[i].credit()).filter(|&c| c >= 1.0).reduce(f64::max);
        let Some(peak) = peak else { break };
        let candidates: Vec<_> = eligible
            .iter()
            .copied()
            .filter(|&i| reserves[i].credit() >= 1.0 && reserves[i].credit() >= peak - 0.5)
            .collect();
        let choice = if candidates.len() > 1 { roll() } else { 0.0 };
        let selected = candidates[((choice * candidates.len() as f64) as usize).min(candidates.len() - 1)];
        let r = &mut reserves[selected];
        r.credit_bits = (r.credit() - 1.0).to_bits();
        r.issued += 1;
    }
}

impl CombatRuntime {
    /// 专用 Clever Boss 固定100倍；其他角色保持本场设置，绝不改写全局倍率。
    pub(crate) fn clever_actor_multiplier_micros(&self, actor: EntityIdx) -> u32 {
        if self.is_clever_boss(actor) {
            100_000_000
        } else if self.scratch.clever_skill_multiplier_micros == 0 {
            1_000_000
        } else {
            self.scratch.clever_skill_multiplier_micros
        }
    }
    /// 只识别真实 Boss，本体分身保留 Boss 身份时同样采用固定策略。
    pub(crate) fn is_clever_boss(&self, actor: EntityIdx) -> bool {
        self.entities
            .get(actor)
            .is_some_and(|e| e.template.name == "clever" && e.runtime.flags.contains(PlayerKindFlags::BOSS))
    }
    /// 所有技能统一准备一次，避免逐技能发放绕过库存批量限制。
    pub(crate) fn prepare_skill_reserves(&mut self, actor: EntityIdx, chances: &[(&str, f64)]) {
        let multiplier = f64::from(self.clever_actor_multiplier_micros(actor)) / 1_000_000.0;
        let rng = &mut self.rng;
        prepare_reserves(&mut self.scratch.skill_reserves, actor, chances, multiplier, || {
            // 53位均匀相位；独立于Python随机数实现，不读取正式战斗未来随机流。
            let mut bits = 0u64;
            for _ in 0..7 {
                bits = (bits << 8) | u64::from(rng.next_u8());
            }
            (bits >> 3) as f64 / (1u64 << 53) as f64
        });
    }

    pub(crate) fn has_skill_reserve(&self, actor: EntityIdx, skill: BuiltinActiveSkill) -> bool {
        self.scratch
            .skill_reserves
            .iter()
            .any(|r| r.actor == actor && r.skill == skill.export_name() && r.available() > 0)
    }

    /// 同名多槽共享一次扣除；未选中的槽、目标和预览不额外消耗库存。
    pub(crate) fn consume_skill_reserve(&mut self, actor: EntityIdx, skill: BuiltinActiveSkill) {
        let entry = self
            .scratch
            .skill_reserves
            .iter_mut()
            .find(|r| r.actor == actor && r.skill == skill.export_name())
            .expect("选择的技能必须已有储备");
        assert!(entry.available() > 0, "技能储备不足");
        entry.consumed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conserved(reserves: &[SkillReserve]) {
        for r in reserves {
            assert!((r.initial_credit() + r.earned() - r.issued as f64 - r.credit()).abs() < 1e-10);
            assert!(r.consumed <= r.issued);
        }
    }

    #[test]
    fn repeated_issuance_matches_instant_v6_reference_outcomes() {
        // 平台v6实际运行的24步结果，覆盖倍率、禁用恢复、空机会、抽签和消耗。
        let mut stock = Vec::new();
        let mut draws = 0usize;
        for step in 0..24 {
            let chances: &[(&str, f64)] = if step == 10 {
                &[]
            } else if step % 6 < 4 {
                &[("a", 0.5), ("b", 0.25), ("c", 0.125)]
            } else {
                &[("b", 0.5), ("c", 0.25)]
            };
            prepare_reserves(&mut stock, EntityIdx(0), chances, 4.0, || {
                let value = ((draws * 37 % 101) as f64 + 0.5) / 101.0;
                draws += 1;
                value
            });
            if !chances.is_empty() {
                let skill = chances[step % chances.len()].0;
                let r = stock.iter_mut().find(|r| r.skill == skill).unwrap();
                if r.available() > 0 {
                    r.consumed += 1;
                }
            }
            conserved(&stock);
        }
        assert_eq!(draws, 12);
        for (r, (credit, earned, issued, consumed)) in stock.iter().zip([
            (0.404950495049505, 6.400000000000001, 6, 5),
            (0.27128712871287197, 9.900000000000002, 10, 7),
            (1.4376237623762373, 6.700000000000002, 6, 6),
        ]) {
            assert!((r.credit() - credit).abs() < 1e-12);
            assert!((r.earned() - earned).abs() < 1e-12);
            assert_eq!((r.issued, r.consumed), (issued, consumed));
        }
    }

    #[test]
    fn multiplier_normalizes_after_merging_same_skill() {
        // 同名两个槽合并为0.75，与0.25一起乘4、各封顶1后归一为各0.5。
        let mut stock = Vec::new();
        for _ in 0..40 {
            prepare_reserves(
                &mut stock,
                EntityIdx(0),
                &[("fire", 0.5), ("fire", 0.25), ("ice", 0.25)],
                4.0,
                || 0.0,
            );
        }
        assert_eq!(stock.len(), 2);
        assert_eq!(stock[0].earned(), 20.0);
        assert_eq!(stock[1].earned(), 20.0);
        assert_eq!(stock.iter().map(SkillReserve::available).sum::<u64>(), 39);
        assert_eq!(stock.iter().map(SkillReserve::credit).sum::<f64>(), 1.0);
        conserved(&stock);
    }

    #[test]
    fn issuance_uses_inventory_peak_window_and_keeps_unissued_credit() {
        // 单次库存少时可发两张；库存达到合法项数后最多补一张，保留超过1的额度。
        let mut stock = Vec::new();
        prepare_reserves(&mut stock, EntityIdx(0), &[("a", 0.5), ("b", 0.5)], 1.0, || 0.75);
        assert_eq!(stock.iter().map(SkillReserve::available).sum::<u64>(), 2);
        for r in &mut stock {
            r.credit_bits = 2.0f64.to_bits();
        }
        let mut draws = 0;
        prepare_reserves(&mut stock, EntityIdx(0), &[("a", 0.5), ("b", 0.5)], 1.0, || {
            draws += 1;
            0.99
        });
        assert_eq!(draws, 1);
        assert_eq!(stock[1].available(), 2);
        assert_eq!(stock[0].credit(), 2.5);
        // 最高额度之外超过0.5的项不能参与这次发行抽取。
        stock[0].credit_bits = 4.0f64.to_bits();
        stock[1].credit_bits = 2.0f64.to_bits();
        prepare_reserves(&mut stock, EntityIdx(0), &[("a", 0.0), ("b", 0.0)], 1.0, || {
            panic!("唯一最高项无需抽签")
        });
        assert_eq!(stock[0].available(), 2);
        assert_eq!(stock[0].credit(), 3.0);
    }

    #[test]
    fn unavailable_stock_is_preserved_and_can_burst_after_saving() {
        let mut stock = Vec::new();
        for _ in 0..40 {
            prepare_reserves(&mut stock, EntityIdx(0), &[("a", 0.5), ("b", 0.25)], 1.0, || 0.0);
        }
        let before = stock.clone();
        prepare_reserves(&mut stock, EntityIdx(0), &[], 1.0, || panic!("无合法项不抽签"));
        assert_eq!(stock, before);
        for _ in 0..8 {
            stock[1].consumed += 1;
        }
        assert_eq!(stock[1].consumed, 8);
        // 新实体即使技能同名也从独立相位开始，不能继承旧实体的库存。
        prepare_reserves(&mut stock, EntityIdx(1), &[("a", 0.5)], 1.0, || 0.0);
        assert_eq!(stock[2].available(), 0);
        conserved(&stock);
    }
}
