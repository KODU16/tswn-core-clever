//! 按原版扫描份额累计技能次数；随机初相位只抽一次，使用时不重复掷骰。
use super::*;
use serde::{Deserialize, Serialize};

pub const RESERVE_UNIT: u64 = 1 << 32;

/// 接受0.1～100倍，保存六位小数，拒绝NaN、无穷和越界。
pub fn multiplier_micros(value: f64) -> Result<u32, &'static str> {
    if !value.is_finite() || !(0.1..=100.0).contains(&value) {
        return Err("技能倍率须为0.1～100之间的数字");
    }
    Ok((value * 1_000_000.0).round() as u32)
}

/// 按实体、固定技能槽及技能身份隔离，克隆体不会继承本体库存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReserve {
    pub actor: EntityIdx,
    pub lane: usize,
    pub skill_id: u32,
    pub initial_phase: u64,
    pub fraction: u64,
    pub earned_units: u64,
    pub issued: u64,
    pub consumed: u64,
}

impl SkillReserve {
    pub fn available(&self) -> u64 { self.issued - self.consumed }

    /// 固定点向下取整，累计误差每次小于 2^-32；不额外增加发动额度。
    pub(crate) fn accrue(&mut self, share: f64) {
        let units = (share.clamp(0.0, 1.0) * RESERVE_UNIT as f64).floor() as u64;
        self.earned_units = self.earned_units.checked_add(units).expect("技能累计额度溢出");
        let total = self.fraction + units;
        self.issued += total / RESERVE_UNIT;
        self.fraction = total % RESERVE_UNIT;
    }
}

impl CombatRuntime {
    /// 仅在合法行动机会累计；探测在副本上执行，正式结算重放同一随机相位。
    pub(crate) fn accrue_skill_reserve(&mut self, actor: EntityIdx, lane: usize, skill: BuiltinActiveSkill, share: f64) -> bool {
        let skill_id = self.entities.get(actor).unwrap().template.skills.skills()[lane].0;
        let key = self
            .scratch
            .skill_reserves
            .iter()
            .position(|r| r.actor == actor && r.lane == lane && r.skill_id == skill_id);
        let index = key.unwrap_or_else(|| {
            let mut phase = 0u64;
            for _ in 0..4 {
                phase = (phase << 8) | u64::from(self.rng.next_u8());
            }
            self.scratch.skill_reserves.push(SkillReserve {
                actor,
                lane,
                skill_id,
                initial_phase: phase,
                fraction: phase,
                earned_units: 0,
                issued: 0,
                consumed: 0,
            });
            self.scratch.skill_reserves.len() - 1
        });
        debug_assert_eq!(
            self.builtin_active_skill(self.entities.get(actor).unwrap().template.skills.skills()[lane]),
            Some(skill)
        );
        let entry = &mut self.scratch.skill_reserves[index];
        let multiplier = if self.scratch.clever_skill_multiplier_micros == 0 {
            1.0
        } else {
            f64::from(self.scratch.clever_skill_multiplier_micros) / 1_000_000.0
        };
        // 各项独立放大并封顶1；不归一总份额，高倍率可同时补充多个技能。
        entry.accrue(share * multiplier);
        entry.available() > 0
    }

    /// 只有正式选择的主动技扣一次；普攻、预览、其他目标及搜索副本不会扣真实库存。
    pub(crate) fn consume_skill_reserve(&mut self, actor: EntityIdx, lane: usize) {
        let skill_id = self.entities.get(actor).unwrap().template.skills.skills()[lane].0;
        let entry = self
            .scratch
            .skill_reserves
            .iter_mut()
            .find(|r| r.actor == actor && r.lane == lane && r.skill_id == skill_id)
            .expect("选择的技能必须已有储备");
        assert!(entry.available() > 0, "技能储备不足");
        entry.consumed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_uses_can_burst_but_never_exceed_original_shares() {
        // 两个原始熟练度64的技能，扫描份额为1/2和1/4，存40次机会再连续兑现。
        let mut a = SkillReserve {
            actor: EntityIdx(0),
            lane: 0,
            skill_id: 1,
            initial_phase: 0,
            fraction: 0,
            earned_units: 0,
            issued: 0,
            consumed: 0,
        };
        let mut b = a.clone();
        for _ in 0..40 {
            a.accrue(0.5);
            b.accrue(0.25);
        }
        assert_eq!((a.available(), b.available()), (20, 10));
        for _ in 0..10 {
            b.consumed += 1;
        }
        assert_eq!(b.available(), 0);
        for r in [&a, &b] {
            assert_eq!(r.initial_phase + r.earned_units, r.issued * RESERVE_UNIT + r.fraction);
            assert!(r.consumed <= r.issued);
        }
        // 无新额度时保留库存；降为低熟练度不会凭空清零或多发。
        let before = a.available();
        a.accrue(0.0);
        assert_eq!(a.available(), before);
        for _ in 0..128 {
            a.accrue(1.0 / 128.0);
        }
        assert_eq!(a.available(), before + 1);
    }
}
