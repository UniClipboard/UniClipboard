#!/usr/bin/env python3
"""Join source-bound real-role samples with weak GPU owners; never infer residency."""
import argparse,json,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--assert-no-dead-textures',action='store_true');a=p.parse_args()
def read(f):return json.loads(f.read_text(encoding='utf-8-sig'))
a.output.parent.mkdir(parents=True,exist_ok=True)
summary=a.output.with_suffix('.samples.json')
subprocess.run([sys.executable,str(Path(__file__).with_name('summarize.py')),str(a.input),'--output',str(summary)],check=True,stdout=subprocess.DEVNULL)
s=read(summary);result={'build':s['build'],'source_manifest_sha256':s['source_manifest_sha256'],'units':'bytes; seconds; milliseconds; counts. GPU imageRGBABytes describe base texture payload, not driver allocation or residency.','runs':{}}
violations=[]
for name,data in s['runs'].items():
 receipt=data['receipt'];directory=a.input/name;rows=read(directory/'samples.json');roles=receipt['identities'];records={}
 if receipt['ownedProcessesRemaining'] or receipt['clipboardSequenceBefore']!=receipt['clipboardSequenceAfter']:raise SystemExit('cleanup/clipboard invariant failed')
 for role,identity in roles.items():
  file=directory/f"runtime.gpu-{identity['pid']}.jsonl"
  if file.exists():records[role]=[json.loads(line) for line in file.read_text().splitlines()]
 phases={}
 for phase,item in data['phases'].items():
  phase_rows=[x for x in rows if x['phase']==phase and x['elapsed']>=item['stableElapsedStart']]
  begin=min(x['utcMillis'] for x in phase_rows);end=max(x['utcMillis'] for x in phase_rows);out={}
  for role in ['gui','quick-panel']:
   if role not in item['roles']:continue
   windows={}
   for event in records.get(role,[]):
    if begin<=event['utcMillis']<=end:windows[event['hwnd']]=event
   metric=item['roles'][role];out[role]={'memory':metric['median'],'go':metric.get('goMedian'),'cache':metric.get('cacheMedian'),'renderers':list(windows.values()),'gpu':{key:sum(x[key] for x in windows.values()) for key in ['imageRGBABytes','deadImageCount','deadImageRGBABytes']}}
   if phase=='after-diagnostic-gc' and out[role]['gpu']['deadImageCount']:violations.append({'run':name,'role':role,'gpu':out[role]['gpu']})
  if phase=='after-diagnostic-gc' and len(out.get('quick-panel',{}).get('renderers',[]))!=2:raise SystemExit('missing two panel renderers in diagnostic phase')
  phases[phase]=out
 result['runs'][name]={'receipt':receipt,'phases':phases,'diagnostic_gpu_events':{role:[x for x in events if x['event']!='periodic'] for role,events in records.items()},'satellite_crops':data['satellite_crops']}
result['unowned_texture_assertion']={'verdict':'FAIL' if violations else 'PASS','violations':violations}
a.output.write_text(json.dumps(result,indent=2)+'\n');print(a.output)
if a.assert_no_dead_textures and violations:raise SystemExit('FAIL: dead Go image owners still retain native textures after diagnostic GC')
