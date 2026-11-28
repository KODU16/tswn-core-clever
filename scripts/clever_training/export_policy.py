"""导出混合策略的固定布局参数，供 Rust/WASM 使用同一检查点。"""
import argparse
import hashlib
import json
import shutil
from pathlib import Path

import numpy as np
import torch

# 顺序必须与 clever_policy.rs 的推理布局一致；基线 V 头不参与动作选择。
SHAPES = {
    "xmean": (1318,), "xstd": (1318,), "amean": (327,), "astd": (327,),
    "state.0.weight": (256, 1318), "state.0.bias": (256,),
    "state.2.weight": (256,), "state.2.bias": (256,),
    "state.4.weight": (128, 256), "state.4.bias": (128,),
    "action.0.weight": (128, 327), "action.0.bias": (128,),
    "action.2.weight": (128, 128), "action.2.bias": (128,),
    "q.0.weight": (128, 384), "q.0.bias": (128,),
    "q.3.weight": (1, 128), "q.3.bias": (1,),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model-id", required=True)
    args = parser.parse_args()
    # 仅读取自己训练或仓库随附的检查点，全部转至 CPU 导出。
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    if (checkpoint["feature_version"], checkpoint["state_width"], checkpoint["action_width"]) != (
        "mechanism-summary-trial-v1", 1318, 327
    ) and (checkpoint["feature_version"], checkpoint["state_width"], checkpoint["action_width"]) != (
        "mechanism-summary-reserve-v3", 1469, 329
    ):
        raise ValueError("检查点特征版本或维度与 Rust 推理不一致")
    # v3 增加库存与倍率列，网络隐层保持一致，布局按检查点维度导出。
    state_width, action_width = checkpoint["state_width"], checkpoint["action_width"]
    shapes = dict(SHAPES)
    shapes.update(xmean=(state_width,), xstd=(state_width,), amean=(action_width,), astd=(action_width,))
    shapes["state.0.weight"] = (256, state_width)
    shapes["action.0.weight"] = (128, action_width)
    chunks, layout, offset = [], [], 0
    for name, shape in shapes.items():
        value = checkpoint[name] if name in checkpoint else checkpoint["state_dict"][name].numpy()
        array = np.asarray(value, dtype="<f4")
        if array.shape != shape or not np.isfinite(array).all():
            raise ValueError(f"{name}: 非有限值或形状不符 {array.shape}")
        if name in ("xstd", "astd") and (array <= 0).any():
            raise ValueError(f"{name}: 标准差必须为正")
        chunks.append(array.ravel())
        layout.append({"name": name, "shape": list(shape), "offset_floats": offset, "length": array.size})
        offset += array.size
    blob = np.concatenate(chunks).astype("<f4").tobytes()
    manifest = {
        "id": args.model_id, "feature_version": checkpoint["feature_version"],
        "state_width": state_width, "action_width": action_width,
        "training_seed": checkpoint["seed"], "epoch": checkpoint["epoch"],
        "dtype": "float32 little-endian row-major",
        "weights_sha256": hashlib.sha256(blob).hexdigest(),
        "training_checkpoint_sha256": hashlib.sha256(args.checkpoint.read_bytes()).hexdigest(),
        "inference_float_count": offset, "layout": layout,
        "selection": "best validation gain checkpoint; validation patience early stop",
        "input_exclusions": ["name", "suffix", "seed", "PBB score", "heuristic score"],
    }
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "weights.bin").write_bytes(blob)
    target = args.output / "training-best.pt"
    if args.checkpoint.resolve() != target.resolve():
        shutil.copy2(args.checkpoint, target)
    (args.output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({"id": args.model_id, "weights_sha256": manifest["weights_sha256"]}))


if __name__ == "__main__":
    main()
