"""Clever 原始决策数据的独立 GPU 试训；不替代 Rust encoder-v1 或线上策略。"""
import argparse
import collections
import gzip
import hashlib
import json
import math
import pathlib
import random
import time

import numpy as np

# 注册表编号仅解析技能语义；名字、seed、PBB 分数和随机流均不进入特征。
REGISTRY = {**{6 + i: i for i in range(25)}, 35: 25, 34: 26, 36: 27,
            42: 28, 33: 29, 39: 30, 40: 31, 41: 32, 37: 33, 38: 34,
            **{i: 35 + i for i in range(6)}, 31: 41, 32: 42}
STATUS = ['charm', 'curse', 'poison', 'haste', 'slow', 'iron', 'ice', 'berserk',
          'fire', 'covid_infection', 'lazy_infection', 'unknown']
STATS = ['hp', 'attack', 'defense', 'speed', 'agility', 'magic', 'resistance',
         'wisdom', 'magic_point', 'shield']


def feature_definition():
    # 列名记录完整顺序，便于恢复检查点；不宣称兼容生产 Rust 编码器。
    names = STATS + ['max_hp', 'hp_ratio', 'alive', 'is_summon', 'at_boost',
        'speed_points', 'hidden', 'assassinating', 'upgraded', 'charging',
        'charge_step', 'accumulating', 'protect_from_count', 'protecting']
    skills = ['fire', 'ice', 'thunder', 'quake', 'absorb', 'poison', 'rapid', 'critical',
        'half', 'exchange', 'berserk', 'charm', 'haste', 'slow', 'curse', 'heal',
        'revive', 'disperse', 'iron', 'charge', 'accumulate', 'assassinate',
        'summon', 'clone', 'shadow']
    # 被动技能以归档 fixed_lane_key 标识；额外类别保留注册表中的语义。
    skills += ['passive_lane_' + str(i) for i in range(25, 35)]
    skills += ['custom_summon', 'custom_fire', 'custom_explode', 'custom_possess',
               'custom_heal', 'registry_5', 'summon_explode', 'summon_share_damage']
    names += [f'skill.{skill}.{field}' for skill in skills for field in ['level', 'build_level']]
    names += [f'status.{kind}.{field}' for kind in STATUS for field in ['count', 'step', 'magnitude']]
    pool_names = ['count'] + [f'{op}.{name}' for op in ['mean', 'max'] for name in names]
    context = ['actor.' + name for name in names]
    context += [f'{group}.{name}' for group in ['alive_allies', 'alive_enemies', 'dead_allies', 'dead_enemies'] for name in pool_names]
    action = ['basic_physical', 'basic_magic'] + ['builtin_lane_' + str(i) for i in range(25)] + ['other_action']
    action += ['cost', 'skill_level', 'amount', 'self_targets', 'ally_targets', 'enemy_targets']
    action += ['targets.' + name for name in pool_names]
    return {'version': 'mechanism-summary-trial-v1', 'context_columns': context,
            'action_columns': action, 'registry_to_semantic_index': REGISTRY}


def save_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf8')


def entity_vector(entity, runtime=None):
    """只读取明确的战斗数值；汇总所有实体，不沿用旧版六实体截断。"""
    r = runtime if runtime is not None else entity['runtime']
    t = entity['template']
    v = [float(r.get(k, 0)) for k in STATS]
    hpmax = max(1, t['max_hp'])
    v += [hpmax, r['hp'] / hpmax, float(r['alive']), float(r['owner'] != entity['id']),
          r.get('at_boost_millionths', 1000000) / 1e6,
          r.get('move_state', {}).get('speed_points', 0),
          float(bool(r.get('hide'))), float(bool(r.get('assassinate'))),
          float(r.get('upgrade_active', False)),
          float(r.get('charge', {}).get('active', False)),
          r.get('charge', {}).get('step', 0),
          float(r.get('accumulate', {}).get('active', False)),
          len(r.get('protect_from', [])), float(r.get('protect_to') is not None)]
    levels = np.zeros((43, 2), dtype=np.float32)
    for lane in t['skills']['lanes']:
        key = REGISTRY.get(lane['skill_id'])
        if key is None:
            raise ValueError('unknown skill registry id: ' + str(lane['skill_id']))
        levels[key] = np.maximum(levels[key], [lane['level'], lane['build_level']])
    v += levels.ravel().tolist()
    states = np.zeros((len(STATUS), 3), dtype=np.float32)
    for entry in entity['states']:
        payload = entry['payload']
        kind = payload['kind']
        key = STATUS.index(kind) if kind in STATUS else len(STATUS) - 1
        detail = payload.get(kind) or {}
        states[key, 0] += 1
        if isinstance(detail, dict):
            states[key, 1] = max(states[key, 1], detail.get('step', 0))
            states[key, 2] = max(states[key, 2], detail.get('protect', detail.get('damage', 0)))
    return np.asarray(v + states.ravel().tolist(), dtype=np.float32)


