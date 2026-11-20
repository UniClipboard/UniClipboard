#!/usr/bin/env python3
"""Keep process counters, native crop hashes and bounded latency/cache receipts."""
import argparse,csv,hashlib,importlib.util,json,statistics
from collections import defaultdict
from pathlib import Path
from PIL import Image

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('base_summary',HERE.parent/'windows-desktop-memory/summarize.py')
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)

def summarize(root,output):
 runs=base.summarize(root);pixel_groups=defaultdict(list)
 for r in runs:
  folder=root/r['run'];r['variant'],r['architecture']=r['run'].split('-')[:2]
  samples=base.read(folder/'samples.json')
  r['group_phases']=[]
  for phase in dict.fromkeys(row['phase'] for row in samples):
   low=30 if phase=='visible' else 15 if phase in ('hidden','reopened') else 0
   paired=defaultdict(dict)
   for row in samples:
    if row['phase']==phase:paired[row['elapsedSeconds']][row['role']]=row
   complete=[(elapsed,roles) for elapsed,roles in paired.items() if 'desktop' in roles and 'service' in roles]
   settled=[roles for elapsed,roles in complete if elapsed>=low]
   metrics={}
   for key in ['workingSet','privateWorkingSet','privateBytes']:
    values=[sum(role[key] for role in roles.values()) for roles in settled]
    metrics[key]={'min':min(values),'median':statistics.median(values),'max':max(values)}
   r['group_phases'].append({'phase':phase,'settled_from_seconds':low,'paired_samples':len(settled),'metrics':metrics,'sampled_full_phase_peak':{key:max(sum(role[key] for role in roles.values()) for _,roles in complete) for key in ['workingSet','privateWorkingSet','privateBytes']},'total_ws_sum_may_count_shared_pages_twice':True})
  for phase in r['phases']:
   rows=[row for row in samples if row['phase']==phase['phase'] and row['role']==phase['role']]
   phase['sampled_full_phase_peak']={key:max(row[key] for row in rows) for key in ['workingSet','privateWorkingSet','privateBytes']}

  f=folder/'runtime.latency.jsonl'
  r['latency_events']=[json.loads(line) for line in f.read_text().splitlines() if line.strip()] if f.exists() else []
  r['latency_events']=[x for x in r['latency_events'] if x['pid']==r['identity']['desktopPID']]
  f=folder/'runtime.cache.jsonl'
  if f.exists():
   rows=[json.loads(x) for x in f.read_text().splitlines() if x.strip()]
   if rows:r['cache']={'last':rows[-1],'max':{k:max(x[k] for x in rows) for k in rows[0] if isinstance(rows[0][k],(int,float))}}
  r['native_pixels']={}
  for name in ['selected-native.png','selected-after-native.png']:
   f=folder/name
   if not f.exists():continue
   im=Image.open(f).convert('RGB')
   if im.size!=(2200,1440) or r['identity']['dpi']!=192:raise RuntimeError('Native geometry differs from fixed DPI192 protocol')
   crop=im.crop((1600,260,2080,560))
   sha=hashlib.sha256(crop.tobytes()).hexdigest()
   r['native_pixels'][name]={'width':im.width,'height':im.height,'crop_pixels':[1600,260,2080,560],'crop_rgb_sha256':sha,'whole_capture_sha256':hashlib.sha256(f.read_bytes()).hexdigest()}
   if r['identity']['dataset']=='images':pixel_groups[r['architecture']+'/'+name].append(sha)
 result={'schema':1,'units':'bytes, seconds and counts; latency fields are milliseconds','runs':runs,'pixel_comparisons':{k:{'captures':len(v),'unique_crop_hashes':sorted(set(v)),'equal':len(set(v))==1} for k,v in pixel_groups.items()}}
 output.mkdir(parents=True,exist_ok=True)
 (output/'measurements.json').write_text(json.dumps(result,separators=(',',':'))+'\n')
 with (output/'measurements.csv').open('w',newline='') as f:
  w=csv.writer(f,lineterminator='\n');w.writerow(['run','variant','architecture','dataset','role','phase','samples','total_ws_bytes','private_ws_bytes','private_commit_bytes','handles','gdi','cpu_seconds_at_end'])
  for r in runs:
   for p in r['phases']:
    m=p['metrics'];w.writerow([r['run'],r['variant'],r['architecture'],r['identity']['dataset'],p['role'],p['phase'],p['sampleCount'],*[m[k]['median'] for k in ['workingSet','privateWorkingSet','privateBytes','handles','gdi']],p['cpuSecondsAtPhaseEnd']])
 print('SUMMARY accepted='+str(len(runs))+' native_pixel_groups_equal='+str(all(x['equal'] for x in result['pixel_comparisons'].values())),flush=True)
 return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args();summarize(a.input,a.output)
