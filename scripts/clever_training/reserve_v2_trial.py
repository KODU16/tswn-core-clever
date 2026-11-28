"""v2 混合倍率试训：当前动作后原版续战标签，不冒充长期储备收益。"""
import sys,json,gzip,pathlib,hashlib,collections,struct,math,time,os
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[2]
RAW_PARENT=pathlib.Path('C:/codex_tmp')
RAW=pathlib.Path(r'C:/codex_tmp/clever-battle-data-v2')
OUT=ROOT/'crates/nn_handoff/clever-reserve-v2-mixed-all-20261127'
OUT.mkdir(parents=True,exist_ok=True)
sys.path.insert(0,str(ROOT/'scripts/clever_training'))
import gpu_trial as old
SKILLS=['fire','ice','thunder','quake','absorb','poison','rapid','critical','half','exchange','berserk','charm','haste','slow','curse','heal','revive','disperse','iron','charge','accumulate','assassinate','summon','clone','shadow']
def read(p):return json.loads(p.read_text(encoding='utf8'))
def lines(p):
 with gzip.open(p,'rt',encoding='utf8') as f:
  for l in f:yield json.loads(l)
def decode(v):
 return struct.unpack('<d',struct.pack('<Q',int(v)))[0]
def entity_id(v):
 return int(str(v).removeprefix('e'))
def multiplier(s,d):
 for obj in [s['decision_context'],s,d,s['round_boundary_state']]:
  for key in ['skill_multiplier','clever_skill_multiplier','multiplier']:
   if key in obj:return float(obj[key])
  if obj.get('clever_skill_multiplier_micros'):
   return float(obj['clever_skill_multiplier_micros'])/1e6
 raise ValueError('missing multiplier')
def reserves(s):
 ctx=s['decision_context']
 if 'skill_reserves' not in ctx:raise ValueError('decision-point reserve missing; boundary fallback prohibited')
 v=ctx['skill_reserves']
 if isinstance(v,dict):
  if 'entries' in v:v=v['entries']
  else:raise ValueError('unsupported reserve map '+str(list(v)[:8]))
 return v
def reserve_values(r):
 count=float(r.get('available',float(r.get('issued',0))-float(r.get('consumed',0))))
 if 'credit_bits' in r:credit=decode(r['credit_bits'])
 elif 'credit' in r:credit=float(r['credit'])
 elif 'issuance_credit' in r:credit=float(r['issuance_credit'])
 else:raise ValueError('missing pending credit '+str(list(r)))
 return math.log1p(max(0,count)),math.log1p(max(0,credit))
def encode_v2(s,d):
 # 技能语义由导出名确定；专属技能无普通槽号时归入其他动作，不把null当整数。
 import copy
 encoded=copy.deepcopy(s)
 for candidate in encoded["decision_context"]["candidates"]:
  action=candidate["canonical_action"]
  name=(action.get("skill_export_name") or "").removeprefix("core.skill.")
  action["skill_slot_key"]=SKILLS.index(name) if name in SKILLS else -1
 x,a,k=old.encode(encoded)
 ctx=s['decision_context'];actor=entity_id(ctx['actor_entity_id']);team=ctx['effective_team']
 entities={entity_id(e['id']):e for e in s['round_boundary_state']['entities']}
 actor_stock=np.zeros((25,2),dtype=np.float32);ally=np.zeros((25,2),dtype=np.float32);enemy=np.zeros((25,2),dtype=np.float32)
 stock={}
 for r in reserves(s):
  owner=entity_id(r.get('actor',r.get('actor_entity_id',actor)))
  skill=r.get('skill',r.get('export_name',''))
  if skill.startswith('core.skill.'):skill=skill[len('core.skill.'):]
  if skill not in SKILLS:continue
  idx=SKILLS.index(skill);values=reserve_values(r);stock[(owner,idx)]=values
  if owner==actor:actor_stock[idx]+=values
  if owner in entities and entities[owner]['runtime']['alive']:
   (ally if entities[owner]['runtime']['team']==team else enemy)[idx]+=values
 m=multiplier(s,d)
 # 倍率使用对数；库存/额度按技能语义编码，不输入相位、seed、名字或启发式分数。
 x=np.concatenate([x,[math.log(min(m,50.0))],actor_stock.ravel(),ally.ravel(),enemy.ravel()]).astype(np.float32)
 extras=[]
 for c in ctx['candidates']:
  c=c['canonical_action'];name=(c.get('skill_export_name') or '').removeprefix('core.skill.')
  idx=SKILLS.index(name) if name in SKILLS else -1
  values=actor_stock[idx] if c['action_kind']=='builtin_skill' and 0<=idx<25 else [0,0]
  extras.append(values)
 a=np.concatenate([a,np.asarray(extras,dtype=np.float32)],axis=1)
 return x,a,k,m
