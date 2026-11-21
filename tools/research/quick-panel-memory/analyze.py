#!/usr/bin/env python3
"""Validate complete paired cohorts and aggregate run medians without mixing roles."""
import argparse,json,statistics,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('cohorts',nargs='+',type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True);runs=[]
for cohort in a.cohorts:
 journal=json.loads((cohort/'journal.json').read_text())
 if any(row['exit_code'] for row in journal):raise SystemExit('cohort has a failed run: '+str(cohort))
 for row in journal:
  directory=cohort/row['name'];target=a.output/(cohort.name+'-'+row['name']+'.json')
  subprocess.run([sys.executable,str(Path(__file__).with_name('summarize.py')),str(directory),'--output',str(target)],check=True,stdout=subprocess.DEVNULL)
  summary=json.loads(target.read_text());data=next(iter(summary['runs'].values()));receipt=data['receipt'];identities=receipt['identities']
  if receipt['verdict']!='PASS' or receipt['ownedProcessesRemaining'] or receipt['clipboardSequenceBefore']!=receipt['clipboardSequenceAfter']:raise SystemExit('run/cleanup invariant failed')
  if len({v['pid'] for v in identities.values()})!=4:raise SystemExit('four distinct role PIDs required')
  for role,identity in identities.items():
   expected='unicd.exe' if role=='service' else 'unic-desktop.exe'
   if identity['sha256'].lower()!=summary['build']['binary_sha256'][expected]:raise SystemExit('role executable identity mismatch')
   if role!='service':
    native=identity['nativeMachine'];process=identity['processMachine'];arch=summary['build']['arch']
    image=summary['build']['pe_machine'][expected]
    # x64-on-Arm has no WOW64 layer. Keep UNKNOWN from that API as raw
    # evidence; classify using the loaded image digest/PE and native host.
    if native!=0xaa64 or image!=('0xaa64' if arch=='arm64' else '0x8664') or process not in ((0,) if arch=='arm64' else (0,0x8664)):raise SystemExit('unexpected guest/runtime architecture')
  expected_roles={'gui','quick-panel','tray','service'}
  if set(data['phases']['panel-with-gui']['roles'])!=expected_roles:raise SystemExit('visible role set mismatch')
  if set(data['phases']['panel-gui-exited']['roles'])!=expected_roles-{'gui'}:raise SystemExit('GUI did not exit or PID was reused')
  variant='candidate' if row['name'].endswith('candidate') else 'control'
  runs.append({'arch':summary['build']['arch'],'dataset':receipt['dataset'],'variant':variant,'name':cohort.name+'/'+row['name'],'data':data,'summary':target.name,'build':summary['build']})
def stats(values):
 values=[v for v in values if v is not None]
 return {'n':len(values),'median':statistics.median(values) if values else None,'min':min(values) if values else None,'max':max(values) if values else None}
comparison={};crop_sets={}
for arch,dataset in sorted({(r['arch'],r['dataset']) for r in runs}):
 key=arch+'/'+dataset;comparison[key]={}
 for variant in ['control','candidate']:
  selected=[r for r in runs if r['arch']==arch and r['dataset']==dataset and r['variant']==variant];phases={}
  for phase in selected[0]['data']['phases']:
   groups={metric:stats([r['data']['phases'][phase]['groupMedian'][metric] for r in selected]) for metric in ['privateCommit','privateWorkingSet','workingSet']};roles={}
   for role in selected[0]['data']['phases'][phase]['roles']:
    roles[role]={metric:stats([r['data']['phases'][phase]['roles'][role]['median'][metric] for r in selected]) for metric in ['privateCommit','privateWorkingSet','workingSet','handles','gdi','user','threads']}
    for metric in ['heapAlloc','heapSys','heapReleased','sys','goroutines']:
     roles[role]['go/'+metric]=stats([r['data']['phases'][phase]['roles'][role].get('goMedian',{}).get(metric) for r in selected])
   phases[phase]={'group':groups,'roles':roles}
  timing={}
  for label in ['coldOriginal','warmOriginal','coldSearch','warmSearch','selection']:
   values=[]
   for run in selected:
    opens=[e for e in run['data']['receipt']['timing'] if e['event']=='panel-open'];selections=[e['intentToOriginalMillis'] for e in run['data']['receipt']['timing'] if e['event']=='panel-selection']
    if label=='selection':value=statistics.median(selections) if selections else None
    elif label=='coldOriginal':value=opens[0].get('originalReadyMillis')
    elif label=='warmOriginal':value=statistics.median([e['originalReadyMillis'] for e in opens[1:] if 'originalReadyMillis' in e]) if dataset=='images' else None
    elif label=='coldSearch':value=opens[0]['searchObservedMillis']
    else:value=statistics.median(e['searchObservedMillis'] for e in opens[1:])
    values.append(value)
   timing[label]=stats(values)
  cpu_phase='selection-final-hidden' if 'selection-final-hidden' in selected[0]['data']['phases'] else 'rehide-3'
  cpu=stats([r['data']['phases'][cpu_phase]['roles']['quick-panel']['cpuAtEnd'] for r in selected])
  peak=stats([r['data']['phases']['panel-with-gui']['roles']['quick-panel']['kernelPeakWorkingSetAtEnd'] for r in selected])
  comparison[key][variant]={'runs':[r['name'] for r in selected],'phases':phases,'timing':timing,'panelCpuThroughPhase':cpu_phase,'panelCpuSeconds':cpu,'panelColdKernelPeakWS':peak}
  for run in selected:
   if dataset=='images':
    for frame,crop in run['data']['satellite_crops'].items():crop_sets.setdefault(arch+'/'+frame,set()).add(crop['rgba_sha256'])
 # Bytes of CLI/service/helper must be identical within an architecture pair.
 for program in ['unic.exe','unicd.exe','helper.exe']:
  hashes={r['build']['binary_sha256'][program] for r in runs if r['arch']==arch}
  if len(hashes)!=1:raise SystemExit('non-Desktop programs differ within architecture')
if any(len(values)!=1 for values in crop_sets.values()):raise SystemExit('native image crop mismatch')
result={'units':'bytes; seconds; milliseconds; counts','aggregation':'median of run medians; ranges are run medians; group counters pair samples','comparison':comparison,'nativeCropEquivalence':{key:list(values)[0] for key,values in crop_sets.items()},'runSummaries':[r['summary'] for r in runs]}
(a.output/'comparison.json').write_text(json.dumps(result,indent=2)+'\n');print(a.output/'comparison.json')
