#!/usr/bin/env python3
"""Summarize synthetic real-process runs. Never infer a native heap or GPU remainder."""
import argparse,csv,hashlib,json,statistics
from collections import defaultdict
from pathlib import Path

def read(path):
    raw=path.read_bytes()
    return json.loads(raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig'))
def q(values,f):
    a=sorted(values)
    return a[min(len(a)-1,int((len(a)-1)*f))]
def summarize(root):
    result=[]
    for folder in sorted(root.iterdir()):
        if not (folder/'identity.json').is_file() or not (folder/'samples.json').is_file():continue
        ident=read(folder/'identity.json')
        if not ident.get('hiddenVerified') or not ident.get('reopenVerified'):continue
        if not ident.get('diagnostic') and not ident.get('nativeCaptureReviewed'):continue
        samples=read(folder/'samples.json')
        groups=defaultdict(list)
        for r in samples:groups[(r['role'],r['phase'])].append(r)
        rows=[]
        for (role,phase),all_rows in groups.items():
            low=30 if phase=='visible' else 15 if phase in ('hidden','reopened') else 0
            selected=[r for r in all_rows if r['elapsedSeconds']>=low]
            if not selected:continue
            stats={}
            for key in ('workingSet','privateWorkingSet','sharedWorkingSet','privateBytes','virtualBytes','handles','threads','gdi','user'):
                a=[r[key] for r in selected]
                stats[key]={'min':min(a),'median':statistics.median(a),'p95':q(a,.95),'max':max(a)}
            first,last=selected[0],selected[-1]
            dt=last['elapsedSeconds']-first['elapsedSeconds']
            rows.append({'role':role,'phase':phase,'sampleCount':len(selected),'settledFromSeconds':low,'metrics':stats,'cpuSecondsAtPhaseEnd':last['cpuSeconds'],'cpuCoreFraction':(last['cpuSeconds']-first['cpuSeconds'])/dt if dt else None,'sampledStartupPeak':{k:max(r[k] for r in all_rows if r['elapsedSeconds']<=10) for k in ('workingSet','privateWorkingSet','privateBytes')} if phase=='visible' else None})
        runtime={}
        for f in sorted(folder.glob('runtime*.jsonl')):
            records=[]
            for line in f.read_text(encoding='utf-8-sig').splitlines():
                try:records.append(json.loads(line))
                except json.JSONDecodeError:pass
            target_pid=ident['desktopPID'] if 'unic-desktop.exe' in f.name else ident['servicePID'] if 'unicd.exe' in f.name else None
            if target_pid is not None and any('pid' in r for r in records):
                records=[r for r in records if r.get('pid')==target_pid]
            if records:runtime[f.name]={'count':len(records),'last':records[-1],'max':{k:max(r[k] for r in records if k in r) for k in records[-1] if isinstance(records[-1][k],(int,float)) and k not in ('pid','elapsedSeconds')}}
        result.append({'run':folder.name,'identity':ident,'phases':rows,'runtimeAggregates':runtime,'evidenceSHA256':{f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in folder.iterdir() if f.is_file()}})
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    results=summarize(a.input);a.output.mkdir(parents=True,exist_ok=True)
    (a.output/'measurements.json').write_text(json.dumps({'schema':1,'units':'bytes, seconds, counts','runs':results},indent=2)+'\n')
    with (a.output/'measurements.csv').open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['run','dataset','repetition','role','phase','samples','working_set_bytes','private_working_set_bytes','private_commit_bytes','gdi','user','handles','cpu_core_fraction'])
        for r in results:
            for phase in r['phases']:
                m=phase['metrics'];writer.writerow([r['run'],r['identity']['dataset'],r['identity']['repetition'],phase['role'],phase['phase'],phase['sampleCount'],*[m[k]['median'] for k in ('workingSet','privateWorkingSet','privateBytes','gdi','user','handles')],phase['cpuCoreFraction']])
    # A symptom detector, not an optimization acceptance threshold: metric/group unknown in the user report.
    for r in results:
        for metric in ('privateWorkingSet','workingSet','privateBytes'):
            peak=max(p['metrics'][metric]['max'] for p in r['phases'] if p['role']=='desktop')
            print(r['run'],metric,'>=200MB' if peak>=200_000_000 else '<200MB',round(peak/1e6,2))