def prepare():
 manifest=read(RAW/'manifest.json')
 print(json.dumps({'stage':'manifest','continuation_policy':manifest.get('continuation_policy'),'engine_commit':manifest.get('engine_commit')}),flush=True)
 journal={}
 for l in (RAW/'progress.journal').read_text(encoding='utf8').splitlines():
  j=json.loads(l)
  if 'battle_id' in j and 'full_names' in j:journal[j['battle_id']]=j
 sample=next(lines(sorted(RAW.glob('states-*.gz'))[0]))
 dec=next(lines(sorted(RAW.glob('decisions-*.gz'))[0]))
 old.save_json(OUT/'schema-sample.json',{'state':sample,'decision':dec})
 print(json.dumps({'stage':'schema','context_keys':list(sample['decision_context']),'reserve_sample':reserves(sample)[:2],'decision_keys':list(dec),'multiplier':multiplier(sample,dec)},ensure_ascii=True),flush=True)
 records=[];names={};counts=collections.Counter();shards=[];seen=set()
 for completep in sorted(RAW.glob('complete-*.json')):
  complete=read(completep);idx=complete['shard_index']
  paths={k:RAW/f'{k}-{idx:06d}.jsonl.gz' for k in ['states','decisions']}
  for k,p in paths.items():
   digest=hashlib.sha256(p.read_bytes()).hexdigest()
   if digest!=complete[k+'_sha256']:raise ValueError('shard hash mismatch')
   shards.append({'file':p.name,'sha256':digest})
  states={s['state_id']:s for s in lines(paths['states'])};ds=list(lines(paths['decisions']))
  if len(states)!=len(ds) or len(ds)!=complete['decisions']:raise ValueError('shard count mismatch')
  for d in ds:
   s=states[d['state_id']]
   if s['split']!=d['split'] or s['battle_id']!=d['battle_id']:raise ValueError('state mismatch')
   if d['decision_id'] in seen:raise ValueError('duplicate decision')
   seen.add(d['decision_id'])
   j=journal[d['battle_id']]
   for name in sum(j['full_names'],[]):
    bucket=int.from_bytes(hashlib.sha256(('clever-names-v1:'+name).encode()).digest()[:4],'big')%100
    split='train' if bucket<80 else 'validation' if bucket<90 else 'test'
    if split!=d['split'] or names.setdefault(name,split)!=split:raise ValueError('name split leakage')
   x,a,k,m=encode_v2(s,d)
   base=np.asarray(d['baseline']['outcomes'],dtype=np.float32)
   if d['baseline'].get('unresolved',0) or not set(base)<={0.,1.}:raise ValueError('unresolved baseline')
   gains=[];qs=[];ses=[]
   if len(a)!=len(d['actions']):raise ValueError('candidate count mismatch')
   for i,act in enumerate(d['actions']):
    if act['candidate_index']!=i or act['canonical_action']!=s['decision_context']['candidates'][i]['canonical_action']:raise ValueError('candidate mismatch')
    v=np.asarray(act['outcomes'],dtype=np.float32)
    if len(v)!=len(base) or act.get('unresolved',0) or not set(v)<={0.,1.}:raise ValueError('unresolved outcomes')
    if int(v.sum())!=act['wins']:raise ValueError('wins mismatch')
    diff=v-base;gain=float(diff.mean())
    if abs(gain-act['paired_mean'])>1e-6:raise ValueError('paired label mismatch')
    gains.append(gain);qs.append(float(v.mean()));ses.append(float(diff.std(ddof=1)/math.sqrt(len(v))))
   # 物魔不是可学分支：同目标存在两种旧候选时，仅保留简单伤害估计选定类型。
   keep=[];ctx=s['decision_context'];actor_r=ctx['actor_runtime_snapshot']
   entities={entity_id(e['id']):e['runtime'] for e in s['round_boundary_state']['entities']}
   for i,cand in enumerate(ctx['candidates']):
    c=cand['canonical_action']
    if c['action_kind']!='basic_attack':keep.append(i);continue
    targets=c['target_entity_ids']
    same=[z for z,cc in enumerate(ctx['candidates']) if cc['canonical_action']['action_kind']=='basic_attack' and cc['canonical_action']['target_entity_ids']==targets]
    if len(same)==1:keep.append(i);continue
    target=entities[entity_id(targets[0])];at=actor_r['attack'];mag=actor_r['magic']
    phys=max(0,at)*max(0,at+64)/max(1,target['defense']+64)
    magic=max(0,mag)*max(0,mag+64)/max(1,target['resistance']+64)
    use=actor_r['magic_point']>=max(0,(mag-at)>>2) and magic>phys
    if bool(c.get('use_magic'))==use:keep.append(i)
   if not keep:continue
   records.append(dict(x=x,a=a[keep],kinds=np.asarray(k)[keep],gain=np.asarray(gains)[keep],q=np.asarray(qs)[keep],se=np.asarray(ses)[keep],baseline=float(base.mean()),split=d['split'],battle=hashlib.sha256(json.dumps(j['full_names'],sort_keys=True).encode()).hexdigest(),pack=str(m),team_size=j['team_size'],multiplier=m))
   counts[(d['split'],m)]+=1
  print(json.dumps({'stage':'prepare','shard':idx,'decisions':len(records)}),flush=True)
 n=len(records);maximum=max(len(r['a']) for r in records)
 arrays={'x':np.stack([r['x'] for r in records]),'a':np.zeros((n,maximum,records[0]['a'].shape[1]),np.float32),'mask':np.zeros((n,maximum),bool)}
 for key in ['gain','q','se','kinds']:arrays[key]=np.zeros((n,maximum),np.int64 if key=='kinds' else np.float32)
 for i,r in enumerate(records):
  size=len(r['a']);arrays['a'][i,:size]=r['a'];arrays['mask'][i,:size]=True
  for key in ['gain','q','se','kinds']:arrays[key][i,:size]=r[key]
 for key in ['baseline','split','battle','pack','team_size','multiplier']:arrays[key]=np.asarray([r[key] for r in records])
 if not np.isfinite(arrays['x']).all() or not np.isfinite(arrays['a']).all():raise ValueError('nonfinite features')
 np.savez_compressed(OUT/'prepared.npz',**arrays)
 old.save_json(OUT/'audit.json',{'decisions':n,'actions':int(arrays['mask'].sum()),'battles':len(set(arrays['battle'])),'names':len(names),'cross_split_name_overlap':0,'distribution':{str(k):v for k,v in counts.items()},'manifest':manifest,'shards':shards,'feature_widths':[arrays['x'].shape[-1],arrays['a'].shape[-1]],'limitations':['legacy continuation labels: one-step policy improvement only','engine_commit unknown','no guarantee on unsampled multipliers or global optimality']})
 return arrays
