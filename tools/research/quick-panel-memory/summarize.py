#!/usr/bin/env python3
"""Compact source-bound role measurements; counters remain bytes."""
import argparse, hashlib, json, statistics
from pathlib import Path
from PIL import Image, ImageStat
p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args()
def read(f):return json.loads(f.read_text(encoding='utf-8-sig'))
def median(values):return statistics.median(values) if values else None
source=read(a.input/'build-identity.json');result={'build':{k:v for k,v in source.items() if k!='source_sha256'},'source_manifest_sha256':hashlib.sha256(json.dumps(source['source_sha256'],sort_keys=True).encode()).hexdigest(),'product_source_sha256':{k:v for k,v in source['source_sha256'].items() if k.startswith('app/desktop/') and k.endswith('.go') and not k.endswith('_test.go')},'runs':{}}
for directory in sorted(a.input.iterdir()):
 if not directory.is_dir() or not (directory/'receipt.json').exists():continue
 receipt=read(directory/'receipt.json');rows=read(directory/'samples.json') if (directory/'samples.json').exists() else []
 if isinstance(rows,dict):rows=rows.get('value',[])
 phases={}
 for phase in dict.fromkeys(x['phase'] for x in rows):
  data=[x for x in rows if x['phase']==phase];end=max(x['elapsed'] for x in data);stable=[x for x in data if x['elapsed']>=end/2];roles={}
  for role in dict.fromkeys(x['role'] for x in data):
   r=[x for x in stable if x['role']==role];allrows=[x for x in data if x['role']==role]
   roles[role]={'samples':len(r),'median':{k:median([x[k] for x in r]) for k in ['privateWorkingSet','workingSet','privateCommit','handles','gdi','user','threads','visible']},'peak':{k:max(x[k] for x in allrows) for k in ['privateWorkingSet','workingSet','privateCommit']},'kernelPeakWorkingSetAtEnd':allrows[-1].get('kernelPeakWorkingSet'),'cpuAtEnd':allrows[-1]['cpu'],'cpuInPhase':allrows[-1]['cpu']-allrows[0]['cpu']}
  ticks={}
  for row in stable:ticks.setdefault(row.get('sample',round(row['elapsed'])),[]).append(row)
  groups=[{k:sum(r[k] for r in group) for k in ['privateWorkingSet','workingSet','privateCommit','cpu']} for group in ticks.values() if len({r['role'] for r in group})==len(roles)]
  phases[phase]={'stableElapsedStart':end/2,'stableElapsedEnd':end,'roles':roles,'groupMedian':{k:median([g[k] for g in groups]) for k in ['privateWorkingSet','workingSet','privateCommit','cpu']},'groupPairing':'same sample; total WS may double-count shared pages'}
 go={}
 for f in directory.glob('runtime.unic*.jsonl'):
  records=[json.loads(line) for line in f.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
  if not records:continue
  role=next((name for name,item in receipt['identities'].items() if item['pid']==records[0]['pid']),None)
  if not role:continue
  # Metric clocks start during process initialization; report all samples rather than pretend exact phase alignment.
  go[role]={'samples':len(records),'last':records[-1],'peak':{k:max(r[k] for r in records) for k in ['heapAlloc','heapSys','sys','goroutines']}}
 for phase,item in phases.items():
  phaseRows=[x for x in rows if x['phase']==phase and x['elapsed']>=item['stableElapsedStart']]
  if not phaseRows:continue
  begin=min(x['utcMillis'] for x in phaseRows);end=max(x['utcMillis'] for x in phaseRows)
  for role,identity in receipt['identities'].items():
   if role not in item['roles']:continue
   # Windows paths are not host Path separators.
   metricsFile=next(iter(directory.glob(f"runtime.unic*-{identity['pid']}.jsonl")),None)
   if metricsFile:
    logs=[json.loads(line) for line in metricsFile.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
    selected=[r for r in logs if begin<=r.get('utcMillis',-1)<=end]
    item['roles'][role]['goMedian']={k:median([r[k] for r in selected]) for k in ['heapAlloc','heapSys','heapInuse','heapReleased','sys','goroutines','numGC']}
   cacheFile=directory/f"runtime.{identity['pid']}.cache.jsonl"
   if cacheFile.exists():
    logs=[json.loads(line) for line in cacheFile.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
    selected=[r for r in logs if begin<=r.get('utcMillis',-1)<=end]
    item['roles'][role]['cacheMedian']={k:median([r[k] for r in selected]) for k in ['imageObjects','fullImages','releasedImages','imageCPUPixelBytes','details','inflight','wanted','projectedRows']}
 crops={}
 for frame in directory.glob('*satellite.png'):
  image=Image.open(frame).convert('RGBA');width,height=image.size
  crop=image.crop((width//2-64,height//2-64,width//2+64,height//2+64))
  deviation=ImageStat.Stat(crop.convert('RGB')).stddev
  if receipt['dataset']=='images' and min(deviation)<5:raise SystemExit('native image crop is blank: '+str(frame))
  crops[frame.name]={'frame_size':[width,height],'center_crop_size':[128,128],'rgba_sha256':hashlib.sha256(crop.tobytes()).hexdigest(),'channel_stddev':deviation}
 result['runs'][directory.name]={'receipt':receipt,'phases':phases,'go':go,'frame_sha256':{f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in directory.glob('*.png')},'satellite_crops':crops,'seed_history_sha256':hashlib.sha256((directory/'seed-history.json').read_bytes()).hexdigest() if (directory/'seed-history.json').exists() else None}
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n')
print(a.output)
