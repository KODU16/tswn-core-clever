"""从已审计 v3 原始包导出数值回归样本，核验 Rust/WASM 与 PyTorch 一致。"""
import argparse
import json
import math
import struct
from pathlib import Path
import numpy as np
import torch
import torch.nn.functional as F
import reserve_v2_trial as base


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw-parent', type=Path, default=Path('C:/codex_tmp/clever-v3-raw'))
    parser.add_argument('--checkpoint', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    cp = torch.load(args.checkpoint, map_location='cpu', weights_only=False)
    w = cp['state_dict']
    samples = []
    # 每个倍率各取1v1、2v2真实决策，避免只检验人工构造的零库存。
    for suffix in ['', '-mult01', '-mult50', '-mult100']:
        raw = args.raw_parent / ('clever-battle-data-v3' + suffix)
        journal = {j['battle_id']: j for j in [json.loads(l) for l in (raw/'progress.journal').read_text(encoding='utf8').splitlines()] if 'battle_id' in j and 'team_size' in j}
        selected = set()
        for path in sorted(raw.glob('states-*.gz')):
            for s in base.lines(path):
                size = journal[s['battle_id']]['team_size']
                ctx = s['decision_context']
                if size in selected or len(ctx['candidates']) < 2:
                    continue
                x, a, kinds, m = base.encode_v2(s, {})
                xt = torch.from_numpy(np.clip((x-np.asarray(cp['xmean'],np.float32))/np.asarray(cp['xstd'],np.float32),-10,10).astype(np.float32))
                at = torch.from_numpy(np.clip((a-np.asarray(cp['amean'],np.float32))/np.asarray(cp['astd'],np.float32),-10,10).astype(np.float32))
                def linear(v, name): return F.linear(v, w[name+'.weight'], w[name+'.bias'])
                with torch.no_grad():
                    state=F.silu(linear(xt,'state.0'))
                    state=F.layer_norm(state,(256,),w['state.2.weight'],w['state.2.bias'],1e-5)
                    state=F.silu(linear(state,'state.4'))
                    act=F.silu(linear(F.silu(linear(at,'action.0')),'action.2'))
                    tiled=state[None,:].expand_as(act)
                    scores=linear(F.silu(linear(torch.cat([tiled,act,tiled*act],-1),'q.0')),'q.3').squeeze(-1)
                actions=[]
                for c,k in zip(ctx['candidates'],kinds):
                    c=c['canonical_action']
                    actions.append(dict(key=k,cost=c.get('magic_cost') or 0,level=c.get('skill_level') or 0,amount=c.get('amount') or 0,targets=[base.entity_id(t) for t in c['target_entity_ids']]))
                reserves=[]
                for r in base.reserves(s):
                    row=dict(actor=base.entity_id(r['actor']),skill=r['skill'],issued=r['issued'],consumed=r['consumed'])
                    for key in ['credit','earned','initial_credit']:
                        row[key+'_bits']=r.get(key+'_bits',struct.unpack('<Q',struct.pack('<d',r.get(key,0)))[0])
                    reserves.append(row)
                samples.append(dict(actor=base.entity_id(ctx['actor_entity_id']),team=ctx['effective_team'],
                    actor_runtime=ctx['actor_runtime_snapshot'],
                    entities=[dict(id=e['id'],max_hp=e['template']['max_hp'],runtime=e['runtime'],skills=e['template']['skills'],states=[st['payload'] for st in e['states']]) for e in s['round_boundary_state']['entities']],
                    actions=actions,context=x.tolist(),action_features=a.tolist(),expected_logits=scores.tolist(),
                    skill_reserves=reserves,multiplier=m))
                selected.add(size)
                if selected == {1,2}: break
            if selected == {1,2}: break
        if selected != {1,2}: raise ValueError('missing population in '+str(raw))
    args.output.write_text(json.dumps(samples,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    print(json.dumps({'samples':len(samples),'output':str(args.output)}))


if __name__ == '__main__': main()