def prepare_all():
 global RAW,OUT
 target=OUT
 packs=[];audits=[]
 # 主目录和三个倍率目录必须全部读取，分别校验分片后再合并候选张量。
 for folder in ['clever-battle-data-v2','clever-battle-data-v2-mult01','clever-battle-data-v2-mult50','clever-battle-data-v2-mult100']:
  RAW=RAW_PARENT/folder
  OUT=target/'packs'/folder
  OUT.mkdir(parents=True,exist_ok=True)
  arrays=prepare() if not (OUT/'prepared.npz').exists() else dict(np.load(OUT/'prepared.npz',allow_pickle=False))
  packs.append(arrays);audits.append(read(OUT/'audit.json'))
 OUT=target
 maximum=max(p['mask'].shape[1] for p in packs)
 for p in packs:
  pad=maximum-p['mask'].shape[1]
  for key in ['a','mask','gain','q','se','kinds']:
   padding=[(0,0),(0,pad)]+([(0,0)] if key=='a' else [])
   p[key]=np.pad(p[key],padding)
 arrays={key:np.concatenate([p[key] for p in packs]) for key in packs[0]}
 # 同阵容跨倍率的决策作为同一统计簇；饱和区相同训练样本仅计一次。
 unique=np.ones(len(arrays['x']),bool);seen=set()
 for i in np.flatnonzero(arrays['split']=='train'):
  mask=arrays['mask'][i]
  fingerprint=hashlib.sha256(arrays['x'][i].tobytes()+arrays['a'][i,mask].tobytes()+arrays['q'][i,mask].tobytes()+arrays['gain'][i,mask].tobytes()).digest()
  if fingerprint in seen:unique[i]=False
  seen.add(fingerprint)
 arrays['training_unique']=unique
 np.savez_compressed(OUT/'prepared.npz',**arrays)
 old.save_json(OUT/'audit.json',{'packs':audits,'decisions':len(unique),'actions':int(arrays['mask'].sum()),'duplicate_training_rows_excluded':int((~unique).sum()),'distribution':{str(m):{split:int(((arrays['multiplier']==m)&(arrays['split']==split)).sum()) for split in ['train','validation','test']} for m in np.unique(arrays['multiplier'])},'saturation':'feature multiplier capped at 50; original multiplier retained for reporting','continuation_policy':'legacy','cross_split_name_overlap':0})
 return arrays