def pooled(entities, vectors, width):
    if not entities:
        return np.zeros(width * 2 + 1, dtype=np.float32)
    a = np.stack([vectors[e['id']] for e in entities])
    return np.concatenate(([len(entities)], a.mean(0), a.max(0))).astype(np.float32)


def encode(state):
    ctx = state['decision_context']
    entities = state['round_boundary_state']['entities']
    actor_id = int(ctx['actor_entity_id'].removeprefix('e'))
    team = ctx['effective_team']
    entities_by_id = {e['id']: e for e in entities}
    runtime = {e['id']: e['runtime'] for e in entities}
    runtime[actor_id] = ctx['actor_runtime_snapshot']
    vecs = {e['id']: entity_vector(e, runtime[e['id']]) for e in entities}
    width = len(vecs[actor_id])
    groups = [[e for e in entities if (runtime[e['id']]['team'] == team) == ally
               and bool(runtime[e['id']]['alive']) == living]
              for ally, living in [(True, True), (False, True), (True, False), (False, False)]]
    context = np.concatenate([vecs[actor_id]] + [pooled(g, vecs, width) for g in groups])
    actions = []
    kinds = []
    for cand in ctx['candidates']:
        c = cand['canonical_action']
        onehot = np.zeros(28, dtype=np.float32)
        if c['action_kind'] == 'basic_attack':
            key = 1 if c.get('use_magic') else 0
        elif c['action_kind'] == 'builtin_skill' and 0 <= c['skill_slot_key'] <= 24:
            key = c['skill_slot_key'] + 2
        else:
            key = 27
        onehot[key] = 1
        targets = [entities_by_id[int(i.removeprefix('e'))] for i in c['target_entity_ids']]
        extra = [c.get('magic_cost') or 0, c.get('skill_level') or 0, c.get('amount') or 0,
                 sum(e['id'] == actor_id for e in targets),
                 sum(runtime[e['id']]['team'] == team for e in targets),
                 sum(runtime[e['id']]['team'] != team for e in targets)]
        actions.append(np.concatenate([onehot, extra, pooled(targets, vecs, width)]))
        kinds.append(key)
    return context, np.asarray(actions, dtype=np.float32), kinds


def extract_archive(archive, target):
    import tarfile
    # 手动限制解包边界；归档内的脚本和补丁仅作数据保存，绝不执行。
    target.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as tar:
        for member in tar:
            dest = (target / member.name).resolve()
            if not dest.is_relative_to(target.resolve()):
                raise ValueError('archive path escapes output directory')
            if member.isdir():
                dest.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                dest.parent.mkdir(parents=True, exist_ok=True)
                with tar.extractfile(member) as src, dest.open('wb') as out:
                    import shutil
                    shutil.copyfileobj(src, out)
            else:
                raise ValueError('archive contains non-regular file')
    return [p.parent for p in sorted(target.glob('*/manifest.json'))]


def json_lines(path):
    with gzip.open(path, 'rt', encoding='utf8') as stream:
        for line in stream:
            yield json.loads(line)


