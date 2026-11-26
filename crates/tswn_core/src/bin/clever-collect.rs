//! clever-collect：离线战斗数据采集 CLI（CPU_SIMULATION_AGENT.md 协议）。
//!
//! 把 fAIgFUL 名字池变成 Clever 出招策略训练数据：
//! - 确定性集合划分：sha256("clever-names-v1:"+完整名字) 前 4 字节 % 100；
//! - 同一 split 内抽两队（1v1..4v4），正式轨迹只用原版策略；
//! - 在真实决策机会上分叉：每个候选动作与 baseline 共用同一组试算 seed 续局
//!   （采集核心在 tswn_core::runtime::collect，本 bin 只做 CLI/分片/落盘）；
//! - 输出 UTF-8 JSONL.gz 分片（states/decisions）+ manifest 等审计文件。
//!
//! 样本内容只由 (master_seed, 配置, battle_index) 决定；线程数与执行顺序不影响样本。

use clap::{Args, Parser};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use tswn_core::runtime::collect::{self, CollectConfig, CollectedBattle, CollectedDecision, RNG_BOUNDARY_DESC};

/// 本协议的数据格式版本；字段语义变化时递增。
const SCHEMA_VERSION: u64 = 1;
/// 每场战斗最多采集的决策数（规范固定值）。
const MAX_DECISIONS_PER_BATTLE: usize = 4;

/// 提取 CSV 行的首列（candidate_id），处理引号包裹与双引号转义
/// （名字可能含 `"`、`,` 等字符，CSV 写入端按标准规则加引号）。
fn csv_first_field(line: &str) -> String {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => match chars.next() {
                    Some('"') => out.push('"'),
                    _ => break, // 字段结束引号
                },
                _ => out.push(c),
            }
        }
        return out;
    }
    line.split(',').next().unwrap_or("").trim().to_owned()
}

// ---------------------------------------------------------------------------
// 确定性工具
// ---------------------------------------------------------------------------

/// sha256 摘要的十六进制文本。
fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 字符串 sha256 前 8 字节（小端）作为 u64。
fn sha256_u64(text: &str) -> u64 {
    let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

/// splitmix64：u64 上的双射（与 GPU 构造器同一实现）。
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

/// 规范集合划分规则；返回 ("train"|"validation"|"test")。
fn split_of(full_name: &str) -> &'static str {
    let digest: [u8; 32] = Sha256::digest(format!("clever-names-v1:{full_name}").as_bytes()).into();
    let bucket = u32::from_be_bytes(digest[..4].try_into().unwrap()) % 100;
    if bucket < 80 {
        "train"
    } else if bucket < 90 {
        "validation"
    } else {
        "test"
    }
}

/// 小批次子集条件：digest[4] == 0（规范定义）。
fn in_d4_zero_subset(full_name: &str) -> bool {
    let digest: [u8; 32] = Sha256::digest(format!("clever-names-v1:{full_name}").as_bytes()).into();
    digest[4] == 0
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Clone)]
#[command(
    name = "clever-collect",
    version,
    about = "离线战斗决策数据采集（CPU_SIMULATION_AGENT.md）"
)]
struct Cli {
    #[command(flatten)]
    collect: CollectArgs,
}

#[derive(Args, Clone)]
struct CollectArgs {
    /// 名字 CSV（candidate_id 列 = 含 @ 后缀的完整名字）。
    #[arg(long)]
    names_csv: PathBuf,
    /// 输出目录（分片、manifest、审计文件）。
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "clever-data-v1")]
    master_seed: String,
    #[arg(long, default_value_t = 20)]
    workers: usize,
    /// 每个候选与 baseline 的续局次数。
    #[arg(long, default_value_t = 64)]
    trials: usize,
    /// 单次续局的回合预算（含正式轨迹）。
    #[arg(long, default_value_t = 10000)]
    max_rounds: u64,
    /// 目标独立决策数（按 80/10/10 分配三集合）。
    #[arg(long, default_value_t = 160)]
    decisions: usize,
    /// 初始队伍规模集合（循环使用）。
    #[arg(long, default_value = "1,2,3,4")]
    team_sizes: String,
    /// 名字子集：all=全量池；d4-zero=digest[4]==0 小批次池。
    #[arg(long, default_value = "all")]
    subset: String,
    /// 断点续跑：跳过已落盘分片，其余配置必须一致。
    #[arg(long)]
    resume: bool,
    /// 每分片目标决策数（100~500）。
    #[arg(long, default_value_t = 200)]
    decisions_per_shard: usize,
    /// 源归档 SHA-256（由外部脚本传入，写入 manifest）。
    #[arg(long)]
    source_archive_sha256: Option<String>,
    /// 审计模式：用引擎名字构造 API 重新生成属性与技能，供与 CSV 核对（配合 --audit-count）。
    #[arg(long)]
    audit_names: bool,
    /// 审计模式抽样个数。
    #[arg(long, default_value_t = 128)]
    audit_count: usize,
    /// 验收模式：从进度日志复跑 N 场纯原版正式轨迹，与采集时的胜者/回合数比对。
    #[arg(long)]
    verify_legacy: Option<usize>,
}

// ---------------------------------------------------------------------------
// 名字池与战斗规划
// ---------------------------------------------------------------------------

struct NamePool {
    split: &'static str,
    names: Vec<String>,
}

