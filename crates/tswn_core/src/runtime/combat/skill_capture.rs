//! 捕获实际执行入口；日志名称只用于显示，不参与技能效果匹配。
use super::*;
use crate::runtime::skill_call::SkillCall;
impl CombatRuntime {
    /// 普通战斗不分配凭据，喂菜的派生调用也不再次入库。
    pub(crate) fn begin_skill_capture(
        &self,
        entry: &str,
        caster: EntityIdx,
        arguments: impl FnOnce() -> Vec<String>,
        updates: &RunUpdates,
    ) -> Option<(usize, SkillCall)> {
        if self.restaurant_bosses.is_empty() || self.scratch.replay_skill_target.is_some() || self.is_restaurant_actor(caster) {
            return None;
        }
        let mut state = self.entities.get(caster)?.runtime.clone();
        // 批次编号只用于反馈链去重，不属于可重放的技能情景。
        state.counter.last_updates_id = None;
        state.counter_xuan_yue.last_updates_id = None;
        if let Some(s) = &mut state.rosmontic {
            s.restaurant = None;
        }
        Some((
            updates.updates.len(),
            SkillCall {
                entry: entry.to_owned(),
                arguments: arguments(),
                caster,
                caster_state: Box::new(state),
            },
        ))
    }
    /// 内层实际招式优先，外层随机选招器不会覆盖已经标明的调用。
    pub(crate) fn end_skill_capture(&self, capture: Option<(usize, SkillCall)>, updates: &mut RunUpdates) {
        let Some((start, call)) = capture else { return };
        for event in updates.updates.iter_mut().skip(start) {
            if event.caster != call.caster.0 as usize || event.dialogue || event.skill_call.is_some() {
                continue;
            }
            if let Some(name) = event.skill_name.clone().or_else(|| captured_skill_name(&event.message)) {
                event.skill_name = Some(name);
                event.skill_call = Some(Box::new(call.clone()));
            }
        }
    }
    /// 目标覆盖只在菜品重放期间生效，原技能的伤害与状态链保持原入口。
    pub(crate) fn captured_skill_target(&self, caster: EntityIdx) -> Option<EntityIdx> {
        self.scratch.replay_skill_target.filter(|(a, _)| *a == caster).map(|(_, t)| t)
    }
    pub(crate) fn replay_captured_skill(&mut self, call: &SkillCall, target: EntityIdx, updates: &mut RunUpdates) -> bool {
        if self.replay_skill_alice_ran(call, target, updates) {
            return true;
        }
        if self.replay_skill_cirno(call, target, updates) {
            return true;
        }
        if self.replay_skill_conan(call, target, updates) {
            return true;
        }
        if self.replay_skill_fish(call, target, updates) {
            return true;
        }
        if self.replay_skill_flandre_letty_chen(call, target, updates) {
            return true;
        }
        if self.replay_skill_mario(call, target, updates) {
            return true;
        }
        if self.replay_skill_pcb_hooks(call, target, updates) {
            return true;
        }
        if self.replay_skill_pcb_summons(call, target, updates) {
            return true;
        }
        if self.replay_skill_phantom_youmu_yuyuko(call, target, updates) {
            return true;
        }
        if self.replay_skill_quilt(call, target, updates) {
            return true;
        }
        if self.replay_skill_rc(call, target, updates) {
            return true;
        }
        if self.replay_skill_reimu_marisa(call, target, updates) {
            return true;
        }
        if self.replay_skill_remaining(call, target, updates) {
            return true;
        }
        if self.replay_skill_remaining_children(call, target, updates) {
            return true;
        }
        if self.replay_skill_remaining_hooks(call, target, updates) {
            return true;
        }
        if self.replay_skill_remaining_skills(call, target, updates) {
            return true;
        }
        if self.replay_skill_return_void(call, target, updates) {
            return true;
        }
        if self.replay_skill_rosmontic(call, target, updates) {
            return true;
        }
        if self.replay_skill_rumia(call, target, updates) {
            return true;
        }
        if self.replay_skill_scarlet_mansion(call, target, updates) {
            return true;
        }
        if self.replay_skill_scarlet_support(call, target, updates) {
            return true;
        }
        if self.replay_skill_slime(call, target, updates) {
            return true;
        }
        if self.replay_skill_smdd(call, target, updates) {
            return true;
        }
        if self.replay_skill_squall(call, target, updates) {
            return true;
        }
        if self.replay_skill_thanos(call, target, updates) {
            return true;
        }
        if self.replay_skill_tiger(call, target, updates) {
            return true;
        }
        if self.replay_skill_touhou(call, target, updates) {
            return true;
        }
        if self.replay_skill_wheel(call, target, updates) {
            return true;
        }
        if self.replay_skill_xjy(call, target, updates) {
            return true;
        }
        if self.replay_skill_yi(call, target, updates) {
            return true;
        }
        if self.replay_skill_yuri(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_assassinate(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_boss(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_control(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_debuff(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_lifecycle(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_offense(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_summon(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_team(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_weapon(call, target, updates) {
            return true;
        }
        if self.replay_skill_standard_zombie(call, target, updates) {
            return true;
        }
        false
    }
}
/// 接收任意名称，不包含原版或 Boss 技能白名单；非技能状态消息不入库。
pub(super) fn captured_skill_name(text: &str) -> Option<String> {
    let rest = text.strip_prefix("[0]")?.trim_start();
    // 动词可出现在“为[1]使用”等句式中，名称仍从原始发动事件读取。
    for verb in [
        "使用",
        "发动",
        "发起",
        "开始",
        "释放",
        "施放",
        "发射",
        "投掷",
        "准备",
        "展开",
        "呼叫",
        "装备",
        "执行",
        "触发",
        "进行",
        "打了个",
        "说了一声",
        "召唤了",
    ] {
        if let Some((_, tail)) = rest.split_once(&format!("{verb}[")) {
            let name = tail.split_once(']')?.0;
            if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                return Some(name.to_owned());
            }
        }
    }
    let tail = rest.strip_prefix('[')?;
    let (name, suffix) = tail.split_once(']')?;
    // 单独的技能标题可抓取，“[回避]了攻击”等结果消息不当作一次出招。
    (suffix.is_empty() || suffix.starts_with(['，', '。', '！', '!', ' ']))
        .then(|| name.to_owned())
        .filter(|name| !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()))
}
