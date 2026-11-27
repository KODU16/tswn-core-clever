"""固定相同留出集，比较混合模型与按 1v1/多人分流的专家模型。"""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys

import numpy as np


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf8')


def bootstrap(values, groups, seed=20261127):
    # 对整场战斗重采样，避免将同一场的多步决策视为独立观测。
    unique, inverse = np.unique(groups, return_inverse=True)
    sums = np.bincount(inverse, weights=values)
    counts = np.bincount(inverse)
    rng = np.random.default_rng(seed)
    means = []
    for _ in range(3000):
        sampled = rng.integers(len(unique), size=len(unique))
        means.append(sums[sampled].sum() / counts[sampled].sum())
    return np.quantile(means, [.025, .975]).tolist()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data', type=pathlib.Path, required=True)
    parser.add_argument('--prepared', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--seeds', type=int, nargs='+', default=[20261127, 20261128, 20261129])
    parser.add_argument('--epochs', type=int, default=40)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    source = pathlib.Path(__file__).with_name('gpu_trial.py')
    prepared_hash = hashlib.sha256(args.prepared.read_bytes()).hexdigest()
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    # 固定对照配置后一次跑完；不根据测试集表现更换超参数或挑选随机种子。
    design = {'seeds': args.seeds, 'max_epochs': args.epochs, 'prepared_sha256': prepared_hash,
              'trainer_sha256': source_hash,
              'strategies': {'mixed': [1, 2, 3, 4], 'single': [1], 'multi': [2, 3, 4]},
              'selection': 'validation selected advantage; patience 10; all seeds reported'}
    save(args.output / 'experiment_design.json', design)
    shutil.copy2(source, args.output / 'gpu_trial.py')
    for seed in args.seeds:
        for label, sizes in design['strategies'].items():
            dest = args.output / f'{label}-{seed}'
            dest.mkdir(parents=True, exist_ok=True)
            print(json.dumps({'stage': 'training', 'strategy': label, 'seed': seed}), flush=True)
            command = [sys.executable, str(source.resolve()), '--data', str(args.data.resolve()),
                       '--prepared', str(args.prepared.resolve()), '--output', str(dest.resolve()),
                       '--seed', str(seed), '--epochs', str(args.epochs), '--team-sizes', *map(str, sizes)]
            with (dest / 'training.log').open('w', encoding='utf8') as log:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    with np.load(args.prepared, allow_pickle=False) as data:
        arrays = {k: data[k] for k in data.files}
    result = {'design': design, 'splits_by_team_size': {}, 'validation': {}, 'test': {}}
    for split in ['train', 'validation', 'test']:
        result['splits_by_team_size'][split] = {str(size): int(((arrays['split'] == split) &
            (arrays['team_size'] == size)).sum()) for size in [1, 2, 3, 4]}
    for split in ['validation', 'test']:
        ids = np.flatnonzero(arrays['split'] == split)
        mixed_rows, expert_rows = [], []
        for seed in args.seeds:
            mixed = np.load(args.output / f'mixed-{seed}' / f'predictions-{split}.npz')
            assert np.array_equal(mixed['ids'], ids)
            routed = np.full(len(arrays['split']), np.nan)
            for label in ['single', 'multi']:
                expert = np.load(args.output / f'{label}-{seed}' / f'predictions-{split}.npz')
                routed[expert['ids']] = expert['gains']
            assert np.isfinite(routed[ids]).all()
            mixed_rows.append(mixed['gains'])
            expert_rows.append(routed[ids])
        mixed_rows, expert_rows = np.stack(mixed_rows), np.stack(expert_rows)
        for group, sizes in {'all': [1, 2, 3, 4], 'single': [1], 'multi': [2, 3, 4],
                             '1v1': [1], '2v2': [2], '3v3': [3], '4v4': [4]}.items():
            use = np.isin(arrays['team_size'][ids], sizes)
            if not use.any():
                continue
            mixed = mixed_rows[:, use].mean(0)
            expert = expert_rows[:, use].mean(0)
            groups = arrays['battle'][ids[use]]
            uniform = (arrays['gain'][ids[use]] * arrays['mask'][ids[use]]).sum(1) / arrays['mask'][ids[use]].sum(1)
            result[split][group] = {'decisions': int(use.sum()), 'battles': len(set(groups)),
                'mixed_gain': float(mixed.mean()), 'expert_gain': float(expert.mean()),
                'uniform_gain': float(uniform.mean()),
                'mixed_gain_ci95': bootstrap(mixed, groups), 'expert_gain_ci95': bootstrap(expert, groups),
                'expert_minus_mixed': float((expert - mixed).mean()),
                'expert_minus_mixed_ci95': bootstrap(expert - mixed, groups),
                'mixed_gain_by_seed': mixed_rows[:, use].mean(1).tolist(),
                'expert_gain_by_seed': expert_rows[:, use].mean(1).tolist()}
    save(args.output / 'comparison.json', result)
    print(json.dumps(result), flush=True)


if __name__ == '__main__':
    main()
