# Clever GPU 试训

`gpu_trial.py` 读取战斗模拟导出的原始状态和配对动作结果，训练一个用于动作排序的 CUDA 网络。它是独立试训工具，尚未接入线上 Clever，也不是生产 `encoder-v1` 的替代实现。

## 数据处理

- 读取 `clever-battle-data-v1-batch001.tar.gz` 和 `clever-battle-data-v1-supplement.tar.gz`；补充归档中的所有一级数据包分别处理。
- 仅纳入有 `complete-*.json` 的分片，先验证 states/decisions 压缩文件的 SHA-256，再核对状态、候选动作、胜负数量与配对收益。
- 原有 train/validation/test 划分保持不变，并对参与名字的哈希分区进行核验。名字和 seed 仅用于审计，不是输入特征。
- pilot 每个动作仅 64 次续战，本次排除；正式与补充数据均为 256 次。
- 不读取 PBB 模型分数，不使用旧 `nn_features` 中的启发式分数。技能熟练度和真实战斗属性属于正常机制输入。
- 数值缓存 `prepared.npz` 只可复用已校验、与当前特征版本一致的结果。修改特征实现后应使用新输出目录重新准备，不可沿用旧缓存。

输入是行动者、存活/死亡友方和敌方的真实属性、技能及状态摘要，加上候选动作和目标摘要。池化包含全部实体，不按六实体截断。1318 维情景向量与 327 维动作向量的列顺序写入 `feature_definition.json`。注册表编号只用于解析技能类别，不作为连续数值输入。

特征有意做了简化：其他实体仍是干净回合边界状态，只有行动者使用决策时运行态快照；部分状态仅保留计数、持续时间和幅度摘要；池化会丢失部分实体关系和顺序。归档 manifest 的 `engine_commit` 为 `unknown`，不能宣称数据与当前引擎完全一致。后续正式接入应在 Rust 侧重放决策边界并使用唯一、完整且版本化的编码器。

## 环境与运行

本机持久环境：`C:/codex_envs/clever-gpu`，Python 3.11.5、NumPy 2.4.6、PyTorch 2.14.1+cu130，RTX 4060 Laptop GPU，驱动 595.97。脚本要求 CUDA 可用，不会静默改用 CPU。

```powershell
C:/codex_envs/clever-gpu/Scripts/python.exe scripts/clever_training/gpu_trial.py --data data --output crates/nn_handoff/clever-trial

C:/codex_envs/clever-gpu/Scripts/python.exe scripts/clever_training/compare_training.py --data data --prepared crates/nn_handoff/clever-trial/prepared.npz --output crates/nn_handoff/clever-comparison
```

比较脚本固定运行种子 20261127、20261128、20261129，每个种子训练混合模型、1v1 专家和多人专家。`--team-sizes 1` 或 `--team-sizes 2 3 4` 可单独训练对应专家。相同测试决策上按原始队伍人数路由专家，召唤物数量不改变分流类别。

标准化参数仅拟合所选训练分区。网络包含情景塔、动作塔、动作胜率头和原版基线头，共 479106 个参数。AdamW 学习率 0.0003、weight decay 0.01、batch 128、最多 40 轮、验证收益连续 10 轮不改善早停，开启 CUDA AMP。目标组合为动作胜率 BCE、基线 BCE、配对收益 MSE 和列表排序监督；全部正、负、零收益候选都参与，较高收益在软目标中获得较高权重。

每份模型用验证集平均已选动作收益选择检查点，测试集不参与早停、调参或种子选择。每个模型按相同最大训练轮数比较，并非相同梯度更新次数或相同运行时间预算。

## 结果解释

`gain` 是选择这一步之后、双方改用原版续战时，相对记录的原版基线的胜率差。它不是完整学习策略的实战胜率，也不是相对当前公式版 Clever 的提升。训练标签中的经验最优候选仅是受采样噪声影响、偏乐观的参照上限。

模型提供独立动作胜率误差、均匀候选策略、仅按训练集技能收益先验选择的基线和按人数分层的指标。混合/专家比较报告全部预定种子的平均表现与种子间结果，不挑最好种子。置信区间按整场战斗重采样，反映当前留出场景的不确定性，未覆盖共享名字、续战随机流关联或编码器缺失等全部误差来源；平均种子的结果也不等同于 logits 集成策略。

## 按时间继续训练混合模型

先依据验证集选择已有混合检查点，再按时间预算继续。旧版检查点不含优化器，本次沿用权重和标准化参数，新建较低学习率的 AdamW，按经过的时间余弦下降至初始学习率的 3%。

```powershell
C:/codex_envs/clever-gpu/Scripts/python.exe scripts/clever_training/gpu_trial.py --data data --prepared crates/nn_handoff/clever-gpu-trial-20261127/prepared.npz --output crates/nn_handoff/clever-gpu-trial-20261127/mixed-10min --resume crates/nn_handoff/clever-gpu-trial-20261127/mixed-20261129/best.pt --seed 20261129 --minutes 10 --lr 0.0001
```

时间是最长预算，出现明显过拟合时立即提前停止。具体检测要求至少记录十轮，最近五轮平均训练损失比开头五轮低 0.002、验证胜率 MSE 比历史最低值高 5% 以上，且平均动作收益比最佳值低 0.1 个百分点，三项同时满足即停止。始终保存验证集最佳检查点，包括继续训练前的权重。达到时间预算时在当前完整轮次结束后停止；准备数据与最终评估不计入训练预算。每轮记录训练损失、验证动作收益、胜率回归误差和学习率，最终只评估选定的最佳检查点。

`best.pt` 包含权重和标准化参数；`last.pt` 保存最后一轮权重及当时的优化器和 AMP 状态，供检查过拟合或后续扩展精确恢复。当前 `--resume` 始终执行上述优化器热重启，不会自动恢复优化器动量。本次长训在收到立即停止要求后已终止，未用满 600 秒；已落盘的最佳权重经独立只读评估重新核验。其确切脚本快照与运行设计保存在本地输出目录。通过 `--evaluate-only --resume <best.pt>` 可重算既有检查点的留出指标而不做梯度更新。

同时输出配置、列定义、学习曲线数据、预测和审计。模型与训练张量保存在已忽略的 `crates/nn_handoff/`，不加入 Git。按当前要求仅保留混合模型权重，专家模型权重已移除，历史对照指标继续留档。