def prepare(data, output):
    records, audit, name_splits, roster_seen = [], [], {}, set()
    seen_states, seen_ids = set(), set()
    # 补充归档内有四个数据包，逐一发现，不能只读取第一个 manifest。
    jobs = []
    for archive_pack in ['batch001', 'supplement']:
        archive = data / ('clever-battle-data-v1-' + archive_pack + '.tar.gz')
        jobs.extend((archive, root) for root in extract_archive(archive, output / 'raw' / archive_pack))
    for archive, root in jobs:
        pack = root.name.removeprefix('clever-battle-data-v1-')
        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf8'))
        journal = {}
        for line in (root / 'progress.journal').read_text(encoding='utf8').splitlines():
            j = json.loads(line)
            if 'battle_id' in j and 'full_names' in j:
                journal[j['battle_id']] = j
        counts = collections.Counter()
        shard_hashes = []
        for complete_path in sorted(root.glob('complete-*.json')):
            complete = json.loads(complete_path.read_text(encoding='utf8'))
            index = complete['shard_index']
            paths = {k: root / f'{k}-{index:06d}.jsonl.gz' for k in ['states', 'decisions']}
            for kind, path in paths.items():
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
                assert digest == complete[kind + '_sha256'], path
                shard_hashes.append({'file': path.name, 'sha256': digest})
            states = {s['state_id']: s for s in json_lines(paths['states'])}
            decisions = list(json_lines(paths['decisions']))
            assert len(states) == len(decisions) == complete['decisions']
            for d in decisions:
                s = states[d['state_id']]
                assert s['split'] == d['split'] and s['battle_id'] == d['battle_id']
                assert s['decision_context']['actor_entity_id'] == d['actor_entity_id']
                assert s['decision_context']['effective_team'] == d['perspective_effective_team']
                unique_id = (pack, d['decision_id'])
                assert unique_id not in seen_ids
                seen_ids.add(unique_id)
                j = journal[d['battle_id']]
                for name in sum(j['full_names'], []):
                    bucket = int.from_bytes(hashlib.sha256(('clever-names-v1:' + name).encode()).digest()[:4], 'big') % 100
                    split = 'train' if bucket < 80 else 'validation' if bucket < 90 else 'test'
                    assert split == d['split'], (pack, d['battle_id'], 'name split mismatch')
                    assert name_splits.setdefault(name, split) == split
                roster_key = json.dumps(j['full_names'], ensure_ascii=True)
                roster_seen.add((d['split'], roster_key))
                fingerprint = hashlib.sha256(json.dumps(s['round_boundary_state'], sort_keys=True).encode()).hexdigest()
                assert (d['split'], fingerprint) not in seen_states, 'duplicate state across packs'
                seen_states.add((d['split'], fingerprint))
                baseline = d['baseline']
                base = np.asarray(baseline['outcomes'], dtype=np.float32)
                assert len(base) == baseline['n_trials'] == 256
                assert baseline['unresolved'] == 0 and set(base) <= {0., 1.}
                assert sum(base) == baseline['wins'] and baseline['wins'] + baseline['losses'] == 256
                assert len(d['actions']) == len(s['decision_context']['candidates'])
                context, actions, kinds = encode(s)
                gains, probabilities, ses = [], [], []
                for i, a in enumerate(d['actions']):
                    assert a['candidate_index'] == i
                    assert a['canonical_action'] == s['decision_context']['candidates'][i]['canonical_action']
                    outcomes = np.asarray(a['outcomes'], dtype=np.float32)
                    assert len(outcomes) == 256 and set(outcomes) <= {0., 1.} and a['unresolved'] == 0
                    assert outcomes.sum() == a['wins'] and a['wins'] + a['losses'] == 256
                    diff = outcomes - base
                    gain = float(diff.mean())
                    assert abs(gain - a['paired_mean']) < 1e-7
                    assert abs(gain - a['advantage_lower']) < 1e-7
                    assert abs(gain - a['advantage_upper']) < 1e-7
                    gains.append(gain)
                    probabilities.append(float(outcomes.mean()))
                    ses.append(float(diff.std(ddof=1) / 16))
                records.append(dict(x=context, a=actions, kinds=kinds, gain=gains, q=probabilities,
                                    se=ses, baseline=float(base.mean()), split=d['split'],
                                    battle=pack + ':' + d['battle_id'], pack=pack, team_size=j['team_size']))
                counts[d['split']] += 1
                counts['actions'] += len(actions)
            print(json.dumps({'stage': 'prepare', 'pack': pack, 'shard': index, 'decisions': len(records)}), flush=True)
        audit.append({'pack': pack, 'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
                      'counts': dict(counts), 'completed_shards': len(shard_hashes) // 2,
                      'engine_commit': manifest.get('engine_commit'), 'shards': shard_hashes})
    # 只保存数值训练张量与不入模的审计元数据；不保存名字作为模型输入。
    n, maximum = len(records), max(len(r['a']) for r in records)
    arrays = {'x': np.stack([r['x'] for r in records]),
              'a': np.zeros((n, maximum, len(records[0]['a'][0])), dtype=np.float32),
              'mask': np.zeros((n, maximum), dtype=bool)}
    for k in ['gain', 'q', 'se', 'kinds']:
        arrays[k] = np.zeros((n, maximum), dtype=np.float32 if k != 'kinds' else np.int64)
    for i, r in enumerate(records):
        m = len(r['a'])
        arrays['a'][i, :m] = r['a']
        arrays['mask'][i, :m] = True
        for k in ['gain', 'q', 'se', 'kinds']:
            arrays[k][i, :m] = r[k]
    for k in ['baseline', 'split', 'battle', 'pack', 'team_size']:
        arrays[k] = np.asarray([r[k] for r in records])
    np.savez_compressed(output / 'prepared.npz', **arrays)
    save_json(output / 'audit.json', {'packs': audit, 'decisions': n, 'actions': int(arrays['mask'].sum()),
              'battles': len(set(arrays['battle'])), 'audited_names': len(name_splits),
              'rosters': len(roster_seen),
              'cross_split_name_overlap': 0, 'duplicate_states': 0,
              'pilot': 'excluded: only 64 rollouts, retained untouched',
              'features': 'mechanism-summary-trial-v1', 'state_width': arrays['x'].shape[-1],
              'action_width': arrays['a'].shape[-1], 'max_candidates': maximum,
              'limitations': ['actor snapshot updated; other actors remain at clean boundary',
                              'manifest engine_commit unknown; archived README is not exact provenance',
                              'paired seed alignment follows exported protocol, not independent replay']})
    return arrays