fn load_pools(csv: &Path, subset: &str) -> Result<(Vec<NamePool>, usize), String> {
    let file = File::open(csv).map_err(|e| format!("打开 CSV 失败：{e}"))?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header).map_err(|e| format!("读取 CSV 表头失败：{e}"))?;
    let first_col = header.split(',').next().unwrap_or("").trim();
    if first_col != "candidate_id" {
        return Err(format!("CSV 首列应为 candidate_id（实际：{first_col:?}）"));
    }
    let use_d4 = match subset {
        "all" => false,
        "d4-zero" => true,
        other => return Err(format!("未知子集规则：{other}（应为 all 或 d4-zero）")),
    };
    let mut pools = [Vec::new(), Vec::new(), Vec::new()];
    let mut seen = HashSet::new();
    let mut rows = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| format!("读取 CSV 失败：{e}"))?;
        if n == 0 {
            break;
        }
        rows += 1;
        // 只取首列（完整名字，含 @ 后缀）；名字由引擎构造，不进网络特征。
        let full_name = csv_first_field(&line);
        if full_name.is_empty() || !seen.insert(full_name.to_owned()) {
            continue;
        }
        if use_d4 && !in_d4_zero_subset(&full_name) {
            continue;
        }
        match split_of(&full_name) {
            "train" => pools[0].push(full_name.to_owned()),
            "validation" => pools[1].push(full_name.to_owned()),
            _ => pools[2].push(full_name.to_owned()),
        }
    }
    let [p0, p1, p2] = pools;
    let pool_list = vec![
        NamePool {
            split: "train",
            names: p0,
        },
        NamePool {
            split: "validation",
            names: p1,
        },
        NamePool {
            split: "test",
            names: p2,
        },
    ];
    Ok((pool_list, rows))
}

/// 一场战斗的确定性规划：split、队伍规模与两队名单。
struct BattlePlan {
    battle_index: u64,
    split: &'static str,
    team_size: usize,
    groups: Vec<Vec<String>>,
}

/// 战斗序列规划：split 按 80/10/10 循环，队伍规模按给定集合循环。
fn plan_battle(battle_index: u64, master_seed: &str, pools: &[NamePool], sizes: &[usize]) -> Result<BattlePlan, String> {
    let split = match battle_index % 10 {
        0..=7 => "train",
        8 => "validation",
        _ => "test",
    };
    let pool = pools.iter().find(|p| p.split == split).ok_or("缺 split 名字池")?;
    let team_size = sizes[(battle_index as usize) % sizes.len()];
    if pool.names.len() < team_size * 2 {
        return Err(format!(
            "split {split} 名字池过小（{}，需要 {}）",
            pool.names.len(),
            team_size * 2
        ));
    }
    let mut rng = splitmix64(sha256_u64(&format!("{master_seed}:roster:{split}:{battle_index:08}")));
    // 无放回均匀抽样：同场内名字不重复。
    let mut picked: Vec<usize> = Vec::with_capacity(team_size * 2);
    while picked.len() < team_size * 2 {
        let candidate = (rng % pool.names.len() as u64) as usize;
        rng = splitmix64(rng);
        if picked.contains(&candidate) {
            continue;
        }
        picked.push(candidate);
    }
    let team_a: Vec<String> = picked[..team_size].iter().map(|&i| pool.names[i].clone()).collect();
    let team_b: Vec<String> = picked[team_size..].iter().map(|&i| pool.names[i].clone()).collect();
    Ok(BattlePlan {
        battle_index,
        split,
        team_size,
        groups: vec![team_a, team_b],
    })
}

// ---------------------------------------------------------------------------
// 战斗报告（worker 线程 → 写线程）
// ---------------------------------------------------------------------------

struct BattleReport {
    battle_index: u64,
    split: &'static str,
    team_size: usize,
    decisions: Vec<CollectedDecision>,
    audit_line: Value,
    skip_counts: BTreeMap<&'static str, u64>,
    error: Option<String>,
}

fn error_report(battle_index: u64, split: &'static str, team_size: usize, error: String) -> BattleReport {
    BattleReport {
        battle_index,
        split,
        team_size,
        decisions: Vec::new(),
        audit_line: json!({"battle_index": battle_index, "error": error}),
        skip_counts: BTreeMap::new(),
        error: Some(error),
    }
}

fn run_battle(args: &CollectArgs, plan: &BattlePlan) -> BattleReport {
    let config = CollectConfig {
        trials: args.trials,
        max_rounds: args.max_rounds,
    };
    let seed_line = format!("clever-collect:{}:{}:{:08}", args.master_seed, plan.split, plan.battle_index);
    match collect::collect_battle(&plan.groups, &seed_line, plan.battle_index, plan.split, &config) {
        Ok(CollectedBattle {
            decisions,
            mut audit_line,
            skip_counts,
        }) => {
            audit_line["team_size"] = json!(plan.team_size);
            BattleReport {
                battle_index: plan.battle_index,
                split: plan.split,
                team_size: plan.team_size,
                decisions,
                audit_line,
                skip_counts,
                error: None,
            }
        }
        Err(error) => error_report(plan.battle_index, plan.split, plan.team_size, error),
    }
}

// ---------------------------------------------------------------------------
// 输出落盘（单写线程）
// ---------------------------------------------------------------------------