def main():
 global RAW_PARENT, OUT
 import argparse
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--raw-parent',type=pathlib.Path,default=RAW_PARENT)
 parser.add_argument('--output',type=pathlib.Path,default=OUT)
 parser.add_argument('--minutes',type=float,default=10)
 parser.add_argument('--evaluate-only',action='store_true')
 args=parser.parse_args()
 if not 0 < args.minutes <= 10:parser.error('minutes must be in (0,10]')
 RAW_PARENT=args.raw_parent;OUT=args.output
 OUT.mkdir(parents=True,exist_ok=True)
 for folder in ['mixed','verified']:(OUT/folder).mkdir(exist_ok=True)
 cache=OUT/'prepared.npz'
 arrays=prepare_all() if not cache.exists() else dict(np.load(cache,allow_pickle=False))
 # 复用原训练器结构、损失和早停；扩展列定义并让检查点显式标记新版本。
 # 显式传入版本和列定义，不动态改写训练器源码。
 original=old.feature_definition()
 original['version']='mechanism-summary-reserve-v2'
 original['validation_patience']=20
 original['overfit_window']=3
 original['overfit_gain_tolerance']=0.0002
 original['context_columns']+=['log_skill_multiplier']+[f'{group}.{skill}.{field}' for group in ['actor_reserve','ally_reserve','enemy_reserve'] for skill in SKILLS for field in ['log1p_available','log1p_credit']]
 original['action_columns']+=['reserve.log1p_available','reserve.log1p_credit']

 print(json.dumps({'stage':'training_start','state_width':arrays['x'].shape[-1],'action_width':arrays['a'].shape[-1],'minutes':args.minutes,'multipliers':np.unique(arrays['multiplier']).tolist()}),flush=True)
 if not args.evaluate_only:
  old.train(arrays,OUT/'mixed',100000,20261127,[1,2,3,4],None,args.minutes,0.0003,False,original)
 # 最佳检查点独立无梯度重评估；按倍率报告，不额外重训专家模型。
 old.train(arrays,OUT/'verified',1,20261127,[1,2,3,4],OUT/'mixed/best.pt',0,0.0003,True,original)
 metrics=read(OUT/'verified/metrics.json');training=read(OUT/'mixed/metrics.json')
 report={'training':training,'verified':metrics,'data':read(OUT/'audit.json'),'deployment':'candidate only; production unchanged pending complete-policy validation'}
 old.save_json(OUT/'report.json',report)
 print(json.dumps({'stage':'complete','output':str(OUT),'stop_reason':training['stop_reason'],'epochs':training['epochs_run'],'best_epoch':training['best_epoch'],'seconds':training['training_seconds'],'validation':metrics['validation'],'test':metrics['test']},ensure_ascii=True),flush=True)
if __name__=='__main__':
 for name in ['mixed','verified']:(OUT/name).mkdir(exist_ok=True)
 try:main()
 except Exception:
  import traceback
  error=traceback.format_exc();(OUT/'failure.txt').write_text(error,encoding='utf8');print(error,flush=True);raise
