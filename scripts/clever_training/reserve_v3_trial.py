"""v3 完整发放规则数据：仅训练跨倍率、跨人数的单一混合模型。"""
import argparse
import hashlib
import json
import pathlib
import shutil
import sys
import numpy as np
import reserve_v2_trial as base

ROOT = base.ROOT
FOLDERS = ['clever-battle-data-v3', 'clever-battle-data-v3-mult01',
           'clever-battle-data-v3-mult50', 'clever-battle-data-v3-mult100']


def prepare(raw, output):
    packs, audits, names = [], [], {}
    encode = base.encode_v2

    def checked_encode(state, decision):
        # 决策点库存必须已发放；不接受旧规则标签或没有可用库存的技能候选。
        if state['round_boundary_state'].get('clever_reserve_version') != 6:
            raise ValueError('requires reserve version 6')
        actor = base.entity_id(state['decision_context']['actor_entity_id'])
        stock = {}
        for row in base.reserves(state):
            owner = base.entity_id(row.get('actor', actor))
            issued, consumed = row['issued'], row['consumed']
            if consumed < 0 or issued < consumed:
                raise ValueError('invalid issued/consumed')
            stock[owner, row['skill']] = issued - consumed
        for candidate in state['decision_context']['candidates']:
            action = candidate['canonical_action']
            if action['action_kind'] == 'builtin_skill' and stock.get((actor, action['skill_export_name']), 0) < 1:
                raise ValueError('illegal skill candidate without stock')
        return encode(state, decision)

    base.encode_v2 = checked_encode
    for folder in FOLDERS:
        base.RAW = raw / folder
        manifest = base.read(base.RAW / 'manifest.json')
        if manifest.get('continuation_policy') != 'reserve-v6':
            raise ValueError('v3 requires reserve-v6 continuation')
        # 名字只校验跨包隔离，不作为模型输入。
        for line in (base.RAW / 'progress.journal').read_text(encoding='utf8').splitlines():
            item = json.loads(line)
            if 'full_names' not in item:
                continue
            for name in sum(item['full_names'], []):
                if names.setdefault(name, item['split']) != item['split']:
                    raise ValueError('cross-pack name leakage')
        base.OUT = output / 'packs' / folder
        base.OUT.mkdir(parents=True, exist_ok=True)
        arrays = base.prepare()
        audit = base.read(base.OUT / 'audit.json')
        audit['limitations'] = ['reserve-v6 continuation uses collector policy, not learned full-policy win rate',
                                'engine_commit unknown', 'only first four decision opportunities sampled',
                                'no guarantee on unsampled multipliers or global optimality']
        base.old.save_json(base.OUT / 'audit.json', audit)
        packs.append(arrays)
        audits.append(audit)
    maximum = max(p['mask'].shape[1] for p in packs)
    for pack in packs:
        pad = maximum - pack['mask'].shape[1]
        for key in ['a', 'mask', 'gain', 'q', 'se', 'kinds']:
            pack[key] = np.pad(pack[key], [(0, 0), (0, pad)] + ([(0, 0)] if key == 'a' else []))
    arrays = {key: np.concatenate([p[key] for p in packs]) for key in packs[0]}
    unique, seen = np.ones(len(arrays['x']), bool), set()
    # 饱和区完全相同的训练样本去重，保留原倍率供分层验证。
    for i in np.flatnonzero(arrays['split'] == 'train'):
        mask = arrays['mask'][i]
        fingerprint = hashlib.sha256(arrays['x'][i].tobytes() + arrays['a'][i, mask].tobytes()
                                     + arrays['q'][i, mask].tobytes() + arrays['gain'][i, mask].tobytes()).digest()
        if fingerprint in seen:
            unique[i] = False
        seen.add(fingerprint)
    arrays['training_unique'] = unique
    np.savez_compressed(output / 'prepared.npz', **arrays)
    base.old.save_json(output / 'audit.json', {
        'packs': audits, 'decisions': len(unique), 'actions': int(arrays['mask'].sum()),
        'duplicate_training_rows_excluded': int((~unique).sum()), 'cross_split_name_overlap': 0,
        'continuation_policy': 'reserve-v6',
        'team_size_by_split': {split: {str(int(size)): int(((arrays['split'] == split) & (arrays['team_size'] == size)).sum())
                                      for size in np.unique(arrays['team_size'])}
                               for split in ['train', 'validation', 'test']},
        'distribution': {str(m): {split: int(((arrays['multiplier'] == m) & (arrays['split'] == split)).sum())
                                for split in ['train', 'validation', 'test']}
                         for m in np.unique(arrays['multiplier'])}})
    return arrays


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw-parent', type=pathlib.Path, default=pathlib.Path('C:/codex_tmp/clever-v3-raw'))
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'crates/nn_handoff/clever-reserve-v3-mixed-20261128')
    parser.add_argument('--minutes', type=float, default=10)
    parser.add_argument('--evaluate-only', action='store_true')
    args = parser.parse_args()
    if not 0 < args.minutes <= 10:
        parser.error('minutes must be in (0,10]')
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    for folder in ['mixed', 'verified']:
        (output / folder).mkdir(exist_ok=True)
    # 新版本使用独立缓存和特征版本，禁止加载 v1/v2 权重继续训练。
    arrays = dict(np.load(output / 'prepared.npz', allow_pickle=False)) if (output / 'prepared.npz').exists() else prepare(args.raw_parent, output)
    definition = base.old.feature_definition()
    definition.update(version='mechanism-summary-reserve-v3', validation_patience=20,
                      overfit_window=3, overfit_gain_tolerance=0.0002)
    definition['context_columns'] += ['log_skill_multiplier'] + [
        f'{group}.{skill}.{field}' for group in ['actor_reserve', 'ally_reserve', 'enemy_reserve']
        for skill in base.SKILLS for field in ['log1p_available', 'log1p_credit']]
    definition['action_columns'] += ['reserve.log1p_available', 'reserve.log1p_credit']
    for script in ['reserve_v3_trial.py', 'reserve_v2_trial.py', 'gpu_trial.py']:
        shutil.copy2(ROOT / 'scripts/clever_training' / script, output / script)
    print(json.dumps({'stage': 'training_start', 'decisions': len(arrays['x']),
                      'multipliers': np.unique(arrays['multiplier']).tolist(), 'minutes': args.minutes}), flush=True)
    if not args.evaluate_only:
        base.old.train(arrays, output / 'mixed', 100000, 20261128, [1, 2, 3, 4],
                       None, args.minutes, 0.0003, False, definition)
    # 只对验证集选出的最佳检查点做最终留出测试，不用测试结果调参。
    base.old.train(arrays, output / 'verified', 1, 20261128, [1, 2, 3, 4],
                   output / 'mixed/best.pt', 0, 0.0003, True, definition)
    report = {'training': base.read(output / 'mixed/metrics.json'),
              'verified': base.read(output / 'verified/metrics.json'),
              'data': base.read(output / 'audit.json'),
              'deployment': 'candidate only; not yet deployed',
              # 显式记录有效早停配置，覆盖共享训练器旧元数据中的默认 patience。
              'effective_early_stopping': {'validation_patience': 20, 'overfit_window': 3,
                                           'minimum_epochs': 10, 'mse_increase_fraction': 0.05,
                                           'gain_drop': 0.0002},
              'interpretation': 'one-step choice then collector reserve-v6 continuation; not learned full-policy battle strength'}
    base.old.save_json(output / 'report.json', report)
    print(json.dumps({'stage': 'complete', 'output': str(output), 'training': report['training']}, ensure_ascii=True), flush=True)


if __name__ == '__main__':
    main()
