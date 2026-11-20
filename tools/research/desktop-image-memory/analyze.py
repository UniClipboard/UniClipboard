#!/usr/bin/env python3
"""Aggregate run medians without adding overlapping optimization deltas."""
import argparse,json,statistics
from collections import defaultdict
from datetime import datetime
from pathlib import Path

p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
x=json.loads(a.input.read_text());groups=defaultdict(list)
def stats(values):return {'n':len(values),'median':statistics.median(values),'min':min(values),'max':max(values)}
for r in x['runs']:
 groups[(r['architecture'],r['identity']['dataset'],r['variant'])].append(r)
rows=[]
for (arch,dataset,variant),runs in sorted(groups.items()):
 row={'architecture':arch,'dataset':dataset,'variant':variant,'run_count':len(runs),'phases':{}}
 phases=list(dict.fromkeys(p['phase'] for r in runs for p in r['group_phases']))
 for phase in phases:
  current={'group':{},'desktop':{},'service':{}}
  for role in current:
   for metric in ['privateBytes','privateWorkingSet','workingSet']:
    values=[]
    for r in runs:
     source=r['group_phases'] if role=='group' else [p for p in r['phases'] if p['role']==role]
     q=next((q for q in source if q['phase']==phase),None)
     if q:values.append(q['metrics'][metric]['median'])
    if values:current[role][metric]=stats(values)
  row['phases'][phase]=current
 cpu=[sum(p['cpuSecondsAtPhaseEnd'] for p in r['phases'] if p['phase']=='visible') for r in runs]
 row['group_cpu_seconds_at_visible_end']=stats(cpu)
 manual=[r['latency_events'][-1]['selectionMillis'] for r in runs if len(r['latency_events'])>=2]
 command=[r['identity']['selectionActionToObservedReadyMillis'] for r in runs if 'selectionActionToObservedReadyMillis' in r['identity']]
 startup=[r['latency_events'][0]['unixMillis']-datetime.fromisoformat(r['identity']['desktopStart'].replace('Z','+00:00')).timestamp()*1000 for r in runs if r['latency_events']]
 if manual:row['ui_observed_selection_ms']=stats(manual)
 if command:row['command_to_observed_selection_ms']=stats(command)
 if startup:row['native_process_start_to_first_original_ms']=stats(startup)
 row['full_visible_sampled_peak_private_commit']=stats([next(p for p in r['group_phases'] if p['phase']=='visible')['sampled_full_phase_peak']['privateBytes'] for r in runs])
 rows.append(row)
comparisons=[]
for arch in ['arm64','amd64']:
 for dataset in ['images','empty','text']:
  base=next((r for r in rows if (r['architecture'],r['dataset'],r['variant'])==(arch,dataset,'baseline')),None)
  if base is None:continue
  for variant in ['admission','streaming','combined']:
   optimized=next((r for r in rows if (r['architecture'],r['dataset'],r['variant'])==(arch,dataset,variant)),None)
   if optimized is None:continue
   c={'architecture':arch,'dataset':dataset,'variant':variant,'baseline_runs':base['run_count'],'variant_runs':optimized['run_count'],'phases':{}}
   for phase in base['phases']:
    if phase not in optimized['phases']:continue
    c['phases'][phase]={}
    for metric in ['privateBytes','privateWorkingSet']:
     b=base['phases'][phase]['group'][metric]['median'];v=optimized['phases'][phase]['group'][metric]['median']
     c['phases'][phase][metric]={'baseline':b,'variant':v,'reduction':b-v,'reduction_fraction':(b-v)/b}
   b=base['group_cpu_seconds_at_visible_end']['median'];v=optimized['group_cpu_seconds_at_visible_end']['median'];c['cpu_increase_fraction']=(v-b)/b
   if 'ui_observed_selection_ms' in optimized:c['selection_increase_ms']=optimized['ui_observed_selection_ms']['median']-base['ui_observed_selection_ms']['median']
   c['three_repetitions_complete']=base['run_count']==optimized['run_count']==3
   c['cpu_investigation_threshold_exceeded']=c['cpu_increase_fraction']>.20
   c['selection_investigation_threshold_exceeded']=c.get('selection_increase_ms',0)>100
   comparisons.append(c)
a.output.write_text(json.dumps({'schema':1,'units':'memory bytes, CPU seconds, latency milliseconds; positive reduction means less memory','rows':rows,'comparisons':comparisons,'native_pixel_comparisons':x['pixel_comparisons']},indent=2)+'\n')
for r in rows:
 if r['dataset']=='images':
  v=r['phases']['visible']['group'];h=r['phases']['hidden']['group'];print(r['architecture'],r['variant'],'n='+str(r['run_count']),'visible/privateWS/privateCommit/hiddenCommit MB',*[round(q/1e6,2) for q in [v['privateWorkingSet']['median'],v['privateBytes']['median'],h['privateBytes']['median']]],'cpu',round(r['group_cpu_seconds_at_visible_end']['median'],3))
