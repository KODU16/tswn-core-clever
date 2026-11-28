# Clever 混合模型 v3

当前线上模型为 clever-mixed-v3-20261128-e8，使用完整 reserve-v6 续战标签，从头混合训练0.1/1/50/100倍。模型参数不含名字、后缀、seed、随机初相位或PBB/启发式分数。

1469维情景和329维动作在旧机制摘要上加入发放后的同名技能可用次数、未发额度和对数倍率；50/100倍倍率表示相同。标准化仅拟合训练分区。518018个训练参数，推理仅使用情景塔、动作塔与评分头；SiLU，LayerNorm epsilon=1e-5，关闭Dropout，输入标准化后裁剪至[-10,10]。

28轮训练约43秒，验证收益连续20轮未改善而提前停止，未达到明确过拟合阈值。仅验证集选择第8轮；独立无梯度复核一致。训练数据50670个决策、128884个候选，排除761条重复训练样本。验证所选收益+4.7305个百分点，测试+4.5923个百分点；相对均匀候选提高3.0065个百分点，相对技能先验提高1.1684个百分点。基线是归档首个物理普攻后按采集器reserve-v6续战，指标不表示整场模型实战胜率提升。

重要覆盖限制：实际数据只有1v1/2v2；验证只有1v1、测试只有2v2，无3v3/4v4证据。每场只采集前四次决策，长期储存时机未充分覆盖，engine_commit为unknown。完整结果和审计见training-report.json、training-history.json、feature_definition.json。

- training-best.pt：最佳混合检查点、标准化与训练元数据。
- weights.bin：Rust/WASM共用小端float32推理参数。
- manifest.json：版本、形状和SHA-256。

导出与特征核验：

~~~powershell
python scripts/clever_training/export_policy.py --checkpoint crates/tswn_core/models/clever-mixed-v3/training-best.pt --output crates/tswn_core/models/clever-mixed-v3 --model-id clever-mixed-v3-20261128-e8
python scripts/clever_training/export_v3_fixtures.py --checkpoint crates/tswn_core/models/clever-mixed-v3/training-best.pt --output crates/tswn_core/src/runtime/clever_policy/real-decisions.json
~~~

八个真实发放点样本覆盖四倍率和两个人数，验证逐列特征、PyTorch/Rust logits及最佳动作。线上通过clever_model_info查询实际权重版本。专用clever@!始终使用此模型与100倍，其他Boss保留其原机制。
