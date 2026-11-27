//! 技能执行凭据：编译期自动记录入口和实参，重放调用当前实现，不复制技能效果。
use super::{EntityIdx, PlayerRuntime};
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillCall {
    pub entry: String,
    pub arguments: Vec<String>,
    pub caster: EntityIdx,
    /// 餐厅字段在捕获时清空，避免递归保存配方。
    pub caster_state: Box<PlayerRuntime>,
}
