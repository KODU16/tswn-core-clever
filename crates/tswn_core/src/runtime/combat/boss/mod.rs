//! Boss 专属机制族：每只 Boss 一个文件，文件名表达具体 Boss 对象。
//!
//! 与 [`super::skills::boss`]（Boss 技能的三个 match 派发）是不同层次：这里是单只
//! Boss 的专属实现，`skills/boss.rs` 是「技能 → Boss 技能」的派发逻辑。

pub mod cirno;
#[cfg(test)]
pub mod cirno_tests;
pub mod conan;
pub mod fish;
#[cfg(test)]
pub mod fish_tests;
pub mod mario;
pub mod quilt;
#[cfg(test)]
pub mod quilt_tests;
// 涵虚状态与技能独立实现，伤害钩子接入共用链。
pub mod rc;
#[cfg(test)]
mod rc_tests;
mod reimu_marisa;
mod return_void;
// 餐厅的免疫、食材记录与两回合做菜流程。
mod restaurant;
mod restaurant_methods;
#[cfg(test)]
mod restaurant_tests;
#[cfg(test)]
mod return_void_tests;
pub mod rosmontic;
#[cfg(test)]
mod rosmontic_tests;
mod rumia;
mod scarlet_mansion;
mod scarlet_support;
pub mod slime;
pub mod smdd;
#[cfg(test)]
pub mod smdd_tests;
pub mod squall;
#[cfg(test)]
pub mod squall_tests;
pub mod thanos;
pub mod tiger;
#[cfg(test)]
pub mod tiger_tests;
// 最后一批角色与小队共享状态、支援和防御钩子。
mod remaining;
mod remaining_children;
mod remaining_hooks;
mod remaining_skills;
#[cfg(test)]
mod remaining_tests;
mod touhou;
// 第二批东方角色共用召唤与状态钩子，招式按机制族拆分。
mod alice_ran;
mod flandre_letty_chen;
mod pcb_hooks;
mod pcb_summons;
#[cfg(test)]
mod pcb_tests;
mod phantom_youmu_yuyuko;
#[cfg(test)]
mod touhou_tests;
pub mod wheel;
pub mod xjy;
#[cfg(test)]
pub mod xjy_tests;
pub mod yi;
#[cfg(test)]
pub mod yi_tests;
pub mod yuri;
