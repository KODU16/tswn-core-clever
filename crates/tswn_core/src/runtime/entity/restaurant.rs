//! 餐厅记录技能的真实来源与待喂菜，随实体快照保存，续局不依赖显示层日志。
use super::EntityIdx;
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestaurantIngredient {
    pub label: String,
    /// 原版技能编号；人物材料为空。虚拟来源为空，真实来源保留实体身份。
    pub skill: Option<u8>,
    /// 非原版技能保存可重放入口，后续机制修改直接作用于菜品。
    #[serde(default)]
    pub invocation: Option<Box<crate::runtime::skill_call::SkillCall>>,
    pub source: Option<EntityIdx>,
    pub person: Option<EntityIdx>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RestaurantRuntime {
    pub history: Vec<RestaurantIngredient>,
    pub dish: Option<EntityIdx>,
    pub ingredients: Vec<RestaurantIngredient>,
    /// 额外待喂菜品各自保存配方，连同首道菜最多同时存活六道。
    pub dishes: Vec<(EntityIdx, Vec<RestaurantIngredient>)>,
    pub completed: u32,
    pub limit: u32,
    pub stopped: bool,
    /// 厨师属于非入场的虚拟实体，菜效果不会再次成为技能材料。
    pub chef: Option<EntityIdx>,
    pub feeding: bool,
}
