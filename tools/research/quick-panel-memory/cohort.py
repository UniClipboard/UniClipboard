#!/usr/bin/env python3
"""Alternate reviewed stages; stop at the first failed run, retaining evidence."""
import argparse,json,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--control-stage',required=True,type=Path);p.add_argument('--candidate-stage',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--vm',required=True);p.add_argument('--repetitions',type=int,default=3);p.add_argument('--selections',type=int,choices=range(4),default=0);p.add_argument('--datasets',nargs='+',default=['images'],choices=['empty','text','images']);a=p.parse_args()
if a.output.exists():raise SystemExit('fresh cohort output required')
a.output.mkdir(parents=True);journal=[]
for rep in range(1,a.repetitions+1):
 for dataset in a.datasets:
  for variant in (['control','candidate'] if rep%2 else ['candidate','control']):
   name=f'{dataset}-{rep}-{variant}';stage=getattr(a,variant+'_stage');print(name,flush=True)
   code=subprocess.call([sys.executable,str(Path(__file__).with_name('run.py')),'--stage',str(stage),'--output',str(a.output/name),'--vm',a.vm,'--datasets',dataset,'--selections',str(a.selections)])
   journal.append({'name':name,'exit_code':code});(a.output/'journal.json').write_text(json.dumps(journal,indent=2)+'\n')
   if code:raise SystemExit(code)
