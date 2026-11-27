# Clever 混合模型 v1

模型 `clever-mixed-v1-20261127-e15` 来自混合人数训练的 seed 20261129、第 15 轮。根据验证集动作收益选取；续训出现过拟合后停止，未替换这份最佳权重。完整训练结果见 [训练报告](../../../../docs/training/clever-gpu-trial-2026-11-27.md)。

- `training-best.pt`：原始混合检查点，含标准化参数与训练元数据。
- `weights.bin`：Rust/WASM 实际内嵌的推理参数，小端 float32、行优先。
- `manifest.json`：模型标识、参数形状、以 float 为单位的偏移和两个文件的 SHA-256。

网络使用 1318 维情景和 327 维候选动作特征；情景塔、动作塔经拼接及逐元素乘积输入动作评分头。SiLU 激活，LayerNorm epsilon 为 1e-5，标准化后裁剪至 [-10,10]，推理关闭 Dropout。基线 V 头不参与决策。布局及实现见 `src/runtime/clever_policy.rs`。

Clever 对合法候选选最高分，再由正式引擎结算，不叠加旧熟练度分数。模型未覆盖的专属动作／技能回退旧 Clever 搜索，Boss 保持原机制。网页仍默认原版，主动选择“Clever 学习模型（混合 v1）”启用。

使用 Python/PyTorch 环境复现导出：

```powershell
python scripts/clever_training/export_policy.py --checkpoint crates/tswn_core/models/clever-mixed-v1/training-best.pt --output crates/tswn_core/models/clever-mixed-v1 --model-id clever-mixed-v1-20261127-e15
```

三个真实决策的 Rust 特征和分数与 Python/PyTorch 对齐；浏览器 WASM 实测最大分数误差约 4.77e-7。离线 +9.33 个百分点仅表示当前一步选择、随后原版续战的收益，完整学习策略的实战强度由网页对局进一步评估。