def train(arrays, output, epochs, seed, team_sizes, resume=None, minutes=0, learning_rate=3e-4, evaluate_only=False):
    import torch
    import torch.nn as nn
    import torch.nn.functional as F
    assert torch.cuda.is_available(), 'CUDA required; no silent CPU fallback'
    definition = feature_definition()
    assert len(definition['context_columns']) == arrays['x'].shape[-1]
    assert len(definition['action_columns']) == arrays['a'].shape[-1]
    save_json(output / 'feature_definition.json', definition)
    torch.set_num_threads(8)
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)
    torch.backends.cudnn.deterministic = True
    torch.backends.cudnn.benchmark = False
    # 专家模型与混合模型复用同一划分，只按参战人数限制样本。
    eligible = np.isin(arrays['team_size'], team_sizes)
    train_ids = np.flatnonzero((arrays['split'] == 'train') & eligible)
    val_ids = np.flatnonzero((arrays['split'] == 'validation') & eligible)
    test_ids = np.flatnonzero((arrays['split'] == 'test') & eligible)
    if min(len(train_ids), len(val_ids), len(test_ids)) == 0:
        raise ValueError('each selected population requires train, validation and test samples')
    # 标准化参数仅拟合训练分区；测试集不参与早停或配置选择。
    xmean, xstd = arrays['x'][train_ids].mean(0), arrays['x'][train_ids].std(0).clip(.1)
    av = arrays['a'][train_ids][arrays['mask'][train_ids]]
    amean, astd = av.mean(0), av.std(0).clip(.1)
    x = np.clip((arrays['x'] - xmean) / xstd, -10, 10)
    a = np.clip((arrays['a'] - amean) / astd, -10, 10)
    assert np.isfinite(x).all() and np.isfinite(a).all()
    tensor = {k: torch.as_tensor(v, device='cuda') for k, v in
              dict(x=x, a=a, mask=arrays['mask'], q=arrays['q'], gain=arrays['gain'],
                   baseline=arrays['baseline'].astype(np.float32)).items()}

    class Policy(nn.Module):
        def __init__(self):
            super().__init__()
            self.state = nn.Sequential(nn.Linear(x.shape[-1], 256), nn.SiLU(), nn.LayerNorm(256),
                                       nn.Dropout(.1), nn.Linear(256, 128), nn.SiLU())
            self.action = nn.Sequential(nn.Linear(a.shape[-1], 128), nn.SiLU(), nn.Linear(128, 128), nn.SiLU())
            self.q = nn.Sequential(nn.Linear(384, 128), nn.SiLU(), nn.Dropout(.1), nn.Linear(128, 1))
            self.v = nn.Linear(128, 1)

        def forward(self, context, action):
            state = self.state(context)
            act = self.action(action)
            tiled = state[:, None, :].expand_as(act)
            return self.q(torch.cat([tiled, act, tiled * act], -1)).squeeze(-1), self.v(state).squeeze(-1)

    model = Policy().cuda()
    resumed_epoch = 0
    if resume:
        # 旧检查点只存了权重，校验尺度后以较低学习率重建优化器继续训练。
        previous = torch.load(resume, weights_only=False, map_location='cuda')
        assert previous['feature_version'] == 'mechanism-summary-trial-v1'
        for key, expected in [('xmean', xmean), ('xstd', xstd), ('amean', amean), ('astd', astd)]:
            np.testing.assert_allclose(previous[key], expected, rtol=0, atol=1e-6)
        model.load_state_dict(previous['state_dict'])
        resumed_epoch = previous['epoch']
    opt = torch.optim.AdamW(model.parameters(), lr=learning_rate, weight_decay=.01)
    scaler = torch.amp.GradScaler('cuda')
    best, best_epoch, stale, history = -math.inf, 0, 0, []
    started = time.time()

    def predict(ids):
        model.eval()
        out = []
        with torch.no_grad():
            for ids_batch in np.array_split(ids, max(1, math.ceil(len(ids) / 256))):
                q, _ = model(tensor['x'][ids_batch], tensor['a'][ids_batch])
                out.append(q.float().cpu().numpy())
        return np.concatenate(out)

    def checkpoint(epoch):
        return {'state_dict': model.state_dict(), 'xmean': xmean.tolist(), 'xstd': xstd.tolist(),
                'amean': amean.tolist(), 'astd': astd.tolist(), 'feature_version': 'mechanism-summary-trial-v1',
                'epoch': epoch, 'seed': seed, 'state_width': x.shape[-1], 'action_width': a.shape[-1],
                'optimizer_state_dict': opt.state_dict(), 'grad_scaler_state_dict': scaler.state_dict()}

    initial_validation = None
    if resume:
        initial_pred = predict(val_ids)
        initial_choice = np.where(arrays['mask'][val_ids], initial_pred, -np.inf).argmax(1)
        initial_validation = float(arrays['gain'][val_ids, initial_choice].mean())
        best, best_epoch = initial_validation, resumed_epoch
        torch.save(checkpoint(resumed_epoch), output / 'best.pt')
    budget_seconds = minutes * 60
    stop_reason = 'evaluation_only' if evaluate_only else 'epoch_limit'
    # 达到时间预算或出现持续过拟合即停止；评估模式不执行任何梯度更新。
    iterations = [] if evaluate_only else range(1, (2**31 if budget_seconds else epochs) + 1)
    for local_epoch in iterations:
        elapsed = time.time() - started
        if budget_seconds and elapsed >= budget_seconds:
            stop_reason = 'time_budget'
            break
        epoch = resumed_epoch + local_epoch
        if budget_seconds:
            fraction = min(1., elapsed / budget_seconds)
            rate = learning_rate * (.03 + .97 * .5 * (1 + math.cos(math.pi * fraction)))
            for group in opt.param_groups:
                group['lr'] = rate
        model.train()
        order = np.random.permutation(train_ids)
        losses = []
        for start in range(0, len(order), 128):
            ids = order[start:start + 128]
            mask = tensor['mask'][ids]
            opt.zero_grad(set_to_none=True)
            with torch.amp.autocast('cuda'):
                logits, base_logits = model(tensor['x'][ids], tensor['a'][ids])
                qloss = F.binary_cross_entropy_with_logits(logits, tensor['q'][ids], reduction='none')
                qloss = ((qloss * mask).sum(1) / mask.sum(1)).mean()
                vloss = F.binary_cross_entropy_with_logits(base_logits, tensor['baseline'][ids])
                predicted_gain = logits.sigmoid() - base_logits.sigmoid()[:, None]
                advantage_loss = (((predicted_gain - tensor['gain'][ids]).square() * mask).sum(1) / mask.sum(1)).mean()
                target = torch.softmax((tensor['gain'][ids] / .08).masked_fill(~mask, -1e4), 1)
                rank_loss = -(target * F.log_softmax((logits / .5).masked_fill(~mask, -1e4), 1)).sum(1).mean()
                loss = qloss + .25 * vloss + 2 * advantage_loss + rank_loss
            scaler.scale(loss).backward()
            scaler.unscale_(opt)
            torch.nn.utils.clip_grad_norm_(model.parameters(), 5)
            scaler.step(opt)
            scaler.update()
            losses.append(float(loss.detach()))
        pred = predict(val_ids)
        selected = np.where(arrays['mask'][val_ids], pred, -np.inf).argmax(1)
        gain = float(arrays['gain'][val_ids, selected].mean())
        probabilities = 1 / (1 + np.exp(-np.clip(pred, -30, 30)))
        validation_error = float(((probabilities - arrays['q'][val_ids]) ** 2)[arrays['mask'][val_ids]].mean())
        row = {'epoch': epoch, 'loss': float(np.mean(losses)), 'validation_selected_gain': gain,
               'validation_probability_mse': validation_error, 'learning_rate': opt.param_groups[0]['lr'],
               'elapsed_seconds': time.time() - started}
        history.append(row)
        print(json.dumps(row), flush=True)
        if gain > best + 1e-6:
            best, best_epoch, stale = gain, epoch, 0
            torch.save(checkpoint(epoch), output / 'best.pt')
        else:
            stale += 1
        # 同时出现训练损失下降、验证误差上升和动作收益下降，才判断明显过拟合。
        if len(history) >= 10:
            recent, initial = history[-5:], history[:5]
            train_falling = np.mean([r['loss'] for r in recent]) < np.mean([r['loss'] for r in initial]) - .002
            mse_rising = np.mean([r['validation_probability_mse'] for r in recent]) > min(r['validation_probability_mse'] for r in history) * 1.05
            gain_falling = np.mean([r['validation_selected_gain'] for r in recent]) < best - .001
            if train_falling and mse_rising and gain_falling:
                stop_reason = 'overfitting: lower train loss, higher validation MSE, lower validation gain'
                break
        if not budget_seconds and stale >= 10:
            stop_reason = 'validation_patience'
            break
    if history:
        torch.save(checkpoint(history[-1]['epoch']), output / 'last.pt')
    save_json(output / 'history.json', history)
    model.load_state_dict(torch.load(output / 'best.pt', weights_only=False, map_location='cuda')['state_dict'])
    # 技能先验只使用训练集；为动作选择提供不看当前状态的朴素参照。
    priors = np.zeros(28)
    for kind in range(28):
        matched = (arrays['kinds'][train_ids] == kind) & arrays['mask'][train_ids]
        if matched.any():
            priors[kind] = arrays['gain'][train_ids][matched].mean()

    def cluster_ci(values, groups):
        unique, inverse = np.unique(groups, return_inverse=True)
        sums = np.bincount(inverse, weights=values)
        counts = np.bincount(inverse)
        rng = np.random.default_rng(seed + 88)
        estimates = []
        for _ in range(2000):
            sample = rng.integers(len(unique), size=len(unique))
            estimates.append(sums[sample].sum() / counts[sample].sum())
        return np.quantile(estimates, [.025, .975]).tolist()

    def evaluate(ids):
        pred = predict(ids)
        mask = arrays['mask'][ids]
        selected = np.where(mask, pred, -np.inf).argmax(1)
        selected_gain = arrays['gain'][ids, selected]
        uniform = (arrays['gain'][ids] * mask).sum(1) / mask.sum(1)
        prior_choice = np.where(mask, priors[arrays['kinds'][ids]], -np.inf).argmax(1)
        prior_gain = arrays['gain'][ids, prior_choice]
        qhat = 1 / (1 + np.exp(-np.clip(pred, -30, 30)))
        truth = arrays['q'][ids]
        mse = (qhat - truth) ** 2
        bce = -(truth * np.log(qhat.clip(1e-7, 1)) + (1 - truth) * np.log((1 - qhat).clip(1e-7, 1)))
        result = {'decisions': len(ids), 'battles': len(set(arrays['battle'][ids])),
                  'selected_gain': float(selected_gain.mean()),
                  'selected_gain_battle_bootstrap_ci95': cluster_ci(selected_gain, arrays['battle'][ids]),
                  'uniform_gain': float(uniform.mean()), 'train_skill_prior_gain': float(prior_gain.mean()),
                  'gain_over_uniform': float((selected_gain - uniform).mean()),
                  'gain_over_uniform_ci95': cluster_ci(selected_gain - uniform, arrays['battle'][ids]),
                  'gain_over_skill_prior': float((selected_gain - prior_gain).mean()),
                  'gain_over_skill_prior_ci95': cluster_ci(selected_gain - prior_gain, arrays['battle'][ids]),
                  'empirical_oracle_gain_optimistic': float(np.where(mask, arrays['gain'][ids], -np.inf).max(1).mean()),
                  'probability_mse_to_rollout_rate': float(mse[mask].mean()),
                  'probability_mae_to_rollout_rate': float(np.abs(qhat - truth)[mask].mean()),
                  'probability_cross_entropy': float(bce[mask].mean()), 'by_team_size': {}, 'by_pack': {}}
        for field in ['team_size', 'pack']:
            for value in np.unique(arrays[field][ids]):
                use = arrays[field][ids] == value
                result['by_' + field][str(value)] = {'decisions': int(use.sum()),
                    'selected_gain': float(selected_gain[use].mean()), 'uniform_gain': float(uniform[use].mean())}
        np.savez_compressed(output / ('predictions-' + str(arrays['split'][ids[0]]) + '.npz'),
                            ids=ids, logits=pred, selected=selected, gains=selected_gain)
        return result

    results = {'seed': seed, 'team_sizes': team_sizes, 'training_decisions': len(train_ids),
               'evaluate_only': evaluate_only, 'stop_reason': stop_reason,
               'resume_checkpoint': str(resume) if resume else None,
               'resume_sha256': hashlib.sha256(resume.read_bytes()).hexdigest() if resume else None,
               'resumed_epoch': resumed_epoch, 'initial_validation_selected_gain': initial_validation,
               'requested_minutes': minutes, 'last_epoch': history[-1] if history else None,
               'resume_optimizer': 'new AdamW with warm restart' if resume else None,
               'best_epoch': best_epoch, 'epochs_run': len(history),
               'training_seconds': time.time() - started, 'torch': torch.__version__,
               'gpu': torch.cuda.get_device_name(0), 'cuda': torch.version.cuda,
               'peak_gpu_allocated_mib': torch.cuda.max_memory_allocated() / 2**20,
               'parameters': sum(p.numel() for p in model.parameters()),
               'validation': evaluate(val_ids), 'test': evaluate(test_ids),
               'configuration': {'optimizer': 'AdamW', 'lr': learning_rate, 'weight_decay': .01,
                    'schedule': 'wall-clock cosine to 3% lr' if minutes else 'constant',
                    'batch_size': 128, 'max_epochs': epochs, 'patience': 0 if minutes else 10, 'amp': True,
                    'q_loss_weight': 1, 'v_loss_weight': .25, 'advantage_mse_weight': 2,
                    'listwise_loss_weight': 1, 'target_temperature': .08, 'logit_temperature': .5}}
    save_json(output / 'history.json', history)
    save_json(output / 'metrics.json', results)
    print(json.dumps(results), flush=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--data', type=pathlib.Path, required=True)
    p.add_argument('--output', type=pathlib.Path, required=True)
    p.add_argument('--prepared', type=pathlib.Path, help='复用已校验的数值缓存，避免重复解包')
    p.add_argument('--team-sizes', type=int, nargs='+', default=[1, 2, 3, 4])
    p.add_argument('--evaluate-only', action='store_true', help='只验证已有检查点，不进行训练')
    p.add_argument('--resume', type=pathlib.Path, help='沿用模型权重与尺度，以新 AdamW 继续训练')
    p.add_argument('--minutes', type=float, default=0, help='按分钟运行，替代基于轮数的早停')
    p.add_argument('--lr', type=float, default=3e-4)
    p.add_argument('--epochs', type=int, default=40)
    p.add_argument('--seed', type=int, default=20261127)
    args = p.parse_args()
    if args.evaluate_only and not args.resume:
        p.error('--evaluate-only requires --resume')
    if args.minutes < 0 or args.lr <= 0 or args.epochs < 1:
        p.error('minutes must be nonnegative; lr and epochs must be positive')
    if not args.team_sizes or not set(args.team_sizes) <= {1, 2, 3, 4}:
        p.error('team-sizes must be selected from 1, 2, 3, 4')
    args.output.mkdir(parents=True, exist_ok=True)
    cache = args.prepared if args.prepared else args.output / 'prepared.npz'
    if args.prepared and not cache.exists():
        raise FileNotFoundError(cache)
    if cache.exists():
        with np.load(cache, allow_pickle=False) as loaded:
            arrays = {k: loaded[k] for k in loaded.files}
    else:
        arrays = prepare(args.data, args.output)
    train(arrays, args.output, args.epochs, args.seed, args.team_sizes, args.resume, args.minutes, args.lr, args.evaluate_only)


if __name__ == '__main__':
    main()