struct ShardState {
    decisions: Vec<(String, Value, Value)>, // (decision_id, state_line, decision_line)
    battles_seen: BTreeMap<u64, usize>,
    errors: Vec<Value>,
    by_split: BTreeMap<&'static str, u64>,
    /// 分片内汇总（仅已落盘前缀计入最终 summary）。
    totals: Totals,
}

fn write_gzip_jsonl(path: &Path, lines: &[Value]) -> Result<String, String> {
    let tmp = path.with_extension("jsonl.gz.tmp");
    {
        let file = File::create(&tmp).map_err(|e| format!("创建临时分片失败：{e}"))?;
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        for line in lines {
            serde_json::to_writer(&mut encoder, line).map_err(|e| format!("序列化分片行失败：{e}"))?;
            encoder.write_all(b"\n").map_err(|e| format!("写分片失败：{e}"))?;
        }
        encoder.finish().map_err(|e| format!("关闭分片失败：{e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("分片提交失败：{e}"))?;
    let data = fs::read(path).map_err(|e| format!("回读分片失败：{e}"))?;
    Ok(sha256_hex(&data))
}

fn flush_shard(out: &Path, index: u64, shard: &ShardState) -> Result<(usize, Value), String> {
    let mut sorted: Vec<_> = shard.decisions.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let states: Vec<Value> = sorted.iter().map(|(_, s, _)| (*s).clone()).collect();
    let decisions: Vec<Value> = sorted.iter().map(|(_, _, d)| (*d).clone()).collect();
    let states_sha = write_gzip_jsonl(&out.join(format!("states-{index:06}.jsonl.gz")), &states)?;
    let decisions_sha = write_gzip_jsonl(&out.join(format!("decisions-{index:06}.jsonl.gz")), &decisions)?;
    let complete = json!({
        "schema_version": SCHEMA_VERSION,
        "shard_index": index,
        "battles": shard.battles_seen.keys().map(|b| *b).collect::<Vec<_>>(),
        "decisions": states.len(),
        "states_sha256": states_sha,
        "decisions_sha256": decisions_sha,
        "failed_battles": shard.errors,
    });
    let path = out.join(format!("complete-{index:06}.json"));
    let tmp = out.join(format!("complete-{index:06}.json.tmp"));
    fs::write(&tmp, serde_json::to_string_pretty(&complete).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写 complete 失败：{e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("complete 提交失败：{e}"))?;
    Ok((states.len(), complete))
}

/// 汇总统计（贯穿写线程生命周期）。
#[derive(Default)]
struct Totals {
    decisions: u64,
    rollouts: u64,
    unresolved: u64,
    failed_battles: u64,
    battles_done: u64,
    by_split: BTreeMap<&'static str, u64>,
    by_team_size: BTreeMap<usize, u64>,
    skip_counts: BTreeMap<String, u64>,
    candidates_sum: u64,
    skipped_candidates: u64,
    rounds_sum: u64,
    decisions_with_boundary: u64,
}

fn write_progress(out: &Path, totals: &Totals) -> Result<(), String> {
    let json = json!({
        "decisions": totals.decisions,
        "rollouts": totals.rollouts,
        "unresolved": totals.unresolved,
        "failed_battles": totals.failed_battles,
        "battles_done": totals.battles_done,
        "by_split": totals.by_split,
        "by_team_size": totals.by_team_size,
        "skip_counts": totals.skip_counts,
    });
    fs::write(
        out.join("progress.json"),
        serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 progress 失败：{e}"))?;
    Ok(())
}

/// 缓冲一份战斗报告；错误单独记日志。
fn buffer_report(
    report: &BattleReport,
    shards: &mut BTreeMap<u64, ShardState>,
    totals: &mut Totals,
    journal_writer: &mut impl Write,
    battles_per_shard: u64,
) -> Result<(), String> {
    let shard_index = report.battle_index / battles_per_shard;
    let shard = shards.entry(shard_index).or_insert(ShardState {
        decisions: Vec::new(),
        battles_seen: BTreeMap::new(),
        errors: Vec::new(),
        by_split: BTreeMap::new(),
        totals: Totals::default(),
    });
    shard.battles_seen.insert(report.battle_index, report.decisions.len());
    if let Some(_error) = &report.error {
        shard.errors.push(report.audit_line.clone());
        totals.failed_battles += 1;
        serde_json::to_writer(&mut *journal_writer, &report.audit_line).map_err(|e| format!("写进度日志失败：{e}"))?;
        journal_writer.write_all(b"\n").map_err(|e| e.to_string())?;
        journal_writer.flush().map_err(|e| e.to_string())?;
        return Ok(());
    }
    for decision in &report.decisions {
        shard.decisions.push((
            decision.decision_id.clone(),
            decision.state_line.clone(),
            decision.decision_line.clone(),
        ));
        totals.decisions += 1;
        totals.rollouts += decision.rollouts;
        totals.unresolved += decision.unresolved as u64;
        totals.candidates_sum += decision.candidates as u64;
        totals.skipped_candidates += decision.skipped_candidates as u64;
        totals.rounds_sum += report.audit_line["rounds"].as_u64().unwrap_or(0);
        if decision.state_line["round_boundary_state"].is_object() {
            totals.decisions_with_boundary += 1;
        }
        *totals.by_split.entry(report.split).or_insert(0) += 1;
        *totals.by_team_size.entry(report.team_size).or_insert(0) += 1;
        *shard.by_split.entry(report.split).or_insert(0) += 1;
        shard.totals.decisions += 1;
        shard.totals.rollouts += decision.rollouts;
        shard.totals.unresolved += decision.unresolved as u64;
        shard.totals.candidates_sum += decision.candidates as u64;
        shard.totals.skipped_candidates += decision.skipped_candidates as u64;
        shard.totals.rounds_sum += report.audit_line["rounds"].as_u64().unwrap_or(0);
        if decision.state_line["round_boundary_state"].is_object() {
            shard.totals.decisions_with_boundary += 1;
        }
        *shard.totals.by_split.entry(report.split).or_insert(0) += 1;
        *shard.totals.by_team_size.entry(report.team_size).or_insert(0) += 1;
    }
    for (reason, count) in &report.skip_counts {
        *totals.skip_counts.entry((*reason).to_owned()).or_insert(0) += *count;
        *shard.totals.skip_counts.entry((*reason).to_owned()).or_insert(0) += *count;
    }
    totals.battles_done += 1;
    shard.totals.battles_done += 1;
    // 进度日志记录每场战斗的审计行（正式轨迹复现信息 + 终止状态），resume 与验收依赖它。
    serde_json::to_writer(&mut *journal_writer, &report.audit_line).map_err(|e| format!("写进度日志失败：{e}"))?;
    journal_writer.write_all(b"\n").map_err(|e| e.to_string())?;
    journal_writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// 前缀顺序落盘：只有分片内战斗齐了且序号紧跟已落盘前缀才写盘；
/// 在每次落盘后按前缀累计判断停止条件，保证停止边界确定。
#[allow(clippy::too_many_arguments)]
fn flush_prefix(
    out: &Path,
    shards: &mut BTreeMap<u64, ShardState>,
    flushed_prefix: &mut u64,
    decisions_by_split: &mut BTreeMap<&'static str, u64>,
    journal_writer: &mut impl Write,
    battles_per_shard: u64,
    max_battles: u64,
    stop: &AtomicBool,
    final_shard: &mut Option<u64>,
    targets: &[(usize, &'static str)],
    flushed_totals: &mut Totals,
) -> Result<(), String> {
    loop {
        let Some(shard) = shards.remove(flushed_prefix) else { break };
        let shard_start = *flushed_prefix * battles_per_shard;
        let shard_end = ((*flushed_prefix + 1) * battles_per_shard - 1).min(max_battles - 1);
        let complete = (shard_start..=shard_end).all(|b| shard.battles_seen.contains_key(&b));
        if !complete {
            shards.insert(*flushed_prefix, shard);
            break;
        }
        let (n, complete_json) = flush_shard(out, *flushed_prefix, &shard)?;
        for (split, count) in &shard.by_split {
            *decisions_by_split.entry(split).or_insert(0) += *count;
        }
        // 分片内统计并入已落盘前缀汇总（最终 summary 只反映前缀内容）。
        flushed_totals.decisions += shard.totals.decisions;
        flushed_totals.rollouts += shard.totals.rollouts;
        flushed_totals.unresolved += shard.totals.unresolved;
        flushed_totals.candidates_sum += shard.totals.candidates_sum;
        flushed_totals.skipped_candidates += shard.totals.skipped_candidates;
        flushed_totals.rounds_sum += shard.totals.rounds_sum;
        flushed_totals.decisions_with_boundary += shard.totals.decisions_with_boundary;
        flushed_totals.battles_done += shard.totals.battles_done;
        for (split, count) in &shard.totals.by_split {
            *flushed_totals.by_split.entry(split).or_insert(0) += *count;
        }
        for (size, count) in &shard.totals.by_team_size {
            *flushed_totals.by_team_size.entry(*size).or_insert(0) += *count;
        }
        for (reason, count) in &shard.totals.skip_counts {
            *flushed_totals.skip_counts.entry(reason.clone()).or_insert(0) += *count;
        }
        flushed_totals.failed_battles += shard.errors.len() as u64;
        eprintln!("[clever-collect] 分片 {flushed_prefix:06} 落盘：{n} 决策");
        let journal_line = json!({
            "type": "shard",
            "shard_index": flushed_prefix,
            "decisions": n,
            "complete_file": complete_json,
        });
        serde_json::to_writer(&mut *journal_writer, &journal_line).map_err(|e| e.to_string())?;
        journal_writer.write_all(b"\n").map_err(|e| e.to_string())?;
        journal_writer.flush().map_err(|e| e.to_string())?;
        *flushed_prefix += 1;
        // 停止条件：在连续分片前缀上三集合全部达标。
        if final_shard.is_none()
            && targets
                .iter()
                .all(|(target, split)| decisions_by_split.get(split).copied().unwrap_or(0) >= *target as u64)
        {
            *final_shard = Some(*flushed_prefix - 1);
            stop.store(true, Ordering::SeqCst);
            eprintln!(
                "[clever-collect] 目标决策数已达成（train {} / validation {} / test {}），停止于分片 {}",
                decisions_by_split.get("train").copied().unwrap_or(0),
                decisions_by_split.get("validation").copied().unwrap_or(0),
                decisions_by_split.get("test").copied().unwrap_or(0),
                *flushed_prefix - 1
            );
        }
    }
    Ok(())
}

/// 单写线程：接收战斗报告，按分片前缀顺序落盘，在连续分片前缀上判断停止条件。
#[allow(clippy::too_many_arguments)]
fn writer_loop(
    receiver: mpsc::Receiver<BattleReport>,
    args: &CollectArgs,
    pools: &[NamePool],
    battles_per_shard: u64,
    max_battles: u64,
    stop: Arc<AtomicBool>,
    resume: Option<(BTreeMap<&'static str, u64>, u64)>, // (已有决策数 per split, 已按序落盘分片数)
) -> Result<(), String> {
    let out = &args.out;
    fs::create_dir_all(out).map_err(|e| format!("创建输出目录失败：{e}"))?;
    let targets = [
        (args.decisions * 80 / 100, "train"),
        (args.decisions * 10 / 100, "validation"),
        (args.decisions * 10 / 100, "test"),
    ];
    let mut totals = Totals::default();
    let mut flushed_totals = Totals::default();
    let mut decisions_by_split: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut flushed_prefix = 0u64;
    if let Some((existing, done_shards)) = resume {
        for (split, count) in existing {
            decisions_by_split.insert(split, count);
        }
        flushed_prefix = done_shards;
        // 已落盘分片的汇总从上次 summary.json 读回（重跑未落盘分片不计入）。
        if let Ok(previous) = fs::read_to_string(out.join("summary.json"))
            && let Ok(value) = serde_json::from_str::<Value>(&previous)
        {
            let take = |key: &str| value[key].as_u64().unwrap_or(0);
            let take_map = |key: &str| -> BTreeMap<String, u64> {
                value[key]
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect())
                    .unwrap_or_default()
            };
            flushed_totals.decisions = take("decisions");
            flushed_totals.rollouts = take("rollouts");
            flushed_totals.unresolved = take("unresolved");
            flushed_totals.failed_battles = take("failed_battles");
            flushed_totals.battles_done = take("battles_done");
            flushed_totals.skip_counts = take_map("skip_counts");
            for (split, count) in take_map("by_split") {
                if let Some(target) = ["train", "validation", "test"].iter().find(|s| **s == split) {
                    *flushed_totals.by_split.entry(target).or_insert(0) += count;
                }
            }
            for (size, count) in take_map("by_team_size") {
                if let Ok(n) = size.parse::<usize>() {
                    flushed_totals.by_team_size.insert(n, count);
                }
            }
        }
        totals.decisions = decisions_by_split.values().sum();
        eprintln!(
            "[clever-collect] resume：已有 {} 决策、{} 分片（汇总读回 {} 决策）",
            totals.decisions, done_shards, flushed_totals.decisions
        );
    }
    let mut shards: BTreeMap<u64, ShardState> = BTreeMap::new();
    let journal_path = out.join("progress.journal");
    let journal_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&journal_path)
        .map_err(|e| format!("打开进度日志失败：{e}"))?;
    let mut journal_writer = BufWriter::new(journal_file);
    let mut final_shard: Option<u64> = None;

    for report in receiver {
        if let Some(last) = final_shard
            && report.battle_index / battles_per_shard > last
        {
            continue; // 停止边界之后的战斗不再计入（确定性丢弃）。
        }
        buffer_report(&report, &mut shards, &mut totals, &mut journal_writer, battles_per_shard)?;
        flush_prefix(
            out,
            &mut shards,
            &mut flushed_prefix,
            &mut decisions_by_split,
            &mut journal_writer,
            battles_per_shard,
            max_battles,
            &stop,
            &mut final_shard,
            &targets,
            &mut flushed_totals,
        )?;
        let _ = write_progress(out, &totals);
    }
    // 全部战斗收尾：按序落盘剩余完整分片（含未达标时报告缺口）。
    flush_prefix(
        out,
        &mut shards,
        &mut flushed_prefix,
        &mut decisions_by_split,
        &mut journal_writer,
        battles_per_shard,
        max_battles,
        &stop,
        &mut final_shard,
        &targets,
        &mut flushed_totals,
    )?;
    let shortfall: Vec<Value> = targets
        .iter()
        .map(|(target, split)| {
            json!({
                "split": split,
                "target": target,
                "collected": decisions_by_split.get(split).copied().unwrap_or(0),
                "met": decisions_by_split.get(split).copied().unwrap_or(0) >= *target as u64,
            })
        })
        .collect();
    let summary = json!({
        "schema_version": SCHEMA_VERSION,
        "decisions": flushed_totals.decisions,
        "rollouts": flushed_totals.rollouts,
        "unresolved": flushed_totals.unresolved,
        "unresolved_rate": if flushed_totals.rollouts == 0 { 0.0 } else { flushed_totals.unresolved as f64 / flushed_totals.rollouts as f64 },
        "by_split": flushed_totals.by_split,
        "by_team_size": flushed_totals.by_team_size,
        "skip_counts": flushed_totals.skip_counts,
        "failed_battles": flushed_totals.failed_battles,
        "battles_done": flushed_totals.battles_done,
        "avg_candidates_mean": if flushed_totals.decisions == 0 { 0.0 } else { flushed_totals.candidates_sum as f64 / flushed_totals.decisions as f64 },
        "skipped_candidates": flushed_totals.skipped_candidates,
        "decisions_with_round_boundary_state": flushed_totals.decisions_with_boundary,
        "avg_formal_rounds_per_battle": if flushed_totals.battles_done == 0 { 0.0 } else { flushed_totals.rounds_sum as f64 / flushed_totals.battles_done as f64 },
        "targets": shortfall,
        "pools": pools.iter().map(|p| json!({"split": p.split, "names": p.names.len()})).collect::<Vec<_>>(),
        "flushed_shards": flushed_prefix,
    });
    fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 summary 失败：{e}"))?;
    write_progress(out, &totals)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// manifest / 审计文件
// ---------------------------------------------------------------------------

fn git_head() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn git_diff_sha() -> Option<String> {
    let output = Command::new("git").args(["diff", "HEAD"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    if output.stdout.is_empty() {
        return Some("clean".to_owned());
    }
    Some(sha256_hex(&output.stdout))
}

fn write_manifest(args: &CollectArgs, rows: usize, csv_sha: &str) -> Result<(), String> {
    let binary_sha = fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
        .map(|data| sha256_hex(&data))
        .unwrap_or_else(|_| "unknown".to_owned());
    let config = json!({
        "schema_version": SCHEMA_VERSION,
        "master_seed": args.master_seed,
        "trials_per_action": args.trials,
        "max_rounds": args.max_rounds,
        "target_decisions": args.decisions,
        "team_sizes": args.team_sizes,
        "subset": args.subset,
        "decisions_per_shard": args.decisions_per_shard,
        "max_decisions_per_battle": MAX_DECISIONS_PER_BATTLE,
    });
    let configuration_sha256 = sha256_hex(serde_json::to_string(&config).unwrap_or_default().as_bytes());
    let manifest = json!({
        "schema_version": SCHEMA_VERSION,
        "source_archive_sha256": args.source_archive_sha256,
        "source_csv_sha256": csv_sha,
        "source_rows": rows,
        "engine_commit": git_head(),
        "dirty_patch_sha256": git_diff_sha(),
        "binary_sha256": binary_sha,
        "master_seed": args.master_seed,
        "split_rule": "sha256(\"clever-names-v1:\"+完整名字) 前 4 字节大端 % 100：<80 train，<90 validation，否则 test；同名去重后整名在同一集合",
        "roster_sampling_rule": "同 split 内无放回均匀抽样，两队 1v1..4v4；随机源 = splitmix64(sha256(master_seed:roster:split:battle_index))，与线程数无关",
        "decision_sampling_rule": "每场正式轨迹的前 4 个真实决策机会全部采集（均匀分层基础采样）",
        "candidate_semantics": "Clever 候选语义：条件满足的主动技能全部进候选（无资格 roll、无顺序先后、无空发）+ 普攻目标全枚举；Boss/专属脚本/强制行动不产候选",
        "checkpoint_semantics": "检查点为回合调度边界（行动选择前）的完整战斗克隆；round_boundary_state 为效果队列排空时导出的 BattleModelState（否则沿用最近成功边界）",
        "rng_injection_boundary": RNG_BOUNDARY_DESC,
        "continuation_policy": "legacy",
        "trials_per_action": args.trials,
        "max_rounds": args.max_rounds,
        "feature_schema_id": "none-v1",
        "configuration_sha256": configuration_sha256,
        "planned_jobs": {"target_decisions": args.decisions, "targets_by_split": {"train": args.decisions * 80 / 100, "validation": args.decisions * 10 / 100, "test": args.decisions * 10 / 100}},
        "toolchain_versions": {"rustc": rustc_version()},
        "created_at": unix_now(),
    });
    let path = args.out.join("manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写 manifest 失败：{e}"))?;
    Ok(())
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn unix_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn write_feature_schema(out: &Path) -> Result<(), String> {
    let schema = json!({
        "schema_version": SCHEMA_VERSION,
        "feature_schema_id": "none-v1",
        "features_exported": false,
        "note": "首批不导出预编码特征。GPU 侧从 states 分片的 round_boundary_state（完整 BattleModelState）与 decision_context（行动者运行时快照、候选表、已结算前置条件）重新编码；不导出旧 nn_features（884 维末维为启发式摘要，且每侧固定六实体会截断召唤物）。",
    });
    fs::write(
        out.join("feature_schema.json"),
        serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 feature_schema 失败：{e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-legacy：纯原版复跑比对（验收：开启/关闭采样时正式轨迹一致）
// ---------------------------------------------------------------------------

fn verify_legacy_replay(out: &Path, count: usize, max_rounds: u64, decisions_per_shard: usize) -> Result<(), String> {
    let journal = out.join("progress.journal");
    let file = File::open(&journal).map_err(|e| format!("打开进度日志失败：{e}"))?;
    let reader = BufReader::new(file);
    let battles_per_shard = (decisions_per_shard.max(100).min(500) / MAX_DECISIONS_PER_BATTLE) as u64;
    let mut checked = 0usize;
    let mut mismatches = Vec::new();
    let mut samples = Vec::new();
    // 先读全部日志：战斗行在分片行之前，需先知道已落盘分片数再过滤。
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    let mut values = Vec::new();
    let mut flushed_shards = 0u64;
    for line in lines {
        let value: Value = serde_json::from_str(&line).map_err(|e| format!("进度日志解析失败：{e}"))?;
        if value["type"] == "shard" {
            flushed_shards += 1;
            continue;
        }
        if value["error"].is_string() {
            continue;
        }
        values.push(value);
    }
    for value in values {
        // 只复跑已落盘分片内的正式轨迹（停止边界后丢弃的战斗不计入）。
        if let Some(index) = value["battle_index"].as_u64()
            && index >= flushed_shards.saturating_mul(battles_per_shard)
        {
            continue;
        }
        if checked >= count {
            break;
        }
        let Some(full_names) = value["full_names"].as_array() else {
            continue;
        };
        let Some(seed_line) = value["battle_seed"].as_str() else {
            continue;
        };
        let groups: Vec<Vec<String>> = full_names
            .iter()
            .filter_map(|team| {
                team.as_array()
                    .map(|names| names.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect())
            })
            .collect();
        if groups.len() != 2 {
            continue;
        }
        match collect::replay_legacy_battle(&groups, seed_line, max_rounds) {
            Ok((winner, rounds)) => {
                let audit_winner = value["winner_team"].as_u64().map(|w| w as usize);
                let audit_rounds = value["rounds"].as_u64().unwrap_or(0);
                checked += 1;
                if winner != audit_winner || rounds != audit_rounds {
                    mismatches.push(json!({
                        "battle_index": value["battle_index"],
                        "audit_winner": audit_winner,
                        "replay_winner": winner,
                        "audit_rounds": audit_rounds,
                        "replay_rounds": rounds,
                    }));
                }
                if samples.len() < 3 {
                    samples.push(json!({
                        "battle_index": value["battle_index"],
                        "winner": winner,
                        "rounds": rounds,
                        "match": winner == audit_winner && rounds == audit_rounds,
                    }));
                }
            }
            Err(error) => mismatches.push(json!({"battle_index": value["battle_index"], "error": error})),
        }
    }
    let report = json!({
        "schema_version": SCHEMA_VERSION,
        "checked": checked,
        "mismatches": mismatches.len(),
        "samples": samples,
        "detail": mismatches,
        "note": "纯原版复跑（无采样探测）与采集审计行（winner_team/rounds）逐场比对；一致即证明采样不改动正式轨迹。",
    });
    fs::write(
        out.join("verify_legacy.json"),
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 verify_legacy.json 失败：{e}"))?;
    eprintln!(
        "[clever-collect] verify-legacy：复跑 {} 场，不一致 {} 场 -> {}",
        checked,
        mismatches.len(),
        out.join("verify_legacy.json").display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// audit-names：引擎重生成名字属性/技能，供与 CSV 核对
// ---------------------------------------------------------------------------

fn audit_names(names_csv: &Path, out: &Path, count: usize) -> Result<(), String> {
    let file = File::open(names_csv).map_err(|e| format!("打开 CSV 失败：{e}"))?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header).map_err(|e| e.to_string())?;
    // 确定性抽样：对行号做 splitmix64，保留 hash 最小的 count 行（等概率均匀）。
    let mut selected: Vec<(u64, String)> = Vec::with_capacity(count);
    let mut line = String::new();
    let mut row = 0u64;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let hash = splitmix64(row);
        let full_name = csv_first_field(&line);
        if selected.len() < count {
            selected.push((hash, full_name));
            selected.sort_unstable();
        } else if let Some(last) = selected.last()
            && hash < last.0
        {
            selected.pop();
            selected.push((hash, full_name));
            selected.sort_unstable();
        }
        row += 1;
    }
    let mut records = Vec::new();
    let mut failures = Vec::new();
    for (_hash, full_name) in selected {
        match collect::audit_name_record(&full_name) {
            Ok(record) => records.push(record),
            Err(error) => failures.push(json!({"full_name": full_name, "error": error})),
        }
    }
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    fs::write(
        out.join("audit_names.json"),
        serde_json::to_string_pretty(&json!({"schema_version": SCHEMA_VERSION, "records": records, "failures": failures}))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 audit_names.json 失败：{e}"))?;
    eprintln!(
        "[clever-collect] audit-names 完成：{} 个名字 -> {}",
        records.len(),
        out.join("audit_names.json").display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 进度日志与 resume
// ---------------------------------------------------------------------------

/// 从 progress.journal 恢复：已按序落盘分片数、累计决策数（per split，仅已落盘分片内）。
/// 返回 (by_split, 已落盘分片数, 已覆盖战斗前缀数)。
fn load_resume_state(out: &Path, battles_per_shard: u64) -> Result<(BTreeMap<&'static str, u64>, u64, u64), String> {
    let path = out.join("progress.journal");
    if !path.exists() {
        return Ok((BTreeMap::new(), 0, 0));
    }
    let file = File::open(&path).map_err(|e| format!("打开进度日志失败：{e}"))?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    let mut flushed_shards = 0u64;
    let mut max_battle = 0u64;
    let mut values = Vec::new();
    for line in lines {
        let value: Value = serde_json::from_str(&line).map_err(|e| format!("进度日志解析失败：{e}"))?;
        if value["type"] == "shard" {
            flushed_shards += 1;
            continue;
        }
        if let Some(index) = value["battle_index"].as_u64() {
            max_battle = max_battle.max(index);
            values.push(value);
        }
    }
    // 已覆盖前缀 = 连续已落盘分片（0..flushed_shards 均按序写盘才安全）；
    // 分片落盘是原子的，未落盘分片的战斗会整体重跑。
    let covered = flushed_shards.saturating_mul(battles_per_shard).min(max_battle + 1);
    // 只统计已落盘分片内的战斗，未落盘分片的战斗行在重跑后重新计入。
    let mut by_split: BTreeMap<&'static str, u64> = BTreeMap::new();
    for value in values {
        let Some(index) = value["battle_index"].as_u64() else {
            continue;
        };
        if index >= covered {
            continue;
        }
        let split: &'static str = match value["split"].as_str() {
            Some("train") => "train",
            Some("validation") => "validation",
            Some("test") => "test",
            _ => continue,
        };
        if let Some(decisions) = value["decisions_taken"].as_u64() {
            *by_split.entry(split).or_insert(0) += decisions;
        }
    }
    Ok((by_split, flushed_shards, covered))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    if let Some(count) = cli.collect.verify_legacy {
        if let Err(error) = verify_legacy_replay(&cli.collect.out, count, cli.collect.max_rounds, cli.collect.decisions_per_shard)
        {
            eprintln!("[clever-collect] verify-legacy 失败：{error}");
            std::process::exit(1);
        }
        return;
    }
    if cli.collect.audit_names {
        if let Err(error) = audit_names(&cli.collect.names_csv, &cli.collect.out, cli.collect.audit_count) {
            eprintln!("[clever-collect] audit-names 失败：{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run_collect(&cli.collect) {
        eprintln!("[clever-collect] 采集失败：{error}");
        std::process::exit(1);
    }
}

fn run_collect(args: &CollectArgs) -> Result<(), String> {
    if args.trials == 0 || args.workers == 0 || args.decisions == 0 {
        return Err("workers/trials/decisions 必须为正".into());
    }
    let sizes: Vec<usize> = args
        .team_sizes
        .split(',')
        .map(|s| s.trim().parse::<usize>().map_err(|_| format!("无效队伍规模：{s}")))
        .collect::<Result<_, _>>()?;
    if sizes.iter().any(|&s| !(1..=4).contains(&s)) {
        return Err("队伍规模应在 1..=4".into());
    }
    // 战斗上限：每个目标决策预留一场战斗（每场至多 4 决策，通常有富余）。
    let max_battles = (args.decisions * 2) as u64;
    let battles_per_shard = ((args.decisions_per_shard.max(100).min(500)) / MAX_DECISIONS_PER_BATTLE) as u64;
    let out = &args.out;
    let resume_state = if args.resume {
        let state = load_resume_state(out, battles_per_shard)?;
        eprintln!("[clever-collect] resume：已覆盖 {} 场战斗、{} 个分片", state.2, state.1);
        Some(state)
    } else {
        None
    };
    if out.exists() {
        let manifest = out.join("manifest.json");
        if manifest.exists() && !args.resume {
            return Err(format!("输出目录已有 manifest（{}），续跑需 --resume", manifest.display()));
        }
    }
    let started = std::time::Instant::now();
    eprintln!("[clever-collect] 读取名字池（subset={}）...", args.subset);
    let (pools, rows) = load_pools(&args.names_csv, &args.subset)?;
    for pool in &pools {
        eprintln!("[clever-collect]   {}: {} 名字", pool.split, pool.names.len());
    }
    let csv_sha = {
        let data = fs::read(&args.names_csv).map_err(|e| format!("计算 CSV 摘要失败：{e}"))?;
        sha256_hex(&data)
    };
    fs::create_dir_all(out).map_err(|e| format!("创建输出目录失败：{e}"))?;
    fs::write(
        out.join("skill_mapping.json"),
        serde_json::to_string_pretty(&collect::skill_mapping_table()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写 skill_mapping 失败：{e}"))?;
    write_feature_schema(out)?;

    // resume：跳过已落盘分片覆盖的前缀（分片落盘原子，未落盘分片整体重跑）。
    let covered_battles = resume_state.as_ref().map(|(_, _, covered)| *covered).unwrap_or(0);
    let resume_for_writer = resume_state.map(|(by_split, shards, _)| (by_split, shards));
    let pools = Arc::new(pools);
    let stop = Arc::new(AtomicBool::new(false));
    let next = Arc::new(AtomicUsize::new(covered_battles as usize));
    let (sender, receiver) = mpsc::channel();
    let mut handles = Vec::new();
    for _ in 0..args.workers.max(1) {
        let (args, pools, sizes, next, stop, sender) = (
            args.clone(),
            pools.clone(),
            sizes.clone(),
            next.clone(),
            stop.clone(),
            sender.clone(),
        );
        handles.push(std::thread::spawn(move || {
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed) as u64;
                if index >= max_battles {
                    break;
                }
                let plan = match plan_battle(index, &args.master_seed, &pools, &sizes) {
                    Ok(plan) => plan,
                    Err(error) => {
                        let split = match index % 10 {
                            0..=7 => "train",
                            8 => "validation",
                            _ => "test",
                        };
                        let size = sizes[(index as usize) % sizes.len()];
                        let _ = sender.send(error_report(index, split, size, error));
                        continue;
                    }
                };
                let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_battle(&args, &plan)));
                let report = match report {
                    Ok(report) => report,
                    Err(_) => error_report(index, plan.split, plan.team_size, "panic".into()),
                };
                if sender.send(report).is_err() {
                    break;
                }
            }
        }));
    }
    drop(sender);
    writer_loop(
        receiver,
        args,
        &pools,
        battles_per_shard,
        max_battles,
        stop.clone(),
        resume_for_writer,
    )?;
    for handle in handles {
        let _ = handle.join();
    }
    write_manifest(args, rows, &csv_sha)?;
    let elapsed = started.elapsed().as_secs_f64();
    eprintln!("[clever-collect] 完成：输出 {}（{elapsed:.1}s）", out.display());
    Ok(())
}
